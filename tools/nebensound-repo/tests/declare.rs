//! `nebensound-repo declare`: phase 0 as the job Declare runs it, and what it
//! hands the jobs after it.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use nebensound_repo::Audience;
use nebensound_repo::declare::{self, Run};
use nebensound_repo::output::Output;
use nebensound_repo::plan::Plan;
use serde_json::{Value, json};
use support::{Git, call_with};
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

/// `declare` on `git` with these variables beside the files of the run.
fn declare(git: &Git, files: &Files, variables: &[(&str, &str)]) -> (String, u8) {
    let output = files.output();
    let summary = files.summary();
    let mut all = vec![
        ("GITHUB_OUTPUT", output.to_str().unwrap()),
        ("GITHUB_STEP_SUMMARY", summary.to_str().unwrap()),
        ("NEBENSOUND_NAME", "case"),
        ("NEBENSOUND_DEFAULT_BRANCH", "main"),
    ];
    all.extend_from_slice(variables);
    call_with(
        &["nebensound-repo", "-C", git.root(), "declare"],
        &all,
        Output::Workflow,
    )
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
                "path": "app", "writers": ["cargo"]}])
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
    // The report of phase 0 comes first, as `check` prints it.
    assert!(text.find("Tree").unwrap() < text.find("::group::The plan").unwrap());
    // No pull request, no change of the audience to name.
    assert!(!text.contains("Audience changed"), "{text}");
    assert_eq!(Files::read(&files.summary()), "");
}

#[test]
fn after_a_push_to_a_pre_branch_no_unit_builds() {
    let git = repository();
    let files = Files::new();
    let (text, code) = declare(
        &git,
        &files,
        &[
            ("NEBENSOUND_VISIBILITY", "public"),
            ("NEBENSOUND_PRE_BRANCH", "true"),
        ],
    );
    assert_eq!(code, 0, "{text}");
    assert_eq!(files.outputs()[2], ("builds".to_owned(), "[]".to_owned()));
    assert!(
        text.contains("A push to a pre-branch resolves no version, so no unit builds."),
        "{text}"
    );
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
    assert_eq!(Files::read(&files.summary()), format!("{line}\n"));
    // The versions are those of the pull request.
    let plan: Value = serde_json::from_str(&files.outputs()[0].1).unwrap();
    let version = plan["versions"][0]["version"].as_str().unwrap();
    assert!(version.ends_with("-pr.7"), "{plan}");

    // A summary that cannot be written fails the call.
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
    assert_eq!(Files::read(&files.summary()), "");
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
    assert!(
        text.ends_with("there is no plan: phase 0 found errors, and `check` says which\n"),
        "{text}"
    );
    assert_eq!(Files::read(&files.output()), "");
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
    assert_eq!(code, 2, "{text}");
    assert!(
        text.starts_with("NEBENSOUND_PULL_REQUEST is \"seven\""),
        "{text}"
    );
}

/// A curl that writes its configuration beside itself and prints `answer`.
fn curl(answer: &str) -> TempDir {
    let bin = tempfile::tempdir().unwrap();
    let file = bin.path().join("curl");
    fs::write(
        &file,
        format!("#!/bin/sh\ncat > \"$(dirname \"$0\")/config\"\nprintf '%s' '{answer}'\n"),
    )
    .unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
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

    // A call that fails stops phase 0 before it runs.
    let bin = curl("");
    fs::write(
        bin.path().join("curl"),
        "#!/bin/sh\necho refused >&2\nexit 7\n",
    )
    .unwrap();
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
    assert_eq!(
        declare::units(&plan),
        json!([{"kind": "repository", "name": null, "path": ".", "checks": ["json"],
                "build": null}])
    );
    assert_eq!(
        declare::builds(&plan, false),
        json!([{"kind": "component", "name": "a", "path": "a", "checks": [], "build": "docker",
                "writers": ["cargo"]}])
    );
    assert_eq!(declare::builds(&plan, true), json!([]));
    assert_eq!(declare::units(&json!({})), json!([]));
    assert_eq!(declare::builds(&json!({}), false), json!([]));
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
