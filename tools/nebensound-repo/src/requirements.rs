//! What a release needs has a release already, or the pull request fails.
//!
//! The release stage on `main` tags and releases only the units a commit
//! touches. Every commit that touches a unit raises its version, so in steady
//! state the current version of a unit a commit does not touch was released
//! when it arose. The gap lies in versions from before the release stage ran
//! in a repository, and in a failed run nobody repaired. It does no harm to a
//! tag or a GitHub Release, which need no record of what a package requires.
//! It does harm on a registry, where packages require each other by version
//! (a target with `dependencies-by-version` in the register): a package
//! published there would require a version the registry does not hold.
//!
//! So on a pull request, every unit that gets a version in the run and
//! publishes to such a target is followed through what it requires by path,
//! as the version writer `cargo` follows it ([`writer::requirements`]). Every
//! unit reached that the pull request does not touch must have a record of its
//! current version, a tag `<prefix><version>`; otherwise the error
//! `unreleased-requirement` names the package, the unit, the version and the
//! way out. The test runs where the versions of a pull request are read, in
//! the job `Version`, and not on `main`, where it would come after the merge.

use crate::finding::{Finding, Rule};
use crate::history::DECLARATION;
use crate::names::TargetName;
use crate::plan::plan;
use crate::repository::Repository;
use crate::writer::{self, Order, WriteError};
use crate::{Analysis, Central};

/// The findings of a pull request with this number on a repository phase 0
/// analysed: one `unreleased-requirement` per package that would be
/// published to a registry and per unit it requires without a record.
///
/// # Errors
///
/// Returns [`WriteError`] when a manifest the writer would read cannot be read,
/// is not TOML, or holds no package where a unit needs one.
pub fn findings(
    repository: &Repository,
    analysis: &Analysis,
    central: &Central,
    pull_request: u64,
) -> Result<Vec<Finding>, WriteError> {
    let mut plan = plan(analysis, central);
    plan.pull_request = Some(pull_request);
    let orders = Order::of(&plan);
    let mut found = Vec::new();
    for (index, versioned) in plan.versions.iter().enumerate() {
        let registries: Vec<&TargetName> = versioned
            .targets
            .iter()
            .filter(|target| {
                central
                    .register()
                    .target
                    .get(*target)
                    .is_some_and(|target| target.dependencies_by_version)
            })
            .collect();
        if registries.is_empty() {
            continue;
        }
        let Some(required) = writer::requirements(repository, &orders, index)? else {
            continue;
        };
        for (reached, version) in &required.untouched {
            let unit = &plan.versions[*reached].unit;
            if unit.recorded {
                continue;
            }
            found.push(
                Finding::error(
                    Rule::UnreleasedRequirement,
                    format!(
                        "the package {} goes to {} with this pull request, where packages \
                         require each other by version, and it requires {} at {version}, its \
                         current version, by path. No tag {}{version} records that version, so \
                         the package would require a version that was never released. Touch {} \
                         in this pull request, with a change below {} or a change of {DECLARATION}, \
                         so that it gets a new version that is released with the merge",
                        required.package,
                        listed(&registries),
                        unit.name,
                        unit.prefix,
                        unit.name,
                        unit.path,
                    ),
                )
                .about(versioned.unit.name.clone()),
            );
        }
    }
    Ok(found)
}

/// Targets as a sentence names them: `a`, or `a and b`.
fn listed(targets: &[&TargetName]) -> String {
    let names: Vec<&str> = targets.iter().map(|target| target.as_str()).collect();
    names.join(" and ")
}
