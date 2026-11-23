//! The cases of the blocks whose check the tool carries out: run through the
//! tool, built with Cargo, on every system.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use support::{Scene, write};
use xtask::Setup;
use xtask::cases::{self, Case, Direct, Expect};

const REGISTER: &str =
    "[check.json]\nextensions = [\".json\"]\n[check.demo]\nextensions = [\".demo\"]\n";

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
          WRITTEN_CHECKS: '["demo", "json"]'
      - name: demo
        if: ${{ !cancelled() && contains(matrix.checks, 'demo') }}
        uses: $/.github/actions/check-demo
        with:
          working-directory: ${{ matrix.path }}
      - name: json
        if: ${{ !cancelled() && contains(matrix.checks, 'json') }}
        uses: $/.github/actions/check-json
        with:
          working-directory: ${{ matrix.path }}
      - name: Version writers not written yet
        env:
          WRITTEN: '[]'
  builds:
    strategy:
      matrix:
        include: ${{ fromJSON(needs.declare.outputs.builds) }}
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
      run: echo 'plan={"units":[{"path":".","checks":["demo"]}]}' >> "$GITHUB_OUTPUT"
"#;

/// A block whose steps are never run here: its check runs from the tool.
const BLOCK: &str = r"
inputs:
  working-directory: {}
runs:
  using: composite
  steps:
    - shell: bash
      run: exit 1
";

const DEMO: &str = r"
inputs:
  working-directory: {}
runs:
  using: composite
  steps:
    - shell: bash
      run: echo demo
";

/// The tool as far as the xtask calls it: `setup` on the repository of the
/// blocks, phase 0, the plan and `run` on the repository of a case, each on
/// the repository in the working directory, as the files there decide.
const NEBENSOUND_REPO: &str = r#"#!/bin/sh
[ "$1" = "-C" ] && [ "$(cd "$2" && pwd -P)" = "$(pwd -P)" ] || { echo "not on the case: $*"; exit 2; }
shift 2
case "$1" in
  setup)
    shift
    echo "setup $* GITHUB_ACTIONS=$GITHUB_ACTIONS"
    [ -f setup-fails ] && { echo "cannot download"; exit 1; }
    exit 0 ;;
  check)
    echo "check $2 $3 $4 GITHUB_ACTIONS=$GITHUB_ACTIONS"
    [ -f phase0-fails ] && { echo "error[x]: wrong"; exit 1; }
    [ -f vanish-at-plan ] && rm -f "$0"
    exit 0 ;;
  plan)
    [ -f garbage-plan ] && { echo garbage; exit 0; }
    if [ -f no-unit ]; then echo '{"units":[]}'; else echo '{"units":[{"path":".","checks":["json"]}]}'; fi
    [ -f vanish-at-run ] && rm -f "$0"
    exit 0 ;;
  run)
    echo "run $2 $3 $4"
    [ -f bad ] && { echo "::error file=bad::wrong"; exit 1; }
    exit 0 ;;
esac
"#;

fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    fs::set_permissions(path, permissions).unwrap();
}

/// A stand-in for `cargo` that names `tool` as the executable it built,
/// among messages of other kinds, and exits with `code`.
fn cargo(scene: &Scene, tool: &Path, code: u8) -> PathBuf {
    let path = scene.path().join(format!("cargo-{code}"));
    script(
        &path,
        &format!(
            "#!/bin/sh\necho \"$*\" > \"$(dirname \"$0\")/built\"\n\
             echo 'warning: from cargo' >&2\necho 'no JSON'\n\
             echo '{{\"reason\":\"compiler-artifact\",\"target\":{{\"name\":\"other\"}},\"executable\":\"/x\"}}'\n\
             echo '{{\"reason\":\"compiler-artifact\",\"target\":{{\"name\":\"nebensound-repo\"}},\"executable\":null}}'\n\
             echo '{{\"reason\":\"compiler-artifact\",\"target\":{{\"name\":\"nebensound-repo\"}},\"executable\":\"{}\"}}'\n\
             exit {code}\n",
            tool.display()
        ),
    );
    path
}

