//! `nebensound-repo build cargo-package`: the build of one Cargo package, run
//! from the tool.
//!
//! No real toolchain runs here. `cargo`, `rustup` and `rustc` are shell
//! scripts that print how they were called and act the way the fixture tells
//! them: `cargo metadata` prints the metadata the fixture wrote, `cargo
//! package` leaves the crate the fixture made where Cargo would, and a call
//! fails when a line of the fixture's `fail` file is part of its arguments.
//! That the real toolchain builds what the block built is shown by the cases
//! of the block, which `cargo xtask check --all` runs through `build`. Hence
//! Unix only: the gate runs on Linux.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use nebensound_repo::Register;
use nebensound_repo::build::cargo_package::{artifact, encoded};
use nebensound_repo::build::{self, Build, Context, Machine};
use nebensound_repo::output::Output;
use nebensound_repo::path::RelativePath;
use nebensound_repo::repository::Repository;
use nebensound_repo::run;
use serde_json::{Value, json};
use support::{Git, call_with};
use tempfile::TempDir;

/// Held by every test of this file that starts the programs below: Linux
/// refuses to start a file another process holds open for writing.
static ALONE: Mutex<()> = Mutex::new(());

fn alone() -> MutexGuard<'static, ()> {
    ALONE.lock().unwrap_or_else(PoisonError::into_inner)
}

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

/// What every program says first: whether a variable that steers the toolchain
/// reached it. The test process has `CARGO_MANIFEST_DIR`, since Cargo sets it
/// for every test it runs, and the build takes it out with every other
/// variable whose name starts with `CARGO` or `RUST`.
const SEES: &str = r#"echo "$(basename "$0") sees CARGO_MANIFEST_DIR=${CARGO_MANIFEST_DIR-unset}" >&2
"#;

const CARGO: &str = r#"
echo "cargo $* in $(basename "$(pwd -P)")" >&2
failing "cargo $*" && { echo "error: it failed" >&2; exit 101; }
case "$1" in
  metadata)
    case "$*" in
      *--no-deps*) cat "$here/metadata" ;;
      *) cat "$here/metadata-all" ;;
    esac ;;
  package)
    if [ -f "$here/crate" ]; then
      mkdir -p "$here/target/package"
      cat "$here/crate" > "$here/target/package/$(cat "$here/crate-name")"
    fi ;;
esac
exit 0
"#;

const RUSTUP: &str = r#"
echo "rustup $* in $(basename "$(pwd -P)")" >&2
failing "rustup $*" && exit 1
if [ "$1 $2" = "show active-toolchain" ]; then
  [ -f "$here/no-toolchain" ] || echo "1.98.1-x86_64-unknown-linux-gnu (overridden by a file)"
fi
exit 0
"#;

const RUSTC: &str = r#"
echo "rustc 1.98.1"
failing "rustc $*" && exit 1
exit 0
"#;

fn executable(file: &Path, script: &str) {
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, script).unwrap();
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A `.crate`: a tar archive compressed with gzip, of these files.
fn crate_of(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, text) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(text.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, path, text.as_bytes())
            .unwrap();
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &builder.into_inner().unwrap()).unwrap();
    encoder.finish().unwrap()
}

/// A repository with one commit, the programs above and what they are told.
struct Fixture {
    base: TempDir,
    git: Git,
}

