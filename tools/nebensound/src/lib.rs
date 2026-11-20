//! The tool around the library: find the tree, run phase 0, show the result.
//!
//! The binary is three lines. Everything else lives here so that it can be
//! checked without starting a process — and so that the interface and the
//! printed report use one analysis instead of two that can drift apart.
//!
//! The interface is the `ui` feature and is off by default. What CI builds and
//! runs is the build without it: the report needs no terminal library, and the
//! most trusted code in the organization compiles nothing it does not call.

#[cfg(feature = "ui")]
pub mod draw;
pub mod files;
pub mod ignored;
pub mod output;
pub mod packages;
pub mod plan;
pub mod reference;
pub mod report;
pub mod repository;
pub mod rules;
pub mod setup;
#[cfg(feature = "ui")]
pub mod terminal;
#[cfg(feature = "ui")]
pub mod ui;
pub mod versions;
pub mod writer;

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use nebensound_declaration::path::RelativePath;
use nebensound_declaration::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER};
use nebensound_declaration::{Analysis, Central};
use thiserror::Error;

use crate::output::Output;
use crate::repository::{Repository, central_from};
use crate::versions::PullRequest;

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
    Plan {
        /// For a pull request: its versions are those its squash merge would
        /// get, with `-pr.<number>` behind them.
        #[command(flatten)]
        pull_request: PullRequestArgs,
    },
    /// Print the files a check covers in a unit, less what `[[exclude]]` takes
    /// out: what its building block checks.
    Files {
        /// The check, as the register names it.
        check: String,
        /// The path of the unit, as the plan names it.
        unit: String,
        /// End every path with a zero byte instead of a line break.
        #[arg(short = 'z')]
        zero: bool,
    },
    /// Print the Rust packages `[[exclude]]` takes out as a whole, each with
    /// its reason, as JSON: what `check-rust` leaves out of every step.
    ExcludedPackages,
    /// Write the version of every unit with the version writer `cargo` that
    /// the commit touches, and a version for each unit they depend on by path
    /// (its last release, or its pending version when it was never released),
    /// into its Cargo.toml, its dependents and Cargo.lock, in the working
    /// tree. Never committed: it is done in the checkout of a run,
    /// before building.
    WriteVersions {
        /// Take the versions from a plan `nebensound plan` printed, for a
        /// checkout without the history to resolve them from.
        #[arg(long, value_name = "FILE")]
        plan: Option<PathBuf>,
        /// Start only from the units of the workspace Cargo finds at this
        /// path, as the package test of one workspace does; their path
        /// dependencies are followed wherever they lie.
        #[arg(long, value_name = "PATH")]
        workspace: Option<String>,
    },
    /// Print the version of every unit that carries one: its last release, how
    /// it is released, the version of the commit at hand and how many commits
    /// wait for the next release.
    Version {
        /// Print the table as Markdown, for the summary of a run.
        #[arg(long)]
        markdown: bool,
        /// For a pull request: the versions its squash merge would get, with
        /// `-pr.<number>` behind them.
        #[command(flatten)]
        pull_request: PullRequestArgs,
    },
    // It describes the format, not a repository, so it reads none.
    /// Print the JSON Schema of `.nebensound/repo.toml`, for editors and Taplo.
    Schema,
    // Like the schema, it describes the format and reads no repository.
    /// Print the register as Markdown, as `docs/register.md` holds it.
    Register,
    /// Find or install the programs that the checks the declaration starts
    /// call: by default those on the computer, and the pinned version of what
    /// is missing.
    Setup {
        /// Install the pinned version of every program into the tool's folder
        /// in the user's cache directory, whatever the computer has.
        #[arg(long, conflicts_with = "check")]
        pinned: bool,
        /// Install a missing runtime without asking.
        #[arg(long, conflicts_with = "check")]
        with_runtimes: bool,
        /// Change nothing; say what is missing or differs.
        #[arg(long)]
        check: bool,
    },
    /// Open the interface. Only in a build with the `ui` feature.
    #[cfg(feature = "ui")]
    Ui,
}

/// The pull request a call reads the repository for, if any.
#[derive(Debug, Clone, Args)]
pub struct PullRequestArgs {
    /// Read `HEAD` as the merge commit of this pull request: the versions are
    /// those its squash merge would get, with `-pr.<number>` behind them.
    #[arg(long, value_name = "NUMBER", requires = "message", value_parser = clap::value_parser!(u64).range(1..))]
    pub pull_request: Option<u64>,
    /// The message the squash merge of the pull request would carry: its
    /// title and description.
    #[arg(long, requires = "pull_request")]
    pub message: Option<String>,
}

