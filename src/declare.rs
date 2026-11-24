//! `nebensound-repo declare`: phase 0 as the job `Declare` runs it, and what it
//! hands the jobs after it.
//!
//! In this order:
//!
//! 1. **Phase 0** on the repository, as `check` runs it, with the name, the
//!    default branch and the visibility the event of the run names
//!    ([`Run`]). The visibility decides whether the declared audience fits: an
//!    event that does not carry it is answered by the API, which the token of
//!    the run may always read the repository's metadata from. When phase 0
//!    finds errors, nothing else follows and nothing is handed on: then
//!    nothing is to be started.
//! 2. **The plan** (`plan`), on a pull request for its squash merge, as the
//!    job `Version` reads it: every version in it is the one that merge would
//!    get, with `-pr.<number>` behind it, so that the version writers and the
//!    package test of `check-rust` prove what a merge would publish.
//! 3. **A change of the audience**, on a pull request: the old and the new
//!    value as a notice and in the summary of the run, since the value decides
//!    which rules hold for the repository, and a reviewer should not have to
//!    find it in the diff. Before the excluded packages: GitHub keeps only the
//!    first annotations of a job, and a long list of them would hide this one.
//! 4. **The excluded packages**: every Rust package `[[exclude]]` takes out as
//!    a whole, with its reason, as a notice, since the declaration gives the
//!    reason. `check-rust` leaves such a package out of every step.
//! 5. **The matrices**: the units that run a check (`units`), and the builds
//!    of this run (`builds`), each with the version writers its unit selects,
//!    the name of its job and the runner it runs on. A matrix cannot leave
//!    out an entry, and a unit that runs no check would get a job with nothing
//!    in it. A unit that builds carries a version, and it builds only when the
//!    plan gives it a version of this run: a unit the commit does not touch
//!    has no new artifact to hand on. A build whose register entry lists
//!    targets builds once per target, each on the runner the register names
//!    beside it, with the target in the name of its job; any other build
//!    builds once, on [`RUNNER`]. After a push to a pre-branch no unit builds:
//!    the run resolves no version of its own.
//!
//! The plan and the two matrices are the outputs of the step, one line of
//! JSON each.

use serde_json::{Value, json};

use crate::Register;
use crate::audience::Visibility;
use crate::packages::{self, Excluded};
use crate::plan::Plan;
use crate::release::GitHub;
use crate::workflow::{self, Log, Variables, given};

/// What the event of the run says of the repository, and whether the run
/// follows a push to a pre-branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The repository's name, as `NEBENSOUND_NAME` gives it.
    pub name: Option<String>,
    /// Its default branch, as `NEBENSOUND_DEFAULT_BRANCH` gives it.
    pub default_branch: Option<String>,
    /// How GitHub shows it.
    pub visibility: Visibility,
    /// Whether the run follows a push to a pre-branch.
    pub pre_branch: bool,
}

impl Run {
    /// Reads the run from its variables, asking the API for the visibility
    /// when `NEBENSOUND_VISIBILITY` is empty.
    ///
    /// # Errors
    ///
    /// Returns a sentence when the visibility is none GitHub gives, or the
    /// API cannot be asked for it.
    pub fn read(variables: Variables<'_>) -> Result<Self, String> {
        let visibility = match given(variables, "NEBENSOUND_VISIBILITY") {
            Some(visibility) => visibility,
            None => api(variables)?.visibility()?,
        };
        let visibility = match visibility.as_str() {
            "public" => Visibility::Public,
            "private" => Visibility::Private,
            "internal" => Visibility::Internal,
            other => {
                return Err(format!(
                    "the visibility of the repository is {other:?}, and GitHub gives public, \
                     private or internal"
                ));
            }
        };
        Ok(Self {
            name: given(variables, "NEBENSOUND_NAME"),
            default_branch: given(variables, "NEBENSOUND_DEFAULT_BRANCH"),
            visibility,
            pre_branch: workflow::pre_branch(variables),
        })
    }
}

/// How to reach the repository on the API with the token of the run.
fn api(variables: Variables<'_>) -> Result<GitHub, String> {
    let required = |name: &str| {
        given(variables, name).ok_or_else(|| {
            format!(
                "the event names no visibility, and asking GitHub for it needs {name}, which is \
                 not set"
            )
        })
    };
    Ok(GitHub {
        api: required("GITHUB_API_URL")?.trim_end_matches('/').to_owned(),
        repository: required("GITHUB_REPOSITORY")?,
        token: required("GITHUB_TOKEN")?,
        search_path: variables("PATH").unwrap_or_default().into(),
        protocols: "=https".to_owned(),
    })
}

