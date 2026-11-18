//! `cargo xtask`: the call, and a whole run against a made-up repository.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use support::{Scene, write};
use xtask::Setup;

fn setup(root: &Path, scratch: &Path, os: &str) -> Setup {
    Setup {
        root: root.to_path_buf(),
        os: os.to_owned(),
        arch: "x86_64".to_owned(),
        env: std::env::vars_os().collect(),
        origin: "file:///nowhere".to_owned(),
        scratch: scratch.to_path_buf(),
    }
}

fn call(args: &[&str], setup: &Setup) -> (u8, String) {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let mut out = Vec::new();
    let code = xtask::run(&args, setup, &mut out);
    (code, String::from_utf8(out).unwrap())
}

const REGISTER: &str =
    "[check.demo]\nextensions = [\".demo\"]\n[check.more]\nextensions = [\".more\"]\n";

const WORKFLOW: &str = r#"
jobs:
  declare:
    steps:
      - uses: $/.github/actions/tool
      - uses: $/.github/actions/phase0
  units:
    steps:
      - name: Not written yet
        env:
          WRITTEN: '["demo", "more"]'
      - name: demo
        if: ${{ !cancelled() && contains(matrix.checks, 'demo') }}
        uses: $/.github/actions/check-demo
      - name: more
        if: ${{ !cancelled() && contains(matrix.checks, 'more') }}
        uses: $/.github/actions/check-more
"#;

const TOOL: &str =
    "runs:\n  using: composite\n  steps:\n    - shell: bash\n      run: echo the tool\n";

const PHASE0: &str = r#"
outputs:
  plan:
    value: ${{ steps.plan.outputs.plan }}
runs:
  using: composite
  steps:
    - id: plan
      shell: bash
      run: echo 'plan={"units":[{"path":".","checks":["demo","more"]}]}' >> "$GITHUB_OUTPUT"
"#;

const BLOCK: &str = r#"
inputs:
  working-directory: {}
runs:
  using: composite
  steps:
    - shell: bash
      run: |
        if [ -f bad ]; then echo "::error file=bad::wrong"; exit 1; fi
        if [ -f broken ]; then echo garbage >> "$GITHUB_OUTPUT"; fi
"#;

/// A repository with two blocks: `check-demo`, whose cases hold, and
/// `check-more`, with one case of each kind that does not hold and one that
/// cannot be run.
fn repository(scene: &Scene) -> PathBuf {
    let root = scene.path().join("root");
    write(
        &root,
        &[
            (
                "tools/nebensound/crates/nebensound-declaration/register.toml",
                REGISTER,
            ),
            (".github/workflows/nebensound.yml", WORKFLOW),
            (".github/actions/tool/action.yml", TOOL),
            (".github/actions/phase0/action.yml", PHASE0),
            (".github/actions/check-demo/action.yml", BLOCK),
            (".github/actions/check-demo/tests/pass/good/file", ""),
            (".github/actions/check-demo/tests/fail/bad/bad", ""),
            (
                ".github/actions/check-demo/tests/fail/bad/expected-finding",
                "::error file=bad::wrong\n",
            ),
            (".github/actions/check-more/action.yml", BLOCK),
            (".github/actions/check-more/tests/pass/bad/bad", ""),
            (".github/actions/check-more/tests/pass/broken/broken", ""),
            (".github/actions/check-more/tests/fail/good/file", ""),
            (
                ".github/actions/check-more/tests/fail/good/expected-finding",
                "anything\n",
            ),
        ],
    );
    root
}