impl PullRequestArgs {
    /// The pull request, when both are given; clap sees to it that neither
    /// comes alone.
    fn read(&self) -> Option<PullRequest> {
        self.pull_request
            .zip(self.message.clone())
            .map(|(number, message)| PullRequest { number, message })
    }
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
    /// The plan that was handed in does not read.
    #[error("the plan {path} does not read: {message}")]
    Plan {
        /// Where it was looked for.
        path: String,
        /// What it failed on.
        message: String,
    },
    /// The versions could not be written.
    #[error(transparent)]
    Write(#[from] writer::WriteError),
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
    dispatch(&cli, register, defaults, output, out)
}

/// Everything after the arguments were read.
///
/// Not generic, unlike its caller: it is compiled once, so every test binary
/// runs the same copy of it, and a call that only one of them makes still
/// counts as run.
fn dispatch(cli: &Cli, register: &str, defaults: &str, output: Output, out: &mut dyn Write) -> u8 {
    // Whether a call makes sense does not depend on what it would have been
    // called on, so the path is checked before the repository is read at all.
    let job = match call(cli) {
        Ok(Call::Schema) => {
            let schema = nebensound_declaration::schema::json_schema();
            say(out, &format!("{:#}\n", schema.as_value()));
            return OK;
        }
        Ok(Call::Register) => {
            return match central_from(register, defaults) {
                Ok(central) => {
                    say(out, &reference::register(&central));
                    OK
                }
                Err(failure) => {
                    say(out, &format!("{failure}\n"));
                    FOUND
                }
            };
        }
        Ok(Call::Repository(job)) => job,
        Err(failure) => {
            say(out, &format!("{failure}\n"));
            return MISUSE;
        }
    };
    match execute(cli, &job, register, defaults, output, out) {
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
enum Call {
    /// Print the schema of the declaration. It is the same for every
    /// repository, so none is read for it.
    Schema,
    /// Print the register as a page. It is the same for every repository too.
    Register,
    /// Something about the repository.
    Repository(Job),
}

/// What `write-versions` is asked for.
struct Writing {
    /// The plan to take the versions from; [`None`] resolves them from the
    /// history.
    plan: Option<PathBuf>,
    /// A folder of the workspace whose units start the writing; [`None`]
    /// starts from every unit.
    workspace: Option<RelativePath>,
}

/// A call about the repository.
enum Job {
    /// Run phase 0 and print the whole result.
    Check,
    /// Print the annotated tree.
    Tree,
    /// Say what holds for this path.
    Explain(RelativePath),
    /// Print which unit runs which check, for the pull request if there is
    /// one.
    Plan(Option<PullRequest>),
    /// Print the files a check covers in a unit.
    Files {
        check: String,
        unit: RelativePath,
        zero: bool,
    },
    /// Print the packages taken out as a whole.
    ExcludedPackages,
    /// Write the versions.
    WriteVersions(Writing),
    /// Print the versions.
    Version {
        markdown: bool,
        pull_request: Option<PullRequest>,
    },
    /// Find or install the programs of the checks.
    Setup(setup::Options),
    /// Open the interface.
    #[cfg(feature = "ui")]
    Ui,
}

/// What the call asks for, refusing a path that is not one.
fn call(cli: &Cli) -> Result<Call, Failure> {
    Ok(Call::Repository(match &cli.command {
        Some(Command::Schema) => return Ok(Call::Schema),
        Some(Command::Register) => return Ok(Call::Register),
        None | Some(Command::Check) => Job::Check,
        Some(Command::Tree) => Job::Tree,
        Some(Command::Plan { pull_request }) => Job::Plan(pull_request.read()),
        Some(Command::ExcludedPackages) => Job::ExcludedPackages,
        Some(Command::WriteVersions { plan, workspace }) => Job::WriteVersions(Writing {
            plan: plan.clone(),
            workspace: workspace.as_deref().map(path_of).transpose()?,
        }),
        Some(Command::Version {
            markdown,
            pull_request,
        }) => Job::Version {
            markdown: *markdown,
            pull_request: pull_request.read(),
        },
        Some(Command::Setup {
            pinned,
            with_runtimes,
            check,
        }) => Job::Setup(setup::Options {
            mode: match (*check, *pinned) {
                (true, _) => setup::Mode::Check,
                (false, true) => setup::Mode::Pinned,
                (false, false) => setup::Mode::Missing,
            },
            with_runtimes: *with_runtimes,
        }),
        #[cfg(feature = "ui")]
        Some(Command::Ui) => Job::Ui,
        Some(Command::Explain { path }) => Job::Explain(path_of(path)?),
        Some(Command::Files { check, unit, zero }) => Job::Files {
            check: check.clone(),
            unit: path_of(unit)?,
            zero: *zero,
        },
    }))
}

fn path_of(value: &str) -> Result<RelativePath, Failure> {
    RelativePath::new(value).map_err(|source| Failure::Path {
        value: value.to_owned(),
        source,
    })
}

/// The repository the call reads: for the versions or the plan of a pull
/// request, `HEAD` as the squash merge of that pull request would put it on
/// its base branch.
fn repository_of(cli: &Cli, job: &Job) -> Repository {
    match job {
        Job::Version {
            pull_request: Some(pull_request),
            ..
        }
        | Job::Plan(Some(pull_request)) => {
            Repository::at(&cli.repository).as_pull_request(pull_request.message.clone())
        }
        _ => Repository::at(&cli.repository),
    }
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

    let repository = repository_of(cli, job);
    let central = central_from(register, defaults)?;
    let analysis = match job {
        // Asked for by the building blocks in a unit job, whose checkout holds
        // only the commit at hand: the history is read in Declare, and the
        // versions come with the plan it hands on. Which programs `setup`
        // needs follows from the checks the plan starts, never from a
        // version, so a question about a release does not keep anyone from
        // setting up the programs either.
        Job::Files { .. }
        | Job::ExcludedPackages
        | Job::WriteVersions(Writing { plan: Some(_), .. })
        | Job::Setup(_) => repository.analyse_files(&central),
        _ => repository.analyse(&central),
    }?;

    match job {
        Job::Check => {
            say(
                out,
                &format!(
                    "{}\n\n{}\n\n{}\n\n{}\n\n{}\n",
                    report::configuration(&analysis, &central),
                    rules::report(&analysis, &repository.reader(), rules::Defaults::BUILT_IN),
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
        Job::Plan(_) if analysis.failed() => say(
            out,
            "there is no plan: phase 0 found errors, and `check` says which\n",
        ),
        Job::Plan(pull_request) => say(out, &plan_of(&analysis, &central, pull_request.as_ref())),
        // The same reason as for the plan: a list read off a declaration with
        // errors would check the wrong files.
        Job::Files { .. } if analysis.failed() => say(
            out,
            "there is no file list: phase 0 found errors, and `check` says which\n",
        ),
        Job::Files { check, unit, zero } => match files::files(&analysis, &central, check, unit) {
            Ok(list) => say(out, &files::render(&list, *zero)),
            Err(refusal) => {
                say(out, &format!("{refusal}\n"));
                return Ok(FOUND);
            }
        },
        // The same reason again: a package read off a declaration with errors
        // could be left out without a valid reason.
        Job::ExcludedPackages if analysis.failed() => say(
            out,
            "there is no list of excluded packages: phase 0 found errors, and `check` says \
             which\n",
        ),
        Job::ExcludedPackages => say(
            out,
            &format!(
                "{}\n",
                packages::to_json(&repository.excluded_packages(&analysis))
            ),
        ),
        // The same reason again: a version read off a declaration with errors
        // could be written into the wrong package.
        Job::WriteVersions(_) if analysis.failed() => say(
            out,
            "no version is written: phase 0 found errors, and `check` says which\n",
        ),
        Job::WriteVersions(writing) => say(
            out,
            &write_versions(&repository, &analysis, &central, writing)?,
        ),
        // The same reason once more: a version read off a declaration with
        // errors could be one the history does not give, so what is printed
        // then is every finding.
        Job::Version {
            markdown,
            pull_request,
        } => say(
            out,
            &versions::report(&analysis, *markdown, pull_request.as_ref()),
        ),
        // The same reason again: the programs of a declaration with errors
        // could be those of the wrong checks.
        Job::Setup(_) if analysis.failed() => say(
            out,
            "there is nothing to set up: phase 0 found errors, and `check` says which\n",
        ),
        Job::Setup(options) => {
            let checks = setup::started(&plan::plan(&analysis, &central));
            let mut environment = setup::Environment::current();
            return Ok(setup::run(
                central.register(),
                &checks,
                *options,
                &mut environment,
                out,
            ));
        }
        #[cfg(feature = "ui")]
        Job::Ui => {
            terminal::open(&analysis, output).map_err(|source| Failure::Terminal { source })?;
        }
    }

    Ok(if analysis.failed() { FOUND } else { OK })
}

/// The plan as one line of JSON, for the pull request if there is one.
fn plan_of(analysis: &Analysis, central: &Central, pull_request: Option<&PullRequest>) -> String {
    let mut plan = plan::plan(analysis, central);
    plan.pull_request = pull_request.map(|pull_request| pull_request.number);
    format!("{}\n", plan.to_json())
}

/// Writes the versions as `writing` asks and returns what was done, ended by a
/// line break.
fn write_versions(
    repository: &Repository,
    analysis: &Analysis,
    central: &Central,
    writing: &Writing,
) -> Result<String, Failure> {
    let orders = match &writing.plan {
        Some(file) => fs::read_to_string(file)
            .map_err(|error| error.to_string())
            .and_then(|text| writer::Order::read(&text).map_err(|error| error.to_string()))
            .map_err(|message| Failure::Plan {
                path: file.display().to_string(),
                message,
            })?,
        None => writer::Order::of(&plan::plan(analysis, central)),
    };
    let manifests: Vec<RelativePath> = analysis
        .tree
        .files()
        .into_iter()
        .map(|(node, _)| node.path.clone())
        .filter(|path| path.file_name() == Some("Cargo.toml"))
        .collect();
    Ok(format!(
        "{}\n",
        writer::write(repository, &manifests, &orders, writing.workspace.as_ref())?
    ))
}
