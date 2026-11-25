//! `nebensound-repo build cargo-release`: the release build of one binary for
//! the target of the machine, run from the tool.
//!
//! No real toolchain runs here. `cargo`, `rustup` and `rustc` are shell scripts
//! that print how they were called and act the way the fixture tells them:
//! `cargo metadata` prints the metadata the fixture wrote, `cargo build` puts
//! the binary the fixture wrote where Cargo would, and a call fails when a line
//! of the fixture's `fail` file is part of its arguments. That the real
//! toolchain builds, starts and packs a binary is shown by the cases of the
//! block, which `cargo xtask check --all` runs through `build`, and by this
//! repository's own build. Hence Unix only: the gate runs on Linux.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::io::Read as _;
use std::path::PathBuf;
use std::time::Duration;

use nebensound_repo::Register;
use nebensound_repo::build::cargo_release::{archive, artifact, licences, platform, target_for};
use nebensound_repo::build::{self, Build, Context, Machine};
use nebensound_repo::console::Console;
use nebensound_repo::output::Output;
use nebensound_repo::path::RelativePath;
use nebensound_repo::repository::Repository;
use nebensound_repo::run;
use serde_json::{Value, json};
use support::{Git, call_with, program::executable};
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

/// What every program says first: whether a variable that steers the toolchain
/// reached it. Cargo sets `CARGO_MANIFEST_DIR` for every test it runs, and the
/// build takes it out with every other variable whose name starts with `CARGO`
/// or `RUST`.
const SEES: &str = r#"echo "$(basename "$0") sees CARGO_MANIFEST_DIR=${CARGO_MANIFEST_DIR-unset}" >&2
"#;

/// `cargo build` writes the fixture's `binary` where Cargo puts the binary
/// `app` for the target it is called with.
const CARGO: &str = r#"
echo "cargo $* in $(basename "$(pwd -P)")" >&2
failing "cargo $*" && { echo "error: it failed" >&2; exit 101; }
case "$1" in
  metadata) cat "$here/metadata" ;;
  build)
    target=""
    previous=""
    for arg in "$@"; do
      [ "$previous" = "--target" ] && target="$arg"
      previous="$arg"
    done
    if [ -f "$here/binary" ]; then
      mkdir -p "$here/target/$target/release"
      cat "$here/binary" > "$here/target/$target/release/app"
      chmod 755 "$here/target/$target/release/app"
    fi ;;
esac
exit 0
"#;

const RUSTUP: &str = r#"
echo "rustup $* in $(basename "$(pwd -P)")" >&2
failing "rustup $*" && exit 1
exit 0
"#;

const RUSTC: &str = r#"
echo "rustc 1.98.1"
failing "rustc $*" && exit 1
exit 0
"#;

/// A binary that prints its name and the version of the run, and what it sees.
const PRINTS: &str = "#!/bin/sh\n\
    echo \"app sees CARGO_MANIFEST_DIR=${CARGO_MANIFEST_DIR-unset}\" >&2\n\
    echo \"app 1.2.3-pr.4\"\n";

/// The declaration of the fixture: `app` builds cargo-release.
const DECLARATION: &str = r#"[repo]
audience = "public"
pipeline = "none"

[component.app]
path     = "app"
pipeline = { version = ["cargo"], build = "cargo-release" }
"#;

/// The machine a test builds on: Linux on `x86_64`, which gives the target
/// `x86_64-unknown-linux-musl`, with little time for the binary to answer.
fn linux() -> Machine {
    Machine {
        os: "linux".to_owned(),
        arch: "x86_64".to_owned(),
        processes: PathBuf::from("/proc"),
        patience: Duration::from_secs(1),
        interval: Duration::from_millis(5),
        answer: Duration::from_secs(5),
    }
}

/// A repository with one commit, the programs above and what they are told.
struct Fixture {
    base: TempDir,
    git: Git,
}

/// How one build is called.
struct Call {
    unit: &'static str,
    machine: Machine,
    version: Option<&'static str>,
    temp: Option<PathBuf>,
    outputs: Option<PathBuf>,
    register: Register,
}

impl Call {
    fn of(unit: &'static str) -> Self {
        Self {
            unit,
            machine: linux(),
            version: Some("1.2.3-pr.4"),
            temp: None,
            outputs: None,
            register: Register::builtin().unwrap(),
        }
    }
}