#[test]
fn help_is_no_failure_and_a_wrong_call_is_one() {
    let scene = Scene::new();
    let setup = setup(scene.path(), scene.path(), "linux");
    for help in ["help", "--help", "-h"] {
        let (code, out) = call(&[help], &setup);
        assert_eq!(code, 0, "{out}");
        assert!(out.starts_with("Usage: cargo xtask check --all"), "{out}");
    }
    for (args, message) in [
        (&[][..], "no task named"),
        (&["check"], "needs --all or the names of blocks"),
        (
            &["check", "--all", "check-demo"],
            "either --all or the names of blocks",
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

#[test]
fn every_case_runs_and_every_problem_is_named() {
    let scene = Scene::new();
    let root = repository(&scene);
    let setup = setup(&root, scene.path(), "linux");

    let (code, out) = call(&["check", "check-demo"], &setup);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("They agree.\n"), "{out}");
    assert!(
        out.contains("::group::The tool\n  > echo the tool\n    | the tool\n::endgroup::\n"),
        "{out}"
    );
    assert!(
        out.contains("check-demo, passing case `good`: as expected\n"),
        "{out}"
    );
    assert!(
        out.contains("check-demo, failing case `bad`: as expected\n"),
        "{out}"
    );
    assert!(
        out.ends_with("Every building block holds to its test cases.\n"),
        "{out}"
    );

    let (code, out) = call(&["check", "--all"], &setup);
    assert_eq!(code, 1, "{out}");
    for line in [
        "::error::check-more, passing case `bad`: the block fails on a case that is meant to pass\n",
        "::error::check-more, passing case `broken`: cannot be run: a line of GITHUB_OUTPUT",
        "::error::check-more, failing case `good`: the block passes a case that is meant to fail",
        "3 problem(s), each named above.\n",
    ] {
        assert!(out.contains(line), "{line}\n{out}");
    }
}

#[test]
fn an_unknown_block_is_a_wrong_call() {
    let scene = Scene::new();
    let root = repository(&scene);
    let (code, out) = call(
        &["check", "check-nothing"],
        &setup(&root, scene.path(), "linux"),
    );
    assert_eq!(code, 2, "{out}");
    assert!(
        out.contains(
            "there is no building block `check-nothing`; there are: check-demo, check-more"
        ),
        "{out}"
    );
}

#[test]
fn elsewhere_than_linux_the_cases_do_not_run() {
    let scene = Scene::new();
    let root = repository(&scene);
    let (code, out) = call(&["check", "--all"], &setup(&root, scene.path(), "macos"));
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("They agree.\n"), "{out}");
    assert!(
        out.contains("this machine runs macos. Run `cargo xtask check` on Linux."),
        "{out}"
    );
    assert!(!out.contains("::group::"), "{out}");
}

#[test]
fn problems_of_the_layout_and_of_the_run_are_counted_together() {
    let scene = Scene::new();
    let root = repository(&scene);
    // A broken case: the layout names it, and running the block fails on it.
    write(
        &root,
        &[(".github/actions/check-demo/tests/fail/stray", "")],
    );
    let (code, out) = call(
        &["check", "check-demo"],
        &setup(&root, scene.path(), "linux"),
    );
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("::error::") && out.contains("stray is no case"),
        "{out}"
    );
    assert!(out.ends_with("2 problem(s), each named above.\n"), "{out}");
}

#[test]
fn a_run_without_its_tool_or_its_room_stops() {
    let scene = Scene::new();
    let root = repository(&scene);
    write(
        &root,
        &[(
            ".github/actions/tool/action.yml",
            "runs:\n  using: composite\n  steps:\n    - shell: bash\n      run: exit 1\n",
        )],
    );
    let (code, out) = call(&["check", "--all"], &setup(&root, scene.path(), "linux"));
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("::error::the action `tool` failed, so no case can run\n"),
        "{out}"
    );

    fs::remove_file(root.join(".github/actions/tool/action.yml")).unwrap();
    let (code, out) = call(
        &["check", "check-demo"],
        &setup(&root, scene.path(), "linux"),
    );
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`tool` has no action.yml"), "{out}");
    assert!(out.contains("::error::cannot read"), "{out}");

    let file = scene.path().join("a file");
    fs::write(&file, "").unwrap();
    let other = Scene::new();
    let root = repository(&other);
    let (code, out) = call(&["check", "--all"], &setup(&root, &file, "linux"));
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("::error::cannot make a directory for the run"),
        "{out}"
    );
}

#[test]
fn a_repository_that_cannot_be_read_is_a_finding() {
    let scene = Scene::new();
    let (code, out) = call(
        &["check", "--all"],
        &setup(scene.path(), scene.path(), "linux"),
    );
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("::error::cannot read"), "{out}");
}

#[test]
fn the_processor_is_named_as_the_runner_names_it() {
    assert_eq!(xtask::runner_arch("x86_64"), "X64");
    assert_eq!(xtask::runner_arch("aarch64"), "ARM64");
    assert_eq!(xtask::runner_arch("riscv64"), "riscv64");
}

#[test]
fn the_current_setup_is_this_repository_on_this_machine() {
    let setup = Setup::current();
    assert!(setup.root.join(".github/actions").is_dir());
    assert_eq!(setup.os, std::env::consts::OS);
    assert_eq!(setup.origin, "https://github.com");
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
