//! `nebensound-repo gather`: the one result of the checks and the one result
//! of the builds, as the closing jobs `Check` and `Build` give them.
//!
//! A matrix gives one result per unit, and those names change with the
//! repository; a ruleset can only require a name that stays. So `Check` and
//! `Build` each turn the results of the jobs they wait for into one. They read
//! nothing of the repository: what they are handed is what GitHub says of
//! those jobs, and for the builds the list Declare handed the build matrix.
//!
//! The preparing job `Declare` decides which jobs start. When it does not
//! pass, whether for an error of phase 0 or for one in the versions of a pull
//! request, nothing starts, and both fail and name it as the place to look.

use clap::ValueEnum;

use crate::workflow::Log;

/// How a job the closing job waits for ended, as GitHub names it in
/// `needs.<job>.result`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Outcome {
    /// Every step passed.
    Success,
    /// A step failed.
    Failure,
    /// The run was cancelled.
    Cancelled,
    /// The job never started: its condition said no.
    Skipped,
}

impl Outcome {
    /// The name GitHub gives it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
        }
    }
}

/// The result of the checks, from how the preparing job (`declare`) and the
/// jobs of the units (`checks`) ended: whether it passes. A plan that starts no check
/// skips the jobs of the units, and that passes.
pub fn checks(declare: Outcome, checks: Outcome, log: &mut Log<'_>) -> bool {
    if declare != Outcome::Success {
        log.error(&format!(
            "The preparing job Declare did not pass ({}), so nothing was checked. Its \
             log says why.",
            declare.as_str()
        ));
        return false;
    }
    match checks {
        Outcome::Success => {
            log.line("Every check the plan started passed.");
            true
        }
        Outcome::Skipped => {
            log.line("The plan starts no check here; there was nothing to do.");
            true
        }
        other => {
            log.error(&format!(
                "At least one check did not pass ({}).",
                other.as_str()
            ));
            false
        }
    }
}

/// How the builds of a run went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Builds<'a> {
    /// How the preparing job ended.
    pub declare: Outcome,
    /// How the closing job of the checks ended.
    pub check: Outcome,
    /// How the jobs of the builds ended.
    pub builds: Outcome,
    /// The units Declare handed the build matrix, as JSON.
    pub planned: &'a str,
    /// Whether the run follows a push to a pre-branch.
    pub pre_branch: bool,
}

/// The result of the builds: whether it passes.
///
/// When nothing builds in this run, as after a push to a pre-branch, it
/// passes and says why, whatever the checks found. Otherwise it passes only
/// when the checks passed: a build that never started because a check failed
/// is no success.
pub fn builds(builds: &Builds<'_>, log: &mut Log<'_>) -> bool {
    if builds.declare != Outcome::Success {
        log.error(&format!(
            "The preparing job Declare did not pass ({}), so nothing was built. Its log \
             says why.",
            builds.declare.as_str()
        ));
        return false;
    }
    let nothing = serde_json::from_str::<Vec<serde_json::Value>>(builds.planned)
        .is_ok_and(|planned| planned.is_empty());
    if nothing {
        log.line(if builds.pre_branch {
            "Nothing builds in this run: a push to a pre-branch resolves no version."
        } else {
            "Nothing builds in this run: no unit that builds has a version of this run."
        });
        return true;
    }
    if builds.check != Outcome::Success {
        log.error(&format!(
            "Check did not pass ({}), so nothing was built.",
            builds.check.as_str()
        ));
        return false;
    }
    match builds.builds {
        Outcome::Success => {
            log.line("Every build the plan started passed.");
            true
        }
        Outcome::Skipped => {
            log.error("The plan names builds, and none of them ran.");
            false
        }
        other => {
            log.error(&format!(
                "At least one build did not pass ({}).",
                other.as_str()
            ));
            false
        }
    }
}