impl Fixture {
    /// The package `app` at `app`, with one binary, a licence at the root and
    /// a binary that prints the version of the run.
    fn new() -> Self {
        let base = tempfile::tempdir().unwrap();
        let bin = base.path().join("bin");
        for (name, script) in [("cargo", CARGO), ("rustup", RUSTUP), ("rustc", RUSTC)] {
            executable(
                &bin.join(name),
                format!("#!/bin/sh\n{FAILING}{SEES}{script}"),
            );
        }
        let utilities = base.path().join("utilities");
        fs::create_dir_all(&utilities).unwrap();
        let computer = std::env::var_os("PATH").unwrap_or_default();
        for utility in ["basename", "cat", "chmod", "dirname", "mkdir", "sleep"] {
            let found = nebensound_repo::setup::find::search(utility, &computer, false).unwrap();
            std::os::unix::fs::symlink(found, utilities.join(utility)).unwrap();
        }
        fs::create_dir_all(base.path().join("tmp")).unwrap();
        let git = Git::new();
        git.write(&[
            (".nebensound/repo.toml", DECLARATION),
            ("LICENSE", "The licence.\n"),
            ("app/Cargo.toml", "[package]\nname = \"app\"\n"),
        ]);
        git.commit("chore: start");
        let fixture = Self { base, git };
        fixture.metadata(&[("app", "app", &["app"], &["bin"])]);
        fixture.mark("binary", PRINTS);
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

    /// The metadata `cargo metadata` prints: each package with its folder,
    /// its targets and the kind they all have.
    fn metadata(&self, packages: &[(&str, &str, &[&str], &[&str])]) -> &Self {
        let root = self.root();
        let packages: Vec<Value> = packages
            .iter()
            .map(|(name, dir, targets, kind)| {
                let targets: Vec<Value> = targets
                    .iter()
                    .map(|target| {
                        json!({"name": target, "kind": kind, "src_path": "/x.rs",
                               "edition": "2024"})
                    })
                    .collect();
                json!({
                    "id": format!("{name}-id"), "name": name, "version": "1.2.3-pr.4",
                    "source": null, "manifest_path": root.join(dir).join("Cargo.toml"),
                    "targets": targets, "publish": null, "rust_version": null
                })
            })
            .collect();
        self.mark(
            "metadata",
            &json!({
                "packages": packages,
                "workspace_members": [],
                "target_directory": self.base().join("target"),
                "resolve": null
            })
            .to_string(),
        )
    }

    fn search_path(&self) -> std::ffi::OsString {
        std::env::join_paths([self.base().join("bin"), self.base().join("utilities")]).unwrap()
    }

    fn released(&self) -> PathBuf {
        self.base().join("target/x86_64-unknown-linux-musl/release")
    }

    /// Builds as `call` says: the exit code and what it printed.
    fn run(&self, call: &Call) -> (u8, String) {
        let root = self.root();
        let repository = Repository::at(&root);
        let search_path = self.search_path();
        let outputs = call
            .outputs
            .clone()
            .unwrap_or_else(|| self.base().join("outputs"));
        let temp = call.temp.clone().unwrap_or_else(|| self.base().join("tmp"));
        let variables = BTreeMap::from([(
            "GITHUB_OUTPUT".to_owned(),
            outputs.to_string_lossy().into_owned(),
        )]);
        let read = |name: &str| variables.get(name).cloned();
        let env = nebensound_repo::setup::Environment::current();
        let context = Context {
            repository: &repository,
            root: &root,
            search_path: &search_path,
            temp: &temp,
            variables: &read,
            register: &call.register,
            env: &env,
            machine: &call.machine,
            component: Some("app"),
            version: call.version,
        };
        let mut out = Vec::new();
        let mut console = Console::new(&mut out, true);
        let code = build::run(
            &context,
            Build::CargoRelease,
            &RelativePath::new(call.unit).unwrap(),
            &mut console,
        );
        let code = console.end(code);
        (code, String::from_utf8(out).unwrap())
    }

    fn build(&self) -> (u8, String) {
        self.run(&Call::of("app"))
    }

    fn outputs(&self) -> String {
        fs::read_to_string(self.base().join("outputs")).unwrap_or_default()
    }
}

fn has(text: &str, line: &str) {
    assert!(
        text.lines().any(|found| found == line),
        "no line {line:?} in\n{text}"
    );
}

/// The files of an archive: name, contents, mode and modification time.
fn unpacked(bytes: &[u8]) -> Vec<(String, String, u32, u64)> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    archive
        .entries()
        .unwrap()
        .map(|entry| {
            let mut entry = entry.unwrap();
            let header = entry.header().clone();
            let mut text = String::new();
            entry.read_to_string(&mut text).unwrap();
            (
                entry.path().unwrap().to_string_lossy().into_owned(),
                text,
                header.mode().unwrap(),
                header.mtime().unwrap(),
            )
        })
        .collect()
}

