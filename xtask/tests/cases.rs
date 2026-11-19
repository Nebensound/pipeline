//! The test cases of the building blocks, and running a block against one.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;
use std::path::Path;

use support::{Scene, write};
use xtask::cases::{self, Bench, Case, Expect, FINDING};
use xtask::runner::Job;

#[test]
fn cases_are_found_passing_first_each_kind_by_name() {
    let scene = Scene::new();
    let block = scene.path().join("check-demo");
    write(
        &block,
        &[
            ("tests/pass/b/file", ""),
            ("tests/pass/a/file", ""),
            ("tests/pass/.DS_Store", ""),
            ("tests/fail/.DS_Store", ""),
            (
                "tests/fail/z/expected-finding",
                "# A comment.\n\n  first *\nsecond\n",
            ),
        ],
    );
    let found = cases::discover(&block).unwrap();
    let names: Vec<(&str, &Expect)> = found
        .iter()
        .map(|case| (case.name.as_str(), &case.expect))
        .collect();
    let fail = Expect::Fail(vec!["first *".to_owned(), "second".to_owned()]);
    assert_eq!(
        names,
        [("a", &Expect::Pass), ("b", &Expect::Pass), ("z", &fail)]
    );
    assert_eq!(found[0].label(), "passing case `a`");
    assert_eq!(found[2].label(), "failing case `z`");
    assert!(
        cases::discover(&scene.path().join("no block"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn what_is_no_case_is_refused() {
    for (files, message) in [
        (
            &[("tests/pass/stray", "")][..],
            "is no case: a case is a directory",
        ),
        (&[("tests/fail/a/file", "")], "names no finding"),
        (
            &[("tests/fail/a/expected-finding", "# only a comment\n")],
            "names no finding",
        ),
        (
            &[("tests/pass/a/expected-finding", "x")],
            "only a failing case names a finding",
        ),
        (&[("tests/pass", "")], "cannot list"),
    ] {
        let scene = Scene::new();
        write(scene.path(), files);
        let error = cases::discover(scene.path()).unwrap_err();
        assert!(error.to_string().contains(message), "{files:?}: {error}");
    }
}

#[test]
fn a_pattern_matches_a_whole_line() {
    for (pattern, line, expected) in [
        ("exact", "exact", true),
        ("exact", "exact and more", false),
        ("exact", "not exact", false),
        ("start*", "start and more", true),
        ("*end", "the end", true),
        ("*end", "the end?", false),
        ("*middle*", "in the middle of it", true),
        ("a*b*c", "a-b-c", true),
        ("a*b*c", "a-c-b", false),
        ("a*b*c", "abc", true),
        ("a*b*b", "ab", false),
        ("a*x*c", "abc", false),
        ("*", "", true),
        ("**", "anything", true),
        ("", "", true),
        ("", "x", false),
    ] {
        assert_eq!(
            cases::matches(pattern, line),
            expected,
            "{pattern:?} on {line:?}"
        );
    }
}

#[test]
fn a_run_is_judged_against_what_the_case_expects() {
    let log = ["one line".to_owned(), "::error file=a::wrong".to_owned()];
    assert_eq!(cases::judge(&Expect::Pass, true, &log), None);
    assert!(
        cases::judge(&Expect::Pass, false, &log)
            .unwrap()
            .contains("meant to pass")
    );
    let fail = Expect::Fail(vec![
        "::error file=a::*".to_owned(),
        "missing*".to_owned(),
        "other".to_owned(),
    ]);
    assert!(
        cases::judge(&fail, true, &log)
            .unwrap()
            .contains("lets the mistake through")
    );
    assert_eq!(
        cases::judge(&fail, false, &log).unwrap(),
        "the block fails, but without the finding the case names: no line matches `missing*`, `other`"
    );
    assert_eq!(
        cases::judge(&Expect::Fail(vec!["one *".to_owned()]), false, &log),
        None
    );
}

#[test]
fn the_plan_names_the_units_of_a_check() {
    let plan = r#"{"units":[
        {"path":".","checks":["json","toml"]},
        {"path":"tools","checks":["rust"]},
        {"path":"other","checks":["json"]},
        {"path":"none"},
        {"checks":["json"]}
    ]}"#;
    assert_eq!(cases::units(plan, "json").unwrap(), [".", "other"]);
    assert!(cases::units(plan, "yaml").unwrap().is_empty());
    assert!(
        cases::units("not json", "json")
            .unwrap_err()
            .to_string()
            .contains("no JSON")
    );
    assert!(
        cases::units("{}", "json")
            .unwrap_err()
            .to_string()
            .contains("names no units")
    );
}

const PHASE0: &str = r#"
outputs:
  plan:
    value: ${{ steps.plan.outputs.plan }}
runs:
  using: composite
  steps:
    - shell: bash
      run: |
        if [ -f phase0-fails ]; then exit 1; fi
    - id: plan
      shell: bash
      run: |
        checks='["demo", "rust", "short", "missing"]'
        if [ -f no-units ]; then checks='[]'; fi
        if [ -f no-plan ]; then echo "plan=nothing" >> "$GITHUB_OUTPUT"; exit 0; fi
        echo "plan={\"units\":[{\"path\":\".\",\"checks\":$checks},{\"path\":\"sub\",\"checks\":$checks}]}" >> "$GITHUB_OUTPUT"
"#;

const BLOCK: &str = r#"
inputs:
  working-directory:
    required: true
runs:
  using: composite
  steps:
    - shell: bash
      env:
        UNIT: ${{ inputs.working-directory }}
      run: |
        if git ls-files | grep -q expected-finding; then echo "the finding is committed"; exit 1; fi
        if [ -f "$UNIT/bad" ]; then echo "::error file=$UNIT/bad::wrong"; exit 1; fi
        echo "$UNIT is fine"
"#;

/// A root with phase 0, the block `check-demo` and its copy `check-rust`,
/// and the cases `good`, `bad`, `phase0-fails`, `no-units` and `no-plan`.
fn root(scene: &Scene) -> std::path::PathBuf {
    let root = scene.path().join("root");
    write(
        &root,
        &[
            (".github/actions/phase0/action.yml", PHASE0),
            (".github/actions/check-demo/action.yml", BLOCK),
            (
                ".github/actions/check-rust/action.yml",
                &BLOCK.replace(
                    "  steps:\n",
                    "  steps:\n    - name: Make room on the runner\n      shell: bash\n      run: exit 1\n",
                ),
            ),
            (".github/actions/check-short/action.yml", "inputs:\n  working-directory: {}\nruns:\n  using: composite\n  steps: []\n"),
            ("cases/good/sub/file", ""),
            ("cases/good/file", ""),
            ("cases/bad/sub/bad", ""),
            ("cases/bad/expected-finding", "::error file=sub/bad::wrong\n"),
            ("cases/phase0-fails/phase0-fails", ""),
            ("cases/no-units/no-units", ""),
            ("cases/no-plan/no-plan", ""),
        ],
    );
    fs::create_dir_all(scene.path().join("scratch")).unwrap();
    root
}

fn run(
    scene: &Scene,
    block: &str,
    case: &str,
    expect: Expect,
) -> (Result<Option<String>, xtask::Error>, String) {
    let root = scene.path().join("root");
    run_in(
        scene,
        &root,
        &scene.path().join("scratch"),
        block,
        case,
        expect,
    )
}

fn run_in(
    scene: &Scene,
    root: &Path,
    scratch: &Path,
    block: &str,
    case: &str,
    expect: Expect,
) -> (Result<Option<String>, xtask::Error>, String) {
    let host = scene.host();
    let job = Job::default();
    let case = Case {
        name: case.to_owned(),
        dir: root.join("cases").join(case),
        expect,
    };
    let mut out = Vec::new();
    let verdict = cases::run(
        Bench {
            host: &host,
            job: &job,
            root,
            scratch,
        },
        block,
        &case,
        &mut out,
    );
    (verdict, String::from_utf8(out).unwrap())
}

fn finding() -> Expect {
    Expect::Fail(vec!["::error file=sub/bad::wrong".to_owned()])
}

#[test]
fn a_passing_case_passes_and_a_failing_one_fails_with_its_finding() {
    let scene = Scene::new();
    root(&scene);
    let (verdict, out) = run(&scene, "check-demo", "good", Expect::Pass);
    assert_eq!(verdict.unwrap(), None, "{out}");
    assert!(
        out.starts_with("::group::check-demo, passing case `good`\n"),
        "{out}"
    );
    assert!(out.ends_with("::endgroup::\n"), "{out}");
    assert!(
        out.contains("    | . is fine") && out.contains("    | sub is fine"),
        "{out}"
    );

    let (verdict, out) = run(&scene, "check-demo", "bad", finding());
    assert_eq!(verdict.unwrap(), None, "{out}");

    let (verdict, out) = run(&scene, "check-demo", "bad", Expect::Pass);
    assert!(verdict.unwrap().unwrap().contains("meant to pass"), "{out}");
}

#[test]
fn a_step_the_xtask_leaves_out_does_not_run() {
    let scene = Scene::new();
    root(&scene);
    let (verdict, out) = run(&scene, "check-rust", "good", Expect::Pass);
    assert_eq!(verdict.unwrap(), None, "{out}");
    assert!(
        out.contains("  - Make room on the runner: left out by the xtask"),
        "{out}"
    );
}

#[test]
fn a_case_the_block_never_reaches_says_why() {
    let scene = Scene::new();
    root(&scene);
    let (verdict, out) = run(&scene, "check-demo", "phase0-fails", Expect::Pass);
    assert!(
        verdict
            .unwrap()
            .unwrap()
            .contains("phase 0 fails on the case"),
        "{out}"
    );
    let (verdict, out) = run(&scene, "check-demo", "no-units", Expect::Pass);
    assert!(
        verdict
            .unwrap()
            .unwrap()
            .contains("no unit of the case starts the check `demo`"),
        "{out}"
    );
}

#[test]
fn a_case_that_cannot_run_is_an_error() {
    let scene = Scene::new();
    let root = root(&scene);
    let scratch = scene.path().join("scratch");
    for (block, case, message) in [
        ("build-demo", "good", "is no check"),
        ("check-demo", "missing", "`cp -R"),
        ("check-demo", "no-plan", "no JSON"),
        ("check-missing", "good", "cannot read"),
    ] {
        let (verdict, out) = run_in(&scene, &root, &scratch, block, case, Expect::Pass);
        let error = verdict.unwrap_err().to_string();
        assert!(error.contains(message), "{block} {case}: {error}\n{out}");
        assert!(out.ends_with("::endgroup::\n"), "{out}");
    }

    let file = scene.path().join("a file");
    fs::write(&file, "").unwrap();
    let (verdict, _) = run_in(&scene, &root, &file, "check-demo", "good", Expect::Pass);
    assert!(
        verdict
            .unwrap_err()
            .to_string()
            .contains("cannot make a directory for the case")
    );

    let bare = scene.path().join("bare");
    write(&bare, &[("cases/good/file", "")]);
    let (verdict, _) = run_in(&scene, &bare, &scratch, "check-demo", "good", Expect::Pass);
    assert!(verdict.unwrap_err().to_string().contains("phase0"));

    // A block without the step the xtask leaves out of it.
    write(
        &root,
        &[(
            ".github/actions/check-rust/action.yml",
            "inputs:\n  working-directory: {}\nruns:\n  using: composite\n  steps: []\n",
        )],
    );
    let (verdict, _) = run_in(&scene, &root, &scratch, "check-rust", "good", Expect::Pass);
    assert!(
        verdict
            .unwrap_err()
            .to_string()
            .contains("no such step any more")
    );
}

#[test]
fn the_finding_is_not_part_of_the_case() {
    let scene = Scene::new();
    root(&scene);
    assert!(
        scene.path().join("root/cases/bad").join(FINDING).is_file(),
        "the case holds it"
    );
    let (verdict, out) = run(&scene, "check-short", "bad", finding());
    assert!(
        verdict
            .unwrap()
            .unwrap()
            .contains("lets the mistake through"),
        "{out}"
    );
}
