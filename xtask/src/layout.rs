//! The register, the building blocks under `.github/actions/` and
//! `nebensound.yml` say the same thing.
//!
//! A block's directory is named after its register entry, with the category in
//! front: `check-<name>`, `version-<writer>`, `build-<name>`,
//! `publish-<target>`, the dots of a target turned into hyphens. So the
//! directory is the assignment, and no table has to be kept beside it.
//!
//! Every block belongs to a register entry, and every block of a check, a
//! version writer or a build is called from a step of `nebensound.yml` that is
//! named after the entry and switched on it, in exactly the form the workflow
//! documents for its kind. A check or a build the register knows and nobody
//! has written yet has no directory: a step `Not written yet` fails for it by
//! name, so the list of written checks in one such step and the list of
//! written builds in another name exactly the checks and builds that have a
//! block. A version writer is held the same way by the step
//! `Version writers not written yet`. Every block brings its test cases, at
//! least one passing and one failing. Beside the blocks lie only the
//! workflow's own machinery ([`MACHINERY`]), which belongs to no register
//! entry; it is called from `nebensound.yml` all the same.

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

/// The steps that fail for every check, and for every build, without a
/// block. Which of the two a step is says its list: [`WRITTEN_CHECKS`] or
/// [`WRITTEN_BUILDS`].
const NOT_WRITTEN: &str = "Not written yet";

/// The variable of a step [`NOT_WRITTEN`] that lists the written checks.
const WRITTEN_CHECKS: &str = "WRITTEN_CHECKS";

/// The variable of a step [`NOT_WRITTEN`] that lists the written builds.
const WRITTEN_BUILDS: &str = "WRITTEN_BUILDS";

/// The step that fails for every version writer without a block.
const WRITERS_NOT_WRITTEN: &str = "Version writers not written yet";

/// The keys of a step that calls a block, in the form `nebensound.yml`
/// documents for it. Any other key changes what the block does in the job:
/// `continue-on-error` keeps the job green whatever the block finds,
/// `timeout-minutes` cuts it short, `env` steers its tools.
const CALL_KEYS: &[&str] = &["if", "name", "uses", "with"];

/// The input a step hands a check or build block: the unit's own path, so
/// that every unit is checked and built where it lies.
const CALL_WITH: (&str, &str) = ("working-directory", "${{ matrix.path }}");

/// The plan of phase 0: the one input of a version writer's block, and the
/// one a step may hand a check block beside [`CALL_WITH`], for a check that
/// writes the versions it carries before it tests a package. It says what the
/// commit is, not where or how to check.
const CALL_PLAN: (&str, &str) = ("plan", "${{ needs.declare.outputs.plan }}");

/// The form of the step that calls a block of one kind.
#[derive(Debug)]
struct Form {
    /// The condition the step is switched on, with `{}` for the entry's name.
    switch: &'static str,
    /// What the step may hand the block under `with`, each choice in the order
    /// of its keys.
    with: &'static [&'static [(&'static str, &'static str)]],
}

/// A check runs after a check that failed before it, so that one run shows
/// every finding of the unit.
const CHECK: Form = Form {
    switch: "${{ !cancelled() && contains(matrix.checks, '{}') }}",
    with: &[&[CALL_WITH], &[CALL_PLAN, CALL_WITH]],
};

/// A version writer writes the versions of the plan before the unit's build.
const VERSION: Form = Form {
    switch: "${{ contains(matrix.writers, '{}') }}",
    with: &[&[CALL_PLAN]],
};

/// A build runs only when every step before it passed: nothing is built from
/// versions that were not written.
const BUILD: Form = Form {
    switch: "${{ matrix.build == '{}' }}",
    with: &[&[CALL_WITH]],
};

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
    /// The list of written builds, or why there is none.
    builds: Result<Vec<String>, String>,
}

