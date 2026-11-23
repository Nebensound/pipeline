//! What a step of a workflow run hands the tool, and what the tool hands back.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use nebensound_repo::workflow::{self, Log};

/// The variables of a run, from pairs.
fn run(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

/// What a log printed, of a workflow run or not.
fn logged(workflow: bool, write: impl FnOnce(&mut Log<'_>)) -> String {
    let mut out = Vec::new();
    let mut log = Log::new(&mut out, workflow);
    write(&mut log);
    String::from_utf8(out).unwrap()
}

#[test]
fn a_variable_counts_only_when_it_is_set_and_not_empty() {
    let variables = run(&[("A", "x"), ("B", "")]);
    let read = |name: &str| variables.get(name).cloned();
    assert_eq!(workflow::given(&read, "A").as_deref(), Some("x"));
    assert_eq!(workflow::given(&read, "B"), None);
    assert_eq!(workflow::given(&read, "C"), None);
}

#[test]
fn only_true_is_a_push_to_a_pre_branch() {
    for (value, expected) in [("true", true), ("false", false), ("", false)] {
        let variables = run(&[("NEBENSOUND_PRE_BRANCH", value)]);
        let read = |name: &str| variables.get(name).cloned();
        assert_eq!(workflow::pre_branch(&read), expected, "{value}");
    }
    assert!(!workflow::pre_branch(&|_| None));
}

#[test]
fn the_pull_request_is_its_number_and_the_message_of_its_squash_merge() {
    let variables = run(&[
        ("NEBENSOUND_PULL_REQUEST", "17"),
        ("NEBENSOUND_TITLE", "fix: the title"),
        ("NEBENSOUND_BODY", "The description."),
    ]);
    let read = |name: &str| variables.get(name).cloned();
    let pull_request = workflow::pull_request(&read).unwrap().unwrap();
    assert_eq!(pull_request.number, 17);
    assert_eq!(pull_request.message, "fix: the title\n\nThe description.");

    // Without a description the message is the title alone.
    let variables = run(&[
        ("NEBENSOUND_PULL_REQUEST", "3"),
        ("NEBENSOUND_TITLE", "-fix: starts like an option"),
        ("NEBENSOUND_BODY", ""),
    ]);
    let read = |name: &str| variables.get(name).cloned();
    let pull_request = workflow::pull_request(&read).unwrap().unwrap();
    assert_eq!(pull_request.message, "-fix: starts like an option");

    // A run of no pull request names none.
    let variables = run(&[("NEBENSOUND_PULL_REQUEST", "")]);
    let read = |name: &str| variables.get(name).cloned();
    assert_eq!(workflow::pull_request(&read).unwrap(), None);
    assert_eq!(workflow::pull_request(&|_| None).unwrap(), None);

    for number in ["0", "x", "-1"] {
        let variables = run(&[("NEBENSOUND_PULL_REQUEST", number)]);
        let read = |name: &str| variables.get(name).cloned();
        let refusal = workflow::pull_request(&read).unwrap_err();
        assert!(
            refusal.contains("a whole number from 1") && refusal.contains(number),
            "{refusal}"
        );
    }
}

#[test]
fn outputs_go_to_the_file_of_the_step_and_nowhere_outside_a_run() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("output");
    fs::write(&file, "before=1\n").unwrap();
    let variables = run(&[("GITHUB_OUTPUT", file.to_str().unwrap())]);
    let read = |name: &str| variables.get(name).cloned();

    workflow::output(&read, &[("a", "1"), ("b", "two words")]).unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "before=1\na=1\nb=two words\n"
    );

    // A line break would end the value early, so nothing is written.
    let refusal = workflow::output(&read, &[("c", "x"), ("d", "two\nlines")]).unwrap_err();
    assert!(refusal.contains("the output d"), "{refusal}");
    let refusal = workflow::output(&read, &[("e", "a\rb")]).unwrap_err();
    assert!(refusal.contains("the output e"), "{refusal}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "before=1\na=1\nb=two words\n"
    );

    // Outside a run there is no file, and nothing to refuse.
    workflow::output(&|_| None, &[("a", "1")]).unwrap();

    // A file that cannot be written fails.
    let variables = run(&[("GITHUB_OUTPUT", dir.path().to_str().unwrap())]);
    let read = |name: &str| variables.get(name).cloned();
    let refusal = workflow::output(&read, &[("a", "1")]).unwrap_err();
    assert!(
        refusal.starts_with("the outputs of the step cannot be written to "),
        "{refusal}"
    );
}