impl Fixture {
    /// The workspace with the package `app` at `app`, which depends on `lib`
    /// at `lib` by path, and `lib`, which depends on nothing local; `app`
    /// packages at 1.2.3.
    fn new() -> Self {
        let base = tempfile::tempdir().unwrap();
        let bin = base.path().join("bin");
        for (name, script) in [("cargo", CARGO), ("rustup", RUSTUP), ("rustc", RUSTC)] {
            executable(
                &bin.join(name),
                &format!("#!/bin/sh\n{FAILING}{SEES}{script}"),
            );
        }
        let utilities = base.path().join("utilities");
        fs::create_dir_all(&utilities).unwrap();
        let computer = std::env::var_os("PATH").unwrap_or_default();
        for utility in ["basename", "cat", "dirname", "mkdir"] {
            let found = nebensound_repo::setup::find::search(utility, &computer, false).unwrap();
            std::os::unix::fs::symlink(found, utilities.join(utility)).unwrap();
        }
        fs::create_dir_all(base.path().join("tmp")).unwrap();
        let git = Git::new();
        git.write(&[
            (".nebensound/repo.toml", DECLARATION),
            ("Cargo.toml", "[workspace]\nmembers = [\"app\", \"lib\"]\n"),
            ("app/Cargo.toml", "[package]\nname = \"app\"\n"),
            ("lib/Cargo.toml", "[package]\nname = \"lib\"\n"),
        ]);
        git.commit("chore: start");
        let fixture = Self { base, git };
        fixture.metadata();
        fixture.packaged(&[
            (
                "app-1.2.3/Cargo.toml",
                "[package]\nname = \"app\"\nversion = \"1.2.3\"\n",
            ),
            ("app-1.2.3/src/lib.rs", "pub fn f() {}\n"),
        ]);
        fixture
    }

    fn root(&self) -> PathBuf {
        fs::canonicalize(self.git.path()).unwrap()
    }

