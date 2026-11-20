//! The register, the building blocks under `.github/actions/` and
//! `nebensound.yml` say the same thing.
//!
//! A block's directory is named after its register entry, with the category in
//! front: `check-<name>`, `version-<writer>`, `build-<name>`,
//! `publish-<target>`, the dots of a target turned into hyphens. So the
//! directory is the assignment, and no table has to be kept beside it.
//!
//! Every block belongs to a register entry. A check block is called from a
//! step of `nebensound.yml` that is named after the entry and switched on it,
//! in exactly the form the workflow documents for it. A check the register
//! knows and nobody has written yet has no directory: the step
//! `Not written yet` fails for it by name, so that step's list of written
//! checks names exactly the checks that have a block. A version writer is held
//! the same way by the step `Version writers not written yet`; no step calls
//! its block yet, since the Build phase that runs it before building is still
//! to come. Every block brings its test cases, at least one passing and one
//! failing. Beside the blocks lie only the workflow's own machinery
//! ([`MACHINERY`]), which belongs to no register entry; it is called from
//! `nebensound.yml` all the same.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use yaml_rust2::{Yaml, YamlLoader};

use crate::Error;
use crate::cases::{self, ACTIONS, Case, Expect};

/// The directories under `.github/actions/` that are no building block: the
/// workflow's own machinery.
pub const MACHINERY: &[&str] = &["phase0", "tool"];

/// The register, from the repository root.
pub const REGISTER: &str = "tools/nebensound/crates/nebensound-declaration/register.toml";

/// The central workflow, from the repository root.
pub const WORKFLOW: &str = ".github/workflows/nebensound.yml";

/// The step that fails for every check without a block.
const NOT_WRITTEN: &str = "Not written yet";

/// The step that fails for every version writer without a block.
const WRITERS_NOT_WRITTEN: &str = "Version writers not written yet";

/// The keys of a step that calls a check block, in the form `nebensound.yml`
/// documents for it. Any other key changes what the check does in the job:
/// `continue-on-error` keeps the job green whatever the block finds,
/// `timeout-minutes` cuts it short, `env` steers its tools.
const CALL_KEYS: &[&str] = &["if", "name", "uses", "with"];

/// The input a step hands a check block: the unit's own path, so that every
/// unit is checked where it lies.
const CALL_WITH: (&str, &str) = ("working-directory", "${{ matrix.path }}");

/// The one input a step may hand a check block beside [`CALL_WITH`]: the plan
/// of phase 0, for a check that writes the versions it carries before it
/// tests a package. It says what the commit is, not where or how to check.
const CALL_PLAN: (&str, &str) = ("plan", "${{ needs.declare.outputs.plan }}");

/// What the comparison found.
#[derive(Debug)]
pub struct Layout {
    /// The building blocks, by directory name.
    pub blocks: Vec<String>,
    /// Every disagreement, one sentence each.
    pub problems: Vec<String>,
}

/// One directory under `.github/actions/`.
#[derive(Debug)]
pub struct Directory {
    /// Its name.
    pub name: String,
    /// Whether it holds an `action.yml`.
    pub action: bool,
    /// Its test cases.
    pub cases: Result<Vec<Case>, Error>,
}

/// Compares the register, the blocks and `nebensound.yml` of the repository at
/// `root`.
///
/// # Errors
///
/// When one of them cannot be read at all.
pub fn check(root: &Path) -> Result<Layout, Error> {
    let register = read(&root.join(REGISTER))?;
    let workflow = read(&root.join(WORKFLOW))?;
    let directories = directories(&root.join(ACTIONS))?;
    compare(&register, &workflow, &directories)
}

fn read(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))
}

