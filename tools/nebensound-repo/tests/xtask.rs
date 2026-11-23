//! `nebensound-repo xtask`: the repository's own checks, with the toolchain the
//! xtask pins.
//!
//! `rustup` and `cargo` are shell scripts that print how they were called and
//! fail when the fixture says so. Hence Unix only.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use nebensound_repo::output::Output;
use nebensound_repo::workflow::Log;
use nebensound_repo::xtask;
use support::{call_with, directory};
use tempfile::TempDir;

static ALONE: Mutex<()> = Mutex::new(());

fn alone() -> MutexGuard<'static, ()> {
    ALONE.lock().unwrap_or_else(PoisonError::into_inner)
}

const RUSTUP: &str = r#"#!/bin/sh
here=$(cd "$(dirname "$0")/.." && pwd -P)
echo "rustup $* in $(basename "$(pwd -P)")" >&2
[ "$1" = toolchain ] && [ -f "$here/install-fails" ] && exit 1
if [ "$1 $2" = "show active-toolchain" ]; then
  [ -f "$here/no-toolchain" ] || echo "1.98.1-aarch64-apple-darwin (overridden)"
fi
exit 0
"#;

const CARGO: &str = r#"#!/bin/sh
here=$(cd "$(dirname "$0")/.." && pwd -P)
echo "cargo $* in $(basename "$(pwd -P)") with $RUSTUP_TOOLCHAIN"
echo "::error::from the xtask"
[ -f "$here/xtask-fails" ] && exit 1
exit 0
"#;

/// The programs above in a folder of their own, beside the utilities they use.
fn programs() -> TempDir {
    let base = tempfile::tempdir().unwrap();
    for (name, script) in [("rustup", RUSTUP), ("cargo", CARGO)] {
        let file = base.path().join("bin").join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, script).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let computer = std::env::var_os("PATH").unwrap_or_default();
    fs::create_dir_all(base.path().join("utilities")).unwrap();
    for utility in ["basename", "dirname"] {
        let found = nebensound_repo::setup::find::search(utility, &computer, false).unwrap();
        std::os::unix::fs::symlink(found, base.path().join("utilities").join(utility)).unwrap();
    }
    base
}

fn search_path(base: &Path) -> std::ffi::OsString {
    std::env::join_paths([base.join("bin"), base.join("utilities")]).unwrap()
}

fn xtask_in(root: &Path, base: &Path) -> (bool, String) {
    let mut out = Vec::new();
    let mut log = Log::new(&mut out, true);
    let passed = xtask::run(root, &search_path(base), &mut log);
    (passed, String::from_utf8(out).unwrap())
}

#[test]
fn the_xtask_runs_with_the_toolchain_it_pins() {
    let _alone = alone();
    let base = programs();
    let repository = directory(&[
        ("xtask/Cargo.toml", ""),
        ("xtask/rust-toolchain.toml", ""),
        ("rust-toolchain.toml", ""),
    ]);
    let (passed, text) = xtask_in(repository.path(), base.path());
    assert!(passed, "{text}");
    for line in [
        "> rustup toolchain install",
        "rustup toolchain install in xtask",
        "rustup show active-toolchain in xtask",
        "Running the xtask with 1.98.1-aarch64-apple-darwin, pinned in xtask/rust-toolchain.toml.",
        "> cargo xtask check --all",
        "::error::from the xtask",
    ] {
        assert!(text.lines().any(|found| found == line), "{line}\n{text}");
    }
    let root = fs::canonicalize(repository.path()).unwrap();
    let name = root.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        text.contains(&format!(
            "cargo xtask check --all in {name} with 1.98.1-aarch64-apple-darwin"
        )),
        "{text}"
    );

    // Without a pin in xtask/, the one at the root.
    fs::remove_file(repository.path().join("xtask/rust-toolchain.toml")).unwrap();
    let (passed, text) = xtask_in(repository.path(), base.path());
    assert!(passed, "{text}");
    assert!(text.contains("pinned in rust-toolchain.toml."), "{text}");

    // What the xtask finds fails the call.
    fs::write(base.path().join("xtask-fails"), "").unwrap();
    let (passed, _) = xtask_in(repository.path(), base.path());
    assert!(!passed);
}

#[test]
fn an_xtask_without_a_pin_or_a_toolchain_does_not_run() {
    let _alone = alone();
    let base = programs();
    let repository = directory(&[("xtask/Cargo.toml", "")]);
    let (passed, text) = xtask_in(repository.path(), base.path());
    assert!(!passed);
    assert_eq!(
        text,
        "::error file=xtask/Cargo.toml::The xtask pins no toolchain: neither xtask/ nor the \
         repository root holds a rust-toolchain.toml.\n"
    );

    fs::write(repository.path().join("xtask/rust-toolchain"), "").unwrap();
    fs::write(base.path().join("install-fails"), "").unwrap();
    let (passed, text) = xtask_in(repository.path(), base.path());
    assert!(!passed, "{text}");
    assert!(!text.contains("cargo xtask"), "{text}");

    fs::remove_file(base.path().join("install-fails")).unwrap();
    fs::write(base.path().join("no-toolchain"), "").unwrap();
    let (passed, text) = xtask_in(repository.path(), base.path());
    assert!(!passed, "{text}");
    assert!(
        text.contains("::error::rustup names no active toolchain for the xtask."),
        "{text}"
    );

    let empty = directory(&[]);
    let (passed, text) = xtask_in(empty.path(), base.path());
    assert!(!passed);
    assert_eq!(
        text,
        "::error::The repository has no xtask/Cargo.toml, so there is no xtask to run.\n"
    );
}

#[test]
fn the_call_runs_the_xtask_of_the_repository_it_is_given() {
    let _alone = alone();
    let base = programs();
    let repository = directory(&[("xtask/Cargo.toml", ""), ("rust-toolchain", "")]);
    let path = search_path(base.path());
    let root = repository.path().to_str().unwrap();
    let (text, code) = call_with(
        &["nebensound-repo", "-C", root, "xtask"],
        &[("PATH", path.to_str().unwrap())],
        Output::Elsewhere,
    );
    assert_eq!(code, 0, "{text}");
    // Outside a run, what the xtask annotates is a plain line.
    assert!(text.contains("\nerror: from the xtask\n"), "{text}");

    fs::write(base.path().join("xtask-fails"), "").unwrap();
    let (text, code) = call_with(
        &["nebensound-repo", "-C", root, "xtask"],
        &[("PATH", path.to_str().unwrap())],
        Output::Workflow,
    );
    assert_eq!(code, 1, "{text}");
}
