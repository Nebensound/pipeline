//! The versions as a table: what `nebensound-repo version` prints, and what the
//! job `Version` writes into the summary of a run.
//!
//! One table in two forms, text for a terminal and Markdown for the summary,
//! built from the same rows, so that the two cannot say different things about
//! the same commit.

use crate::{Analysis, TagVersion, UnitVersion};

use crate::report::{self, shown};

/// A pull request, as the job `Version` names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    /// Its number.
    pub number: u64,
    /// The message its squash merge would carry: its title and description.
    pub message: String,
}

/// The version of a pull request: the version its merge would get, with
/// `-pr.<number>` behind it. It names artifacts and is never published.
#[must_use]
pub fn of_pull_request(version: &TagVersion, number: u64) -> String {
    format!("{version}-pr.{number}")
}

/// The column headers, in the order of [`row`].
fn headers(pull_request: Option<u64>) -> [&'static str; 6] {
    [
        "Unit",
        "Path",
        "Release",
        "Current",
        if pull_request.is_some() {
            "This pull request"
        } else {
            "This commit"
        },
        "Waiting",
    ]
}

/// One unit as a row of the table, unescaped.
///
/// The current version is the commit's own where it has one, and only that
/// one carries the number of a pull request. Waiting is filled in two stages
/// only: in one stage every commit that touches the unit is a release of its
/// own, and nothing waits.
fn row(unit: &UnitVersion, pull_request: Option<u64>) -> [String; 6] {
    let written = |version: &TagVersion| match pull_request {
        None => version.to_string(),
        Some(number) => of_pull_request(version, number),
    };
    let version = unit
        .version
        .as_ref()
        .map_or_else(|| "none".to_owned(), written);
    let current = match (&unit.version, &unit.current) {
        (Some(_), _) => version.clone(),
        (None, Some(current)) => current.to_string(),
        (None, None) => "none".to_owned(),
    };
    let waiting = unit
        .waiting
        .map_or_else(String::new, |waiting| waiting.to_string());
    [
        unit.name.to_string(),
        unit.path.to_string(),
        unit.release.to_string(),
        current,
        version,
        waiting,
    ]
}

/// What `nebensound-repo version` prints: the table as text or as Markdown, for
/// the pull request if there is one.
///
/// A version read off a declaration with errors could be one the history does
/// not give, so then there is no table but every finding, so that the job says
/// why it failed.
#[must_use]
pub fn report(analysis: &Analysis, markdown: bool, pull_request: Option<&PullRequest>) -> String {
    let pull_request = pull_request.map(|pull_request| pull_request.number);
    match (analysis.failed(), markdown) {
        (true, false) => refusal_text(analysis),
        (true, true) => refusal_markdown(analysis),
        (false, false) => text(&analysis.versions, pull_request),
        (false, true) => self::markdown(&analysis.versions, pull_request),
    }
}

/// The versions as an aligned text table.
#[must_use]
pub fn text(versions: &[UnitVersion], pull_request: Option<u64>) -> String {
    let mut rows = vec![headers(pull_request).map(ToOwned::to_owned)];
    rows.extend(
        versions
            .iter()
            .map(|unit| row(unit, pull_request).map(|cell| shown(&cell))),
    );
    let mut widths = [0; 6];
    for cells in &rows {
        for (width, cell) in widths.iter_mut().zip(cells) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let mut lines = vec!["Versions".to_owned(), String::new()];
    for cells in &rows {
        let line: String = cells
            .iter()
            .zip(widths)
            .map(|(cell, width)| format!("{cell:width$}"))
            .collect::<Vec<_>>()
            .join("  ");
        lines.push(format!("  {}", line.trim_end()));
    }
    format!("{}\n", lines.join("\n"))
}

/// The versions as a Markdown table, for the summary of a run.
#[must_use]
pub fn markdown(versions: &[UnitVersion], pull_request: Option<u64>) -> String {
    let mut lines = vec!["## Versions".to_owned(), String::new()];
    lines.push(table_line(&headers(pull_request).map(ToOwned::to_owned)));
    lines.push(table_line(&headers(pull_request).map(|_| "---".to_owned())));
    for unit in versions {
        lines.push(table_line(
            &row(unit, pull_request).map(|cell| escaped(&cell)),
        ));
    }
    lines.push(String::new());
    lines.push(
        "Current is the highest version the history gives a commit up to this one. A unit this \
         commit does not touch has no version of its own here. Waiting, for a unit released in \
         two stages, is the commits since its last promotion that touch it, this commit \
         included."
            .to_owned(),
    );
    if let Some(number) = pull_request {
        lines.push(String::new());
        lines.push(format!(
            "This is pull request {number}. Its version is the one its squash merge would get, \
             with `-pr.{number}` behind it: it names artifacts and is never published."
        ));
    }
    format!("{}\n", lines.join("\n"))
}

/// Why there is no table, with every finding, as text.
#[must_use]
pub fn refusal_text(analysis: &Analysis) -> String {
    format!(
        "there are no versions: phase 0 found errors\n\n{}\n",
        report::findings(analysis)
    )
}

/// Why there is no table, with every finding, as Markdown.
#[must_use]
pub fn refusal_markdown(analysis: &Analysis) -> String {
    let mut lines = vec![
        "## Versions".to_owned(),
        String::new(),
        "There are no versions: phase 0 found errors.".to_owned(),
        String::new(),
    ];
    lines.extend(
        analysis
            .findings()
            .iter()
            .map(|finding| format!("- {}", escaped(&finding.to_string()))),
    );
    format!("{}\n", lines.join("\n"))
}

fn table_line(cells: &[String]) -> String {
    format!("| {} |", cells.join(" | "))
}

/// A piece of text as Markdown shows it literally.
///
/// A path or a finding can hold any character, and one of them could start a
/// link, close a table cell or open an element. Every ASCII punctuation mark is
/// escaped, which Markdown allows for each of them, and a control character is
/// written out as [`shown`] writes it.
fn escaped(text: &str) -> String {
    shown(text)
        .chars()
        .fold(String::new(), |mut out, character| {
            if character.is_ascii_punctuation() {
                out.push('\\');
            }
            out.push(character);
            out
        })
}
