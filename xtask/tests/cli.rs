//! `cargo xtask`: the call, `setup`, and what the binary answers.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::Command;

use support::{Scene, program::executable, write};
use xtask::Setup;

fn setup(root: &Path, scratch: &Path) -> Setup {
    Setup {
        root: root.to_path_buf(),
        env: std::env::vars_os().collect(),
        scratch: scratch.to_path_buf(),
        cargo: "cargo".into(),
    }
}

fn call(args: &[&str], setup: &Setup) -> (u8, String) {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let mut out = Vec::new();
    let code = xtask::run(&args, setup, &mut out);
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn help_is_no_failure_and_a_wrong_call_is_one() {
    let scene = Scene::new();
    let setup = setup(scene.path(), scene.path());
    for help in ["help", "--help", "-h"] {
        let (code, out) = call(&[help], &setup);
        assert_eq!(code, 0, "{out}");
        assert!(out.starts_with("Usage: cargo xtask check --all"), "{out}");
        assert!(out.contains("cargo xtask setup [<option> ...]"), "{out}");
    }
    for (args, message) in [
        (&[][..], "no task named"),
        (&["check"], "`check` takes --all and nothing else"),
        (
            &["check", "check-demo"],
            "`check` takes --all and nothing else",
        ),
        (
            &["check", "--all", "check-demo"],
            "`check` takes --all and nothing else",
        ),
        (&["build"], "there is no task `build`"),
    ] {
        let (code, out) = call(args, &setup);
        assert_eq!(code, 2, "{args:?}: {out}");
        assert!(
            out.lines().next().unwrap().contains(message),
            "{args:?}: {out}"
        );
        assert!(out.contains("Usage:"), "{out}");
    }
}

/// A stand-in for `cargo` that writes its arguments, its working directory and
/// whether it saw `RUSTUP_TOOLCHAIN` to `call`, one per line, and exits with
/// the code it is told.
fn fake_cargo(scene: &Scene, code: u8) -> OsString {
    let script = scene.path().join(format!("cargo-{code}"));
    executable(
        &script,
        format!(
            "#!/bin/sh\n{{ for arg in \"$@\"; do echo \"$arg\"; done; pwd; \
             echo \"toolchain=${{RUSTUP_TOOLCHAIN-unset}}\"; }} > \"$(dirname \"$0\")/call\"\n\
             exit {code}\n"
        ),
    );
    script.into()
}

#[test]
fn setup_calls_the_tool_of_this_commit_with_every_option() {
    let scene = Scene::new();
    let root = scene.path().canonicalize().unwrap().join("root");
    write(&root, &[("Cargo.toml", "")]);
    let mut setup = setup(&root, scene.path());
    setup.env.retain(|(name, _)| name != "RUSTUP_TOOLCHAIN");
    setup.env.push((
        OsString::from("RUSTUP_TOOLCHAIN"),
        OsString::from("nightly"),
    ));

    setup.cargo = fake_cargo(&scene, 0);
    let (code, out) = call(&["setup", "--pinned", "--with-runtimes"], &setup);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "");
    let manifest = root.join("Cargo.toml");
    // Cargo starts at the root, where the tool's package lies, so that rustup takes the
    // toolchain the tool pins, and without the xtask's RUSTUP_TOOLCHAIN.
    let expected = [
        "run",
        "--quiet",
        "--locked",
        "--manifest-path",
        &manifest.to_string_lossy(),
        "--",
        "-C",
        &root.to_string_lossy(),
        "setup",
        "--pinned",
        "--with-runtimes",
        &root.to_string_lossy(),
        "toolchain=unset",
    ]
    .join("\n");
    assert_eq!(
        fs::read_to_string(scene.path().join("call")).unwrap(),
        format!("{expected}\n")
    );

    // Its exit code is the tool's, also after `help`.
    setup.cargo = fake_cargo(&scene, 3);
    let (code, out) = call(&["setup", "--check"], &setup);
    assert_eq!(code, 3, "{out}");
    assert_eq!(out, "");
    for help in ["help", "--help", "-h"] {
        setup.cargo = fake_cargo(&scene, 1);
        let (code, out) = call(&["setup", help], &setup);
        assert_eq!(code, 1);
        assert_eq!(out, "");
        assert!(
            fs::read_to_string(scene.path().join("call"))
                .unwrap()
                .contains(&format!("\nsetup\n{help}\n"))
        );
    }

    setup.cargo = scene.path().join("no-cargo").into();
    let (code, out) = call(&["setup"], &setup);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.starts_with("::error::cannot start `")
            && out.contains(&format!(
                "no-cargo` in {} to build the tool",
                root.display()
            )),
        "{out}"
    );
}

#[test]
fn setup_killed_by_a_signal_is_a_failure() {
    let scene = Scene::new();
    let script = scene.path().join("cargo-killed");
    executable(&script, "#!/bin/sh\nkill -9 $$\n");
    let mut setup = setup(scene.path(), scene.path());
    setup.cargo = script.into();
    let (code, out) = call(&["setup"], &setup);
    assert_eq!(code, 1, "{out}");
    assert!(!out.contains("cannot start"), "{out}");
}

#[test]
fn a_repository_that_cannot_be_read_is_a_finding() {
    let scene = Scene::new();
    let (code, out) = call(&["check", "--all"], &setup(scene.path(), scene.path()));
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("::error::cannot read"), "{out}");
}

#[test]
fn the_current_setup_is_this_repository_on_this_machine() {
    let setup = Setup::current();
    assert!(setup.root.join(".github/actions").is_dir());
    assert_eq!(setup.cargo, "cargo");
    assert!(!setup.env.is_empty());
}

#[test]
fn a_program_that_fails_or_does_not_start_is_an_error() {
    let missing = xtask::quietly("/nonexistent/program".as_ref(), &[]).unwrap_err();
    assert!(
        missing
            .to_string()
            .contains("cannot start `/nonexistent/program`"),
        "{missing}"
    );
    let failing = xtask::quietly(
        "sh".as_ref(),
        &[
            OsString::from("-c"),
            OsString::from("echo complaint >&2; exit 1"),
        ],
    )
    .unwrap_err();
    assert_eq!(
        failing.to_string(),
        "`sh -c echo complaint >&2; exit 1` failed: complaint"
    );
    assert!(xtask::quietly("true".as_ref(), &[]).is_ok());
    let error: Box<dyn std::error::Error> = Box::new(failing);
    assert!(error.to_string().contains("complaint"));
}

#[test]
fn the_binary_answers_with_its_exit_code() {
    let help = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("help")
        .output()
        .unwrap();
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).starts_with("Usage:"));
    let wrong = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("nothing")
        .output()
        .unwrap();
    assert_eq!(wrong.status.code(), Some(2));
}
