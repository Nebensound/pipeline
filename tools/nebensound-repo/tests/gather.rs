//! `nebensound-repo gather`: the one result of the checks and of the builds.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound_repo::gather::{self, Builds, Outcome};
use nebensound_repo::output::Output;
use nebensound_repo::workflow::Log;

const ONE: &str = r#"[{"path":"tools/a","build":"cargo-package","writers":["cargo"]}]"#;

/// Whether the closing job passes, and what it printed in a run.
fn said(gathered: impl FnOnce(&mut Log<'_>) -> bool) -> (bool, String) {
    let mut out = Vec::new();
    let mut log = Log::new(&mut out, true);
    let passed = gathered(&mut log);
    drop(log);
    (passed, support::printed(&String::from_utf8(out).unwrap()))
}

fn builds(declare: Outcome, check: Outcome, builds: Outcome, planned: &str) -> (bool, String) {
    let gathered = Builds {
        declare,
        check,
        builds,
        planned,
        pre_branch: false,
    };
    said(|log| gather::builds(&gathered, log))
}

#[test]
fn every_result_has_the_name_github_gives_it() {
    for (outcome, name) in [
        (Outcome::Success, "success"),
        (Outcome::Failure, "failure"),
        (Outcome::Cancelled, "cancelled"),
        (Outcome::Skipped, "skipped"),
    ] {
        assert_eq!(outcome.as_str(), name);
    }
}

#[test]
fn the_checks_pass_when_every_started_check_passed_or_none_started() {
    let (passed, text) = said(|log| gather::checks(Outcome::Success, Outcome::Success, log));
    assert!(passed, "{text}");
    assert_eq!(text, "Every check the plan started passed.\n");

    let (passed, text) = said(|log| gather::checks(Outcome::Success, Outcome::Skipped, log));
    assert!(passed, "{text}");
    assert_eq!(
        text,
        "The plan starts no check here; there was nothing to do.\n"
    );

    for outcome in [Outcome::Failure, Outcome::Cancelled] {
        let (passed, text) = said(|log| gather::checks(Outcome::Success, outcome, log));
        assert!(!passed, "{text}");
        assert_eq!(
            text,
            format!(
                "::error::At least one check did not pass ({}).\n",
                outcome.as_str()
            )
        );
    }

    let (passed, text) = said(|log| gather::checks(Outcome::Failure, Outcome::Skipped, log));
    assert!(!passed, "{text}");
    assert_eq!(
        text,
        "::error::Phase 0 did not pass (failure), so nothing was checked.\n"
    );
}

#[test]
fn without_phase_0_the_builds_fail() {
    let (passed, text) = builds(Outcome::Failure, Outcome::Skipped, Outcome::Skipped, "");
    assert!(!passed);
    assert!(text.contains("Phase 0 did not pass (failure)"), "{text}");
}

#[test]
fn when_nothing_builds_the_builds_pass_whatever_check_found() {
    for check in [Outcome::Success, Outcome::Failure, Outcome::Cancelled] {
        let (passed, text) = builds(Outcome::Success, check, Outcome::Skipped, "[]");
        assert!(passed, "{check:?}: {text}");
        assert_eq!(
            text,
            "Nothing builds in this run: no unit that builds has a version of this run.\n"
        );
    }
}

#[test]
fn after_a_push_to_a_pre_branch_the_builds_pass_and_say_why_nothing_built() {
    let gathered = Builds {
        declare: Outcome::Success,
        check: Outcome::Success,
        builds: Outcome::Skipped,
        planned: "[]",
        pre_branch: true,
    };
    let (passed, text) = said(|log| gather::builds(&gathered, log));
    assert!(passed, "{text}");
    assert_eq!(
        text,
        "Nothing builds in this run: a push to a pre-branch resolves no version.\n"
    );
}

#[test]
fn a_planned_build_after_a_failed_check_fails() {
    let (passed, text) = builds(Outcome::Success, Outcome::Failure, Outcome::Skipped, ONE);
    assert!(!passed);
    assert!(text.contains("Check did not pass (failure)"), "{text}");
}

#[test]
fn the_builds_pass_when_every_planned_build_passed() {
    let (passed, text) = builds(Outcome::Success, Outcome::Success, Outcome::Success, ONE);
    assert!(passed, "{text}");
    assert!(
        text.contains("Every build the plan started passed"),
        "{text}"
    );
}

#[test]
fn planned_builds_that_did_not_run_or_failed_fail() {
    for (result, finding) in [
        (
            Outcome::Skipped,
            "The plan names builds, and none of them ran",
        ),
        (
            Outcome::Failure,
            "At least one build did not pass (failure)",
        ),
        (
            Outcome::Cancelled,
            "At least one build did not pass (cancelled)",
        ),
    ] {
        let (passed, text) = builds(Outcome::Success, Outcome::Success, result, ONE);
        assert!(!passed, "{result:?}: {text}");
        assert!(text.contains(finding), "{result:?}: {text}");
    }
}

fn call(args: &[&str], variables: &[(&str, &str)]) -> (String, u8) {
    support::call_with(args, variables, Output::Workflow)
}

#[test]
fn the_closing_jobs_call_the_tool_with_the_results_they_are_handed() {
    let (text, code) = call(
        &[
            "nebensound-repo",
            "gather",
            "checks",
            "--declare",
            "success",
            "--checks",
            "failure",
        ],
        &[],
    );
    assert_eq!(code, 1, "{text}");
    assert_eq!(
        text,
        "::error::At least one check did not pass (failure).\n"
    );

    let (text, code) = call(
        &[
            "nebensound-repo",
            "gather",
            "checks",
            "--declare",
            "success",
            "--checks",
            "skipped",
        ],
        &[],
    );
    assert_eq!(code, 0, "{text}");

    let builds = [
        "nebensound-repo",
        "gather",
        "builds",
        "--declare",
        "success",
        "--check",
        "failure",
        "--builds",
        "skipped",
        "--planned",
        "[]",
    ];
    let (text, code) = call(&builds, &[("NEBENSOUND_PRE_BRANCH", "true")]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("a push to a pre-branch"), "{text}");
    let (text, code) = call(&builds, &[]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("no unit that builds"), "{text}");

    // A result GitHub does not give is a misuse.
    let (text, code) = call(
        &[
            "nebensound-repo",
            "gather",
            "checks",
            "--declare",
            "",
            "--checks",
            "success",
        ],
        &[],
    );
    assert_eq!(code, 2, "{text}");
}

#[test]
fn a_call_reads_the_variables_of_the_process() {
    // Whether this process runs after a push to a pre-branch decides only
    // which sentence says that nothing builds.
    let (text, code) = support::call(&[
        "nebensound-repo",
        "gather",
        "builds",
        "--declare",
        "success",
        "--check",
        "success",
        "--builds",
        "skipped",
        "--planned",
        "[]",
    ]);
    assert_eq!(code, 0, "{text}");
    assert!(text.starts_with("Nothing builds in this run: "), "{text}");
}
