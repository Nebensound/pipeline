//! The tasks of this repository, reached from its root as `cargo xtask`.
//!
//! There are two, the standard tasks of a repository's xtask:
//!
//! - `check` holds the building blocks, the register and `nebensound.yml` to
//!   each other ([`layout`]), and runs every building block against its test
//!   cases ([`cases`]) through the tool of this commit, on every system, a
//!   check or a build after `nebensound-repo setup --pinned <name>` has set up
//!   the programs the register names for it, in a run on GitHub Actions with
//!   `--with-runtimes` as well. A block whose work the tool does not carry out
//!   is named as a problem: its cases cannot run.
//!   The central workflow runs `cargo xtask check --all` as the last step of
//!   `Check repository`.
//! - `setup` calls `nebensound-repo setup` for this repository, with the tool
//!   of this commit, to find or install the programs its checks call, and with
//!   `--with-runtimes` a runtime one of them needs.
//!
//! The binary is a few lines. Everything else lives here, so that it can be
//! checked without starting the process.

pub mod action;
pub mod cases;
pub mod layout;
pub mod process;

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::cases::ACTIONS;

/// Nothing to report.
const OK: u8 = 0;
/// A check found something.
const FOUND: u8 = 1;
/// The call was wrong.
const MISUSE: u8 = 2;

const USAGE: &str = "\
Usage: cargo xtask check --all
       cargo xtask setup [<option> ...]

check --all   Holds the building blocks, the register and nebensound.yml to
              each other, and runs every building block against its test cases
              in tests/pass/ and tests/fail/, through the tool of this
              commit, on every system. Every block's work is carried out by
              the tool: the checks css, dockerfile, html, json, markdown,
              python, rust, toml, yaml, the builds cargo-package and docker
              and the version writer cargo. For a check or a build,
              `nebensound-repo setup --pinned <name>` first sets up the
              programs the register names for it, and a runtime one of them
              needs is taken from the computer. In a run on GitHub Actions the
              setup also gets --with-runtimes, so that a runtime that is
              missing there or too old is installed in the pinned version. A
              block whose work the tool does not carry out is named as a
              problem, since its cases cannot run.
setup         Calls `nebensound-repo setup` for this repository, built from
              this commit, to find or install the programs its checks call.
              Every option goes to it unchanged: --pinned, --with-runtimes,
              --check, the names of checks and builds.
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
    /// The environment of this process.
    pub env: Vec<(OsString, OsString)>,
    /// Where the run keeps what it makes, each run in a directory of its own.
    pub scratch: PathBuf,
    /// The `cargo` that builds and starts the tool for `setup`.
    pub cargo: OsString,
}

impl Setup {
    /// This process: the repository the xtask was built in, on this machine.
    #[must_use]
    pub fn current() -> Self {
        Self {
            root: Path::new(env!("CARGO_MANIFEST_DIR")).join(".."),
            env: std::env::vars_os().collect(),
            scratch: std::env::temp_dir(),
            cargo: "cargo".into(),
        }
    }
}

/// What a call asks for.
enum Call<'a> {
    Help,
    Check,
    Setup(&'a [String]),
}

