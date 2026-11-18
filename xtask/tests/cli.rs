//! Runs the built `cargo xtask` binary the way the workflow does.

use std::process::Command;

fn xtask(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .output()
        .expect("the xtask binary runs")
}

#[test]
fn an_unknown_task_exits_non_zero_and_says_why() {
    let output = xtask(&["probe"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown task"));
}

#[test]
fn help_exits_zero_and_prints_the_usage() {
    let output = xtask(&["help"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("check --all"));
}
