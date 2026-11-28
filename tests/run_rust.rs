//! `nebensound-repo run rust`: the Rust check, run from the tool.
//!
//! No real toolchain runs here. `cargo`, `rustup`, `rustc`, `rustfmt` and
//! `cargo-llvm-cov` are shell scripts that print how they were called and act
//! the way the fixture tells them: `cargo metadata` prints the metadata the
//! fixture wrote, a call fails when a line of the fixture's `fail` file is
//! part of its arguments, and `cargo package` leaves the packages the fixture
//! names where Cargo would. That the real toolchain finds what the block
//! found is shown by the cases of the block, which `cargo xtask check --all`
//! runs through `run`. Hence Unix only: the gate runs on Linux.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use nebensound_repo::Register;
use nebensound_repo::console::Console;
use nebensound_repo::names::{Reason, VersionWriterName};
use nebensound_repo::output::Output;
use nebensound_repo::packages::Excluded;
use nebensound_repo::path::RelativePath;
use nebensound_repo::programs::Platform;
use nebensound_repo::repository::Repository;
use nebensound_repo::run::rust::{self, Local, Metadata};
use nebensound_repo::run::{self, Check, Context, Packages};
use nebensound_repo::setup::{Environment, find};
use nebensound_repo::toolchain;
use nebensound_repo::writer::Order;
use serde_json::{Value, json};
use support::{QUIET, call, program::executable};
use tempfile::TempDir;

/// Whether a call fails: a line of the fixture's `fail` file is part of what
/// it was called with.
const FAILING: &str = r#"here=$(cd "$(dirname "$0")/.." && pwd -P)
failing() {
  [ -f "$here/fail" ] || return 1
  while IFS= read -r pattern; do
    [ -n "$pattern" ] || continue
    case "$1" in *"$pattern"*) return 0 ;; esac
  done < "$here/fail"
  return 1
}
"#;

const CARGO: &str = r#"
echo "cargo $* in $(basename "$(pwd -P)") RUSTDOCFLAGS=${RUSTDOCFLAGS-unset} CARGO_MANIFEST_DIR=${CARGO_MANIFEST_DIR-unset}" >&2
case "$1" in
  metadata)
    failing "$*" && { echo "error: failed to load manifest" >&2; exit 101; }
    if [ -f "$here/after" ] && grep -q '^version' Cargo.toml; then cat "$here/after"; else cat "$here/metadata"; fi
    exit 0 ;;
  fmt)
    if [ -f "$here/named" ]; then
      case "$*" in
        *--files-with-diff*) cat "$here/named" ;;
        *) echo "Diff in $(pwd -P)/src/lib.rs at line 1:" ;;
      esac
      exit 1
    fi
    failing "$*" && { echo "error: rustfmt cannot format" >&2; exit 1; }
    exit 0 ;;
  package)
    failing "$*" && exit 101
    while IFS= read -r dir; do
      mkdir -p "target/package/$dir/src"
      echo "[package]" > "target/package/$dir/Cargo.toml"
      echo "pub fn f() {}" > "target/package/$dir/src/lib.rs"
    done < "$here/packaged"
    [ -f "$here/unreadable" ] && chmod 000 "target/package/$(head -n 1 "$here/packaged")/src/lib.rs"
    [ -f "$here/read-only" ] && chmod 444 Cargo.toml
    exit 0 ;;
esac
failing "$*" && { echo "error: it failed" >&2; exit 101; }
exit 0
"#;

const RUSTUP: &str = r#"
echo "rustup $* in $(basename "$(pwd -P)")" >&2
if [ "$1 $2" = "show active-toolchain" ]; then
  [ -f "$here/no-toolchain" ] || echo "1.98.1-x86_64-unknown-linux-gnu (overridden by '$(pwd -P)/rust-toolchain.toml')"
  exit 0
fi
failing "$*" && exit 1
exit 0
"#;

const RUSTC: &str = r#"
echo "rustc 1.98.1 ($*)"
failing "rustc $*" && exit 1
exit 0
"#;

