//! `nebensound-repo unwritten`: what has no building block yet fails by name.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;

use nebensound_repo::output::Output;
use nebensound_repo::unwritten::{self, Named};
use nebensound_repo::workflow::Log;

/// Whether it passes, and what it printed in a run.
fn said(held: impl FnOnce(&mut Log<'_>) -> bool) -> (bool, String) {
    let mut out = Vec::new();
    let mut log = Log::new(&mut out, true);
    let passed = held(&mut log);
    drop(log);
    (passed, support::printed(&String::from_utf8(out).unwrap()))
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

const PLAN: &str = r#"{"versions":[
  {"kind":"repository","name":null,"writers":["cargo","npm"],"targets":["crates.io"]},
  {"kind":"component","name":"lib","writers":["npm"],"targets":[]}
]}"#;

#[test]
fn a_list_of_names_reads_as_json_or_not_at_all() {
    assert_eq!(
        unwritten::names(r#"["a", "b"]"#, "the list").unwrap(),
        names(&["a", "b"])
    );
    let refusal = unwritten::names("a, b", "the list").unwrap_err();
    assert!(
        refusal.starts_with("the list is no JSON list of names: "),
        "{refusal}"
    );
}

#[test]
fn every_check_without_a_block_fails_by_name() {
    let (passed, text) = said(|log| {
        unwritten::checks(
            "tools/a",
            &names(&["json", "new", "other"]),
            &names(&["json"]),
            log,
        )
    });
    assert!(!passed);
    assert_eq!(
        text,
        "::error::The check 'new' has no building block yet, so 'tools/a' is not checked for \
         it. It is to be written under .github/actions/check-new.\n\
         ::error::The check 'other' has no building block yet, so 'tools/a' is not checked for \
         it. It is to be written under .github/actions/check-other.\n"
    );

    let (passed, text) =
        said(|log| unwritten::checks(".", &names(&["json"]), &names(&["json"]), log));
    assert!(passed);
    assert_eq!(text, "Every check of '.' has a building block.\n");
}

#[test]
fn a_build_without_a_block_fails_by_name() {
    let (passed, text) = said(|log| unwritten::build(".", "docker", &names(&["docker"]), log));
    assert!(passed);
    assert_eq!(text, "The build 'docker' of '.' has a building block.\n");

    let (passed, text) = said(|log| unwritten::build("a", "new", &names(&["docker"]), log));
    assert!(!passed);
    assert_eq!(
        text,
        "::error::The build 'new' has no building block yet, so 'a' is not built. It is to be \
         written under .github/actions/build-new.\n"
    );
}

#[test]
fn every_version_writer_and_publish_target_without_a_block_fails_by_name() {
    let (passed, text) =
        said(|log| unwritten::named(PLAN, Named::Writers, &names(&["cargo"]), log).unwrap());
    assert!(!passed);
    assert_eq!(
        text,
        "::error::The version writer 'npm' has no building block yet, so the version of the \
         repository is not written. It is to be written under .github/actions/version-npm.\n\
         ::error::The version writer 'npm' has no building block yet, so the version of \
         component lib is not written. It is to be written under .github/actions/version-npm.\n"
    );

    let (passed, text) = said(|log| unwritten::named(PLAN, Named::Targets, &[], log).unwrap());
    assert!(!passed);
    assert_eq!(
        text,
        "::error::The publish target 'crates.io' has no building block yet, so the repository \
         is not published there. It is to be written under .github/actions/publish-crates-io.\n"
    );

    let (passed, text) =
        said(|log| unwritten::named(PLAN, Named::Writers, &names(&["cargo", "npm"]), log).unwrap());
    assert!(passed);
    assert_eq!(
        text,
        "Every version writer the plan names has a building block.\n"
    );
    let (passed, text) =
        said(|log| unwritten::named(PLAN, Named::Targets, &names(&["crates.io"]), log).unwrap());
    assert!(passed);
    assert_eq!(
        text,
        "Every publish target the plan names has a building block.\n"
    );
}

#[test]
fn without_a_plan_there_is_nothing_to_look_at() {
    for (kind, noun) in [
        (Named::Writers, "version writer"),
        (Named::Targets, "publish target"),
    ] {
        let (passed, text) = said(|log| unwritten::named("\n", kind, &[], log).unwrap());
        assert!(passed);
        assert_eq!(
            text,
            format!("There is no plan, so there is no {noun} to look at.\n")
        );
    }
    let mut out = Vec::new();
    let mut log = Log::new(&mut out, true);
    let refusal = unwritten::named("{", Named::Writers, &[], &mut log).unwrap_err();
    assert!(refusal.starts_with("the plan does not read: "), "{refusal}");
}

#[test]
fn the_steps_call_the_tool_with_what_they_hold() {
    let call = |args: &[&str]| {
        let mut all = vec!["nebensound-repo", "unwritten"];
        all.extend(args);
        support::call_with(&all, &[], Output::Workflow)
    };
    // The steps hand the unit in as `--unit=<path>`, so a path that starts
    // with `-` is read as the unit, not as a flag.
    let (text, code) = call(&[
        "checks",
        "--unit=-lib",
        "--planned",
        r#"["json","new"]"#,
        "--written",
        r#"["json"]"#,
    ]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("The check 'new' has no building block yet, so '-lib' is not checked"),
        "{text}"
    );
    let (text, code) = call(&[
        "build",
        "--unit=-lib",
        "--planned",
        "docker",
        "--written",
        r#"["docker"]"#,
    ]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("of '-lib' has a building block"), "{text}");

    let dir = tempfile::tempdir().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(&plan, PLAN).unwrap();
    let plan = plan.to_str().unwrap();
    let (text, code) = call(&["writers", "--plan", plan, "--written", r#"["cargo","npm"]"#]);
    assert_eq!(code, 0, "{text}");
    let (text, code) = call(&["targets", "--plan", plan, "--written", "[]"]);
    assert_eq!(code, 1, "{text}");

    // What does not read is a misuse, named.
    for (args, refusal) in [
        (
            vec!["checks", "--unit", ".", "--planned", "x", "--written", "[]"],
            "::error::the checks of the unit is no JSON list of names: ",
        ),
        (
            vec!["checks", "--unit", ".", "--planned", "[]", "--written", "x"],
            "::error::the list of written checks is no JSON list of names: ",
        ),
        (
            vec!["build", "--unit", ".", "--planned", "a", "--written", "x"],
            "::error::the list of written builds is no JSON list of names: ",
        ),
        (
            vec!["writers", "--plan", plan, "--written", "x"],
            "::error::the list of written version writers is no JSON list of names: ",
        ),
        (
            vec!["targets", "--plan", plan, "--written", "x"],
            "::error::the list of written publish targets is no JSON list of names: ",
        ),
        (
            vec!["writers", "--plan", "/nonexistent/plan", "--written", "[]"],
            "::error::the plan /nonexistent/plan does not read: ",
        ),
    ] {
        let (text, code) = call(&args);
        assert_eq!(code, 2, "{args:?}: {text}");
        assert!(text.starts_with(refusal), "{args:?}: {text}");
    }
    fs::write(dir.path().join("broken.json"), "{").unwrap();
    let broken = dir.path().join("broken.json");
    for kind in ["writers", "targets"] {
        let (text, code) = call(&[kind, "--plan", broken.to_str().unwrap(), "--written", "[]"]);
        assert_eq!(code, 2, "{text}");
        assert!(
            text.starts_with("::error::the plan does not read: "),
            "{text}"
        );
        let (text, code) = call(&[kind, "--plan", "/nonexistent/plan", "--written", "[]"]);
        assert_eq!(code, 2, "{text}");
        assert!(
            text.starts_with("::error::the plan /nonexistent/plan does not read: "),
            "{text}"
        );
    }
}
