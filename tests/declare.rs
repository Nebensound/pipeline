//! `nebensound-repo declare`: the preparing job Declare, one call: phase 0,
//! the versions, and what it hands the jobs after it.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;
use std::path::PathBuf;

use nebensound_repo::audience::Visibility;
use nebensound_repo::declare::{self, Run};
use nebensound_repo::output::Output;
use nebensound_repo::plan::Plan;
use nebensound_repo::workflow::Log;
use nebensound_repo::{Audience, Register};
use serde_json::{Value, json};
use support::{Git, call_with, program::executable};
use tempfile::TempDir;

/// A repository whose component `app` builds cargo-package with the version
/// writer cargo, and whose package `vendor/x` an `[[exclude]]` takes out.
fn declared(audience: &str) -> String {
    format!(
        "[repo]\naudience = \"{audience}\"\npipeline = \"none\"\n\n[component.app]\npath = \
         \"app\"\npipeline = {{ version = [\"cargo\"], build = \"cargo-package\" }}\n\n\
         [[exclude]]\npath = \"vendor/x\"\nreason = \"code of others, 100% as it came\"\n"
    )
}

fn repository() -> Git {
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", &declared("public")),
        ("README.md", "# x\n"),
        ("app/Cargo.toml", "[package]\nname = \"app\"\n"),
        ("app/README.md", "# app\n"),
        ("vendor/x/Cargo.toml", "[package]\nname = \"x\"\n"),
        ("vendor/x/src/lib.rs", "//! x\n"),
    ]);
    git.commit("feat: start");
    git
}

/// Where a run writes its outputs and its summary.
struct Files {
    dir: TempDir,
}

impl Files {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn output(&self) -> PathBuf {
        self.dir.path().join("output")
    }

    fn summary(&self) -> PathBuf {
        self.dir.path().join("summary")
    }

    fn read(path: &PathBuf) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }

    /// The outputs the step wrote, by name.
    fn outputs(&self) -> Vec<(String, String)> {
        Self::read(&self.output())
            .lines()
            .map(|line| {
                let (name, value) = line.split_once('=').unwrap();
                (name.to_owned(), value.to_owned())
            })
            .collect()
    }
}

/// `declare` on `git` with these variables beside the files of the run: what
/// it printed, without the lines that stop and resume the processing of
/// workflow commands.
fn declare(git: &Git, files: &Files, variables: &[(&str, &str)]) -> (String, u8) {
    let (log, code) = declare_log(git, files, variables);
    (support::printed(&log), code)
}

/// `declare` as [`declare`] calls it: its log as the runner gets it, with the
/// stretches in which the processing of workflow commands is stopped.
fn declare_log(git: &Git, files: &Files, variables: &[(&str, &str)]) -> (String, u8) {
    let output = files.output();
    let summary = files.summary();
    let mut all = vec![
        ("GITHUB_OUTPUT", output.to_str().unwrap()),
        ("GITHUB_STEP_SUMMARY", summary.to_str().unwrap()),
        ("NEBENSOUND_NAME", "case"),
        ("NEBENSOUND_DEFAULT_BRANCH", "main"),
    ];
    all.extend_from_slice(variables);
    let (text, code) = call_with(
        &["nebensound-repo", "-C", git.root(), "declare"],
        &all,
        Output::Workflow,
    );
    (text, code)
}

/// What `version` prints on `git` with these arguments behind it, as the
/// preparing job reports it.
fn version(git: &Git, args: &[&str]) -> String {
    let mut all = vec!["nebensound-repo", "-C", git.root(), "version"];
    all.extend(args);
    let (text, code) = call_with(&all, &[], Output::Workflow);
    let text = support::printed(&text);
    assert_eq!(code, 0, "{text}");
    text
}

