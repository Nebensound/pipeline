//! A push to a pre-branch is checked, and resolves, builds and publishes
//! nothing.
//!
//! `nebensound.yml` sets `NEBENSOUND_PRE_BRANCH` for the whole run, and the
//! calls of the tool that would read the versions of the branch as those of
//! `main` read it: `declare` and `gather builds`, whose tests show what they do
//! with it.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::Path;

use yaml_rust2::{Yaml, YamlLoader};

/// `nebensound.yml`, parsed.
fn workflow() -> Yaml {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/nebensound.yml");
    YamlLoader::load_from_str(&fs::read_to_string(path).unwrap()).unwrap()[0].clone()
}

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
