//! The versions as a table: what `nebensound version` prints, and what the job
//! `Version` writes into the summary of a run.
//!
//! One table in two forms, text for a terminal and Markdown for the summary,
//! built from the same rows, so that the two cannot say different things about
//! the same commit.

use nebensound_declaration::{Analysis, TagVersion, UnitVersion, Version};

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
        "Last release",
        if pull_request.is_some() {
            "This pull request"
        } else {
            "This commit"
        },
        "Waiting",
    ]
}

/// One unit as a row of the table, unescaped.
fn row(unit: &UnitVersion, pull_request: Option<u64>) -> [String; 6] {
    let last = if unit.base == Version::ZERO {
        "none".to_owned()
    } else {
        unit.base.to_string()
    };
    let version = match (&unit.version, pull_request) {
        (None, _) => "none".to_owned(),
        (Some(version), None) => version.to_string(),
        (Some(version), Some(number)) => of_pull_request(version, number),
    };
    [
        unit.name.to_string(),
        unit.path.to_string(),
        unit.release.to_string(),
        last,
        version,
        unit.waiting.to_string(),
    ]
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
        "Last release is the highest full release, waiting the commits since it that touch the \
         unit, this commit included. A unit this commit does not touch has no version here."
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