#[test]
fn phase_0_passes_and_hands_on_the_plan_and_the_matrices() {
    let git = repository();
    let files = Files::new();
    let (text, code) = declare(&git, &files, &[("NEBENSOUND_VISIBILITY", "public")]);
    assert_eq!(code, 0, "{text}");
    let outputs = files.outputs();
    let names: Vec<&str> = outputs.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["plan", "units", "builds"]);
    let plan: Value = serde_json::from_str(&outputs[0].1).unwrap();
    assert_eq!(plan["audience"], "public");
    let units: Value = serde_json::from_str(&outputs[1].1).unwrap();
    assert!(
        units
            .as_array()
            .unwrap()
            .iter()
            .all(|unit| !unit["checks"].as_array().unwrap().is_empty()),
        "{units}"
    );
    assert_eq!(units[0]["kind"], "repository");
    let builds: Value = serde_json::from_str(&outputs[2].1).unwrap();
    assert_eq!(
        builds,
        json!([{"build": "cargo-package", "checks": [], "kind": "component", "name": "app",
                "path": "app", "writers": ["cargo"], "job": "Build component app",
                "runs-on": "ubuntu-latest"}])
    );
    for line in [
        "::group::The plan",
        &outputs[0].1,
        "::group::The excluded packages",
        r#"[{"path":"vendor/x","reason":"code of others, 100% as it came"}]"#,
        "::notice title=Excluded package::The Rust package at vendor/x is excluded from every \
         step of check-rust: code of others, 100%25 as it came",
        "::group::The units that run a check",
        "::group::The units that build",
    ] {
        assert!(text.lines().any(|found| found == line), "{line}\n{text}");
    }
    // The report of phase 0 comes first, as `check` prints it, then the
    // table of the versions, then what is handed on.
    let table = version(&git, &[]);
    let at = text.find(&table).unwrap();
    assert!(text.find("Tree").unwrap() < at, "{text}");
    assert!(at < text.find("::group::The plan").unwrap(), "{text}");
    // The same table as Markdown into the summary. No pull request, no change
    // of the audience to name.
    assert!(!text.contains("Audience changed"), "{text}");
    assert_eq!(
        Files::read(&files.summary()),
        version(&git, &["--markdown"])
    );

    // Without a summary file, as outside a run, only the log.
    let (text, code) = call_with(
        &["nebensound-repo", "-C", git.root(), "declare"],
        &[("NEBENSOUND_VISIBILITY", "public")],
        Output::Workflow,
    );
    let text = support::printed(&text);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains(&table), "{text}");
}

#[test]
fn after_a_push_to_a_pre_branch_no_version_is_resolved_and_no_unit_builds() {
    let git = repository();
    let files = Files::new();
    let pre_branch = [
        ("NEBENSOUND_VISIBILITY", "public"),
        ("NEBENSOUND_PRE_BRANCH", "true"),
        ("GITHUB_REF_NAME", "pre-x%1"),
    ];
    let (text, code) = declare(&git, &files, &pre_branch);
    assert_eq!(code, 0, "{text}");
    assert_eq!(files.outputs()[2], ("builds".to_owned(), "[]".to_owned()));
    assert!(
        text.contains("A push to a pre-branch resolves no version, so no unit builds."),
        "{text}"
    );
    // No table: the sentence why, as a notice and in the summary.
    assert!(!text.contains("This commit"), "{text}");
    assert!(
        text.contains(
            "::notice title=Pre-branch::This run follows a push to the pre-branch `pre-x%251`."
        ),
        "{text}"
    );
    let written = Files::read(&files.summary());
    for part in [
        "`pre-x%1`",
        "not built yet",
        "no version is resolved",
        "nothing is built",
        "nothing is published",
    ] {
        assert!(written.contains(part), "{written}");
    }
    assert!(!written.contains("## Versions"), "{written}");

    // A summary that cannot be written fails the call, and nothing is handed
    // on.
    let unwritable = Files::new();
    let dir = unwritable.dir.path().to_str().unwrap();
    let output = unwritable.output();
    let mut variables = pre_branch.to_vec();
    variables.push(("GITHUB_STEP_SUMMARY", dir));
    variables.push(("GITHUB_OUTPUT", output.to_str().unwrap()));
    let (text, code) = call_with(
        &["nebensound-repo", "-C", git.root(), "declare"],
        &variables,
        Output::Workflow,
    );
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::the summary of the run cannot be written to "),
        "{text}"
    );
    assert_eq!(Files::read(&output), "");
}

