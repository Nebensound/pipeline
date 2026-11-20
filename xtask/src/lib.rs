//! The tasks of this repository, reached from its root as `cargo xtask`.
//!
//! There are two, the standard tasks of a repository's xtask:
//!
//! - `check` holds the building blocks, the register and `nebensound.yml` to
//!   each other ([`layout`]), and runs every building block against its test
//!   cases ([`cases`]) by running the block's own steps, read from its
//!   `action.yml` ([`runner`]). The central workflow runs
//!   `cargo xtask check --all` as the last step of `Check repository`.
//! - `setup` calls `nebensound-repo setup` for this repository, with the tool
//!   of this commit, to find or install the programs its checks call, and
//!   then, on Linux, checks what only the test cases need ([`MACHINE`]):
//!   Node.js and Go on the `PATH` in the versions the blocks' steps ask for,
//!   jq, npm, Python's pip and rustup. It installs none of them: the blocks
//!   fetch their own programs when the cases run them, and these stay the
//!   machine's, so it names each one that is missing. Elsewhere the cases do
//!   not run, so it leaves them out and says why.
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

/// What the steps of the blocks call on the machine beside the programs
/// [`runner::needs`] finds, each with its name and the command that shows it
/// is there. Phase 0, `check-json`, `check-rust` and `check-toml` run jq;
/// `nebensound-repo setup` can install a jq of its own, but into its cache and
/// not onto the `PATH` the steps search. `check-json`, `check-markdown` and
/// `check-yaml` install their programs with npm, which `actions/setup-node`
/// puts beside Node.js on GitHub, and which some systems ship apart from
/// Node.js. `check-yaml` runs Python's pip, which some systems ship apart
/// from Python. The tool and `check-rust` install toolchains with rustup.
pub const MACHINE: &[(&str, &str, &[&str])] = &[
    ("jq", "jq", &["--version"]),
    ("npm", "npm", &["--version"]),
    ("pip", "python3", &["-m", "pip", "--version"]),
    ("rustup", "rustup", &["--version"]),
];

/// The options after `setup` that ask the tool for its help, after which
/// nothing is checked.
const HELP: &[&str] = &["help", "--help", "-h"];

const USAGE: &str = "\
Usage: cargo xtask check --all
       cargo xtask setup [<option> ...]

check --all   Holds the building blocks, the register and nebensound.yml to
              each other, and runs every building block against its test cases
              in tests/pass/ and tests/fail/. The cases run the blocks' own
              steps, which are written for the Linux runners of GitHub Actions,
              so for now they run on Linux only.
setup         Calls `nebensound-repo setup` for this repository, built from
              this commit, to find or install the programs its checks call.
              Every option goes to it unchanged: --pinned, --with-runtimes,
              --check.
              Then, on Linux, it checks, and installs nothing, what only the
              test cases need: Node.js and Go in the versions the blocks ask
              for, jq, npm, Python's pip and rustup.
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
    /// The `cargo` that builds and starts the tool for `setup`.
    pub cargo: OsString,
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
            cargo: "cargo".into(),
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
    if setup.os != "linux" {
        say(
            out,
            &format!(
                "::error::For now the test cases run only on Linux, since they run the steps of \
                 the building blocks, which are written for the Linux runners of GitHub Actions, \
                 and this machine runs {}.\n",
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

/// `setup`: `nebensound-repo setup` for this repository, built from this
/// commit with the toolchain the tool pins and handed every option unchanged,
/// then, on Linux, what only the test cases need ([`machine`]). The tool shares
/// the terminal, so that it can ask before it installs a runtime, and gets the
/// developer's environment less what would steer Cargo.
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
        .envs(runner::outside_cargo(setup.env.clone()))
        .status();
    let code = match status {
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
            return FOUND;
        }
    };
    if options.iter().any(|option| HELP.contains(&option.as_str())) {
        return code;
    }
    let machine = if setup.os == "linux" {
        machine(setup, out)
    } else {
        say(
            out,
            &format!(
                "For now the test cases run only on Linux, and this machine runs {}, so what only \
                 they need is not checked here.\n",
                setup.os
            ),
        );
        OK
    };
    if code == OK { machine } else { code }
}

/// Checks what only the test cases need and installs nothing: the programs
/// the blocks' steps expect in a version ([`runner::needs`]), and
/// [`MACHINE`], each in the environment a step gets. Each is named with what
/// was found; the result is [`FOUND`] when one is missing.
fn machine(setup: &Setup, out: &mut dyn Write) -> u8 {
    say(out, "What the test cases need besides:\n");
    let needs = match block_needs(&setup.root) {
        Ok(needs) => needs,
        Err(error) => {
            say(out, &format!("::error::{error}\n"));
            return FOUND;
        }
    };
    let env = runner::environment(setup.env.clone());
    let mut missing = 0;
    for need in &needs {
        let (program, version) = (need.program, &need.version);
        match runner::probe(need, &env) {
            Ok(printed) => say(
                out,
                &format!("{program} {version}: on the PATH, {printed}\n"),
            ),
            Err(printed) => {
                say(
                    out,
                    &format!(
                        "{program} {version}: missing, the PATH has {printed}. Put {program} \
                         {version} on the PATH.\n"
                    ),
                );
                missing += 1;
            }
        }
    }
    for (name, program, args) in MACHINE {
        let found = Command::new(program)
            .args(*args)
            .env_clear()
            .envs(env.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_owned()
            });
        if let Some(printed) = found {
            say(out, &format!("{name}: found, {printed}\n"));
        } else {
            say(
                out,
                &format!(
                    "{name}: missing, `{program} {}` fails on the PATH the steps get.\n",
                    args.join(" ")
                ),
            );
            missing += 1;
        }
    }
    if missing == 0 {
        OK
    } else {
        say(
            out,
            &format!("{missing} program(s) the test cases need are missing, each named above.\n"),
        );
        FOUND
    }
}

/// What the steps of every action under `.github/actions/` expect on the
/// `PATH`, each program and version once.
fn block_needs(root: &Path) -> Result<Vec<runner::Need>, Error> {
    let mut needs = std::collections::BTreeSet::new();
    for dir in layout::directories(&root.join(ACTIONS))? {
        if dir.action {
            let action = action::Action::load(&root.join(ACTIONS).join(&dir.name))?;
            needs.extend(runner::needs(&action));
        }
    }
    Ok(needs.into_iter().collect())
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
        installs: scratch.path().join("installs"),
        os: "Linux".to_owned(),
        arch: runner_arch(&setup.arch),
        bash: "bash".into(),
        git: "git".into(),
    };
    let mut job = Job::default();

    // The tool, the way every unit job gets it, so that each case runs the
    // `nebensound-repo` of this commit.
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
