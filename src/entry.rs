//! The entries of the check matrix of a run, and what `nebensound-repo job`
//! runs for one of them.
//!
//! The preparing job hands the jobs of the checks one matrix. Each entry is
//! one job, and each job is the same call of the tool with its entry
//! ([`Entry::to_json`]): the tool works out everything else from the
//! repository at the commit of the run, and nothing but passed or failed
//! crosses from one job to another. The entries are:
//!
//! - **One for all reading checks** of all units ([`Work::Reading`]): the
//!   checks whose register entry names the side `reading`, which run only
//!   programs of the pipeline on the repository's files. It also finds what
//!   the run asks for and nobody has written yet: the checks, the builds, the
//!   version writers and the publish targets. It always stands, since that is
//!   found nowhere else, and the base set is in force in every repository.
//! - **One per executing check and unit** ([`Work::Check`]): a check whose
//!   register entry names the side `executing`, which runs code of the
//!   repository, in each unit the plan starts it at.
//! - **One for the repository's own check** ([`Work::Xtask`]), where
//!   `xtask/Cargo.toml` exists: `cargo xtask check --all` is code of the
//!   repository.
//!
//! Each entry carries, as data, the name of its job, its runner, how much of
//! the history its checkout needs (`fetch-depth`: the whole for a check that
//! writes the versions before it tests a package, the commit at hand for every
//! other) and, for an entry that starts Cargo, its cache ([`Cache`]).

use std::fmt::Write as _;
use std::path::Path;

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::finding::UnitName;
use crate::names::CheckName;
use crate::path::RelativePath;
use crate::plan::Plan;
use crate::register::{Register, Side};
use crate::resolve::Unit;
use crate::workflow::{Log, Variables};
use crate::{build, release, run, writer};

/// The runner every entry runs on: the one every other job of the workflow
/// runs on.
pub const RUNNER: &str = crate::declare::RUNNER;

/// The name of the job of the reading entry, as it shows in a run.
pub const READING_JOB: &str = "Check files";

/// The name of the job of the repository's own check.
pub const XTASK_JOB: &str = "Check xtask";

/// The manifest that says a repository has an xtask.
pub const XTASK: &str = "xtask/Cargo.toml";

/// What an entry runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Work {
    /// Every reading check of every unit, and what is not written yet.
    Reading,
    /// One executing check in one unit.
    Check {
        /// The check, as the tool runs it.
        check: run::Check,
        /// The check, as the register names it.
        name: CheckName,
        /// The unit, by its path.
        unit: RelativePath,
    },
    /// The repository's own check, `cargo xtask check --all`.
    Xtask,
}

/// One entry of the check matrix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What it runs.
    pub work: Work,
    /// The name of its job.
    pub job: String,
    /// How many commits its checkout holds: `0` for the whole history, which
    /// a check that writes the versions resolves them from, `1` for the
    /// commit at hand.
    pub fetch_depth: u8,
    /// The Cargo cache of an entry that starts Cargo.
    pub cache: Option<Cache>,
}

/// The Cargo cache of an entry: what a generic cache step restores before
/// the entry runs and saves after it.
///
/// The key holds the entry and its runner, and a digest of every file that
/// decides what Cargo builds of the dependencies and with which toolchain:
/// each `Cargo.toml`, `Cargo.lock`, `rust-toolchain` and
/// `rust-toolchain.toml` Git tracks, and each `.cargo/config` and
/// `.cargo/config.toml`, wherever they stand, and for a check the register's
/// entry of it, its programs with their pinned versions and its coverage
/// toolchain. It leaves out the exact build of the toolchain a channel such
/// as `stable` resolves to on the day, the runner's image and the sources of
/// the workspace: none of them decides whether an artifact may be used.
/// Cargo does, by the fingerprint it keeps beside every artifact, which holds
/// the compiler's version, the profile, the features, the flags and the
/// sources the artifact was built from, and it builds anew whatever does not
/// match. So a cache restored under an older key, or one that holds what
/// another commit built, changes how long the entry takes, never what it
/// finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cache {
    /// The key it is saved under.
    pub key: String,
    /// The start of the keys an older cache of the same entry is restored
    /// from when none is saved under the key.
    pub restore: String,
    /// What it holds: what Cargo downloaded, and the folder it builds into.
    pub paths: Vec<String>,
}

