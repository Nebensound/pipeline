//! The register, the building blocks and `nebensound.yml` say the same thing.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::path::Path;

use support::{Scene, write};
use xtask::Error;
use xtask::cases::{Case, Expect};
use xtask::layout::{self, Directory};

#[test]
fn this_repository_agrees() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let found = layout::check(&root).unwrap();
    assert_eq!(found.problems, Vec::<String>::new());
    assert!(
        found.blocks.contains(&"check-json".to_owned()),
        "{:?}",
        found.blocks
    );
    assert!(
        !found.blocks.contains(&"phase0".to_owned()),
        "{:?}",
        found.blocks
    );
}

const REGISTER: &str = r#"
[check.demo]
extensions = [".demo"]
[check.other]
extensions = [".other"]
[check.unwritten]
extensions = [".none"]
[version.cargo]
copy = "x"
[build.cargo-package]
produces = "crate"
[target."crates.io"]
accepts = ["crate"]
"#;

/// A case of each kind.
fn both() -> Vec<Case> {
    vec![case(Expect::Pass), case(Expect::Fail(vec!["x".to_owned()]))]
}

fn case(expect: Expect) -> Case {
    Case {
        name: "a".to_owned(),
        dir: "a".into(),
        expect,
    }
}

fn dir(name: &str, action: bool, cases: Result<Vec<Case>, Error>) -> Directory {
    Directory {
        name: name.to_owned(),
        action,
        cases,
    }
}

fn workflow(written: &str, steps: &str) -> String {
    format!(
        "jobs:\n  declare:\n    steps:\n      - uses: $/.github/actions/tool\n      - uses: actions/checkout@v7\n      - run: echo\n  units:\n    steps:\n      - name: Not written yet\n        env:\n          WRITTEN: '{written}'\n{}",
        support::indent(steps, 6)
    )
}

const AGREEING_STEPS: &str = "- name: demo\n  if: ${{ !cancelled() && contains(matrix.checks, 'demo') }}\n  uses: $/.github/actions/check-demo\n- uses: $/.github/actions/phase0\n";

