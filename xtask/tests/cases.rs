//! The test cases of the building blocks, and running a block against one.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;
use std::path::Path;

use support::{Scene, write};
use xtask::cases::{self, Bench, Case, Expect, FINDING, FORBIDDEN, Read};
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
    assert_eq!(cases::judge(&Expect::Pass, &[], true, &log), None);
    assert!(
        cases::judge(&Expect::Pass, &[], false, &log)
            .unwrap()
            .contains("meant to pass")
    );
    let fail = Expect::Fail(vec![
        "::error file=a::*".to_owned(),
        "missing*".to_owned(),
        "other".to_owned(),
    ]);
    assert!(
        cases::judge(&fail, &[], true, &log)
            .unwrap()
            .contains("lets the mistake through")
    );
    assert_eq!(
        cases::judge(&fail, &[], false, &log).unwrap(),
        "the block fails, but without the finding the case names: no line matches `missing*`, `other`"
    );
    assert_eq!(
        cases::judge(&Expect::Fail(vec!["one *".to_owned()]), &[], false, &log),
        None
    );
}

#[test]
fn a_case_may_forbid_commands() {
    let scene = Scene::new();
    let block = scene.path().join("check-demo");
    write(
        &block,
        &[
            (
                "tests/pass/a/forbidden-line",
                "# A comment.\n\n  ::warning::x*\n",
            ),
            ("tests/fail/b/expected-finding", "x"),
            ("tests/fail/b/forbidden-line", "# A comment.\n##[warning]x*"),
            ("tests/fail/c/expected-finding", "x"),
        ],
    );
    let found = cases::discover(&block).unwrap();
    let forbidden: Vec<(&str, &[String])> = found
        .iter()
        .map(|case| (case.name.as_str(), case.forbidden.as_slice()))
        .collect();
    assert_eq!(
        forbidden,
        [
            ("a", &["::warning::x*".to_owned()][..]),
            ("b", &["##[warning]x*".to_owned()][..]),
            ("c", &[][..]),
        ]
    );

    let scene = Scene::new();
    write(
        scene.path(),
        &[("tests/pass/a/forbidden-line", "# only a comment\n")],
    );
    let error = cases::discover(scene.path()).unwrap_err();
    assert!(error.to_string().contains("names no command"), "{error}");
}

#[test]
fn the_log_is_read_the_way_the_runner_reads_it() {
    let log: Vec<String> = [
        "before",
        "  ::stop-commands::t1",
        "::warning::inside",
        "a ##[warning]inside",
        "::other::not the token",
        "::error::not the token either ##[t1]",
        "  ::T1::",
        "after",
        "::stop-commands-not::x",
        "x ##[stop-commands]t2",
        "inside again",
        "y ##[t2] resumes",
        "last",
    ]
    .map(str::to_owned)
    .to_vec();
    let read: Vec<Read> = cases::read(&log)
        .into_iter()
        .map(|(read, _)| read)
        .collect();
    assert_eq!(
        read,
        [
            Read::Live,
            Read::Switch,
            Read::Stopped,
            Read::Stopped,
            Read::Stopped,
            Read::Stopped,
            Read::Switch,
            Read::Live,
            Read::Live,
            Read::Switch,
            Read::Stopped,
            Read::Switch,
            Read::Live,
        ]
    );
    assert_eq!(
        cases::printed(&log),
        [
            "before",
            "::warning::inside",
            "a ##[warning]inside",
            "::other::not the token",
            "::error::not the token either ##[t1]",
            "after",
            "::stop-commands-not::x",
            "inside again",
            "last"
        ]
    );
}

#[test]
fn the_command_of_a_line_is_the_one_the_runner_reads() {
    for (line, expected) in [
        ("::warning::x", Some("::warning::x")),
        ("  ::Warning::x", Some("::Warning::x")),
        (
            "  ::error file=a##[warning]b::m",
            Some("::error file=a##[warning]b::m"),
        ),
        ("[warn] ::warning::x", None),
        ("::no second", None),
        ("::no second ##[warning]x", Some("##[warning]x")),
        ("::unknown::##[warning]x", Some("##[warning]x")),
        ("text ##[warning]x and more", Some("##[warning]x and more")),
        ("text ##[warning file=a]x", Some("##[warning file=a]x")),
        ("text ##[unknown]x", None),
        ("text ##[warning", None),
        ("plain", None),
    ] {
        assert_eq!(cases::command(line), expected, "{line:?}");
    }
}