#[test]
fn the_artifact_carries_the_path_the_version_the_commit_and_the_target() {
    assert_eq!(
        artifact("tools/foo", "1.2.3-pr.4", "abc", "aarch64-apple-darwin"),
        "executable-tools%2Ffoo@1.2.3-pr.4@abc@aarch64-apple-darwin"
    );
}

#[test]
fn a_target_is_built_on_a_machine_of_its_platform() {
    assert_eq!(
        platform("x86_64-unknown-linux-musl"),
        Some(("x86_64", "linux"))
    );
    assert_eq!(platform("aarch64-apple-darwin"), Some(("aarch64", "macos")));
    assert_eq!(platform("x86_64-apple-darwin"), Some(("x86_64", "macos")));
    assert_eq!(platform("x86_64-pc-windows-msvc"), None);

    let register = Register::builtin().unwrap();
    let targets = &register
        .build
        .iter()
        .find(|(name, _)| name.as_str() == "cargo-release")
        .unwrap()
        .1
        .targets;
    let on = |os: &str, arch: &str| Machine {
        os: os.to_owned(),
        arch: arch.to_owned(),
        ..linux()
    };
    assert_eq!(
        target_for(targets, &on("linux", "x86_64")),
        Ok("x86_64-unknown-linux-musl")
    );
    assert_eq!(
        target_for(targets, &on("macos", "aarch64")),
        Ok("aarch64-apple-darwin")
    );
    assert_eq!(
        target_for(targets, &on("macos", "x86_64")),
        Ok("x86_64-apple-darwin")
    );
    assert_eq!(
        target_for(targets, &on("linux", "aarch64")),
        Err(
            "cargo-release builds for aarch64-apple-darwin, x86_64-apple-darwin, \
             x86_64-unknown-linux-musl, each on a machine of that platform, and this one is \
             linux on aarch64."
                .to_owned()
        )
    );
    assert_eq!(
        target_for(&BTreeMap::new(), &on("linux", "x86_64")),
        Err(
            "cargo-release builds for no target, each on a machine of that platform, and this \
             one is linux on x86_64."
                .to_owned()
        )
    );
    let mut two = targets.clone();
    two.insert(
        "x86_64-unknown-linux-gnu".to_owned(),
        "ubuntu-latest".to_owned(),
    );
    assert_eq!(
        target_for(&two, &on("linux", "x86_64")),
        Err(
            "The register lists more than one target of cargo-release for linux on x86_64 \
             (x86_64-unknown-linux-gnu, x86_64-unknown-linux-musl), so the build cannot tell \
             which to build."
                .to_owned()
        )
    );
}

#[test]
fn the_same_files_give_the_same_archive() {
    let entries = vec![
        ("app".to_owned(), b"binary".to_vec(), 0o755),
        ("LICENSE".to_owned(), b"licence".to_vec(), 0o644),
    ];
    let first = archive(&entries).unwrap();
    assert_eq!(first, archive(&entries).unwrap());
    // gzip: no time, the best compression, no operating system.
    assert_eq!(&first[..10], &[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 2, 255]);
    assert_eq!(
        unpacked(&first),
        [
            ("app".to_owned(), "binary".to_owned(), 0o755, 0),
            ("LICENSE".to_owned(), "licence".to_owned(), 0o644, 0),
        ]
    );
    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(first.as_slice())
        .read_to_end(&mut tar)
        .unwrap();
    // ustar, with neither owner nor group.
    assert_eq!(&tar[257..263], b"ustar\0");
    assert_eq!(&tar[265..265 + 32], &[0; 32]);

    // A name tar refuses.
    assert!(archive(&[("../app".to_owned(), Vec::new(), 0o755)]).is_err());
}

