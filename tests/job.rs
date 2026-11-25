//! `nebensound-repo job`: the entries of the check matrix, and one call that
//! runs one of them.
//!
//! It reads this repository, whose own entries run through the call, so the
//! crate `cargo package` builds leaves it out (see its `Cargo.toml`).
//!
//! No real program runs here. The programs of the reading checks are shell
//! scripts pinned in the tool's folder of a home of the test's own, and
//! Node.js, rustup and Cargo stand on its search path, so that the call sets
//! up nothing and downloads nothing. Hence Unix only: the gate runs on Linux.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use nebensound_repo::Register;
use nebensound_repo::entry::{self, Entry, Work};
use nebensound_repo::finding::UnitName;
use nebensound_repo::names::ProgramName;
use nebensound_repo::output::Output;
use nebensound_repo::path::RelativePath;
use nebensound_repo::plan;
use nebensound_repo::run;
use nebensound_repo::setup::{Environment, find};
use serde_json::{Value, json};
use support::{Git, call_with, program::executable};
use tempfile::TempDir;

/// Prettier, which finds every file with `unformatted` in its name
/// unformatted.
const PRETTIER: &str = r#"#!/bin/sh
echo "Checking formatting..."
status=0
for arg in "$@"; do
  case "$arg" in *unformatted*) echo "[warn] $arg"; status=1 ;; esac
done
exit "$status"
"#;

/// Taplo, which reads what it is handed and finds nothing.
const TAPLO: &str = "#!/bin/sh\ncat > /dev/null\nexit 0\n";

/// lychee, which finds every link.
const LYCHEE: &str = r##"#!/bin/sh
while [ $# -gt 0 ]; do
  case "$1" in --output) out=$2; shift ;; esac
  shift
done
printf '%s\n' "# Summary" > "$out"
exit 0
"##;

/// actionlint, which finds nothing.
const ACTIONLINT: &str = "#!/bin/sh\necho '[]'\nexit 0\n";

/// Node.js: its version, or markdownlint's script, which finds nothing.
const NODE: &str = r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo v24.0.0; exit 0; fi
printf '%s' '{"frontMatter":null,"groups":[]}' > "$RESULTS"
exit 0
"#;

/// rustup, which installs a toolchain, names it, and fails to install the
/// one a fixture's workspace pins when the fixture says so.
const RUSTUP: &str = r#"#!/bin/sh
echo "rustup $*" >&2
if [ "$1 $2" = "show active-toolchain" ]; then echo "1.98.1-x86_64-unknown-linux-gnu (default)"; exit 0; fi
[ -f "$(dirname "$0")/../refuse" ] && { echo "error: no such toolchain"; exit 1; }
exit 0
"#;

/// Cargo, which says how it was called.
const CARGO: &str = "#!/bin/sh\necho \"cargo $* with $RUSTUP_TOOLCHAIN\"\nexit 0\n";

/// The reading entry, as the matrix holds it.
fn reading() -> Value {
    json!({"entry": "reading", "job": "Check files", "runs-on": "ubuntu-latest",
           "fetch-depth": 1})
}

/// A computer with the programs above: pinned in the tool's folder of its
/// home, or on its search path, and a file for the summary of the run.
struct Computer {
    _dir: TempDir,
    base: PathBuf,
}

