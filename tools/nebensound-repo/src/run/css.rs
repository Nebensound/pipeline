//! The CSS check: every `.css` and `.scss` file is formatted the way Prettier
//! writes it and passes Stylelint.

use std::path::Path;

use super::{Entry, Session, Tool};
use crate::rules;

/// What runs Stylelint on Node.js, through its own `lint`.
///
/// The pipeline's default (`rules/stylelint.yaml`, compiled into this tool)
/// lies beneath the repository's own configuration, which is laid over it
/// rule by rule: a rule the repository does not name keeps the default's
/// value, so its file names only what differs, and a change of the default
/// reaches it. What its own `extends` names comes along with it, over the
/// default as well.
///
/// The configuration of a file is the one Stylelint finds for it, the first
/// of its file names (`package.json`, `.stylelintrc`, `.stylelintrc.json`,
/// `.stylelintrc.yaml`, `stylelint.config.mjs`, ...) in the file's folder or
/// the nearest folder above it, up to the repository root. Stylelint gets it
/// as the last of what its configuration extends, with the default first, so
/// Stylelint itself lays the one over the other. An `overrides` entry matches
/// its `files` from the repository root.
///
/// Its rules hold, but nothing in it takes a file out: `ignoreFiles` is set
/// aside, `.stylelintignore` is not read (an empty ignore file is named
/// instead), and every file is handed over as its content, so no pattern
/// reads its name as a glob. A file that a configuration it extends would
/// still take out fails, since it was not checked. Stylelint resolves a
/// package named in a configuration from the folder of the configuration and
/// then from where the pinned package lies, so only the pinned packages and
/// their companions are found.
///
/// It prints no workflow command. Every finding is handed to the tool as data,
/// one JSON object per line of the file `FINDINGS` (see
/// [`FINDINGS`](super::FINDINGS)), and the tool writes it, on its line: a
/// warning fails nothing, an error fails the check, as on the command line.
///
/// It reads where the pinned package lies (`TOOLS`), the list of files, each
/// ended by a zero byte (`LIST`), the default (`DEFAULT`, a `.yaml` file, so
/// that it is read as YAML), the empty ignore file (`NOTHING`) and where its
/// findings go (`FINDINGS`) from its environment.
pub const STYLELINT: &str = r#"
import fs from "node:fs";
import { basename, dirname, join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

const { TOOLS, LIST, DEFAULT, NOTHING, FINDINGS } = process.env;
const root = process.cwd();
const load = async (name, ...path) =>
  import(pathToFileURL(join(TOOLS, "node_modules", name, ...path)).href);
const { default: stylelint } = await load("stylelint", "lib", "index.mjs");
const { cosmiconfig } = await load("cosmiconfig", "dist", "index.js");

// The configuration Stylelint finds for a file: the first of its
// file names in the file folder, else in the folder above, up to
// the repository root. What lies above the root is no part of the
// repository, and a configuration in a home folder counts as
// little.
const explorer = cosmiconfig("stylelint", { searchStrategy: "none" });
const found = new Map();
const own = async (file) => {
  for (let dir = dirname(resolve(file)); ; dir = dirname(dir)) {
    if (!found.has(dir)) {
      found.set(dir, await explorer.search(dir));
    }
    const result = found.get(dir);
    if (result && !result.isEmpty) {
      return result;
    }
    if (dir === root || !dir.startsWith(root + sep)) {
      return null;
    }
  }
};

const { config: defaults } = await explorer.load(DEFAULT);
// A finding goes to the tool as data, which writes it.
const found = (finding) => fs.appendFileSync(FINDINGS, `${JSON.stringify(finding)}\n`);
const message = (error) => (error instanceof Error ? error.message : String(error));

const list = fs.readFileSync(LIST, "utf8").split("\0").filter((file) => file !== "");
const shown = new Set();
let laid = false;
for (const file of list) {
  const absolute = resolve(file);
  let result;
  try {
    const config = await own(file);
    if (config && !shown.has(config.filepath)) {
      shown.add(config.filepath);
      laid = true;
      console.log(`Rules: the pipeline default, and laid over it ${relative(root, config.filepath)}`);
    }
    // The configuration is named by its file, so that the paths
    // in it stay relative to it, except where it stands in
    // package.json, which Stylelint would read whole.
    const layer = config
      ? basename(config.filepath) === "package.json" ? config.config : config.filepath
      : [];
    ({ results: [result] } = await stylelint.lint({
      code: fs.readFileSync(absolute, "utf8"),
      codeFilename: absolute,
      config: { extends: [defaults, layer].flat(), ignoreFiles: [] },
      configBasedir: root,
      cwd: TOOLS,
      ignorePath: NOTHING,
      allowEmptyInput: true,
    }));
  } catch (error) {
    found({ level: "error", file, message: `Stylelint cannot check it: ${message(error)}` });
    process.exitCode = 1;
    continue;
  }
  if (result.ignored) {
    found({ level: "error", file, message: "what the Stylelint configuration extends takes it out, and only an [[exclude]] entry may; it was not checked" });
    process.exitCode = 1;
    continue;
  }
  for (const warning of result.invalidOptionWarnings ?? []) {
    found({ level: "error", file, message: `Stylelint: ${warning.text}` });
    process.exitCode = 1;
  }
  // Every finding is one on its line. A warning fails nothing, an
  // error fails the check, as on the command line.
  for (const warning of result.warnings) {
    const level = warning.severity === "error" ? "error" : "warning";
    found({ level, file, line: warning.line, column: warning.column, message: warning.text });
    if (level === "error") {
      process.exitCode = 1;
    }
  }
}
if (!laid) {
  console.log("Rules: the pipeline default");
}
"#;

/// Runs the check; whether it found nothing.
pub(super) fn run(session: &mut Session<'_, '_, '_>) -> bool {
    // A missing program is named and fails the check, and the parts whose
    // programs were found still run, so that one run reports every finding.
    let prettier = session.tool("prettier", false);
    let stylelint = session.tool("stylelint", true);
    let mut list = Vec::new();
    for file in &session.files {
        list.extend_from_slice(file.as_bytes());
        list.push(0);
    }
    let Some(temp) = session.folder(|_| {
        let entries: Vec<Entry> = vec![
            ("nothing-ignored".to_owned(), Some(Vec::new())),
            (super::FINDINGS.to_owned(), Some(Vec::new())),
            ("files".to_owned(), Some(list)),
            (
                "stylelint.yaml".to_owned(),
                Some(rules::STYLELINT.as_bytes().to_vec()),
            ),
        ];
        entries
    }) else {
        return false;
    };
    let files = session.files.clone();
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    // Each part runs even when the other failed, so that one run reports
    // every finding at once.
    let formatted = prettier
        .is_some_and(|prettier| session.prettier(&prettier, &temp.join("nothing-ignored"), &files));
    let linted = stylelint.is_some_and(|stylelint| stylelint_run(session, &stylelint, &temp));
    formatted && linted
}

/// Stylelint over the files, with the repository's configuration laid over
/// the pipeline's default: see [`STYLELINT`]. What it prints goes into the
/// output as it is, and the findings it hands over after it.
fn stylelint_run(session: &mut Session<'_, '_, '_>, stylelint: &Tool, temp: &Path) -> bool {
    // The pinned copy, which `Session::tool` takes only with its runtime
    // when it is installed with npm, as the register has it.
    let Some(node) = &stylelint.runtime else {
        session.log.error(
            None,
            None,
            "Stylelint runs on Node.js, and the register gives it no runtime to run on",
        );
        return false;
    };
    let mut command = session.command(node, &stylelint.search_path);
    command
        .args(["--input-type=module", "--eval", STYLELINT])
        .env("TOOLS", &stylelint.folder)
        .env("LIST", temp.join("files"))
        .env("DEFAULT", temp.join("stylelint.yaml"))
        .env("NOTHING", temp.join("nothing-ignored"))
        .env("FINDINGS", temp.join(super::FINDINGS));
    let ran = session.capture(command, "Stylelint");
    session.log.lines(&ran.text);
    session.handed(&temp.join(super::FINDINGS), "Stylelint") && ran.success
}