#[test]
fn the_licence_is_the_nearest_folders() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let unit = |path: &str| RelativePath::new(path).unwrap();
    // None anywhere.
    fs::create_dir_all(root.join("tools/app/LICENSE")).unwrap();
    assert!(licences(root, &unit("tools/app")).is_empty());
    // The root's, for the root and for a unit below it.
    for name in ["LICENSE-MIT", "LICENSE-APACHE", "README.md"] {
        fs::write(root.join(name), "").unwrap();
    }
    let both = vec![root.join("LICENSE-APACHE"), root.join("LICENSE-MIT")];
    assert_eq!(licences(root, &unit(".")), both);
    assert_eq!(licences(root, &unit("tools/app")), both);
    // A folder between takes precedence, in any case and spelling.
    fs::write(root.join("tools/Licence.txt"), "").unwrap();
    fs::write(root.join("tools/COPYING"), "").unwrap();
    assert_eq!(
        licences(root, &unit("tools/app")),
        [root.join("tools/COPYING"), root.join("tools/Licence.txt")]
    );
}

#[test]
fn every_step_runs_in_its_order_and_names_the_artifact() {
    let fixture = Fixture::new();
    let (code, text) = fixture.build();
    assert_eq!(code, 0, "{text}");
    let head = fixture.git.head();
    let artifact = format!("executable-app@1.2.3-pr.4@{head}@x86_64-unknown-linux-musl");
    let built = fixture.released().join("app");
    let archived = fixture
        .released()
        .join("app-x86_64-unknown-linux-musl.tar.gz");
    for line in [
        "::group::Install the toolchain the workspace pins, with the target",
        "This machine is linux on x86_64, so the build is for x86_64-unknown-linux-musl.",
        "> rustup toolchain install",
        "rustup toolchain install in app",
        "> rustup target add x86_64-unknown-linux-musl",
        "> rustc --version",
        "::group::Find the binary of the unit",
        "cargo metadata --locked --format-version 1 --no-deps in app",
        &format!(
            "'app' is the binary app of the package app at 1.2.3-pr.4; its artifact is \
             '{artifact}'."
        ),
        "::group::Build the binary",
        "> cargo build --release --locked --target x86_64-unknown-linux-musl --package app \
         --bin app",
        "::group::Start the binary",
        &format!("> {} --version", built.display()),
        "app 1.2.3-pr.4",
        "::group::Pack the archive",
        &format!(
            "Beside the binary: {}",
            fixture.root().join("LICENSE").display()
        ),
        "The archive for x86_64-unknown-linux-musl holds the binary app and the licence.",
    ] {
        has(&text, line);
    }
    let bytes = fs::read(&archived).unwrap();
    let digest = text
        .lines()
        .find(|line| line.ends_with(&format!("  {}", archived.display())))
        .unwrap();
    assert_eq!(
        digest.split_whitespace().next().unwrap(),
        nebensound_repo::build::cargo_package::sha256(&bytes)
    );
    assert_eq!(
        unpacked(&bytes),
        [
            ("app".to_owned(), PRINTS.to_owned(), 0o755, 0),
            ("LICENSE".to_owned(), "The licence.\n".to_owned(), 0o644, 0),
        ]
    );
    assert!(!text.contains("failed"), "{text}");
    assert_eq!(
        fixture.outputs(),
        format!("artifact={artifact}\narchive={}\n", archived.display())
    );
    // What the binary printed went into a file of its own, which goes again.
    assert_eq!(fs::read_dir(fixture.base().join("tmp")).unwrap().count(), 0);
}

#[test]
fn every_program_starts_without_the_variables_that_steer_the_toolchain() {
    assert!(
        std::env::var_os("CARGO_MANIFEST_DIR").is_some(),
        "the test needs a variable the build takes out; Cargo sets this one for its tests"
    );
    let fixture = Fixture::new();
    let (code, text) = fixture.build();
    assert_eq!(code, 0, "{text}");
    let seen: Vec<&str> = text
        .lines()
        .filter(|line| line.contains(" sees "))
        .collect();
    assert_eq!(
        seen,
        [
            "rustup sees CARGO_MANIFEST_DIR=unset",
            "rustup sees CARGO_MANIFEST_DIR=unset",
            "rustc sees CARGO_MANIFEST_DIR=unset",
            "cargo sees CARGO_MANIFEST_DIR=unset",
            "cargo sees CARGO_MANIFEST_DIR=unset",
            "app sees CARGO_MANIFEST_DIR=unset",
        ],
        "{text}"
    );
}