#[test]
fn a_command_counts_only_outside_a_stopped_stretch() {
    let log: Vec<String> = [
        "::warning::x before",
        "::stop-commands::t",
        "::warning::x inside",
        "a ##[warning]y inside",
        "::error file=a::stopped",
        "plain text inside",
        "::t::",
        "::error file=b::outside",
        "b ##[warning]y outside",
    ]
    .map(str::to_owned)
    .to_vec();
    let forbidden = ["::warning::x*".to_owned(), "##[warning]y*".to_owned()];
    assert_eq!(
        cases::judge(&Expect::Pass, &forbidden, true, &log).unwrap(),
        "the block starts a command the case forbids: `::warning::x before` (`::warning::x*`), \
         `##[warning]y outside` (`##[warning]y*`)"
    );
    // The tool's own annotation on a file whose name holds the older form is
    // the command `error`, and starts no warning.
    let own = ["::error file=a##[warning]y.dockerfile::m".to_owned()];
    assert_eq!(cases::judge(&Expect::Pass, &forbidden, true, &own), None);
    assert_eq!(
        cases::judge(&Expect::Pass, &forbidden, true, &log[1..8]),
        None
    );

    // An annotation counts outside a stopped stretch only; any other line
    // anywhere.
    let stopped = Expect::Fail(vec!["::error file=a::*".to_owned()]);
    assert!(
        cases::judge(&stopped, &[], false, &log)
            .unwrap()
            .contains("no line matches `::error file=a::*`")
    );
    let found = Expect::Fail(vec![
        "::error file=b::*".to_owned(),
        "plain text*".to_owned(),
    ]);
    assert_eq!(cases::judge(&found, &[], false, &log), None);
    let switch = Expect::Fail(vec!["::t::".to_owned()]);
    assert!(cases::judge(&switch, &[], false, &log).is_some());
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

#[test]
fn the_plan_names_the_units_of_a_version_writer() {
    let plan = r#"{"versions":[
        {"path":".","writers":[]},
        {"path":"lib","writers":["cargo"]},
        {"path":"doc","writers":["asciidoc","cargo"]},
        {"path":"none"},
        {"writers":["cargo"]}
    ]}"#;
    assert_eq!(cases::writers(plan, "cargo").unwrap(), ["lib", "doc"]);
    assert!(cases::writers(plan, "other").unwrap().is_empty());
    assert!(
        cases::writers("not json", "cargo")
            .unwrap_err()
            .to_string()
            .contains("no JSON")
    );
    assert!(
        cases::writers("{}", "cargo")
            .unwrap_err()
            .to_string()
            .contains("names no versions")
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
        checks='["demo", "rust", "short", "missing", "planned"]'
        writers='["demo"]'
        build='"demo"'
        if [ -f no-units ]; then checks='[]'; writers='[]'; build=null; fi
        version='"0.1.0"'
        if [ -f ghost-writer ]; then writers='["ghost"]'; fi
        if [ -f untouched ]; then version=null; fi
        if [ -f no-plan ]; then echo "plan=nothing" >> "$GITHUB_OUTPUT"; exit 0; fi
        sub='"kind":"component","name":"sub","path":"sub"'
        echo "plan={\"units\":[{\"kind\":\"repository\",\"name\":null,\"path\":\".\",\"checks\":$checks,\"build\":null},{$sub,\"checks\":$checks,\"build\":$build}],\"versions\":[{$sub,\"version\":$version,\"writers\":$writers}]}" >> "$GITHUB_OUTPUT"
"#;

/// A version writer's block: it gets the plan and no path.
const WRITER: &str = r#"
inputs:
  plan:
    required: true
runs:
  using: composite
  steps:
    - shell: bash
      env:
        PLAN: ${{ inputs.plan }}
      run: |
        case "$PLAN" in *'"versions"'*) ;; *) echo "no plan"; exit 1 ;; esac
        if [ -f sub/bad ]; then echo "::error file=sub/bad::wrong"; exit 1; fi
        touch sub/written
        echo "written once"
"#;