const RUSTFMT: &str = r#"
echo "rustfmt $*" >&2
if [ -f "$here/named-others" ]; then
  case "$*" in
    *--files-with-diff*) for arg in "$@"; do case "$arg" in /*) echo "$arg" ;; esac; done ;;
    *) echo "Diff in a path dependency" ;;
  esac
  exit 1
fi
exit 0
"#;

const LLVM_COV: &str = r#"
echo "cargo-llvm-cov $* RUSTUP_TOOLCHAIN=$RUSTUP_TOOLCHAIN"
failing "cargo-llvm-cov $*" && { echo "error: coverage is below 100%"; exit 1; }
exit 0
"#;

fn path(text: &str) -> RelativePath {
    RelativePath::new(text).unwrap()
}

/// A repository on disk, the programs above, and what the check is handed.
struct Fixture {
    base: TempDir,
    /// The files of the check.
    files: Vec<&'static str>,
    /// Every file Git tracks.
    tracked: Vec<&'static str>,
    /// The files of `tracked` that no `[[exclude]]` entry takes out, as far
    /// as a test names them.
    kept: Vec<&'static str>,
    /// The packages `[[exclude]]` takes out, folder and reason.
    excluded: Vec<(&'static str, &'static str)>,
    /// The versions the package test writes.
    orders: Vec<Order>,
    /// The register; the built-in one unless a test changes it.
    register: Register,
    /// Where the check makes its folder.
    temp: PathBuf,
    workflow: bool,
}

impl Fixture {
    /// A workspace at the root: the members `demo` (the root package) and
    /// `inner`, and `vendor/old`, a member the declaration excludes, with
    /// the path dependency `vendor/old/kept` inside it; the excluded path
    /// dependency `third`, and `outside`, a path dependency outside the
    /// repository. `demo` and `inner` are published.
    fn new() -> Self {
        let base = tempfile::tempdir().unwrap();
        let bin = base.path().join("bin");
        for (name, script) in [
            ("cargo", CARGO),
            ("rustup", RUSTUP),
            ("rustc", RUSTC),
            ("rustfmt", RUSTFMT),
            ("cargo-llvm-cov", LLVM_COV),
        ] {
            executable(&bin.join(name), format!("#!/bin/sh\n{FAILING}{script}"));
        }
        for dir in ["tmp", "cache", "repo", "outside", "utilities"] {
            fs::create_dir_all(base.path().join(dir)).unwrap();
        }
        // The utilities the programs above use, and nothing else of the
        // computer, so that a program a test takes away is not found
        // elsewhere.
        let computer = std::env::var_os("PATH").unwrap_or_default();
        for utility in [
            "basename", "cat", "chmod", "dirname", "grep", "head", "mkdir",
        ] {
            let found = find::search(utility, &computer, false).unwrap();
            std::os::unix::fs::symlink(found, base.path().join("utilities").join(utility)).unwrap();
        }
        let temp = fs::canonicalize(base.path().join("tmp")).unwrap();
        let fixture = Self {
            base,
            files: vec![
                "src/lib.rs",
                "inner/src/lib.rs",
                "vendor/old/kept/src/lib.rs",
            ],
            tracked: vec![
                "Cargo.lock",
                "Cargo.toml",
                "inner/Cargo.toml",
                "inner/src/lib.rs",
                "src/lib.rs",
                "third/Cargo.toml",
                "third/src/lib.rs",
                "vendor/old/Cargo.toml",
                "vendor/old/README.md",
                "vendor/old/kept/Cargo.toml",
                "vendor/old/kept/src/lib.rs",
                "vendor/old/src/lib.rs",
            ],
            kept: Vec::new(),
            excluded: vec![
                ("vendor/old", "a vendored copy"),
                ("third", "code of others"),
            ],
            orders: vec![Order {
                unit: "the repository".to_owned(),
                path: RelativePath::root(),
                version: Some("1.2.3".to_owned()),
                current: Some("1.2.3".to_owned()),
                writers: vec![VersionWriterName::new("cargo").unwrap()],
            }],
            register: Register::builtin().unwrap(),
            temp,
            workflow: true,
        };
        fixture
            .file(
                "Cargo.toml",
                "[package]\nname = \"demo\"\nedition = \"2024\"\n\n[workspace]\nmembers = [\"inner\", \"vendor/old\"]\n",
            )
            .file(
                "Cargo.lock",
                "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.0.0\"\n",
            )
            .file("inner/Cargo.toml", "[package]\nname = \"inner\"\n")
            .file("vendor/old/Cargo.toml", "[package]\nname = \"old\"\n")
            .file("vendor/old/kept/Cargo.toml", "[package]\nname = \"kept\"\n")
            .file("third/Cargo.toml", "[package]\nname = \"third\"\n")
            .file("src/lib.rs", "//! The crate.\n")
            .file("inner/src/lib.rs", "//! The inner crate.\n")
            .mark("packaged", "demo-1.2.3\ninner-1.2.3\n");
        let metadata = fixture.metadata("0.0.0");
        fixture.mark("metadata", &metadata.to_string());
        fixture.mark("after", &fixture.metadata("1.2.3").to_string());
        fixture
    }

    fn root(&self) -> PathBuf {
        fs::canonicalize(self.base.path().join("repo")).unwrap()
    }

    fn outside(&self) -> PathBuf {
        fs::canonicalize(self.base.path().join("outside")).unwrap()
    }

    /// Writes a file of the repository.
    fn file(&self, name: &str, content: &str) -> &Self {
        let file = self.base.path().join("repo").join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
        self
    }

    /// Writes a file that tells the programs what to do.
    fn mark(&self, name: &str, content: &str) -> &Self {
        fs::write(self.base.path().join(name), content).unwrap();
        self
    }

    /// Makes the calls that hold one of these fail.
    fn fail(&self, patterns: &[&str]) -> &Self {
        self.mark("fail", &format!("{}\n", patterns.join("\n")))
    }

    /// The metadata of the workspace, with the published packages at this
    /// version.
    fn metadata(&self, version: &str) -> Value {
        let root = self.root();
        let at = |dir: &str| {
            if dir == "." {
                root.clone()
            } else {
                root.join(dir)
            }
        };
        let package = |name: &str, dir: PathBuf, publish: Value, msrv: Value, edition: &str| {
            json!({
                "id": format!("{name}-id"),
                "name": name,
                "version": if publish.is_null() { version } else { "0.0.0" },
                "source": null,
                "manifest_path": dir.join("Cargo.toml"),
                "targets": [
                    {"src_path": dir.join("src/lib.rs"), "edition": edition},
                    {"src_path": dir.join("src/lib.rs"), "edition": edition}
                ],
                "publish": publish,
                "rust_version": msrv,
            })
        };
        json!({
            "packages": [
                package("demo", at("."), Value::Null, json!("1.100"), "2024"),
                package("inner", at("inner"), Value::Null, json!("1.70"), "2024"),
                package("old", at("vendor/old"), json!([]), json!("1.9"), "2021"),
                package("kept", at("vendor/old/kept"), json!([]), Value::Null, "2021"),
                package("third", at("third"), json!([]), Value::Null, "2021"),
                package("outside", self.outside(), json!([]), Value::Null, "2018"),
                {
                    "id": "serde-id", "name": "serde", "version": "1.0.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "manifest_path": "/registry/serde/Cargo.toml", "targets": [],
                    "publish": null, "rust_version": "1.31"
                }
            ],
            "workspace_members": ["demo-id", "inner-id", "old-id"],
            "target_directory": root.join("target"),
            "resolve": {"nodes": [
                {"id": "demo-id", "dependencies": ["inner-id", "serde-id"]},
                {"id": "inner-id", "dependencies": ["third-id"]},
                {"id": "third-id", "dependencies": []},
                {"id": "old-id", "dependencies": ["kept-id"]},
                {"id": "serde-id", "dependencies": []}
            ]}
        })
    }

    fn env(&self) -> Environment {
        Environment {
            computer: "linux-x86_64".to_owned(),
            platform: Some(Platform::LinuxX86_64),
            windows: false,
            search_path: std::env::join_paths([
                self.base.path().join("bin"),
                self.base.path().join("utilities"),
            ])
            .unwrap(),
            folder: Some(self.base.path().join("cache")),
            protocols: "=file".to_owned(),
            answers: None,
        }
    }

    /// Runs the check over the unit `unit`: its exit code and what it printed.
    fn run_in(&self, unit: &str) -> (u8, String) {
        let env = self.env();
        let root = self.root();
        let repository = Repository::at(&root);
        let tracked: Vec<RelativePath> = self.tracked.iter().map(|file| path(file)).collect();
        let kept: Vec<RelativePath> = self.kept.iter().map(|file| path(file)).collect();
        let excluded: Vec<Excluded> = self
            .excluded
            .iter()
            .map(|(folder, reason)| Excluded {
                folder: path(folder),
                reason: Reason::new(reason).unwrap(),
            })
            .collect();
        let context = Context {
            register: &self.register,
            env: &env,
            root: &root,
            temp: &self.temp,
            packages: Packages {
                repository: &repository,
                tracked: &tracked,
                kept: &kept,
                excluded: &excluded,
                orders: &self.orders,
            },
        };
        let files: Vec<RelativePath> = self.files.iter().map(|file| path(file)).collect();
        let refs: Vec<&RelativePath> = files.iter().collect();
        let mut out = Vec::new();
        let mut console = Console::new(&mut out, self.workflow);
        let code = run::run(&context, Check::Rust, &path(unit), &refs, &mut console);
        let code = console.end(code);
        (code, support::printed(&String::from_utf8(out).unwrap()))
    }

    fn run(&self) -> (u8, String) {
        self.run_in(".")
    }

    /// What the repository's manifests hold.
    fn manifests(&self) -> Vec<String> {
        ["Cargo.toml", "Cargo.lock"]
            .iter()
            .map(|name| fs::read_to_string(self.root().join(name)).unwrap())
            .collect()
    }
}

fn has(text: &str, line: &str) {
    assert!(
        text.lines().any(|found| found == line),
        "no line {line:?} in\n{text}"
    );
}

/// The position of the first line that starts with `start`.
fn at(text: &str, start: &str) -> usize {
    text.lines()
        .position(|line| line.starts_with(start))
        .unwrap_or_else(|| panic!("no line starts with {start:?} in\n{text}"))
}

#[test]
fn every_step_runs_in_its_order_and_leaves_out_what_is_excluded() {
    let fixture = Fixture::new();
    let before = fixture.manifests();
    let (code, text) = fixture.run();
    assert_eq!(code, 0, "{text}");
    let root = fixture.root();
    let root = root.display();
    let outside = fixture.outside();
    let outside = outside.display();

    has(&text, "3 Rust file(s) to check:");
    let groups: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("::group::"))
        .collect();
    assert_eq!(
        groups,
        [
            "Install the toolchain the workspace pins",
            "Find the packages",
            "Formatting",
            "todo!, unimplemented! and dbg! stay errors",
            "Clippy, all features",
            "Clippy, no default features",
            "Tests, all features",
            "Tests, no default features",
            "Documentation without warnings",
            "Release build",
            "Oldest supported Rust version",
            "Coverage tools",
            "Regions, lines and functions are fully covered",
            "Each published package builds and tests from its own contents",
        ],
        "{text}"
    );
    assert_eq!(
        text.lines().filter(|line| *line == "::endgroup::").count(),
        groups.len()
    );

    // The toolchain the workspace pins, with rustfmt and Clippy.
    has(&text, "> rustup toolchain install");
    has(&text, "rustup toolchain install in repo");
    has(&text, "> rustup component add rustfmt clippy");
    has(&text, "rustc 1.98.1 (--version)");

    packages_named(&text, &outside.to_string());
    // Formatting: the members through cargo fmt, the others through rustfmt,
    // one call per edition, each root file once.
    has(
        &text,
        "cargo fmt --package=demo --package=inner -- --check --config disable_all_formatting=false \
         --files-with-diff in repo RUSTDOCFLAGS=unset CARGO_MANIFEST_DIR=unset",
    );
    has(
        &text,
        &format!(
            "rustfmt --edition 2018 --check --config disable_all_formatting=false \
             --files-with-diff {outside}/src/lib.rs"
        ),
    );
    has(
        &text,
        &format!(
            "rustfmt --edition 2021 --check --config disable_all_formatting=false \
             --files-with-diff {root}/vendor/old/kept/src/lib.rs"
        ),
    );
    has(
        &text,
        "Every file is formatted the way cargo fmt writes it.",
    );

    // Every step from here leaves out the excluded member.
    for line in [
        "> cargo clippy --locked --workspace --exclude=old --all-features --all-targets --no-deps \
         -- -D warnings -D clippy::todo -D clippy::unimplemented -D clippy::dbg_macro",
        "> cargo clippy --locked --workspace --exclude=old --no-default-features --all-targets \
         --no-deps -- -D warnings -D clippy::todo -D clippy::unimplemented -D clippy::dbg_macro",
        "> cargo test --locked --workspace --exclude=old --all-features",
        "> cargo test --locked --workspace --exclude=old --all-features --doc",
        "> cargo test --locked --workspace --exclude=old --no-default-features",
        "> cargo doc --locked --workspace --exclude=old --all-features --no-deps",
        "> cargo build --locked --workspace --exclude=old --all-features --release",
    ] {
        has(&text, line);
    }
    has(
        &text,
        "cargo doc --locked --workspace --exclude=old --all-features --no-deps in repo \
         RUSTDOCFLAGS=-D warnings CARGO_MANIFEST_DIR=unset",
    );

    // The oldest version a checked package claims, compared as numbers: the
    // excluded member's claim plays no part.
    has(
        &text,
        "Building with 1.70, the oldest version a package claims.",
    );
    has(&text, "> rustup toolchain install 1.70 --profile minimal");
    has(
        &text,
        "> cargo +1.70 build --locked --workspace --exclude=old --all-features",
    );

    coverage_ran(&text, &root.to_string());
    package_test_ran(&text, &root.to_string());

    // The manifests are as they were, and the folder of the copies is gone.
    assert_eq!(fixture.manifests(), before);
    assert_eq!(fs::read_dir(&fixture.temp).unwrap().count(), 0);
    assert!(
        at(&text, "::group::Coverage tools") < at(&text, "> cargo-llvm-cov"),
        "{text}"
    );
}

/// What the fixture's packages are named as.
fn packages_named(text: &str, outside: &str) {
    // The packages, each with what holds for it.
    has(text, "Checked: demo, inner");
    has(
        text,
        "Formatted only, a path dependency that is no member: kept at vendor/old/kept",
    );
    has(
        text,
        &format!("Formatted only, a path dependency that is no member: outside at {outside}"),
    );
    has(
        text,
        "Left out of every step, a member: old at vendor/old: a vendored copy",
    );
    has(
        text,
        "Left out of every step, a path dependency: third at third: code of others",
    );
}

/// What coverage prints for the fixture's workspace.
fn coverage_ran(text: &str, root: &str) {
    // Coverage on the toolchain the register pins, without the excluded
    // packages: the folder of one without packages inside, the files of one
    // with a package inside it.
    has(
        text,
        "> rustup toolchain install nightly-2026-11-14 --profile minimal --component \
         llvm-tools-preview",
    );
    has(text, "rustc 1.98.1 (+nightly-2026-11-14 --version)");
    let pattern = format!(
        "^({}$|{}/)",
        regex::escape(&format!("{root}/vendor/old/src/lib.rs")),
        regex::escape(&format!("{root}/third"))
    );
    has(text, &format!("Not counted: {pattern}"));
    has(
        text,
        &format!(
            "cargo-llvm-cov llvm-cov --locked --workspace --exclude=old --all-features \
             --ignore-filename-regex {pattern} --fail-under-lines 100 --fail-under-regions 100 \
             --fail-under-functions 100 RUSTUP_TOOLCHAIN=nightly-2026-11-14"
        ),
    );
}

/// What the package test of the fixture's workspace prints: the versions
/// written, both published packages packaged in one call with every local
/// package they reach patched in, and each copy tested with its sibling's
/// copy in place of crates.io.
fn package_test_ran(text: &str, root: &str) {
    has(text, "Wrote Cargo.toml.");
    has(
        text,
        &format!("Patched in from its folder, not packaged here: third at {root}/third"),
    );
    has(
        text,
        &format!(
            "> cargo package --locked --allow-dirty --package demo --package inner \
             --config patch.crates-io.demo.path=\"{root}\" \
             --config patch.crates-io.inner.path=\"{root}/inner\" \
             --config patch.crates-io.third.path=\"{root}/third\""
        ),
    );
    let tested = |name: &str, sibling: &str| {
        let copy = text
            .lines()
            .find(|line| {
                line.starts_with("> cargo +1.98.1-x86_64-unknown-linux-gnu test --all-features")
                    && line.contains(&format!("patch.crates-io.{sibling}.path="))
            })
            .unwrap_or_else(|| panic!("no test of {name}\n{text}"));
        assert!(
            copy.contains(&format!("{sibling}-1.2.3\""))
                && copy.ends_with(&format!(
                    "--config patch.crates-io.third.path=\"{root}/third\""
                )),
            "{copy}"
        );
        has(
            text,
            &format!(
                "cargo {} in {name}-1.2.3 RUSTDOCFLAGS=unset CARGO_MANIFEST_DIR=unset",
                copy.trim_start_matches("> cargo ")
            ),
        );
    };
    tested("demo", "inner");
    tested("inner", "demo");
    has(text, "Verified 2 package(s) outside the source tree.");
}

/// For each step, what makes it fail (part of the arguments of the call
/// that fails), the step, and how a line of its group starts.
const FAILURES: &[(&str, &str, &str)] = &[
    (
        "toolchain install",
        "Install the toolchain the workspace pins",
        "> rustup toolchain install",
    ),
    (
        "component add",
        "Install the toolchain the workspace pins",
        "> rustup component add rustfmt clippy",
    ),
    (
        "rustc --version",
        "Install the toolchain the workspace pins",
        "> rustc --version",
    ),
    (
        "metadata",
        "Find the packages",
        "cargo metadata gave nothing the check can read; what it said stands above.",
    ),
    (
        "fmt --package",
        "Formatting",
        "Formatting failed and named no file; what rustfmt said stands above.",
    ),
    (
        "clippy --locked --workspace --exclude=old --all-features",
        "Clippy, all features",
        "error: it failed",
    ),
    (
        "--no-default-features --all-targets",
        "Clippy, no default features",
        "error: it failed",
    ),
    (
        "test --locked --workspace --exclude=old --all-features",
        "Tests, all features",
        "error: it failed",
    ),
    ("--doc", "Tests, all features", "error: it failed"),
    (
        "test --locked --workspace --exclude=old --no-default-features",
        "Tests, no default features",
        "error: it failed",
    ),
    (
        "doc --locked",
        "Documentation without warnings",
        "error: it failed",
    ),
    ("--release", "Release build", "error: it failed"),
    (
        "install 1.70",
        "Oldest supported Rust version",
        "> rustup toolchain install 1.70 --profile minimal",
    ),
    (
        "+1.70 build",
        "Oldest supported Rust version",
        "error: it failed",
    ),
    (
        "llvm-tools-preview",
        "Coverage tools",
        "> rustup toolchain install nightly-2026-11-14 --profile minimal --component llvm-tools-preview",
    ),
    (
        "rustc +nightly",
        "Coverage tools",
        "> rustc +nightly-2026-11-14 --version",
    ),
    (
        "cargo-llvm-cov llvm-cov",
        "Regions, lines and functions are fully covered",
        "error: coverage is below 100%",
    ),
    (
        "package --locked",
        "Each published package builds and tests from its own contents",
        "> cargo package --locked",
    ),
    (
        " test --all-features --config patch.crates-io.inner",
        "Each published package builds and tests from its own contents",
        "error: it failed",
    ),
];

#[test]
fn the_first_step_that_fails_ends_the_check() {
    for &(pattern, step, last) in FAILURES {
        let fixture = Fixture::new();
        fixture.fail(&[pattern]);
        let before = fixture.manifests();
        let (code, text) = fixture.run();
        assert_eq!(code, 1, "{pattern}\n{text}");
        let failed = format!("{step}: failed");
        has(&text, &failed);
        assert_eq!(
            text.lines().last(),
            Some(failed.as_str()),
            "{pattern}: nothing runs after the step that failed\n{text}"
        );
        let lines: Vec<&str> = text.lines().collect();
        let end = lines
            .iter()
            .rposition(|line| *line == "::endgroup::")
            .unwrap();
        let inside = &lines[..end];
        assert!(
            inside.iter().any(|line| line.starts_with(last)),
            "{pattern}: {last:?}\n{text}"
        );
        assert_eq!(fixture.manifests(), before, "{pattern}");
    }
}

#[test]
fn an_unformatted_file_is_a_finding_on_it_and_the_differences_follow() {
    let fixture = Fixture::new();
    let root = fixture.root();
    fixture
        .mark(
            "named",
            &format!(
                "{0}/src/lib.rs\n{0}/target/generated.rs\n/elsewhere/lib.rs\n",
                root.display()
            ),
        )
        .mark("named-others", "");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=src/lib.rs::not formatted the way cargo fmt writes it",
    );
    // A file that is not one of the check's is named as it is.
    has(&text, &format!("{}/target/generated.rs", root.display()));
    has(&text, "/elsewhere/lib.rs");
    has(
        &text,
        "::error file=vendor/old/kept/src/lib.rs::not formatted the way cargo fmt writes it",
    );
    has(
        &text,
        &format!("{}/src/lib.rs", fixture.outside().display()),
    );
    // Then each call again without --files-with-diff, for the differences.
    has(
        &text,
        &format!("Diff in {}/src/lib.rs at line 1:", root.display()),
    );
    has(&text, "Diff in a path dependency");
    has(
        &text,
        &format!(
            "> rustfmt --edition 2021 --check --config disable_all_formatting=false \
             {}/vendor/old/kept/src/lib.rs",
            root.display()
        ),
    );
}

#[test]
fn only_the_files_of_the_checked_packages_are_read_for_what_lowers_the_three() {
    let fixture = Fixture::new();
    // The path dependency that is formatted only is never linted.
    fixture.file("vendor/old/kept/src/lib.rs", "#![allow(clippy::todo)]\n");
    let (code, text) = fixture.run();
    assert_eq!(code, 0, "{text}");
    has(
        &text,
        "Nothing in the 2 Rust file(s) of the checked packages and no configuration of Clippy \
         lowers todo!, unimplemented! or dbg! below an error.",
    );
    assert!(
        at(&text, "::group::todo!") < at(&text, "::group::Clippy"),
        "{text}"
    );
}

#[test]
fn what_lowers_todo_unimplemented_or_dbg_fails_before_clippy_runs() {
    let mut fixture = Fixture::new();
    fixture
        .file(
            "src/lib.rs",
            "//! The crate.\n#![allow(clippy::todo)]\n\n#[expect(clippy::dbg_macro, reason = \
             \"a trace\")]\nfn f() {}\n",
        )
        .file(
            "inner/src/lib.rs",
            "#[expect(clippy::todo, reason = \"comes with the next issue\")]\nfn f() {}\n",
        )
        .file(
            "inner/tests/t.rs",
            "#[cfg_attr(test, allow(clippy::dbg_macro))]\nfn t() {}\n",
        )
        // A path dependency that is formatted only is never linted, so what
        // it says about Clippy's lints plays no part.
        .file("vendor/old/kept/src/lib.rs", "#![allow(clippy::todo)]\n")
        .file("crates/clippy.toml", "allow-dbg-in-tests = true\n")
        .file(".clippy.toml", "msrv = \"1.85\"\n")
        // A configuration an `[[exclude]]` entry takes out is not read.
        .file("vendor/old/clippy.toml", "allow-dbg-in-tests = true\n");
    fixture.files.push("inner/tests/t.rs");
    fixture.kept = vec!["crates/clippy.toml", ".clippy.toml", "src/lib.rs"];
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=src/lib.rs,line=2,col=4::#![allow(clippy::todo)] covers the whole crate or \
         module, and the Rust check holds todo!, unimplemented! and dbg! as errors there; exempt \
         the one item with #[expect(…, reason = \"…\")] on it",
    );
    has(
        &text,
        "::error file=inner/tests/t.rs,line=1,col=18::allow(clippy::dbg_macro) lowers what the \
         Rust check holds as errors; exempt the one item with #[expect(…, reason = \"…\")] on \
         it, written as it stands",
    );
    has(
        &text,
        "::error file=crates/clippy.toml,line=1,col=22::allow-dbg-in-tests = true lets dbg! pass \
         in every test, and the Rust check holds it as an error there; exempt the one item with \
         #[expect(clippy::dbg_macro, reason = \"…\")] on it",
    );
    assert_eq!(text.matches("::error ").count(), 3, "{text}");
    has(&text, "todo!, unimplemented! and dbg! stay errors: failed");
    assert!(!text.contains("cargo clippy"), "{text}");
}

#[test]
fn a_file_the_search_cannot_read_is_a_finding_and_one_without_tokens_is_left_to_clippy() {
    let mut fixture = Fixture::new();
    fixture.file("inner/src/broken.rs", "fn f() {\n");
    // A file of a package that is out is not read.
    fixture.files.extend([
        "inner/src/broken.rs",
        "inner/src/gone.rs",
        "third/src/gone.rs",
    ]);
    fixture.kept = vec!["clippy.toml"];
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error file=inner/src/gone.rs::cannot be read: "),
        "{text}"
    );
    assert!(
        text.contains("::error file=clippy.toml::cannot be read: "),
        "{text}"
    );
    assert!(!text.contains("file=third/src/gone.rs"), "{text}");
    has(
        &text,
        "inner/src/broken.rs holds no tokens Rust could read; Clippy reports that where it \
         compiles the file.",
    );
    has(&text, "todo!, unimplemented! and dbg! stay errors: failed");
}

#[test]
fn a_file_belongs_to_the_package_of_the_nearest_manifest_above_it() {
    let local = |dir: &str| Local {
        name: dir.to_owned(),
        dir: dir.to_owned(),
        member: true,
        nested: Vec::new(),
        reason: None,
        targets: Vec::new(),
        rust_version: None,
    };
    let locals = [local("."), local("crates/a"), local("/outside")];
    let tracked: Vec<RelativePath> = [
        "Cargo.toml",
        "crates/a/Cargo.toml",
        "crates/a/fixtures/project/Cargo.toml",
        "crates/a/src/lib.rs",
        "crates/NotCargo.toml",
        "README.md",
    ]
    .iter()
    .map(|file| path(file))
    .collect();
    let manifests = rust::manifest_folders(&tracked);
    assert_eq!(
        manifests.iter().map(String::as_str).collect::<Vec<_>>(),
        [".", "crates/a", "crates/a/fixtures/project"]
    );
    let owner = |file: &str| rust::owner(&locals, &manifests, file).map(|local| local.dir.as_str());
    assert_eq!(owner("build.rs"), Some("."));
    assert_eq!(owner("src/lib.rs"), Some("."));
    assert_eq!(owner("crates/b/src/lib.rs"), Some("."));
    assert_eq!(owner("crates/a/src/lib.rs"), Some("crates/a"));
    assert_eq!(owner("crates/a/tests/deep/t.rs"), Some("crates/a"));
    // A project Cargo does not see is no package of the workspace.
    assert_eq!(owner("crates/a/fixtures/project/src/lib.rs"), None);
    // Nor is the manifest of a workspace alone.
    assert_eq!(rust::owner(&locals[1..], &manifests, "src/lib.rs"), None);
    assert_eq!(rust::owner(&locals, &BTreeSet::new(), "src/lib.rs"), None);
}

#[test]
fn outside_a_workflow_run_a_finding_is_a_line() {
    let mut fixture = Fixture::new();
    fixture.workflow = false;
    let root = fixture.root();
    fixture.mark("named", &format!("{}/src/lib.rs\n", root.display()));
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "src/lib.rs: error: not formatted the way cargo fmt writes it",
    );
    has(&text, "Formatting");
    assert!(!text.contains("::"), "{text}");
    // The line that the gate decides belongs to the command, not to the check.
    assert!(!text.contains(run::OUTSIDE), "{text}");
}

#[test]
fn a_program_that_cannot_be_started_fails_its_step() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.base.path().join("bin/rustup")).unwrap();
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    assert!(
        text.lines()
            .any(|line| line.starts_with("::error::rustup cannot be started: ")),
        "{text}"
    );

    // Formatting, when rustfmt is missing: what cargo fmt hands over still
    // passes, the path dependencies cannot be checked.
    let fixture = Fixture::new();
    fs::remove_file(fixture.base.path().join("bin/rustfmt")).unwrap();
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(&text, "Formatting: failed");
    has(
        &text,
        "Formatting failed and named no file; what rustfmt said stands above.",
    );
    assert!(
        text.lines()
            .any(|line| line.starts_with("::error::rustfmt cannot be started: ")),
        "{text}"
    );
}

#[test]
fn metadata_that_does_not_read_fails_the_check() {
    let fixture = Fixture::new();
    fixture.mark("metadata", "not JSON");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "cargo metadata gave nothing the check can read; what it said stands above.",
    );
    has(&text, "Find the packages: failed");
}

#[test]
fn a_workspace_whose_members_are_all_excluded_is_only_formatted() {
    let mut fixture = Fixture::new();
    fixture
        .excluded
        .extend([("inner", "generated"), (".", "generated")]);
    let (code, text) = fixture.run();
    assert_eq!(code, 0, "{text}");
    has(&text, "Checked: ");
    has(
        &text,
        "Every member of the workspace is excluded, so there is nothing to check.",
    );
    has(&text, "::group::Formatting");
    assert!(!text.contains("cargo fmt --package"), "{text}");
    assert!(text.contains("rustfmt --edition 2021"), "{text}");
    assert!(!text.contains("Clippy"), "{text}");

    // With every local package out there is nothing to format either.
    let mut fixture = Fixture::new();
    fixture.excluded = vec![
        (".", "generated"),
        ("inner", "generated"),
        ("vendor/old", "generated"),
        ("vendor/old/kept", "generated"),
        ("third", "generated"),
    ];
    let outside = fixture.outside();
    let metadata = fixture.metadata("0.0.0");
    let mut packages = metadata["packages"].as_array().unwrap().clone();
    packages.retain(|package| package["manifest_path"] != json!(outside.join("Cargo.toml")));
    let mut metadata = metadata.clone();
    metadata["packages"] = Value::Array(packages);
    fixture.mark("metadata", &metadata.to_string());
    let (code, text) = fixture.run();
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("::group::Formatting"), "{text}");
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("::group::"))
            .count(),
        2,
        "{text}"
    );
}

#[test]
fn without_a_rust_version_there_is_no_oldest_version_to_build_with() {
    // Nothing excluded either: every step takes the whole workspace, and
    // coverage counts every file.
    let mut fixture = Fixture::new();
    fixture.excluded.clear();
    let mut metadata = fixture.metadata("0.0.0");
    for package in metadata["packages"].as_array_mut().unwrap() {
        package["rust_version"] = Value::Null;
    }
    fixture.mark("metadata", &metadata.to_string());
    let (code, text) = fixture.run();
    assert_eq!(code, 0, "{text}");
    has(
        &text,
        "No package claims a rust-version, so there is no oldest version to build with.",
    );
    assert!(!text.contains("Not counted"), "{text}");
    assert!(!text.contains("--exclude="), "{text}");
    has(&text, "> cargo test --locked --workspace --all-features");
}

#[test]
fn coverage_needs_its_toolchain_in_the_register_and_cargo_llvm_cov() {
    let mut fixture = Fixture::new();
    for (name, check) in &mut fixture.register.check {
        if name.as_str() == "rust" {
            check.coverage_toolchain = None;
        }
    }
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::the register pins no coverage toolchain for the rust check",
    );
    has(&text, "Coverage tools: failed");

    let fixture = Fixture::new();
    fs::remove_file(fixture.base.path().join("bin/cargo-llvm-cov")).unwrap();
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::cargo-llvm-cov 0.9.0 is neither pinned in the tool's folder nor on the \
         computer; `nebensound-repo setup` installs it",
    );
}

#[test]
fn a_workspace_that_publishes_nothing_verifies_nothing() {
    let fixture = Fixture::new();
    let mut after = fixture.metadata("1.2.3");
    for package in after["packages"].as_array_mut().unwrap() {
        package["publish"] = json!([]);
    }
    fixture.mark("after", &after.to_string());
    let (code, text) = fixture.run();
    assert_eq!(code, 0, "{text}");
    has(
        &text,
        "Nothing is published from here, so there is nothing to verify.",
    );
    assert!(!text.contains("cargo package"), "{text}");
}

#[test]
fn what_keeps_the_package_test_from_its_end_fails_it_and_the_manifests_go_back() {
    // The writer refuses a unit without a package.
    let mut fixture = Fixture::new();
    fixture.orders[0].path = path("inner");
    fixture.file("inner/Cargo.toml", "# no package\n");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "the repository selects the version writer cargo, and inner/Cargo.toml has no [package] \
         with a name: there is no package to give the version to",
    );

    // The metadata after writing does not read.
    let fixture = Fixture::new();
    fixture.mark("after", "garbage");
    let before = fixture.manifests();
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(&text, "Wrote Cargo.toml.");
    assert_eq!(fixture.manifests(), before);

    // There is no folder for the copies.
    let mut fixture = Fixture::new();
    fixture.temp = fixture.base.path().join("no such folder");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::no folder can be made in "),
        "{text}"
    );

    // A file of a package cannot be copied.
    let fixture = Fixture::new();
    fixture.mark("unreadable", "");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    let root = fixture.root();
    assert!(
        text.contains(&format!(
            "::error::the package demo 1.2.3 cannot be copied out of {}/target/package/demo-1.2.3: ",
            root.display()
        )),
        "{text}"
    );
    fs::set_permissions(
        root.join("target/package/demo-1.2.3/src/lib.rs"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();

    // rustup names no toolchain.
    let fixture = Fixture::new();
    fixture.mark("no-toolchain", "");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "rustup names no active toolchain for the workspace, so no copy can be tested.",
    );

    // A manifest cannot be put back.
    let fixture = Fixture::new();
    fixture.mark("read-only", "");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error file=Cargo.toml::cannot be put back as it is committed: "),
        "{text}"
    );
    has(&text, "Verified 2 package(s) outside the source tree.");
    let lock = fs::read_to_string(fixture.root().join("Cargo.lock")).unwrap();
    assert!(lock.contains("version = \"0.0.0\""), "{lock}");
}

#[test]
fn a_unit_below_the_root_runs_in_its_own_folder() {
    let mut fixture = Fixture::new();
    fixture.orders.clear();
    fixture.files = vec!["inner/src/lib.rs"];
    let mut metadata = fixture.metadata("0.0.0");
    for package in metadata["packages"].as_array_mut().unwrap() {
        package["publish"] = json!([]);
    }
    fixture.mark("metadata", &metadata.to_string());
    let (code, text) = fixture.run_in("inner");
    assert_eq!(code, 0, "{text}");
    has(&text, "rustup toolchain install in inner");
    has(&text, "1 Rust file(s) to check:");
    has(
        &text,
        "Nothing to write: every file already holds what it should.",
    );
    has(
        &text,
        "Nothing is published from here, so there is nothing to verify.",
    );
}

#[test]
fn nothing_to_check_is_said_and_passes() {
    let mut fixture = Fixture::new();
    fixture.files.clear();
    let (code, text) = fixture.run();
    assert_eq!(code, 0, "{text}");
    assert_eq!(text, "No Rust file below '.'; there is nothing to check.\n");
}

#[test]
fn only_what_steers_the_toolchain_is_taken_out() {
    for name in [
        "RUSTFLAGS",
        "RUSTDOCFLAGS",
        "RUSTUP_TOOLCHAIN",
        "RUSTC_WRAPPER",
        "RUST_BACKTRACE",
        "CARGO",
        "CARGO_TARGET_DIR",
        "CARGO_INCREMENTAL",
        "CARGO_LLVM_COV_TARGET_DIR",
    ] {
        assert!(toolchain::steers(OsStr::new(name)), "{name}");
    }
    for name in [
        "CARGO_HOME",
        "RUSTUP_HOME",
        "PATH",
        "HOME",
        "TMPDIR",
        "XCARGO",
    ] {
        assert!(!toolchain::steers(OsStr::new(name)), "{name}");
    }
}

#[test]
fn versions_compare_as_numbers() {
    assert_eq!(rust::numbers("1.70.0"), [1, 70, 0]);
    assert_eq!(rust::numbers("1.x"), [1, 0]);
    let local = |name: &str, version: Option<&str>, member: bool, reason: Option<&str>| Local {
        name: name.to_owned(),
        dir: name.to_owned(),
        member,
        nested: Vec::new(),
        reason: reason.map(str::to_owned),
        targets: Vec::new(),
        rust_version: version.map(str::to_owned),
    };
    let locals = [
        local("a", Some("1.100"), true, None),
        local("b", Some("1.9"), true, Some("out")),
        local("c", Some("1.8"), false, None),
        local("d", None, true, None),
        local("e", Some("1.70"), true, None),
    ];
    assert_eq!(rust::oldest(&locals), Some("1.70"));
    assert_eq!(rust::oldest(&locals[1..4]), None);
}

#[test]
fn what_coverage_leaves_out_is_matched_from_the_start() {
    let root = Path::new("/w.s/repo");
    let local = |dir: &str, nested: &[&str], reason: Option<&str>| Local {
        name: dir.to_owned(),
        dir: dir.to_owned(),
        member: true,
        nested: nested.iter().map(|dir| (*dir).to_owned()).collect(),
        reason: reason.map(str::to_owned),
        targets: Vec::new(),
        rust_version: None,
    };
    assert_eq!(rust::uncounted(root, &[local("a", &[], None)], &[]), None);
    assert_eq!(
        rust::uncounted(root, &[local(".", &[], Some("all"))], &[]),
        Some(r"^(/w\.s/repo/)".to_owned())
    );
    let tracked = [
        path("main.rs"),
        path("src/x+y.rs"),
        path("src/notes.md"),
        path("inner/lib.rs"),
        path("other/lib.rs"),
    ];
    assert_eq!(
        rust::uncounted(root, &[local(".", &["inner"], Some("all"))], &tracked),
        Some(
            r"^(/w\.s/repo/main\.rs$|/w\.s/repo/src/x\+y\.rs$|/w\.s/repo/other/lib\.rs$)"
                .to_owned()
        )
    );
}

#[test]
fn local_packages_are_named_by_their_folder_from_the_root() {
    let metadata: Metadata = serde_json::from_value(json!({
        "packages": [
            {"id": "r", "name": "r", "version": "0.0.0", "source": null,
             "manifest_path": "/repo/Cargo.toml", "targets": [], "publish": null,
             "rust_version": null},
            {"id": "o", "name": "o", "version": "0.0.0", "source": null,
             "manifest_path": "/else/o/Cargo.toml", "targets": [], "publish": null,
             "rust_version": null}
        ],
        "workspace_members": ["r"],
        "target_directory": "/repo/target",
        "resolve": null
    }))
    .unwrap();
    let locals = rust::locals(
        &metadata,
        Path::new("/repo"),
        &[("/else/o".to_owned(), "elsewhere".to_owned())],
    );
    assert_eq!(locals[0].dir, ".");
    // As the block had it: below the root lies every other local package.
    assert_eq!(locals[0].nested, ["/else/o"]);
    assert_eq!(locals[1].dir, "/else/o");
    assert_eq!(locals[1].reason.as_deref(), Some("elsewhere"));
    // Without a resolve, only the packages named are reached.
    assert_eq!(
        rust::reached(&metadata, &["r".to_owned()]),
        [(true, "r".to_owned(), "/repo".to_owned())]
    );
    // The folder stands as a TOML string, whatever it holds.
    for folder in ["/x/plain", "/x/it's \"here\"", "/x/back\\slash"] {
        let patch = rust::patch("a", folder);
        let (key, value) = patch.split_once('=').unwrap();
        assert_eq!(key, "patch.crates-io.a.path");
        let table: toml::Table = toml::from_str(&format!("p = {value}")).unwrap();
        assert_eq!(table["p"].as_str(), Some(folder), "{patch}");
    }
}

#[test]
fn the_call_hands_the_check_its_plan_or_resolves_one() {
    let fine = support::repository(&[(".nebensound/repo.toml", QUIET)]);
    let root = fine.path().to_str().unwrap();
    // A plan that does not read is refused before anything runs.
    let missing = fine.path().join("no-plan.json");
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root,
        "run",
        "--plan",
        missing.to_str().unwrap(),
        "rust",
        ".",
    ]);
    assert_eq!(code, 1, "{text}");
    // Outside a workflow run the call opens with the line that the gate
    // decides, also when it ends before the check starts.
    assert!(
        text.starts_with(&format!(
            "{}\nthe plan {} does not read: ",
            run::OUTSIDE,
            missing.display()
        )),
        "{text}"
    );
    // Without one, the versions are resolved from the history, which a
    // repository without a commit does not have.
    let (text, code) = call(&["nebensound-repo", "-C", root, "run", "rust", "."]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(text.lines().next(), Some(run::OUTSIDE), "{text}");
    assert_eq!(text.lines().count(), 2, "{text}");
    let plan = fine.path().join("plan.json");
    fs::write(&plan, "{\"versions\":[]}").unwrap();
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root,
        "run",
        "--plan",
        plan.to_str().unwrap(),
        "rust",
        ".",
    ]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(text.lines().next(), Some(run::OUTSIDE), "{text}");
    assert!(
        text.contains("the plan does not start the rust check at ."),
        "{text}"
    );
}

/// The metadata of the one package `demo` at `root`, at this version.
fn demo_metadata(root: &Path, version: &str) -> Value {
    json!({
        "packages": [{
            "id": "demo-id", "name": "demo", "version": version, "source": null,
            "manifest_path": root.join("Cargo.toml"),
            "targets": [{"src_path": root.join("src/lib.rs"), "edition": "2024"}],
            "publish": null, "rust_version": null
        }],
        "workspace_members": ["demo-id"],
        "target_directory": root.join("target"),
        "resolve": {"nodes": [{"id": "demo-id", "dependencies": []}]}
    })
}

/// The programs above in a folder of their own, beside a repository with
/// history: one release, and a pull request checked out as GitHub checks one
/// out, the merge commit, detached.
fn pull_request_repository() -> (TempDir, support::Git) {
    let base = tempfile::tempdir().unwrap();
    let base_path = fs::canonicalize(base.path()).unwrap();
    for (name, script) in [
        ("cargo", CARGO),
        ("rustup", RUSTUP),
        ("rustc", RUSTC),
        ("rustfmt", RUSTFMT),
        ("cargo-llvm-cov", LLVM_COV),
    ] {
        executable(
            &base_path.join("bin").join(name),
            format!("#!/bin/sh\n{FAILING}{script}"),
        );
    }
    let computer = std::env::var_os("PATH").unwrap_or_default();
    fs::create_dir_all(base_path.join("utilities")).unwrap();
    for utility in [
        "basename", "cat", "chmod", "dirname", "grep", "head", "mkdir",
    ] {
        let found = find::search(utility, &computer, false).unwrap();
        std::os::unix::fs::symlink(found, base_path.join("utilities").join(utility)).unwrap();
    }
    fs::create_dir_all(base_path.join("home")).unwrap();
    let git = support::Git::new();
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
        (
            "Cargo.lock",
            "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.0.0\"\n",
        ),
        ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.98.1\"\n"),
        ("src/lib.rs", "//! A library.\n"),
    ]);
    let released = git.commit("feat: start");
    git.tag("v0.0.1", &released);
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
    (base, git)
}

#[test]
fn without_a_plan_a_pull_request_is_read_from_the_event_as_the_plan_reads_it() {
    let (base, git) = pull_request_repository();
    let base_path = fs::canonicalize(base.path()).unwrap();
    let root = fs::canonicalize(git.path()).unwrap();
    let root_text = root.to_str().unwrap();

    let (plan, code) = call(&[
        "nebensound-repo",
        "-C",
        root_text,
        "plan",
        "--pull-request",
        "4",
        "--message",
        "fix: change the library",
    ]);
    assert_eq!(code, 0, "{plan}");
    let parsed: Value = serde_json::from_str(&plan).unwrap();
    let version = parsed["versions"][0]["version"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(version, "0.0.2-pr.4");
    let file = base_path.join("plan.json");
    fs::write(&file, &plan).unwrap();
    fs::write(
        base_path.join("metadata"),
        demo_metadata(&root, "0.0.0").to_string(),
    )
    .unwrap();
    fs::write(
        base_path.join("after"),
        demo_metadata(&root, &version).to_string(),
    )
    .unwrap();
    fs::write(base_path.join("packaged"), format!("demo-{version}\n")).unwrap();

    let search_path =
        std::env::join_paths([base_path.join("bin"), base_path.join("utilities")]).unwrap();
    let home = base_path.join("home");
    let check = |args: &[&str], more: &[(&str, &str)]| {
        let mut variables = vec![
            ("PATH", search_path.to_str().unwrap()),
            ("HOME", home.to_str().unwrap()),
        ];
        variables.extend_from_slice(more);
        let mut all = vec!["nebensound-repo", "-C", root_text, "run"];
        all.extend_from_slice(args);
        all.extend(["rust", "."]);
        let (text, code) = support::call_with(&all, &variables, Output::Workflow);
        (code, support::printed(&text))
    };
    let written = format!("the repository: the package demo at . gets {version}");

    let (code, from_plan) = check(&["--plan", file.to_str().unwrap()], &[]);
    assert_eq!(code, 0, "{from_plan}");
    assert!(from_plan.contains(&written), "{from_plan}");
    let (code, from_event) = check(
        &[],
        &[
            ("NEBENSOUND_PULL_REQUEST", "4"),
            ("NEBENSOUND_TITLE", "fix: change the library"),
        ],
    );
    assert_eq!(code, 0, "{from_event}");
    assert!(from_event.contains(&written), "{from_event}");
    // The rest of the two logs differs only in the folders the check makes.
    let lines = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| !line.contains("nebensound-repo-run-"))
            .map(ToOwned::to_owned)
            .collect()
    };
    assert_eq!(lines(&from_event), lines(&from_plan));
    // The manifests are put back as they are committed.
    assert_eq!(
        fs::read_to_string(root.join("Cargo.toml")).unwrap(),
        "[package]\nname = \"demo\"\nedition = \"2024\"\n"
    );
    // Without the event's pull request the merge commit is read as it
    // stands: a version without `-pr.4`.
    let (_, unread) = check(&[], &[]);
    assert!(
        unread.contains("the repository: the package demo at . gets 0.0.2 ("),
        "{unread}"
    );
    // A number that is none is a misuse, and nothing is checked.
    let (code, text) = check(&[], &[("NEBENSOUND_PULL_REQUEST", "x")]);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("NEBENSOUND_PULL_REQUEST is \"x\""), "{text}");
    assert!(!text.contains("Rust file(s)"), "{text}");
}
