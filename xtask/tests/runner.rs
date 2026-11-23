//! Running actions the way the runner of GitHub Actions runs them.
//!
//! Every script here is plain bash that also runs on bash 3.2.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use support::{Scene, composite, git};
use xtask::runner::{self, Job, Place};

fn ok(result: (Result<runner::Outcome, xtask::Error>, String)) -> (runner::Outcome, String) {
    let (outcome, out) = result;
    (
        outcome.unwrap_or_else(|error| panic!("{error}\n{out}")),
        out,
    )
}

fn error(result: (Result<runner::Outcome, xtask::Error>, String)) -> String {
    let (outcome, out) = result;
    match outcome {
        Ok(outcome) => panic!("it ran: {outcome:?}\n{out}"),
        Err(error) => error.to_string(),
    }
}

/// An executable script.
fn executable(path: &Path, script: &str) {
    support::write(
        path.parent().unwrap(),
        &[(path.file_name().unwrap().to_str().unwrap(), script)],
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn outputs_flow_from_step_to_step_and_out_of_the_action() {
    let _alone = support::alone();
    let scene = Scene::new();
    let (outcome, out) = ok(scene.run(
        r#"
outputs:
  result:
    value: ${{ steps.second.outputs.echoed }}
runs:
  using: composite
  steps:
    - id: first
      shell: bash
      run: |
        echo "value=one" >> "$GITHUB_OUTPUT"
        printf 'lines<<END\na\nb\nEND\n' >> "$GITHUB_OUTPUT"
        echo "::error file=x::not an annotation here"
    - id: second
      shell: bash
      env:
        VALUE: ${{ steps.first.outputs.value }}
        LINES: ${{ steps.FIRST.outputs.LINES }}
      run: |
        echo "echoed=$VALUE/$LINES" | tr '\n' ' ' >> "$GITHUB_OUTPUT"
        echo >> "$GITHUB_OUTPUT"
"#,
        &[],
    ));
    assert!(outcome.success, "{out}");
    assert_eq!(
        outcome.outputs.get("result").map(String::as_str),
        Some("one/a b ")
    );
    assert!(
        outcome
            .log
            .contains(&"::error file=x::not an annotation here".to_owned())
    );
    assert!(
        out.contains("    | ::error file=x::not an annotation here"),
        "{out}"
    );
}

#[test]
fn after_a_failure_only_steps_that_ask_for_status_run() {
    let _alone = support::alone();
    let scene = Scene::new();
    let (outcome, out) = ok(scene.run(
        r"
outputs:
  first:
    value: ${{ steps.failing.outcome }}
  skipped:
    value: ${{ steps.skipped.conclusion }}
runs:
  using: composite
  steps:
    - id: failing
      name: Failing
      shell: bash
      run: exit 3
    - id: skipped
      name: Skipped
      shell: bash
      run: echo never
    - name: Not cancelled
      if: ${{ !cancelled() && steps.failing.outcome == 'failure' }}
      shell: bash
      run: echo still running
    - name: On failure
      if: failure()
      shell: bash
      run: echo after the failure
",
        &[],
    ));
    assert!(!outcome.success, "{out}");
    assert_eq!(outcome.log, ["still running", "after the failure"]);
    assert_eq!(
        outcome.outputs.get("first").map(String::as_str),
        Some("failure")
    );
    assert_eq!(
        outcome.outputs.get("skipped").map(String::as_str),
        Some("skipped")
    );
    assert!(out.contains("  x Failing: failed"), "{out}");
    assert!(out.contains("  - Skipped: skipped"), "{out}");
}

#[test]
fn the_environment_and_the_path_last_for_the_job() {
    let _alone = support::alone();
    let scene = Scene::new();
    let bin = scene.path().join("bin");
    executable(&bin.join("hello"), "#!/bin/sh\necho hello from the path\n");
    let host = scene.host();
    let mut job = Job::default();
    let first = scene.action(
        "first",
        &composite(&format!(
            "echo \"FOO=bar\" >> \"$GITHUB_ENV\"\nprintf 'MULTI<<EOF\\na\\nb\\nEOF\\n' >> \"$GITHUB_ENV\"\necho '{}' >> \"$GITHUB_PATH\"\necho >> \"$GITHUB_PATH\"",
            bin.display()
        )),
    );
    let (outcome, out) = ok(scene.run_on(&host, &mut job, &first, &[], &[]));
    assert!(outcome.success, "{out}");
    assert_eq!(job.path, [bin.display().to_string()]);

    let second = scene.action("second", &composite("hello\necho \"$FOO\" \"$MULTI\""));
    let (outcome, out) = ok(scene.run_on(&host, &mut job, &second, &[], &[]));
    assert!(outcome.success, "{out}");
    assert_eq!(outcome.log, ["hello from the path", "bar a", "b"]);
}

#[test]
fn a_step_sees_what_the_runner_hands_it() {
    let _alone = support::alone();
    let scene = Scene::new();
    let mut host = scene.host();
    host.env.push(("CARGO_PKG_NAME".into(), "xtask".into()));
    host.env.push(("RUSTUP_TOOLCHAIN".into(), "1.0.0".into()));
    host.env.push(("CARGO_TERM_COLOR".into(), "always".into()));
    host.env
        .push(("GITHUB_REPOSITORY".into(), "someone/else".into()));
    host.env = runner::environment(host.env);
    let dir = scene.action(
        "main",
        &composite(
            "echo \"$CI $GITHUB_ACTIONS $RUNNER_OS $RUNNER_ARCH\"\n\
             echo \"$GITHUB_REPOSITORY $GITHUB_REPOSITORY_OWNER $GITHUB_RUN_ID $GITHUB_RUN_NUMBER $GITHUB_RUN_ATTEMPT $GITHUB_SERVER_URL\"\n\
             [ \"$(cd \"$GITHUB_WORKSPACE\" && pwd -P)\" = \"$(pwd -P)\" ] && echo workspace\n\
             [ -d \"$RUNNER_TEMP\" ] && echo temp\n\
             [ -f \"$GITHUB_ACTION_PATH/action.yml\" ] && echo action\n\
             echo \"[${CARGO_PKG_NAME:-}] [${RUSTUP_TOOLCHAIN:-}] [${CARGO_TERM_COLOR:-}]\"\n\
             [ \"$CARGO_INSTALL_ROOT\" = \"$(dirname \"$RUNNER_TEMP\")/installs\" ] && echo installs\n\
             case \"$PATH\" in \"$CARGO_INSTALL_ROOT/bin:\"*) echo installs first;; esac\n\
             echo \"${{ github.workspace }}\" | grep -q workspace && echo github.workspace\n\
             echo \"${{ github.action_path }}\" | grep -q main && echo github.action_path\n\
             echo \"${{ runner.temp }}\" | grep -q temp && echo runner.temp\n\
             echo \"[${{ runner.os }}] [${{ runner.arch }}] [${{ github.token }}] [${{ github.server_url }}] [${{ job.check_run_id }}]\"\n\
             echo \"${{ github.repository }} ${{ github.repository_owner }} ${{ github.run_id }} ${{ github.run_number }} ${{ github.run_attempt }}\"",
        ),
    );
    let (outcome, out) = ok(scene.run_on(&host, &mut Job::default(), &dir, &[], &[]));
    assert!(outcome.success, "{out}");
    assert_eq!(
        outcome.log,
        [
            "true true Linux X64",
            "xtask/case xtask 0 0 1 file:///nowhere",
            "workspace",
            "temp",
            "action",
            "[] [] []",
            "installs",
            "installs first",
            "github.workspace",
            "github.action_path",
            "runner.temp",
            "[Linux] [X64] [] [file:///nowhere] [0]",
            "xtask/case xtask 0 0 1",
        ]
    );
}

#[test]
fn inputs_come_from_the_caller_else_from_their_default() {
    let _alone = support::alone();
    let scene = Scene::new();
    let (outcome, out) = ok(scene.run(
        r#"
inputs:
  given: {}
  defaulted:
    default: ${{ runner.os }}-default
  bare: {}
runs:
  using: composite
  steps:
    - shell: bash
      run: echo "${{ inputs.given }} ${{ inputs.DEFAULTED }} [${{ inputs.bare }}]"
"#,
        &[("Given", "from the caller")],
    ));
    assert!(outcome.success, "{out}");
    assert_eq!(outcome.log, ["from the caller Linux-default []"]);
}

#[test]
fn a_case_is_no_pull_request() {
    let _alone = support::alone();
    // As on a push to `main`: phase 0 reads the pull request's number to
    // build the plan for its merge, and finds none.
    let scene = Scene::new();
    let (outcome, out) = ok(scene.run(
        "runs:\n  using: composite\n  steps:\n    - shell: bash\n      env:\n        NUMBER: ${{ github.event.pull_request.number }}\n      run: echo \"[$NUMBER] [${{ github.event.pull_request.title }}]\"\n",
        &[],
    ));
    assert!(outcome.success, "{out}");
    assert_eq!(outcome.log, ["[] []"]);
}

#[test]
fn the_repository_of_a_case_has_a_name_a_default_branch_and_a_visibility() {
    let _alone = support::alone();
    // Phase 0 hands all three to the tool, which holds the first two against
    // the conventions and the declared audience against the third; a case's
    // are those every repository has, and the visibility that fits the
    // audience every case declares.
    let scene = Scene::new();
    let (outcome, out) = ok(scene.run(
        "runs:\n  using: composite\n  steps:\n    - shell: bash\n      env:\n        NAME: ${{ github.event.repository.name }}\n      run: echo \"$NAME ${{ github.event.repository.default_branch }} ${{ github.event.repository.visibility }}\"\n",
        &[],
    ));
    assert!(outcome.success, "{out}");
    assert_eq!(outcome.log, ["case main public"]);
}

#[test]
fn a_step_runs_in_its_working_directory() {
    let _alone = support::alone();
    let scene = Scene::new();
    fs::create_dir_all(scene.workspace().join("sub/dir")).unwrap();
    let (outcome, out) = ok(scene.run(
        "inputs:\n  where: {}\nruns:\n  using: composite\n  steps:\n    - shell: bash\n      working-directory: ${{ inputs.where }}/dir\n      run: basename \"$PWD\"\n",
        &[("where", "sub")],
    ));
    assert!(outcome.success, "{out}");
    assert_eq!(outcome.log, ["dir"]);
}

#[test]
fn a_named_step_can_be_left_out_and_has_to_exist() {
    let _alone = support::alone();
    let scene = Scene::new();
    let dir = scene.action(
        "main",
        "runs:\n  using: composite\n  steps:\n    - name: Dangerous\n      shell: bash\n      run: exit 1\n    - shell: bash\n      run: echo after\n",
    );
    let host = scene.host();
    let (outcome, out) = ok(scene.run_on(&host, &mut Job::default(), &dir, &[], &["Dangerous"]));
    assert!(outcome.success, "{out}");
    assert_eq!(outcome.log, ["after"]);
    assert!(
        out.contains("  - Dangerous: left out by the xtask"),
        "{out}"
    );

    let message = error(scene.run_on(&host, &mut Job::default(), &dir, &[], &["Renamed"]));
    assert!(message.contains("no such step any more"), "{message}");
}

#[test]
fn what_the_runner_would_not_run_is_refused() {
    let _alone = support::alone();
    let scene = Scene::new();
    for (yaml, inputs, message) in [
        ("runs:\n  using: node24\n", &[][..], "runs on `node24`"),
        (
            "runs:\n  using: composite\n  steps:\n    - run: echo\n",
            &[],
            "of the runner's choice",
        ),
        (
            "runs:\n  using: composite\n  steps:\n    - run: echo\n      shell: sh\n",
            &[],
            "runs on the shell sh",
        ),
        (
            "runs:\n  using: composite\n  steps: []\n",
            &[("unknown", "x")],
            "has no input `unknown`",
        ),
        (
            "inputs:\n  a:\n    default: ${{ x.y }}\nruns:\n  using: composite\n  steps: []\n",
            &[],
            "`x.y`",
        ),
        (
            "runs:\n  using: composite\n  steps:\n    - run: echo\n      shell: bash\n      if: x.y\n",
            &[],
            "`x.y`",
        ),
        (
            "runs:\n  using: composite\n  steps:\n    - run: echo\n      shell: bash\n      env:\n        A: ${{ x.y }}\n",
            &[],
            "`x.y`",
        ),
        (
            "runs:\n  using: composite\n  steps:\n    - run: echo\n      shell: bash\n      working-directory: ${{ x.y }}\n",
            &[],
            "`x.y`",
        ),
        (
            "runs:\n  using: composite\n  steps:\n    - run: echo ${{ x.y }}\n      shell: bash\n",
            &[],
            "`x.y`",
        ),
        (
            "runs:\n  using: composite\n  steps:\n    - run: echo ${{ steps.a }}\n      shell: bash\n",
            &[],
            "names no value",
        ),
        // On GitHub these have a value, so reading them as empty would run a
        // block one way here and another way for every caller.
        (
            "runs:\n  using: composite\n  steps:\n    - run: echo ${{ github.event_name }}\n      shell: bash\n",
            &[],
            "`github.event_name` names no value the xtask knows",
        ),
        (
            "runs:\n  using: composite\n  steps:\n    - run: echo\n      shell: bash\n      if: github.event_name == 'pull_request'\n",
            &[],
            "`github.event_name` names no value the xtask knows",
        ),
        (
            "runs:\n  using: composite\n  steps:\n    - run: echo ${{ github.event.repository.owner }}\n      shell: bash\n",
            &[],
            "`github.event.repository.owner` names no value the xtask knows",
        ),
        (
            "runs:\n  using: composite\n  steps:\n    - run: echo ${{ runner.name }}\n      shell: bash\n",
            &[],
            "`runner.name` names no value the xtask knows",
        ),
        (
            "runs:\n  using: composite\n  steps:\n    - run: echo\n      shell: bash\n      if: job.status == 'success'\n",
            &[],
            "`job.status` names no value the xtask knows",
        ),
        (
            "runs:\n  using: composite\n  steps:\n    - uses: a/b@c\n      with:\n        a: ${{ x.y }}\n",
            &[],
            "`x.y`",
        ),
        (
            "outputs:\n  a:\n    value: ${{ x.y }}\nruns:\n  using: composite\n  steps: []\n",
            &[],
            "`x.y`",
        ),
        (
            "runs:\n  using: composite\n  steps:\n    - shell: bash\n      run: echo garbage >> \"$GITHUB_OUTPUT\"\n",
            &[],
            "GITHUB_OUTPUT",
        ),
        (
            "runs:\n  using: composite\n  steps:\n    - shell: bash\n      run: echo garbage >> \"$GITHUB_ENV\"\n",
            &[],
            "GITHUB_ENV",
        ),
    ] {
        let found = error(scene.run(yaml, inputs));
        assert!(found.contains(message), "{yaml}\n{found}");
    }
}

#[test]
fn an_action_the_xtask_cannot_fetch_is_refused() {
    let _alone = support::alone();
    let scene = Scene::new();
    for (action, message) in [
        ("./local", "is not one the xtask fetches"),
        ("$/.github/actions/x", "is not one the xtask fetches"),
        ("docker://alpine@3", "is not one the xtask fetches"),
        ("owner/repo", "is not one the xtask fetches"),
        ("owner@v1", "names no repository"),
        ("owner/missing@v1", "failed"),
    ] {
        let yaml = format!("runs:\n  using: composite\n  steps:\n    - uses: {action}\n");
        let found = error(scene.run(&yaml, &[]));
        assert!(found.contains(message), "{action}\n{found}");
    }
    let message = error(scene.run_on(
        &scene.host(),
        &mut Job::default(),
        &scene.path().join("nothing"),
        &[],
        &[],
    ));
    assert!(message.contains("cannot read"), "{message}");
}

#[test]
fn a_step_that_cannot_be_prepared_or_started_is_an_error() {
    let _alone = support::alone();
    let scene = Scene::new();
    let dir = scene.action("main", &composite("echo"));
    let file = scene.path().join("a file");
    fs::write(&file, "").unwrap();
    let workspace = scene.workspace();
    let mut out = Vec::new();
    let prepared = scene.host().run(
        &mut Job::default(),
        &dir,
        &BTreeMap::new(),
        Place {
            workspace: &workspace,
            temp: &file,
        },
        &[],
        &mut out,
    );
    assert!(
        prepared
            .unwrap_err()
            .to_string()
            .contains("cannot prepare a step"),
        "a temporary directory that is a file"
    );

    let mut host = scene.host();
    host.bash = OsString::from("/nonexistent/bash");
    let message = error(scene.run_on(&host, &mut Job::default(), &dir, &[], &[]));
    assert!(
        message.contains("cannot start /nonexistent/bash"),
        "{message}"
    );
}

#[test]
fn file_commands_are_read_as_github_writes_them() {
    let _alone = support::alone();
    let read = |text: &str| runner::file_commands(text, "GITHUB_OUTPUT");
    assert_eq!(
        read("a=1\n\nb<<X\nline=one\nline<<two\nX\nc=x<<y\nd<<E=F\nv\nE=F\n").unwrap(),
        [
            ("a".to_owned(), "1".to_owned()),
            ("b".to_owned(), "line=one\nline<<two".to_owned()),
            ("c".to_owned(), "x<<y".to_owned()),
            ("d".to_owned(), "v".to_owned()),
        ]
    );
    assert!(
        read("b<<X\nnever closed\n")
            .unwrap_err()
            .to_string()
            .contains("never closes")
    );
    assert!(
        read("no separator\n")
            .unwrap_err()
            .to_string()
            .contains("neither")
    );
}

#[test]
fn a_step_gets_only_what_a_runner_step_has_too() {
    let _alone = support::alone();
    let vars: Vec<(OsString, OsString)> = [
        "PATH",
        "HOME",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "LANG",
        "LC_ALL",
        "XDG_CACHE_HOME",
        "https_proxy",
        "CARGO",
        "CARGO_MANIFEST_DIR",
        "CARGO_PKG_VERSION",
        "CARGO_TARGET_DIR",
        "CARGO_TERM_COLOR",
        "CARGO_BUILD_TARGET",
        "RUSTFLAGS",
        "RUSTUP_TOOLCHAIN",
        "NODE_OPTIONS",
        "PYTHONPATH",
        "GOFLAGS",
        "TERM",
    ]
    .iter()
    .map(|name| (OsString::from(name), OsString::from("x")))
    .collect();
    let kept: Vec<String> = runner::environment(vars)
        .into_iter()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        kept,
        [
            "PATH",
            "HOME",
            "CARGO_HOME",
            "RUSTUP_HOME",
            "LANG",
            "LC_ALL",
            "XDG_CACHE_HOME",
            "https_proxy"
        ]
    );
}

#[test]
fn outside_cargo_leaves_out_what_would_steer_cargo() {
    let _alone = support::alone();
    let vars: Vec<(OsString, OsString)> = [
        "PATH",
        "HOME",
        "CARGO_HOME",
        "TERM",
        "CARGO",
        "CARGO_MANIFEST_DIR",
        "CARGO_PKG_VERSION",
        "CARGO_BIN_EXE_xtask",
        "CARGO_TARGET_DIR",
        "RUSTUP_TOOLCHAIN",
    ]
    .iter()
    .map(|name| (OsString::from(name), OsString::from("x")))
    .collect();
    let kept: Vec<String> = runner::outside_cargo(vars)
        .into_iter()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    assert_eq!(kept, ["PATH", "HOME", "CARGO_HOME", "TERM"]);
}

#[test]
fn a_cache_is_left_out() {
    let _alone = support::alone();
    let scene = Scene::new();
    let (outcome, out) = ok(scene.run(
        "runs:\n  using: composite\n  steps:\n    - id: cache\n      uses: actions/cache/restore@v6\n    - if: steps.cache.outputs.cache-hit != 'true'\n      shell: bash\n      run: echo built\n    - uses: Swatinem/rust-cache@v2\n      with:\n        workspaces: x\n",
        &[],
    ));
    assert!(outcome.success, "{out}");
    assert_eq!(outcome.log.len(), 3, "{out}");
    assert!(
        outcome.log[0].contains("actions/cache/restore@v6 is left out here"),
        "{out}"
    );
    assert_eq!(outcome.log[1], "built");
}

#[test]
fn an_upload_keeps_the_files_on_this_machine_and_needs_them_there() {
    let _alone = support::alone();
    let scene = Scene::new();
    support::write(&scene.workspace(), &[("out/a.crate", "")]);
    let absolute = scene.workspace().join("out/a.crate");
    let upload = |path: &str| {
        ok(scene.run(
            &format!(
                "runs:\n  using: composite\n  steps:\n    - uses: actions/upload-artifact@v7\n      with:\n        name: crate-x\n        path: {path}\n"
            ),
            &[],
        ))
    };

    let (outcome, out) = upload("out/a.crate");
    assert!(outcome.success, "{out}");
    assert_eq!(
        outcome.log,
        [
            "actions/upload-artifact@v7 would upload the artifact `crate-x`: out/a.crate; here it stays on this machine"
        ]
    );
    let (outcome, out) = upload(&format!(
        "|\n          {}\n          out\n",
        absolute.display()
    ));
    assert!(outcome.success, "{out}");

    let (outcome, out) = upload("|\n          out/a.crate\n          out/b.crate\n");
    assert!(!outcome.success, "{out}");
    assert_eq!(
        outcome.log,
        [
            "actions/upload-artifact@v7 would upload the artifact `crate-x`, and finds no file at out/b.crate"
        ]
    );
    let (outcome, out) = upload("''");
    assert!(!outcome.success, "{out}");
    assert!(
        outcome.log[0].ends_with("finds no file at all: no path is named"),
        "{out}"
    );
}

/// A host whose PATH holds only `bin`.
fn host_with_path(scene: &Scene, bin: &Path) -> runner::Host {
    let mut host = scene.host();
    host.env.retain(|(name, _)| name != "PATH");
    host.env.push((
        "PATH".into(),
        format!("{}:/usr/bin:/bin", bin.display()).into(),
    ));
    host
}

#[test]
fn a_setup_step_stands_in_with_the_tool_on_the_path_in_its_version() {
    let _alone = support::alone();
    let scene = Scene::new();
    let bin = scene.path().join("bin");
    executable(&bin.join("node"), "#!/bin/sh\necho v24.11.1\n");
    executable(
        &bin.join("go"),
        "#!/bin/sh\necho go version go1.26.3 linux/amd64\n",
    );
    let host = host_with_path(&scene, &bin);
    let setup = |node: &str, go: &str| {
        let dir = scene.action(
            "main",
            &format!(
                "runs:\n  using: composite\n  steps:\n    - uses: actions/setup-node@v7\n      with:\n        node-version: {node}\n    - if: always()\n      uses: actions/setup-go@v7\n      with:\n        go-version: \"{go}\"\n"
            ),
        );
        ok(scene.run_on(&host, &mut Job::default(), &dir, &[], &[]))
    };

    let (outcome, out) = setup("24", "1.26");
    assert!(outcome.success, "{out}");
    assert!(
        outcome.log[0].contains("`node` from the PATH: v24.11.1"),
        "{out}"
    );

    let (outcome, out) = setup("22", "1.2");
    assert!(!outcome.success, "{out}");
    assert!(
        outcome.log[0].contains("would put node 22 on the PATH"),
        "{out}"
    );
    assert!(outcome.log[0].contains("\"v24.11.1\""), "{out}");
    assert!(
        outcome.log[1].contains("would put go 1.2 on the PATH"),
        "{out}"
    );
}

#[test]
fn a_setup_step_without_its_tool_fails() {
    let _alone = support::alone();
    let scene = Scene::new();
    let empty = scene.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    let mut host = scene.host();
    host.env.retain(|(name, _)| name != "PATH");
    host.env.push(("PATH".into(), empty.clone().into()));
    let dir = scene.action(
        "main",
        "runs:\n  using: composite\n  steps:\n    - uses: actions/setup-node@v7\n      with:\n        node-version: 24\n",
    );
    let (outcome, out) = ok(scene.run_on(&host, &mut Job::default(), &dir, &[], &[]));
    assert!(!outcome.success, "{out}");
    assert!(outcome.log[0].contains("that is \"none\""), "{out}");
}

/// A repository `owner/<name>` below `origin`, with `files` committed and
/// tagged `v1`.
fn remote(origin: &Path, name: &str, files: &[(&str, &str)]) {
    let dir = origin.join("owner").join(name);
    support::write(&dir, files);
    git(&dir, &["init", "--quiet"]);
    git(&dir, &["add", "--all"]);
    git(&dir, &["commit", "--quiet", "--message", "An action"]);
    git(&dir, &["tag", "v1"]);
}

#[test]
fn a_composite_action_a_step_calls_is_fetched_and_run() {
    let _alone = support::alone();
    let scene = Scene::new();
    let origin = scene.path().join("origin");
    remote(
        &origin,
        "composite",
        &[
            (
                "action.yml",
                "inputs:\n  greeting:\n    default: hello\noutputs:\n  said:\n    value: ${{ steps.say.outputs.said }}\nruns:\n  using: composite\n  steps:\n    - id: say\n      shell: bash\n      run: |\n        echo \"${{ inputs.greeting }} from $(basename \"${{ github.action_path }}\")\"\n        echo \"said=${{ inputs.greeting }}\" >> \"$GITHUB_OUTPUT\"\n",
            ),
            (
                "sub/action.yml",
                "runs:\n  using: composite\n  steps:\n    - shell: bash\n      run: echo from the sub-action\n",
            ),
            (
                "node/action.yml",
                "runs:\n  using: node24\n  main: index.js\n",
            ),
        ],
    );
    let host = support::host(
        &scene.path().join("store"),
        &format!("file://{}", origin.display()),
    );
    let dir = scene.action(
        "main",
        "outputs:\n  said:\n    value: ${{ steps.call.outputs.said }}\nruns:\n  using: composite\n  steps:\n    - id: call\n      uses: owner/composite@v1\n      with:\n        Greeting: hi\n    - uses: owner/composite/sub@v1\n",
    );
    let (outcome, out) = ok(scene.run_on(&host, &mut Job::default(), &dir, &[], &[]));
    assert!(outcome.success, "{out}");
    assert_eq!(outcome.log, ["hi from v1", "from the sub-action"]);
    assert_eq!(outcome.outputs.get("said").map(String::as_str), Some("hi"));

    let node = scene.action(
        "node",
        "runs:\n  using: composite\n  steps:\n    - uses: owner/composite/node@v1\n",
    );
    let message = error(scene.run_on(&host, &mut Job::default(), &node, &[], &[]));
    assert!(message.contains("runs on `node24`"), "{message}");
}

#[test]
fn an_action_pinned_to_a_commit_is_fetched() {
    let _alone = support::alone();
    let scene = Scene::new();
    let origin = scene.path().join("origin");
    remote(
        &origin,
        "pinned",
        &[(
            "action.yml",
            "runs:\n  using: composite\n  steps:\n    - shell: bash\n      run: echo pinned\n",
        )],
    );
    let repository = origin.join("owner/pinned");
    // GitHub hands out any commit it holds; git over `file://` only when told.
    git(
        &repository,
        &["config", "uploadpack.allowAnySHA1InWant", "true"],
    );
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&repository)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let commit = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    // A later commit on the branch, so that the pinned one is not its tip.
    support::write(
        &repository,
        &[(
            "action.yml",
            "runs:\n  using: composite\n  steps:\n    - shell: bash\n      run: echo later\n",
        )],
    );
    git(
        &repository,
        &["commit", "--quiet", "--all", "--message", "Later"],
    );

    let host = support::host(
        &scene.path().join("store"),
        &format!("file://{}", origin.display()),
    );
    let dir = scene.action(
        "main",
        &format!("runs:\n  using: composite\n  steps:\n    - uses: owner/pinned@{commit}\n"),
    );
    let (outcome, out) = ok(scene.run_on(&host, &mut Job::default(), &dir, &[], &[]));
    assert!(outcome.success, "{out}");
    assert_eq!(outcome.log, ["pinned"]);
}