/// A build block: it gets the path, and builds only what the writer wrote.
const BUILD: &str = r#"
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
        if [ ! -f "$UNIT/written" ]; then echo "$UNIT has no version"; exit 1; fi
        if [ -f "$UNIT/unbuildable" ]; then echo "::error file=$UNIT/unbuildable::wrong"; exit 1; fi
        echo "$UNIT built"
"#;

/// A check block that takes the plan beside its path.
const PLANNED: &str = r#"
inputs:
  working-directory:
    required: true
  plan:
    required: false
    default: ""
runs:
  using: composite
  steps:
    - shell: bash
      env:
        UNIT: ${{ inputs.working-directory }}
        PLAN: ${{ inputs.plan }}
      run: |
        case "$PLAN" in *'"units"'*) echo "$UNIT has the plan" ;; *) echo "no plan"; exit 1 ;; esac
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

/// A root with phase 0, the block `check-demo`, `check-planned` that takes
/// the plan, the version writer `version-demo`, the build `build-demo`, and
/// the cases `good`, `bad`, `unbuildable`, `ghost-writer`, `phase0-fails`,
/// `no-units`, `untouched` and `no-plan`.
fn root(scene: &Scene) -> std::path::PathBuf {
    let root = scene.path().join("root");
    write(
        &root,
        &[
            (".github/actions/phase0/action.yml", PHASE0),
            (".github/actions/check-demo/action.yml", BLOCK),
            (
                ".github/actions/check-short/action.yml",
                "inputs:\n  working-directory: {}\nruns:\n  using: composite\n  steps: []\n",
            ),
            (".github/actions/check-planned/action.yml", PLANNED),
            (".github/actions/version-demo/action.yml", WRITER),
            (".github/actions/build-demo/action.yml", BUILD),
            ("cases/good/sub/file", ""),
            ("cases/unbuildable/sub/unbuildable", ""),
            (
                "cases/unbuildable/expected-finding",
                "::error file=sub/unbuildable::wrong\n",
            ),
            ("cases/ghost-writer/ghost-writer", ""),
            ("cases/good/file", ""),
            ("cases/bad/sub/bad", ""),
            (
                "cases/bad/expected-finding",
                "::error file=sub/bad::wrong\n",
            ),
            ("cases/bad/forbidden-line", "::never::*\n"),
            ("cases/phase0-fails/phase0-fails", ""),
            ("cases/no-units/no-units", ""),
            ("cases/untouched/untouched", ""),
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
        forbidden: Vec::new(),
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
fn a_check_that_takes_the_plan_gets_it() {
    let scene = Scene::new();
    root(&scene);
    let (verdict, out) = run(&scene, "check-planned", "good", Expect::Pass);
    assert_eq!(verdict.unwrap(), None, "{out}");
    assert!(
        out.contains("    | . has the plan") && out.contains("    | sub has the plan"),
        "{out}"
    );
}

#[test]
fn a_version_writer_runs_once_with_the_plan() {
    let scene = Scene::new();
    root(&scene);
    let (verdict, out) = run(&scene, "version-demo", "good", Expect::Pass);
    assert_eq!(verdict.unwrap(), None, "{out}");
    assert_eq!(out.matches("    | written once").count(), 1, "{out}");

    let (verdict, out) = run(&scene, "version-demo", "bad", finding());
    assert_eq!(verdict.unwrap(), None, "{out}");

    let (verdict, out) = run(&scene, "version-demo", "no-units", Expect::Pass);
    assert!(
        verdict
            .unwrap()
            .unwrap()
            .contains("no unit of the case selects the version writer `demo`"),
        "{out}"
    );
}

#[test]
fn a_build_runs_for_each_of_its_units_after_their_version_writers() {
    let scene = Scene::new();
    root(&scene);
    let (verdict, out) = run(&scene, "build-demo", "good", Expect::Pass);
    assert_eq!(verdict.unwrap(), None, "{out}");
    assert_eq!(out.matches("    | written once").count(), 1, "{out}");
    assert_eq!(out.matches("    | sub built").count(), 1, "{out}");
    assert!(
        out.find("written once") < out.find("sub built"),
        "the writer runs first: {out}"
    );

    let unbuildable = Expect::Fail(vec!["::error file=sub/unbuildable::wrong".to_owned()]);
    let (verdict, out) = run(&scene, "build-demo", "unbuildable", unbuildable);
    assert_eq!(verdict.unwrap(), None, "{out}");

    // A writer that fails keeps the build from running, and its finding
    // counts.
    let (verdict, out) = run(&scene, "build-demo", "bad", finding());
    assert_eq!(verdict.unwrap(), None, "{out}");
    assert!(!out.contains("has no version"), "{out}");

    let (verdict, out) = run(&scene, "build-demo", "no-units", Expect::Pass);
    assert!(
        verdict
            .unwrap()
            .unwrap()
            .contains("no unit of the case starts the build `demo` with a version of the run"),
        "{out}"
    );

    // A unit the commit does not touch builds nothing, as in a build job.
    let (verdict, out) = run(&scene, "build-demo", "untouched", Expect::Pass);
    assert!(
        verdict
            .unwrap()
            .unwrap()
            .contains("no unit of the case starts the build `demo` with a version of the run"),
        "{out}"
    );
    assert!(!out.contains("sub built"), "{out}");

    let (verdict, out) = run(&scene, "build-demo", "ghost-writer", Expect::Pass);
    assert!(
        verdict.unwrap_err().to_string().contains("version-ghost"),
        "{out}"
    );
    let (verdict, out) = run(&scene, "build-demo", "no-plan", Expect::Pass);
    assert!(
        verdict.unwrap_err().to_string().contains("no JSON"),
        "{out}"
    );
}

#[test]
fn the_plan_names_the_units_of_a_build_with_their_writers() {
    let plan = r#"{"units":[
        {"kind":"repository","name":null,"path":".","build":"demo"},
        {"kind":"component","name":"lib","path":"lib","build":"demo"},
        {"kind":"component","name":"doc","path":"doc","build":"demo"},
        {"kind":"component","name":"odd","path":"odd","build":"demo"},
        {"kind":"component","name":"other","path":"other","build":"other"},
        {"kind":"component","name":"none","path":"none","build":null},
        {"kind":"component","name":"lost","build":"demo"},
        {"kind":"component","name":"untouched","path":"untouched","build":"demo"},
        {"kind":"component","name":"unversioned","path":"unversioned","build":"demo"}
    ],"versions":[
        {"kind":"repository","name":null,"version":"1.0.0","writers":[]},
        {"kind":"component","name":"lib","version":"0.2.0-pr.7","writers":["cargo"]},
        {"kind":"component","name":"odd","version":"0.1.0","writers":[1, "cargo"]},
        {"kind":"component","name":"other","version":"0.1.0","writers":["cargo"]},
        {"kind":"component","name":"doc","version":"0.1.0"},
        {"kind":"component","name":"lost","version":"0.1.0","writers":[]},
        {"kind":"component","name":"untouched","version":null,"writers":["cargo"]}
    ]}"#;
    let built = |path: &str, writers: &[&str]| cases::Built {
        path: path.to_owned(),
        writers: writers.iter().map(|writer| (*writer).to_owned()).collect(),
    };
    // `untouched` has no version of the run and `unversioned` no entry under
    // `versions` at all: neither builds.
    assert_eq!(
        cases::builds(plan, "demo").unwrap(),
        [
            built(".", &[]),
            built("lib", &["cargo"]),
            built("doc", &[]),
            built("odd", &["cargo"]),
        ]
    );
    assert!(
        cases::builds(r#"{"units":[{"path":"x","build":"demo"}]}"#, "demo")
            .unwrap()
            .is_empty()
    );
    assert!(cases::builds(plan, "absent").unwrap().is_empty());
    assert!(
        cases::builds("not json", "demo")
            .unwrap_err()
            .to_string()
            .contains("no JSON")
    );
    assert!(
        cases::builds("{}", "demo")
            .unwrap_err()
            .to_string()
            .contains("names no units")
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
        ("publish-demo", "good", "no check, version writer or build"),
        ("version-demo", "no-plan", "no JSON"),
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
}

#[test]
fn the_finding_and_the_forbidden_lines_are_not_part_of_the_case() {
    let scene = Scene::new();
    root(&scene);
    assert!(
        scene.path().join("root/cases/bad").join(FINDING).is_file(),
        "the case holds it"
    );
    assert!(
        scene
            .path()
            .join("root/cases/bad")
            .join(FORBIDDEN)
            .is_file(),
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