/// The sentence that names a change of the audience on a pull request, if
/// the plan carries one.
#[must_use]
pub fn audience_change(plan: &Plan) -> Option<String> {
    let audience = plan.audience?;
    (plan.previous_audience != Some(audience)).then(|| {
        format!(
            "This pull request changes the audience of the repository from {} to {audience}.",
            plan.previous_audience
                .map_or("none declared", |previous| previous.as_str())
        )
    })
}

/// The matrix of the units that run a check: the units of the plan, as it
/// prints them, that start one at least.
#[must_use]
pub fn units(plan: &Value) -> Value {
    plan["units"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|unit| {
            unit["checks"]
                .as_array()
                .is_some_and(|checks| !checks.is_empty())
        })
        .cloned()
        .collect()
}

/// The runner of a build whose register entry lists no targets: the one
/// every other job of the workflow runs on.
pub const RUNNER: &str = "ubuntu-latest";

/// The matrix of the builds of this run: the units of the plan, as it prints
/// them, that start a build and have a version of this run, each with the
/// version writers it selects, the name of its job (`job`) and the runner it
/// runs on (`runs-on`). A unit whose build lists targets in the register
/// stands once per target, with the target (`target`) and its runner, and the
/// target in the name of its job. None after a push to a pre-branch.
#[must_use]
pub fn builds(plan: &Value, pre_branch: bool, register: &Register) -> Value {
    if pre_branch {
        return json!([]);
    }
    let versions = plan["versions"].as_array().cloned().unwrap_or_default();
    plan["units"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|unit| !unit["build"].is_null())
        .filter_map(|unit| {
            let version = versions.iter().find(|version| {
                version["kind"] == unit["kind"] && version["name"] == unit["name"]
            })?;
            if version["version"].is_null() {
                return None;
            }
            let mut unit = unit.clone();
            unit["writers"] = version["writers"].clone();
            Some(unit)
        })
        .flat_map(|unit| {
            let job = match unit["name"].as_str() {
                Some(name) => format!("Build component {name}"),
                None => "Build repository".to_owned(),
            };
            let targets = register
                .build
                .iter()
                .find(|(name, _)| unit["build"].as_str() == Some(name.as_str()))
                .map(|(_, build)| build.targets.clone())
                .unwrap_or_default();
            if targets.is_empty() {
                let mut unit = unit;
                unit["job"] = json!(job);
                unit["runs-on"] = json!(RUNNER);
                return vec![unit];
            }
            targets
                .into_iter()
                .map(|(target, runner)| {
                    let mut entry = unit.clone();
                    entry["job"] = json!(format!("{job} ({target})"));
                    entry["runs-on"] = json!(runner);
                    entry["target"] = json!(target);
                    entry
                })
                .collect()
        })
        .collect()
}

/// Hands the plan and the matrices on, and names what a reader of the run
/// should see: whether everything could be handed on.
pub fn hand_on(
    plan: &Plan,
    register: &Register,
    run: &Run,
    excluded: &[Excluded],
    variables: Variables<'_>,
    log: &mut Log<'_>,
) -> bool {
    let json = plan.to_json();
    log.command("::group::The plan");
    log.line(&json);
    log.command("::endgroup::");
    if plan.pull_request.is_some()
        && let Some(line) = audience_change(plan)
    {
        log.command(&workflow::notice("Audience changed", &line));
        if let Err(message) = workflow::summary(variables, &format!("{line}\n")) {
            log.error(&message);
            return false;
        }
    }
    log.command("::group::The excluded packages");
    log.line(&packages::to_json(excluded));
    log.command("::endgroup::");
    for package in excluded {
        log.command(&workflow::notice(
            "Excluded package",
            &format!(
                "The Rust package at {} is excluded from every step of check-rust: {}",
                package.folder,
                package.reason.as_str()
            ),
        ));
    }
    // The plan is JSON the tool wrote, so it reads.
    let value: Value = serde_json::from_str(&json).unwrap_or_default();
    let units = units(&value);
    let builds = builds(&value, run.pre_branch, register);
    log.command("::group::The units that run a check");
    log.lines(&format!("{units:#}"));
    log.command("::endgroup::");
    log.command("::group::The units that build");
    if run.pre_branch {
        log.line("A push to a pre-branch resolves no version, so no unit builds.");
    }
    log.lines(&format!("{builds:#}"));
    log.command("::endgroup::");
    match workflow::output(
        variables,
        &[
            ("plan", &json),
            ("units", &units.to_string()),
            ("builds", &builds.to_string()),
        ],
    ) {
        Ok(()) => true,
        Err(message) => {
            log.error(&message);
            false
        }
    }
}
