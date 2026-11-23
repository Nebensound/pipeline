//! The register, the building blocks and `nebensound.yml` say the same thing.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::path::Path;

use support::{Scene, write};
use xtask::Error;
use xtask::cases::{Case, Expect};
use xtask::layout::{self, Directory};

#[test]
fn this_repository_agrees() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let found = layout::check(&root).unwrap();
    assert_eq!(found.problems, Vec::<String>::new());
    assert!(
        found.blocks.contains(&"check-json".to_owned()),
        "{:?}",
        found.blocks
    );
    assert!(
        !found.blocks.contains(&"phase0".to_owned()),
        "{:?}",
        found.blocks
    );
}

const REGISTER: &str = r#"
[check.demo]
extensions = [".demo"]
[check.other]
extensions = [".other"]
[check.unwritten]
extensions = [".none"]
[version.cargo]
copy = "x"
[build.cargo-package]
produces = "crate"
[target."crates.io"]
accepts = ["crate"]
"#;

/// A case of each kind.
fn both() -> Vec<Case> {
    vec![case(Expect::Pass), case(Expect::Fail(vec!["x".to_owned()]))]
}

fn case(expect: Expect) -> Case {
    Case {
        name: "a".to_owned(),
        dir: "a".into(),
        expect,
    }
}

/// A block whose action.yml declares no input `plan`.
fn dir(name: &str, action: bool, cases: Result<Vec<Case>, Error>) -> Directory {
    Directory {
        name: name.to_owned(),
        action,
        plan: Ok(false),
        cases,
    }
}

/// A block with both kinds of cases whose action.yml declares the input
/// `plan`, or cannot be read.
fn planned(name: &str, plan: Result<bool, Error>) -> Directory {
    Directory {
        plan,
        ..dir(name, true, Ok(both()))
    }
}

fn workflow(written: &str, steps: &str) -> String {
    workflow_with(written, "[]", steps)
}

/// The same with a list of written version writers.
fn workflow_with(written: &str, writers: &str, steps: &str) -> String {
    workflow_all(written, writers, "[]", steps)
}

/// The same with a list of written builds.
fn workflow_all(written: &str, writers: &str, builds: &str, steps: &str) -> String {
    workflow_jobs(written, writers, builds, steps, "")
}

/// The same with the steps of the build job, below its `Not written yet`.
fn workflow_jobs(
    written: &str,
    writers: &str,
    builds: &str,
    steps: &str,
    building: &str,
) -> String {
    workflow_targets(written, writers, builds, "[]", steps, building)
}

/// The same with a list of written publish targets.
fn workflow_targets(
    written: &str,
    writers: &str,
    builds: &str,
    targets: &str,
    steps: &str,
    building: &str,
) -> String {
    format!(
        "jobs:\n  declare:\n    steps:\n      - uses: $/.github/actions/tool\n      - uses: actions/checkout@v7\n      - run: echo\n  units:\n    strategy:\n      matrix:\n        include: ${{{{ fromJSON(needs.declare.outputs.units) }}}}\n    steps:\n      - name: Not written yet\n        env:\n          WRITTEN_CHECKS: '{written}'\n{}      - name: Version writers not written yet\n        env:\n          WRITTEN: '{writers}'\n      - name: Publish targets not written yet\n        env:\n          WRITTEN: '{targets}'\n  builds:\n    strategy:\n      matrix:\n        include: ${{{{ fromJSON(needs.declare.outputs.builds) }}}}\n    steps:\n      - name: Not written yet\n        env:\n          WRITTEN_BUILDS: '{builds}'\n{}",
        support::indent(steps, 6),
        support::indent(building, 6)
    )
}

/// What a step that hands a check or build block other keys or inputs is
/// told, when the block declares no input `plan`.
const CHECK_FORM: &str = "other keys than `if`, `name`, `uses`, `with`, or that hands it other than `working-directory: ${{ matrix.path }}`";

/// The same when the block declares the input `plan`.
const PLANNED_FORM: &str = "other keys than `if`, `name`, `uses`, `with`, or that hands it other than `plan: ${{ needs.declare.outputs.plan }}` and `working-directory: ${{ matrix.path }}`";

/// The same when the block's action.yml cannot be read.
const EITHER_FORM: &str = "other keys than `if`, `name`, `uses`, `with`, or that hands it other than `working-directory: ${{ matrix.path }}`, or `plan: ${{ needs.declare.outputs.plan }}` and `working-directory: ${{ matrix.path }}`";

