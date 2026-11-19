//! The register, the building blocks under `.github/actions/` and
//! `nebensound.yml` say the same thing.
//!
//! A block's directory is named after its register entry, with the category in
//! front: `check-<name>`, `version-<writer>`, `build-<name>`,
//! `publish-<target>`, the dots of a target turned into hyphens. So the
//! directory is the assignment, and no table has to be kept beside it.
//!
//! Every block belongs to a register entry and is called from a step of
//! `nebensound.yml` that is named after the entry and switched on it, with
//! exactly the condition the workflow documents for it. A check
//! the register knows and nobody has written yet has no directory: the step
//! `Not written yet` fails for it by name, so that step's list of written
//! checks names exactly the checks that have a block. Every block brings its
//! test cases, at least one passing and one failing. Beside the blocks lie
//! only the workflow's own machinery ([`MACHINERY`]), which belongs to no
//! register entry; it is called from `nebensound.yml` all the same.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use yaml_rust2::YamlLoader;

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
}

/// What `nebensound.yml` says about the blocks.
#[derive(Debug)]
struct Calls {
    /// The steps that call a block.
    calls: Vec<Call>,
    /// The list of written checks, or why there is none.
    written: Result<Vec<String>, String>,
}

/// The steps that call a block, and the list of written checks.
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
    for job in jobs.values() {
        for step in job["steps"].as_vec().into_iter().flatten() {
            let text = |key: &str| step[key].as_str().unwrap_or_default().to_owned();
            if let Some(block) = text("uses").strip_prefix("$/.github/actions/") {
                found.push(Call {
                    block: block.to_owned(),
                    name: text("name"),
                    condition: text("if"),
                });
            }
            if text("name") == NOT_WRITTEN {
                written.push(
                    step["env"]["WRITTEN"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                );
            }
        }
    }
    let written = match written.as_slice() {
        [list] => serde_json::from_str::<Vec<String>>(list).map_err(|error| {
            format!(
                "the list of written checks in `{NOT_WRITTEN}` is no JSON list of names: {error}"
            )
        }),
        _ => Err(format!(
            "nebensound.yml has {} steps named `{NOT_WRITTEN}`, and needs exactly one",
            written.len()
        )),
    };
    Ok(Calls {
        calls: found,
        written,
    })
}

/// Compares the three.
///
/// # Errors
///
/// When the register is no TOML, or `nebensound.yml` no workflow.
pub fn compare(register: &str, workflow: &str, directories: &[Directory]) -> Result<Layout, Error> {
    let (entries, mut problems) = entries(register)?;
    let Calls { calls, written } = calls(workflow)?;
    let written: BTreeSet<String> = match written {
        Ok(list) => list.into_iter().collect(),
        Err(problem) => {
            problems.push(problem);
            BTreeSet::new()
        }
    };
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
        if entry.category != "check" {
            problems.push(format!(
                "`{name}` is a block for the {} `{}`, and the xtask knows only how a check is \
                 called and tested; it has to learn the other kind first",
                entry.category, entry.name
            ));
            continue;
        }
        // The whole condition, as nebensound.yml documents it: a substring
        // would let `!contains(…)` or `contains(…) || true` through.
        let switch = format!(
            "${{{{ !cancelled() && contains(matrix.checks, '{}') }}}}",
            entry.name
        );
        if called.is_empty() {
            problems.push(format!("`{name}` is never called from nebensound.yml"));
        }
        if called
            .iter()
            .any(|call| call.name != entry.name || call.condition.trim() != switch)
        {
            problems.push(format!(
                "nebensound.yml calls `{name}` from a step that is not named `{}` and switched \
                 on it with `{switch}`",
                entry.name
            ));
        }
        if !written.contains(&entry.name) {
            problems.push(format!(
                "`{name}` exists, but `{NOT_WRITTEN}` still fails for `{}`",
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
    for check in &written {
        if !names.contains(format!("check-{check}").as_str()) {
            problems.push(format!(
                "`{NOT_WRITTEN}` counts `{check}` as written, but it has no block"
            ));
        }
    }
    Ok(Layout { blocks, problems })
}
