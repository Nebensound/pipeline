//! Coverage: every Python file of the list is measured, the tests and
//! scripts as well, and each has to have every line and every branch covered.
//!
//! A file coverage.py never saw counts as not covered at all: no test imports
//! it, or the repository's `omit` leaves it out, which counts for nothing here
//! and is held against the declaration by phase 0. A file without a statement
//! has nothing to cover. Lines the repository marks with its coverage.py
//! exclusions (`# pragma: no cover`, `exclude_also`) are its choice, like
//! `#[coverage(off)]` in Rust. A threshold the repository sets for coverage.py
//! decides nothing here.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{EMPTY, Interpreter, Paths, Programs, command, has_extension, prefixed, stream, uv};
use crate::run::{Session, relative};

/// coverage.py's JSON report, as far as the check reads it.
#[derive(Debug, Clone, Deserialize)]
pub struct Report {
    /// Every file it measured, by the path it names it with.
    pub files: BTreeMap<String, Measured>,
}

/// One file coverage.py measured.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Measured {
    /// The lines no test ran.
    #[serde(default)]
    pub missing_lines: Vec<u64>,
    /// The branches no test took: from a line to a line, or to the exit of
    /// the code object where the second is not above 0.
    #[serde(default)]
    pub missing_branches: Vec<(i64, i64)>,
    /// Its numbers.
    #[serde(default)]
    pub summary: Summary,
}

/// The numbers of a file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Summary {
    /// How much of it is covered, as coverage.py shows it.
    #[serde(default)]
    pub percent_covered_display: Option<String>,
}

/// Numbers as spans: `1-3, 7` for 1, 2, 3 and 7.
#[must_use]
pub fn ranges(numbers: &[u64]) -> String {
    let mut sorted = numbers.to_vec();
    sorted.sort_unstable();
    let mut spans: Vec<(u64, u64)> = Vec::new();
    for number in sorted {
        match spans.last_mut() {
            Some(span) if number == span.1 + 1 => span.1 = number,
            _ => spans.push((number, number)),
        }
    }
    spans
        .iter()
        .map(|&(first, last)| {
            if first == last {
                first.to_string()
            } else {
                format!("{first}-{last}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// What is said about a file that is not fully covered, and the first line
/// that is not; [`None`] for one that is.
#[must_use]
pub fn uncovered(entry: &Measured) -> Option<(u64, String)> {
    let lines = &entry.missing_lines;
    let branches = &entry.missing_branches;
    let mut parts = Vec::new();
    if !lines.is_empty() {
        parts.push(format!("line(s) {}", ranges(lines)));
    }
    if !branches.is_empty() {
        let shown: Vec<String> = branches
            .iter()
            .map(|&(from, to)| {
                if to > 0 {
                    format!("{from}->{to}")
                } else {
                    format!("{from}->exit")
                }
            })
            .collect();
        parts.push(format!("branch(es) {}", shown.join(", ")));
    }
    let first = lines
        .iter()
        .copied()
        .chain(
            branches
                .iter()
                .map(|&(from, _)| u64::try_from(from).unwrap_or(0)),
        )
        .min()?;
    Some((
        first,
        format!("coverage.py: not covered: {}", parts.join(" and ")),
    ))
}

/// The files of the report by their path from the repository root `root`: a
/// path the report names relatively is read from the unit's folder `unit`.
#[must_use]
pub fn by_path(report: Report, root: &Path, unit: &Path) -> BTreeMap<String, Measured> {
    report
        .files
        .into_iter()
        .map(|(key, entry)| {
            let path = unit.join(&key);
            let real = fs::canonicalize(&path).unwrap_or(path);
            (relative(root, &real).to_string_lossy().into_owned(), entry)
        })
        .collect()
}

/// The data coverage.py recorded, its parts combined into one file: the
/// file, or nothing, with why written.
fn recorded(session: &mut Session<'_, '_>, paths: &Paths, programs: &Programs) -> Option<PathBuf> {
    let data = paths.temp("coverage-data");
    let name = data.file_name().map(OsString::from).unwrap_or_default();
    let mut parts: Vec<PathBuf> = fs::read_dir(&paths.temp)
        .into_iter()
        .flatten()
        .map_while(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().is_some_and(|file| {
                let mut prefix = name.clone();
                prefix.push(".");
                file.as_encoded_bytes()
                    .starts_with(prefix.as_encoded_bytes())
            })
        })
        .collect();
    parts.sort();
    if !parts.is_empty() {
        let mut combine = uv(paths, programs);
        combine
            .args(["run", "--no-sync", "--with"])
            .arg(&programs.coverage)
            .args(["coverage", "combine"])
            .arg(prefixed("--data-file=", &data))
            .args(&parts);
        if !stream(session, combine, "uv") {
            return None;
        }
    }
    if !fs::metadata(&data).is_ok_and(|file| file.len() > 0) {
        session.log.error(
            None,
            None,
            "coverage.py recorded nothing: the tests did not run",
        );
        return None;
    }
    Some(data)
}

/// Every line and branch is covered.
pub(super) fn check(
    session: &mut Session<'_, '_>,
    paths: &Paths,
    programs: &Programs,
    python: &Interpreter,
) -> bool {
    let Some(data) = recorded(session, paths, programs) else {
        return false;
    };
    let json = paths.temp("coverage.json");
    let mut report = uv(paths, programs);
    report
        .args(["run", "--no-sync", "--with"])
        .arg(&programs.coverage)
        .args(["coverage", "json"])
        .arg(prefixed("--data-file=", &data))
        .arg("-o")
        .arg(&json);
    // Its own threshold, if the repository sets one, decides nothing here.
    stream(session, report, "uv");
    let read = fs::read_to_string(&json)
        .map_err(|error| error.to_string())
        .and_then(|text| serde_json::from_str::<Report>(&text).map_err(|error| error.to_string()));
    let measured = match read {
        Ok(report) => by_path(report, &paths.root, &paths.dir),
        Err(error) => {
            session
                .log
                .error(None, None, &format!("coverage.py wrote no report: {error}"));
            return false;
        }
    };
    let files: Vec<String> = session
        .files
        .iter()
        .filter(|file| has_extension(file, "py"))
        .cloned()
        .collect();
    let mut failed = false;
    for file in &files {
        let Some(entry) = measured.get(file) else {
            if !empty(session, paths, python, file) {
                session.log.error(
                    Some(file),
                    None,
                    "coverage.py: measured none of it: no test runs it, or an `omit` setting \
                     leaves it out; every line and branch of every file counts",
                );
                failed = true;
            }
            continue;
        };
        let Some((first, message)) = uncovered(entry) else {
            continue;
        };
        session.log.line(&format!(
            "{file}: {}% covered",
            entry
                .summary
                .percent_covered_display
                .as_deref()
                .unwrap_or("?")
        ));
        session
            .log
            .error(Some(file), Some((&first.to_string(), None)), &message);
        failed = true;
    }
    if !failed {
        session.log.line(&format!(
            "Every line and branch of {} file(s) is covered.",
            files.len()
        ));
    }
    !failed
}

/// Whether a file holds no statement beyond its docstring, as Python's own
/// parser reads it ([`EMPTY`]); one that cannot be read or parsed holds one.
fn empty(session: &mut Session<'_, '_>, paths: &Paths, python: &Interpreter, file: &str) -> bool {
    let search = session.context.env.search_path.clone();
    let mut asked = command(paths, &python.python, &paths.root, &search);
    asked.args(["-I", "-c", EMPTY]).arg(paths.root.join(file));
    session
        .output(asked, "the project's Python")
        .is_some_and(|output| output.status.success())
}
