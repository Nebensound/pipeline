//! `nebensound-repo unwritten`: what the register knows and nobody has written
//! a building block for yet, failed by name.
//!
//! A check, a build, a version writer or a publish target without a block
//! would otherwise report nothing and leave a pull request without a signal.
//! So the step `Not written yet` of a unit's job, and the steps
//! `Version writers not written yet` and `Publish targets not written yet` of
//! `Check`, fail for each one by name and say where its block is to be
//! written.
//!
//! What has a block is a list in the step itself (`WRITTEN_CHECKS`,
//! `WRITTEN_BUILDS`, `WRITTEN`), which `cargo xtask check` holds to the blocks
//! that exist; the step hands it in as it is. What the run asks for comes from
//! Declare, as today: the checks and the build of the unit's matrix entry, and
//! the plan for the version writers and the publish targets.

use serde_json::Value;

use crate::workflow::Log;

/// Reads a list of names, as a step hands in `WRITTEN_CHECKS` or the checks
/// of its matrix entry.
///
/// # Errors
///
/// Returns a sentence naming `what` when the text is no JSON list of names.
pub fn names(text: &str, what: &str) -> Result<Vec<String>, String> {
    serde_json::from_str::<Vec<String>>(text)
        .map_err(|error| format!("{what} is no JSON list of names: {error}"))
}

/// The checks of the unit at `unit` (`planned`) that have no block
/// (`written` names those that have): whether there is none.
pub fn checks(unit: &str, planned: &[String], written: &[String], log: &mut Log<'_>) -> bool {
    let missing: Vec<&String> = planned
        .iter()
        .filter(|check| !written.contains(check))
        .collect();
    if missing.is_empty() {
        log.line(&format!("Every check of '{unit}' has a building block."));
        return true;
    }
    for check in missing {
        log.error(&format!(
            "The check '{check}' has no building block yet, so '{unit}' is not checked for it. \
             It is to be written under .github/actions/check-{check}."
        ));
    }
    false
}

/// The build of the unit at `unit` (`planned`), held against the builds that
/// have a block: whether it has one.
pub fn build(unit: &str, planned: &str, written: &[String], log: &mut Log<'_>) -> bool {
    if written.iter().any(|build| build == planned) {
        log.line(&format!(
            "The build '{planned}' of '{unit}' has a building block."
        ));
        return true;
    }
    log.error(&format!(
        "The build '{planned}' has no building block yet, so '{unit}' is not built. It is to be \
         written under .github/actions/build-{planned}."
    ));
    false
}

/// What a plan names for a unit that carries a version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    /// Its version writers.
    Writers,
    /// Its publish targets.
    Targets,
}

impl Named {
    /// The key of a version entry of the plan that lists them.
    fn key(self) -> &'static str {
        match self {
            Self::Writers => "writers",
            Self::Targets => "targets",
        }
    }

    /// What one of them is called in a sentence.
    fn noun(self) -> &'static str {
        match self {
            Self::Writers => "version writer",
            Self::Targets => "publish target",
        }
    }
}

/// The version writers or publish targets the plan names for any unit that
/// have no block (`written` names those that have): whether there is none.
/// Without a plan, as when phase 0 failed, there is nothing to look at, and
/// that passes; the closing job fails for phase 0 already.
///
/// # Errors
///
/// Returns a sentence when the plan does not read.
pub fn named(
    plan: &str,
    kind: Named,
    written: &[String],
    log: &mut Log<'_>,
) -> Result<bool, String> {
    let noun = kind.noun();
    if plan.trim().is_empty() {
        log.line(&format!(
            "There is no plan, so there is no {noun} to look at."
        ));
        return Ok(true);
    }
    let plan: Value =
        serde_json::from_str(plan).map_err(|error| format!("the plan does not read: {error}"))?;
    let mut passed = true;
    for version in plan["versions"].as_array().into_iter().flatten() {
        let unit = match version["name"].as_str() {
            Some(name) if version["kind"] == "component" => format!("component {name}"),
            _ => "the repository".to_owned(),
        };
        for name in version[kind.key()]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|name| !written.iter().any(|written| written == name))
        {
            passed = false;
            log.error(&match kind {
                Named::Writers => format!(
                    "The version writer '{name}' has no building block yet, so the version of \
                     {unit} is not written. It is to be written under \
                     .github/actions/version-{name}."
                ),
                Named::Targets => format!(
                    "The publish target '{name}' has no building block yet, so {unit} is not \
                     published there. It is to be written under .github/actions/publish-{}.",
                    name.replace('.', "-")
                ),
            });
        }
    }
    if passed {
        log.line(&format!(
            "Every {noun} the plan names has a building block."
        ));
    }
    Ok(passed)
}