#[test]
fn the_summary_is_appended_to_and_absent_outside_a_run() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("summary");
    let variables = run(&[("GITHUB_STEP_SUMMARY", file.to_str().unwrap())]);
    let read = |name: &str| variables.get(name).cloned();
    workflow::summary(&read, "one\n").unwrap();
    workflow::summary(&read, "two\n").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "one\ntwo\n");

    workflow::summary(&|_| None, "nowhere\n").unwrap();

    let variables = run(&[("GITHUB_STEP_SUMMARY", dir.path().to_str().unwrap())]);
    let read = |name: &str| variables.get(name).cloned();
    let refusal = workflow::summary(&read, "x").unwrap_err();
    assert!(
        refusal.starts_with("the summary of the run cannot be written to "),
        "{refusal}"
    );
}

#[test]
fn a_notice_is_escaped_the_way_the_runner_reads_it() {
    assert_eq!(
        workflow::notice("A: b, c", "50% of it\nand more"),
        "::notice title=A%3A b%2C c::50%25 of it%0Aand more"
    );
}

#[test]
fn a_log_writes_workflow_commands_in_a_run_and_plain_lines_elsewhere() {
    let write = |log: &mut Log<'_>| {
        log.line("::group::A group");
        log.lines("one\ntwo");
        log.error("broken");
        log.error_on("a/file", "broken there");
        log.line("::endgroup::");
    };
    assert_eq!(
        logged(true, write),
        "::group::A group\none\ntwo\n::error::broken\n::error file=a/file::broken there\n\
         ::endgroup::\n"
    );
    assert_eq!(
        logged(false, write),
        "A group\none\ntwo\nerror: broken\na/file: error: broken there\n"
    );
}

#[test]
fn a_step_is_a_group_and_says_when_it_failed() {
    let text = logged(true, |log| {
        assert_eq!(log.step("Passes", |_| Some(1)), Some(1));
        assert_eq!(log.step("Fails", |_| None::<()>), None);
    });
    assert_eq!(
        text,
        "::group::Passes\n::endgroup::\n::group::Fails\n::endgroup::\nFails: failed\n"
    );
}

#[test]
fn a_program_streams_into_the_log_or_is_captured() {
    let text = logged(true, |log| {
        let mut command = Command::new("sh");
        command.args(["-c", "echo out; echo err >&2; printf 'cr\\r\\n'"]);
        assert!(log.stream(command, "sh"));
        let mut command = Command::new("sh");
        command.args(["-c", "exit 3"]);
        assert!(!log.stream(command, "sh"));
        assert!(!log.stream(Command::new("/nonexistent/program"), "nothing"));
    });
    assert!(text.contains("> sh -c echo out;"), "{text}");
    assert!(text.contains("\nout\n"), "{text}");
    assert!(text.contains("\nerr\n"), "{text}");
    assert!(text.contains("\ncr\n"), "{text}");
    assert!(
        text.contains("::error::nothing cannot be started: "),
        "{text}"
    );

    let text = logged(true, |log| {
        let mut command = Command::new("sh");
        command.args(["-c", "echo kept; echo said >&2"]);
        assert_eq!(log.capture(command, "sh").unwrap(), b"kept\n");
        let mut command = Command::new("sh");
        command.args(["-c", "echo lost; exit 2"]);
        assert_eq!(log.capture(command, "a failing sh"), None);
        assert_eq!(
            log.capture(Command::new("/nonexistent/program"), "nothing"),
            None
        );
    });
    assert!(text.contains("said\n"), "{text}");
    assert!(!text.contains("kept"), "{text}");
    assert!(text.contains("a failing sh failed ("), "{text}");
    assert!(
        text.contains("::error::nothing cannot be started: "),
        "{text}"
    );
}
