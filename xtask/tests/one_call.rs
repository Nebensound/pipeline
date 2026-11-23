//! A step of `nebensound.yml` or of an action under `.github/actions/` is one
//! call of the tool: all logic is Rust, in the tool, and tested.
//!
//! The exceptions are named, each with what keeps it: the action `tool`, which
//! gets the tool and cannot call it before it has it, until the tool is a
//! release binary; and `build-docker`, the image build, which moves into the
//! tool on its own.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::Path;

use yaml_rust2::{Yaml, YamlLoader};

/// The actions whose steps still hold more than a call.
const EXCEPTIONS: &[&str] = &["build-docker", "tool"];

fn parsed(path: &Path) -> Yaml {
    YamlLoader::load_from_str(&fs::read_to_string(path).unwrap()).unwrap()[0].clone()
}

/// What keeps `run` from being one call of the tool, if anything does.
fn more_than_a_call(run: &str) -> Option<String> {
    let run = run.trim_end();
    if run.contains('\n') || !run.starts_with("nebensound-repo ") {
        return Some("holds more than one call of the tool".to_owned());
    }
    ["jq ", " && ", " || ", "; ", "$(", " | ", "`"]
        .into_iter()
        .find(|shell| run.contains(shell))
        .map(|shell| format!("holds shell beside the call ({shell:?})"))
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
    let mut excepted = 0;
    for entry in fs::read_dir(&actions).unwrap() {
        let dir = entry.unwrap().path();
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let action = parsed(&dir.join("action.yml"));
        let steps = scripts(&action["runs"]["steps"], &format!("action {name}"));
        if EXCEPTIONS.contains(&name.as_str()) {
            // The list only shrinks: an action that has become one call
            // everywhere leaves it, so that the rule holds it from then on.
            assert!(
                steps.iter().any(|(_, run)| more_than_a_call(run).is_some()),
                "action {name} is one call of the tool everywhere now; take it off EXCEPTIONS"
            );
            excepted += 1;
            continue;
        }
        all.extend(steps);
        seen += 1;
    }
    assert!(seen > 0);
    assert_eq!(
        excepted,
        EXCEPTIONS.len(),
        "every exception names an action"
    );
    assert!(!all.is_empty());
    for (place, run) in all {
        let fault = more_than_a_call(&run);
        assert!(
            fault.is_none(),
            "{place} {}:\n{}",
            fault.unwrap_or_default(),
            run.trim_end()
        );
    }
}