/// A repository with `check-json`, whose check runs from the tool, and
/// `check-demo`, which runs its steps; each case of `check-json` names what
/// it expects.
fn repository(scene: &Scene) -> PathBuf {
    let root = scene.path().join("root");
    write(
        &root,
        &[
            ("tools/nebensound-repo/register.toml", REGISTER),
            (".github/workflows/nebensound.yml", WORKFLOW),
            (".github/actions/tool/action.yml", TOOL),
            (".github/actions/phase0/action.yml", PHASE0),
            (".github/actions/check-demo/action.yml", DEMO),
            (".github/actions/check-demo/tests/pass/good/file", ""),
            (".github/actions/check-demo/tests/fail/never/file", ""),
            (
                ".github/actions/check-demo/tests/fail/never/expected-finding",
                "x\n",
            ),
            (".github/actions/check-json/action.yml", BLOCK),
            (".github/actions/check-json/tests/pass/good/file", ""),
            (".github/actions/check-json/tests/fail/bad/bad", ""),
            (
                ".github/actions/check-json/tests/fail/bad/expected-finding",
                "::error file=bad::wrong\n",
            ),
        ],
    );
    root
}

/// A run on a developer's computer: without `GITHUB_ACTIONS`, even when the
/// tests themselves run on GitHub Actions.
fn setup(root: &Path, scratch: &Path, os: &str, cargo: PathBuf) -> Setup {
    Setup {
        root: root.to_path_buf(),
        os: os.to_owned(),
        arch: "x86_64".to_owned(),
        env: std::env::vars_os()
            .filter(|(name, _)| name != "GITHUB_ACTIONS")
            .collect(),
        origin: "file:///nowhere".to_owned(),
        scratch: scratch.to_path_buf(),
        cargo: cargo.into(),
    }
}

