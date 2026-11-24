//! The Markdown check: every `.md` and `.markdown` file is formatted the way
//! Prettier writes it, passes Markdownlint, and its links into the repository
//! lead somewhere.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::links::{self, Handed};
use super::{Entry, Session, Tool};
use crate::linter_config::markdownlint::{self, Handed as Rules};
use crate::path::RelativePath;
use crate::rules;

pub use super::links::{Broken, broken, lychee_line, lychee_lines, lychee_root};

/// What the script is handed: the files in groups of the same rules, in the
/// folder of what the programs are handed (see [`Rules::job`]).
const JOB: &str = "markdownlint.json";
/// What markdownlint found, as the script writes it, in that folder.
const RESULTS: &str = "markdownlint-results.json";

/// What runs markdownlint on Node.js, through its library.
///
/// The library lints the files it is handed with the rules it is handed and
/// reads nothing else: no file of configuration, no module of rules, no
/// plugin. The tool reads the repository's configuration
/// ([`crate::linter_config::markdownlint`]), lays it over the pipeline's
/// default (`rules/markdownlint.jsonc`, compiled into this tool) rule by
/// rule, works out each file's rules from the differences by pattern, and
/// hands the files in groups of the same rules. Nothing in the configuration
/// takes a file out: the files are those `files` names.
///
/// It reads where the pinned package lies (`TOOLS`), what it is handed
/// (`JOB`) and where it writes what markdownlint found (`RESULTS`) from its
/// environment, and writes the results as markdownlint gives them, one object
/// per group, for the tool to read ([`Results`]). A `frontMatter` that is no
/// pattern JavaScript reads is not handed over: the script writes why beside
/// the results, and markdownlint lints with the front matter it knows by
/// default, so that one run still shows every finding.
pub const MARKDOWNLINT: &str = r#"
import fs from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const { TOOLS, JOB, RESULTS } = process.env;
const { lint } = await import(
  pathToFileURL(join(TOOLS, "node_modules", "markdownlint", "lib", "exports-promise.mjs")).href
);
const job = JSON.parse(fs.readFileSync(JOB, "utf8"));
let frontMatter;
let refused = null;
if (job.frontMatter !== null) {
  try {
    frontMatter = new RegExp(job.frontMatter, "u");
  } catch (error) {
    refused = error.message;
  }
}
const groups = [];
for (const { config, files } of job.groups) {
  groups.push(await lint({ files, config, frontMatter, noInlineConfig: job.noInlineConfig }));
}
fs.writeFileSync(RESULTS, JSON.stringify({ frontMatter: refused, groups }));
"#;