#[test]
fn a_fetch_that_fails_leaves_no_checkout_behind() {
    let _alone = support::alone();
    let scene = Scene::new();
    let origin = scene.path().join("origin");
    remote(
        &origin,
        "composite",
        &[(
            "action.yml",
            "runs:\n  using: composite\n  steps:\n    - shell: bash\n      run: echo fetched\n",
        )],
    );
    let store = scene.path().join("store");
    let host = support::host(&store, &format!("file://{}", origin.display()));
    let dir = scene.action(
        "main",
        "runs:\n  using: composite\n  steps:\n    - uses: owner/composite@v2\n",
    );
    let message = error(scene.run_on(&host, &mut Job::default(), &dir, &[], &[]));
    assert!(message.contains("failed"), "{message}");
    assert!(!store.join("owner/composite/v2").exists());

    // Once the ref exists, the next run fetches it rather than taking an
    // empty checkout for the action.
    git(&origin.join("owner/composite"), &["tag", "v2"]);
    let (outcome, out) = ok(scene.run_on(&host, &mut Job::default(), &dir, &[], &[]));
    assert!(outcome.success, "{out}");
    assert_eq!(outcome.log, ["fetched"]);
}

#[test]
fn what_the_steps_expect_on_the_path_is_read_from_the_action() {
    let _alone = support::alone();
    let action = xtask::action::Action::parse(
        r"
runs:
  using: composite
  steps:
    - uses: actions/setup-node@v7
      with:
        Node-Version: 24
    - uses: actions/setup-go@v7
      with:
        go-version: ${{ inputs.go }}
    - uses: actions/setup-go@v7
    - uses: Swatinem/rust-cache@v2
    - uses: someone/else@v1
      with:
        node-version: 22
    - shell: bash
      run: echo
",
        "the action",
    )
    .unwrap();
    let needs = runner::needs(&action);
    let found: Vec<(&str, &str)> = needs
        .iter()
        .map(|need| (need.program, need.version.as_str()))
        .collect();
    assert_eq!(found, [("node", "24")]);

    let other =
        xtask::action::Action::parse("runs:\n  using: docker\n  image: x\n", "docker").unwrap();
    assert!(runner::needs(&other).is_empty());
}