fn call(args: &[&str], setup: &Setup) -> (u8, String) {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let mut out = Vec::new();
    let code = xtask::run(&args, setup, &mut out);
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn the_cases_of_a_block_from_the_tool_run_through_the_tool() {
    let _alone = support::alone();
    let scene = Scene::new();
    let root = repository(&scene);
    let tool = scene.path().join("nebensound-repo");
    script(&tool, NEBENSOUND_REPO);
    let setup = setup(&root, scene.path(), "linux", cargo(&scene, &tool, 0));

    let (code, out) = call(&["check", "--all"], &setup);
    assert_eq!(code, 1, "{out}");
    for line in [
        "::group::The tool, built with Cargo\n",
        "    | warning: from cargo\n",
        "::group::check-json, the programs of its check\n",
        "  > nebensound-repo setup --pinned json\n",
        "    | setup --pinned json GITHUB_ACTIONS=true\n",
        "check-json, passing case `good`: as expected\n",
        "check-json, failing case `bad`: as expected\n",
        "  > nebensound-repo check --name=case --default-branch=main --visibility=public\n",
        "    | check --name=case --default-branch=main --visibility=public GITHUB_ACTIONS=true\n",
        "  > nebensound-repo run json -- .\n",
        "    | run json -- .\n",
        "check-demo, passing case `good`: as expected\n",
        "::group::The tool\n",
        "1 problem(s), each named above.\n",
    ] {
        assert!(out.contains(line), "{line}\n{out}");
    }
    let built = fs::read_to_string(scene.path().join("built")).unwrap();
    assert!(
        built
            .starts_with("build --release --locked --quiet --message-format=json --manifest-path "),
        "{built}"
    );

    // A case phase 0 refuses, and one whose plan starts the check nowhere.
    let tests = root.join(".github/actions/check-json/tests");
    write(
        &tests,
        &[
            ("fail/phase0/phase0-fails", ""),
            ("fail/phase0/expected-finding", "x\n"),
            ("pass/nowhere/no-unit", ""),
        ],
    );
    let (code, out) = call(&["check", "--all"], &setup);
    assert_eq!(code, 1, "{out}");
    for line in [
        "::error::check-json, failing case `phase0`: phase 0 fails on the case, so the block \
         never runs; a case has to pass phase 0\n",
        "::error::check-json, passing case `nowhere`: no unit of the case starts the check \
         `json`, so the block never runs\n",
        "3 problem(s), each named above.\n",
    ] {
        assert!(out.contains(line), "{line}\n{out}");
    }
    // The programs of the check are set up before its first case.
    let set_up = out
        .find("  > nebensound-repo setup --pinned json\n")
        .unwrap();
    let first = out.find("::group::check-json, passing case").unwrap();
    assert!(set_up < first, "{out}");

    // A setup that fails is a problem of its own, and the cases still run.
    write(&root, &[("setup-fails", "")]);
    let (code, out) = call(&["check", "--all"], &setup);
    assert_eq!(code, 1, "{out}");
    for line in [
        "    | cannot download\n",
        "::error::check-json: `nebensound-repo setup --pinned json` failed, so its cases run \
         with the programs that are there\n",
        "check-json, failing case `bad`: as expected\n",
        "4 problem(s), each named above.\n",
    ] {
        assert!(out.contains(line), "{line}\n{out}");
    }
}

#[test]
fn on_github_actions_the_setup_may_install_a_runtime() {
    let _alone = support::alone();
    let scene = Scene::new();
    let root = repository(&scene);
    let tool = scene.path().join("nebensound-repo");
    script(&tool, NEBENSOUND_REPO);
    let mut setup = setup(&root, scene.path(), "linux", cargo(&scene, &tool, 0));
    setup.env.push(("GITHUB_ACTIONS".into(), "true".into()));

    let (code, out) = call(&["check", "--all"], &setup);
    assert_eq!(code, 1, "{out}");
    for line in [
        "  > nebensound-repo setup --pinned --with-runtimes json\n",
        "    | setup --pinned --with-runtimes json GITHUB_ACTIONS=true\n",
        "check-json, passing case `good`: as expected\n",
        "check-json, failing case `bad`: as expected\n",
    ] {
        assert!(out.contains(line), "{line}\n{out}");
    }

    // A setup that fails names the call it made.
    write(&root, &[("setup-fails", "")]);
    let (code, out) = call(&["check", "--all"], &setup);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains(
            "::error::check-json: `nebensound-repo setup --pinned --with-runtimes json` failed, \
             so its cases run with the programs that are there\n"
        ),
        "{out}"
    );
}

#[test]
fn only_github_actions_set_to_true_is_a_run_on_github() {
    let env = |value: &str| vec![("GITHUB_ACTIONS".into(), value.into())];
    assert!(cases::on_github(&env("true")));
    assert!(!cases::on_github(&env("false")));
    assert!(!cases::on_github(&env("")));
    assert!(!cases::on_github(&[("CI".into(), "true".into())]));
    assert!(!cases::on_github(&[]));
}

#[test]
fn elsewhere_than_linux_only_the_blocks_from_the_tool_run() {
    let _alone = support::alone();
    let scene = Scene::new();
    let root = repository(&scene);
    let tool = scene.path().join("nebensound-repo");
    script(&tool, NEBENSOUND_REPO);
    let setup = setup(&root, scene.path(), "macos", cargo(&scene, &tool, 0));

    let (code, out) = call(&["check", "--all"], &setup);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("check-json, failing case `bad`: as expected\n"),
        "{out}"
    );
    assert!(!out.contains("::group::The tool\n"), "{out}");
    assert!(
        out.contains(
            "::error::For now the test cases of check-demo run only on Linux, since they run the \
             steps of the building blocks"
        ),
        "{out}"
    );
    assert!(out.ends_with("1 problem(s), each named above.\n"), "{out}");
}

