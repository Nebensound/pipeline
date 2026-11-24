//! The link check the Markdown and the HTML check share: every link into the
//! repository leads somewhere.

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use super::{Entry, Session, Tool};
use crate::lychee_config;

/// The configuration the tool writes for lychee, in the folder of what the
/// programs are handed.
const CONFIG: &str = "lychee.toml";
/// The files lychee reads, one per line, in that folder.
const LINES: &str = "lines";
/// lychee's report, in that folder.
const REPORT: &str = "links.md";

/// What the link check hands lychee.
pub(super) struct Handed {
    /// The configuration the tool writes from the repository's (see
    /// [`lychee_config`]).
    config: Vec<u8>,
    /// The files lychee reads: see [`lychee_lines`].
    lines: String,
}

impl Handed {
    /// The entries of the folder of what the programs are handed that the
    /// link check reads, empty where nothing is handed over.
    pub(super) fn entries(handed: Option<&Self>) -> [Entry; 2] {
        [
            (
                CONFIG.to_owned(),
                Some(
                    handed
                        .map(|handed| handed.config.clone())
                        .unwrap_or_default(),
                ),
            ),
            (
                LINES.to_owned(),
                Some(
                    handed
                        .map(|handed| handed.lines.clone().into_bytes())
                        .unwrap_or_default(),
                ),
            ),
        ]
    }
}

/// What the link check hands lychee, read from the repository's
/// `lychee.toml` and `.lycheeignore` at the root (see [`lychee_config`]), or
/// [`None`] where it hands nothing, with every reason written as a finding.
///
/// The configuration is always named, never looked for: without one named,
/// lychee would take the first it finds where it starts, `lychee.toml`, else
/// a lychee table in `Cargo.toml`, `pyproject.toml` or `package.json`. Where
/// the repository has no `lychee.toml`, the one written sets nothing beyond
/// `.lycheeignore`. A linked `lychee.toml` phase 0 refuses already, and the
/// link check reads it as none. `.lycheeignore` is read only as a regular
/// file: phase 0 does not judge it, and a link could point out of the
/// repository.
///
/// The files are the ones the check covers less those `exclude_path` takes
/// out, matched against the path from the root as phase 0 matches it.
pub(super) fn hand_over(session: &mut Session<'_, '_, '_>) -> Option<Handed> {
    let root = session.context.root;
    let config = read(session, &root.join(CONFIG), CONFIG, false)?;
    let ignore = read(session, &root.join(".lycheeignore"), ".lycheeignore", true)?;
    let handed = match lychee_config::hand_over(&config, &ignore) {
        Ok(handed) => handed,
        Err(reasons) => {
            for reason in reasons {
                session.log.error(Some(CONFIG), None, &reason);
            }
            return None;
        }
    };
    let files: Vec<String> = session
        .files
        .iter()
        .filter(|file| {
            !handed
                .exclude_path
                .as_ref()
                .is_some_and(|set| set.is_match(file))
        })
        .cloned()
        .collect();
    Some(Handed {
        config: handed.settings.to_string().into_bytes(),
        lines: lychee_lines(root, &files),
    })
}

/// A file at the root as text, empty where there is none or, unless `strict`,
/// where it is no regular file; [`None`] with a finding where it cannot be
/// read, or where it is there but no regular file and `strict` holds.
fn read(
    session: &mut Session<'_, '_, '_>,
    path: &Path,
    name: &str,
    strict: bool,
) -> Option<String> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Some(String::new());
    };
    if !metadata.is_file() {
        if !strict {
            return Some(String::new());
        }
        session.log.error(
            Some(name),
            None,
            "is a link or no regular file; the link check reads it only as it stands in the \
             repository",
        );
        return None;
    }
    match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) => {
            session
                .log
                .error(Some(name), None, &format!("cannot be read: {error}"));
            None
        }
    }
}

/// The files as the link check reads them: one absolute path per line, the
/// root in front of the path from the root. A file name holding a line break
/// is not something it could be given.
///
/// The path is absolute because lychee starts in a folder of its own, not at
/// the root; its report names each file by that path, and the link check
/// takes the root off again. A line holding a glob character is expanded, which
/// yields the same path. Where lychee would misread a line, one character of
/// it is put in brackets, which makes the line a glob naming just that file
/// ([`lychee_line`]). The root is written as [`lychee_root`] writes it.
#[must_use]
pub fn lychee_lines(root: &Path, files: &[String]) -> String {
    let root = lychee_root(root);
    let mut text = String::new();
    for file in files {
        text.push_str(&lychee_line(&format!("{root}/{file}")));
        text.push('\n');
    }
    text
}

