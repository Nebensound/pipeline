//! The YAML check: every `.yml` and `.yaml` file is formatted the way
//! Prettier writes it and passes yamllint, every GitHub Actions workflow
//! among them passes actionlint, and the Dependabot configuration among them
//! matches the Dependabot schema.

use std::ffi::OsString;
use std::path::Path;

use serde::Deserialize;

use super::{Entry, Session, Tool};
use crate::rules;
use crate::setup::find;

/// What runs yamllint's own linter, on the Python of its pinned copy.
///
/// The pipeline's default (`rules/yamllint.yaml`, compiled into this tool)
/// lies beneath the repository's own yamllint configuration at its root,
/// which is laid over it rule by rule, the way yamllint lays a configuration
/// over the one its `extends` names: a rule the repository does not name
/// keeps the default's value, and an option it does not name keeps the
/// default's option of that rule. So its file names only what differs, and a
/// change of the default reaches it. What its own `extends` names comes along
/// with it, over the default as well.
///
/// Its rules hold, but its `ignore` and `ignore-from-file` take nothing out:
/// yamllint would skip what they name even when the file is named to it, so
/// its own linter runs here with them set aside, file by file, the way its
/// command line does. It prints no workflow command: every finding is handed
/// to the tool as data, one JSON object per line of the file `FINDINGS` (see
/// [`FINDINGS`](super::FINDINGS)), and the tool writes it on its line, with
/// the message yamllint's `github` format gives it. A warning fails nothing,
/// an error fails the check, as on the command line.
///
/// It reads the default (`DEFAULT`), the repository's configuration if it has
/// one (`OWN`), the list of files, each ended by a zero byte (`LIST`), and
/// where its findings go (`FINDINGS`) from its environment.
pub const YAMLLINT: &str = r#"
import json
import locale
import os
import sys

from yamllint import linter
from yamllint.config import YamlLintConfig, YamlLintConfigError


class Layered(YamlLintConfig):
    """A configuration laid over a base the way yamllint lays one over
    the configuration its `extends` names: after its own `extends` is
    read, before the rules are checked."""

    def __init__(self, file, base):
        self.base = base
        super().__init__(file=file)

    def parse(self, raw_content):
        super().parse(raw_content)
        try:
            self.extend(self.base)
        except Exception as error:
            raise YamlLintConfigError(f"invalid config: {error}") from error


config = os.environ["DEFAULT"]
try:
    conf = YamlLintConfig(file=config)
    if os.environ["OWN"]:
        config = os.environ["OWN"]
        conf = Layered(config, conf)
except YamlLintConfigError as error:
    sys.exit(f"{config}: {error}")
# Both `ignore` and `ignore-from-file` end up here.
conf.ignore = None
if conf.locale is not None:
    locale.setlocale(locale.LC_ALL, conf.locale)
with open(os.environ["LIST"], "rb") as listing:
    files = [entry.decode() for entry in listing.read().split(b"\0") if entry]

# Every finding goes to the tool as data, with the message yamllint's own
# `github` format gives it; the tool writes it.
worst = 0
with open(os.environ["FINDINGS"], "a", encoding="utf-8") as found:
    for path in files:
        with open(path, "rb") as content:
            problems = list(linter.run(content, conf, path))
        for problem in problems:
            worst = max(worst, linter.PROBLEM_LEVELS[problem.level])
            rule = f"[{problem.rule}] " if problem.rule else ""
            finding = {
                "level": problem.level,
                "file": path,
                "line": problem.line,
                "column": problem.column,
                "message": f"{problem.line}:{problem.column} {rule}{problem.desc}",
            }
            found.write(json.dumps(finding) + "\n")
sys.exit(1 if worst == linter.PROBLEM_LEVELS["error"] else 0)
"#;

/// The names yamllint looks for its configuration under at the root, in its
/// order.
const OWN: &[&str] = &[".yamllint", ".yamllint.yaml", ".yamllint.yml"];

/// Messages of actionlint 1.7.12 that are left out.
///
/// The first two only for a `uses:` that begins with `$/`: that version does
/// not know the form yet and reports it as an invalid reference. `$/` is how
/// a repository reaches its own building blocks at the commit that is
/// running, so the finding would be wrong in every workflow that uses it.
/// Every other finding on the same line and every other malformed `uses:` is
/// still reported.
///
/// The third for exactly one key: that version does not know `queue` in a
/// `concurrency` section yet, which GitHub documents and the central workflow
/// uses, so that a waiting run is never replaced. Every other unknown key of
/// the section is still reported, and so is every other finding about it.
///
/// The fourth for exactly two properties of the context `job`: that version
/// does not know `job.workflow_sha` and `job.workflow_repository` yet, the
/// commit and the repository of the workflow file that defines the job, which
/// GitHub documents and the central workflow hands the action `tool`, so that
/// it finds the release of the pipeline's own commit. Every other unknown
/// property of `job` is still reported.
///
/// The patterns go once actionlint knows the form, the key and the properties.
const ACTIONLINT_IGNORED: &[&str] = &[
    r#"^specifying action "\$/[^"]+" in invalid format because ref is missing"#,
    r#"^reusable workflow call "\$/[^"]+" at "uses" is not following the format"#,
    r#"^unexpected key "queue" for "concurrency" section\. "#,
    r#"^property "workflow_(repository|sha)" is not defined in object type \{check_run_id: number; "#,
];