/// The file names whose contents go into the key of a Cargo cache, wherever
/// they stand.
const KEYED: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
];

/// The configuration files of Cargo whose contents go into the key, in a
/// folder `.cargo` wherever it stands.
const KEYED_CONFIG: &[&str] = &["config", "config.toml"];

/// The version of the form of the key: raised when what it covers changes,
/// so that no cache saved under the old form is taken for the new.
const KEY_FORM: &str = "v1";

/// What Cargo downloads, in its home on the runner: the index and the
/// archives of the registry and the clones of Git dependencies. The sources
/// Cargo unpacks from the archives are left out, since unpacking them is
/// quicker than restoring them, and so are the programs in `bin`, which
/// nothing here installs and nothing restored is to put on the search path.
const DOWNLOADS: &[&str] = &[
    "~/.cargo/registry/index/",
    "~/.cargo/registry/cache/",
    "~/.cargo/git/db/",
];

/// Which side a check of the plan is on: run in the reading entry, run in an
/// entry of its own, or not written yet. A check the tool runs and whose
/// register entry names its side is on that side; any other is not written,
/// and a test of the register holds that every check the tool runs names
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placed {
    /// It reads, and runs in the reading entry.
    Reading(run::Check),
    /// It executes, and runs in an entry of its own per unit.
    Executing(run::Check),
    /// Nobody has written it yet; the reading entry fails by name for it.
    Unwritten,
}

/// Where a check of this name goes.
#[must_use]
pub fn placed(register: &Register, name: &str) -> Placed {
    let side = register
        .check
        .iter()
        .find(|(check, _)| check.as_str() == name)
        .and_then(|(_, check)| check.kind);
    match (run::Check::of(name), side) {
        (Some(check), Some(Side::Reading)) => Placed::Reading(check),
        (Some(check), Some(Side::Executing)) => Placed::Executing(check),
        _ => Placed::Unwritten,
    }
}

/// How a unit is named in the name of a job and in the table of the reading
/// entry: `repository`, or `component <name>`.
#[must_use]
pub fn label(name: &UnitName) -> String {
    match name {
        UnitName::Repository => "repository".to_owned(),
        UnitName::Component(name) => format!("component {name}"),
    }
}

/// The entries of the plan, in this order: the reading entry, one entry per
/// executing check and unit, units in the order of the plan and checks in
/// name order, and the repository's own check where `tracked` holds
/// `xtask/Cargo.toml`. The cache keys read the files `tracked` names below
/// `root`.
#[must_use]
pub fn entries(
    plan: &Plan,
    register: &Register,
    tracked: &[RelativePath],
    root: &Path,
) -> Vec<Entry> {
    let mut entries = vec![Entry {
        work: Work::Reading,
        job: READING_JOB.to_owned(),
        fetch_depth: 1,
        cache: None,
    }];
    for unit in &plan.units {
        for name in &unit.checks {
            let Placed::Executing(check) = placed(register, name.as_str()) else {
                continue;
            };
            // The Rust check is the one executing check that starts Cargo.
            let cache = (check == run::Check::Rust).then(|| {
                let identity = format!("{}-{}", check.name(), short(&unit.path.to_string()));
                let mut paths: Vec<String> =
                    DOWNLOADS.iter().map(|path| (*path).to_owned()).collect();
                paths.push(target(&unit.path));
                cache(
                    &identity,
                    &pinned(register, name.as_str()),
                    tracked,
                    root,
                    paths,
                )
            });
            entries.push(Entry {
                work: Work::Check {
                    check,
                    name: name.clone(),
                    unit: unit.path.clone(),
                },
                job: format!("Check {} ({})", check.name(), label(&unit.name)),
                fetch_depth: u8::from(!check.writes_versions()),
                cache,
            });
        }
    }
    if tracked.iter().any(|path| path.to_string() == XTASK) {
        let mut paths: Vec<String> = DOWNLOADS.iter().map(|path| (*path).to_owned()).collect();
        paths.extend(["target/".to_owned(), "xtask/target/".to_owned()]);
        entries.push(Entry {
            work: Work::Xtask,
            job: XTASK_JOB.to_owned(),
            fetch_depth: 1,
            cache: Some(cache("xtask", "", tracked, root, paths)),
        });
    }
    entries
}

