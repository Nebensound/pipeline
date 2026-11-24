//! The YAML check: every `.yml` and `.yaml` file parses and is formatted the
//! way Prettier writes it, every GitHub Actions workflow among them passes
//! actionlint, and the Dependabot configuration among them matches the
//! Dependabot schema.
//!
//! Whether a file parses, and whether the Dependabot configuration matches
//! its schema, is the tool's own judgement, with `yaml-rust2` and `jsonschema`
//! linked into it. Prettier and actionlint are the programs it starts.

use std::ffi::OsString;
use std::fmt::Write as _;

use jsonschema::paths::{Location, LocationSegment};
use serde::Deserialize;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::{Marker, ScanError};
use yaml_rust2::yaml::Hash;
use yaml_rust2::{Yaml, YamlLoader};

use super::{Finding, Session, Tool};
use crate::setup::find;

/// The schema the Dependabot configuration is held against: the
/// `dependabot-2.0.json` of `SchemaStore`, vendored beside the tool (see
/// `schemas/README.md`) and compiled in, so that the validation sends no
/// request.
pub const DEPENDABOT: &str = include_str!("../../schemas/dependabot-2.0.json");

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
/// The patterns go once actionlint knows the form and the key.
const ACTIONLINT_IGNORED: &[&str] = &[
    r#"^specifying action "\$/[^"]+" in invalid format because ref is missing"#,
    r#"^reusable workflow call "\$/[^"]+" at "uses" is not following the format"#,
    r#"^unexpected key "queue" for "concurrency" section\. "#,
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
    // A missing program is named and fails the check, and the parts that do
    // not need it still run, so that one run reports every finding.
    let prettier = session.tool("prettier", false);
    // actionlint only where there is a workflow: the outer [`None`] when
    // nothing needs it.
    let actionlint = (!workflows.is_empty()).then(|| session.tool("actionlint", false));
    let Some(temp) = session.folder(|_| vec![("nothing-ignored".to_owned(), Some(Vec::new()))])
    else {
        return false;
    };
    let all: Vec<&str> = files.iter().map(String::as_str).collect();
    // Each part runs even when one before it failed, so that one run reports
    // every finding at once.
    let formatted = prettier
        .is_some_and(|prettier| session.prettier(&prettier, &temp.join("nothing-ignored"), &all));
    let mut parsed = true;
    // The Dependabot configuration as read, once it parses: one that does not
    // is named already, and there is nothing to hold against the schema.
    let mut configuration = None;
    for file in &all {
        let Some(bytes) = session.contents(file) else {
            parsed = false;
            continue;
        };
        let found = findings(&bytes);
        for finding in &found {
            session.finding(file, finding);
        }
        if !found.is_empty() {
            parsed = false;
        } else if Some(*file) == dependabot {
            configuration = Some(bytes);
        }
    }
    session.log.line(&format!("{} file(s) parsed.", all.len()));
    let workflows_pass = actionlint.is_none_or(|actionlint| {
        actionlint.is_some_and(|actionlint| actionlint_run(session, &actionlint, &workflows))
    });
    let dependabot_passes = dependabot
        .zip(configuration)
        .is_none_or(|(dependabot, bytes)| schema(session, dependabot, &bytes));
    formatted && parsed && workflows_pass && dependabot_passes
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

/// What the parser finds wrong with a YAML file: every key that a mapping
/// holds a second time, and the first error that stops the parser, such as a
/// syntax error or an alias that names no anchor declared before it in its
/// document. Empty when the file is valid YAML. A file of several documents
/// is read whole.
#[must_use]
pub fn findings(bytes: &[u8]) -> Vec<Finding> {
    let text = match super::utf8(bytes, "YAML") {
        Ok(text) => text,
        Err(finding) => return vec![finding],
    };
    let mut keys = Keys::default();
    let parsed = Parser::new_from_str(text).load(&mut keys, true);
    let mut found = keys.found;
    if let Err(error) = parsed {
        found.push(broken(text, &error));
    }
    found
}

/// The finding of an error that stopped the parser in `text`, at its line
/// and column. An alias that names no anchor is named.
fn broken(text: &str, error: &ScanError) -> Finding {
    let mark = *error.marker();
    let message = if error.info() == "while parsing node, found unknown anchor" {
        // The parser counts its places in characters.
        let alias: String = text
            .chars()
            .skip(mark.index())
            .take_while(|character| !character.is_whitespace() && !",[]{}".contains(*character))
            .collect();
        format!(
            "not valid YAML: the alias \"{alias}\" names no anchor declared before it in its \
             document"
        )
    } else {
        format!("not valid YAML: {}", error.info())
    };
    Finding {
        position: Some(place(mark)),
        message,
    }
}

/// A place the parser marks, as line and column from 1: it counts the
/// columns from 0.
fn place(mark: Marker) -> (usize, usize) {
    (mark.line(), mark.col() + 1)
}

/// The key a mapping holds more than once, read off the parser's events.
///
/// Keys are compared by their text, so `1` and `"1"` are the same key, and
/// `true` and `True` are not. Only a key that is a scalar is compared. The
/// merge key `<<` may stand several times: each brings in another mapping.
#[derive(Default)]
struct Keys {
    /// The collections that are open, the innermost last.
    open: Vec<Collection>,
    found: Vec<Finding>,
}

/// An open collection.
enum Collection {
    /// A mapping: its keys so far, and whether its next node is a key.
    Mapping {
        keys: Vec<String>,
        key_next: bool,
    },
    Sequence,
}

impl Keys {
    /// Whether the node that starts now is a key of the open mapping.
    fn is_key(&mut self) -> bool {
        match self.open.last_mut() {
            Some(Collection::Mapping { key_next, .. }) => {
                let key = *key_next;
                *key_next = !key;
                key
            }
            Some(Collection::Sequence) | None => false,
        }
    }
}

impl MarkedEventReceiver for Keys {
    fn on_event(&mut self, event: Event, mark: Marker) {
        match event {
            Event::Scalar(text, ..) => {
                if self.is_key()
                    && text != "<<"
                    && let Some(Collection::Mapping { keys, .. }) = self.open.last_mut()
                {
                    if keys.contains(&text) {
                        self.found.push(Finding {
                            position: Some(place(mark)),
                            message: format!(
                                "the key \"{text}\" stands twice in this mapping: a mapping \
                                 holds each key once"
                            ),
                        });
                    } else {
                        keys.push(text);
                    }
                }
            }
            Event::Alias(_) => {
                self.is_key();
            }
            Event::MappingStart(..) => {
                self.is_key();
                self.open.push(Collection::Mapping {
                    keys: Vec::new(),
                    key_next: true,
                });
            }
            Event::SequenceStart(..) => {
                self.is_key();
                self.open.push(Collection::Sequence);
            }
            Event::MappingEnd | Event::SequenceEnd => {
                self.open.pop();
            }
            _ => {}
        }
    }
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

/// The Dependabot configuration against [`DEPENDABOT`]. Without this, a
/// mistake in the file shows only after the merge, when Dependabot itself
/// fails to read it: an unknown key, a missing one, a value of the wrong kind.
///
/// Each finding is one on the file, `<JSON path>: <message>`, the message
/// naming the key.
fn schema(session: &mut Session<'_, '_, '_>, dependabot: &str, bytes: &[u8]) -> bool {
    let found = schema_findings(DEPENDABOT, bytes);
    session.log.line(&format!(
        "{dependabot} against the Dependabot schema: {} finding(s).",
        found.len()
    ));
    for finding in &found {
        session.finding(dependabot, finding);
    }
    found.is_empty()
}

/// What a YAML file breaks of a JSON Schema: one finding for each error the
/// validator reports, at the path of the value it is about. The file's first
/// document is held against it, with every merge key `<<` applied, as YAML
/// 1.1 defines the key. A file whose loading fails is one finding at the
/// place where it does, such as a key that the file writes twice in two
/// ways YAML reads as one, `true` and `True`; and a schema that is no JSON
/// Schema the validator can use is one finding that says so.
#[must_use]
pub fn schema_findings(schema: &str, bytes: &[u8]) -> Vec<Finding> {
    let validator = serde_json::from_str(schema)
        .map_err(|error| format!("the schema does not read: {error}"))
        .and_then(|schema: serde_json::Value| {
            jsonschema::validator_for(&schema)
                .map_err(|error| format!("the schema cannot be used: {error}"))
        });
    let validator = match validator {
        Ok(validator) => validator,
        Err(message) => {
            return vec![Finding {
                position: None,
                message,
            }];
        }
    };
    let text = match super::utf8(bytes, "YAML") {
        Ok(text) => text,
        Err(finding) => return vec![finding],
    };
    let documents = match YamlLoader::load_from_str(text) {
        Ok(documents) => documents,
        Err(error) => return vec![broken(text, &error)],
    };
    let instance = documents
        .first()
        .map_or(serde_json::Value::Null, |document| {
            crate::rules::json(&merged(document))
        });
    validator
        .iter_errors(&instance)
        .map(|error| Finding {
            position: None,
            message: format!("{}: {error}", json_path(error.instance_path())),
        })
        .collect()
}

/// `yaml` with every merge key `<<` applied, as YAML 1.1 defines the key: the
/// loader resolves aliases but keeps `<<` as a key of its own, which the
/// schema would then name as an unknown property.
///
/// A `<<` whose value is a mapping, or a list of mappings, stands for those
/// keys of them that the mapping does not hold itself; of two mappings in the
/// list, the earlier one wins. The mapping's own keys stay first. A `<<`
/// whose value is anything else stays as it is.
fn merged(yaml: &Yaml) -> Yaml {
    match yaml {
        Yaml::Hash(hash) => {
            let mut own = Hash::new();
            let mut brought = Vec::new();
            for (key, value) in hash {
                let merge = matches!(key, Yaml::String(name) if name == "<<");
                match (merge, merged(value)) {
                    (true, Yaml::Hash(mapping)) => brought.push(mapping),
                    (true, Yaml::Array(items))
                        if items.iter().all(|item| matches!(item, Yaml::Hash(_))) =>
                    {
                        brought.extend(items.into_iter().filter_map(Yaml::into_hash));
                    }
                    (_, value) => {
                        own.insert(key.clone(), value);
                    }
                }
            }
            for (key, value) in brought.into_iter().flatten() {
                if !own.contains_key(&key) {
                    own.insert(key, value);
                }
            }
            Yaml::Hash(own)
        }
        Yaml::Array(items) => Yaml::Array(items.iter().map(merged).collect()),
        other => other.clone(),
    }
}

/// A place in a value as a JSON path, `$` for the value itself, `.name` for
/// a key and `[n]` for an item of a list.
#[must_use]
pub fn json_path(location: &Location) -> String {
    let mut path = "$".to_owned();
    for segment in location {
        match segment {
            LocationSegment::Property(name) => {
                let _ = write!(path, ".{name}");
            }
            LocationSegment::Index(index) => {
                let _ = write!(path, "[{index}]");
            }
        }
    }
    path
}
