//! A step of `nebensound.yml` or of an action under `.github/actions/` is one
//! call of the tool: all logic is Rust, in the tool, and tested.
//!
//! The exceptions are named, each with what keeps it: the action `tool`, which
//! gets the tool and cannot call it before it has it, until the tool is a
//! release binary; `build-docker`, the image build, which moves into the tool
//! on its own; and `check-python`, whose check moves into the tool on its own
//! as well.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::Path;

use yaml_rust2::{Yaml, YamlLoader};

/// The actions whose steps still hold more than a call.
const EXCEPTIONS: &[&str] = &["build-docker", "check-python", "tool"];

fn parsed(path: &Path) -> Yaml {
    YamlLoader::load_from_str(&fs::read_to_string(path).unwrap()).unwrap()[0].clone()
}

/// Every `run:` of these steps, named by `place`.
fn scripts(steps: &Yaml, place: &str) -> Vec<(String, String)> {
    steps
        .as_vec()
        .into_iter()
        .flatten()
        .filter_map(|step| {
            step["run"].as_str().map(|run| {
                let name = step["name"].as_str().unwrap_or("a step without a name");
                (format!("{place}, {name}"), run.to_owned())
            })
        })
        .collect()
}

#[test]
fn every_step_outside_the_exceptions_is_one_call_of_the_tool() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut all = Vec::new();
    let workflow = parsed(&root.join(".github/workflows/nebensound.yml"));
    for (job, body) in workflow["jobs"].as_hash().unwrap() {
        let job = job.as_str().unwrap();
        all.extend(scripts(
            &body["steps"],
            &format!("nebensound.yml, job {job}"),
        ));
    }
    let actions = root.join(".github/actions");
    let mut seen = 0;
    for entry in fs::read_dir(&actions).unwrap() {
        let dir = entry.unwrap().path();
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        if EXCEPTIONS.contains(&name.as_str()) {
            continue;
        }
        let action = parsed(&dir.join("action.yml"));
        all.extend(scripts(&action["runs"]["steps"], &format!("action {name}")));
        seen += 1;
    }
    assert!(seen > 0);
    assert!(!all.is_empty());
    for (place, run) in all {
        let run = run.trim_end();
        assert!(
            !run.contains('\n') && run.starts_with("nebensound-repo "),
            "{place} holds more than one call of the tool:\n{run}"
        );
        for shell in ["jq ", " && ", " || ", "; ", "$(", " | ", "`"] {
            assert!(
                !run.contains(shell),
                "{place} holds shell beside the call ({shell:?}): {run}"
            );
        }
    }
}
