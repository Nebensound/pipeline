//! The test cases of the building blocks, and running a block against one.
//!
//! A block's cases lie beside its `action.yml`: `tests/pass/<case>/` and
//! `tests/fail/<case>/`. Each case is a small repository of its own, with its
//! own `.nebensound/repo.toml`. To run one, the xtask copies it into a fresh
//! Git repository and runs phase 0 on it the way `Declare` does. A check block
//! then runs once for every unit the plan starts the block's check for, the
//! way a unit job does, and gets the plan as well when it takes one. A version
//! writer's block runs once with the plan, provided a unit of the plan selects
//! the writer. A build block runs once for every unit the plan starts the
//! build for and gives a version of the run, the way a build job does: after
//! the blocks of the version writers the unit selects, each with the plan, and
//! only when they passed; it gets the plan as well when it takes one. A
//! passing case passes when the block succeeds everywhere. A failing case
//! passes when the block fails and every line of its [`FINDING`] file matches
//! a line the block printed.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::action::Action;
use crate::runner::{Host, Job, Place};
use crate::{Error, say};

/// The file in a failing case that names the findings the block must report.
/// It is not copied into the case's repository.
///
/// Each line that is not empty and does not start with `#` must match a whole
/// line the block printed, where `*` stands for any text.
pub const FINDING: &str = "expected-finding";

/// Where the building blocks lie, from the repository root.
pub const ACTIONS: &str = ".github/actions";

/// Entries under `tests/pass/` and `tests/fail/` that are no case and are
/// passed over: files a desktop leaves behind and Git ignores, so that the
/// gate never sees them. Counting them would fail the layout locally where
/// the gate passes.
pub const LEFT_BEHIND: &[&str] = &[".DS_Store"];

/// The steps of a block the xtask does not run, each with why: the block, the
/// step's name and the reason. Each of them has to be a step of its block.
pub const SKIPPED: &[(&str, &str, &str)] = &[(
    "check-rust",
    "Make room on the runner",
    "it deletes software of the machine it runs on with sudo; on a developer's machine that \
     software is theirs",
)];

/// What a case expects of its block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expect {
    /// The block succeeds.
    Pass,
    /// The block fails, and prints a line matching each of these patterns.
    Fail(Vec<String>),
}

/// One case.
#[derive(Clone, Debug)]
pub struct Case {
    /// The name of its directory.
    pub name: String,
    /// Its directory.
    pub dir: PathBuf,
    /// What it expects.
    pub expect: Expect,
}

impl Case {
    /// How the case is named in a message.
    #[must_use]
    pub fn label(&self) -> String {
        match self.expect {
            Expect::Pass => format!("passing case `{}`", self.name),
            Expect::Fail(_) => format!("failing case `{}`", self.name),
        }
    }
}

/// The cases of the block in `block`: the passing ones first, each kind in the
/// order of its names. A block without a `tests/` folder has none. What
/// [`LEFT_BEHIND`] names is passed over.
///
/// # Errors
///
/// When an entry under `tests/pass/` or `tests/fail/` is no case: not a
/// directory, a failing case without its findings, a passing case with some.
pub fn discover(block: &Path) -> Result<Vec<Case>, Error> {
    let mut cases = Vec::new();
    for (kind, failing) in [("pass", false), ("fail", true)] {
        let dir = block.join("tests").join(kind);
        if !dir.exists() {
            continue;
        }
        let entries = fs::read_dir(&dir)
            .map_err(|error| Error::new(format!("cannot list {}: {error}", dir.display())))?;
        let mut paths: Vec<PathBuf> = entries
            .map_while(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                !path
                    .file_name()
                    .is_some_and(|name| LEFT_BEHIND.iter().any(|left| name == *left))
            })
            .collect();
        paths.sort();
        for path in paths {
            cases.push(case(path, failing)?);
        }
    }
    Ok(cases)
}