#[test]
fn agreeing_parts_find_nothing() {
    let found = layout::compare(
        REGISTER,
        &workflow(r#"["demo"]"#, AGREEING_STEPS),
        &[
            dir("check-demo", true, Ok(both())),
            dir("phase0", true, Ok(Vec::new())),
            dir("tool", true, Ok(Vec::new())),
        ],
    )
    .unwrap();
    assert_eq!(found.problems, Vec::<String>::new());
    assert_eq!(found.blocks, ["check-demo"]);
}

#[test]
fn every_disagreement_is_named() {
    let steps = "- name: wrong\n  if: ${{ contains(matrix.checks, 'demo') }}\n  uses: $/.github/actions/check-demo\n- name: other\n  if: always()\n  uses: $/.github/actions/check-other\n- uses: $/.github/actions/check-gone\n";
    let found = layout::compare(
        REGISTER,
        &workflow(r#"["demo", "ghost"]"#, steps),
        &[
            dir("build-cargo-package", true, Ok(both())),
            dir("check-cases", true, Ok(both())),
            dir("check-demo", true, Ok(vec![case(Expect::Pass)])),
            dir(
                "check-other",
                false,
                Ok(vec![case(Expect::Fail(vec!["x".to_owned()]))]),
            ),
            dir("check-preset", true, Ok(both())),
            dir(
                "check-unwritten",
                true,
                Err(Error::new("a broken case".to_owned())),
            ),
            dir("phase0", true, Ok(Vec::new())),
            dir("tool", true, Ok(Vec::new())),
        ],
    )
    .unwrap();
    assert_eq!(
        found.problems,
        [
            "`build-cargo-package` is a block for the build `cargo-package`, and the xtask knows only how a check is called and tested; it has to learn the other kind first",
            "`check-cases` belongs to no register entry, and it is no machinery of the workflow (phase0, tool)",
            "nebensound.yml calls `check-demo` from a step that is not named `demo` and switched on it with `contains(matrix.checks, 'demo')`",
            "`check-demo` has no failing test case under tests/fail/",
            "`check-other` has no action.yml",
            "nebensound.yml calls `check-other` from a step that is not named `other` and switched on it with `contains(matrix.checks, 'other')`",
            "`check-other` exists, but `Not written yet` still fails for `other`",
            "`check-other` has no passing test case under tests/pass/",
            "`check-preset` belongs to no register entry, and it is no machinery of the workflow (phase0, tool)",
            "`check-unwritten` is never called from nebensound.yml",
            "`check-unwritten` exists, but `Not written yet` still fails for `unwritten`",
            "a broken case",
            "`phase0` is never called from nebensound.yml",
            "nebensound.yml calls `check-gone`, which does not exist",
            "`Not written yet` counts `ghost` as written, but it has no block",
        ]
    );
    assert_eq!(
        found.blocks,
        [
            "build-cargo-package",
            "check-demo",
            "check-other",
            "check-unwritten"
        ]
    );
}

#[test]
fn entries_that_share_a_block_name_are_named() {
    let found = layout::compare(
        "[target.\"a.b\"]\naccepts = []\n[target.a-b]\naccepts = []\n",
        &workflow("[]", ""),
        &[dir("tool", true, Ok(Vec::new()))],
    )
    .unwrap();
    assert_eq!(
        found.problems,
        ["the target `a-b` and the target `a.b` would both be the block `publish-a-b`"]
    );
}

#[test]
fn the_list_of_written_checks_has_to_be_there_once_and_readable() {
    let twice = "- name: Not written yet\n  env:\n    WRITTEN: '[]'\n";
    for (workflow, message) in [
        (
            "jobs:\n  a:\n    steps: []\n".to_owned(),
            "has 0 steps named `Not written yet`",
        ),
        (workflow("[]", twice), "has 2 steps named `Not written yet`"),
        (workflow("not json", ""), "is no JSON list of names"),
    ] {
        let found = layout::compare(REGISTER, &workflow, &[]).unwrap();
        assert!(
            found
                .problems
                .iter()
                .any(|problem| problem.contains(message)),
            "{workflow}\n{:?}",
            found.problems
        );
    }
}

#[test]
fn parts_that_cannot_be_read_are_errors() {
    for (register, workflow, message) in [
        ("not = = toml", "jobs: {}\n", "the register is no TOML"),
        (REGISTER, "jobs: [unclosed", "nebensound.yml is no YAML"),
        (REGISTER, "name: no jobs\n", "nebensound.yml has no jobs"),
        (REGISTER, "", "nebensound.yml has no jobs"),
    ] {
        let error = layout::compare(register, workflow, &[]).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }

    let scene = Scene::new();
    let root = scene.path();
    let missing = |root: &Path| layout::check(root).unwrap_err().to_string();
    assert!(missing(root).contains("register.toml"));
    write(root, &[(layout::REGISTER, REGISTER)]);
    assert!(missing(root).contains("nebensound.yml"));
    write(root, &[(layout::WORKFLOW, &workflow("[]", ""))]);
    assert!(missing(root).contains("cannot list"));
}

#[test]
fn every_directory_is_listed_with_its_cases() {
    let scene = Scene::new();
    let actions = scene.path().join("actions");
    write(
        &actions,
        &[
            ("check-b/action.yml", ""),
            ("check-b/tests/pass/a/file", ""),
            ("check-a/README.md", ""),
            ("a file", ""),
        ],
    );
    let found = layout::directories(&actions).unwrap();
    let names: Vec<(&str, bool, usize)> = found
        .iter()
        .map(|dir| {
            (
                dir.name.as_str(),
                dir.action,
                dir.cases.as_ref().unwrap().len(),
            )
        })
        .collect();
    assert_eq!(names, [("check-a", false, 0), ("check-b", true, 1)]);
}