impl Computer {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(dir.path()).unwrap();
        let computer = Self { _dir: dir, base };
        fs::create_dir_all(computer.base.join("home")).unwrap();
        for (name, script) in [("node", NODE), ("rustup", RUSTUP), ("cargo", CARGO)] {
            executable(&computer.base.join("bin").join(name), script);
        }
        let path = std::env::var_os("PATH").unwrap_or_default();
        for utility in ["cat", "dirname"] {
            let found = find::search(utility, &path, false).unwrap();
            std::os::unix::fs::symlink(found, computer.base.join("bin").join(utility)).unwrap();
        }
        for (name, script) in [
            ("prettier", PRETTIER),
            ("taplo", TAPLO),
            ("lychee", LYCHEE),
            ("actionlint", ACTIONLINT),
            ("markdownlint", "{}\n"),
            ("cargo-llvm-cov", "#!/bin/sh\nexit 0\n"),
        ] {
            computer.pin(name, script);
        }
        computer
    }

    /// A computer whose tool's folder holds nothing: setting up the programs
    /// fails, since there is no curl to download them with.
    fn bare() -> Self {
        let computer = Self::new();
        fs::remove_dir_all(computer.base.join("home")).unwrap();
        fs::create_dir_all(computer.base.join("home")).unwrap();
        computer
    }

    /// The variables of the run: the search path, the home, the summary.
    fn variables(&self) -> Vec<(&'static str, String)> {
        vec![
            ("PATH", self.base.join("bin").display().to_string()),
            ("HOME", self.base.join("home").display().to_string()),
            (
                "GITHUB_STEP_SUMMARY",
                self.base.join("summary").display().to_string(),
            ),
        ]
    }

    /// The computer the call sees, as the variables describe it.
    fn environment(&self) -> Environment {
        let variables = self.variables();
        Environment::of(&|name: &str| {
            variables
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.clone())
        })
    }

    /// A program pinned in the tool's folder, where `setup --pinned` puts it.
    fn pin(&self, name: &str, script: &str) {
        let register = Register::builtin().unwrap();
        let environment = self.environment();
        let name = ProgramName::new(name).unwrap();
        let program = register.program.get(&name).unwrap();
        let dir = find::program_folder(
            environment.folder.as_deref().unwrap(),
            &name,
            &program.version,
        );
        let file = find::program_executable(&register, &name, program, &dir, &environment).unwrap();
        executable(&file, script);
        fs::write(dir.join(find::MARK), "").unwrap();
    }

    /// The summary of the run, as the call wrote it.
    fn summary(&self) -> String {
        fs::read_to_string(self.base.join("summary")).unwrap_or_default()
    }

    /// `job` with this entry in the repository at `root`, with these
    /// variables beside the computer's.
    fn job(&self, root: &Path, given: &Value, more: &[(&str, &str)]) -> (String, u8) {
        // The first of a name counts, so what is given here wins.
        let mut variables: Vec<(&str, String)> = more
            .iter()
            .map(|(name, value)| (*name, (*value).to_owned()))
            .collect();
        variables.extend(self.variables());
        let borrowed: Vec<(&str, &str)> = variables
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        let (text, code) = call_with(
            &[
                "nebensound-repo",
                "-C",
                root.to_str().unwrap(),
                "job",
                &given.to_string(),
            ],
            &borrowed,
            Output::Workflow,
        );
        (support::printed(&text), code)
    }
}

fn path(text: &str) -> RelativePath {
    RelativePath::new(text).unwrap()
}

/// A repository with Rust at its root, the component `tools` with Python,
/// an xtask and Cargo's configuration, and the files the cache keys read.
const MIXED: &str = r#"[repo]
audience = "public"
pipeline = { check = ["rust"] }

[component.tools]
path     = "tools"
pipeline = { check = ["python"] }
"#;

const MIXED_FILES: &[&str] = &[
    ".nebensound/repo.toml",
    "Cargo.toml",
    "Cargo.lock",
    "src/lib.rs",
    "xtask/Cargo.toml",
    "xtask/src/main.rs",
    ".cargo/config.toml",
    "nested/.cargo/config",
    "tools/main.py",
    "README.md",
];

/// The entries of [`MIXED`], the files they read below `root`.
fn mixed_entries(root: &Path) -> Vec<Entry> {
    let analysis = support::analysis(MIXED, MIXED_FILES);
    let plan = plan::plan(&analysis, &support::central());
    let tracked: Vec<RelativePath> = MIXED_FILES.iter().map(|file| path(file)).collect();
    entry::entries(&plan, &Register::builtin().unwrap(), &tracked, root)
}