/// The folder Cargo builds the workspace at a unit's path into.
fn target(unit: &RelativePath) -> String {
    if unit.is_root() {
        "target/".to_owned()
    } else {
        format!("{unit}/target/")
    }
}

/// What the register pins for a check, as it goes into the key: its programs
/// with their versions, and its coverage toolchain.
fn pinned(register: &Register, name: &str) -> String {
    let check = register
        .check
        .iter()
        .find(|(check, _)| check.as_str() == name)
        .map(|(_, check)| check);
    let programs = check
        .into_iter()
        .flat_map(|check| &check.programs)
        .map(|program| {
            let version = register
                .program
                .get(program)
                .map_or("", |program| program.version.as_str());
            format!("{program} {version}\n")
        });
    let toolchain = check
        .and_then(|check| check.coverage_toolchain.as_ref())
        .map(|toolchain| format!("coverage {toolchain}\n"));
    programs.chain(toolchain).collect()
}

/// The cache of the entry `identity`, with `pinned` and the keyed files in
/// its key.
fn cache(
    identity: &str,
    pinned: &str,
    tracked: &[RelativePath],
    root: &Path,
    paths: Vec<String>,
) -> Cache {
    let mut digest = Sha256::new();
    digest.update(pinned.as_bytes());
    let mut keyed: Vec<&RelativePath> = tracked.iter().filter(|path| is_keyed(path)).collect();
    keyed.sort();
    for path in keyed {
        // A file Git tracks and the working tree lacks reads as empty: its
        // path still stands in the key.
        let content = std::fs::read(root.join(path.to_string())).unwrap_or_default();
        let name = path.to_string();
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name.as_bytes());
        digest.update((content.len() as u64).to_le_bytes());
        digest.update(&content);
    }
    let restore = format!("nebensound-cargo-{KEY_FORM}-{identity}-{RUNNER}-");
    Cache {
        key: format!("{restore}{}", hex(&digest.finalize())),
        restore,
        paths,
    }
}

/// Whether a file's contents go into the key of a Cargo cache.
fn is_keyed(path: &RelativePath) -> bool {
    let name = path.file_name().unwrap_or_default();
    if KEYED.contains(&name) {
        return true;
    }
    let text = path.to_string();
    KEYED_CONFIG.iter().any(|config| {
        text == format!(".cargo/{config}") || text.ends_with(&format!("/.cargo/{config}"))
    })
}

/// The first twelve hexadecimal digits of the digest of a text: a short name
/// for a unit's path in a key, which may hold any character a path may.
fn short(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))[..12].to_owned()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

impl Entry {
    /// The entry as one object of the matrix: what it runs (`entry`, and for
    /// a check `check` and `unit`), the name of its job (`job`), its runner
    /// (`runs-on`), how many commits its checkout holds (`fetch-depth`) and,
    /// where it has one, its cache (`cache`, with the inputs of GitHub's
    /// cache action: `key`, `restore-keys` and `path`, one path per line).
    /// An entry without a cache has no key `cache`, so that the matrix holds
    /// no null.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut entry = match &self.work {
            Work::Reading => json!({ "entry": "reading" }),
            Work::Check { check, unit, .. } => {
                json!({ "entry": "check", "check": check.name(), "unit": unit.to_string() })
            }
            Work::Xtask => json!({ "entry": "xtask" }),
        };
        entry["job"] = json!(self.job);
        entry["runs-on"] = json!(RUNNER);
        entry["fetch-depth"] = json!(self.fetch_depth);
        if let Some(cache) = &self.cache {
            entry["cache"] = json!({
                "key": cache.key,
                "restore-keys": cache.restore,
                "path": cache.paths.join("\n"),
            });
        }
        entry
    }
}