/// The repository root as the link check writes it in front of each file and
/// takes it off lychee's report again: as it stands, except a Windows
/// verbatim path, which is what `fs::canonicalize` returns there. Its `?`
/// would make the line a glob, which the `glob` crate reads as a network
/// share `?` that matches no file, and lychee would check nothing and pass.
/// Without the prefix it
/// names the same folder: `\\?\C:\repo` becomes `C:\repo`, and
/// `\\?\UNC\server\share` becomes `\\server\share`.
#[must_use]
pub fn lychee_root(root: &Path) -> String {
    let root = root.to_string_lossy();
    if let Some(share) = root.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else {
        root.strip_prefix(r"\\?\").unwrap_or(&root).to_owned()
    }
}

/// One path as a line for the link check, which lychee reads as just that
/// file. One character is put in brackets, which makes the line a glob:
///
/// - every glob character, since `[docs]/a.md` would otherwise match nothing
///   and pass;
/// - a `#` at the start, which makes the line a comment;
/// - white space at either end, which lychee trims off;
/// - the first letter of a first segment holding a `:` after its second
///   character, which lychee would read as an address, such as
///   `mailto:a.md`. A `:` right after the first letter, as in the drive of
///   `C:\repo`, lychee reads as a path, not as an address, so the line stays
///   a path: a bracket there would make it a glob that names no drive.
///
/// The `#` and the white space at the start, and the address, concern a path
/// from the root only: an absolute path, as [`lychee_lines`] writes, starts
/// with `/`, or on Windows with a drive and `:` or with `\\`.
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
        && !segment[1..].starts_with(':')
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
/// lychee reads what [`hand_over`] wrote into `folder`, the folder of what
/// the programs are handed, and nothing else of the repository's
/// configuration: it starts in `folder`, so the `.lycheeignore` it would read
/// where it starts is not the repository's, and it is named every
/// configuration, so it looks for none. Which addresses it leaves out, which
/// status codes it accepts and how long it waits stay the repository's
/// configuration, carried over by the tool. `--hidden` and `--no-ignore` keep
/// it from skipping a file in a hidden folder or one a `.gitignore` names. A
/// link from the root of a site, such as `/style.css`, is resolved below
/// `root`; every configuration of `more` is handed over behind the one the
/// tool wrote. Where `exclude_path` took out every file, lychee does not
/// start.
///
/// Its report, in Markdown, goes into the output; every broken link in it
/// becomes a finding on the file that holds it, at the line and column
/// lychee names, the file named from the repository root.
pub(super) fn check(
    session: &mut Session<'_, '_, '_>,
    lychee: &Tool,
    handed: &Handed,
    folder: &Path,
    more: &[&Path],
    root: &Path,
) -> bool {
    if handed.lines.is_empty() {
        session.log.line(
            "lychee is not started: `exclude_path` in lychee.toml takes out every file it would \
             read",
        );
        return true;
    }
    let report = folder.join(REPORT);
    let mut args: Vec<OsString> = vec![
        "--offline".into(),
        "--include-fragments".into(),
        "--no-progress".into(),
        "--hidden".into(),
        "--no-ignore".into(),
        "--config".into(),
        folder.join(CONFIG).into(),
    ];
    for config in more {
        args.push("--config".into());
        args.push(config.into());
    }
    args.extend([
        "--root-dir".into(),
        root.into(),
        "--files-from".into(),
        folder.join(LINES).into(),
        "--format".into(),
        "markdown".into(),
        "--output".into(),
        report.clone().into(),
    ]);
    let mut command = session.command(&lychee.executable, &lychee.search_path);
    command.current_dir(folder).args(&args);
    let ran = session.capture(command, "lychee");
    session.log.lines(&ran.text);
    let report = fs::read_to_string(report).unwrap_or_default();
    session.log.lines(&report);
    let prefix = format!("{}/", lychee_root(session.context.root));
    for link in broken(&report) {
        session.log.error(
            Some(link.file.strip_prefix(&prefix).unwrap_or(&link.file)),
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
