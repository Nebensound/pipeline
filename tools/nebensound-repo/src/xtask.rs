//! `nebensound-repo xtask`: the repository's own checks, as its xtask runs
//! them, `cargo xtask check --all`, as the last step of `Check repository`.
//!
//! The toolchain is the one the xtask pins: the `rust-toolchain.toml` (or
//! `rust-toolchain`) in `xtask/`, else the one in the repository root. rustup
//! resolves it from `xtask/`, and the command runs from the root with that
//! toolchain, since the root is where `cargo xtask` is reached. An xtask that
//! pins none fails here rather than running on whatever the computer brings.
//!
//! rustup and Cargo start under the rule of [`crate::toolchain`], as for the
//! Rust check and the build: without the variables of this process that steer
//! Cargo, rustc and rustup ([`steers`](crate::toolchain::steers)), so that no
//! setting that comes as a variable changes what the xtask finds. The one it
//! sets itself, `RUSTUP_TOOLCHAIN` for the toolchain it names, is set after
//! they are taken out. Settings that come as files still apply, and `xtask`
//! does not hunt them down: a directory override rustup keeps in its settings
//! file, which ranks above `rust-toolchain.toml`, and Cargo's configuration
//! files in the folders above the repository and in `CARGO_HOME`. A hosted
//! runner has neither, so they are ways a run on a developer's computer can
//! differ from the gate's, whose result is the one that counts.

use crate::workflow::Log;
use std::ffi::OsStr;
use std::path::Path;

/// The files that pin the xtask's toolchain, from the repository root, in the
/// order rustup would find them from `xtask/`.
pub const PINS: &[&str] = &[
    "xtask/rust-toolchain.toml",
    "xtask/rust-toolchain",
    "rust-toolchain.toml",
    "rust-toolchain",
];

/// Runs the xtask of the repository at `root` with the programs on
/// `search_path`: whether it passed.
pub fn run(root: &Path, search_path: &OsStr, log: &mut Log<'_>) -> bool {
    if !root.join("xtask/Cargo.toml").is_file() {
        log.error("The repository has no xtask/Cargo.toml, so there is no xtask to run.");
        return false;
    }
    let Some(pin) = PINS.iter().find(|pin| root.join(pin).is_file()) else {
        log.error_on(
            "xtask/Cargo.toml",
            "The xtask pins no toolchain: neither xtask/ nor the repository root holds a \
             rust-toolchain.toml.",
        );
        return false;
    };
    let command = |program: &str, dir: &Path| crate::toolchain::command(program, dir, search_path);
    let xtask = root.join("xtask");
    let mut install = command("rustup", &xtask);
    install.args(["toolchain", "install"]);
    if !log.stream(install, "rustup") {
        return false;
    }
    let mut active = command("rustup", &xtask);
    active.args(["show", "active-toolchain"]);
    let toolchain = log.capture(active, "rustup").and_then(|printed| {
        String::from_utf8_lossy(&printed)
            .split_whitespace()
            .next()
            .map(ToOwned::to_owned)
    });
    let Some(toolchain) = toolchain else {
        log.error("rustup names no active toolchain for the xtask.");
        return false;
    };
    log.line(&format!(
        "Running the xtask with {toolchain}, pinned in {pin}."
    ));
    let mut check = command("cargo", root);
    check
        .args(["xtask", "check", "--all"])
        .env("RUSTUP_TOOLCHAIN", &toolchain);
    log.stream(check, "cargo")
}