fn case(dir: PathBuf, failing: bool) -> Result<Case, Error> {
    let name = dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if !dir.is_dir() {
        return Err(Error::new(format!(
            "{} is no case: a case is a directory",
            dir.display()
        )));
    }
    let finding = fs::read_to_string(dir.join(FINDING)).ok();
    let expect = match (failing, finding) {
        (false, None) => Expect::Pass,
        (false, Some(_)) => {
            return Err(Error::new(format!(
                "the passing case {} holds {FINDING}; only a failing case names a finding",
                dir.display()
            )));
        }
        (true, finding) => {
            let patterns: Vec<String> = finding
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_owned)
                .collect();
            if patterns.is_empty() {
                return Err(Error::new(format!(
                    "the failing case {} names no finding: its {FINDING} has to name at least \
                     one",
                    dir.display()
                )));
            }
            Expect::Fail(patterns)
        }
    };
    Ok(Case { name, dir, expect })
}

/// Whether a line matches a pattern of a [`FINDING`] file: the whole line,
/// with `*` standing for any text.
#[must_use]
pub fn matches(pattern: &str, line: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = line.strip_prefix(first) else {
        return false;
    };
    let others: Vec<&str> = parts.collect();
    let Some((last, middle)) = others.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        let Some(index) = rest.find(part) else {
            return false;
        };
        rest = &rest[index + part.len()..];
    }
    rest.ends_with(last)
}

