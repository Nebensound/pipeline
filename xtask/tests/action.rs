//! Reading an `action.yml`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use xtask::action::{Action, Kind, Runs, Step};

const FULL: &str = r"
name: demo
inputs:
  given:
    description: An input without a default.
    required: true
  number:
    default: 24
  real:
    default: 1.26
  flag:
    default: false
  empty:
    default:
outputs:
  plan:
    value: ${{ steps.plan.outputs.plan }}
runs:
  using: composite
  steps:
    - name: First
      id: plan
      if: ${{ !cancelled() }}
      shell: bash
      working-directory: ${{ inputs.given }}
      env:
        A: one
        B: 2
      run: |
        echo first
        echo second
    - uses: actions/setup-node@v7
      with:
        node-version: 24
    - shell: bash
      run: echo third
";

#[test]
fn a_composite_action_is_read_whole() {
    let action = Action::parse(FULL, "demo").unwrap();
    let defaults: Vec<(&str, Option<&str>)> = action
        .inputs
        .iter()
        .map(|input| (input.name.as_str(), input.default.as_deref()))
        .collect();
    assert_eq!(
        defaults,
        [
            ("given", None),
            ("number", Some("24")),
            ("real", Some("1.26")),
            ("flag", Some("false")),
            ("empty", Some("")),
        ]
    );
    assert_eq!(
        action.outputs,
        [(
            "plan".to_owned(),
            "${{ steps.plan.outputs.plan }}".to_owned()
        )]
    );
    let Runs::Composite(steps) = &action.runs else {
        panic!("{:?}", action.runs);
    };
    let first = &steps[0];
    assert_eq!(first.name.as_deref(), Some("First"));
    assert_eq!(first.id.as_deref(), Some("plan"));
    assert_eq!(first.condition.as_deref(), Some("${{ !cancelled() }}"));
    assert_eq!(
        first.working_directory.as_deref(),
        Some("${{ inputs.given }}")
    );
    assert_eq!(
        first.env,
        [
            ("A".to_owned(), "one".to_owned()),
            ("B".to_owned(), "2".to_owned())
        ]
    );
    let Kind::Run { script, shell } = &first.kind else {
        panic!("{:?}", first.kind);
    };
    assert_eq!(script, "echo first\necho second\n");
    assert_eq!(shell.as_deref(), Some("bash"));
    let Kind::Uses { action: used, with } = &steps[1].kind else {
        panic!("{:?}", steps[1].kind);
    };
    assert_eq!(used, "actions/setup-node@v7");
    assert_eq!(with, &[("node-version".to_owned(), "24".to_owned())]);

    let titles: Vec<String> = steps.iter().map(Step::title).collect();
    assert_eq!(titles, ["First", "actions/setup-node@v7", "echo third"]);
}

#[test]
fn a_step_without_a_name_or_a_script_line_has_an_empty_title() {
    let action = Action::parse(
        "runs:\n  using: composite\n  steps:\n    - run: ''\n      shell: bash\n",
        "demo",
    )
    .unwrap();
    let Runs::Composite(steps) = &action.runs else {
        panic!("{:?}", action.runs);
    };
    assert_eq!(steps[0].title(), "");
}

#[test]
fn an_action_on_node_is_named_by_what_it_runs_on() {
    let action = Action::parse("runs:\n  using: node24\n  main: index.js\n", "demo").unwrap();
    assert!(matches!(&action.runs, Runs::Other(using) if using == "node24"));
    assert!(action.inputs.is_empty());
}

#[test]
fn an_action_is_loaded_from_either_file_name() {
    let scene = support::Scene::new();
    support::write(
        scene.path(),
        &[
            ("yml/action.yml", "runs:\n  using: node24\n"),
            ("yaml/action.yaml", "runs:\n  using: node20\n"),
        ],
    );
    let short = Action::load(&scene.path().join("yml")).unwrap();
    assert!(matches!(&short.runs, Runs::Other(using) if using == "node24"));
    let long = Action::load(&scene.path().join("yaml")).unwrap();
    assert!(matches!(&long.runs, Runs::Other(using) if using == "node20"));
    let missing = Action::load(&scene.path().join("none")).unwrap_err();
    assert!(missing.to_string().contains("cannot read"), "{missing}");
}

/// Every way an action can be beyond what the xtask reads.
#[test]
fn what_cannot_be_read_is_refused_with_its_place() {
    let steps = |step: &str| {
        format!(
            "runs:\n  using: composite\n  steps:\n{}",
            support::indent(step, 4)
        )
    };
    for (yaml, message) in [
        ("key: [unclosed".to_owned(), "is no YAML"),
        (String::new(), "is empty"),
        (
            "inputs: [1]\nruns:\n  using: node24\n".to_owned(),
            "inputs in demo is no mapping",
        ),
        (
            "inputs:\n  ? - a\n  : {}\nruns:\n  using: node24\n".to_owned(),
            "inputs in demo is no single value",
        ),
        (
            "inputs:\n  a:\n    default: [1]\nruns:\n  using: node24\n".to_owned(),
            "the default of input `a`",
        ),
        (
            "outputs:\n  a:\n    description: none\nruns:\n  using: node24\n".to_owned(),
            "the value of output `a`",
        ),
        (
            "outputs: [1]\nruns:\n  using: node24\n".to_owned(),
            "outputs in demo is no mapping",
        ),
        ("runs: {}\n".to_owned(), "runs.using"),
        (
            "runs:\n  using: composite\n  steps: {}\n".to_owned(),
            "runs.steps in demo is no list",
        ),
        (steps("- just text"), "step 1 in demo is no mapping"),
        (steps("- {}"), "step 1 in demo is empty"),
        (
            steps("- run: a\n  shell: bash\n  continue-on-error: true"),
            "sets `continue-on-error`",
        ),
        (
            steps("- run: a\n  uses: b@c"),
            "exactly one of `run` and `uses`",
        ),
        (steps("- name: nothing"), "exactly one of `run` and `uses`"),
        (steps("- run: [a]"), "`run` of step 1"),
        (steps("- uses: [a]"), "`uses` of step 1"),
        (steps("- run: a\n  shell: [a]"), "`shell` of step 1"),
        (steps("- run: a\n  name: [a]"), "`name` of step 1"),
        (steps("- run: a\n  id: [a]"), "`id` of step 1"),
        (steps("- run: a\n  if: [a]"), "`if` of step 1"),
        (
            steps("- run: a\n  working-directory: [a]"),
            "`working-directory` of step 1",
        ),
        (
            steps("- uses: a@b\n  with: [a]"),
            "`with` of step 1 in demo is no mapping",
        ),
        (
            steps("- uses: a@b\n  with:\n    a: [b]"),
            "`with` of step 1 in demo is no single value",
        ),
        (
            steps("- run: a\n  env: [a]"),
            "`env` of step 1 in demo is no mapping",
        ),
        (
            steps("- run: a\n  env:\n    a: [b]"),
            "`env` of step 1 in demo is no single value",
        ),
    ] {
        let error = Action::parse(&yaml, "demo").expect_err(&yaml);
        assert!(error.to_string().contains(message), "{yaml}\n{error}");
    }
}