/// Runs the check; whether it found nothing.
pub(super) fn run(session: &mut Session<'_, '_, '_>) -> bool {
    // A missing program is named and fails the check, and the parts whose
    // programs were found still run, so that one run reports every finding.
    let prettier = session.tool("prettier", false);
    let linter = session.tool(markdownlint::NAME, true);
    let lychee = session.tool("lychee", false);
    let handed = links::hand_over(session);
    let (configured, whole) = configuration(session);
    let job = configured.job(
        &markdownlint::default_rules(rules::MARKDOWNLINT),
        &session.files,
    );
    let Some(temp) = session.folder(|_| {
        let mut entries: Vec<Entry> = vec![
            ("nothing-ignored".to_owned(), Some(Vec::new())),
            (JOB.to_owned(), Some(job.into_bytes())),
            (RESULTS.to_owned(), Some(Vec::new())),
        ];
        entries.extend(Handed::entries(handed.as_ref()));
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
    let rules_hold = linter
        .is_some_and(|linter| markdownlint_run(session, &linter, &temp, configured.file.as_ref()));
    let root = session.context.root;
    let links_lead = handed.is_some_and(|handed| {
        lychee.is_some_and(|lychee| links::check(session, &lychee, &handed, &temp, &[], root))
    });
    formatted && whole && rules_hold && links_lead
}

/// The repository's configuration of markdownlint, read the way the rules
/// report reads it, with the lines the report shows for it, and whether it
/// is handed over whole: every problem with it is a finding at its file, and
/// what it is about is not handed over.
fn configuration(session: &mut Session<'_, '_, '_>) -> (Rules, bool) {
    let packages = &session.context.packages;
    let files: Vec<&RelativePath> = packages.tracked.iter().collect();
    let kept: BTreeSet<&RelativePath> = packages.kept.iter().collect();
    let read = packages.repository.reader();
    let (handed, problems) = markdownlint::read_in(
        session.context.register,
        &files,
        &|file| kept.contains(file),
        &read,
    );
    for line in rules::markdownlint(rules::MARKDOWNLINT, &handed, &problems) {
        session.log.line(&line);
    }
    for problem in &problems {
        session
            .log
            .error(Some(&problem.file.to_string()), None, &problem.message);
    }
    (handed, problems.is_empty())
}

/// markdownlint over the files, with the rules [`configuration`] gives each:
/// see [`MARKDOWNLINT`]. What the script prints goes into the output as it
/// is, then every finding, as a line `<file>:<line>[:<column>] <severity>
/// <rule> <description>` the way markdownlint-cli2 prints it, and as a
/// finding on its line. A warning fails nothing, an error fails the check.
/// A `frontMatter` the script could not make a pattern of fails the check
/// at `file`, the repository's file it came from.
fn markdownlint_run(
    session: &mut Session<'_, '_, '_>,
    markdownlint: &Tool,
    temp: &Path,
    file: Option<&RelativePath>,
) -> bool {
    // The pinned copy, which `Session::tool` takes only with its runtime
    // when it is installed with npm, as the register has it.
    let Some(node) = &markdownlint.runtime else {
        session.log.error(
            None,
            None,
            "markdownlint runs on Node.js, and the register gives it no runtime to run on",
        );
        return false;
    };
    let mut command = session.command(node, &markdownlint.search_path);
    command
        .args(["--input-type=module", "--eval", MARKDOWNLINT])
        .env("TOOLS", &markdownlint.folder)
        .env("JOB", temp.join(JOB))
        .env("RESULTS", temp.join(RESULTS));
    let ran = session.capture(command, "markdownlint");
    session.log.lines(&ran.text);
    let results = fs::read_to_string(temp.join(RESULTS)).unwrap_or_default();
    let results: Results = match serde_json::from_str(&results) {
        Ok(results) => results,
        Err(error) => {
            session.log.error(
                None,
                None,
                &format!("what markdownlint found does not read: {error}"),
            );
            return false;
        }
    };
    let mut held = ran.success;
    if let Some(reason) = &results.front_matter {
        held = false;
        session.log.error(
            file.map(ToString::to_string).as_deref(),
            None,
            &format!(
                "`frontMatter` is not a pattern JavaScript reads, so it is not handed to \
                 markdownlint, which skips the front matter it knows by default: {reason}"
            ),
        );
    }
    for (file, found) in results.groups.iter().flatten() {
        for lint in found {
            let line = lint.line_number.to_string();
            let column = lint.column().map(|column| column.to_string());
            let message = lint.message();
            let severity = lint.severity.as_deref().unwrap_or("error");
            session.log.line(&format!(
                "{file}:{line}{} {severity} {message}",
                column
                    .as_deref()
                    .map(|column| format!(":{column}"))
                    .unwrap_or_default()
            ));
            let position = Some((line.as_str(), column.as_deref()));
            if severity == "warning" {
                session.log.warning(Some(file.as_str()), position, &message);
            } else {
                held = false;
                session.log.error(Some(file.as_str()), position, &message);
            }
        }
    }
    held
}

/// What the script writes: why `frontMatter` was not handed over, if it was
/// not, and what markdownlint found, one map from file to findings per group.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    /// Why JavaScript did not read `frontMatter` as a pattern, where it did
    /// not.
    pub front_matter: Option<String>,
    /// What markdownlint found in each group.
    pub groups: Vec<std::collections::BTreeMap<String, Vec<Lint>>>,
}

/// One finding of markdownlint, as its library gives it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lint {
    /// The line, counted from 1.
    pub line_number: u64,
    /// The rule's names, its number first.
    pub rule_names: Vec<String>,
    /// What the rule asks for.
    pub rule_description: String,
    /// What is wrong in particular, where the rule says.
    pub error_detail: Option<String>,
    /// The text it is about, where the rule says.
    pub error_context: Option<String>,
    /// Where in the line it starts and how long it is, where the rule says.
    pub error_range: Option<Vec<u64>>,
    /// `error` or `warning`, as the configuration gives the rule.
    pub severity: Option<String>,
}

impl Lint {
    /// The column it starts at, where the rule gives one.
    #[must_use]
    pub fn column(&self) -> Option<u64> {
        self.error_range
            .as_deref()
            .and_then(<[u64]>::first)
            .copied()
            .filter(|column| *column > 0)
    }

    /// What it says, the way markdownlint-cli2 writes it: the rule's names,
    /// what it asks for, and the detail and the context where they are given.
    #[must_use]
    pub fn message(&self) -> String {
        fn given(text: Option<&String>) -> Option<&str> {
            text.map(String::as_str).filter(|text| !text.is_empty())
        }
        let detail = given(self.error_detail.as_ref())
            .map(|detail| format!(" [{detail}]"))
            .unwrap_or_default();
        let context = given(self.error_context.as_ref())
            .map(|context| format!(" [Context: \"{context}\"]"))
            .unwrap_or_default();
        format!(
            "{} {}{detail}{context}",
            self.rule_names.join("/"),
            self.rule_description
        )
    }
}