/// The matrix of the entries, as one JSON list.
#[must_use]
pub fn matrix(entries: &[Entry]) -> Value {
    entries.iter().map(Entry::to_json).collect()
}

/// Whether the entry a call is handed claims a check that writes the
/// versions: then the call reads the whole history, for the pull request of
/// the run if there is one. Whether the plan holds the entry is decided
/// after.
#[must_use]
pub fn writes_versions(given: &Value) -> bool {
    given["entry"] == "check"
        && given["check"]
            .as_str()
            .and_then(run::Check::of)
            .is_some_and(run::Check::writes_versions)
}

/// What the reading entry finds that the run asks for and nobody has
/// written yet, one sentence each, split by where the summary shows it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Unwritten {
    /// Every check of a unit of the plan that is not written. The table of
    /// the summary shows each as a cell `not written yet`.
    pub checks: Vec<String>,
    /// Every build, version writer and publish target the run names and the
    /// tool does not carry out. The summary lists them below the table.
    pub others: Vec<String>,
}

impl Unwritten {
    /// Whether everything the run asks for is written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.checks.is_empty() && self.others.is_empty()
    }

    /// Every sentence, the checks first.
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.checks.iter().chain(&self.others)
    }
}

/// What the reading entry finds that the run asks for and nobody has
/// written yet: every check of a unit of the plan that is not written, every
/// build the plan names for a unit that the tool does not carry out, and
/// every version writer and publish target a unit that carries a version
/// names that the tool does not carry out. The entry has the checkout but not
/// the history, so a build counts whether or not the unit builds in this run,
/// as a publish target does.
#[must_use]
pub fn unwritten(plan: &Plan, units: &[Unit], register: &Register) -> Unwritten {
    let mut found = Unwritten::default();
    for unit in &plan.units {
        let label = label(&unit.name);
        for check in &unit.checks {
            if placed(register, check.as_str()) == Placed::Unwritten {
                found.checks.push(format!(
                    "The check '{check}' is not written yet, so the {label} is not checked for it."
                ));
            }
        }
        if let Some(name) = &unit.build
            && build::Build::of(name.as_str()).is_none()
        {
            found.others.push(format!(
                "The build '{name}' is not written yet, so the {label} is not built."
            ));
        }
    }
    for unit in units.iter().filter(|unit| unit.carries_version) {
        let label = label(&unit.name);
        for name in unit
            .pipeline
            .version
            .iter()
            .filter(|name| name.as_str() != writer::CARGO)
        {
            found.others.push(format!(
                "The version writer '{name}' is not written yet, so the version of the {label} is \
                 not written."
            ));
        }
        for name in unit
            .pipeline
            .targets()
            .iter()
            .filter(|name| name.as_str() != release::assets::TARGET)
        {
            found.others.push(format!(
                "The publish target '{name}' is not written yet, so the {label} is not published \
                 there."
            ));
        }
    }
    found
}

/// The reading checks the plan starts, each once, in name order.
#[must_use]
pub fn reading_checks(plan: &Plan, register: &Register) -> Vec<CheckName> {
    let mut checks: Vec<CheckName> = plan
        .units
        .iter()
        .flat_map(|unit| unit.checks.iter())
        .filter(|check| matches!(placed(register, check.as_str()), Placed::Reading(_)))
        .cloned()
        .collect();
    checks.sort();
    checks.dedup();
    checks
}

/// What the reading entry works on: the plan, the units phase 0 resolved,
/// the register and the variables of the run, for its summary.
pub struct Reading<'a> {
    /// The plan of the commit at hand.
    pub plan: &'a Plan,
    /// The units phase 0 resolved, for the version writers and publish
    /// targets of those that carry a version.
    pub units: &'a [Unit],
    /// The register, which says on which side each check is.
    pub register: &'a Register,
    /// The variables of the run: where its summary goes.
    pub variables: Variables<'a>,
}

/// How a check went in a unit, as the table of the reading entry shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Passed,
    Failed,
    NotRun,
    Unwritten,
}