/// Every directory under `dir`, in the order of their names. A directory
/// without an `action.yml` is listed as well, so that a misspelt file cannot
/// hide a block from the comparison.
///
/// # Errors
///
/// When `dir` cannot be listed.
pub fn directories(dir: &Path) -> Result<Vec<Directory>, Error> {
    let entries = fs::read_dir(dir)
        .map_err(|error| Error::new(format!("cannot list {}: {error}", dir.display())))?;
    let mut found: Vec<Directory> = entries
        .map_while(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .map(|path| Directory {
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            action: path.join("action.yml").is_file(),
            cases: cases::discover(&path),
        })
        .collect();
    found.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(found)
}

/// A register entry a block can belong to.
#[derive(Debug)]
struct Entry {
    category: &'static str,
    name: String,
}

/// Every block name the register allows, with its entry, and the entries
/// whose block names fall together.
fn entries(register: &str) -> Result<(BTreeMap<String, Entry>, Vec<String>), Error> {
    let table: toml::Table = register
        .parse()
        .map_err(|error| Error::new(format!("the register is no TOML: {error}")))?;
    let mut blocks: BTreeMap<String, Entry> = BTreeMap::new();
    let mut clashes = Vec::new();
    for (category, prefix) in [
        ("check", "check"),
        ("version", "version"),
        ("build", "build"),
        ("target", "publish"),
    ] {
        let names: Vec<String> = table
            .get(category)
            .and_then(toml::Value::as_table)
            .map(|names| names.keys().cloned().collect())
            .unwrap_or_default();
        for name in names {
            let block = format!("{prefix}-{}", name.replace('.', "-"));
            let entry = Entry { category, name };
            if let Some(earlier) = blocks.get(&block) {
                clashes.push(format!(
                    "the {} `{}` and the {} `{}` would both be the block `{block}`",
                    earlier.category, earlier.name, entry.category, entry.name
                ));
            } else {
                blocks.insert(block, entry);
            }
        }
    }
    Ok((blocks, clashes))
}

/// A step of `nebensound.yml` that calls a block.
#[derive(Debug)]
struct Call {
    block: String,
    name: String,
    condition: String,
    /// Every key of the step, in order; a key that is no text reads empty.
    keys: Vec<String>,
    /// What the step hands the block under `with`; a key or value that is no
    /// text reads empty.
    with: Vec<(String, String)>,
}

/// What `nebensound.yml` says about the blocks.
#[derive(Debug)]
struct Calls {
    /// The steps that call a block.
    steps: Vec<Call>,
    /// The list of written checks, or why there is none.
    written: Result<Vec<String>, String>,
    /// The list of written version writers, or why there is none.
    writers: Result<Vec<String>, String>,
}

/// The steps that call a block, and the lists of written checks and version
/// writers.
fn calls(workflow: &str) -> Result<Calls, Error> {
    let documents = YamlLoader::load_from_str(workflow)
        .map_err(|error| Error::new(format!("nebensound.yml is no YAML: {error}")))?;
    let Some(jobs) = documents
        .first()
        .and_then(|document| document["jobs"].as_hash())
    else {
        return Err(Error::new("nebensound.yml has no jobs".to_owned()));
    };
    let mut found = Vec::new();
    let mut written = Vec::new();
    let mut writers = Vec::new();
    for job in jobs.values() {
        for step in job["steps"].as_vec().into_iter().flatten() {
            let text = |key: &str| step[key].as_str().unwrap_or_default().to_owned();
            if let Some(block) = text("uses").strip_prefix("$/.github/actions/") {
                let string = |yaml: &Yaml| yaml.as_str().unwrap_or_default().to_owned();
                found.push(Call {
                    block: block.to_owned(),
                    name: text("name"),
                    condition: text("if"),
                    keys: step
                        .as_hash()
                        .into_iter()
                        .flat_map(|hash| hash.keys().map(string))
                        .collect(),
                    with: step["with"]
                        .as_hash()
                        .into_iter()
                        .flat_map(|hash| {
                            hash.iter().map(|(key, value)| (string(key), string(value)))
                        })
                        .collect(),
                });
            }
            let list = || {
                step["env"]["WRITTEN"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            };
            match text("name").as_str() {
                NOT_WRITTEN => written.push(list()),
                WRITERS_NOT_WRITTEN => writers.push(list()),
                _ => {}
            }
        }
    }
    Ok(Calls {
        steps: found,
        written: listed(&written, NOT_WRITTEN, "checks"),
        writers: listed(&writers, WRITERS_NOT_WRITTEN, "version writers"),
    })
}

/// The one list of written names the steps named `step` hold, or why there
/// is none.
fn listed(lists: &[String], step: &str, what: &str) -> Result<Vec<String>, String> {
    match lists {
        [list] => serde_json::from_str::<Vec<String>>(list).map_err(|error| {
            format!("the list of written {what} in `{step}` is no JSON list of names: {error}")
        }),
        _ => Err(format!(
            "nebensound.yml has {} steps named `{step}`, and needs exactly one",
            lists.len()
        )),
    }
}

/// The names a list holds, or none, with why it is missing among `problems`.
fn names_of(list: Result<Vec<String>, String>, problems: &mut Vec<String>) -> BTreeSet<String> {
    match list {
        Ok(list) => list.into_iter().collect(),
        Err(problem) => {
            problems.push(problem);
            BTreeSet::new()
        }
    }
}

/// Compares the three.
///
/// # Errors
///
/// When the register is no TOML, or `nebensound.yml` no workflow.
pub fn compare(register: &str, workflow: &str, directories: &[Directory]) -> Result<Layout, Error> {
    let (entries, mut problems) = entries(register)?;
    let Calls {
        steps: calls,
        written,
        writers,
    } = calls(workflow)?;
    let written = names_of(written, &mut problems);
    let writers = names_of(writers, &mut problems);
    let names: BTreeSet<&str> = directories.iter().map(|dir| dir.name.as_str()).collect();
    let mut blocks = Vec::new();

    for dir in directories {
        let name = dir.name.as_str();
        if !dir.action {
            problems.push(format!("`{name}` has no action.yml"));
        }
        let called: Vec<&Call> = calls.iter().filter(|call| call.block == name).collect();
        if MACHINERY.contains(&name) {
            if called.is_empty() {
                problems.push(format!("`{name}` is never called from nebensound.yml"));
            }
            continue;
        }
        let Some(entry) = entries.get(name) else {
            problems.push(format!(
                "`{name}` belongs to no register entry, and it is no machinery of the workflow \
                 ({})",
                MACHINERY.join(", ")
            ));
            continue;
        };
        blocks.push(name.to_owned());
        match entry.category {
            "check" => {
                problems.extend(check_calls(name, &entry.name, &called));
                if !written.contains(&entry.name) {
                    problems.push(format!(
                        "`{name}` exists, but `{NOT_WRITTEN}` still fails for `{}`",
                        entry.name
                    ));
                }
            }
            "version" => {
                if !writers.contains(&entry.name) {
                    problems.push(format!(
                        "`{name}` exists, but `{WRITERS_NOT_WRITTEN}` still fails for `{}`",
                        entry.name
                    ));
                }
            }
            category => {
                problems.push(format!(
                    "`{name}` is a block for the {category} `{}`, and the xtask knows only how \
                     a check and a version writer are called and tested; it has to learn the \
                     other kind first",
                    entry.name
                ));
                continue;
            }
        }
        match &dir.cases {
            Err(error) => problems.push(error.to_string()),
            Ok(cases) => {
                for (kind, failing) in [("passing", false), ("failing", true)] {
                    let any = cases
                        .iter()
                        .any(|case| matches!(case.expect, Expect::Fail(_)) == failing);
                    if !any {
                        let folder = if failing { "fail" } else { "pass" };
                        problems.push(format!(
                            "`{name}` has no {kind} test case under tests/{folder}/"
                        ));
                    }
                }
            }
        }
    }
    for call in &calls {
        if !names.contains(call.block.as_str()) {
            problems.push(format!(
                "nebensound.yml calls `{}`, which does not exist",
                call.block
            ));
        }
    }
    for check in &written {
        if !names.contains(format!("check-{check}").as_str()) {
            problems.push(format!(
                "`{NOT_WRITTEN}` counts `{check}` as written, but it has no block"
            ));
        }
    }
    for writer in &writers {
        if !names.contains(format!("version-{writer}").as_str()) {
            problems.push(format!(
                "`{WRITERS_NOT_WRITTEN}` counts `{writer}` as written, but it has no block"
            ));
        }
    }
    Ok(Layout { blocks, problems })
}

/// What is wrong with the steps that call the block `name` of the check
/// `check`: none at all, or one that is not in the form `nebensound.yml`
/// documents.
fn check_calls(name: &str, check: &str, called: &[&Call]) -> Vec<String> {
    let mut problems = Vec::new();
    // The whole condition, as nebensound.yml documents it: a substring
    // would let `!contains(…)` or `contains(…) || true` through.
    let switch = format!("${{{{ !cancelled() && contains(matrix.checks, '{check}') }}}}");
    if called.is_empty() {
        problems.push(format!("`{name}` is never called from nebensound.yml"));
    }
    if called
        .iter()
        .any(|call| call.name != check || call.condition.trim() != switch)
    {
        problems.push(format!(
            "nebensound.yml calls `{name}` from a step that is not named `{check}` and \
             switched on it with `{switch}`"
        ));
    }
    // The rest of the step as well: a check can be silenced through any
    // other key just as through its switch, and pointed at the root
    // instead of the unit through its input.
    if called.iter().any(|call| {
        let mut keys: Vec<&str> = call.keys.iter().map(String::as_str).collect();
        keys.sort_unstable();
        let mut with: Vec<(&str, &str)> = call
            .with
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        with.sort_unstable();
        keys != CALL_KEYS || (with != [CALL_WITH] && with != [CALL_PLAN, CALL_WITH])
    }) {
        problems.push(format!(
            "nebensound.yml calls `{name}` from a step with other keys than `{}`, or that \
             hands it other than `{}: {}`, and `{}: {}` beside it at most",
            CALL_KEYS.join("`, `"),
            CALL_WITH.0,
            CALL_WITH.1,
            CALL_PLAN.0,
            CALL_PLAN.1
        ));
    }
    problems
}
