//! The plan: which unit runs which check, what it builds, and how that reads
//! as JSON.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound::plan::{self, Plan};
use support::{QUIET, analysis, call, central, repository};

fn planned(source: &str, paths: &[&str]) -> Plan {
    plan::plan(&analysis(source, paths), &central())
}

/// The checks a unit starts, by the name the JSON gives it.
fn checks(plan: &Plan, name: Option<&str>) -> Vec<String> {
    let wanted = name.map_or_else(
        || "\"name\":null".to_owned(),
        |name| format!("\"name\":\"{name}\""),
    );
    let unit = plan
        .units
        .iter()
        .find(|unit| {
            Plan {
                units: vec![(*unit).clone()],
            }
            .to_json()
            .contains(&wanted)
        })
        .expect("the unit is in the plan");
    unit.checks.iter().map(ToString::to_string).collect()
}

/// The build a unit starts, by its component name.
fn build(plan: &Plan, name: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(&plan.to_json()).expect("valid JSON");
    let unit = json["units"]
        .as_array()
        .expect("a list of units")
        .iter()
        .find(|unit| unit["name"] == name)
        .expect("the unit is in the plan");
    unit["build"].as_str().map(ToOwned::to_owned)
}

#[test]
fn the_base_set_runs_at_the_repository() {
    let plan = planned(QUIET, &["README.md"]);

    assert_eq!(plan.units.len(), 1);
    assert_eq!(checks(&plan, None), ["json", "markdown", "toml", "yaml"]);
}

#[test]
fn a_component_runs_what_it_declares_and_not_the_base_set_again() {
    let plan = planned(
        "[repo]\npipeline = \"none\"\n\n[component.tool]\npath = \"tool\"\npipeline = { check = \
         [\"rust\", \"markdown\"] }\nversioning = \"none\"\n",
        &["README.md", "tool/src/main.rs", "tool/Cargo.toml"],
    );

    assert_eq!(checks(&plan, None), ["json", "markdown", "toml", "yaml"]);
    assert_eq!(checks(&plan, Some("tool")), ["rust"]);
}

#[test]
fn a_check_runs_once_at_the_outermost_unit_that_has_it() {
    let plan = planned(
        "[repo]\npipeline = \"none\"\n\n[component.outer]\npath = \"a\"\npipeline = { check = \
         [\"rust\"] }\nversioning = \"none\"\n\n[component.inner]\npath = \"a/b\"\npipeline = { \
         check = [\"rust\", \"shell\"] }\nversioning = \"none\"\n\n[component.beside]\npath = \
         \"c\"\npipeline = { check = [\"rust\"] }\nversioning = \"none\"\n",
        &["a/x.rs", "a/b/y.rs", "a/b/z.sh", "c/w.rs"],
    );

    assert_eq!(checks(&plan, Some("outer")), ["rust"]);
    assert_eq!(checks(&plan, Some("inner")), ["shell"]);
    // A sibling is contained by neither, so it keeps its own run.
    assert_eq!(checks(&plan, Some("beside")), ["rust"]);
}

#[test]
fn two_units_at_one_path_run_a_check_once_and_not_never() {
    let plan = planned(
        "[repo]\npipeline = \"none\"\n\n[component.first]\npath = \"a\"\npipeline = { check = \
         [\"rust\"] }\nversioning = \"none\"\n\n[component.second]\npath = \"a\"\npipeline = { \
         check = [\"rust\"] }\nversioning = \"none\"\n",
        &["a/x.rs"],
    );

    let runs: usize = ["first", "second"]
        .iter()
        .map(|name| checks(&plan, Some(name)).len())
        .sum();
    assert_eq!(runs, 1, "{}", plan.to_json());
    // Components are resolved in name order, so `first` comes first and keeps
    // the check; the sum alone would let it move to `second` unnoticed.
    assert_eq!(checks(&plan, Some("first")), ["rust"], "{}", plan.to_json());
    assert!(
        checks(&plan, Some("second")).is_empty(),
        "{}",
        plan.to_json()
    );
}

#[test]
fn a_generated_unit_is_in_the_plan_and_starts_nothing_of_its_own() {
    let plan = planned(
        "[repo]\npipeline = \"none\"\n\n[component.docs]\npath = \"doc\"\npipeline = { check = \
         [\"typst\"] }\nversioning = \"child-files\"\n",
        &["doc/a.typ"],
    );

    assert_eq!(checks(&plan, Some("docs")), ["typst"]);
    assert!(checks(&plan, Some("a")).is_empty(), "{}", plan.to_json());
}