impl Cell {
    fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::NotRun => "not run",
            Self::Unwritten => "not written yet",
        }
    }
}

/// Runs the reading entry: whether it passed.
///
/// First what is not written yet, failed by name ([`unwritten`]). Then the
/// programs of every reading check the plan starts, set up by `set_up`, and
/// then each reading check in every unit the plan starts it at, by
/// `check`, the log in one group per check. Last a table of check by unit,
/// into the log and into the summary of the run, with what else is not
/// written yet below it. When the programs cannot be set up, no check runs.
pub fn reading(
    reading: &Reading<'_>,
    set_up: &mut dyn FnMut(&[CheckName], &mut Log<'_>) -> bool,
    check: &mut dyn FnMut(run::Check, &RelativePath, &mut Log<'_>) -> bool,
    log: &mut Log<'_>,
) -> bool {
    let Reading {
        plan,
        units,
        register,
        variables,
    } = *reading;
    let missing = unwritten(plan, units, register);
    log.command("::group::Not written yet");
    if missing.is_empty() {
        log.line("Everything the run asks for is written.");
    }
    for sentence in missing.iter() {
        log.error(sentence);
    }
    log.command("::endgroup::");

    let checks = reading_checks(plan, register);
    log.command("::group::The programs of the reading checks");
    let ready = set_up(&checks, log);
    log.command("::endgroup::");
    if !ready {
        log.error("The programs of the reading checks could not be set up, so none of them ran.");
    }

    // The rows: every check of the plan that is not executing, in name
    // order; the columns: every unit that starts one of them, in the order
    // of the plan.
    let mut rows: Vec<&CheckName> = plan
        .units
        .iter()
        .flat_map(|unit| unit.checks.iter())
        .filter(|name| !matches!(placed(register, name.as_str()), Placed::Executing(_)))
        .collect();
    rows.sort();
    rows.dedup();
    let columns: Vec<&crate::plan::Planned> = plan
        .units
        .iter()
        .filter(|unit| unit.checks.iter().any(|name| rows.contains(&name)))
        .collect();
    let mut cells = vec![vec![None; columns.len()]; rows.len()];
    for (row, name) in rows.iter().enumerate() {
        let placed = placed(register, name.as_str());
        let started = columns
            .iter()
            .enumerate()
            .filter(|(_, unit)| unit.checks.contains(name));
        let Placed::Reading(runnable) = placed else {
            for (column, _) in started {
                cells[row][column] = Some(Cell::Unwritten);
            }
            continue;
        };
        if !ready {
            for (column, _) in started {
                cells[row][column] = Some(Cell::NotRun);
            }
            continue;
        }
        log.command(&format!("::group::Check {name}"));
        for (column, unit) in started {
            log.line(&format!("{name} in the {}:", label(&unit.name)));
            let passed = check(runnable, &unit.path, log);
            cells[row][column] = Some(if passed { Cell::Passed } else { Cell::Failed });
        }
        log.command("::endgroup::");
    }
    let passed = missing.is_empty()
        && ready
        && cells
            .iter()
            .flatten()
            .all(|cell| matches!(cell, None | Some(Cell::Passed)));

    let mut table = String::from("| Check |");
    let mut rule = String::from("| --- |");
    for unit in &columns {
        let _ = write!(table, " {} |", label(&unit.name));
        rule.push_str(" --- |");
    }
    let _ = write!(table, "\n{rule}\n");
    for (row, name) in rows.iter().enumerate() {
        let _ = write!(table, "| {name} |");
        for cell in &cells[row] {
            let _ = write!(table, " {} |", cell.map_or("", Cell::as_str));
        }
        table.push('\n');
    }
    let mut summary = format!("### Reading checks\n\n{table}");
    // A check not written yet already stands in the table.
    if !missing.others.is_empty() {
        summary.push('\n');
        for sentence in &missing.others {
            let _ = writeln!(summary, "- {sentence}");
        }
    }
    log.line("The reading checks:");
    log.lines(&table);
    if let Err(message) = crate::workflow::summary(variables, &summary) {
        log.error(&message);
        return false;
    }
    passed
}