#[test]
fn a_pull_request_that_changes_the_audience_names_it() {
    let git = repository();
    git.write(&[(".nebensound/repo.toml", &declared("internal"))]);
    git.commit("chore: say it is internal");
    let files = Files::new();
    let pull_request = [
        ("NEBENSOUND_VISIBILITY", "internal"),
        ("NEBENSOUND_PULL_REQUEST", "7"),
        ("NEBENSOUND_TITLE", "chore: make it internal"),
        ("NEBENSOUND_BODY", ""),
    ];
    let (text, code) = declare(&git, &files, &pull_request);
    assert_eq!(code, 0, "{text}");
    let line = "This pull request changes the audience of the repository from public to \
                internal.";
    assert!(
        text.contains(&format!("::notice title=Audience changed::{line}\n")),
        "{text}"
    );
    // The versions are those of the pull request, in the table and in the
    // plan.
    let message = [
        "--pull-request",
        "7",
        "--message",
        "chore: make it internal",
    ];
    let table = version(&git, &message);
    assert!(table.contains("-pr.7"), "{table}");
    assert!(text.contains(&table), "{text}");
    let mut markdown = vec!["--markdown"];
    markdown.extend(message);
    assert_eq!(
        Files::read(&files.summary()),
        format!("{}\n{line}\n", version(&git, &markdown))
    );
    let plan: Value = serde_json::from_str(&files.outputs()[0].1).unwrap();
    let version = plan["versions"][0]["version"].as_str().unwrap();
    assert!(version.ends_with("-pr.7"), "{plan}");

    // A summary that cannot be written fails the call, and nothing is handed
    // on.
    let unwritable = Files::new();
    let dir = unwritable.dir.path().to_str().unwrap();
    let mut variables = pull_request.to_vec();
    variables.push(("GITHUB_STEP_SUMMARY", dir));
    let output = unwritable.output();
    variables.push(("GITHUB_OUTPUT", output.to_str().unwrap()));
    let (text, code) = call_with(
        &["nebensound-repo", "-C", git.root(), "declare"],
        &variables,
        Output::Workflow,
    );
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::the summary of the run cannot be written to "),
        "{text}"
    );
    assert_eq!(Files::read(&output), "");

    // A pull request that leaves it as it is names nothing.
    git.write(&[("README.md", "# y\n")]);
    git.commit("docs: say more");
    let files = Files::new();
    let (text, code) = declare(
        &git,
        &files,
        &[
            ("NEBENSOUND_VISIBILITY", "internal"),
            ("NEBENSOUND_PULL_REQUEST", "8"),
            ("NEBENSOUND_TITLE", "fix: something else"),
            ("NEBENSOUND_BODY", "With a description."),
        ],
    );
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("Audience changed"), "{text}");
    assert!(
        !Files::read(&files.summary()).contains("audience"),
        "{text}"
    );
}

#[test]
fn a_change_of_the_audience_that_cannot_be_named_in_the_summary_fails() {
    let dir = tempfile::tempdir().unwrap();
    let summary = dir.path().to_str().unwrap().to_owned();
    let variables = |name: &str| (name == "GITHUB_STEP_SUMMARY").then(|| summary.clone());
    let run = Run {
        name: None,
        default_branch: None,
        visibility: Visibility::Public,
        pre_branch: false,
    };
    let mut out = Vec::new();
    let mut log = Log::new(&mut out, true);
    let handed_on = declare::hand_on(
        &plan(Some(Audience::Public), None),
        &Register::builtin().unwrap(),
        &run,
        &[],
        &variables,
        &mut log,
    );
    drop(log);
    let text = support::printed(&String::from_utf8(out).unwrap());
    assert!(!handed_on, "{text}");
    assert!(
        text.contains("::error::the summary of the run cannot be written to "),
        "{text}"
    );
    assert!(!text.contains("::group::The units"), "{text}");
}

#[test]
fn phase_0_errors_hand_nothing_on() {
    let git = repository();
    let files = Files::new();
    // The repository is meant for the public and GitHub shows it private.
    let (text, code) = declare(&git, &files, &[("NEBENSOUND_VISIBILITY", "private")]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("::error"), "{text}");
    assert_eq!(Files::read(&files.output()), "");
    assert!(!text.contains("::group::The plan"), "{text}");
    // The findings are annotations of the report of phase 0; no table of
    // versions follows, in the log or in the summary.
    assert!(!text.contains("there are no versions"), "{text}");
    assert_eq!(Files::read(&files.summary()), "");
}