#[test]
fn a_need_is_met_only_by_its_version() {
    let _alone = support::alone();
    let scene = Scene::new();
    let bin = scene.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let env = vec![(
        OsString::from("PATH"),
        OsString::from(format!("{}:/usr/bin:/bin", bin.display())),
    )];
    let action = xtask::action::Action::parse(
        "runs:\n  using: composite\n  steps:\n    - uses: actions/setup-node@v7\n      with:\n        node-version: 24\n",
        "the action",
    )
    .unwrap();
    let need = runner::needs(&action).remove(0);

    assert_eq!(runner::probe(&need, &env), Err("none".to_owned()));
    for (printed, expected) in [
        ("v24", Ok("v24".to_owned())),
        ("v24.3.0", Ok("v24.3.0".to_owned())),
        ("v240.1.0", Err("v240.1.0".to_owned())),
        ("v26.13.1", Err("v26.13.1".to_owned())),
    ] {
        executable(&bin.join("node"), &format!("#!/bin/sh\necho {printed}\n"));
        assert_eq!(runner::probe(&need, &env), expected, "{printed}");
    }
}

#[test]
fn a_line_a_step_printed_is_shown_so_that_the_runner_reads_no_command_in_it() {
    // With `    | ` in front, a block's own annotation is no `::` command any
    // more, and the runner would read the older form `##[name]` in the middle
    // of it; the `[` behind `##` is written as its escape.
    assert_eq!(
        runner::shown("::error file=a##[warning]x.dockerfile::not formatted"),
        "    | ::error file=a##\\u{5b}warning]x.dockerfile::not formatted\n"
    );
    assert_eq!(runner::shown("plain #[x]"), "    | plain #[x]\n");
}
