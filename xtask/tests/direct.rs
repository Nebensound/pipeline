//! The cases of the blocks whose work the tool carries out: run through the
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
      - name: Publish targets not written yet
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

/// A block whose steps are never run: its check runs from the tool.
const BLOCK: &str = r"
inputs:
  working-directory: {}
runs:
  using: composite
  steps:
    - shell: bash
      run: exit 1
";

/// The same, for a check that takes the plan of phase 0 as well.
const PLANNED: &str = r"
inputs:
  working-directory: {}
  plan: {}
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
    [ -f plan-unwritable ] && mkdir ../plan.json
    if [ -f no-unit ]; then echo '{"units":[]}'
    elif [ -f versioned ]; then echo '{"units":[{"kind":"component","name":"a","path":"a","checks":[],"build":"cargo-package"},{"kind":"component","name":"b","path":"b","checks":[],"build":"cargo-package"}],"versions":[{"kind":"component","name":"a","path":"a","version":"1.0.0","writers":["cargo"]},{"kind":"component","name":"b","path":"b","version":"1.0.0","writers":[]}]}'
    elif [ -f other-writer ]; then echo '{"units":[{"kind":"component","name":"a","path":"a","checks":[],"build":"cargo-package"}],"versions":[{"kind":"component","name":"a","path":"a","version":"1.0.0","writers":["other"]}]}'
    elif [ -f unversioned ]; then echo '{"units":[{"kind":"component","name":"a","path":"a","checks":[],"build":"cargo-package"}],"versions":[{"kind":"component","name":"a","path":"a","version":null,"writers":[]}]}'
    elif [ -f planned ] || [ -f plan-unwritable ]; then echo '{"units":[{"path":".","checks":["planned"]}]}'
    else echo '::stop-commands::t'; echo '{"units":[{"path":".","checks":["json"]}]}'; echo '::t::'; fi
    [ -f vanish-at-run ] && rm -f "$0"
    exit 0 ;;
  run)
    shift
    git ls-files | grep -q -e expected-finding -e forbidden-line && { echo "the finding is committed"; exit 1; }
    if [ "$1" = "--plan" ]; then echo "plan: $(cat "$2")"; shift 2; fi
    echo "run $*"
    [ -f bad ] && { echo "::error file=bad::wrong"; exit 1; }
    exit 0 ;;
  write-versions)
    [ "$2" = "--plan" ] && [ -f "$3" ] || { echo "no plan: $*"; exit 2; }
    echo "write-versions"
    [ -f write-fails ] && { echo "cannot write"; exit 1; }
    exit 0 ;;
  build)
    shift
    if [ "$1" = "--plan" ]; then echo "plan given"; shift 2; fi
    echo "build $*"
    [ -d "$RUNNER_TEMP" ] && [ "$(basename "$RUNNER_TEMP")" = runner ] && echo "in the case's temp"
    [ -f build-fails ] && { echo "::error::it fails"; exit 1; }
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
/// `check-demo`, whose work the tool does not carry out; each case of
/// `check-json` names what it expects.
fn repository(scene: &Scene) -> PathBuf {
    let root = scene.path().join("root");
    write(
        &root,
        &[
            ("register.toml", REGISTER),
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
            (
                ".github/actions/check-json/tests/fail/bad/forbidden-line",
                "::warning::*\n",
            ),
        ],
    );
    root
}

