//! What the programs the xtask starts get: the environment they start from,
//! and how what they print is shown.
//!
//! What the tool prints for a case is shown with `    | ` in front, so that a
//! reader tells it from the xtask's own lines. In the gate the xtask is a
//! program the tool starts (`nebensound-repo xtask`), and everything it
//! prints, what it shows of a call included, stands in a stretch in which the
//! tool stopped the runner's processing of workflow commands: neither an
//! annotation for a mistake a failing case makes on purpose, nor a name from a
//! case, starts a command there.

use std::ffi::OsString;
use std::io::{self, BufRead as _, BufReader, Write};
use std::process::{Command, Stdio};

use crate::{Error, say};

/// Variables of this process that Cargo does not get when it builds the tool,
/// for `setup` and for the cases.
///
/// `cargo run` sets them for the xtask, and the gate sets `RUSTUP_TOOLCHAIN`
/// to the toolchain the xtask pins. Handed on, they would make Cargo build
/// with the xtask's toolchain instead of the one the tool pins, and into the
/// xtask's target directory.
const CARGO_SETS: &[&str] = &[
    "RUSTUP_TOOLCHAIN",
    "CARGO",
    "CARGO_MANIFEST_DIR",
    "CARGO_MANIFEST_PATH",
    "CARGO_PRIMARY_PACKAGE",
    "CARGO_CRATE_NAME",
    "CARGO_BIN_NAME",
    "CARGO_TARGET_DIR",
];

/// Prefixes of the variables of this process that Cargo does not get when it
/// builds the tool, for the same reason.
const CARGO_SETS_PREFIXES: &[&str] = &["CARGO_PKG_", "CARGO_BIN_EXE_"];

/// The variables of this process a call of the tool for a case gets; every
/// other one is left out.
///
/// A block's call of the tool on a runner starts from the runner's
/// environment, not from a developer's shell, so a variable that steers a
/// program there (`CARGO_TERM_COLOR`, `RUSTFLAGS`, `NODE_OPTIONS`,
/// `PYTHONPATH`, `GOFLAGS`, `RUSTUP_TOOLCHAIN`, …) would change what a case
/// prints here and not in the gate. Only what a runner step has too is handed
/// on: where the user's files, toolchains and caches lie, the locale, and how
/// to reach the network.
const PASSED_ON: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "TZ",
    "LANG",
    "LANGUAGE",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "GOPATH",
    "GOCACHE",
    "GOMODCACHE",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "all_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// Prefixes of the variables of this process a call of the tool for a case
/// gets, for the same reason: the locale and where the user's files lie.
const PASSED_ON_PREFIXES: &[&str] = &["LC_", "XDG_"];

/// The environment a call of the tool for a case starts from: the variables
/// of this process that `PASSED_ON` names, and no other: what a runner step
/// has too.
#[must_use]
pub fn environment(vars: Vec<(OsString, OsString)>) -> Vec<(OsString, OsString)> {
    vars.into_iter()
        .filter(|(name, _)| {
            let name = name.to_string_lossy();
            PASSED_ON.contains(&name.as_ref())
                || PASSED_ON_PREFIXES
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
        })
        .collect()
}

/// The environment of this process, less the variables `cargo run` sets for
/// the xtask and `RUSTUP_TOOLCHAIN` (`CARGO_SETS`): what `setup` builds and
/// starts the tool with on the developer's behalf.
#[must_use]
pub fn outside_cargo(vars: Vec<(OsString, OsString)>) -> Vec<(OsString, OsString)> {
    vars.into_iter()
        .filter(|(name, _)| {
            let name = name.to_string_lossy();
            !CARGO_SETS.contains(&name.as_ref())
                && !CARGO_SETS_PREFIXES
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
        })
        .collect()
}

/// Starts a command with its standard output and error in one stream, shows
/// every line with `    | ` in front and keeps it, and says whether it
/// succeeded.
///
/// # Errors
///
/// When the command cannot be started.
pub fn stream(mut command: Command, out: &mut dyn Write) -> Result<(bool, Vec<String>), Error> {
    let program = command.get_program().to_string_lossy().into_owned();
    let started = io::pipe().and_then(|(reader, writer)| {
        writer.try_clone().and_then(|copy| {
            command.stdin(Stdio::null()).stdout(copy).stderr(writer);
            command.spawn().map(|child| (reader, child))
        })
    });
    // The command holds the writing end until it is dropped, and the lines
    // below end only when every writing end is closed.
    drop(command);
    let (reader, mut child) =
        started.map_err(|error| Error::new(format!("cannot start {program}: {error}")))?;
    let mut log = Vec::new();
    for bytes in BufReader::new(reader).split(b'\n').map_while(Result::ok) {
        let line = String::from_utf8_lossy(&bytes)
            .trim_end_matches('\r')
            .to_owned();
        say(out, &format!("    | {line}\n"));
        log.push(line);
    }
    let success = child.wait().is_ok_and(|status| status.success());
    Ok((success, log))
}
