//! How the tool starts Cargo and the Rust toolchain: one rule for the Rust
//! check (`run rust`) and the build `cargo-package` alike.
//!
//! Every program runs without the variables of this process that steer Cargo,
//! rustc and rustup ([`steers`]), so that no setting that comes as a variable
//! changes what the check finds or what the build hands on. Settings that come
//! as files still apply, and neither hunts them down: a directory override
//! rustup keeps in its settings file, which ranks above `rust-toolchain.toml`,
//! and Cargo's configuration files in the folders above the workspace and in
//! `CARGO_HOME`. A hosted runner has neither, so they are ways a run on a
//! developer's computer can differ from the gate's, whose result is the one
//! that counts.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

/// The variables that start like those that steer Cargo, rustc and rustup
/// and are kept all the same: they say where Cargo and the toolchains are,
/// not what they do.
pub const KEPT: &[&str] = &["CARGO_HOME", "RUSTUP_HOME"];

/// Whether a variable of this process steers Cargo, rustc, rustdoc or rustup
/// and is taken out of what the programs the tool starts inherit: every one
/// whose name starts with `CARGO` or `RUST`, such as `RUSTFLAGS`,
/// `CARGO_TARGET_DIR`, `RUSTUP_TOOLCHAIN` or `CARGO_INCREMENTAL`, less
/// [`KEPT`]. The check and the build set what they need themselves.
#[must_use]
pub fn steers(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    (name.starts_with("CARGO") || name.starts_with("RUST")) && !KEPT.contains(&name.as_ref())
}

/// A command that starts `program` in the folder `dir`, with the search path
/// `search_path`, nothing on its input, and without the variables [`steers`]
/// names.
pub(crate) fn command(program: impl AsRef<OsStr>, dir: &Path, search_path: &OsStr) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(dir)
        .env("PATH", search_path)
        .stdin(Stdio::null());
    for (name, _) in std::env::vars_os() {
        if steers(&name) {
            command.env_remove(name);
        }
    }
    command
}