/// How actionlint writes its findings: as one JSON list per call, which
/// [`actionlint_findings`] reads.
const ACTIONLINT_FORMAT: &str = "{{json .}}";

/// A finding of actionlint, as its JSON format writes it.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct ActionlintFinding {
    /// What is wrong.
    pub message: String,
    /// The workflow, as it was handed to actionlint.
    pub filepath: String,
    /// The line, from 1.
    pub line: u64,
    /// The column, from 1.
    pub column: u64,
    /// The rule that found it, such as `syntax-check`.
    pub kind: String,
}

/// The findings in what actionlint printed in its JSON format: one list per
/// call, one after another. [`None`] when the text is not that, such as when
/// actionlint could not read a file and said so in words.
#[must_use]
pub fn actionlint_findings(text: &str) -> Option<Vec<ActionlintFinding>> {
    serde_json::Deserializer::from_str(text)
        .into_iter::<Vec<ActionlintFinding>>()
        .try_fold(Vec::new(), |mut all, list| {
            all.extend(list.ok()?);
            Some(all)
        })
}

/// Runs the check; whether it found nothing.
pub(super) fn run(session: &mut Session<'_, '_, '_>) -> bool {
    let files = session.files.clone();
    // GitHub runs a workflow from one place only: directly under
    // .github/workflows at the repository root. Dependabot, too, reads its
    // configuration from one place only: .github at the repository root.
    let workflows: Vec<&str> = files
        .iter()
        .map(String::as_str)
        .filter(|file| is_workflow(file))
        .collect();
    let dependabot = files
        .iter()
        .map(String::as_str)
        .rfind(|file| matches!(*file, ".github/dependabot.yml" | ".github/dependabot.yaml"));
    session
        .log
        .line(&format!("{} of them workflow(s).", workflows.len()));
    if let Some(dependabot) = dependabot {
        session
            .log
            .line(&format!("Dependabot configuration: {dependabot}"));
    }
    // A missing program is named and fails the check, and the parts whose
    // programs were found still run, so that one run reports every finding.
    let prettier = session.tool("prettier", false);
    let yamllint = session.tool("yamllint", true);
    // The other two only where there is something for them: the outer
    // [`None`] when nothing needs one.
    let actionlint = (!workflows.is_empty()).then(|| session.tool("actionlint", false));
    let jsonschema =
        dependabot.map(|dependabot| (dependabot, session.tool("check-jsonschema", false)));
    let mut list = Vec::new();
    for file in &files {
        list.extend_from_slice(file.as_bytes());
        list.push(0);
    }
    let Some(temp) = session.folder(|_| {
        let entries: Vec<Entry> = vec![
            ("nothing-ignored".to_owned(), Some(Vec::new())),
            (super::FINDINGS.to_owned(), Some(Vec::new())),
            ("files".to_owned(), Some(list)),
            (
                "yamllint.yaml".to_owned(),
                Some(rules::YAMLLINT.as_bytes().to_vec()),
            ),
        ];
        entries
    }) else {
        return false;
    };
    let all: Vec<&str> = files.iter().map(String::as_str).collect();
    // Each part runs even when one before it failed, so that one run reports
    // every finding at once.
    let formatted = prettier
        .is_some_and(|prettier| session.prettier(&prettier, &temp.join("nothing-ignored"), &all));
    let linted = yamllint.is_some_and(|yamllint| yamllint_run(session, &yamllint, &temp));
    let workflows_pass = actionlint.is_none_or(|actionlint| {
        actionlint.is_some_and(|actionlint| actionlint_run(session, &actionlint, &workflows))
    });
    let dependabot_passes = jsonschema.is_none_or(|(dependabot, jsonschema)| {
        jsonschema.is_some_and(|jsonschema| schema(session, &jsonschema, dependabot))
    });
    formatted && linted && workflows_pass && dependabot_passes
}

/// Whether a file is a workflow: directly under `.github/workflows`, with the
/// extension `.yml` or `.yaml`.
#[must_use]
pub fn is_workflow(file: &str) -> bool {
    file.strip_prefix(".github/workflows/")
        .filter(|name| !name.contains('/'))
        .and_then(|name| {
            name.strip_suffix(".yml")
                .or_else(|| name.strip_suffix(".yaml"))
        })
        .is_some_and(|stem| !stem.is_empty())
}

