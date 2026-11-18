//! The tool around the library: find the tree, run phase 0, show the result.
//!
//! The binary is three lines. Everything else lives here so that it can be
//! checked without starting a process — and so that the interface and the
//! printed report use one analysis instead of two that can drift apart.
//!
//! The interface is the `ui` feature and is off by default. What CI builds and
//! runs is the build without it: the report needs no terminal library, and the
//! most trusted code in the organization compiles nothing it does not call.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg(feature = "ui")]
pub mod draw;
pub mod output;
pub mod plan;
pub mod report;
pub mod repository;
#[cfg(feature = "ui")]
pub mod terminal;
#[cfg(feature = "ui")]
pub mod ui;

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use nebensound_declaration::path::RelativePath;
use nebensound_declaration::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER};
use thiserror::Error;

use crate::output::Output;
use crate::repository::{Repository, central_from};

/// Nothing to report.
const OK: u8 = 0;
/// The repository has a problem.
const FOUND: u8 = 1;
/// The call was wrong.
const MISUSE: u8 = 2;

/// Reads the declaration of a repository, runs phase 0 and shows the result.
#[derive(Debug, Parser)]
#[command(name = "nebensound", version)]
pub struct Cli {
    /// The repository this is about.
    #[arg(short = 'C', long, default_value = ".", global = true)]
    pub repository: PathBuf,

    /// What to do. Without one: check.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// The individual calls.
#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Run phase 0 and print the whole result. The default.
    Check,
    /// Print the annotated tree and nothing else.
    Tree,
    /// Say what holds for one path, and why.
    Explain {
        /// The path inside the repository.
        path: String,
    },
    /// Print which unit runs which check, as JSON for a matrix.
    Plan,
    /// Open the interface. Only in a build with the `ui` feature.
    #[cfg(feature = "ui")]
    Ui,
}

/// Why a call did not go through.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Failure {
    /// The repository could not be read.
    #[error(transparent)]
    Repository(#[from] repository::Error),
    /// The interface got no terminal.
    #[cfg(feature = "ui")]
    #[error("the interface got no terminal: {source}")]
    Terminal {
        /// What it was.
        source: std::io::Error,
    },
    /// The path that was named is not one.
    #[error("{value:?} is not a path inside the repository: {source}")]
    Path {
        /// What was named.
        value: String,
        /// What it failed on.
        source: nebensound_declaration::path::PathError,
    },
}

/// Runs one call and returns the exit code.
///
/// 0 when there is nothing to report; 1 when the repository has a problem; 2
/// when the call itself was wrong.
pub fn run<I, T>(args: I, out: &mut dyn Write) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    run_against(
        args,
        BUILTIN_REGISTER,
        BUILTIN_DEFAULTS,
        Output::current(),
        out,
    )
}

/// The same against a given register, given defaults and a given output.
///
/// There is no switch for any of it. The central data is compiled in so that no
/// repository holds a copy that can fall behind, and where the output goes is
/// what the process happens to have. This form stands beside it so that "the
/// built-in data does not read" and "there is no terminal here" are checked
/// messages rather than claimed ones — and so that no check can take over the
/// terminal it runs in.
pub fn run_against<I, T>(
    args: I,
    register: &str,
    defaults: &str,
    output: Output,
    out: &mut dyn Write,
) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            // --help and --version are not a failure, only an output.
            say(out, &error.render().to_string());
            return if error.use_stderr() { MISUSE } else { OK };
        }
    };
    // Whether a call makes sense does not depend on what it would have been
    // called on, so the path is checked before the repository is read at all.
    let job = match job(&cli) {
        Ok(job) => job,
        Err(failure) => {
            say(out, &format!("{failure}\n"));
            return MISUSE;
        }
    };
    match execute(&cli, &job, register, defaults, output, out) {
        Ok(code) => code,
        Err(failure) => {
            say(out, &format!("{failure}\n"));
            FOUND
        }
    }
}

/// Writes, and stays quiet when the output is gone: a message about that would
/// take the same road.
fn say(out: &mut dyn Write, text: &str) {
    let _ = out.write_all(text.as_bytes());
}

/// One call, with everything about it already checked.
enum Job {
    /// Run phase 0 and print the whole result.
    Check,
    /// Print the annotated tree.
    Tree,
    /// Say what holds for this path.
    Explain(RelativePath),
    /// Print which unit runs which check.
    Plan,
    /// Open the interface.
    #[cfg(feature = "ui")]
    Ui,
}

/// What the call asks for, refusing a path that is not one.
fn job(cli: &Cli) -> Result<Job, Failure> {
    Ok(match &cli.command {
        None | Some(Command::Check) => Job::Check,
        Some(Command::Tree) => Job::Tree,
        Some(Command::Plan) => Job::Plan,
        #[cfg(feature = "ui")]
        Some(Command::Ui) => Job::Ui,
        Some(Command::Explain { path }) => {
            Job::Explain(RelativePath::new(path).map_err(|source| Failure::Path {
                value: path.clone(),
                source,
            })?)
        }
    })
}

fn execute(
    cli: &Cli,
    job: &Job,
    register: &str,
    defaults: &str,
    output: Output,
    out: &mut dyn Write,
) -> Result<u8, Failure> {
    // Only the interface asks where the output goes. A build without it takes
    // the argument all the same, so that the entry point reads the same in
    // both.
    #[cfg(not(feature = "ui"))]
    let _ = output;

    let repository = Repository::at(&cli.repository);
    let central = central_from(register, defaults)?;
    let analysis = repository.analyse(&central)?;

    match job {
        Job::Check => {
            say(
                out,
                &format!(
                    "{}\n\n{}\n\n{}\n\n{}\n",
                    report::configuration(&analysis, &central),
                    report::tree(&analysis),
                    report::findings(&analysis),
                    report::summary(&analysis)
                ),
            );
        }
        Job::Tree => say(out, &format!("{}\n", report::tree(&analysis))),
        Job::Explain(wanted) => {
            let Some(text) = report::explain(&analysis, wanted) else {
                say(
                    out,
                    &format!("{wanted} is not in the tree: git does not list it\n"),
                );
                return Ok(FOUND);
            };
            say(out, &format!("{text}\n"));
        }
        // A plan read off a declaration with errors would start the wrong
        // things with a straight face, so there is none. What is printed then
        // is a sentence, which no matrix takes for a plan.
        Job::Plan if analysis.failed() => say(
            out,
            "there is no plan: phase 0 found errors, and `check` says which\n",
        ),
        Job::Plan => say(
            out,
            &format!("{}\n", plan::plan(&analysis, &central).to_json()),
        ),
        #[cfg(feature = "ui")]
        Job::Ui => {
            terminal::open(&analysis, output).map_err(|source| Failure::Terminal { source })?;
        }
    }

    Ok(if analysis.failed() { FOUND } else { OK })
}
