//! A push to a pre-branch is checked, and resolves, builds and publishes
//! nothing.
//!
//! `nebensound.yml` sets `NEBENSOUND_PRE_BRANCH` for the whole run, and the
//! steps that would read the versions of the branch as those of `main` read it.
//! Their scripts run here as the runner hands them to bash, read from the
//! workflow itself, so no copy of them can drift.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::Path;
use std::process::Command;

use yaml_rust2::{Yaml, YamlLoader};

/// `nebensound.yml`, parsed.
fn workflow() -> Yaml {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/nebensound.yml");
    YamlLoader::load_from_str(&fs::read_to_string(path).unwrap()).unwrap()[0].clone()
}

/// The script of the step `name` in the job `job` of `nebensound.yml`.
fn script(job: &str, name: &str) -> String {
    workflow()["jobs"][job]["steps"]
        .as_vec()
        .unwrap()
        .iter()
        .find(|step| step["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("the step '{name}' of the job '{job}'"))["run"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// What a step said: whether it passed, its log, its outputs and its summary.
struct Said {
    passed: bool,
    log: String,
    output: String,
    summary: String,
}

/// Runs the step `name` of the job `job` with this environment, after a push
/// to a pre-branch or not. `PATH` holds no tool: a step that should leave the
/// tool out and calls it fails.
fn run(job: &str, name: &str, pre_branch: &str, env: &[(&str, &str)]) -> Said {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("output");
    let summary = dir.path().join("summary");
    fs::write(&output, "").unwrap();
    fs::write(&summary, "").unwrap();
    let mut command = Command::new("bash");
    command
        .args(["--noprofile", "--norc", "-eo", "pipefail", "-c"])
        .arg(script(job, name))
        .env("NEBENSOUND_PRE_BRANCH", pre_branch)
        .env("GITHUB_OUTPUT", &output)
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("GITHUB_WORKSPACE", dir.path())
        .env("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin");
    for (key, value) in env {
        command.env(key, value);
    }
    let ran = command.output().unwrap();
    Said {
        passed: ran.status.success(),
        log: String::from_utf8(ran.stdout).unwrap(),
        output: fs::read_to_string(&output).unwrap(),
        summary: fs::read_to_string(&summary).unwrap(),
    }
}

/// A plan with one unit that builds and has a version of the run.
const PLAN: &str = r#"{"units":[{"kind":"repository","name":null,"path":".","checks":[],"build":"docker"}],"versions":[{"kind":"repository","name":null,"path":".","prefix":"v","current":"0.1.0","version":"0.1.0","writers":[],"targets":[]}]}"#;

#[test]
fn the_workflow_names_the_pre_branches_for_the_whole_run() {
    let value = workflow()["env"]["NEBENSOUND_PRE_BRANCH"]
        .as_str()
        .unwrap()
        .to_owned();
    for part in [
        "github.event_name == 'push'",
        "github.ref == 'refs/heads/pre'",
        "startsWith(github.ref, 'refs/heads/pre-')",
    ] {
        assert!(value.contains(part), "{value}");
    }
}

#[test]
fn after_a_push_to_a_pre_branch_version_resolves_nothing_and_says_so() {
    let said = run(
        "version",
        "Report the versions",
        "true",
        &[("NUMBER", ""), ("BRANCH", "pre-x%1")],
    );
    assert!(said.passed, "{}", said.log);
    assert!(
        said.log.starts_with(
            "::notice title=Pre-branch::This run follows a push to the pre-branch `pre-x%251`."
        ),
        "{}",
        said.log
    );
    for part in [
        "not built yet",
        "no version is resolved",
        "nothing is built",
        "nothing is published",
    ] {
        assert!(said.log.contains(part), "{}", said.log);
        assert!(said.summary.contains(part), "{}", said.summary);
    }
    assert!(said.summary.contains("`pre-x%1`"), "{}", said.summary);
}

#[test]
fn elsewhere_version_calls_the_tool() {
    let said = run(
        "version",
        "Report the versions",
        "false",
        &[("NUMBER", ""), ("BRANCH", "main")],
    );
    // The tool is not on the PATH here, so the step fails where it calls it.
    assert!(!said.passed, "{}", said.log);
    assert!(!said.summary.contains("pre-branch"), "{}", said.summary);
}

#[test]
fn after_a_push_to_a_pre_branch_no_unit_builds() {
    let said = run(
        "declare",
        "Name the units that build",
        "true",
        &[("PLAN", PLAN)],
    );
    assert!(said.passed, "{}", said.log);
    assert_eq!(said.output, "builds=[]\n");
    assert!(said.log.contains("resolves no version"), "{}", said.log);
}

#[test]
fn elsewhere_the_unit_with_a_version_builds() {
    let said = run(
        "declare",
        "Name the units that build",
        "false",
        &[("PLAN", PLAN)],
    );
    assert!(said.passed, "{}", said.log);
    assert!(
        said.output.contains(r#""build":"docker""#),
        "{}",
        said.output
    );
}

#[test]
fn after_a_push_to_a_pre_branch_build_passes_and_says_why_nothing_built() {
    let said = run(
        "build",
        "Gather the builds",
        "true",
        &[
            ("DECLARE", "success"),
            ("CHECK", "success"),
            ("BUILDS", "skipped"),
            ("PLANNED", "[]"),
        ],
    );
    assert!(said.passed, "{}", said.log);
    assert!(
        said.log
            .contains("Nothing builds in this run: a push to a pre-branch resolves no version."),
        "{}",
        said.log
    );
}

#[test]
fn publish_runs_after_a_push_to_main_only() {
    let condition = workflow()["jobs"]["publish"]["if"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        condition.contains("github.event_name == 'push' && github.ref == 'refs/heads/main'"),
        "{condition}"
    );
}