#[test]
fn the_first_step_that_fails_ends_the_build() {
    for (pattern, step) in [
        (
            "rustup toolchain install",
            "Install the toolchain the workspace pins, with the target",
        ),
        (
            "rustup target add",
            "Install the toolchain the workspace pins, with the target",
        ),
        (
            "rustc --version",
            "Install the toolchain the workspace pins, with the target",
        ),
        ("--no-deps", "Find the binary of the unit"),
        ("cargo build", "Build the binary"),
    ] {
        let fixture = Fixture::new();
        fixture.fail(&[pattern]);
        let (code, text) = fixture.build();
        assert_eq!(code, 1, "{pattern}: {text}");
        has(&text, &format!("{step}: failed"));
        assert_eq!(fixture.outputs(), "", "{pattern}");
    }
}

#[test]
fn a_machine_without_a_target_builds_nothing() {
    let fixture = Fixture::new();
    let mut call = Call::of("app");
    call.machine.os = "freebsd".to_owned();
    let (code, text) = fixture.run(&call);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("and this one is freebsd on x86_64."),
        "{text}"
    );
    assert!(!text.contains("> rustup"), "{text}");
}

#[test]
fn a_unit_with_two_binaries_or_none_fails_by_name() {
    let fixture = Fixture::new();
    let root = fixture.root();
    for (packages, unit, said) in [
        (
            vec![("app", "app", &["app", "helper"][..], &["bin"][..])],
            "app",
            "::error::The package app of 'app' has 2 binaries (app, helper), and cargo-release \
             builds exactly one.",
        ),
        (
            vec![("app", "app", &["app"][..], &["lib"][..])],
            "app",
            "::error::The package app of 'app' has 0 binaries (none), and cargo-release builds \
             exactly one.",
        ),
        (
            vec![
                ("one", "one", &["one"][..], &["bin"][..]),
                ("two", "two", &["two"][..], &["bin"][..]),
                ("lib", "lib", &["lib"][..], &["lib"][..]),
            ],
            ".",
            "::error::The Cargo.toml of '.' is a workspace without a package of its own, so \
             there is no one binary to build; its packages with a binary: one, two.",
        ),
        (
            vec![("lib", "lib", &["lib"][..], &["lib"][..])],
            ".",
            "::error::The Cargo.toml of '.' is a workspace without a package of its own, so \
             there is no one binary to build; its packages with a binary: none.",
        ),
    ] {
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fixture.metadata(&packages);
        let (code, text) = fixture.run(&Call::of(unit));
        assert_eq!(code, 1, "{text}");
        has(&text, said);
        has(&text, "Find the binary of the unit: failed");
    }
}

#[test]
fn what_keeps_the_binary_from_being_found_is_named() {
    let fixture = Fixture::new();
    // No Cargo.toml at the unit.
    fixture.git.write(&[("nowhere/README.md", "# x\n")]);
    let (code, text) = fixture.run(&Call::of("nowhere"));
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::'nowhere' builds cargo-release but holds no Cargo.toml.",
    );

    // No version of the run.
    let mut call = Call::of("app");
    call.version = None;
    let (code, text) = fixture.run(&call);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::The plan gives 'app' no version of this run, so there is no version for the \
         binary to print.",
    );

    // Metadata the build cannot read.
    fixture.mark("metadata", "not JSON");
    let (code, text) = fixture.build();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::cargo metadata printed nothing the build can read.",
    );

    // A repository without a commit.
    let fixture = Fixture::new();
    fixture.git.run(&["update-ref", "-d", "HEAD"]);
    let (code, text) = fixture.build();
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::The commit of the build is not known: "),
        "{text}"
    );
}

