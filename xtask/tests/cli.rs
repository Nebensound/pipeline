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
        cargo: "cargo".into(),
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
    strategy:
      matrix:
        include: ${{ fromJSON(needs.declare.outputs.units) }}
    steps:
      - name: Not written yet
        env:
          WRITTEN_CHECKS: '["demo", "more"]'
      - name: demo
        if: ${{ !cancelled() && contains(matrix.checks, 'demo') }}
        uses: $/.github/actions/check-demo
        with:
          working-directory: ${{ matrix.path }}
      - name: more
        if: ${{ !cancelled() && contains(matrix.checks, 'more') }}
        uses: $/.github/actions/check-more
        with:
          working-directory: ${{ matrix.path }}
      - name: Version writers not written yet
        env:
          WRITTEN: '[]'
  builds:
    steps:
      - name: Not written yet
        env:
          WRITTEN_BUILDS: '[]'
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

/// A repository with two blocks, `check-demo` and `check-more`, whose cases
/// hold.
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
            (".github/actions/check-more/tests/pass/good/file", ""),
            (".github/actions/check-more/tests/fail/bad/bad", ""),
            (
                ".github/actions/check-more/tests/fail/bad/expected-finding",
                "::error file=bad::wrong\n",
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

#[test]
fn every_case_runs_and_every_problem_is_named() {
    let scene = Scene::new();
    let root = repository(&scene);
    let setup = setup(&root, scene.path(), "linux");

    let (code, out) = call(&["check", "--all"], &setup);
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

    // One case of each kind that does not hold, and one that cannot be run.
    let more = root.join(".github/actions/check-more/tests");
    fs::remove_dir_all(&more).unwrap();
    write(
        &more,
        &[
            ("pass/bad/bad", ""),
            ("pass/broken/broken", ""),
            ("fail/good/file", ""),
            ("fail/good/expected-finding", "anything\n"),
        ],
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
fn elsewhere_than_linux_the_cases_do_not_run() {
    let scene = Scene::new();
    let root = repository(&scene);
    let (code, out) = call(&["check", "--all"], &setup(&root, scene.path(), "macos"));
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("They agree.\n"), "{out}");
    assert!(
        out.contains("::error::For now the test cases run only on Linux, since they run the steps")
            && out.contains("and this machine runs macos.\n"),
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
    let (code, out) = call(&["check", "--all"], &setup(&root, scene.path(), "linux"));
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
    let (code, out) = call(&["check", "--all"], &setup(&root, scene.path(), "linux"));
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

/// A stand-in for `cargo` that writes its arguments, its working directory and
/// whether it saw `RUSTUP_TOOLCHAIN` to `call`, one per line, and exits with
/// the code it is told.
fn fake_cargo(scene: &Scene, code: u8) -> OsString {
    let script = scene.path().join(format!("cargo-{code}"));
    fs::write(
        &script,
        format!(
            "#!/bin/sh\n{{ for arg in \"$@\"; do echo \"$arg\"; done; pwd; \
             echo \"toolchain=${{RUSTUP_TOOLCHAIN-unset}}\"; }} > \"$(dirname \"$0\")/call\"\n\
             exit {code}\n"
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    fs::set_permissions(&script, permissions).unwrap();
    script.into()
}

/// An executable script at `path`.
fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    fs::set_permissions(path, permissions).unwrap();
}

/// A block whose steps ask for Node.js 24 and Go 1.26 through the actions
/// that would put them on the PATH.
const NEEDY: &str = r#"
runs:
  using: composite
  steps:
    - uses: actions/setup-node@v7
      with:
        node-version: 24
    - uses: actions/setup-go@v7
      with:
        go-version: "1.26"
    - shell: bash
      run: echo
"#;

/// A repository root with the tool's directory, a block that asks for
/// Node.js 24 and Go 1.26, a second block asking for Node.js 24 as well, a
/// non-composite action and a directory without an action; and a setup whose
/// PATH is `bin` beside the system's own directories.
fn needy(scene: &Scene) -> (PathBuf, PathBuf, Setup) {
    let root = scene.path().canonicalize().unwrap().join("root");
    write(
        &root,
        &[
            ("tools/nebensound/Cargo.toml", ""),
            (".github/actions/check-a/action.yml", NEEDY),
            (".github/actions/check-b/action.yml", NEEDY),
            (
                ".github/actions/check-c/action.yml",
                "runs:\n  using: node24\n  main: index.js\n",
            ),
            (".github/actions/check-d/README.md", ""),
        ],
    );
    let bin = scene.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let mut setup = setup(&root, scene.path(), "linux");
    setup
        .env
        .retain(|(name, _)| name != "PATH" && name != "RUSTUP_TOOLCHAIN");
    setup.env.push((
        OsString::from("PATH"),
        OsString::from(format!("{}:/usr/bin:/bin", bin.display())),
    ));
    (root, bin, setup)
}

/// Puts on `bin` every program the test cases need, in the versions asked.
fn provide(bin: &Path) {
    script(&bin.join("node"), "echo v24.3.0");
    script(&bin.join("go"), "echo go version go1.26.2 linux/amd64");
    script(&bin.join("jq"), "echo jq-1.8.1");
    script(&bin.join("npm"), "echo 11.6.2");
    script(
        &bin.join("python3"),
        "echo pip 25.3 from /usr/lib/python3/dist-packages/pip",
    );
    script(&bin.join("rustup"), "echo rustup 1.29.0; echo info >&2");
}

#[test]
fn setup_calls_the_tool_of_this_commit_with_every_option() {
    let scene = Scene::new();
    let (root, bin, mut setup) = needy(&scene);
    provide(&bin);
    setup.env.push((
        OsString::from("RUSTUP_TOOLCHAIN"),
        OsString::from("nightly"),
    ));

    setup.cargo = fake_cargo(&scene, 0);
    let (code, out) = call(&["setup", "--pinned", "--with-runtimes"], &setup);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out,
        "What the test cases need besides:\n\
         go 1.26: on the PATH, go version go1.26.2 linux/amd64\n\
         node 24: on the PATH, v24.3.0\n\
         jq: found, jq-1.8.1\n\
         npm: found, 11.6.2\n\
         pip: found, pip 25.3 from /usr/lib/python3/dist-packages/pip\n\
         rustup: found, rustup 1.29.0\n"
    );
    let manifest = root.join("tools/nebensound/Cargo.toml");
    // Cargo starts in the tool's directory, so that rustup takes the
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
        &root.join("tools/nebensound").to_string_lossy(),
        "toolchain=unset",
    ]
    .join("\n");
    assert_eq!(
        fs::read_to_string(scene.path().join("call")).unwrap(),
        format!("{expected}\n")
    );

    // Its exit code is the tool's, and what the cases need is checked all the
    // same.
    setup.cargo = fake_cargo(&scene, 3);
    let (code, out) = call(&["setup", "--check"], &setup);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("node 24: on the PATH"), "{out}");

    // `help` after `setup` is the tool's, and nothing is checked after it.
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
                root.join("tools/nebensound").display()
            )),
        "{out}"
    );
}