/// Declare on a repository that phase 0 refuses: it fails, hands nothing on,
/// and writes no line of the repository's text as a line of its own. What is
/// returned is the log as the runner gets it, with its stopped stretches.
fn refused(git: &Git) -> String {
    let files = Files::new();
    let (log, code) = declare_log(git, &files, &[("NEBENSOUND_VISIBILITY", "public")]);
    let text = support::printed(&log);
    assert_eq!(code, 1, "{text}");
    assert_eq!(Files::read(&files.output()), "");
    assert!(!text.contains("::group::The plan"), "{text}");
    for forged in ["b.md", "::warning::forged"] {
        assert!(
            !text.lines().any(|line| line.starts_with(forged)),
            "{forged}\n{text}"
        );
    }
    log
}

#[test]
fn a_tracked_file_whose_name_holds_a_line_break_stops_phase_0_by_name() {
    let git = repository();
    git.write(&[("docs/a\nb.md", "# a\n")]);
    git.commit("docs: a name with a line break");

    let log = refused(&git);
    // The property names the file in the runner's own escape, which the
    // runner undoes, so the annotation sits on the file; the message shows
    // the name as the report does. It stands outside every stopped stretch,
    // where the runner takes it as an annotation.
    let line = "::error file=docs/a%0Ab.md,title=control-character::[control-character] at \
                docs/a\\nb.md: the name of this file holds a control character, shown here as \
                an escape. A tool that takes names one per line, or as the value of a step \
                output, would misread it; rename the file";
    assert!(support::outside(&log).contains(&line), "{log}");
}

#[test]
fn a_declared_path_that_holds_a_line_break_stops_phase_0_by_name() {
    let git = repository();
    git.write(&[(
        ".nebensound/repo.toml",
        &format!(
            "{}\n[[exclude]]\npath = \"app/a\\n::warning::forged\"\nreason = \"x\"\n",
            declared("public")
        ),
    )]);
    git.commit("chore: a path with a line break");

    let log = refused(&git);
    // It stands outside every stopped stretch, where the runner takes it as an
    // annotation.
    let line = "::error file=.nebensound/repo.toml,title=control-character::[control-character] \
                at .nebensound/repo.toml: the path of [[exclude]] entry 2 is \
                \"app/a\\n::warning::forged\", which holds a control character, shown here as \
                an escape. No tracked file may carry it either; write the path without it";
    assert!(support::outside(&log).contains(&line), "{log}");
}

#[test]
fn a_pull_request_whose_merge_cannot_be_given_a_version_gets_no_plan() {
    let git = Git::new();
    git.write(&[
        (
            ".nebensound/repo.toml",
            "[repo]\naudience = \"public\"\npipeline = \"none\"\nminimum-version = \
             \"0.18446744073709551615.0\"\n",
        ),
        ("README.md", "# x\n"),
    ]);
    git.commit("chore: start");
    // The merge commit GitHub builds for the pull request.
    git.run(&["checkout", "--quiet", "-b", "topic"]);
    git.write(&[("README.md", "# y\n")]);
    git.commit("chore: b");
    git.run(&["checkout", "--quiet", "--detach", "main"]);
    git.run(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "chore: merge",
        "topic",
    ]);
    let files = Files::new();
    let (text, code) = declare(
        &git,
        &files,
        &[
            ("NEBENSOUND_VISIBILITY", "public"),
            ("NEBENSOUND_PULL_REQUEST", "3"),
            ("NEBENSOUND_TITLE", "feat!: break it"),
        ],
    );
    assert_eq!(code, 1, "{text}");
    // Phase 0 on the commit at hand passed; the versions of the merge say
    // why there are none, in the log and in the summary.
    assert!(
        text.contains("there are no versions: phase 0 found errors\n"),
        "{text}"
    );
    assert!(
        Files::read(&files.summary())
            .starts_with("## Versions\n\nThere are no versions: phase 0 found errors.\n"),
        "{text}"
    );
    assert_eq!(Files::read(&files.output()), "");
    assert!(!text.contains("::group::The plan"), "{text}");
}