#[test]
fn without_a_tool_no_case_of_a_block_from_the_tool_runs() {
    let _alone = support::alone();
    let scene = Scene::new();
    let root = repository(&scene);
    let tool = scene.path().join("nebensound-repo");
    script(&tool, NEBENSOUND_REPO);

    // Cargo fails.
    let (code, out) = call(
        &["check", "--all"],
        &setup(&root, scene.path(), "macos", cargo(&scene, &tool, 101)),
    );
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains(
            "::error::Cargo built no tool `nebensound-repo`, so no case of a block that runs from \
             the tool can run\n"
        ),
        "{out}"
    );

    // Cargo names nothing it built.
    let silent = scene.path().join("silent-cargo");
    script(&silent, "#!/bin/sh\nexit 0\n");
    let (code, out) = call(
        &["check", "--all"],
        &setup(&root, scene.path(), "macos", silent),
    );
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("::error::Cargo built no tool"), "{out}");

    // There is no Cargo.
    let (code, out) = call(
        &["check", "--all"],
        &setup(&root, scene.path(), "macos", scene.path().join("no-cargo")),
    );
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("::error::cannot start `") && out.contains("to build the tool"),
        "{out}"
    );

    // What Cargo names cannot be started.
    let (code, out) = call(
        &["check", "--all"],
        &setup(
            &root,
            scene.path(),
            "macos",
            cargo(&scene, &scene.path().join("gone"), 0),
        ),
    );
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("::error::check-json, passing case `good`: cannot be run: cannot start "),
        "{out}"
    );
    assert!(
        out.contains(
            "::error::check-json: the programs of its check cannot be set up: cannot start "
        ),
        "{out}"
    );
}

#[test]
fn a_case_that_breaks_off_cannot_be_run() {
    let _alone = support::alone();
    let scene = Scene::new();
    let env = xtask::runner::environment(std::env::vars_os().collect());
    let git = "git".into();
    let run = |marker: &str, scratch: &Path| {
        let dir = scene.path().join(format!("case-{marker}"));
        write(&dir, &[(marker, "")]);
        let tool = scene.path().join(format!("tool-{marker}"));
        script(&tool, NEBENSOUND_REPO);
        let case = Case {
            name: marker.to_owned(),
            dir,
            expect: Expect::Pass,
        };
        let bench = Direct {
            tool: &tool,
            env: &env,
            git: &git,
            scratch,
            with_runtimes: false,
        };
        let mut out = Vec::new();
        let verdict = cases::direct(bench, "check-json", &case, &mut out);
        (verdict, String::from_utf8(out).unwrap())
    };

    let file = scene.path().join("a file");
    fs::write(&file, "").unwrap();
    let (verdict, out) = run("anything", &file);
    let error = verdict.unwrap_err().to_string();
    assert!(
        error.contains("cannot make a directory for the case"),
        "{error}"
    );
    assert!(out.ends_with("::endgroup::\n"), "{out}");

    let scratch = scene.path().join("scratch");
    fs::create_dir_all(&scratch).unwrap();
    for (marker, message) in [
        ("vanish-at-plan", "cannot start"),
        ("garbage-plan", "the plan of phase 0 is no JSON"),
        ("vanish-at-run", "cannot start"),
    ] {
        let (verdict, out) = run(marker, &scratch);
        let error = verdict.unwrap_err().to_string();
        assert!(error.contains(message), "{marker}: {error}\n{out}");
    }
}

#[test]
fn a_block_from_the_tool_with_a_stray_case_stops_the_run() {
    let _alone = support::alone();
    let scene = Scene::new();
    let root = repository(&scene);
    let tool = scene.path().join("nebensound-repo");
    script(&tool, NEBENSOUND_REPO);
    write(
        &root,
        &[(".github/actions/check-json/tests/pass/stray", "")],
    );
    let (code, out) = call(
        &["check", "--all"],
        &setup(&root, scene.path(), "linux", cargo(&scene, &tool, 0)),
    );
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("stray is no case"), "{out}");
}