/// A run on a developer's computer: without `GITHUB_ACTIONS`, even when the
/// tests themselves run on GitHub Actions.
fn setup(root: &Path, scratch: &Path, cargo: PathBuf) -> Setup {
    Setup {
        root: root.to_path_buf(),
        env: std::env::vars_os()
            .filter(|(name, _)| name != "GITHUB_ACTIONS")
            .collect(),
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
    let setup = setup(&root, scene.path(), cargo(&scene, &tool, 0));

    let (code, out) = call(&["check", "--all"], &setup);
    assert_eq!(code, 1, "{out}");
    for line in [
        "::group::The tool, built with Cargo\n",
        "    | warning: from cargo\n",
        "::group::check-json, the programs it calls\n",
        "  > nebensound-repo setup --pinned json\n",
        "    | setup --pinned json GITHUB_ACTIONS=true\n",
        "check-json, passing case `good`: as expected\n",
        "check-json, failing case `bad`: as expected\n",
        "  > nebensound-repo check --name=case --default-branch=main --visibility=public\n",
        "    | check --name=case --default-branch=main --visibility=public GITHUB_ACTIONS=true\n",
        "  > nebensound-repo run json -- .\n",
        "    | run json -- .\n",
        "::error::check-demo: the tool carries out none of its work, so its cases cannot run: \
         the xtask runs the cases of a block through the tool only\n",
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
    // The cases of a block the tool does not carry out do not run.
    assert!(!out.contains("check-demo, passing case"), "{out}");

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
    let mut setup = setup(&root, scene.path(), cargo(&scene, &tool, 0));
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

/// Takes the check `name` out of the repository at `root`: its block, its
/// register entry and its step.
fn without(root: &Path, name: &str) {
    fs::remove_dir_all(root.join(format!(".github/actions/check-{name}"))).unwrap();
    let register = REGISTER.replace(&format!("[check.{name}]\nextensions = [\".{name}\"]\n"), "");
    let workflow = fs::read_to_string(root.join(".github/workflows/nebensound.yml")).unwrap();
    let start = workflow.find(&format!("      - name: {name}\n")).unwrap();
    let end = start + 1 + workflow[start + 1..].find("      - name: ").unwrap();
    let workflow = format!("{}{}", &workflow[..start], &workflow[end..]).replace(
        r#"'["demo", "json"]'"#,
        &format!(r#"'["{}"]'"#, if name == "demo" { "json" } else { "demo" }),
    );
    write(
        root,
        &[
            ("register.toml", &register),
            (".github/workflows/nebensound.yml", &workflow),
        ],
    );
}

#[test]
fn when_every_case_holds_nothing_is_reported() {
    let _alone = support::alone();
    let scene = Scene::new();
    let root = repository(&scene);
    without(&root, "demo");
    let tool = scene.path().join("nebensound-repo");
    script(&tool, NEBENSOUND_REPO);

    let (code, out) = call(
        &["check", "--all"],
        &setup(&root, scene.path(), cargo(&scene, &tool, 0)),
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("They agree.\n"), "{out}");
    assert!(
        out.contains("check-json, failing case `bad`: as expected\n"),
        "{out}"
    );
    assert!(
        out.ends_with("Every building block holds to its test cases.\n"),
        "{out}"
    );

    // Without room for the run, nothing runs.
    let file = scene.path().join("a file");
    fs::write(&file, "").unwrap();
    let (code, out) = call(
        &["check", "--all"],
        &setup(&root, &file, cargo(&scene, &tool, 0)),
    );
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("::error::cannot make a directory for the run in "),
        "{out}"
    );
    assert!(!out.contains("::group::"), "{out}");
    assert!(out.ends_with("1 problem(s), each named above.\n"), "{out}");
}

#[test]
fn a_disagreement_of_the_layout_alone_fails_the_run() {
    let _alone = support::alone();
    let scene = Scene::new();
    let root = repository(&scene);
    without(&root, "demo");
    let tool = scene.path().join("nebensound-repo");
    script(&tool, NEBENSOUND_REPO);
    // Every case holds; only the block's step carries a key a block's step
    // may not carry.
    write(
        &root,
        &[(
            ".github/actions/check-json/action.yml",
            &BLOCK.replace(
                "      run: exit 1\n",
                "      run: exit 1\n      continue-on-error: true\n",
            ),
        )],
    );

    let (code, out) = call(
        &["check", "--all"],
        &setup(&root, scene.path(), cargo(&scene, &tool, 0)),
    );
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("::error::step 1 of `check-json` carries `continue-on-error`: "),
        "{out}"
    );
    assert!(!out.contains("They agree.\n"), "{out}");
    for case in ["passing case `good`", "failing case `bad`"] {
        assert!(
            out.contains(&format!("check-json, {case}: as expected\n")),
            "{out}"
        );
    }
    assert!(out.ends_with("1 problem(s), each named above.\n"), "{out}");
}

#[test]
fn without_a_block_from_the_tool_no_tool_is_built() {
    let _alone = support::alone();
    let scene = Scene::new();
    let root = repository(&scene);
    without(&root, "json");

    // Cargo is not even there: nothing needs it.
    let (code, out) = call(
        &["check", "--all"],
        &setup(&root, scene.path(), scene.path().join("no-cargo")),
    );
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("They agree.\n"), "{out}");
    assert!(!out.contains("::group::"), "{out}");
    assert!(
        out.contains("::error::check-demo: the tool carries out none of its work"),
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
        &setup(&root, scene.path(), cargo(&scene, &tool, 101)),
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
    let (code, out) = call(&["check", "--all"], &setup(&root, scene.path(), silent));
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("::error::Cargo built no tool"), "{out}");

    // There is no Cargo.
    let (code, out) = call(
        &["check", "--all"],
        &setup(&root, scene.path(), scene.path().join("no-cargo")),
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
            cargo(&scene, &scene.path().join("gone"), 0),
        ),
    );
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("::error::check-json, passing case `good`: cannot be run: cannot start "),
        "{out}"
    );
    assert!(
        out.contains("::error::check-json: the programs it calls cannot be set up: cannot start "),
        "{out}"
    );
}

