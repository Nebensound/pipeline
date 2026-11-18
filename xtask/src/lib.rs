//! The tasks of this repository, reached from its root as `cargo xtask`.
//!
//! There is one: `check`. It holds the building blocks, the register and
//! `nebensound.yml` to each other ([`layout`]), and runs every building block
//! against its test cases ([`cases`]) by running the block's own steps, read
//! from its `action.yml` ([`runner`]). The central workflow runs
//! `cargo xtask check --all` as the last step of `Check repository`.
//!
//! The binary is a few lines. Everything else lives here, so that it can be
//! checked without starting the process.

pub mod action;
pub mod cases;
pub mod expr;
pub mod layout;
pub mod runner;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::cases::{ACTIONS, Bench};
use crate::runner::{Host, Job, Place};

/// Nothing to report.
const OK: u8 = 0;
/// A check found something.
const FOUND: u8 = 1;
/// The call was wrong.
const MISUSE: u8 = 2;

const USAGE: &str = "\
Usage: cargo xtask check --all

check --all   Holds the building blocks, the register and nebensound.yml to
              each other, and runs every building block against its test cases
              in tests/pass/ and tests/fail/. The cases run the blocks' own
              steps, which are written for the Linux runners of GitHub Actions,
              so they run on Linux only.
";

/// Why something could not be done, as one sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    /// An error with this message.
    #[must_use]
    pub fn new(message: String) -> Self {
        Self(message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Writes, and stays quiet when the output is gone: a message about that would
/// take the same road.
pub fn say(out: &mut dyn Write, text: &str) {
    let _ = out.write_all(text.as_bytes());
}

/// Runs a program and waits for it, its output kept back unless it fails.
///
/// # Errors
///
/// When the program cannot be started or fails; the message carries what it
/// printed on standard error.
pub fn quietly(program: &OsStr, args: &[OsString]) -> Result<(), Error> {
    let shown = || {
        std::iter::once(program)
            .chain(args.iter().map(OsString::as_os_str))
            .map(OsStr::to_string_lossy)
            .collect::<Vec<_>>()
            .join(" ")
    };
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| Error::new(format!("cannot start `{}`: {error}", shown())))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::new(format!(
            "`{}` failed: {}",
            shown(),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// Everything a run depends on that comes from outside.
#[derive(Clone, Debug)]
pub struct Setup {
    /// The root of this repository.
    pub root: PathBuf,
    /// The operating system, as `std::env::consts::OS` names it.
    pub os: String,
    /// The processor, as `std::env::consts::ARCH` names it.
    pub arch: String,
    /// The environment of this process.
    pub env: Vec<(OsString, OsString)>,
    /// Where actions `owner/repo@ref` are fetched from.
    pub origin: String,
    /// Where the run keeps what it makes, each run in a directory of its own.
    pub scratch: PathBuf,
}

impl Setup {
    /// This process: the repository the xtask was built in, this machine,
    /// GitHub.
    #[must_use]
    pub fn current() -> Self {
        Self {
            root: Path::new(env!("CARGO_MANIFEST_DIR")).join(".."),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            env: std::env::vars_os().collect(),
            origin: "https://github.com".to_owned(),
            scratch: std::env::temp_dir(),
        }
    }
}

/// `runner.arch` for a processor as `std::env::consts::ARCH` names it.
#[must_use]
pub fn runner_arch(arch: &str) -> String {
    match arch {
        "x86_64" => "X64",
        "aarch64" => "ARM64",
        other => other,
    }
    .to_owned()
}

/// What a call asks for.
enum Call {
    Help,
    Check,
}

fn parse(args: &[String]) -> Result<Call, String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["help" | "--help" | "-h"] => Ok(Call::Help),
        ["check", "--all"] => Ok(Call::Check),
        ["check", ..] => Err("`check` takes --all and nothing else".to_owned()),
        [] => Err("no task named".to_owned()),
        [task, ..] => Err(format!("there is no task `{task}`")),
    }
}

/// Runs one call and returns the exit code: 0 when there is nothing to report,
/// 1 when a check found something, 2 when the call itself was wrong.
pub fn run(args: &[String], setup: &Setup, out: &mut dyn Write) -> u8 {
    match parse(args) {
        Ok(Call::Help) => {
            say(out, USAGE);
            OK
        }
        Ok(Call::Check) => check(setup, out),
        Err(problem) => {
            say(out, &format!("{problem}\n\n{USAGE}"));
            MISUSE
        }
    }
}

fn check(setup: &Setup, out: &mut dyn Write) -> u8 {
    say(
        out,
        "Holding the building blocks, the register and nebensound.yml to each other\n",
    );
    let layout = match layout::check(&setup.root) {
        Ok(layout) => layout,
        Err(error) => {
            say(out, &format!("::error::{error}\n"));
            return FOUND;
        }
    };
    for problem in &layout.problems {
        say(out, &format!("::error::{problem}\n"));
    }
    if layout.problems.is_empty() {
        say(out, "They agree.\n");
    }
    if setup.os != "linux" {
        say(
            out,
            &format!(
                "::error::The test cases run the steps of the building blocks, which are \
                 written for the Linux runners of GitHub Actions, and this machine runs {}. Run \
                 `cargo xtask check` on Linux.\n",
                setup.os
            ),
        );
        return FOUND;
    }
    let failures = match cases(&layout.blocks, setup, out) {
        Ok(failures) => failures,
        Err(error) => {
            say(out, &format!("::error::{error}\n"));
            1
        }
    } + layout.problems.len();
    if failures == 0 {
        say(out, "Every building block holds to its test cases.\n");
        OK
    } else {
        say(out, &format!("{failures} problem(s), each named above.\n"));
        FOUND
    }
}

/// Runs the cases of `blocks` and returns how many did not go as expected.
fn cases(blocks: &[String], setup: &Setup, out: &mut dyn Write) -> Result<usize, Error> {
    let scratch = tempfile::tempdir_in(&setup.scratch).map_err(|error| {
        Error::new(format!(
            "cannot make a directory for the run in {}: {error}",
            setup.scratch.display()
        ))
    })?;
    let host = Host {
        env: runner::environment(setup.env.clone()),
        origin: setup.origin.clone(),
        store: scratch.path().join("actions"),
        os: "Linux".to_owned(),
        arch: runner_arch(&setup.arch),
        bash: "bash".into(),
        git: "git".into(),
    };
    let mut job = Job::default();

    // The tool, the way every unit job gets it, so that each case runs the
    // `nebensound` of this commit.
    say(out, "::group::The tool\n");
    let tool = host.run(
        &mut job,
        &setup.root.join(ACTIONS).join("tool"),
        &BTreeMap::new(),
        Place {
            workspace: &setup.root,
            temp: scratch.path(),
        },
        &[],
        out,
    );
    say(out, "::endgroup::\n");
    if !tool?.success {
        return Err(Error::new(
            "the action `tool` failed, so no case can run".to_owned(),
        ));
    }

    let bench = Bench {
        host: &host,
        job: &job,
        root: &setup.root,
        scratch: scratch.path(),
    };
    let mut failures = 0;
    for block in blocks {
        for case in cases::discover(&setup.root.join(ACTIONS).join(block))? {
            let label = case.label();
            match cases::run(bench, block, &case, out) {
                Ok(None) => say(out, &format!("{block}, {label}: as expected\n")),
                Ok(Some(problem)) => {
                    say(out, &format!("::error::{block}, {label}: {problem}\n"));
                    failures += 1;
                }
                Err(error) => {
                    say(
                        out,
                        &format!("::error::{block}, {label}: cannot be run: {error}\n"),
                    );
                    failures += 1;
                }
            }
        }
    }
    Ok(failures)
}