/// A repository whose component `app`, a library for the private registry,
/// requires `lib` by path, and the merge commit GitHub builds for a pull
/// request that writes `changed` below `app` alone. Nothing records `lib`'s
/// version.
fn unreleased_requirement(changed: &[(&str, &str)]) -> Git {
    let git = Git::new();
    git.write(&[
        (
            ".nebensound/repo.toml",
            "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.app]\npath = \
             \"app\"\npipeline = \"rust-library-package\"\n\n[component.lib]\npath = \
             \"lib\"\npipeline = \"rust-library-package\"\n",
        ),
        ("README.md", "# Packages\n"),
        (
            "app/Cargo.toml",
            "[package]\nname = \"app\"\nedition = \"2024\"\n\n[dependencies]\nlib = { path = \
             \"../lib\" }\n",
        ),
        (
            "lib/Cargo.toml",
            "[package]\nname = \"lib\"\nedition = \"2024\"\n",
        ),
    ]);
    git.commit("chore: start");
    git.run(&["checkout", "--quiet", "-b", "topic"]);
    git.write(changed);
    git.commit("wip");
    git.run(&["checkout", "--quiet", "--detach", "main"]);
    git.run(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "Merge topic",
        "topic",
    ]);
    git
}

#[test]
fn a_pull_request_whose_release_would_require_an_unreleased_version_fails_and_hands_nothing_on() {
    let git = unreleased_requirement(&[("app/README.md", "# App\n")]);
    let files = Files::new();
    let (text, code) = declare(&git, &files, &PULL_REQUEST);
    assert_eq!(code, 1, "{text}");
    // Phase 0 passed, and the versions of the pull request say why the job
    // fails.
    assert!(
        text.contains(
            "there are no versions: a release would require a version that was never released\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("error[unreleased-requirement] component app: the package app goes to"),
        "{text}"
    );
    let summary = Files::read(&files.summary());
    assert!(
        summary.starts_with(
            "## Versions\n\nThere are no versions: a release would require a version that was \
             never released.\n\n"
        ),
        "{summary}"
    );
    assert!(
        summary.contains("- error\\[unreleased\\-requirement\\] component app"),
        "{summary}"
    );
    assert_eq!(Files::read(&files.output()), "");
    assert!(!text.contains("::group::The plan"), "{text}");

    // After the merge, on `main`, nothing is held to it.
    git.run(&["checkout", "--quiet", "main"]);
    git.run(&["merge", "--quiet", "--squash", "topic"]);
    git.commit("fix(app): the change");
    let files = Files::new();
    let (text, code) = declare(&git, &files, &[("NEBENSOUND_VISIBILITY", "public")]);
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("unreleased-requirement"), "{text}");
}

/// The variables of pull request 7.
const PULL_REQUEST: [(&str, &str); 4] = [
    ("NEBENSOUND_VISIBILITY", "public"),
    ("NEBENSOUND_PULL_REQUEST", "7"),
    ("NEBENSOUND_TITLE", "fix(app): the change"),
    ("NEBENSOUND_BODY", ""),
];

#[test]
fn a_manifest_the_requirements_cannot_read_fails_the_call() {
    let git = unreleased_requirement(&[("app/Cargo.toml", "[package\n")]);
    let files = Files::new();
    let (text, code) = declare(&git, &files, &PULL_REQUEST);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("app/Cargo.toml is not TOML: "), "{text}");
    assert_eq!(Files::read(&files.output()), "");
    assert_eq!(Files::read(&files.summary()), "");
}

#[test]
fn outputs_that_cannot_be_written_fail_the_call() {
    let git = repository();
    let files = Files::new();
    let dir = files.dir.path().to_str().unwrap();
    let (text, code) = call_with(
        &["nebensound-repo", "-C", git.root(), "declare"],
        &[("NEBENSOUND_VISIBILITY", "public"), ("GITHUB_OUTPUT", dir)],
        Output::Workflow,
    );
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::the outputs of the step cannot be written to "),
        "{text}"
    );
}