/// yamllint over every file, with the repository's rules laid over the
/// pipeline's default: see [`YAMLLINT`].
fn yamllint_run(session: &mut Session<'_, '_, '_>, yamllint: &Tool, temp: &Path) -> bool {
    let own = OWN.iter().copied().find(|name| session.is_file(name));
    session.log.line(&match own {
        Some(own) => format!("Rules: the pipeline default, and laid over it {own}"),
        None => "Rules: the pipeline default".to_owned(),
    });
    // The pinned copy, whose virtual environment holds the linter.
    let python = find::environment_python(&yamllint.folder, session.context.env.windows);
    let mut command = session.command(&python, &yamllint.search_path);
    command
        .args(["-c", YAMLLINT])
        .env("DEFAULT", temp.join("yamllint.yaml"))
        .env("OWN", own.unwrap_or_default())
        .env("LIST", temp.join("files"))
        .env("FINDINGS", temp.join(super::FINDINGS));
    let ran = session.capture(command, "yamllint");
    session.log.lines(&ran.text);
    session.handed(&temp.join(super::FINDINGS), "yamllint") && ran.success
}

/// actionlint over the workflows: the domain validator for GitHub Actions.
/// With `shellcheck` on the search path it also reads every `run:` script,
/// and with `pyflakes` every Python one; whether they were found is said, since a
/// computer without them reads fewer scripts.
fn actionlint_run(
    session: &mut Session<'_, '_, '_>,
    actionlint: &Tool,
    workflows: &[&str],
) -> bool {
    let windows = session.context.env.windows;
    for (program, name, what) in [
        ("shellcheck", "ShellCheck", "`run:` script"),
        ("pyflakes", "pyflakes", "Python `run:` script"),
    ] {
        session.log.line(
            &match find::search(program, &actionlint.search_path, windows) {
                Some(path) => format!(
                    "{name}: {}, so actionlint reads every {what} with it",
                    path.display()
                ),
                None => format!(
                    "{name} is not on the search path, so actionlint reads no {what} with it"
                ),
            },
        );
    }
    let mut args: Vec<OsString> = Vec::new();
    for pattern in ACTIONLINT_IGNORED {
        args.push("-ignore".into());
        args.push((*pattern).into());
    }
    args.push("-format".into());
    args.push(ACTIONLINT_FORMAT.into());
    let ran = session.over(actionlint, &args, workflows, "actionlint");
    // Each finding becomes an annotation written through [`super::annotation`],
    // so that a file name holding `,` or `:` and a message holding a line
    // break reach the runner whole. What is not in the JSON format stays in
    // the output as it is.
    match actionlint_findings(&ran.text) {
        Some(findings) => {
            for finding in findings {
                let line = finding.line.to_string();
                let column = finding.column.to_string();
                session.log.error(
                    Some(&finding.filepath),
                    Some((&line, Some(&column))),
                    &format!("{} [{}]", finding.message, finding.kind),
                );
            }
        }
        None => session.log.lines(&ran.text),
    }
    ran.success
}

/// The Dependabot configuration against the schema check-jsonschema brings
/// along, so that the validation sends no request. Without it, a mistake in
/// the file shows only after the merge, when Dependabot itself fails to read
/// it: an unknown key, a missing one, a value of the wrong kind.
///
/// A finding reads `<file>::<JSON path>: <message>`, the message naming the
/// key; each becomes one on the file, and stays in the output as it is.
fn schema(session: &mut Session<'_, '_, '_>, jsonschema: &Tool, dependabot: &str) -> bool {
    let args: Vec<OsString> = vec![
        "--builtin-schema".into(),
        "vendor.dependabot".into(),
        "--color".into(),
        "never".into(),
    ];
    let ran = session.over(jsonschema, &args, &[dependabot], "check-jsonschema");
    for line in ran.text.lines() {
        session.log.line(line);
        if let Some((path, message)) = schema_finding(line, dependabot) {
            session
                .log
                .error(Some(dependabot), None, &format!("{path}: {message}"));
        }
    }
    ran.success
}

/// A finding of check-jsonschema on `file`, `  <file>::<JSON path>:
/// <message>`, taken apart into the path and the message.
#[must_use]
pub fn schema_finding<'l>(line: &'l str, file: &str) -> Option<(&'l str, &'l str)> {
    let rest = line.trim_start_matches(' ');
    if rest.len() == line.len() {
        return None;
    }
    let rest = rest.strip_prefix(file)?.strip_prefix("::")?;
    let (path, message) = rest.split_once(':')?;
    let message = message.strip_prefix(' ')?;
    (path.starts_with('$') && !message.is_empty()).then_some((path, message))
}
