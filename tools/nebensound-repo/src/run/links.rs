//! The link check the Markdown and the HTML check share: every link into the
//! repository leads somewhere.

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use super::{Session, Tool};

/// The configuration of the link check, which is named and never looked for.
/// Without one named, lychee takes the first it finds at the root:
/// `lychee.toml`, else a lychee table in `Cargo.toml`, `pyproject.toml` or
/// `package.json`, and an `exclude_path` in one of those three would take
/// files out unseen by phase 0. So it is handed a copy of `lychee.toml`, or
/// an empty file where there is none, which it reads as a configuration that
/// sets nothing. A linked `lychee.toml` phase 0 refuses already.
pub(super) fn config(session: &mut Session<'_, '_>) -> Option<Vec<u8>> {
    let path = session.context.root.join("lychee.toml");
    let regular = fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file());
    if !regular {
        return Some(Vec::new());
    }
    match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) => {
            session.log.error(
                Some("lychee.toml"),
                None,
                &format!("cannot be read: {error}"),
            );
            None
        }
    }
}

/// The files as the link check reads them: one path per line. A file name
/// holding a line break is not something it could be given.
///
/// Each line is the path from the repository root, such as `docs/a.md`:
/// lychee matches `exclude_path` against the string it is handed, and phase
/// 0 judges that setting against exactly this one. A line holding a glob
/// character is expanded, which yields the path from the root as well. So
/// nothing is put in front of a path; where lychee would misread a line, one
/// character of it is put in brackets instead, which makes the line a glob
/// naming just that file:
///
/// - every glob character, since `[docs]/a.md` would otherwise match nothing
///   and pass;
/// - a `#` at the start, which makes the line a comment;
/// - white space at either end, which lychee trims off;
/// - the first letter of a first segment holding a `:`, which lychee would
///   read as an address, such as `mailto:a.md`.
#[must_use]
pub fn lychee_lines(files: &[String]) -> String {
    let mut text = String::new();
    for file in files {
        text.push_str(&lychee_line(file));
        text.push('\n');
    }
    text
}

/// One path as a line for the link check: see [`lychee_lines`].
#[must_use]
pub fn lychee_line(file: &str) -> String {
    let bracket = |c: char| format!("[{c}]");
    let mut line: String = file
        .chars()
        .map(|c| {
            if "[]*?{}".contains(c) {
                bracket(c)
            } else {
                c.to_string()
            }
        })
        .collect();
    if let Some(first) = line.chars().next().filter(|c| *c == '#' || is_space(*c)) {
        line = format!("{}{}", bracket(first), &line[first.len_utf8()..]);
    }
    if let Some(last) = line.chars().next_back().filter(|c| is_space(*c)) {
        line = format!("{}{}", &line[..line.len() - last.len_utf8()], bracket(last));
    }
    let segment = line.split('/').next().unwrap_or_default();
    if let Some(first) = line.chars().next().filter(char::is_ascii_alphabetic)
        && segment[1..].contains(':')
    {
        line = format!("{}{}", bracket(first), &line[1..]);
    }
    line
}

/// White space as the C locale has it.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

/// Links into the repository only: a file that is not there, a heading or an
/// `id` that is not in it. `--offline` sends no request, so a pull request
/// does not turn red because someone else's server is down; links to the
/// outside are not checked in a pull request at all. A file without a link is
/// not a finding.
///
/// It runs at the repository root and reads the repository's `lychee.toml`
/// (the first of `configs`, see [`config`]) and `.lycheeignore` there: which
/// addresses it leaves out, which status codes it accepts, how long it waits
/// stay the repository's configuration. An `exclude_path` in `lychee.toml`
/// would take files out; phase 0 matches it against the same path from the
/// root that each line of `lines` hands over, and fails on every file it
/// takes out that no `[[exclude]]` entry does, so none of them is in the
/// list. `--hidden` and `--no-ignore` keep it from skipping a file in a
/// hidden folder or one a `.gitignore` names. A link from the root of a site,
/// such as `/style.css`, is resolved below `root`; every configuration after
/// the first is handed over behind it.
///
/// Its report, in Markdown, goes into the output; every broken link in it
/// becomes a finding on the file that holds it, at the line and column
/// lychee names. The report goes to `report`.
pub(super) fn check(
    session: &mut Session<'_, '_>,
    lychee: &Tool,
    configs: &[&Path],
    root: &Path,
    lines: &Path,
    report: &Path,
) -> bool {
    let mut args: Vec<OsString> = vec![
        "--offline".into(),
        "--include-fragments".into(),
        "--no-progress".into(),
        "--hidden".into(),
        "--no-ignore".into(),
    ];
    for config in configs {
        args.push("--config".into());
        args.push(config.into());
    }
    args.extend([
        "--root-dir".into(),
        root.into(),
        "--files-from".into(),
        lines.into(),
        "--format".into(),
        "markdown".into(),
        "--output".into(),
        report.into(),
    ]);
    let ran = session.execute(lychee, &args, None, "lychee");
    session.log.lines(&ran.text);
    let report = fs::read_to_string(report).unwrap_or_default();
    session.log.lines(&report);
    for link in broken(&report) {
        session.log.error(
            Some(&link.file),
            link.place
                .as_ref()
                .map(|(line, column)| (line.as_str(), Some(column.as_str()))),
            &link.message,
        );
    }
    ran.success
}

/// A link lychee's report names as broken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broken {
    /// The file that holds it, as lychee names it.
    pub file: String,
    /// Its line and column, if lychee gives them.
    pub place: Option<(String, String)>,
    /// The entry without them, such as `[ERROR] <file:///…/missing.md> |
    /// File not found`.
    pub message: String,
}

/// Every link lychee's Markdown report names as an error or a timeout.
#[must_use]
pub fn broken(report: &str) -> Vec<Broken> {
    let mut found = Vec::new();
    let mut file: Option<&str> = None;
    for line in report.lines() {
        if let Some(heading) = line.strip_prefix("### ") {
            file = heading
                .strip_prefix("Errors in ")
                .or_else(|| heading.strip_prefix("Timeouts in "));
        } else if line.starts_with("## ") || line.starts_with("# ") {
            file = None;
        } else if let (Some(file), Some(entry)) = (file, line.strip_prefix("* ")) {
            let (message, place) = match span(entry) {
                Some((before, row, column, after)) => (
                    format!("{before}{after}"),
                    Some((row.to_owned(), column.to_owned())),
                ),
                None => (entry.to_owned(), None),
            };
            found.push(Broken {
                file: file.to_owned(),
                place,
                message,
            });
        }
    }
    found
}

/// An entry with its place, `… <url> (at <line>:<column>)…`, taken apart: what
/// stands before the place, the line, the column and what stands after it.
fn span(entry: &str) -> Option<(&str, &str, &str, &str)> {
    let (before, rest) = entry.split_once("> (at ")?;
    let (place, after) = rest.split_once(')')?;
    let (row, column) = place.split_once(':')?;
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    (digits(row) && digits(column)).then_some((&entry[..=before.len()], row, column, after))
}
