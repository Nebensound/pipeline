//! Reading an `action.yml`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use xtask::action::Action;

const FULL: &str = r"
name: demo
inputs:
  given:
    description: An input without a default.
    required: true
  Plan:
    default: 24
  1: {}
  1.5: {}
  true: {}
outputs:
  plan:
    value: ${{ steps.plan.outputs.plan }}
runs:
  using: composite
  steps:
    - shell: bash
      run: echo first
";

#[test]
fn the_inputs_of_an_action_are_read_in_order() {
    let action = Action::parse(FULL, "demo").unwrap();
    assert_eq!(action.inputs, ["given", "Plan", "1", "1.5", "true"]);
    // GitHub reads input names without regard to case.
    assert!(action.takes("plan"));
    assert!(action.takes("GIVEN"));
    assert!(!action.takes("working-directory"));
}

#[test]
fn an_action_without_inputs_takes_nothing() {
    for yaml in [
        "runs:\n  using: node24\n  main: index.js\n",
        "inputs:\nruns:\n  using: node24\n",
    ] {
        let action = Action::parse(yaml, "demo").unwrap();
        assert!(action.inputs.is_empty(), "{yaml}");
        assert!(!action.takes("plan"), "{yaml}");
    }
}

#[test]
fn an_action_is_loaded_from_either_file_name() {
    let scene = support::Scene::new();
    support::write(
        scene.path(),
        &[
            ("yml/action.yml", "inputs:\n  short: {}\n"),
            ("yaml/action.yaml", "inputs:\n  long: {}\n"),
        ],
    );
    let short = Action::load(&scene.path().join("yml")).unwrap();
    assert_eq!(short.inputs, ["short"]);
    let long = Action::load(&scene.path().join("yaml")).unwrap();
    assert_eq!(long.inputs, ["long"]);
    let missing = Action::load(&scene.path().join("none")).unwrap_err();
    assert!(missing.to_string().contains("cannot read"), "{missing}");
}

/// Every way an action can be beyond what the xtask reads.
#[test]
fn what_cannot_be_read_is_refused_with_its_place() {
    for (yaml, message) in [
        ("key: [unclosed", "demo is no YAML"),
        ("", "demo is empty"),
        (
            "inputs: [1]\nruns:\n  using: node24\n",
            "inputs in demo is no mapping",
        ),
        (
            "inputs:\n  ? - a\n  : {}\nruns:\n  using: node24\n",
            "inputs in demo is no single value",
        ),
    ] {
        let error = Action::parse(yaml, "demo").expect_err(yaml);
        assert!(error.to_string().contains(message), "{yaml}\n{error}");
    }
}