#[test]
fn a_plan_gives_one_reading_entry_one_per_executing_check_and_one_for_the_xtask() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("Cargo.lock"), "version = 4\n").unwrap();
    let entries = mixed_entries(root);
    let shown: Vec<Value> = entries.iter().map(Entry::to_json).collect();

    assert_eq!(shown[0], reading());
    assert_eq!(entries[0].work, Work::Reading);
    // The Rust check writes the versions, so it reads the whole history, and
    // it starts Cargo, so it has a cache.
    let rust = &shown[1];
    assert_eq!(rust["entry"], "check");
    assert_eq!(rust["check"], "rust");
    assert_eq!(rust["unit"], ".");
    assert_eq!(rust["job"], "Check rust (repository)");
    assert_eq!(rust["runs-on"], "ubuntu-latest");
    assert_eq!(rust["fetch-depth"], 0);
    let key = rust["cache"]["key"].as_str().unwrap();
    let restore = rust["cache"]["restore-keys"].as_str().unwrap();
    assert!(
        restore.starts_with("nebensound-cargo-v1-rust-"),
        "{restore}"
    );
    assert!(restore.ends_with("-ubuntu-latest-"), "{restore}");
    assert!(key.starts_with(restore), "{key}");
    assert_eq!(key.len(), restore.len() + 64, "{key}");
    assert_eq!(
        rust["cache"]["path"],
        "~/.cargo/registry/index/\n~/.cargo/registry/cache/\n~/.cargo/git/db/\ntarget/"
    );
    assert!(matches!(
        &entries[1].work,
        Work::Check { check: run::Check::Rust, name, unit }
            if name.as_str() == "rust" && unit.is_root()
    ));
    // Python executes and starts no Cargo: the commit at hand, no cache.
    assert_eq!(
        shown[2],
        json!({"entry": "check", "check": "python", "unit": "tools",
               "job": "Check python (component tools)", "runs-on": "ubuntu-latest",
               "fetch-depth": 1})
    );
    // The repository's own check, which starts Cargo at the root.
    let xtask = &shown[3];
    assert_eq!(xtask["entry"], "xtask");
    assert_eq!(xtask["job"], "Check xtask");
    assert_eq!(xtask["fetch-depth"], 1);
    assert!(
        xtask["cache"]["key"]
            .as_str()
            .unwrap()
            .starts_with("nebensound-cargo-v1-xtask-ubuntu-latest-"),
        "{xtask}"
    );
    assert!(
        xtask["cache"]["path"]
            .as_str()
            .unwrap()
            .ends_with("\ntarget/\nxtask/target/"),
        "{xtask}"
    );
    assert_eq!(shown.len(), 4, "{shown:#?}");
    assert_eq!(entry::matrix(&entries), Value::Array(shown.clone()));

    // What decides what Cargo builds changes the key; a source file does not.
    let keys = |root: &Path| -> Vec<String> {
        mixed_entries(root)
            .iter()
            .filter_map(|entry| entry.cache.as_ref().map(|cache| cache.key.clone()))
            .collect()
    };
    let before = keys(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "changed").unwrap();
    assert_eq!(keys(root), before);
    for changed in ["Cargo.lock", ".cargo/config.toml", "nested/.cargo/config"] {
        let file = root.join(changed);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "changed").unwrap();
        let after = keys(root);
        assert_ne!(after[0], before[0], "{changed}");
        assert_ne!(after[1], before[1], "{changed}");
        fs::remove_file(&file).unwrap();
    }
    fs::write(root.join("Cargo.lock"), "version = 4\n").unwrap();
    assert_eq!(keys(root), before);
}

#[test]
fn a_unit_below_the_root_builds_into_its_own_target() {
    let source = "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.app]\npath = \
                  \"crates/app\"\npipeline = { check = [\"rust\"] }\n";
    let files = [
        ".nebensound/repo.toml",
        "crates/app/Cargo.toml",
        "crates/app/src/lib.rs",
    ];
    let analysis = support::analysis(source, &files);
    let plan = plan::plan(&analysis, &support::central());
    let tracked: Vec<RelativePath> = files.iter().map(|file| path(file)).collect();
    let dir = tempfile::tempdir().unwrap();
    let entries = entry::entries(&plan, &Register::builtin().unwrap(), &tracked, dir.path());
    let rust = entries[1].to_json();
    assert_eq!(rust["job"], "Check rust (component app)");
    assert_eq!(rust["unit"], "crates/app");
    assert!(
        rust["cache"]["path"]
            .as_str()
            .unwrap()
            .ends_with("\ncrates/app/target/"),
        "{rust}"
    );
    // No xtask, no entry for it.
    assert_eq!(entries.len(), 2);
}