#[test]
fn a_number_that_is_no_pull_request_is_a_misuse() {
    let git = repository();
    let (text, code) = call_with(
        &["nebensound-repo", "-C", git.root(), "declare"],
        &[("NEBENSOUND_PULL_REQUEST", "seven")],
        Output::Workflow,
    );
    let text = support::printed(&text);
    assert_eq!(code, 2, "{text}");
    assert!(
        text.starts_with("NEBENSOUND_PULL_REQUEST is \"seven\""),
        "{text}"
    );
}

/// A curl that writes its configuration beside itself and prints `answer`.
fn curl(answer: &str) -> TempDir {
    let bin = tempfile::tempdir().unwrap();
    executable(
        &bin.path().join("curl"),
        format!("#!/bin/sh\ncat > \"$(dirname \"$0\")/config\"\nprintf '%s' '{answer}'\n"),
    );
    bin
}

#[test]
fn without_a_visibility_in_the_event_github_is_asked() {
    let git = repository();
    let files = Files::new();
    let bin = curl("{\"visibility\":\"public\"}\n200");
    let path = bin.path().to_str().unwrap();
    let api = [
        ("NEBENSOUND_VISIBILITY", ""),
        ("GITHUB_API_URL", "https://api.example/"),
        ("GITHUB_REPOSITORY", "owner/name"),
        ("GITHUB_TOKEN", "the-token"),
        ("PATH", path),
    ];
    let (text, code) = declare(&git, &files, &api);
    assert_eq!(code, 0, "{text}");
    let config = fs::read_to_string(bin.path().join("config")).unwrap();
    assert!(
        config.ends_with("url = \"https://api.example/repos/owner/name\"\n"),
        "{config}"
    );
    assert!(config.contains("request = \"GET\""), "{config}");
    assert!(!text.contains("the-token"), "{text}");

    // What GitHub answers has to name one.
    let bin = curl("{}\n200");
    let path = bin.path().to_str().unwrap();
    let mut variables: Vec<(&str, &str)> = api
        .iter()
        .copied()
        .filter(|(name, _)| *name != "PATH")
        .collect();
    variables.push(("PATH", path));
    let (text, code) = declare(&git, &Files::new(), &variables);
    assert_eq!(code, 1, "{text}");
    assert_eq!(
        text,
        "GitHub answered GET the repository without `visibility`\n"
    );

    // A call that fails stops phase 0 before it runs. Like curl, the stand-in
    // reads its configuration before it fails: one that ended first would
    // leave the tool writing into a closed pipe.
    let bin = tempfile::tempdir().unwrap();
    executable(
        &bin.path().join("curl"),
        "#!/bin/sh\ncat > /dev/null\necho refused >&2\nexit 7\n",
    );
    let path = bin.path().to_str().unwrap();
    let mut variables: Vec<(&str, &str)> = api
        .iter()
        .copied()
        .filter(|(name, _)| *name != "PATH")
        .collect();
    variables.push(("PATH", path));
    let (text, code) = declare(&git, &Files::new(), &variables);
    assert_eq!(code, 1, "{text}");
    assert!(text.starts_with("curl failed on GET "), "{text}");
    assert!(text.ends_with(": refused\n"), "{text}");

    // Without what a call needs, it is not made.
    for missing in ["GITHUB_API_URL", "GITHUB_REPOSITORY", "GITHUB_TOKEN"] {
        let variables: Vec<(&str, &str)> = api
            .iter()
            .copied()
            .filter(|(name, _)| *name != missing)
            .collect();
        let (text, code) = declare(&git, &Files::new(), &variables);
        assert_eq!(code, 1, "{missing}: {text}");
        assert_eq!(
            text,
            format!(
                "the event names no visibility, and asking GitHub for it needs {missing}, which \
                 is not set\n"
            )
        );
    }
}