#[test]
fn setup_names_what_the_test_cases_need_and_the_machine_lacks() {
    let scene = Scene::new();
    let (_, bin, mut setup) = needy(&scene);
    setup.cargo = fake_cargo(&scene, 0);

    // A Node.js of another version, a Go that prints nothing, a jq and an
    // npm, a Python without pip and a rustup that fail; some systems ship
    // Node.js without npm. The PATH ends in the system's directories, so a
    // program of the system would be found behind a missing one.
    script(&bin.join("node"), "echo v26.13.1");
    script(&bin.join("jq"), "exit 127");
    script(&bin.join("npm"), "exit 127");
    script(
        &bin.join("python3"),
        "[ \"$*\" = '-m pip --version' ] && { echo 'No module named pip' >&2; exit 1; }; echo Python 3.13.1",
    );
    script(&bin.join("go"), "exit 0");
    script(&bin.join("rustup"), "echo rustup 1.29.0; exit 1");
    let (code, out) = call(&["setup"], &setup);
    assert_eq!(code, 1, "{out}");
    assert_eq!(
        out,
        "What the test cases need besides:\n\
         go 1.26: missing, the PATH has none. Put go 1.26 on the PATH.\n\
         node 24: missing, the PATH has v26.13.1. Put node 24 on the PATH.\n\
         jq: missing, `jq --version` fails on the PATH the steps get.\n\
         npm: missing, `npm --version` fails on the PATH the steps get.\n\
         pip: missing, `python3 -m pip --version` fails on the PATH the steps get.\n\
         rustup: missing, `rustup --version` fails on the PATH the steps get.\n\
         6 program(s) the test cases need are missing, each named above.\n"
    );
}

#[test]
fn elsewhere_than_linux_setup_leaves_out_what_only_the_cases_need() {
    let scene = Scene::new();
    let (_, bin, mut setup) = needy(&scene);
    setup.os = "macos".to_owned();
    // Nothing the cases need is there, and that fails nothing here.
    script(&bin.join("node"), "echo v26.13.1");
    setup.cargo = fake_cargo(&scene, 0);
    let (code, out) = call(&["setup"], &setup);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out,
        "For now the test cases run only on Linux, and this machine runs macos, so what only \
         they need is not checked here.\n"
    );

    // The tool's own exit code still counts.
    setup.cargo = fake_cargo(&scene, 3);
    let (code, _) = call(&["setup"], &setup);
    assert_eq!(code, 3);
}

#[test]
fn setup_with_actions_it_cannot_read_is_a_failure() {
    let scene = Scene::new();
    let (root, bin, mut setup) = needy(&scene);
    provide(&bin);
    setup.cargo = fake_cargo(&scene, 0);

    fs::write(root.join(".github/actions/check-a/action.yml"), "runs: [").unwrap();
    let (code, out) = call(&["setup"], &setup);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("::error::") && out.contains("is no YAML"),
        "{out}"
    );

    fs::remove_dir_all(root.join(".github")).unwrap();
    let (code, out) = call(&["setup"], &setup);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("::error::cannot list"), "{out}");
}

#[test]
fn setup_killed_by_a_signal_is_a_failure() {
    let scene = Scene::new();
    let script = scene.path().join("cargo-killed");
    fs::write(&script, "#!/bin/sh\nkill -9 $$\n").unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    fs::set_permissions(&script, permissions).unwrap();
    fs::create_dir_all(scene.path().join("tools/nebensound")).unwrap();
    let mut setup = setup(scene.path(), scene.path(), "linux");
    setup.cargo = script.into();
    let (code, out) = call(&["setup"], &setup);
    assert_eq!(code, 1, "{out}");
    assert!(!out.contains("cannot start"), "{out}");
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