#[test]
fn every_check_is_placed_by_its_side() {
    let register = Register::builtin().unwrap();
    assert_eq!(
        entry::placed(&register, "markdown"),
        entry::Placed::Reading(run::Check::Markdown)
    );
    assert_eq!(
        entry::placed(&register, "python"),
        entry::Placed::Executing(run::Check::Python)
    );
    assert_eq!(entry::placed(&register, "cpp"), entry::Placed::Unwritten);
    assert_eq!(
        entry::placed(&register, "nonsense"),
        entry::Placed::Unwritten
    );
    assert_eq!(entry::label(&UnitName::Repository), "repository");
}

#[test]
fn the_reading_entry_names_what_is_not_written_yet() {
    let source = r#"[repo]
audience = "public"
pipeline = "none"

[component.fw]
path     = "fw"
pipeline = { check = ["cpp"], version = ["cargo", "asciidoc"], build = "platformio", publish = ["release-assets", "ghcr"] }
"#;
    let analysis = support::analysis(source, &[".nebensound/repo.toml", "fw/main.c"]);
    let plan = plan::plan(&analysis, &support::central());
    let found = entry::unwritten(&plan, &analysis.units, &Register::builtin().unwrap());
    assert_eq!(
        found.checks,
        ["The check 'cpp' is not written yet, so the component fw is not checked for it."]
    );
    assert_eq!(
        found.others,
        [
            "The build 'platformio' is not written yet, so the component fw is not built.",
            "The version writer 'asciidoc' is not written yet, so the version of the component \
             fw is not written.",
            "The publish target 'ghcr' is not written yet, so the component fw is not published \
             there.",
        ]
    );
    assert!(!found.is_empty());
    assert_eq!(found.iter().count(), 4);
    let checks: Vec<String> = entry::reading_checks(&plan, &Register::builtin().unwrap())
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(checks, ["json", "markdown", "toml", "yaml"]);
}

#[test]
fn the_call_refuses_what_is_no_entry_of_the_plan() {
    let repo = support::repository(&[(".nebensound/repo.toml", support::QUIET)]);
    let computer = Computer::new();
    // No object of JSON: the call is wrong.
    for given in [json!("reading"), json!([reading()])] {
        let (text, code) = computer.job(repo.path(), &given, &[]);
        assert_eq!(code, 2, "{text}");
        assert!(
            text.contains("is no object of JSON, as the matrix holds one"),
            "{text}"
        );
    }
    let (text, code) = call_with(
        &["nebensound-repo", "job", "{not json"],
        &[],
        Output::Elsewhere,
    );
    assert_eq!(code, 2, "{text}");
    // Outside a workflow run the call opens with the line that the gate
    // decides.
    assert_eq!(text.lines().next(), Some(run::OUTSIDE), "{text}");
    // An entry the plan does not hold, such as one with another runner or a
    // check the plan starts nowhere, runs nothing.
    let mut elsewhere = reading();
    elsewhere["runs-on"] = json!("macos-latest");
    let python = json!({"entry": "check", "check": "python", "unit": ".",
                        "job": "Check python (repository)", "runs-on": "ubuntu-latest",
                        "fetch-depth": 1});
    for given in [elsewhere, python, json!({})] {
        let (text, code) = computer.job(repo.path(), &given, &[]);
        assert_eq!(code, 1, "{text}");
        assert!(text.starts_with("the plan holds no such entry: "), "{text}");
        assert!(text.contains("\"entry\": \"reading\""), "{text}");
        assert!(!text.contains("Checks:"), "{text}");
    }
    // A Rust entry reads the pull request of the run, which has to be one.
    let rust = json!({"entry": "check", "check": "rust", "unit": "."});
    let (text, code) = computer.job(repo.path(), &rust, &[("NEBENSOUND_PULL_REQUEST", "x")]);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("NEBENSOUND_PULL_REQUEST is \"x\""), "{text}");
    // Phase 0 found errors: nothing is checked.
    let broken = support::repository(&[
        (".nebensound/repo.toml", support::QUIET),
        ("strange.xyz", "covered by no check\n"),
    ]);
    let (text, code) = computer.job(broken.path(), &reading(), &[]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(
        text,
        "nothing is checked: phase 0 found errors, and `check` says which\n"
    );
}

