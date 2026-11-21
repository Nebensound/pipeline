//! The Markdown check: every `.md` and `.markdown` file is formatted the way
//! Prettier writes it, passes Markdownlint, and its links into the repository
//! lead somewhere.

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use super::{Entry, Session, Tool, relative};
use crate::rules;

/// What runs markdownlint-cli2 on Node.js, through its own `main`.
///
/// The pipeline's default (`rules/markdownlint.jsonc`, compiled into this
/// tool) lies beneath every configuration of the repository, which is laid
/// over it rule by rule: a rule the repository does not name keeps the
/// default's value, so its file names only what differs, and a change of the
/// default reaches it.
///
/// markdownlint-cli2 gets the default as its base options, beneath everything
/// it reads. It lays the `config` of a `.markdownlint-cli2.*` options file
/// over what lies above it rule by rule, in every folder. A `.markdownlint.*`
/// file, which holds rules only, it would take instead of everything above
/// it, the default included. So the one at the root is read here the way
/// markdownlint-cli2 reads it (`extends` included) and laid over the default,
/// and markdownlint-cli2 is handed a view of the files in which that one is
/// not there. One in a folder below the root still takes the place of
/// everything above it in its folder; the reference says so.
///
/// Its rules hold, but nothing in it takes a file out. markdownlint-cli2
/// would skip a named file in three ways: through `ignores` in any options
/// file, and through a negated entry of `globs` or through `gitignore` in the
/// options file of the folder it starts from. So every file is handed over
/// literally (`:` in front), which `gitignore` does not reach; it starts from
/// an empty folder (`BASE`), so the repository's options file counts as that
/// of the repository's folder, whose `globs` play no part; and `ignores` is
/// overridden with an empty list for every folder. What remains is the
/// repository's rules over the default, read as markdownlint-cli2 reads them.
///
/// It reads where the pinned package lies (`TOOLS`), the folder it starts
/// from (`BASE`), the list of files, each ended by a zero byte (`LIST`), and
/// the default (`DEFAULT`) from its environment.
pub const MARKDOWNLINT: &str = r#"
import fs from "node:fs";
import { basename, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const { TOOLS, BASE, LIST, DEFAULT } = process.env;
const load = async (...path) =>
  import(pathToFileURL(join(TOOLS, "node_modules", "markdownlint-cli2", ...path)).href);
const { main } = await load("markdownlint-cli2.mjs");
const { readConfig, extendConfig } = await load("export-markdownlint-promise.mjs");
const { default: parsers } = await load("parsers", "parsers.mjs");

// As markdownlint-cli2 reads a file that holds rules only: the
// first of these names in a folder, `extends` resolved.
const names = [
  ".markdownlint.jsonc", ".markdownlint.json", ".markdownlint.yaml",
  ".markdownlint.yml", ".markdownlint.cjs", ".markdownlint.mjs",
];
const rules = async (file) => {
  if (!/\.[cm]js$/.test(file)) {
    return readConfig(file, parsers, fs);
  }
  const config = { ...(await import(pathToFileURL(file).href)).default };
  return config.extends ? extendConfig(config, file, parsers, fs) : config;
};
const atRoot = names.map((name) => join(process.cwd(), name));
const own = atRoot.find((file) => fs.existsSync(file));
let config = await readConfig(DEFAULT, parsers, fs);
if (own) {
  config = { ...config, ...(await rules(own)) };
}
// What lies over the default at the root, for the log; what lies
// over it in a folder below, `nebensound-repo check` names.
const options = [
  ".markdownlint-cli2.jsonc", ".markdownlint-cli2.yaml",
  ".markdownlint-cli2.cjs", ".markdownlint-cli2.mjs",
].find((name) => fs.existsSync(join(process.cwd(), name)));
const over = [own && basename(own), options].filter(Boolean);
console.log(over.length === 0
  ? "Rules: the pipeline default"
  : `Rules: the pipeline default, and laid over it ${over.join(", then ")}`);
// The view of the files markdownlint-cli2 is handed: the same,
// less the file at the root that holds rules only, which is laid
// over the default above.
const hidden = new Set(atRoot);
const view = {
  ...fs,
  promises: {
    ...fs.promises,
    access: (file, mode) =>
      hidden.has(file)
        ? Promise.reject(Object.assign(new Error(`ENOENT: ${file}`), { code: "ENOENT" }))
        : fs.promises.access(file, mode),
  },
};
const argv = fs.readFileSync(LIST, "utf8")
  .split("\0")
  .filter((file) => file !== "")
  .map((file) => `:${resolve(file)}`);
process.exitCode = await main({
  directory: BASE,
  argv,
  fs: view,
  logMessage: (message) => console.log(message),
  logError: (message) => console.error(message),
  optionsDefault: { config },
  optionsOverride: { ignores: [] },
});
"#;

/// Runs the check; whether it found nothing.
pub(super) fn run(session: &mut Session<'_, '_>) -> bool {
    let prettier = session.tool("prettier", false);
    let markdownlint = session.tool("markdownlint-cli2", true);
    let lychee = session.tool("lychee", false);
    let (Some(prettier), Some(markdownlint), Some(lychee)) = (prettier, markdownlint, lychee)
    else {
        return false;
    };
    let Some(config) = lychee_config(session) else {
        return false;
    };
    let mut list = Vec::new();
    for file in &session.files {
        list.extend_from_slice(file.as_bytes());
        list.push(0);
    }
    let lines = lychee_lines(&session.files);
    let Some(temp) = session.folder(|_| {
        let entries: Vec<Entry> = vec![
            ("nothing-ignored".to_owned(), Some(Vec::new())),
            ("base".to_owned(), None),
            ("files".to_owned(), Some(list)),
            ("lines".to_owned(), Some(lines.into_bytes())),
            ("lychee.toml".to_owned(), Some(config)),
            (
                "markdownlint.jsonc".to_owned(),
                Some(rules::MARKDOWNLINT.as_bytes().to_vec()),
            ),
        ];
        entries
    }) else {
        return false;
    };
    let files = session.files.clone();
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    // Each part runs even when one before it failed, so that one run reports
    // every finding at once.
    let formatted = session.prettier(&prettier, &temp.join("nothing-ignored"), &files);
    let rules_hold = markdownlint_run(session, &markdownlint, &temp);
    let links_lead = links(session, &lychee, &temp);
    formatted && rules_hold && links_lead
}

/// The configuration of the link check, which is named and never looked for.
/// Without one named, lychee takes the first it finds at the root:
/// `lychee.toml`, else a lychee table in `Cargo.toml`, `pyproject.toml` or
/// `package.json`, and an `exclude_path` in one of those three would take
/// files out unseen by phase 0. So it is handed a copy of `lychee.toml`, or
/// an empty file where there is none, which it reads as a configuration that
/// sets nothing. A linked `lychee.toml` phase 0 refuses already.
fn lychee_config(session: &mut Session<'_, '_>) -> Option<Vec<u8>> {
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

/// Markdownlint over the files, with the repository's rules laid over the
/// pipeline's default: see [`MARKDOWNLINT`]. It names every file from the
/// folder it starts from, so that part is taken off again where a finding is
/// written. A finding reads `<file>:<line>[:<column>] error <rule>
/// <message>`; each becomes one on its line, and stays in the output as it
/// is.
fn markdownlint_run(session: &mut Session<'_, '_>, markdownlint: &Tool, temp: &Path) -> bool {
    // The pinned copy, which `Session::tool` takes only with its runtime
    // when it is installed with npm, as the register has it.
    let Some(node) = &markdownlint.runtime else {
        session.log.error(
            None,
            None,
            "markdownlint-cli2 runs on Node.js, and the register gives it no runtime to run on",
        );
        return false;
    };
    // Both without links, so that the part in front of a file is the same
    // as Node.js computes it, from the folder it starts in.
    let base = fs::canonicalize(temp.join("base")).unwrap_or_default();
    let prefix = format!("{}/", relative(&base, session.context.root).display());
    let mut command = session.command(node, &markdownlint.search_path);
    command
        .args(["--input-type=module", "--eval", MARKDOWNLINT])
        .env("TOOLS", &markdownlint.folder)
        .env("BASE", &base)
        .env("LIST", temp.join("files"))
        .env("DEFAULT", temp.join("markdownlint.jsonc"));
    let ran = session.capture(command, "markdownlint-cli2");
    for line in ran.text.lines() {
        let line = line.strip_prefix(&prefix).unwrap_or(line);
        session.log.line(line);
        if let Some((file, row, column, message)) = markdownlint_finding(line) {
            session.log.error(Some(file), Some((row, column)), message);
        }
    }
    ran.success
}

/// A finding of markdownlint-cli2, `<file>:<line>[:<column>] error <rule>
/// <message>`, taken apart. Of two readings the one with the shorter file
/// name holds: `README.md:3:7` is line 3, column 7 of `README.md`, since a
/// column is far more common than a file name that holds `:` and a number.
#[must_use]
pub fn markdownlint_finding(line: &str) -> Option<(&str, &str, Option<&str>, &str)> {
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    let colons: Vec<usize> = line.match_indices(':').map(|(index, _)| index).collect();
    colons.iter().find_map(|&colon| {
        let (file, rest) = (&line[..colon], &line[colon + 1..]);
        if file.is_empty() {
            return None;
        }
        let (place, message) = rest.split_once(" error ")?;
        if message.is_empty() {
            return None;
        }
        match place.split_once(':') {
            None if digits(place) => Some((file, place, None, message)),
            Some((row, column)) if digits(row) && digits(column) => {
                Some((file, row, Some(column), message))
            }
            _ => None,
        }
    })
}

/// Links into the repository only: a file that is not there, a heading that
/// is not in it. `--offline` sends no request, so a pull request does not
/// turn red because someone else's server is down; links to the outside are
/// not checked in a pull request at all. A file without a link is not a
/// finding.
///
/// It runs at the repository root and reads the repository's `lychee.toml`
/// (named with `--config`, see [`lychee_config`]) and `.lycheeignore` there:
/// which addresses it leaves out, which status codes it accepts, how long it
/// waits stay the repository's configuration. An `exclude_path` in
/// `lychee.toml` would take files out; phase 0 matches it against the same
/// path from the root that each line hands over, and fails on every file it
/// takes out that no `[[exclude]]` entry does, so none of them is in the
/// list. `--hidden` and `--no-ignore` keep it from skipping a file in a
/// hidden folder or one a `.gitignore` names.
///
/// Its report, in Markdown, goes into the output; every broken link in it
/// becomes a finding on the file that holds it, at the line and column
/// lychee names.
fn links(session: &mut Session<'_, '_>, lychee: &Tool, temp: &Path) -> bool {
    let report = temp.join("links.md");
    let args: Vec<OsString> = vec![
        "--offline".into(),
        "--include-fragments".into(),
        "--no-progress".into(),
        "--hidden".into(),
        "--no-ignore".into(),
        "--config".into(),
        temp.join("lychee.toml").into(),
        "--root-dir".into(),
        session.context.root.into(),
        "--files-from".into(),
        temp.join("lines").into(),
        "--format".into(),
        "markdown".into(),
        "--output".into(),
        report.clone().into(),
    ];
    let ran = session.execute(lychee, &args, None, "lychee");
    session.log.lines(&ran.text);
    let report = fs::read_to_string(&report).unwrap_or_default();
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