#[test]
fn a_visibility_github_does_not_give_is_refused() {
    let git = repository();
    let (text, code) = declare(&git, &Files::new(), &[("NEBENSOUND_VISIBILITY", "secret")]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(
        text,
        "the visibility of the repository is \"secret\", and GitHub gives public, private or \
         internal\n"
    );
    for visibility in ["private", "internal"] {
        let variables = |name: &str| match name {
            "NEBENSOUND_VISIBILITY" => Some(visibility.to_owned()),
            _ => None,
        };
        let run = Run::read(&variables).unwrap();
        assert_eq!(run.visibility.as_str(), visibility);
        assert_eq!(run.name, None);
        assert!(!run.pre_branch);
    }
}

fn plan(audience: Option<Audience>, previous: Option<Audience>) -> Plan {
    Plan {
        units: Vec::new(),
        versions: Vec::new(),
        pull_request: Some(1),
        audience,
        previous_audience: previous,
    }
}

#[test]
fn a_change_of_the_audience_is_named_with_both_values() {
    assert_eq!(
        declare::audience_change(&plan(Some(Audience::Public), None)).as_deref(),
        Some(
            "This pull request changes the audience of the repository from none declared to \
             public."
        )
    );
    assert_eq!(
        declare::audience_change(&plan(Some(Audience::Public), Some(Audience::Public))),
        None
    );
    assert_eq!(declare::audience_change(&plan(None, None)), None);
}

#[test]
fn the_matrices_take_what_the_plan_starts() {
    let plan = json!({
        "units": [
            {"kind": "repository", "name": null, "path": ".", "checks": ["json"], "build": null},
            {"kind": "component", "name": "a", "path": "a", "checks": [], "build": "docker"},
            {"kind": "component", "name": "b", "path": "b", "checks": [], "build": "docker"},
            {"kind": "component", "name": "c", "path": "c", "checks": [], "build": "docker"}
        ],
        "versions": [
            {"kind": "component", "name": "a", "version": "1.0.0", "writers": ["cargo"]},
            {"kind": "component", "name": "b", "version": null, "writers": []}
        ]
    });
    let register = Register::builtin().unwrap();
    assert_eq!(
        declare::units(&plan),
        json!([{"kind": "repository", "name": null, "path": ".", "checks": ["json"],
                "build": null}])
    );
    assert_eq!(
        declare::builds(&plan, false, &register),
        json!([{"kind": "component", "name": "a", "path": "a", "checks": [], "build": "docker",
                "writers": ["cargo"], "job": "Build component a", "runs-on": "ubuntu-latest"}])
    );
    assert_eq!(declare::builds(&plan, true, &register), json!([]));
    assert_eq!(declare::units(&json!({})), json!([]));
    assert_eq!(declare::builds(&json!({}), false, &register), json!([]));
}

#[test]
fn a_build_with_targets_builds_once_per_target_on_its_runner() {
    let plan = json!({
        "units": [
            {"kind": "repository", "name": null, "path": ".", "checks": [],
             "build": "cargo-release"},
            {"kind": "component", "name": "a", "path": "a", "checks": [], "build": "unknown"}
        ],
        "versions": [
            {"kind": "repository", "name": null, "version": "1.0.0-pr.3", "writers": ["cargo"]},
            {"kind": "component", "name": "a", "version": "2.0.0", "writers": []}
        ]
    });
    let register = Register::builtin().unwrap();
    let entry = |target: &str, runner: &str| {
        json!({"kind": "repository", "name": null, "path": ".", "checks": [],
               "build": "cargo-release", "writers": ["cargo"],
               "job": format!("Build repository ({target})"), "runs-on": runner,
               "target": target})
    };
    assert_eq!(
        declare::builds(&plan, false, &register),
        json!([
            entry("aarch64-apple-darwin", "macos-latest"),
            entry("x86_64-apple-darwin", "macos-26-intel"),
            entry("x86_64-unknown-linux-musl", "ubuntu-latest"),
            // A build the register does not know lists no targets either.
            {"kind": "component", "name": "a", "path": "a", "checks": [], "build": "unknown",
             "writers": [], "job": "Build component a", "runs-on": "ubuntu-latest"}
        ])
    );
}

#[test]
fn a_repository_that_cannot_be_read_stops_the_call() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    let (text, code) = call_with(
        &[
            "nebensound-repo",
            "-C",
            missing.to_str().unwrap(),
            "declare",
        ],
        &[("NEBENSOUND_VISIBILITY", "public")],
        Output::Workflow,
    );
    assert_eq!(code, 1, "{text}");
    assert!(!text.contains("::group::"), "{text}");
}
