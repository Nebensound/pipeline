//! The files one check covers in one unit, as a building block takes them.
//!
//! A building block checks exactly these files and reads no ignore mechanism
//! of its own tools, so `.nebensound/repo.toml` is the only place where a
//! repository takes a file out of a check: what an `[[exclude]]` entry takes
//! out, with its reason, is not named here.
//!
//! The answer is asked for a check and a unit **as the plan starts them**.
//! A check runs once, at the outermost unit it is in force for, and covers
//! every file below that unit; asking for a pair the plan does not start is
//! refused rather than answered with a list nobody checks.

use std::fmt;

use crate::annotate::Coverage;
use crate::path::RelativePath;
use crate::{Analysis, Central};

use crate::plan::plan;
use crate::report::shown;

/// Why there is no list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The register knows no check of this name.
    UnknownCheck(String),
    /// The plan does not start this check at this path.
    NotStarted {
        /// The check that was asked for.
        check: String,
        /// The path that was asked for.
        unit: RelativePath,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCheck(check) => {
                write!(f, "the register has no check {:?}", shown(check))
            }
            Self::NotStarted { check, unit } => write!(
                f,
                "the plan does not start the {check} check at {}; `plan` says where it runs",
                shown(&unit.to_string())
            ),
        }
    }
}

/// The files the check covers in the unit at this path, in path order.
///
/// # Errors
///
/// Returns [`Refusal`] when the register knows no such check, or when the
/// plan does not start it at this path.
pub fn files<'a>(
    analysis: &'a Analysis,
    central: &Central,
    check: &str,
    unit: &RelativePath,
) -> Result<Vec<&'a RelativePath>, Refusal> {
    let Some(check) = central
        .register()
        .check
        .keys()
        .find(|known| known.as_str() == check)
    else {
        return Err(Refusal::UnknownCheck(check.to_owned()));
    };
    let started = plan(analysis, central)
        .units
        .iter()
        .any(|planned| planned.path == *unit && planned.checks.contains(check));
    if !started {
        return Err(Refusal::NotStarted {
            check: check.to_string(),
            unit: unit.clone(),
        });
    }
    Ok(analysis
        .tree
        .files()
        .into_iter()
        .filter(|(node, coverage)| {
            unit.contains(&node.path) && **coverage == Coverage::Covered(check.clone())
        })
        .map(|(node, _)| &node.path)
        .collect())
}

/// The list as it is printed: one path per line, escaped like every other
/// path the tool prints, or with `zero` each path as it stands, ended by a
/// zero byte, for a program to read.
#[must_use]
pub fn render(files: &[&RelativePath], zero: bool) -> String {
    files
        .iter()
        .map(|file| {
            if zero {
                format!("{file}\0")
            } else {
                format!("{}\n", shown(&file.to_string()))
            }
        })
        .collect()
}