/// A repository whose reading checks find a file unformatted, with a
/// component whose check and build are not written yet.
fn unformatted() -> TempDir {
    support::repository(&[
        (
            ".nebensound/repo.toml",
            "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.fw]\npath = \
             \"fw\"\npipeline = { check = [\"cpp\"], build = \"platformio\" }\n",
        ),
        ("unformatted.json", "{}\n"),
        ("fw/main.c", "int main(void) { return 0; }\n"),
    ])
}

#[test]
fn the_reading_entry_runs_every_reading_check_and_writes_a_table() {
    let repo = unformatted();
    let computer = Computer::new();
    let (text, code) = computer.job(repo.path(), &reading(), &[]);
    assert_eq!(code, 1, "{text}");
    // What is not written yet first, then the programs, then one group per
    // check, then the table.
    let groups: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("::group::"))
        .collect();
    assert_eq!(
        groups,
        [
            "Not written yet",
            "The programs of the reading checks",
            "Check json",
            "Check markdown",
            "Check toml",
            "Check yaml",
        ],
        "{text}"
    );
    assert!(
        text.contains(
            "::error::The check 'cpp' is not written yet, so the component fw is not checked \
             for it."
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "::error::The build 'platformio' is not written yet, so the component fw is not \
             built."
        ),
        "{text}"
    );
    assert!(text.contains("json in the repository:"), "{text}");
    assert!(text.contains("[warn] unformatted.json"), "{text}");
    let table = "| Check | repository | component fw |\n\
                 | --- | --- | --- |\n\
                 | cpp |  | not written yet |\n\
                 | json | failed |  |\n\
                 | markdown | passed |  |\n\
                 | toml | passed |  |\n\
                 | yaml | passed |  |\n";
    assert!(
        text.contains(&format!("The reading checks:\n{table}")),
        "{text}"
    );
    assert_eq!(
        computer.summary(),
        format!(
            "### Reading checks\n\n{table}\n- The build 'platformio' is not written yet, so \
             the component fw is not built.\n"
        )
    );
}

#[test]
fn the_reading_entry_passes_when_every_check_passes_and_runs_none_without_its_programs() {
    let repo = support::repository(&[(".nebensound/repo.toml", support::QUIET)]);
    let computer = Computer::new();
    let (text, code) = computer.job(repo.path(), &reading(), &[]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("Everything the run asks for is written."),
        "{text}"
    );
    assert!(
        !computer.summary().contains("\n- "),
        "{}",
        computer.summary()
    );

    // Without the programs, and nothing to download them with, no check runs.
    let bare = Computer::bare();
    let (text, code) = bare.job(repo.path(), &reading(), &[]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(
            "::error::The programs of the reading checks could not be set up, so none of them \
             ran."
        ),
        "{text}"
    );
    assert!(!text.contains("::group::Check json"), "{text}");
    assert!(
        bare.summary().contains("| json | not run |"),
        "{}",
        bare.summary()
    );

    // A summary that cannot be written fails the entry.
    let (text, code) = computer.job(
        repo.path(),
        &reading(),
        &[("GITHUB_STEP_SUMMARY", "/nonexistent/folder/summary")],
    );
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::the summary of the run cannot be written to "),
        "{text}"
    );
}

/// A repository with Rust at its root, with a release and a pull request,
/// checked out as GitHub checks one out: the merge commit, detached.
fn rust_pull_request() -> Git {
    let git = Git::new();
    git.write(&[
        (
            ".nebensound/repo.toml",
            "[repo]\naudience = \"public\"\npipeline = { version = [\"cargo\"], check = \
             [\"rust\"], dependabot = [\"cargo\"] }\n",
        ),
        (
            "Cargo.toml",
            "[package]\nname = \"demo\"\nedition = \"2024\"\n",
        ),
        ("src/lib.rs", "//! A library.\n"),
    ]);
    git.commit("feat: start");
    git.run(&["checkout", "--quiet", "-b", "topic"]);
    git.write(&[("src/lib.rs", "//! A library, changed.\n")]);
    git.commit("wip");
    git.run(&["checkout", "--quiet", "--detach", "main"]);
    git.run(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "Merge topic",
        "topic",
    ]);
    git
}