/// The steps that call a block, and the lists of written checks, version
/// writers and builds.
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
    let mut builds = Vec::new();
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
            let env = &step["env"];
            let list = |yaml: &Yaml| yaml.as_str().unwrap_or_default().to_owned();
            match text("name").as_str() {
                NOT_WRITTEN => {
                    if !env[WRITTEN_CHECKS].is_badvalue() {
                        written.push(list(&env[WRITTEN_CHECKS]));
                    }
                    if !env[WRITTEN_BUILDS].is_badvalue() {
                        builds.push(list(&env[WRITTEN_BUILDS]));
                    }
                }
                WRITERS_NOT_WRITTEN => writers.push(list(&env["WRITTEN"])),
                _ => {}
            }
        }
    }
    Ok(Calls {
        steps: found,
        written: listed(&written, NOT_WRITTEN, WRITTEN_CHECKS, "checks"),
        writers: listed(&writers, WRITERS_NOT_WRITTEN, "WRITTEN", "version writers"),
        builds: listed(&builds, NOT_WRITTEN, WRITTEN_BUILDS, "builds"),
    })
}

/// The one list of written names that the steps named `step` hold in the
/// variable `variable`, or why there is none.
fn listed(lists: &[String], step: &str, variable: &str, what: &str) -> Result<Vec<String>, String> {
    match lists {
        [list] => serde_json::from_str::<Vec<String>>(list).map_err(|error| {
            format!("the list of written {what} in `{step}` is no JSON list of names: {error}")
        }),
        _ => Err(format!(
            "nebensound.yml has {} steps named `{step}` that list the written {what} in \
             `{variable}`, and needs exactly one",
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
        builds,
    } = calls(workflow)?;
    let written = names_of(written, &mut problems);
    let writers = names_of(writers, &mut problems);
    let builds = names_of(builds, &mut problems);
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
        let (form, list, step) = match entry.category {
            "check" => (&CHECK, &written, NOT_WRITTEN),
            "version" => (&VERSION, &writers, WRITERS_NOT_WRITTEN),
            "build" => (&BUILD, &builds, NOT_WRITTEN),
            category => {
                problems.push(format!(
                    "`{name}` is a block for the {category} `{}`, and the xtask knows only how \
                     a check, a version writer and a build are called and tested; it has to \
                     learn the other kind first",
                    entry.name
                ));
                continue;
            }
        };
        problems.extend(held(name, &entry.name, form, &called));
        if !list.contains(&entry.name) {
            problems.push(format!(
                "`{name}` exists, but `{step}` still fails for `{}`",
                entry.name
            ));
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
    for (list, prefix, step, what) in [
        (&written, "check", NOT_WRITTEN, ""),
        (&builds, "build", NOT_WRITTEN, "the build "),
        (&writers, "version", WRITERS_NOT_WRITTEN, ""),
    ] {
        problems.extend(
            list.iter()
                .filter(|name| !names.contains(format!("{prefix}-{name}").as_str()))
                .map(|name| {
                    format!("`{step}` counts {what}`{name}` as written, but it has no block")
                }),
        );
    }
    Ok(Layout { blocks, problems })
}

/// What is wrong with the steps that call the block `name` of the entry
/// `entry`: none at all, or one that is not in the form its kind documents.
fn held(name: &str, entry: &str, form: &Form, called: &[&Call]) -> Vec<String> {
    let mut problems = Vec::new();
    // The whole condition, as nebensound.yml documents it: a substring
    // would let `!contains(…)` or `contains(…) || true` through.
    let switch = form.switch.replace("{}", entry);
    if called.is_empty() {
        problems.push(format!("`{name}` is never called from nebensound.yml"));
    }
    if called
        .iter()
        .any(|call| call.name != entry || call.condition.trim() != switch)
    {
        problems.push(format!(
            "nebensound.yml calls `{name}` from a step that is not named `{entry}` and \
             switched on it with `{switch}`"
        ));
    }
    // The rest of the step as well: a block can be silenced through any
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
        keys != CALL_KEYS || !form.with.contains(&with.as_slice())
    }) {
        let pairs = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(key, value)| format!("`{key}: {value}`"))
                .collect::<Vec<_>>()
                .join(" and ")
        };
        let allowed: Vec<String> = form.with.iter().copied().map(pairs).collect();
        problems.push(format!(
            "nebensound.yml calls `{name}` from a step with other keys than `{}`, or that \
             hands it other than {}",
            CALL_KEYS.join("`, `"),
            allowed.join(", or ")
        ));
    }
    problems
}
