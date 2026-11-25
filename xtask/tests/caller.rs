//! The caller file is one file, wherever it is shown.
//!
//! The README shows the whole caller file a repository adopts. This
//! repository's own `gate.yml` differs from it only in the reference of the
//! workflow, and the documents that quote a part of it quote it as the README
//! has it. A permission or a trigger changed in one place and not in the others
//! fails here.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::Path;

use yaml_rust2::{Yaml, YamlLoader};

/// The text of a file of the repository.
fn text(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path))
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// Every YAML block of a Markdown file below the heading `heading`, up to the
/// next heading of the same or a higher level, each parsed.
fn blocks(path: &str, heading: &str) -> Vec<Yaml> {
    let text = text(path);
    let level = heading.chars().take_while(|c| *c == '#').count();
    let mut found = Vec::new();
    // The YAML block being read, if one is open; a fence of another language
    // is skipped as a whole, so that a `#` at the start of one of its lines is
    // never read as a heading.
    let mut block: Option<Vec<&str>> = None;
    let mut other_fence = false;
    for line in text.lines().skip_while(|line| *line != heading).skip(1) {
        if let Some(lines) = block.as_mut() {
            if line == "```" {
                let source = lines.join("\n");
                found.push(YamlLoader::load_from_str(&source).unwrap()[0].clone());
                block = None;
            } else {
                lines.push(line);
            }
        } else if other_fence {
            other_fence = line != "```";
        } else if line == "```yaml" {
            block = Some(Vec::new());
        } else if line.starts_with("```") {
            other_fence = true;
        } else {
            let hashes = line.chars().take_while(|c| *c == '#').count();
            if hashes > 0 && hashes <= level && line[hashes..].starts_with(' ') {
                break;
            }
        }
    }
    assert!(!found.is_empty(), "{path} holds no YAML below '{heading}'");
    found
}

/// The caller file as the README shows it.
fn readme_caller() -> Yaml {
    blocks("README.md", "## Calling the pipeline")[0].clone()
}

/// This repository's caller file.
fn gate() -> Yaml {
    YamlLoader::load_from_str(&text(".github/workflows/gate.yml")).unwrap()[0].clone()
}

/// A key of a mapping.
fn key(name: &str) -> Yaml {
    Yaml::String(name.to_owned())
}

/// What the caller's job grants, in the order the caller file names it.
const GRANTED: &[(&str, &str)] = &[
    ("contents", "write"),
    ("pull-requests", "read"),
    ("id-token", "write"),
    ("packages", "write"),
    ("attestations", "write"),
    ("deployments", "write"),
];

#[test]
fn the_readme_caller_grants_the_full_list_at_its_job_and_reads_at_the_top() {
    let caller = readme_caller();
    let granted: Vec<(&str, &str)> = caller["jobs"]["nebensound"]["permissions"]
        .as_hash()
        .unwrap()
        .iter()
        .map(|(name, level)| (name.as_str().unwrap(), level.as_str().unwrap()))
        .collect();
    assert_eq!(granted, GRANTED);
    let top: Vec<(&str, &str)> = caller["permissions"]
        .as_hash()
        .unwrap()
        .iter()
        .map(|(name, level)| (name.as_str().unwrap(), level.as_str().unwrap()))
        .collect();
    assert_eq!(top, [("contents", "read")]);
}

#[test]
fn the_readme_caller_starts_on_every_pull_request_state_and_on_the_pre_branches() {
    let on = &readme_caller()["on"];
    let types: Vec<&str> = on["pull_request"]["types"]
        .as_vec()
        .unwrap()
        .iter()
        .map(|kind| kind.as_str().unwrap())
        .collect();
    assert_eq!(types, ["opened", "synchronize", "reopened", "edited"]);
    let branches: Vec<&str> = on["push"]["branches"]
        .as_vec()
        .unwrap()
        .iter()
        .map(|branch| branch.as_str().unwrap())
        .collect();
    assert_eq!(branches, ["main", "pre", "pre-*"]);
}

#[test]
fn gate_is_the_readme_caller_but_for_the_reference() {
    let readme = readme_caller();
    let gate = gate();
    let mut readme = readme.into_hash().unwrap();
    let mut gate = gate.into_hash().unwrap();
    let job = |caller: &mut yaml_rust2::yaml::Hash| {
        let mut job = caller
            .remove(&key("jobs"))
            .unwrap()
            .into_hash()
            .unwrap()
            .remove(&key("nebensound"))
            .unwrap()
            .into_hash()
            .unwrap();
        job.remove(&key("uses")).unwrap();
        job
    };
    assert_eq!(job(&mut gate), job(&mut readme));
    assert_eq!(gate, readme);
}

#[test]
fn the_documents_quote_the_readme_caller() {
    let readme = readme_caller();
    let phases = blocks("docs/phases.md", "## Pull request and push");
    assert_eq!(phases[0]["on"], readme["on"]);
    let pinning = blocks("docs/calling.md", "## Pinning");
    assert_eq!(
        pinning[0]["jobs"]["nebensound"]["permissions"],
        readme["jobs"]["nebensound"]["permissions"]
    );
    let triggers = blocks("docs/calling.md", "## When it runs");
    assert_eq!(triggers[0]["on"], readme["on"]);
}