    fn base(&self) -> PathBuf {
        fs::canonicalize(self.base.path()).unwrap()
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

    /// The metadata `cargo metadata` prints.
    fn metadata(&self) {
        let root = self.root();
        let package = |name: &str, dir: &str| {
            json!({
                "id": format!("{name}-id"), "name": name, "version": "1.2.3", "source": null,
                "manifest_path": root.join(dir).join("Cargo.toml"),
                "targets": [], "publish": null, "rust_version": null
            })
        };
        let metadata = |resolve: Value| {
            json!({
                "packages": [
                    package("app", "app"),
                    package("lib", "lib"),
                    {
                        "id": "serde-id", "name": "serde", "version": "1.0.0",
                        "source": "registry+https://github.com/rust-lang/crates.io-index",
                        "manifest_path": "/registry/serde/Cargo.toml", "targets": [],
                        "publish": null, "rust_version": null
                    }
                ],
                "workspace_members": ["app-id", "lib-id"],
                "target_directory": self.base().join("target"),
                "resolve": resolve
            })
            .to_string()
        };
        self.mark("metadata", &metadata(Value::Null));
        self.mark(
            "metadata-all",
            &metadata(json!({"nodes": [
                {"id": "app-id", "dependencies": ["lib-id", "serde-id"]},
                {"id": "lib-id", "dependencies": []},
                {"id": "serde-id", "dependencies": []}
            ]})),
        );
    }

    /// The crate `cargo package` leaves, holding these files.
    fn packaged(&self, entries: &[(&str, &str)]) -> &Self {
        fs::write(self.base.path().join("crate"), crate_of(entries)).unwrap();
        self.mark("crate-name", "app-1.2.3.crate")
    }

    fn search_path(&self) -> std::ffi::OsString {
        std::env::join_paths([self.base().join("bin"), self.base().join("utilities")]).unwrap()
    }

    /// Builds the unit `unit` in a run whose outputs go to `outputs`: the exit
    /// code and what it printed.
    fn build_in(&self, unit: &str, temp: &Path, outputs: &Path, workflow: bool) -> (u8, String) {
        let root = self.root();
        let repository = Repository::at(&root);
        let search_path = self.search_path();
        let variables = BTreeMap::from([(
            "GITHUB_OUTPUT".to_owned(),
            outputs.to_string_lossy().into_owned(),
        )]);
        let read = |name: &str| variables.get(name).cloned();
        let register = Register::builtin().unwrap();
        let env = nebensound_repo::setup::Environment::current();
        let context = Context {
            repository: &repository,
            root: &root,
            search_path: &search_path,
            temp,
            workflow,
            variables: &read,
            register: &register,
            env: &env,
            machine: &Machine::current(),
            component: Some("app"),
            version: None,
        };
        let mut out = Vec::new();
        let code = build::run(
            &context,
            Build::CargoPackage,
            &RelativePath::new(unit).unwrap(),
            &mut out,
        );
        (code, String::from_utf8(out).unwrap())
    }

    fn build(&self, unit: &str) -> (u8, String) {
        self.build_in(
            unit,
            &self.base().join("tmp"),
            &self.base().join("outputs"),
            true,
        )
    }

    fn outputs(&self) -> String {
        fs::read_to_string(self.base().join("outputs")).unwrap_or_default()
    }
}

/// The declaration of the fixture: `app` builds cargo-package, `lib` does not.
const DECLARATION: &str = r#"[repo]
audience = "public"
pipeline = "none"

[component.app]
path     = "app"
pipeline = { version = ["cargo"], build = "cargo-package" }

[component.lib]
path     = "lib"
pipeline = { version = ["cargo"] }
"#;

fn has(text: &str, line: &str) {
    assert!(
        text.lines().any(|found| found == line),
        "no line {line:?} in\n{text}"
    );
}

#[test]
fn a_build_is_named_as_the_register_names_it() {
    assert_eq!(Build::of("cargo-package"), Some(Build::CargoPackage));
    assert_eq!(Build::CargoPackage.name(), "cargo-package");
    assert!(!Build::CargoPackage.labels_version());
    assert_eq!(Build::of("docker"), Some(Build::Docker));
    assert_eq!(Build::Docker.name(), "docker");
    assert!(Build::Docker.labels_version());
    assert_eq!(Build::of("nonsense"), None);
}

#[test]
fn the_artifact_carries_the_path_encoded_the_version_and_the_commit() {
    assert_eq!(encoded("."), ".");
    assert_eq!(encoded("tools/foo"), "tools%2Ffoo");
    assert_eq!(encoded("a b@c~d_e-f"), "a%20b%40c~d_e-f");
    assert_eq!(encoded("ä"), "%C3%A4");
    assert_eq!(
        artifact("tools/foo", "1.2.3-pr.4", "abc"),
        "crate-tools%2Ffoo@1.2.3-pr.4@abc"
    );
}

#[test]
fn every_step_runs_in_its_order_and_names_the_artifact() {
    let _alone = alone();
    let fixture = Fixture::new();
    let (code, text) = fixture.build("app");
    assert_eq!(code, 0, "{text}");
    let root = fixture.root().display().to_string();
    let head = fixture.git.head();
    let artifact = format!("crate-app@1.2.3@{head}");
    let crate_file = fixture.base().join("target/package/app-1.2.3.crate");
    for line in [
        "::group::Install the toolchain the workspace pins",
        "> rustup toolchain install",
        "rustup toolchain install in app",
        "> rustc --version",
        "::group::Find the package of the unit",
        "cargo metadata --locked --format-version 1 --no-deps in app",
        "cargo metadata --locked --format-version 1 --all-features in app",
        &format!("'app' is the package app 1.2.3; its artifact is '{artifact}'."),
        "::group::Find the local packages it depends on",
        &format!("Patched in from its folder: lib at {root}/lib"),
        "::group::Package the crate",
        &format!(
            "> cargo package --locked --allow-dirty --package app --config \
             patch.crates-io.lib.path=\"{root}/lib\""
        ),
        "::group::Test the crate from its own contents",
        "The packaged manifest names version = \"1.2.3\".",
        &format!(
            "> cargo +1.98.1-x86_64-unknown-linux-gnu test --all-features --config \
             patch.crates-io.lib.path=\"{root}/lib\""
        ),
    ] {
        has(&text, line);
    }
    let digest = text
        .lines()
        .find(|line| line.ends_with(&format!("  {}", crate_file.display())))
        .unwrap();
    assert_eq!(
        digest.split_whitespace().next().unwrap().len(),
        64,
        "{digest}"
    );
    assert!(!text.contains("failed"), "{text}");
    assert_eq!(
        fixture.outputs(),
        format!("artifact={artifact}\ncrate={}\n", crate_file.display())
    );
    // The crate is unpacked into a folder of its own, which goes again.
    assert_eq!(fs::read_dir(fixture.base().join("tmp")).unwrap().count(), 0);
}

#[test]
fn every_program_starts_without_the_variables_that_steer_the_toolchain() {
    let _alone = alone();
    assert!(
        std::env::var_os("CARGO_MANIFEST_DIR").is_some(),
        "the test needs a variable the build takes out; Cargo sets this one for its tests"
    );
    let fixture = Fixture::new();
    let (code, text) = fixture.build("app");
    assert_eq!(code, 0, "{text}");
    let seen: Vec<&str> = text
        .lines()
        .filter(|line| line.contains(" sees "))
        .collect();
    assert_eq!(
        seen,
        [
            // rustup toolchain install, rustc --version
            "rustup sees CARGO_MANIFEST_DIR=unset",
            "rustc sees CARGO_MANIFEST_DIR=unset",
            // cargo metadata twice, cargo package
            "cargo sees CARGO_MANIFEST_DIR=unset",
            "cargo sees CARGO_MANIFEST_DIR=unset",
            "cargo sees CARGO_MANIFEST_DIR=unset",
            // rustup show active-toolchain, cargo test on the unpacked crate
            "rustup sees CARGO_MANIFEST_DIR=unset",
            "cargo sees CARGO_MANIFEST_DIR=unset",
        ],
        "{text}"
    );
}

#[test]
fn a_package_without_local_dependencies_patches_nothing() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .mark("crate-name", "lib-1.2.3.crate")
        .packaged(&[("lib-1.2.3/Cargo.toml", "[package]\nname = \"lib\"\n")]);
    fixture.mark("crate-name", "lib-1.2.3.crate");
    let (code, text) = fixture.build_in(
        "lib",
        &fixture.base().join("tmp"),
        &fixture.base().join("outputs"),
        false,
    );
    assert_eq!(code, 0, "{text}");
    has(&text, "It depends on no other local package.");
    has(
        &text,
        "> cargo package --locked --allow-dirty --package lib",
    );
    // The manifest names no version, and nothing is said of one.
    assert!(!text.contains("The packaged manifest names"), "{text}");
    // Outside a workflow run a group is its name.
    has(&text, "Install the toolchain the workspace pins");
    assert!(!text.contains("::group::"), "{text}");
}

