//! The Markdown check: every `.md` and `.markdown` file is formatted the way
//! Prettier writes it, passes Markdownlint, and its links into the repository
//! lead somewhere.

use std::fs;
use std::path::Path;

use super::{Entry, Session, Tool, links, relative};
use crate::rules;

pub use super::links::{Broken, broken, lychee_line, lychee_lines};

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
    // A missing program is named and fails the check, and the parts whose
    // programs were found still run, so that one run reports every finding.
    let prettier = session.tool("prettier", false);
    let markdownlint = session.tool("markdownlint-cli2", true);
    let lychee = session.tool("lychee", false);
    let Some(config) = links::config(session) else {
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
    let formatted = prettier
        .is_some_and(|prettier| session.prettier(&prettier, &temp.join("nothing-ignored"), &files));
    let rules_hold =
        markdownlint.is_some_and(|markdownlint| markdownlint_run(session, &markdownlint, &temp));
    let root = session.context.root;
    let links_lead = lychee.is_some_and(|lychee| {
        links::check(
            session,
            &lychee,
            &[&temp.join("lychee.toml")],
            root,
            &temp.join("lines"),
            &temp.join("links.md"),
        )
    });
    formatted && rules_hold && links_lead
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