#[test]
fn an_executing_entry_sets_up_its_check_and_runs_it() {
    let git = rust_pull_request();
    let computer = Computer::new();
    let tracked = [".nebensound/repo.toml", "Cargo.toml", "src/lib.rs"];
    let root = fs::canonicalize(git.path()).unwrap();
    let analysis = nebensound_repo::repository::Repository::at(&root)
        .analyse_files(&support::central())
        .unwrap();
    let plan = plan::plan(&analysis, &support::central());
    let tracked: Vec<RelativePath> = tracked.iter().map(|file| path(file)).collect();
    let entries = entry::entries(&plan, &Register::builtin().unwrap(), &tracked, &root);
    let rust = entries[1].to_json();
    assert_eq!(rust["check"], "rust", "{rust}");
    let pull_request = [
        ("NEBENSOUND_PULL_REQUEST", "4"),
        ("NEBENSOUND_TITLE", "fix: change the library"),
    ];
    // The toolchain the workspace pins cannot be installed: the check runs
    // and fails, after its program was set up.
    executable(&computer.base.join("refuse"), "");
    let (text, code) = computer.job(&root, &rust, &pull_request);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("Checks: rust\n"), "{text}");
    assert!(text.contains("1 Rust file(s) to check:"), "{text}");
    assert!(text.contains("rustup toolchain install"), "{text}");
    // Without its program, and nothing to download it with, it does not run.
    let bare = Computer::bare();
    let (text, code) = bare.job(&root, &rust, &pull_request);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("Checks: rust\n"), "{text}");
    assert!(!text.contains("Rust file(s) to check"), "{text}");
}

/// This repository, as the tool's tests find it.
fn here() -> PathBuf {
    fs::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap()
}

/// The entries of this repository, as the preparing job hands them on.
fn entries_here() -> Vec<Value> {
    let root = here();
    let analysis = nebensound_repo::repository::Repository::at(&root)
        .analyse_files(&support::central())
        .unwrap();
    assert!(!analysis.failed());
    let plan = plan::plan(&analysis, &support::central());
    let tracked: Vec<RelativePath> = analysis
        .tree
        .files()
        .into_iter()
        .map(|(node, _)| node.path.clone())
        .collect();
    entry::entries(&plan, &Register::builtin().unwrap(), &tracked, &root)
        .iter()
        .map(Entry::to_json)
        .collect()
}

#[test]
fn this_repository_has_a_reading_entry_its_rust_check_and_its_xtask() {
    let entries = entries_here();
    let jobs: Vec<&str> = entries
        .iter()
        .map(|entry| entry["job"].as_str().unwrap())
        .collect();
    assert_eq!(
        jobs,
        ["Check files", "Check rust (repository)", "Check xtask"]
    );
    assert_eq!(entries[0], reading());
}

#[test]
fn the_reading_entry_of_this_repository_runs_through_the_call() {
    let computer = Computer::new();
    let (text, code) = computer.job(&here(), &reading(), &[]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        computer.summary(),
        "### Reading checks\n\n\
         | Check | repository |\n\
         | --- | --- |\n\
         | json | passed |\n\
         | markdown | passed |\n\
         | toml | passed |\n\
         | yaml | passed |\n"
    );
}

#[test]
fn the_xtask_entry_of_this_repository_runs_through_the_call() {
    let computer = Computer::new();
    let xtask = entries_here().pop().unwrap();
    assert_eq!(xtask["entry"], "xtask");
    let (text, code) = computer.job(&here(), &xtask, &[]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("rustup toolchain install"), "{text}");
    assert!(
        text.contains("cargo xtask check --all with 1.98.1-x86_64-unknown-linux-gnu"),
        "{text}"
    );
    // Nothing is set up for it: rustup provides its toolchain.
    assert!(!text.contains("Checks:"), "{text}");
}