#[test]
fn a_case_that_breaks_off_cannot_be_run() {
    let _alone = support::alone();
    let scene = Scene::new();
    let env = xtask::process::environment(std::env::vars_os().collect());
    let git = "git".into();
    let root = repository(&scene);
    write(
        &root,
        &[(".github/actions/check-planned/action.yml", PLANNED)],
    );
    let run = |marker: &str, scratch: &Path, block: &str| {
        let dir = scene.path().join(format!("case-{marker}"));
        write(&dir, &[(marker, "")]);
        let tool = scene.path().join(format!("tool-{marker}"));
        script(&tool, NEBENSOUND_REPO);
        let case = Case {
            name: marker.to_owned(),
            dir,
            expect: Expect::Pass,
            forbidden: Vec::new(),
        };
        let bench = Direct {
            tool: &tool,
            root: &root,
            env: &env,
            git: &git,
            scratch,
            with_runtimes: false,
        };
        let mut out = Vec::new();
        let verdict = cases::direct(bench, block, &case, &mut out);
        (verdict, String::from_utf8(out).unwrap())
    };

    let file = scene.path().join("a file");
    fs::write(&file, "").unwrap();
    let (verdict, out) = run("anything", &file, "check-json");
    let error = verdict.unwrap_err().to_string();
    assert!(
        error.contains("cannot make a directory for the case"),
        "{error}"
    );
    assert!(out.ends_with("::endgroup::\n"), "{out}");

    let scratch = scene.path().join("scratch");
    fs::create_dir_all(&scratch).unwrap();
    for (marker, block, message) in [
        ("vanish-at-plan", "check-json", "cannot start"),
        (
            "garbage-plan",
            "check-json",
            "the plan of phase 0 is no JSON",
        ),
        ("vanish-at-run", "check-json", "cannot start"),
        (
            "plan-unwritable",
            "check-planned",
            "cannot write the plan to ",
        ),
        ("anything", "check-missing", "cannot read"),
    ] {
        let (verdict, out) = run(marker, &scratch, block);
        let error = verdict.unwrap_err().to_string();
        assert!(error.contains(message), "{marker}: {error}\n{out}");
    }

    // A block that takes the plan hands it to the tool in a file, as its step
    // hands it to the block; one that does not takes none.
    // A case whose directory is gone cannot be copied.
    let tool = scene.path().join("tool-gone");
    script(&tool, NEBENSOUND_REPO);
    let bench = Direct {
        tool: &tool,
        root: &root,
        env: &env,
        git: &git,
        scratch: &scratch,
        with_runtimes: false,
    };
    let gone = Case {
        name: "gone".to_owned(),
        dir: scene.path().join("gone"),
        expect: Expect::Pass,
        forbidden: Vec::new(),
    };
    let error = cases::direct(bench, "check-json", &gone, &mut Vec::new()).unwrap_err();
    assert!(error.to_string().contains("`cp -R"), "{error}");

    let (verdict, out) = run("planned", &scratch, "check-planned");
    assert_eq!(verdict.unwrap(), None, "{out}");
    assert!(
        out.contains(r#"    | plan: {"units":[{"path":".","checks":["planned"]}]}"#),
        "{out}"
    );
    assert!(out.contains("    | run planned -- .\n"), "{out}");
    assert!(out.contains("  > nebensound-repo run --plan "), "{out}");
    let (verdict, out) = run("plain", &scratch, "check-json");
    assert_eq!(verdict.unwrap(), None, "{out}");
    assert!(!out.contains("plan: "), "{out}");
}

/// The repository of the blocks with a version writer and builds that run
/// from the tool, and a way to run one of them against a case of these
/// markers.
struct Blocks {
    scene: Scene,
    root: PathBuf,
    scratch: PathBuf,
}

impl Blocks {
    fn new() -> Self {
        let scene = Scene::new();
        let root = repository(&scene);
        write(
            &root,
            &[
                (".github/actions/version-cargo/action.yml", PLANNED),
                (".github/actions/version-other/action.yml", PLANNED),
                (".github/actions/build-cargo-package/action.yml", BLOCK),
                (".github/actions/build-planned/action.yml", PLANNED),
                (".github/actions/other-thing/action.yml", BLOCK),
            ],
        );
        let scratch = scene.path().join("scratch");
        fs::create_dir_all(&scratch).unwrap();
        Self {
            scene,
            root,
            scratch,
        }
    }

    fn run(
        &self,
        markers: &[&str],
        block: &str,
        expect: Expect,
    ) -> (Result<Option<String>, xtask::Error>, String) {
        let dir = self
            .scene
            .path()
            .join(format!("case-{}-{block}", markers.join("-")));
        let files: Vec<(&str, &str)> = markers.iter().map(|marker| (*marker, "")).collect();
        write(&dir, &files);
        let tool = self.scene.path().join("tool");
        script(&tool, NEBENSOUND_REPO);
        let case = Case {
            name: "case".to_owned(),
            dir,
            expect,
            forbidden: Vec::new(),
        };
        let env = xtask::process::environment(std::env::vars_os().collect());
        let bench = Direct {
            tool: &tool,
            root: &self.root,
            env: &env,
            git: &"git".into(),
            scratch: &self.scratch,
            with_runtimes: false,
        };
        let mut out = Vec::new();
        let verdict = cases::direct(bench, block, &case, &mut out);
        (verdict, String::from_utf8(out).unwrap())
    }
}

#[test]
fn a_version_writer_from_the_tool_writes_once_with_the_plan() {
    let _alone = support::alone();
    let blocks = Blocks::new();
    // The version writer writes once, with the plan.
    let (verdict, out) = blocks.run(&["versioned"], "version-cargo", Expect::Pass);
    assert_eq!(verdict.unwrap(), None, "{out}");
    assert!(
        out.contains("  > nebensound-repo write-versions --plan "),
        "{out}"
    );
    assert!(
        out.contains(
            "    | write-versions
"
        ),
        "{out}"
    );
    let (verdict, out) = blocks.run(
        &["versioned", "write-fails"],
        "version-cargo",
        Expect::Fail(vec!["cannot write".to_owned()]),
    );
    assert_eq!(verdict.unwrap(), None, "{out}");
    let (verdict, out) = blocks.run(&["unversioned"], "version-cargo", Expect::Pass);
    assert_eq!(
        verdict.unwrap().as_deref(),
        Some("no unit of the case selects the version writer `cargo`, so the block never runs"),
        "{out}"
    );
}

#[test]
fn a_build_from_the_tool_runs_after_the_writers_of_its_unit() {
    let _alone = support::alone();
    let blocks = Blocks::new();
    // A build runs once per unit with a version of the run, after the
    // writers that unit selects, and only when they passed.
    let (verdict, out) = blocks.run(&["versioned"], "build-cargo-package", Expect::Pass);
    assert_eq!(verdict.unwrap(), None, "{out}");
    let writes = out
        .matches(
            "    | write-versions
",
        )
        .count();
    assert_eq!(writes, 1, "{out}");
    let written = out
        .find(
            "    | write-versions
",
        )
        .unwrap();
    let built = out
        .find(
            "    | build cargo-package -- a
",
        )
        .unwrap();
    assert!(written < built, "{out}");
    assert!(
        out.contains(
            "    | build cargo-package -- b
"
        ),
        "{out}"
    );
    assert!(!out.contains("plan given"), "{out}");
    // What a build leaves for the step after it lies in the case's folder.
    assert!(out.contains("    | in the case's temp\n"), "{out}");
    let (verdict, out) = blocks.run(
        &["versioned", "write-fails"],
        "build-cargo-package",
        Expect::Fail(vec!["cannot write".to_owned()]),
    );
    assert_eq!(verdict.unwrap(), None, "{out}");
    assert!(!out.contains("build cargo-package -- a"), "{out}");
    let (verdict, out) = blocks.run(
        &["versioned", "build-fails"],
        "build-cargo-package",
        Expect::Fail(vec!["::error::it fails".to_owned()]),
    );
    assert_eq!(verdict.unwrap(), None, "{out}");
    let (verdict, out) = blocks.run(&["versioned"], "build-planned", Expect::Pass);
    assert_eq!(
        verdict.unwrap().as_deref(),
        Some(
            "no unit of the case starts the build `planned` with a version of the run, so the \
             block never runs"
        ),
        "{out}"
    );
    let (verdict, out) = blocks.run(&["unversioned"], "build-cargo-package", Expect::Pass);
    assert!(verdict.unwrap().is_some(), "{out}");

    // A build block that takes the plan gets it as a check block does.
    let dir = blocks
        .root
        .join(".github/actions/build-cargo-package/action.yml");
    fs::write(&dir, PLANNED).unwrap();
    let (verdict, out) = blocks.run(&["versioned"], "build-cargo-package", Expect::Pass);
    assert_eq!(verdict.unwrap(), None, "{out}");
    assert!(
        out.contains(
            "    | plan given
"
        ),
        "{out}"
    );
}

#[test]
fn a_writer_or_a_block_the_tool_does_not_know_cannot_be_run() {
    let _alone = support::alone();
    let blocks = Blocks::new();
    // A writer the tool does not write for, and a block of no known kind,
    // cannot be run.
    let (verdict, out) = blocks.run(&["other-writer"], "version-other", Expect::Pass);
    let error = verdict.unwrap_err().to_string();
    assert!(
        error.contains("the version writer `other`"),
        "{error}\n{out}"
    );
    let (verdict, out) = blocks.run(&["other-writer"], "build-cargo-package", Expect::Pass);
    let error = verdict.unwrap_err().to_string();
    assert!(
        error.contains("the version writer `other`"),
        "{error}\n{out}"
    );
    let (verdict, out) = blocks.run(&["versioned"], "other-thing", Expect::Pass);
    let error = verdict.unwrap_err().to_string();
    assert!(
        error.contains("is no check, version writer or build"),
        "{error}\n{out}"
    );

    // A plan that is none, and a tool that is gone before the writers run.
    for (markers, block, message) in [
        (
            &["no-unit"][..],
            "version-cargo",
            "the plan of phase 0 names no versions",
        ),
        (
            &["garbage-plan"][..],
            "build-cargo-package",
            "the plan of phase 0 is no JSON",
        ),
        (
            &["versioned", "vanish-at-run"][..],
            "build-cargo-package",
            "cannot start",
        ),
    ] {
        let (verdict, out) = blocks.run(markers, block, Expect::Pass);
        let error = verdict.unwrap_err().to_string();
        assert!(error.contains(message), "{markers:?}: {error}\n{out}");
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
        &setup(&root, scene.path(), cargo(&scene, &tool, 0)),
    );
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("stray is no case"), "{out}");
    // The layout names the stray entry, the run stops at it, and the cases of
    // `check-demo` cannot run: each is counted.
    assert!(out.ends_with("3 problem(s), each named above.\n"), "{out}");
}

#[test]
fn a_version_writer_needs_no_programs_set_up_and_a_build_gets_its_own() {
    let _alone = support::alone();
    let scene = Scene::new();
    let env = Vec::new();
    let git = "git".into();
    let tool = scene.path().join("tool");
    script(&tool, NEBENSOUND_REPO);
    let root = repository(&scene);
    let bench = Direct {
        tool: &tool,
        root: &root,
        env: &env,
        git: &git,
        scratch: scene.path(),
        with_runtimes: false,
    };
    let mut out = Vec::new();
    assert_eq!(cases::set_up(bench, &root, "version-cargo", &mut out), None);
    assert!(out.is_empty());
    for block in ["build-cargo-package", "build-docker"] {
        let mut out = Vec::new();
        assert_eq!(cases::set_up(bench, &root, block, &mut out), None);
        let out = String::from_utf8(out).unwrap();
        let name = block.strip_prefix("build-").unwrap();
        assert!(
            out.contains(&format!("  > nebensound-repo setup --pinned {name}\n")),
            "{out}"
        );
    }
}