#[test]
fn the_first_step_that_fails_ends_the_build() {
    let _alone = alone();
    for (pattern, step, said) in [
        (
            "rustup toolchain install",
            "Install the toolchain the workspace pins",
            "",
        ),
        (
            "rustc --version",
            "Install the toolchain the workspace pins",
            "",
        ),
        (
            "--no-deps",
            "Find the package of the unit",
            "cargo metadata failed (",
        ),
        (
            "--all-features",
            "Find the local packages it depends on",
            "cargo metadata failed (",
        ),
        ("cargo package", "Package the crate", "error: it failed"),
        (
            "rustup show",
            "Test the crate from its own contents",
            "::error::rustup names no active toolchain for the workspace, so the crate cannot \
             be tested.",
        ),
        (
            "test --all-features",
            "Test the crate from its own contents",
            "::error::The crate app 1.2.3 fails its tests from its own contents. A file its \
             tests need may be left out of the package, or a dev-dependency may have no \
             version, which cargo package drops.",
        ),
    ] {
        let fixture = Fixture::new();
        fixture.fail(&[pattern]);
        let (code, text) = fixture.build("app");
        assert_eq!(code, 1, "{pattern}: {text}");
        has(&text, &format!("{step}: failed"));
        if !said.is_empty() {
            assert!(text.contains(said), "{pattern}: {text}");
        }
        assert_eq!(fixture.outputs(), "", "{pattern}");
    }
}

#[test]
fn what_keeps_the_package_from_being_found_is_named() {
    let _alone = alone();
    // No Cargo.toml at the unit.
    let fixture = Fixture::new();
    fixture.git.write(&[("nowhere/README.md", "# x\n")]);
    let (code, text) = fixture.build("nowhere");
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::'nowhere' builds cargo-package but holds no Cargo.toml.",
    );

    // A workspace without a package of its own.
    let (code, text) = fixture.build(".");
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::The Cargo.toml of '.' is a workspace without a package of its own, so there \
         is no one crate to build.",
    );

    // Metadata the build cannot read.
    fixture.mark("metadata", "not JSON");
    let (code, text) = fixture.build("app");
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::cargo metadata printed nothing the build can read.",
    );

    // A repository without a commit.
    let fixture = Fixture::new();
    fixture.git.run(&["update-ref", "-d", "HEAD"]);
    let (code, text) = fixture.build("app");
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::The commit of the build is not known: "),
        "{text}"
    );
}