#[test]
fn the_binary_has_to_print_the_version_of_the_run_in_time() {
    for (binary, said) in [
        (None, "app' cannot be started: "),
        (
            Some("#!/bin/sh\necho \"app 1.2.3\"\nexit 3\n"),
            "::error::'app --version' failed (",
        ),
        (
            Some("#!/bin/sh\necho \"app 1.2.3-pr.40\"\n"),
            "::error::'app --version' does not print the version of this run, 1.2.3-pr.4.",
        ),
        (
            Some("#!/bin/sh\nsleep 30\n"),
            "::error::'app --version' did not end within 300ms.",
        ),
    ] {
        let fixture = Fixture::new();
        if let Some(script) = binary {
            fixture.mark("binary", script);
        } else {
            fs::remove_file(fixture.base().join("binary")).unwrap();
        }
        // Only the binary that never ends gets the short time: on a busy
        // machine, a shell that does end can take longer than that to start.
        let mut call = Call::of("app");
        call.machine.answer = if said.contains("did not end") {
            Duration::from_millis(300)
        } else {
            Duration::from_secs(30)
        };
        let (code, text) = fixture.run(&call);
        assert_eq!(code, 1, "{text}");
        assert!(text.contains(said), "{said}: {text}");
        has(&text, "Start the binary: failed");
        assert_eq!(fixture.outputs(), "");
    }

    // No file for what it prints.
    let fixture = Fixture::new();
    let file = fixture.base().join("a file");
    fs::write(&file, "").unwrap();
    let mut call = Call::of("app");
    call.temp = Some(file);
    let (code, text) = fixture.run(&call);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::no file can be made in ")
            && text.contains("for what the binary prints: "),
        "{text}"
    );
}

#[test]
fn what_keeps_the_archive_from_being_written_is_named() {
    // No licence.
    let fixture = Fixture::new();
    fixture.git.remove("LICENSE");
    let (code, text) = fixture.build();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::Neither 'app' nor a folder above it holds a licence, a file whose name starts \
         with LICENSE, LICENCE or COPYING, to put beside the binary.",
    );

    // A folder where the archive goes.
    let fixture = Fixture::new();
    fs::create_dir_all(
        fixture
            .released()
            .join("app-x86_64-unknown-linux-musl.tar.gz"),
    )
    .unwrap();
    let (code, text) = fixture.build();
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("app-x86_64-unknown-linux-musl.tar.gz' cannot be written: "),
        "{text}"
    );
    has(&text, "Pack the archive: failed");

    // Outputs that cannot be written fail the build at its end.
    let fixture = Fixture::new();
    let mut call = Call::of("app");
    call.outputs = Some(fixture.base());
    let (code, text) = fixture.run(&call);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::the outputs of the step cannot be written to "),
        "{text}"
    );
}

#[test]
fn the_call_builds_for_this_machine_what_the_plan_starts() {
    let fixture = Fixture::new();
    let machine = Machine::current();
    let register = Register::builtin().unwrap();
    let targets = &register
        .build
        .iter()
        .find(|(name, _)| name.as_str() == "cargo-release")
        .unwrap()
        .1
        .targets;
    let Ok(target) = target_for(targets, &machine) else {
        // A machine the register builds for no target, such as Linux on
        // ARM, has its refusal tested above.
        return;
    };
    let root = fixture.root().display().to_string();
    let search_path = fixture.search_path();
    let outputs = fixture.base().join("outputs");
    let plan = fixture.base().join("plan.json");
    fs::write(
        &plan,
        r#"{"versions":[{"kind":"component","name":"app","path":"app","version":"1.2.3-pr.4","writers":["cargo"]}]}"#,
    )
    .unwrap();
    let variables = [
        ("PATH", search_path.to_str().unwrap()),
        ("GITHUB_OUTPUT", outputs.to_str().unwrap()),
    ];
    let (text, code) = call_with(
        &[
            "nebensound-repo",
            "-C",
            &root,
            "build",
            "--plan",
            plan.to_str().unwrap(),
            "cargo-release",
            "--",
            "app",
        ],
        &variables,
        Output::Workflow,
    );
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains(run::OUTSIDE), "{text}");
    assert!(
        fixture
            .outputs()
            .starts_with("artifact=executable-app@1.2.3-pr.4@"),
        "{}",
        fixture.outputs()
    );
    assert!(fixture.outputs().contains(&format!("@{target}\n")));

    // A unit the plan starts no such build for.
    let (text, code) = call_with(
        &[
            "nebensound-repo",
            "-C",
            &root,
            "build",
            "cargo-release",
            ".",
        ],
        &variables,
        Output::Elsewhere,
    );
    assert_eq!(code, 1, "{text}");
    assert_eq!(text.lines().next(), Some(run::OUTSIDE), "{text}");
    assert!(
        text.contains("'.' does not build cargo-release: the plan starts no such build there"),
        "{text}"
    );
}
