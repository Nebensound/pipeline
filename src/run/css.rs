//! The CSS check: every `.css` and `.scss` file is formatted the way Prettier
//! writes it and passes Stylelint.

use std::collections::BTreeSet;
use std::path::Path;

use super::{Entry, Session, Tool};
use crate::linter_config::stylelint::{self, Handed};
use crate::path::RelativePath;
use crate::rules;

/// What the script is handed: the files in groups of the same configuration,
/// in the folder of what the programs are handed (see [`Handed::job`]).
const JOB: &str = "stylelint.json";

/// What runs Stylelint on Node.js, through its own `lint`.
///
/// Stylelint is handed a configuration object the tool built, for each group
/// of files, so it looks for no file of configuration: neither the
/// repository's nor one above it. The tool reads the repository's
/// configuration ([`crate::linter_config::stylelint`]), lays its rules over
/// the pipeline's default (`rules/stylelint.yaml`, compiled into this tool)
/// rule by rule, works out each file's rules from the differences by pattern,
/// and hands the files in groups of the same configuration. Nothing of the
/// repository names a module or a file: what the configuration extends and the
/// syntax for SCSS come from the default alone, by package, and the script
/// hands each to Stylelint by its absolute place in the pinned install,
/// resolved from the folder of the pinned packages and refused where it would
/// lie anywhere else. Stylelint runs with that folder as its working
/// directory, so a package the pinned ones name is found beside them, never in
/// the repository.
///
/// Nothing takes a file out: the configuration holds no `ignoreFiles`,
/// `.stylelintignore` is not read (an empty ignore file is named instead),
/// Stylelint's own leaving out of `node_modules` is switched off, and every
/// file is handed over as its content, so no pattern reads its name as a
/// glob. A file Stylelint still leaves out fails, since it was not checked.
///
/// It prints no workflow command. Every finding is handed to the tool as data,
/// one JSON object per line of the file `FINDINGS` (see
/// [`FINDINGS`](super::FINDINGS)), and the tool writes it, on its line: a
/// warning fails nothing, an error fails the check, as on the command line.
///
/// It reads where the pinned package lies (`TOOLS`), what it is handed
/// (`JOB`), the empty ignore file (`NOTHING`) and where its findings go
/// (`FINDINGS`) from its environment.
pub const STYLELINT: &str = r#"
import fs from "node:fs";
import { join, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

const { TOOLS, JOB, NOTHING, FINDINGS } = process.env;
const modules = join(TOOLS, "node_modules");
const load = async (name, ...path) => import(pathToFileURL(join(modules, name, ...path)).href);
const { default: stylelint } = await load("stylelint", "lib", "index.mjs");
const { default: resolveSilent } = await load("stylelint", "lib", "utils", "resolveSilent.mjs");

// A finding goes to the tool as data, which writes it.
const found = (finding) => fs.appendFileSync(FINDINGS, `${JSON.stringify(finding)}\n`);
const message = (error) => (error instanceof Error ? error.message : String(error));

// A package the pipeline's default names, at its place in the pinned
// install: resolved from the folder of the pinned packages, and refused
// where it would lie anywhere else.
const within = fs.realpathSync(modules) + sep;
const pinned = (name) => {
  const path = resolveSilent(modules, name);
  if (path === undefined || !fs.realpathSync(path).startsWith(within)) {
    throw new Error(`"${name}" is not installed with the pinned Stylelint`);
  }
  return path;
};

const job = JSON.parse(fs.readFileSync(JOB, "utf8"));
for (const group of job.groups) {
  let config;
  try {
    config = { ...group.config, extends: group.extends.map(pinned), ignoreFiles: [] };
    if (group.customSyntax !== null) {
      config.customSyntax = pinned(group.customSyntax);
    }
  } catch (error) {
    for (const file of group.files) {
      found({ level: "error", file, message: `Stylelint cannot check it: ${message(error)}` });
    }
    process.exitCode = 1;
    continue;
  }
  for (const file of group.files) {
    const absolute = resolve(file);
    let result;
    try {
      ({ results: [result] } = await stylelint.lint({
        code: fs.readFileSync(absolute, "utf8"),
        codeFilename: absolute,
        config,
        cwd: TOOLS,
        ignorePath: NOTHING,
        disableDefaultIgnores: true,
        allowEmptyInput: true,
      }));
    } catch (error) {
      found({ level: "error", file, message: `Stylelint cannot check it: ${message(error)}` });
      process.exitCode = 1;
      continue;
    }
    if (result === undefined || result.ignored) {
      found({ level: "error", file, message: "Stylelint left it out, and only an [[exclude]] entry may; it was not checked" });
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
}
"#;

/// Runs the check; whether it found nothing.
pub(super) fn run(session: &mut Session<'_, '_, '_>) -> bool {
    // A missing program is named and fails the check, and the parts whose
    // programs were found still run, so that one run reports every finding.
    let prettier = session.tool("prettier", false);
    let linter = session.tool(stylelint::NAME, true);
    let (configured, whole) = configuration(session);
    // The default compiled into the tool reads; a test holds it to that.
    let job = configured.job(
        &stylelint::pinned(rules::STYLELINT).unwrap_or_default(),
        &session.files,
    );
    let Some(temp) = session.folder(|_| {
        let entries: Vec<Entry> = vec![
            ("nothing-ignored".to_owned(), Some(Vec::new())),
            (super::FINDINGS.to_owned(), Some(Vec::new())),
            (JOB.to_owned(), Some(job.into_bytes())),
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
    let linted = linter.is_some_and(|linter| stylelint_run(session, &linter, &temp));
    formatted && whole && linted
}

/// The repository's configuration of Stylelint, read the way the rules
/// report reads it, with the lines the report shows for it, and whether it
/// is handed over whole: every problem with it is a finding at its file, and
/// what it is about is not handed over.
fn configuration(session: &mut Session<'_, '_, '_>) -> (Handed, bool) {
    let packages = &session.context.packages;
    let files: Vec<&RelativePath> = packages.tracked.iter().collect();
    let kept: BTreeSet<&RelativePath> = packages.kept.iter().collect();
    let read = packages.repository.reader();
    let (handed, problems) = stylelint::read_in(
        session.context.register,
        &files,
        &|file| kept.contains(file),
        &read,
    );
    for line in rules::stylelint(rules::STYLELINT, &handed, &problems) {
        session.log.line(&line);
    }
    for problem in &problems {
        session
            .log
            .error(Some(&problem.file.to_string()), None, &problem.message);
    }
    (handed, problems.is_empty())
}

/// Stylelint over the files, with the configuration [`configuration`] gives
/// each: see [`STYLELINT`]. What it prints goes into the output as it is, and
/// the findings it hands over after it.
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
        .env("JOB", temp.join(JOB))
        .env("NOTHING", temp.join("nothing-ignored"))
        .env("FINDINGS", temp.join(super::FINDINGS));
    let ran = session.capture(command, "Stylelint");
    session.log.lines(&ran.text);
    session.handed(&temp.join(super::FINDINGS), "Stylelint") && ran.success
}