/// The line under `with` of a step that hands its block the plan of phase 0.
const WITH_PLAN: &str = "    plan: ${{ needs.declare.outputs.plan }}\n";

const AGREEING_STEPS: &str = "- name: demo\n  if: ${{ !cancelled() && contains(matrix.checks, 'demo') }}\n  uses: $/.github/actions/check-demo\n  with:\n    working-directory: ${{ matrix.path }}\n- uses: $/.github/actions/phase0\n  id: phase0\n";

#[test]
fn agreeing_parts_find_nothing() {
    let found = layout::compare(
        REGISTER,
        &workflow(r#"["demo"]"#, AGREEING_STEPS),
        &[
            dir("check-demo", true, Ok(both())),
            dir("phase0", true, Ok(Vec::new())),
            dir("tool", true, Ok(Vec::new())),
        ],
    )
    .unwrap();
    assert_eq!(found.problems, Vec::<String>::new());
    assert_eq!(found.blocks, ["check-demo"]);
}

#[test]
fn a_switch_that_is_inverted_or_always_true_is_named() {
    for condition in [
        "${{ !cancelled() && !contains(matrix.checks, 'demo') }}",
        "${{ !cancelled() && contains(matrix.checks, 'demo') || true }}",
        "${{ contains(matrix.checks, 'demo') }}",
    ] {
        let steps = format!(
            "- name: demo\n  if: \"{condition}\"\n  uses: $/.github/actions/check-demo\n  with:\n    working-directory: ${{{{ matrix.path }}}}\n- uses: $/.github/actions/phase0\n"
        );
        let found = layout::compare(
            REGISTER,
            &workflow(r#"["demo"]"#, &steps),
            &[
                dir("check-demo", true, Ok(both())),
                dir("phase0", true, Ok(Vec::new())),
                dir("tool", true, Ok(Vec::new())),
            ],
        )
        .unwrap();
        assert_eq!(
            found.problems,
            [
                "nebensound.yml calls `check-demo` from a step that is not named `demo` and switched on it with `${{ !cancelled() && contains(matrix.checks, 'demo') }}`"
            ],
            "{condition}"
        );
    }
}

#[test]
fn a_step_that_silences_its_check_or_moves_it_is_named() {
    let call = "- name: demo\n  if: ${{ !cancelled() && contains(matrix.checks, 'demo') }}\n  uses: $/.github/actions/check-demo\n";
    for rest in [
        // The job stays green whatever the block finds.
        "  continue-on-error: true\n  with:\n    working-directory: ${{ matrix.path }}\n",
        "  timeout-minutes: 1\n  with:\n    working-directory: ${{ matrix.path }}\n",
        // Every unit is checked at the root instead of at its own path.
        "  with:\n    working-directory: .\n",
        "  with:\n    working-directory: ${{ matrix.path }}\n    extra: x\n",
        "",
        // A key or value that is no text.
        "  1: x\n  with:\n    working-directory: ${{ matrix.path }}\n",
        "  with:\n    working-directory: 1\n",
        "  with: x\n",
        // The plan from anywhere but phase 0, or the plan alone.
        "  with:\n    working-directory: ${{ matrix.path }}\n    plan: '{}'\n",
        "  with:\n    plan: ${{ needs.declare.outputs.plan }}\n",
    ] {
        let found = layout::compare(
            REGISTER,
            &workflow(
                r#"["demo"]"#,
                &format!("{call}{rest}- uses: $/.github/actions/phase0\n"),
            ),
            &[
                dir("check-demo", true, Ok(both())),
                dir("phase0", true, Ok(Vec::new())),
                dir("tool", true, Ok(Vec::new())),
            ],
        )
        .unwrap();
        assert_eq!(
            found.problems,
            [format!(
                "nebensound.yml calls `check-demo` from a step with {CHECK_FORM}"
            )],
            "{rest}"
        );
    }
}

/// The problems of a check step `steps` beside the block `check-demo`.
fn check_problems(steps: &str, block: Directory) -> Vec<String> {
    layout::compare(
        REGISTER,
        &workflow(r#"["demo"]"#, steps),
        &[
            block,
            dir("phase0", true, Ok(Vec::new())),
            dir("tool", true, Ok(Vec::new())),
        ],
    )
    .unwrap()
    .problems
}

#[test]
fn a_check_gets_the_plan_exactly_when_its_block_takes_it() {
    let bare = AGREEING_STEPS;
    let with_plan = AGREEING_STEPS.replacen(
        "    working-directory",
        &format!("{WITH_PLAN}    working-directory"),
        1,
    );
    let elsewhere = AGREEING_STEPS.replacen(
        "    working-directory",
        "    plan: '{}'\n    working-directory",
        1,
    );

    // The block takes the plan: the step hands it over, and only from phase 0.
    assert_eq!(
        check_problems(&with_plan, planned("check-demo", Ok(true))),
        Vec::<String>::new()
    );
    assert_eq!(
        check_problems(bare, planned("check-demo", Ok(true))),
        [
            "the step `demo` of nebensound.yml hands `check-demo` no plan, though its action.yml declares the input `plan`: the step hands it `plan: ${{ needs.declare.outputs.plan }}` as well"
        ]
    );
    assert_eq!(
        check_problems(&elsewhere, planned("check-demo", Ok(true))),
        [format!(
            "nebensound.yml calls `check-demo` from a step with {PLANNED_FORM}"
        )]
    );

    // The block takes no plan: the step hands it none.
    assert_eq!(
        check_problems(&with_plan, planned("check-demo", Ok(false))),
        [
            "the step `demo` of nebensound.yml hands `check-demo` the plan, though its action.yml declares no input `plan`: the step hands it no `plan`"
        ]
    );

    // An action.yml that cannot be read is named, and leaves the plan open.
    for steps in [bare, with_plan.as_str()] {
        assert_eq!(
            check_problems(
                steps,
                planned("check-demo", Err(Error::new("a broken action".to_owned())))
            ),
            ["a broken action"]
        );
    }
    assert_eq!(
        check_problems(
            &elsewhere,
            planned("check-demo", Err(Error::new("a broken action".to_owned())))
        ),
        [
            "a broken action".to_owned(),
            format!("nebensound.yml calls `check-demo` from a step with {EITHER_FORM}")
        ]
    );
}

/// The step that calls the version writer `cargo`, as `nebensound.yml`
/// documents it.
const WRITER_STEP: &str = "- name: cargo\n  if: ${{ contains(matrix.writers, 'cargo') }}\n  uses: $/.github/actions/version-cargo\n  with:\n    plan: ${{ needs.declare.outputs.plan }}\n";

/// The step that calls the build `cargo-package`, as `nebensound.yml`
/// documents it.
const BUILD_STEP: &str = "- name: cargo-package\n  if: ${{ matrix.build == 'cargo-package' }}\n  uses: $/.github/actions/build-cargo-package\n  with:\n    working-directory: ${{ matrix.path }}\n";

#[test]
fn a_version_writer_is_held_to_its_list_and_its_cases() {
    let blocks = |cases: Vec<Case>| {
        vec![
            dir("check-demo", true, Ok(both())),
            dir("phase0", true, Ok(Vec::new())),
            dir("tool", true, Ok(Vec::new())),
            dir("version-cargo", true, Ok(cases)),
        ]
    };
    let found = layout::compare(
        REGISTER,
        &workflow_jobs(
            r#"["demo"]"#,
            r#"["cargo"]"#,
            "[]",
            AGREEING_STEPS,
            WRITER_STEP,
        ),
        &blocks(both()),
    )
    .unwrap();
    assert_eq!(found.problems, Vec::<String>::new());
    assert_eq!(found.blocks, ["check-demo", "version-cargo"]);

    let found = layout::compare(
        REGISTER,
        &workflow_with(r#"["demo"]"#, "[]", AGREEING_STEPS),
        &blocks(vec![case(Expect::Pass)]),
    )
    .unwrap();
    assert_eq!(
        found.problems,
        [
            "`version-cargo` is never called from nebensound.yml",
            "`version-cargo` exists, but `Version writers not written yet` still fails for `cargo`",
            "`version-cargo` has no failing test case under tests/fail/",
        ]
    );

    let found = layout::compare(
        REGISTER,
        &workflow_jobs(
            r#"["demo"]"#,
            r#"["cargo", "ghost"]"#,
            "[]",
            AGREEING_STEPS,
            WRITER_STEP,
        ),
        &blocks(both()),
    )
    .unwrap();
    assert_eq!(
        found.problems,
        ["`Version writers not written yet` counts `ghost` as written, but it has no block"]
    );
}

#[test]
fn a_version_writer_is_called_only_in_its_form() {
    for step in [
        // Switched on the build, or with the check's own condition.
        WRITER_STEP.replace(
            "contains(matrix.writers, 'cargo')",
            "matrix.build == 'cargo'",
        ),
        WRITER_STEP.replace("${{ contains", "${{ !cancelled() && contains"),
        WRITER_STEP.replace("name: cargo", "name: write"),
    ] {
        let found = layout::compare(
            REGISTER,
            &workflow_jobs(r#"["demo"]"#, r#"["cargo"]"#, "[]", AGREEING_STEPS, &step),
            &[
                dir("check-demo", true, Ok(both())),
                dir("phase0", true, Ok(Vec::new())),
                dir("tool", true, Ok(Vec::new())),
                dir("version-cargo", true, Ok(both())),
            ],
        )
        .unwrap();
        assert_eq!(
            found.problems,
            [
                "nebensound.yml calls `version-cargo` from a step that is not named `cargo` and switched on it with `${{ contains(matrix.writers, 'cargo') }}`"
            ],
            "{step}"
        );
    }
    for step in [
        // The path instead of the plan, or both.
        WRITER_STEP.replace(
            "plan: ${{ needs.declare.outputs.plan }}",
            "working-directory: ${{ matrix.path }}",
        ),
        format!("{WRITER_STEP}    working-directory: ${{{{ matrix.path }}}}\n"),
        format!("{WRITER_STEP}  continue-on-error: true\n"),
    ] {
        let found = layout::compare(
            REGISTER,
            &workflow_jobs(r#"["demo"]"#, r#"["cargo"]"#, "[]", AGREEING_STEPS, &step),
            &[
                dir("check-demo", true, Ok(both())),
                dir("phase0", true, Ok(Vec::new())),
                dir("tool", true, Ok(Vec::new())),
                dir("version-cargo", true, Ok(both())),
            ],
        )
        .unwrap();
        assert_eq!(
            found.problems,
            [
                "nebensound.yml calls `version-cargo` from a step with other keys than `if`, `name`, `uses`, `with`, or that hands it other than `plan: ${{ needs.declare.outputs.plan }}`"
            ],
            "{step}"
        );
    }
}

#[test]
fn a_build_is_held_to_its_list_its_form_and_its_cases() {
    let blocks = |cases: Vec<Case>| {
        vec![
            dir("build-cargo-package", true, Ok(cases)),
            dir("check-demo", true, Ok(both())),
            dir("phase0", true, Ok(Vec::new())),
            dir("tool", true, Ok(Vec::new())),
        ]
    };
    let found = layout::compare(
        REGISTER,
        &workflow_jobs(
            r#"["demo"]"#,
            "[]",
            r#"["cargo-package"]"#,
            AGREEING_STEPS,
            BUILD_STEP,
        ),
        &blocks(both()),
    )
    .unwrap();
    assert_eq!(found.problems, Vec::<String>::new());
    assert_eq!(found.blocks, ["build-cargo-package", "check-demo"]);

    let found = layout::compare(
        REGISTER,
        &workflow_all(r#"["demo"]"#, "[]", r#"["ghost"]"#, AGREEING_STEPS),
        &blocks(vec![case(Expect::Fail(vec!["x".to_owned()]))]),
    )
    .unwrap();
    assert_eq!(
        found.problems,
        [
            "`build-cargo-package` is never called from nebensound.yml",
            "`build-cargo-package` exists, but `Not written yet` still fails for `cargo-package`",
            "`build-cargo-package` has no passing test case under tests/pass/",
            "`Not written yet` counts the build `ghost` as written, but it has no block",
        ]
    );

    let form = format!("a step with {CHECK_FORM}");
    for (step, problem) in [
        // A build that runs after a step that failed, or switched on another.
        (
            BUILD_STEP.replace("${{ matrix.build", "${{ !cancelled() && matrix.build"),
            "a step that is not named `cargo-package` and switched on it with `${{ matrix.build == 'cargo-package' }}`",
        ),
        (
            BUILD_STEP.replace("== 'cargo-package'", "!= ''"),
            "a step that is not named `cargo-package` and switched on it with `${{ matrix.build == 'cargo-package' }}`",
        ),
        // The plan alone, the plan from anywhere but phase 0, or the root
        // instead of the unit.
        (
            BUILD_STEP.replace(
                "working-directory: ${{ matrix.path }}",
                "plan: ${{ needs.declare.outputs.plan }}",
            ),
            form.as_str(),
        ),
        (format!("{BUILD_STEP}    plan: '{{}}'\n"), form.as_str()),
        (BUILD_STEP.replace("${{ matrix.path }}", "."), form.as_str()),
    ] {
        let found = layout::compare(
            REGISTER,
            &workflow_jobs(
                r#"["demo"]"#,
                "[]",
                r#"["cargo-package"]"#,
                AGREEING_STEPS,
                &step,
            ),
            &blocks(both()),
        )
        .unwrap();
        assert_eq!(
            found.problems,
            [format!(
                "nebensound.yml calls `build-cargo-package` from {problem}"
            )],
            "{step}"
        );
    }
}

#[test]
fn a_build_gets_the_plan_exactly_when_its_block_takes_it() {
    let problems = |step: &str, plan: bool| {
        layout::compare(
            REGISTER,
            &workflow_jobs(
                r#"["demo"]"#,
                "[]",
                r#"["cargo-package"]"#,
                AGREEING_STEPS,
                step,
            ),
            &[
                planned("build-cargo-package", Ok(plan)),
                dir("check-demo", true, Ok(both())),
                dir("phase0", true, Ok(Vec::new())),
                dir("tool", true, Ok(Vec::new())),
            ],
        )
        .unwrap()
        .problems
    };
    let with_plan = format!("{BUILD_STEP}{WITH_PLAN}");
    assert_eq!(problems(&with_plan, true), Vec::<String>::new());
    assert_eq!(
        problems(BUILD_STEP, true),
        [
            "the step `cargo-package` of nebensound.yml hands `build-cargo-package` no plan, though its action.yml declares the input `plan`: the step hands it `plan: ${{ needs.declare.outputs.plan }}` as well"
        ]
    );
    assert_eq!(
        problems(&with_plan, false),
        [
            "the step `cargo-package` of nebensound.yml hands `build-cargo-package` the plan, though its action.yml declares no input `plan`: the step hands it no `plan`"
        ]
    );
    // A step with another key is told the whole form, plan or not.
    assert_eq!(
        problems(&format!("{with_plan}  timeout-minutes: 1\n"), true),
        [format!(
            "nebensound.yml calls `build-cargo-package` from a step with {PLANNED_FORM}"
        )]
    );
    assert_eq!(
        problems(&format!("{BUILD_STEP}  timeout-minutes: 1\n"), true),
        [format!(
            "nebensound.yml calls `build-cargo-package` from a step with {PLANNED_FORM}"
        )]
    );
}

/// The blocks of a check, a version writer and a build, each with its cases.
fn every_kind() -> Vec<Directory> {
    vec![
        dir("build-cargo-package", true, Ok(both())),
        dir("check-demo", true, Ok(both())),
        dir("phase0", true, Ok(Vec::new())),
        dir("tool", true, Ok(Vec::new())),
        dir("version-cargo", true, Ok(both())),
    ]
}

#[test]
fn a_writer_and_a_build_in_their_job_and_order_find_nothing() {
    let found = layout::compare(
        REGISTER,
        &workflow_jobs(
            r#"["demo"]"#,
            r#"["cargo"]"#,
            r#"["cargo-package"]"#,
            AGREEING_STEPS,
            &format!("{WRITER_STEP}{BUILD_STEP}"),
        ),
        &every_kind(),
    )
    .unwrap();
    assert_eq!(found.problems, Vec::<String>::new());
}

#[test]
fn a_version_writer_below_a_build_is_named() {
    // The build would package the manifest Git holds, not the version of the
    // run.
    let found = layout::compare(
        REGISTER,
        &workflow_jobs(
            r#"["demo"]"#,
            r#"["cargo"]"#,
            r#"["cargo-package"]"#,
            AGREEING_STEPS,
            &format!("{BUILD_STEP}{WRITER_STEP}"),
        ),
        &every_kind(),
    )
    .unwrap();
    assert_eq!(
        found.problems,
        [
            "nebensound.yml calls `version-cargo` below `build-cargo-package` in the job `builds`: every version writer stands before every build"
        ]
    );
}

#[test]
fn a_block_called_from_the_job_of_another_kind_is_named() {
    let units = "${{ fromJSON(needs.declare.outputs.units) }}";
    let builds = "${{ fromJSON(needs.declare.outputs.builds) }}";
    for (checks, building, problems) in [
        // A copy of the build in the check job: it would build in every check
        // job, before Check passed and without any writer. Above the writer,
        // but in another job, so no order is broken.
        (
            format!("{AGREEING_STEPS}{BUILD_STEP}"),
            format!("{WRITER_STEP}{BUILD_STEP}"),
            vec![format!(
                "nebensound.yml calls `build-cargo-package` from a job whose matrix is not `{builds}`"
            )],
        ),
        // A version writer in the check job only.
        (
            format!("{AGREEING_STEPS}{WRITER_STEP}"),
            BUILD_STEP.to_owned(),
            vec![format!(
                "nebensound.yml calls `version-cargo` from a job whose matrix is not `{builds}`"
            )],
        ),
        // A check in the build job.
        (
            "- uses: $/.github/actions/phase0\n".to_owned(),
            format!(
                "{}{WRITER_STEP}{BUILD_STEP}",
                AGREEING_STEPS.replace("- uses: $/.github/actions/phase0\n  id: phase0\n", "")
            ),
            vec![format!(
                "nebensound.yml calls `check-demo` from a job whose matrix is not `{units}`"
            )],
        ),
    ] {
        let found = layout::compare(
            REGISTER,
            &workflow_jobs(
                r#"["demo"]"#,
                r#"["cargo"]"#,
                r#"["cargo-package"]"#,
                &checks,
                &building,
            ),
            &every_kind(),
        )
        .unwrap();
        assert_eq!(found.problems, problems, "{checks}\n{building}");
    }

    // A job without a matrix, or one that is no text.
    for matrix in ["", "    strategy:\n      matrix:\n        include: [1]\n"] {
        let workflow = format!(
            "jobs:\n  units:\n    strategy:\n      matrix:\n        include: {units}\n    steps:\n      - name: Not written yet\n        env:\n          WRITTEN_CHECKS: '[]'\n      - name: Version writers not written yet\n        env:\n          WRITTEN: '[\"cargo\"]'\n      - name: Publish targets not written yet\n        env:\n          WRITTEN: '[]'\n  builds:\n{matrix}    steps:\n      - name: Not written yet\n        env:\n          WRITTEN_BUILDS: '[]'\n{}",
            support::indent(WRITER_STEP, 6)
        );
        let found = layout::compare(
            REGISTER,
            &workflow,
            &[dir("version-cargo", true, Ok(both()))],
        )
        .unwrap();
        assert_eq!(
            found.problems,
            [
                format!(
                    "nebensound.yml lists the written builds in `Not written yet` of a job whose matrix is not `{builds}`: the units that run the builds would not fail for one without a block"
                ),
                format!(
                    "nebensound.yml calls `version-cargo` from a job whose matrix is not `{builds}`"
                ),
            ],
            "{matrix}"
        );
    }
}

#[test]
fn a_list_of_written_blocks_outside_the_job_of_its_kind_is_named() {
    let agreeing = workflow_jobs(
        r#"["demo"]"#,
        r#"["cargo"]"#,
        r#"["cargo-package"]"#,
        AGREEING_STEPS,
        &format!("{WRITER_STEP}{BUILD_STEP}"),
    );
    let step = "      - name: Not written yet\n        env:\n";
    let checks = "          WRITTEN_CHECKS: '[\"demo\"]'\n";
    let builds = "          WRITTEN_BUILDS: '[\"cargo-package\"]'\n";
    for (workflow, what, job) in [
        // The written builds in the step of the check job, and the build job
        // without its own: a build without a block would pass there.
        (
            agreeing
                .replace(&format!("{step}{builds}"), "")
                .replace(checks, &format!("{checks}{builds}")),
            "builds",
            "${{ fromJSON(needs.declare.outputs.builds) }}",
        ),
        // The written checks in the step of the build job, and the check job
        // without its own.
        (
            agreeing
                .replace(&format!("{step}{checks}"), "")
                .replace(builds, &format!("{checks}{builds}")),
            "checks",
            "${{ fromJSON(needs.declare.outputs.units) }}",
        ),
    ] {
        assert_ne!(workflow, agreeing);
        let found = layout::compare(REGISTER, &workflow, &every_kind()).unwrap();
        assert_eq!(
            found.problems,
            [format!(
                "nebensound.yml lists the written {what} in `Not written yet` of a job whose matrix is not `{job}`: the units that run the {what} would not fail for one without a block"
            )],
            "{workflow}"
        );
    }
}

#[test]
fn every_disagreement_is_named() {
    let steps = "- name: wrong\n  if: ${{ contains(matrix.checks, 'demo') }}\n  uses: $/.github/actions/check-demo\n  with:\n    working-directory: ${{ matrix.path }}\n- name: other\n  if: always()\n  uses: $/.github/actions/check-other\n- uses: $/.github/actions/check-gone\n";
    let found = layout::compare(
        REGISTER,
        &workflow(r#"["demo", "ghost"]"#, steps),
        &[
            dir("check-cases", true, Ok(both())),
            dir("check-demo", true, Ok(vec![case(Expect::Pass)])),
            dir(
                "check-other",
                false,
                Ok(vec![case(Expect::Fail(vec!["x".to_owned()]))]),
            ),
            dir("check-preset", true, Ok(both())),
            dir(
                "check-unwritten",
                true,
                Err(Error::new("a broken case".to_owned())),
            ),
            dir("phase0", true, Ok(Vec::new())),
            dir("publish-crates-io", true, Ok(both())),
            dir("tool", true, Ok(Vec::new())),
        ],
    )
    .unwrap();
    assert_eq!(
        found.problems,
        [
            "`check-cases` belongs to no register entry, and it is no machinery of the workflow (phase0, tool)",
            "nebensound.yml calls `check-demo` from a step that is not named `demo` and switched on it with `${{ !cancelled() && contains(matrix.checks, 'demo') }}`",
            "`check-demo` has no failing test case under tests/fail/",
            "`check-other` has no action.yml",
            "nebensound.yml calls `check-other` from a step that is not named `other` and switched on it with `${{ !cancelled() && contains(matrix.checks, 'other') }}`",
            &format!("nebensound.yml calls `check-other` from a step with {CHECK_FORM}"),
            "`check-other` exists, but `Not written yet` still fails for `other`",
            "`check-other` has no passing test case under tests/pass/",
            "`check-preset` belongs to no register entry, and it is no machinery of the workflow (phase0, tool)",
            "`check-unwritten` is never called from nebensound.yml",
            "`check-unwritten` exists, but `Not written yet` still fails for `unwritten`",
            "a broken case",
            "`phase0` is never called from nebensound.yml",
            "`publish-crates-io` is a block for the target `crates.io`, and the xtask knows only how a check, a version writer and a build are called and tested; it has to learn the other kind first",
            "nebensound.yml calls `check-gone`, which does not exist",
            "`Not written yet` counts `ghost` as written, but it has no block",
        ]
    );
    assert_eq!(
        found.blocks,
        [
            "check-demo",
            "check-other",
            "check-unwritten",
            "publish-crates-io"
        ]
    );
}

#[test]
fn a_publish_target_counted_as_written_needs_its_block() {
    // No target has a block yet, so the list is empty and agrees.
    let found = layout::compare(
        REGISTER,
        &workflow_targets(r#"["demo"]"#, "[]", "[]", "[]", AGREEING_STEPS, ""),
        &[
            dir("check-demo", true, Ok(both())),
            dir("phase0", true, Ok(Vec::new())),
            dir("tool", true, Ok(Vec::new())),
        ],
    )
    .unwrap();
    assert_eq!(found.problems, Vec::<String>::new());

    // A target counted as written without its block, under the name its block
    // would have: the dot of `crates.io` turned into a hyphen.
    let found = layout::compare(
        REGISTER,
        &workflow_targets(
            r#"["demo"]"#,
            "[]",
            "[]",
            r#"["crates.io"]"#,
            AGREEING_STEPS,
            "",
        ),
        &[
            dir("check-demo", true, Ok(both())),
            dir("phase0", true, Ok(Vec::new())),
            dir("tool", true, Ok(Vec::new())),
        ],
    )
    .unwrap();
    assert_eq!(
        found.problems,
        ["`Publish targets not written yet` counts `crates.io` as written, but it has no block"]
    );
}

#[test]
fn entries_that_share_a_block_name_are_named() {
    let found = layout::compare(
        "[target.\"a.b\"]\naccepts = []\n[target.a-b]\naccepts = []\n",
        &workflow("[]", ""),
        &[dir("tool", true, Ok(Vec::new()))],
    )
    .unwrap();
    assert_eq!(
        found.problems,
        ["the target `a-b` and the target `a.b` would both be the block `publish-a-b`"]
    );
}

#[test]
fn the_lists_of_written_blocks_have_to_be_there_once_and_readable() {
    let twice = "- name: Not written yet\n  env:\n    WRITTEN_CHECKS: '[]'\n";
    let builds_twice = "- name: Not written yet\n  env:\n    WRITTEN_BUILDS: '[]'\n";
    let neither = "jobs:\n  a:\n    steps:\n      - name: Not written yet\n        env:\n          OTHER: '[]'\n";
    let writers_twice = "- name: Version writers not written yet\n  env:\n    WRITTEN: '[]'\n";
    let targets_twice = "- name: Publish targets not written yet\n  env:\n    WRITTEN: '[]'\n";
    for (workflow, message) in [
        (
            "jobs:\n  a:\n    steps: []\n".to_owned(),
            "has 0 steps named `Not written yet` that list the written checks in `WRITTEN_CHECKS`",
        ),
        (
            "jobs:\n  a:\n    steps: []\n".to_owned(),
            "has 0 steps named `Not written yet` that list the written builds in `WRITTEN_BUILDS`",
        ),
        (
            workflow("[]", builds_twice),
            "has 2 steps named `Not written yet` that list the written builds",
        ),
        (
            neither.to_owned(),
            "has 0 steps named `Not written yet` that list the written checks",
        ),
        (
            "jobs:\n  a:\n    steps: []\n".to_owned(),
            "has 0 steps named `Version writers not written yet` that list the written version writers in `WRITTEN`",
        ),
        (
            workflow("[]", twice),
            "has 2 steps named `Not written yet` that list the written checks",
        ),
        (
            workflow("[]", writers_twice),
            "has 2 steps named `Version writers not written yet`",
        ),
        (
            "jobs:\n  a:\n    steps: []\n".to_owned(),
            "has 0 steps named `Publish targets not written yet` that list the written publish targets in `WRITTEN`",
        ),
        (
            workflow("[]", targets_twice),
            "has 2 steps named `Publish targets not written yet`",
        ),
        (
            workflow_targets("[]", "[]", "[]", "not json", "", ""),
            "the list of written publish targets in `Publish targets not written yet` is no JSON list of names",
        ),
        (
            workflow("not json", ""),
            "the list of written checks in `Not written yet` is no JSON list of names",
        ),
        (
            workflow_with("[]", "not json", ""),
            "the list of written version writers in `Version writers not written yet` is no JSON list of names",
        ),
        (
            workflow_all("[]", "[]", "not json", ""),
            "the list of written builds in `Not written yet` is no JSON list of names",
        ),
    ] {
        let found = layout::compare(REGISTER, &workflow, &[]).unwrap();
        assert!(
            found
                .problems
                .iter()
                .any(|problem| problem.contains(message)),
            "{workflow}\n{:?}",
            found.problems
        );
    }
}

#[test]
fn parts_that_cannot_be_read_are_errors() {
    for (register, workflow, message) in [
        ("not = = toml", "jobs: {}\n", "the register is no TOML"),
        (REGISTER, "jobs: [unclosed", "nebensound.yml is no YAML"),
        (REGISTER, "name: no jobs\n", "nebensound.yml has no jobs"),
        (REGISTER, "", "nebensound.yml has no jobs"),
    ] {
        let error = layout::compare(register, workflow, &[]).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }

    let scene = Scene::new();
    let root = scene.path();
    let missing = |root: &Path| layout::check(root).unwrap_err().to_string();
    assert!(missing(root).contains("register.toml"));
    write(root, &[(layout::REGISTER, REGISTER)]);
    assert!(missing(root).contains("nebensound.yml"));
    write(root, &[(layout::WORKFLOW, &workflow("[]", ""))]);
    assert!(missing(root).contains("cannot list"));
}

#[test]
fn every_directory_is_listed_with_its_cases() {
    let scene = Scene::new();
    let actions = scene.path().join("actions");
    write(
        &actions,
        &[
            ("check-b/action.yml", ""),
            ("check-b/tests/pass/a/file", ""),
            ("check-a/README.md", ""),
            (
                "check-c/action.yml",
                "inputs:\n  Plan:\n    required: true\nruns:\n  using: composite\n  steps: []\n",
            ),
            (
                "check-d/action.yml",
                "inputs:\n  working-directory:\n    required: true\nruns:\n  using: composite\n  steps: []\n",
            ),
            ("a file", ""),
        ],
    );
    let found = layout::directories(&actions).unwrap();
    let names: Vec<(&str, bool, usize, Result<bool, String>)> = found
        .iter()
        .map(|dir| {
            (
                dir.name.as_str(),
                dir.action,
                dir.cases.as_ref().unwrap().len(),
                dir.plan.as_ref().copied().map_err(ToString::to_string),
            )
        })
        .collect();
    let empty = format!("{} is empty", actions.join("check-b/action.yml").display());
    assert_eq!(
        names,
        [
            ("check-a", false, 0, Ok(false)),
            ("check-b", true, 1, Err(empty)),
            // GitHub reads input names without regard to case.
            ("check-c", true, 0, Ok(true)),
            ("check-d", true, 0, Ok(false)),
        ]
    );
}