fn parse(args: &[String]) -> Result<Call<'_>, String> {
    if args.first().is_some_and(|task| task == "setup") {
        return Ok(Call::Setup(&args[1..]));
    }
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
/// 1 when a check found something, 2 when the call itself was wrong. `setup`
/// returns the exit code of `nebensound-repo setup`.
pub fn run(args: &[String], setup: &Setup, out: &mut dyn Write) -> u8 {
    match parse(args) {
        Ok(Call::Help) => {
            say(out, USAGE);
            OK
        }
        Ok(Call::Check) => check(setup, out),
        Ok(Call::Setup(options)) => programs(options, setup, out),
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
    let (direct, other): (Vec<String>, Vec<String>) = layout
        .blocks
        .iter()
        .cloned()
        .partition(|block| cases::FROM_THE_TOOL.contains(&block.as_str()));
    for block in &other {
        say(
            out,
            &format!(
                "::error::{block}: the tool carries out none of its work, so its cases cannot \
                 run: the xtask runs the cases of a block through the tool only\n"
            ),
        );
    }
    let failures = match cases(&direct, setup, out) {
        Ok(failures) => failures,
        Err(error) => {
            say(out, &format!("::error::{error}\n"));
            1
        }
    } + layout.problems.len()
        + other.len();
    if failures == 0 {
        say(out, "Every building block holds to its test cases.\n");
        OK
    } else {
        say(out, &format!("{failures} problem(s), each named above.\n"));
        FOUND
    }
}

/// `setup`: `nebensound-repo setup` for this repository, built from this
/// commit with the toolchain the tool pins and handed every option unchanged;
/// its exit code is the tool's. The tool shares the terminal, so that it can
/// ask before it installs a runtime, and gets the developer's environment less
/// what would steer Cargo.
///
/// Cargo runs in the tool's directory: rustup picks the toolchain from the
/// directory it is started in, not from `--manifest-path`, so only there does
/// the tool's `rust-toolchain.toml` count.
fn programs(options: &[String], setup: &Setup, out: &mut dyn Write) -> u8 {
    let tool = setup.root.join("tools/nebensound-repo");
    let manifest = tool.join("Cargo.toml");
    let status = Command::new(&setup.cargo)
        .args(["run", "--quiet", "--locked", "--manifest-path"])
        .arg(&manifest)
        .arg("--")
        .arg("-C")
        .arg(&setup.root)
        .arg("setup")
        .args(options)
        .current_dir(&tool)
        .env_clear()
        .envs(process::outside_cargo(setup.env.clone()))
        .status();
    match status {
        Ok(status) => status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(FOUND),
        Err(error) => {
            say(
                out,
                &format!(
                    "::error::cannot start `{}` in {} to build the tool: {error}\n",
                    setup.cargo.to_string_lossy(),
                    tool.display()
                ),
            );
            FOUND
        }
    }
}

/// Runs the cases of `direct` through the tool, names each with how it went,
/// and returns how many did not go as expected.
fn cases(direct: &[String], setup: &Setup, out: &mut dyn Write) -> Result<usize, Error> {
    if direct.is_empty() {
        return Ok(0);
    }
    let scratch = tempfile::tempdir_in(&setup.scratch).map_err(|error| {
        Error::new(format!(
            "cannot make a directory for the run in {}: {error}",
            setup.scratch.display()
        ))
    })?;
    say(out, "::group::The tool, built with Cargo\n");
    let tool = build(setup, out);
    say(out, "::endgroup::\n");
    let tool = tool?;
    let env = process::environment(setup.env.clone());
    let bench = cases::Direct {
        tool: &tool,
        root: &setup.root,
        env: &env,
        git: &"git".into(),
        scratch: scratch.path(),
        with_runtimes: cases::on_github(&setup.env),
    };
    let mut failures = 0;
    for block in direct {
        if let Some(problem) = cases::set_up(bench, &setup.root, block, out) {
            say(out, &format!("::error::{block}: {problem}\n"));
            failures += 1;
        }
        for case in cases::discover(&setup.root.join(ACTIONS).join(block))? {
            let label = case.label();
            match cases::direct(bench, block, &case, out) {
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

/// Builds the tool of this commit with Cargo, with the toolchain the tool
/// pins and in the release profile the action `tool` builds it in, and
/// returns where Cargo put it.
///
/// Cargo runs in the tool's directory, for its `rust-toolchain.toml`, with
/// the developer's environment less what would steer Cargo, and names what it
/// built in its messages, wherever its target directory is.
fn build(setup: &Setup, out: &mut dyn Write) -> Result<PathBuf, Error> {
    let tool = setup.root.join("tools/nebensound-repo");
    say(
        out,
        &format!("  > cargo build --release --locked in {}\n", tool.display()),
    );
    let output = Command::new(&setup.cargo)
        .args([
            "build",
            "--release",
            "--locked",
            "--quiet",
            "--message-format=json",
            "--manifest-path",
        ])
        .arg(tool.join("Cargo.toml"))
        .current_dir(&tool)
        .env_clear()
        .envs(process::outside_cargo(setup.env.clone()))
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            Error::new(format!(
                "cannot start `{}` in {} to build the tool: {error}",
                setup.cargo.to_string_lossy(),
                tool.display()
            ))
        })?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stderr.lines() {
        say(out, &format!("    | {line}\n"));
    }
    let built = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| {
            message["reason"] == "compiler-artifact"
                && message["target"]["name"] == "nebensound-repo"
        })
        .filter_map(|message| message["executable"].as_str().map(PathBuf::from))
        .next_back();
    match built {
        Some(executable) if output.status.success() => Ok(executable),
        _ => Err(Error::new(
            "Cargo built no tool `nebensound-repo`, so no case of a block that runs from the tool \
             can run"
                .to_owned(),
        )),
    }
}