#[test]
fn what_keeps_the_crate_from_being_tested_is_named() {
    let _alone = alone();
    // cargo package passes and leaves no crate.
    let fixture = Fixture::new();
    fs::remove_file(fixture.base().join("crate")).unwrap();
    let (code, text) = fixture.build("app");
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::cargo package passed, but '")
            && text.contains("app-1.2.3.crate' is not there: "),
        "{text}"
    );

    // A crate that is no archive.
    let fixture = Fixture::new();
    fixture.mark("crate", "not an archive");
    let (code, text) = fixture.build("app");
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("app-1.2.3.crate' cannot be unpacked: "),
        "{text}"
    );

    // A crate that unpacks to something else.
    let fixture = Fixture::new();
    fixture.packaged(&[("other/Cargo.toml", "")]);
    let (code, text) = fixture.build("app");
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("app-1.2.3.crate' does not unpack to app-1.2.3/Cargo.toml."),
        "{text}"
    );

    // No folder to unpack into.
    let fixture = Fixture::new();
    let file = fixture.base().join("a file");
    fs::write(&file, "").unwrap();
    let (code, text) = fixture.build_in("app", &file, &fixture.base().join("outputs"), true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::no folder can be made in ")
            && text.contains("to unpack the crate into: "),
        "{text}"
    );

    // Outputs that cannot be written fail the build at its end.
    let fixture = Fixture::new();
    let (code, text) = fixture.build_in("app", &fixture.base().join("tmp"), &fixture.base(), true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::the outputs of the step cannot be written to "),
        "{text}"
    );
}

#[test]
fn the_call_builds_only_what_the_plan_starts() {
    let _alone = alone();
    let fixture = Fixture::new();
    let root = fixture.root().display().to_string();
    let search_path = fixture.search_path();
    let outputs = fixture.base().join("outputs");
    let variables = [
        ("PATH", search_path.to_str().unwrap()),
        ("GITHUB_OUTPUT", outputs.to_str().unwrap()),
    ];
    let call = |args: &[&str], output: Output| {
        let mut all = vec!["nebensound-repo", "-C", &root, "build"];
        all.extend(args);
        call_with(&all, &variables, output)
    };

    // The whole build, in a run, without the line for a call elsewhere.
    let (text, code) = call(&["cargo-package", "--", "app"], Output::Workflow);
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains(run::OUTSIDE), "{text}");
    assert!(fixture.outputs().starts_with("artifact=crate-app@1.2.3@"));

    // Elsewhere the line comes first, also before a refusal.
    for (args, code, said) in [
        (
            vec!["cargo-package", "lib"],
            1,
            "'lib' does not build cargo-package: the plan starts no such build there",
        ),
        (
            vec!["nonsense", "app"],
            2,
            "`build` carries out the builds cargo-package and docker; \"nonsense\" is not one of \
             them",
        ),
        (
            vec!["cargo-package", "../app"],
            2,
            "\"../app\" is not a path inside the repository: ",
        ),
    ] {
        let (text, found) = call(&args, Output::Elsewhere);
        assert_eq!(found, code, "{args:?}: {text}");
        assert_eq!(text.lines().next(), Some(run::OUTSIDE), "{text}");
        assert!(text.lines().nth(1).unwrap().starts_with(said), "{text}");
    }

    // Phase 0 errors build nothing.
    fixture.git.write(&[(
        ".nebensound/repo.toml",
        &format!("{DECLARATION}\n[component.ghost]\npath = \"ghost\"\npipeline = \"none\"\n"),
    )]);
    let (text, code) = call(&["cargo-package", "app"], Output::Workflow);
    assert_eq!(code, 1, "{text}");
    assert_eq!(
        text,
        "nothing is built: phase 0 found errors, and `check` says which\n"
    );
}
