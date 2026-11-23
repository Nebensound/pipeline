//! The closing job `Build` of `nebensound.yml` reports what its needs found.
//!
//! Its script runs here as the runner hands it to bash, read from the workflow
//! itself, so no copy of it can drift.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::Path;
use std::process::Command;

use yaml_rust2::YamlLoader;

/// The script of the step `name` in the job `job` of `nebensound.yml`.
fn script(job: &str, name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/nebensound.yml");
    let text = fs::read_to_string(path).unwrap();
    let workflow = &YamlLoader::load_from_str(&text).unwrap()[0];
    workflow["jobs"][job]["steps"]
        .as_vec()
        .unwrap()
        .iter()
        .find(|step| step["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("the step '{name}' of the job '{job}'"))["run"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// What the closing job `Build` says for these results of its needs and this
/// list of planned builds: whether it passes, and what it prints.
fn build(declare: &str, check: &str, builds: &str, planned: &str) -> (bool, String) {
    let output = Command::new("bash")
        .args(["--noprofile", "--norc", "-eo", "pipefail", "-c"])
        .arg(script("build", "Gather the builds"))
        .env("DECLARE", declare)
        .env("CHECK", check)
        .env("BUILDS", builds)
        .env("PLANNED", planned)
        .env_remove("NEBENSOUND_PRE_BRANCH")
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8(output.stdout).unwrap(),
    )
}

const ONE: &str = r#"[{"path":"tools/a","build":"cargo-package","writers":["cargo"]}]"#;

#[test]
fn without_phase_0_the_builds_fail() {
    let (passed, said) = build("failure", "skipped", "skipped", "");
    assert!(!passed);
    assert!(said.contains("Phase 0 did not pass (failure)"), "{said}");
}

#[test]
fn when_nothing_builds_the_builds_pass_whatever_check_found() {
    for check in ["success", "failure", "cancelled"] {
        let (passed, said) = build("success", check, "skipped", "[]");
        assert!(passed, "{check}: {said}");
        assert!(
            said.contains("Nothing builds in this run"),
            "{check}: {said}"
        );
    }
}

#[test]
fn a_planned_build_after_a_failed_check_fails() {
    let (passed, said) = build("success", "failure", "skipped", ONE);
    assert!(!passed);
    assert!(said.contains("Check did not pass (failure)"), "{said}");
}

#[test]
fn the_builds_pass_when_every_planned_build_passed() {
    let (passed, said) = build("success", "success", "success", ONE);
    assert!(passed, "{said}");
    assert!(
        said.contains("Every build the plan started passed"),
        "{said}"
    );
}

#[test]
fn planned_builds_that_did_not_run_or_failed_fail() {
    for (builds, finding) in [
        ("skipped", "The plan names builds, and none of them ran"),
        ("failure", "At least one build did not pass (failure)"),
        ("cancelled", "At least one build did not pass (cancelled)"),
    ] {
        let (passed, said) = build("success", "success", builds, ONE);
        assert!(!passed, "{builds}: {said}");
        assert!(said.contains(finding), "{builds}: {said}");
    }
}
