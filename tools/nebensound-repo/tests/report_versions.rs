//! `nebensound-repo report-versions`: the versions as the job Version reports
//! them, and after a push to a pre-branch why there are none.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;

use nebensound_repo::output::Output;
use support::{Git, call_with};

fn repository() -> Git {
    let git = Git::new();
    git.write(&[
        (
            ".nebensound/repo.toml",
            "[repo]\naudience = \"public\"\npipeline = \"none\"\n",
        ),
        ("README.md", "# x\n"),
    ]);
    git.commit("feat: start");
    git
}

fn report(git: &Git, variables: &[(&str, &str)]) -> (String, u8) {
    let (text, code) = call_with(
        &["nebensound-repo", "-C", git.root(), "report-versions"],
        variables,
        Output::Workflow,
    );
    (support::printed(&text), code)
}

#[test]
fn the_table_goes_into_the_log_and_as_markdown_into_the_summary() {
    let git = repository();
    let dir = tempfile::tempdir().unwrap();
    let summary = dir.path().join("summary");
    let file = summary.to_str().unwrap();
    let (text, code) = report(&git, &[("GITHUB_STEP_SUMMARY", file)]);
    assert_eq!(code, 0, "{text}");
    let (table, _) = call_with(
        &["nebensound-repo", "-C", git.root(), "version"],
        &[],
        Output::Workflow,
    );
    assert_eq!(text, support::printed(&table));
    let (markdown, _) = call_with(
        &["nebensound-repo", "-C", git.root(), "version", "--markdown"],
        &[],
        Output::Workflow,
    );
    assert_eq!(
        fs::read_to_string(&summary).unwrap(),
        support::printed(&markdown)
    );

    // For a pull request, the versions its squash merge would get.
    let (text, code) = report(
        &git,
        &[
            ("NEBENSOUND_PULL_REQUEST", "4"),
            ("NEBENSOUND_TITLE", "fix: it"),
            ("NEBENSOUND_BODY", "Why."),
        ],
    );
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("-pr.4"), "{text}");

    // Without a summary file, as outside a run, only the log.
    let (text, code) = report(&git, &[]);
    assert_eq!(code, 0, "{text}");

    // A summary that cannot be written fails the step.
    let directory = dir.path().to_str().unwrap();
    let (text, code) = report(&git, &[("GITHUB_STEP_SUMMARY", directory)]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("the summary of the run cannot be written to "),
        "{text}"
    );
}

#[test]
fn with_phase_0_errors_both_say_which_and_the_step_fails() {
    let git = repository();
    // PHP, which no unit here checks: an error.
    git.write(&[("tool.php", "<?php\n")]);
    git.commit("feat: php");
    let dir = tempfile::tempdir().unwrap();
    let summary = dir.path().join("summary");
    let (text, code) = report(&git, &[("GITHUB_STEP_SUMMARY", summary.to_str().unwrap())]);
    assert_eq!(code, 1, "{text}");
    assert!(!fs::read_to_string(&summary).unwrap().is_empty());
}

#[test]
fn after_a_push_to_a_pre_branch_nothing_is_resolved_and_it_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let summary = dir.path().join("summary");
    // The repository is not read at all: it need not even be one.
    let (text, code) = call_with(
        &["nebensound-repo", "-C", "/nonexistent", "report-versions"],
        &[
            ("NEBENSOUND_PRE_BRANCH", "true"),
            ("GITHUB_REF_NAME", "pre-x%1"),
            ("GITHUB_STEP_SUMMARY", summary.to_str().unwrap()),
        ],
        Output::Workflow,
    );
    assert_eq!(code, 0, "{text}");
    assert!(
        text.starts_with(
            "::notice title=Pre-branch::This run follows a push to the pre-branch `pre-x%251`."
        ),
        "{text}"
    );
    let written = fs::read_to_string(&summary).unwrap();
    for part in [
        "`pre-x%1`",
        "not built yet",
        "no version is resolved",
        "nothing is built",
        "nothing is published",
    ] {
        assert!(written.contains(part), "{written}");
    }

    // A summary that cannot be written fails the step.
    let (text, code) = call_with(
        &["nebensound-repo", "report-versions"],
        &[
            ("NEBENSOUND_PRE_BRANCH", "true"),
            ("GITHUB_STEP_SUMMARY", dir.path().to_str().unwrap()),
        ],
        Output::Workflow,
    );
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::the summary of the run cannot be written to "),
        "{text}"
    );
}

#[test]
fn a_number_that_is_no_pull_request_is_a_misuse() {
    let git = repository();
    let (text, code) = report(&git, &[("NEBENSOUND_PULL_REQUEST", "0")]);
    assert_eq!(code, 2, "{text}");
    assert!(
        text.starts_with("NEBENSOUND_PULL_REQUEST is \"0\""),
        "{text}"
    );
}