/// Judges a run against what the case expects: nothing when it went as
/// expected, else what went wrong.
#[must_use]
pub fn judge(expect: &Expect, success: bool, log: &[String]) -> Option<String> {
    match expect {
        Expect::Pass if success => None,
        Expect::Pass => Some("the block fails on a case that is meant to pass".to_owned()),
        Expect::Fail(_) if success => Some(
            "the block passes a case that is meant to fail, so it lets the mistake through"
                .to_owned(),
        ),
        Expect::Fail(patterns) => {
            let missing: Vec<&str> = patterns
                .iter()
                .filter(|pattern| !log.iter().any(|line| matches(pattern, line)))
                .map(String::as_str)
                .collect();
            if missing.is_empty() {
                None
            } else {
                Some(format!(
                    "the block fails, but without the finding the case names: no line matches {}",
                    missing
                        .iter()
                        .map(|pattern| format!("`{pattern}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            }
        }
    }
}

/// The paths of the units a plan of phase 0 starts `check` for.
///
/// # Errors
///
/// When the plan is no plan.
pub fn units(plan: &str, check: &str) -> Result<Vec<String>, Error> {
    let plan: serde_json::Value = serde_json::from_str(plan)
        .map_err(|error| Error::new(format!("the plan of phase 0 is no JSON: {error}")))?;
    let Some(units) = plan["units"].as_array() else {
        return Err(Error::new("the plan of phase 0 names no units".to_owned()));
    };
    Ok(units
        .iter()
        .filter(|unit| {
            unit["checks"]
                .as_array()
                .is_some_and(|checks| checks.iter().any(|name| name.as_str() == Some(check)))
        })
        .filter_map(|unit| unit["path"].as_str().map(str::to_owned))
        .collect())
}

/// The paths of the units of a plan of phase 0 that select the version
/// writer `writer`.
///
/// # Errors
///
/// When the plan is no plan.
pub fn writers(plan: &str, writer: &str) -> Result<Vec<String>, Error> {
    let plan: serde_json::Value = serde_json::from_str(plan)
        .map_err(|error| Error::new(format!("the plan of phase 0 is no JSON: {error}")))?;
    let Some(units) = plan["versions"].as_array() else {
        return Err(Error::new(
            "the plan of phase 0 names no versions".to_owned(),
        ));
    };
    Ok(units
        .iter()
        .filter(|unit| {
            unit["writers"]
                .as_array()
                .is_some_and(|writers| writers.iter().any(|name| name.as_str() == Some(writer)))
        })
        .filter_map(|unit| unit["path"].as_str().map(str::to_owned))
        .collect())
}

/// One unit a plan of phase 0 starts a build for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Built {
    /// Where it lives.
    pub path: String,
    /// The version writers it selects, which write its version before it
    /// builds.
    pub writers: Vec<String>,
}

/// The units a plan of phase 0 starts the build `build` for, each with the
/// version writers it selects, the way `Declare` names them: a unit whose
/// build is `build` and whose entry under `versions`, the one of the same
/// kind and name, gives it a version of this run. A unit the commit does not
/// touch has the version `null` there and builds nothing, also where a
/// version writer gives it its current version for a dependent.
///
/// # Errors
///
/// When the plan is no plan.
pub fn builds(plan: &str, build: &str) -> Result<Vec<Built>, Error> {
    let plan: serde_json::Value = serde_json::from_str(plan)
        .map_err(|error| Error::new(format!("the plan of phase 0 is no JSON: {error}")))?;
    let Some(units) = plan["units"].as_array() else {
        return Err(Error::new("the plan of phase 0 names no units".to_owned()));
    };
    let versions = plan["versions"].as_array().cloned().unwrap_or_default();
    Ok(units
        .iter()
        .filter(|unit| unit["build"].as_str() == Some(build))
        .filter_map(|unit| {
            let path = unit["path"].as_str()?.to_owned();
            let version = versions.iter().find(|version| {
                version["kind"] == unit["kind"] && version["name"] == unit["name"]
            })?;
            version["version"].as_str()?;
            let writers = version["writers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|writer| writer.as_str().map(str::to_owned))
                .collect();
            Some(Built { path, writers })
        })
        .collect())
}

/// What a block is, by the category in front of its name.
enum Kind<'a> {
    /// `check-<name>`.
    Check(&'a str),
    /// `version-<writer>`.
    Version(&'a str),
    /// `build-<name>`.
    Build(&'a str),
}

/// One run of a block: the version writers that run before it, each with the
/// plan, and the inputs it gets.
struct Call {
    writers: Vec<String>,
    inputs: BTreeMap<String, String>,
}

/// Where one case runs.
#[derive(Clone, Copy, Debug)]
pub struct Bench<'a> {
    /// The machine.
    pub host: &'a Host,
    /// What the job holds before the case: the tool on the PATH.
    pub job: &'a Job,
    /// The root of this repository, where the blocks lie.
    pub root: &'a Path,
    /// Where each case gets a directory of its own.
    pub scratch: &'a Path,
}

/// Runs the block `block` against one case: nothing when it went as expected,
/// else what went wrong. Everything the run prints is folded into a group.
///
/// # Errors
///
/// When the case cannot be run at all.
pub fn run(
    bench: Bench<'_>,
    block: &str,
    case: &Case,
    out: &mut dyn Write,
) -> Result<Option<String>, Error> {
    say(out, &format!("::group::{block}, {}\n", case.label()));
    let verdict = attempt(bench, block, case, out);
    say(out, "::endgroup::\n");
    verdict
}

fn attempt(
    bench: Bench<'_>,
    block: &str,
    case: &Case,
    out: &mut dyn Write,
) -> Result<Option<String>, Error> {
    let kind = if let Some(check) = block.strip_prefix("check-") {
        Kind::Check(check)
    } else if let Some(writer) = block.strip_prefix("version-") {
        Kind::Version(writer)
    } else if let Some(build) = block.strip_prefix("build-") {
        Kind::Build(build)
    } else {
        return Err(Error::new(format!(
            "`{block}` is no check, version writer or build, and the xtask runs the cases of \
             those only"
        )));
    };
    let temp = tempfile::tempdir_in(bench.scratch).map_err(|error| {
        Error::new(format!(
            "cannot make a directory for the case in {}: {error}",
            bench.scratch.display()
        ))
    })?;
    let workspace = temp.path().join("workspace");
    let runner = temp.path().join("runner");
    prepare(bench.host, &case.dir, &workspace, &runner)?;
    let place = Place {
        workspace: &workspace,
        temp: &runner,
    };
    let mut job = bench.job.clone();

    let phase0 = bench.host.run(
        &mut job,
        &bench.root.join(ACTIONS).join("phase0"),
        &BTreeMap::new(),
        place,
        &[],
        out,
    )?;
    if !phase0.success {
        return Ok(Some(
            "phase 0 fails on the case, so the block never runs; a case has to pass phase 0"
                .to_owned(),
        ));
    }
    let plan = phase0.outputs.get("plan").cloned().unwrap_or_default();
    let dir = bench.root.join(ACTIONS).join(block);
    let calls = match calls(&kind, &plan, &dir)? {
        Ok(calls) => calls,
        Err(never) => return Ok(Some(never)),
    };

    let skip: Vec<&str> = SKIPPED
        .iter()
        .filter(|(name, _, _)| *name == block)
        .map(|(_, step, _)| *step)
        .collect();
    let mut success = true;
    let mut log = Vec::new();
    let mut runs = |dir: &Path, inputs: &BTreeMap<String, String>, skip: &[&str]| {
        let outcome = bench.host.run(&mut job, dir, inputs, place, skip, out)?;
        log.extend(outcome.log);
        Ok::<bool, Error>(outcome.success)
    };
    for call in calls {
        // The writers first, each with the plan; the block only when every
        // one of them passed, as a build step after a step that failed is
        // skipped.
        let mut written = true;
        for writer in &call.writers {
            let writer = bench.root.join(ACTIONS).join(format!("version-{writer}"));
            let inputs = BTreeMap::from([("plan".to_owned(), plan.clone())]);
            written &= runs(&writer, &inputs, &[])?;
        }
        success &= written && runs(&dir, &call.inputs, &skip)?;
    }
    Ok(judge(&case.expect, success, &log))
}

/// The runs of a block of `kind` in `dir` the plan of phase 0 asks for, or
/// why the block never runs.
fn calls(kind: &Kind<'_>, plan: &str, dir: &Path) -> Result<Result<Vec<Call>, String>, Error> {
    let alone = |inputs| Call {
        writers: Vec::new(),
        inputs,
    };
    // A check or build block gets the plan beside the unit's path when it
    // takes one, as its step in nebensound.yml hands it over.
    let planned = match kind {
        Kind::Version(_) => false,
        Kind::Check(_) | Kind::Build(_) => Action::load(dir)?
            .inputs
            .iter()
            .any(|input| input.name.eq_ignore_ascii_case("plan")),
    };
    let unit = |path: String| {
        let mut inputs = BTreeMap::from([("working-directory".to_owned(), path)]);
        if planned {
            inputs.insert("plan".to_owned(), plan.to_owned());
        }
        inputs
    };
    Ok(Ok(match *kind {
        Kind::Check(check) => {
            let units = units(plan, check)?;
            if units.is_empty() {
                return Ok(Err(format!(
                    "no unit of the case starts the check `{check}`, so the block never runs"
                )));
            }
            units.into_iter().map(|path| alone(unit(path))).collect()
        }
        Kind::Version(writer) => {
            if writers(plan, writer)?.is_empty() {
                return Ok(Err(format!(
                    "no unit of the case selects the version writer `{writer}`, so the block \
                     never runs"
                )));
            }
            vec![alone(BTreeMap::from([(
                "plan".to_owned(),
                plan.to_owned(),
            )]))]
        }
        Kind::Build(build) => {
            let units = builds(plan, build)?;
            if units.is_empty() {
                return Ok(Err(format!(
                    "no unit of the case starts the build `{build}` with a version of the run, so \
                     the block never runs"
                )));
            }
            units
                .into_iter()
                .map(|built| Call {
                    writers: built.writers,
                    inputs: unit(built.path),
                })
                .collect()
        }
    }))
}

/// Copies a case into a fresh Git repository, with everything but its
/// [`FINDING`] committed, and makes the directory its steps write to.
fn prepare(host: &Host, case: &Path, workspace: &Path, runner: &Path) -> Result<(), Error> {
    let os = |text: &str| OsString::from(text);
    let git = |args: &[&str]| {
        let mut all = vec![os("-C"), workspace.into()];
        all.extend(args.iter().map(|arg| os(arg)));
        (host.git.clone(), all)
    };
    let steps = [
        (os("mkdir"), vec![os("-p"), workspace.into(), runner.into()]),
        (
            os("cp"),
            vec![os("-R"), case.join(".").into(), workspace.into()],
        ),
        git(&["-c", "init.defaultBranch=main", "init", "--quiet"]),
        git(&["add", "--all", "--", ".", &format!(":(exclude){FINDING}")]),
        git(&[
            "-c",
            "user.name=xtask",
            "-c",
            "user.email=xtask@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--no-verify",
            "--allow-empty",
            "--message",
            "The case",
        ]),
    ];
    steps
        .iter()
        .try_for_each(|(program, args)| crate::quietly(program, args))
}