#[test]
fn a_unit_with_a_version_starts_the_build_its_pipeline_names() {
    let plan = planned(
        "[repo]\npipeline = \"none\"\n\n[component.lib]\npath = \"lib\"\npipeline = \
         \"rust-library-package\"\n",
        &["lib/src/lib.rs", "lib/Cargo.toml"],
    );

    assert_eq!(build(&plan, "lib").as_deref(), Some("cargo-package"));
    assert_eq!(
        plan.units[0].build, None,
        "the repository builds nothing here"
    );
}

#[test]
fn a_unit_without_a_version_of_its_own_builds_nothing() {
    let plan = planned(
        "[repo]\npipeline = \"none\"\n\n[component.tool]\npath = \"tool\"\npipeline = { \
         build = \"cargo-release\" }\nversioning = \"none\"\n",
        &["tool/src/main.rs"],
    );

    assert_eq!(build(&plan, "tool"), None, "{}", plan.to_json());
}

#[test]
fn generated_units_build_and_the_unit_that_generated_them_does_not() {
    let plan = planned(
        "[repo]\npipeline = \"none\"\n\n[component.docs]\npath = \"doc\"\npipeline = { \
         check = [\"typst\"], build = \"typst\" }\nversioning = \"child-files\"\n",
        &["doc/a.typ", "doc/b.typ"],
    );

    assert_eq!(build(&plan, "docs"), None, "{}", plan.to_json());
    assert_eq!(build(&plan, "a").as_deref(), Some("typst"));
    assert_eq!(build(&plan, "b").as_deref(), Some("typst"));
}

#[test]
fn a_unit_with_a_version_beside_its_children_leaves_the_building_to_them() {
    let plan = planned(
        "[repo]\npipeline = \"none\"\n\n[component.docs]\npath = \"doc\"\npipeline = { \
         check = [\"typst\"], build = \"typst\" }\nversioning = \"self-and-child-files\"\n",
        &["doc/a.typ", "doc/b.typ"],
    );

    assert!(plan.units.iter().any(|unit| unit.path.to_string() == "doc"));
    assert_eq!(build(&plan, "docs"), None, "{}", plan.to_json());
    assert_eq!(build(&plan, "a").as_deref(), Some("typst"));
    assert_eq!(build(&plan, "b").as_deref(), Some("typst"));
}

#[test]
fn the_json_carries_the_units_and_a_flat_list_for_a_matrix() {
    let plan = planned(
        "[repo]\npipeline = \"none\"\n\n[component.tool]\npath = \"tool\"\npipeline = { check = \
         [\"rust\"] }\nversioning = \"none\"\n",
        &["tool/src/main.rs"],
    );

    let json: serde_json::Value = serde_json::from_str(&plan.to_json()).expect("valid JSON");
    assert_eq!(
        json,
        serde_json::json!({
            "units": [
                {"kind": "repository", "name": null, "path": ".",
                 "checks": ["json", "markdown", "toml", "yaml"], "build": null},
                {"kind": "component", "name": "tool", "path": "tool", "checks": ["rust"],
                 "build": null}
            ],
            "checks": [
                {"kind": "repository", "name": null, "path": ".", "check": "json"},
                {"kind": "repository", "name": null, "path": ".", "check": "markdown"},
                {"kind": "repository", "name": null, "path": ".", "check": "toml"},
                {"kind": "repository", "name": null, "path": ".", "check": "yaml"},
                {"kind": "component", "name": "tool", "path": "tool", "check": "rust"}
            ]
        })
    );
}

#[test]
fn plan_prints_one_line_of_json_and_nothing_else() {
    let dir = repository(&[(".nebensound/repo.toml", QUIET), ("README.md", "# x\n")]);
    let root = dir.path().to_str().expect("a path in UTF-8");
    let (text, code) = call(&["nebensound", "-C", root, "plan"]);

    assert_eq!(code, 0, "{text}");
    assert!(text.starts_with('{'), "{text}");
    let _: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert!(text.ends_with("}\n"), "{text}");
    assert_eq!(text.lines().count(), 1, "{text}");
}

#[test]
fn a_repository_with_errors_gets_no_plan() {
    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("README.md", "# x\n"),
        ("src/main.rs", "fn main() {}\n"),
    ]);
    let root = dir.path().to_str().expect("a path in UTF-8");
    let (text, code) = call(&["nebensound", "-C", root, "plan"]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("there is no plan"), "{text}");
    assert!(!text.contains('{'), "{text}");
}
