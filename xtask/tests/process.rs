//! What the programs the xtask starts get from its environment.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::ffi::OsString;

use xtask::process;

#[test]
fn a_call_of_the_tool_gets_only_what_a_runner_step_has_too() {
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
    let kept: Vec<String> = process::environment(vars)
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
    let kept: Vec<String> = process::outside_cargo(vars)
        .into_iter()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    assert_eq!(kept, ["PATH", "HOME", "CARGO_HOME", "TERM"]);
}
