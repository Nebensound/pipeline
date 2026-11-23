//! The tool `nebensound-repo` for the tasks of a repository: read, resolve and
//! check its `.nebensound/repo.toml`, run phase 0 on it and show the result.
//!
//! One package, a library and the program. What understands the declaration
//! ([`model`], [`register`], [`resolve`](mod@resolve),
//! [`validate`](mod@validate) and the modules they build on) is deliberately
//! free of input and output. It is handed a declaration, a register and a list
//! of the paths a repository holds, and it answers with resolved units and
//! findings. Walking a working tree, reading files and printing a report belong
//! to the modules around it, which is what keeps every rule of the declaration
//! testable without a filesystem.
//!
//! The binary is three lines. Everything else lives here so that it can be
//! checked without starting a process — and so that the interface and the
//! printed report use one analysis instead of two that can drift apart.
//!
//! What only the program needs is the feature `program`, on by default: the
//! downloading and unpacking of `setup` and the JSON Schema of the
//! declaration. Whatever builds on the library alone switches the default
//! features off and compiles neither.
//!
//! The interface is the `ui` feature and is off by default. What CI builds and
//! runs is the build without it: the report needs no terminal library, and the
//! most trusted code in the organization compiles nothing it does not call.

pub mod annotate;
pub mod audience;
pub mod central;
pub mod conformance;
#[cfg(feature = "ui")]
pub mod draw;
pub mod edit;
pub mod files;
pub mod finding;
pub mod history;
pub mod ignored;
pub mod lockfiles;
pub mod model;
pub mod names;
pub mod output;
pub mod packages;
pub mod path;
pub mod pattern;
pub mod plan;
pub mod programs;
pub mod reference;
pub mod register;
pub mod release_fields;
pub mod report;
pub mod repository;
pub mod requirements;
pub mod resolve;
pub(crate) mod ruff_config;
pub mod rules;
#[cfg(feature = "program")]
pub mod run;
#[cfg(feature = "program")]
pub mod schema;
#[cfg(feature = "program")]
pub mod setup;
#[cfg(feature = "ui")]
pub mod terminal;
pub mod tree;
#[cfg(feature = "ui")]
pub mod ui;
pub mod validate;
pub mod version;
pub mod versions;
pub mod workspace;
pub mod writer;

pub use annotate::{Annotated, Coverage, Kind, Node, annotate};
pub use central::{Central, Ruling};
pub use edit::DeclarationFile;
pub use finding::{Finding, Rule, Severity, UnitName};
pub use history::{History, UnitVersion, Versions};
pub use model::{Audience, Declaration, Edition, Pipeline, Publish, Versioning};
pub use register::Register;
pub use resolve::{Resolution, Unit, resolve};
pub use tree::Tree;
pub use validate::{Analysis, analyse, validate};
pub use version::{TagVersion, Version};

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use thiserror::Error;

use crate::audience::Visibility;
use crate::output::Output;
use crate::path::RelativePath;
use crate::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER};
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
#[command(name = "nebensound-repo", version)]
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
    Check {
        /// What GitHub knows about the repository and its declaration does
        /// not hold.
        #[command(flatten)]
        identity: IdentityArgs,
        /// How GitHub shows the repository, as a run knows it: phase 0 then
        /// holds the declared audience against it. Left out, as on a
        /// developer's machine, nothing is compared.
        #[arg(long, value_enum)]
        visibility: Option<Visibility>,
    },
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
    /// the commit touches, and the current version of each unit they depend
    /// on by path, into its Cargo.toml, its dependents and Cargo.lock, in the
    /// working tree. Never committed: it is done in the checkout of a run,
    /// before building.
    WriteVersions {
        /// Take the versions from a plan `nebensound-repo plan` printed, for a
        /// checkout without the history to resolve them from.
        #[arg(long, value_name = "FILE")]
        plan: Option<PathBuf>,
        /// Start only from the units of the workspace Cargo finds at this
        /// path, as the package test of one workspace does; their path
        /// dependencies are followed wherever they lie.
        #[arg(long, value_name = "PATH")]
        workspace: Option<String>,
    },
    /// Print the version of every unit that carries one: its current version
    /// and the version of the commit at hand.
    Version {
        /// Print the table as Markdown, for the summary of a run.
        #[arg(long)]
        markdown: bool,
        /// For a pull request: the versions its squash merge would get, with
        /// `-pr.<number>` behind them.
        #[command(flatten)]
        pull_request: PullRequestArgs,
    },
    // It describes the format, not a repository, so it reads none. Only the
    // program prints it, so it is part of the feature `program`.
    /// Print the JSON Schema of `.nebensound/repo.toml`, for editors and Taplo.
    #[cfg(feature = "program")]
    Schema,
    // Like the schema, it describes the format and reads no repository.
    /// Print the register as Markdown, as `docs/register.md` holds it.
    Register,
    /// Find or install the programs that the checks the declaration starts
    /// call: by default those on the computer, and the pinned version of what
    /// is missing.
    // Only the program downloads and unpacks, so it is part of the feature
    // `program`.
    #[cfg(feature = "program")]
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
        /// Only the programs of these checks, as the register names them,
        /// such as one building block needs. Without any, those of every
        /// check the declaration starts.
        #[arg(value_name = "CHECK")]
        checks: Vec<String>,
    },
    /// Run one check over the files it covers in one unit, less what
    /// `[[exclude]]` takes out, with the programs `setup` finds or installs:
    /// what its building block runs. For the checks css, dockerfile, html,
    /// json, markdown, rust, toml and yaml.
    // It starts the programs `setup` finds, so it is part of the feature
    // `program` as well.
    #[cfg(feature = "program")]
    Run {
        /// The versions the rust check writes before it tests a package: from
        /// a plan `nebensound-repo plan` printed, for a checkout without the
        /// history to resolve them from. Without it they are resolved from
        /// the history, as `plan` resolves them.
        #[arg(long, value_name = "FILE")]
        plan: Option<PathBuf>,
        /// The check, as the register names it.
        check: String,
        /// The path of the unit, as the plan names it.
        unit: String,
    },
    /// Open the interface. Only in a build with the `ui` feature.
    #[cfg(feature = "ui")]
    Ui,
}

/// The repository's name and default branch, as a run hands them in from its
/// event. Each one left out is read from Git where Git knows it.
#[derive(Debug, Clone, Default, Args)]
pub struct IdentityArgs {
    /// The repository's name, as GitHub knows it. Without it, the name the
    /// address of `origin` ends in.
    #[arg(long, value_name = "NAME", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub name: Option<String>,
    /// The repository's default branch, as GitHub knows it. Without it, the
    /// branch `refs/remotes/origin/HEAD` names.
    #[arg(long, value_name = "BRANCH", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub default_branch: Option<String>,
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
    /// The check that was named is not one `run` carries out.
    #[cfg(feature = "program")]
    #[error(
        "`run` carries out the checks css, dockerfile, html, json, markdown, rust, toml and \
         yaml; {check:?} is not one of them, and its building block runs it"
    )]
    NotRun {
        /// What was named.
        check: String,
    },
    /// The path that was named is not one.
    #[error("{value:?} is not a path inside the repository: {source}")]
    Path {
        /// What was named.
        value: String,
        /// What it failed on.
        source: crate::path::PathError,
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
        #[cfg(feature = "program")]
        Ok(Call::Schema) => {
            let schema = crate::schema::json_schema();
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
    #[cfg(feature = "program")]
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
    /// Run phase 0 and print the whole result, with the name and the default
    /// branch the call gives, holding the declared audience against the
    /// visibility if one is given.
    Check(IdentityArgs, Option<Visibility>),
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
    /// Find or install the programs of the checks: those named, or those of
    /// every check the declaration starts.
    #[cfg(feature = "program")]
    Setup(setup::Options, Vec<String>),
    /// Run a check over the files it covers in a unit, with the versions of
    /// this plan if one is given.
    #[cfg(feature = "program")]
    Run {
        check: run::Check,
        unit: RelativePath,
        plan: Option<PathBuf>,
    },
    /// Open the interface.
    #[cfg(feature = "ui")]
    Ui,
}

/// What the call asks for, refusing a path that is not one.
fn call(cli: &Cli) -> Result<Call, Failure> {
    Ok(Call::Repository(match &cli.command {
        #[cfg(feature = "program")]
        Some(Command::Schema) => return Ok(Call::Schema),
        Some(Command::Register) => return Ok(Call::Register),
        None => Job::Check(IdentityArgs::default(), None),
        Some(Command::Check {
            identity,
            visibility,
        }) => Job::Check(identity.clone(), *visibility),
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
        #[cfg(feature = "program")]
        Some(Command::Setup {
            pinned,
            with_runtimes,
            check,
            checks,
        }) => Job::Setup(
            setup::Options {
                mode: match (*check, *pinned) {
                    (true, _) => setup::Mode::Check,
                    (false, true) => setup::Mode::Pinned,
                    (false, false) => setup::Mode::Missing,
                },
                with_runtimes: *with_runtimes,
            },
            checks.clone(),
        ),
        #[cfg(feature = "program")]
        Some(Command::Run { plan, check, unit }) => Job::Run {
            check: run::Check::of(check).ok_or_else(|| Failure::NotRun {
                check: check.clone(),
            })?,
            unit: path_of(unit)?,
            plan: plan.clone(),
        },
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

/// Whether the call reads the files of the repository alone, without its
/// history.
///
/// So are the calls of the building blocks in a unit job, whose checkout holds
/// only the commit at hand: the history is read in Declare, and the versions
/// come with the plan it hands on. Which programs `setup` needs follows from
/// the checks the plan starts, never from a version, so a question about a
/// release does not keep anyone from setting up the programs either. A check
/// that writes versions reads the history only when no plan hands them in.
fn files_alone(job: &Job) -> bool {
    match job {
        Job::Files { .. }
        | Job::ExcludedPackages
        | Job::WriteVersions(Writing { plan: Some(_), .. }) => true,
        #[cfg(feature = "program")]
        Job::Setup(..) => true,
        #[cfg(feature = "program")]
        Job::Run { check, plan, .. } => plan.is_some() || !check.writes_versions(),
        _ => false,
    }
}

/// Phase 0 on the repository as the call needs it: without the history for a
/// call that reads the files alone, and for `check` with a visibility, with
/// the declared audience held against it.
///
/// For the versions of a pull request, what a package it would publish to a
/// registry requires by path is held against the records
/// ([`requirements`](mod@requirements)), unless phase 0 found errors already:
/// a version read off a declaration with errors could be one the history does
/// not give.
fn analysis_for(
    repository: &Repository,
    central: &Central,
    job: &Job,
) -> Result<Analysis, Failure> {
    let mut analysis = if files_alone(job) {
        repository.analyse_files(central)
    } else {
        repository.analyse(central)
    }?;
    Ok(match job {
        Job::Check(_, Some(visibility)) => analysis.with_visibility(*visibility),
        Job::Version {
            pull_request: Some(pull_request),
            ..
        } if !analysis.failed() => {
            let found =
                requirements::findings(repository, &analysis, central, pull_request.number)?;
            analysis.loose.extend(found);
            analysis
        }
        _ => analysis,
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
    let repository = repository_of(cli, job);
    let central = central_from(register, defaults)?;
    let mut analysis = analysis_for(&repository, &central, job)?;

    match job {
        Job::Check(given, _) => say(
            out,
            &check(&repository, &mut analysis, &central, given, output)?,
        ),
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
        #[cfg(feature = "program")]
        Job::Setup(..) if analysis.failed() => say(
            out,
            "there is nothing to set up: phase 0 found errors, and `check` says which\n",
        ),
        #[cfg(feature = "program")]
        Job::Setup(options, named) => {
            return Ok(set_up(&analysis, &central, *options, named, out));
        }
        // The same reason as for the file list: a declaration with errors
        // would check the wrong files.
        #[cfg(feature = "program")]
        Job::Run { .. } if analysis.failed() => say(
            out,
            "nothing is checked: phase 0 found errors, and `check` says which\n",
        ),
        #[cfg(feature = "program")]
        Job::Run { check, unit, plan } => {
            let orders = orders(plan.as_ref(), &analysis, &central)?;
            let checked = Checked {
                repository: &repository,
                analysis: &analysis,
                central: &central,
                orders: &orders,
            };
            return Ok(run_check(cli, &checked, *check, unit, output, out));
        }
        #[cfg(feature = "ui")]
        Job::Ui => {
            terminal::open(&analysis, output).map_err(|source| Failure::Terminal { source })?;
        }
    }

    Ok(if analysis.failed() { FOUND } else { OK })
}

/// `setup`: the programs of the checks named, or of every check the
/// declaration starts, found or installed on this computer.
#[cfg(feature = "program")]
fn set_up(
    analysis: &Analysis,
    central: &Central,
    options: setup::Options,
    named: &[String],
    out: &mut dyn Write,
) -> u8 {
    let checks = if named.is_empty() {
        setup::started(&plan::plan(analysis, central))
    } else {
        match setup::named(central.register(), named) {
            Ok(checks) => checks,
            Err(unknown) => {
                say(
                    out,
                    &format!("the register has no check {:?}\n", report::shown(&unknown)),
                );
                return MISUSE;
            }
        }
    };
    let mut environment = setup::Environment::current();
    setup::run(central.register(), &checks, options, &mut environment, out)
}

/// The repository a check runs on, read: phase 0 on it, and the versions its
/// packages are given.
#[cfg(feature = "program")]
struct Checked<'a> {
    repository: &'a Repository,
    analysis: &'a Analysis,
    central: &'a Central,
    orders: &'a [writer::Order],
}

/// `run`: the check over the files it covers in the unit, at the repository
/// root without links, with the programs on this computer. In a workflow run
/// every finding is an annotation.
#[cfg(feature = "program")]
fn run_check(
    cli: &Cli,
    checked: &Checked<'_>,
    check: run::Check,
    unit: &RelativePath,
    output: Output,
    out: &mut dyn Write,
) -> u8 {
    let Checked {
        repository,
        analysis,
        central,
        orders,
    } = *checked;
    let list = match files::files(analysis, central, check.name(), unit) {
        Ok(list) => list,
        Err(refusal) => {
            say(out, &format!("{refusal}\n"));
            return FOUND;
        }
    };
    // The repository was read, so it is there.
    let root = fs::canonicalize(&cli.repository).unwrap_or(cli.repository.clone());
    let environment = setup::Environment::current();
    let tracked: Vec<RelativePath> = analysis
        .tree
        .files()
        .into_iter()
        .map(|(node, _)| node.path.clone())
        .collect();
    let excluded = repository.excluded_packages(analysis);
    let context = run::Context {
        register: central.register(),
        env: &environment,
        root: &root,
        temp: &std::env::temp_dir(),
        workflow: output == Output::Workflow,
        packages: run::Packages {
            repository,
            tracked: &tracked,
            excluded: &excluded,
            orders,
        },
    };
    run::run(&context, check, unit, &list, out)
}

/// The whole report of phase 0, ended by a line break, with what departs from
/// the organization's conventions among the findings.
fn check(
    repository: &Repository,
    analysis: &mut Analysis,
    central: &Central,
    given: &IdentityArgs,
    output: Output,
) -> Result<String, Failure> {
    let identity = repository.identity(given.name.as_deref(), given.default_branch.as_deref())?;
    let found = conformance::findings(
        &identity,
        analysis
            .tree
            .files()
            .into_iter()
            .map(|(node, _)| &node.path),
    );
    let loose = analysis.tree.attach(found);
    analysis.loose.extend(loose);
    // In a run every finding is an annotation, so that it shows on the run
    // and on the pull request; elsewhere it is a line.
    let findings = if output == Output::Workflow {
        report::annotations(analysis)
    } else {
        report::findings(analysis)
    };
    let mut notes = String::new();
    for note in conformance::notes(&identity) {
        notes.push_str(&report::shown(&note));
        notes.push_str("\n\n");
    }
    Ok(format!(
        "{}\n\n{}\n\n{}\n\n{findings}\n\n{notes}{}\n",
        report::configuration(analysis, central),
        rules::report(analysis, &repository.reader(), rules::Defaults::BUILT_IN),
        report::tree(analysis),
        report::summary(analysis)
    ))
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
    let orders = orders(writing.plan.as_ref(), analysis, central)?;
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

/// The versions of the units: from the plan in `file`, or, without one,
/// resolved from the history as `plan` resolves them.
fn orders(
    file: Option<&PathBuf>,
    analysis: &Analysis,
    central: &Central,
) -> Result<Vec<writer::Order>, Failure> {
    match file {
        Some(file) => fs::read_to_string(file)
            .map_err(|error| error.to_string())
            .and_then(|text| writer::Order::read(&text).map_err(|error| error.to_string()))
            .map_err(|message| Failure::Plan {
                path: file.display().to_string(),
                message,
            }),
        None => Ok(writer::Order::of(&plan::plan(analysis, central))),
    }
}

/// Why a declaration could not be read.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ReadError {
    /// The TOML did not read, or did not match the shape.
    #[error("the declaration is not readable: {0}")]
    Unreadable(#[from] toml::de::Error),
    /// The same pattern stands twice under `[[exclude]]`.
    #[error(
        "{pattern} stands twice under [[exclude]]. A list can say the same thing twice, a \
         statement cannot: of two reasons for the same file, one is untrue"
    )]
    DuplicateExclusion {
        /// The duplicated pattern.
        pattern: String,
    },
    /// A repository excluded a pattern rather than a path.
    #[error(
        "{pattern} takes a whole type or name out of the check. Here a pattern may only add \
         work, never excuse any — write the path, or pull it back in with `include`"
    )]
    WildcardExclusion {
        /// The rejected pattern.
        pattern: String,
    },
    /// A repository made an exclusion depend on the file being empty.
    #[error(
        "{path} sets only-if-empty, which only the central list of what nobody checks may set. \
         Here an exclusion takes its path out as it stands — leave the field out"
    )]
    ConditionalExclusion {
        /// The path of the rejected entry.
        path: String,
    },
    /// The declaration is written for an edition this tool does not know.
    #[error(
        "it is written for edition {edition}, and this tool knows edition {EDITION} only: a \
         newer pipeline is needed to read it"
    )]
    UnknownEdition {
        /// The edition it names.
        edition: i64,
    },
    /// `edition` is a whole number below 1.
    #[error("edition {edition} does not exist: an edition is a whole number from 1")]
    InvalidEdition {
        /// The number it names.
        edition: i64,
    },
}

/// The edition this tool reads: the only one there is. A declaration that
/// leaves `edition` out is read in it.
pub const EDITION: u64 = 1;

/// The top level of a declaration as far as its edition goes, read before
/// anything else of the file: a newer edition may hold what this reader does
/// not know.
#[derive(serde::Deserialize)]
struct EditionOnly {
    #[serde(default)]
    edition: Option<toml::Value>,
}

impl Declaration {
    /// The edition a declaration is written for, read before anything else
    /// of it: [`EDITION`] when it names none.
    ///
    /// A whole number above the editions this tool knows is
    /// [`ReadError::UnknownEdition`], whatever else the file holds, so that a
    /// file written for a newer pipeline says so instead of failing on a
    /// field this reader does not know. A value that is no whole number is
    /// left to [`Declaration::parse`], which refuses it as a value of the
    /// wrong type.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError::Unreadable`] when the TOML does not read,
    /// [`ReadError::UnknownEdition`] for an edition this tool does not know,
    /// and [`ReadError::InvalidEdition`] for a whole number below 1.
    pub fn read_edition(source: &str) -> Result<u64, ReadError> {
        let top: EditionOnly = toml::from_str(source)?;
        match top.edition {
            Some(toml::Value::Integer(edition)) if edition < 1 => {
                Err(ReadError::InvalidEdition { edition })
            }
            Some(toml::Value::Integer(edition)) if edition.cast_unsigned() != EDITION => {
                Err(ReadError::UnknownEdition { edition })
            }
            _ => Ok(EDITION),
        }
    }

    /// Reads the declaration of a repository.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError`] when the TOML does not read, names an edition
    /// this tool does not know, carries a field the shape does not have,
    /// writes an exclusion as a pattern, or makes one depend on the file
    /// being empty.
    pub fn parse(source: &str) -> Result<Self, ReadError> {
        let declaration = Self::parse_central(source)?;
        for entry in &declaration.exclude {
            if entry.path.is_wildcard() {
                return Err(ReadError::WildcardExclusion {
                    pattern: entry.path.to_string(),
                });
            }
            if entry.only_if_empty {
                return Err(ReadError::ConditionalExclusion {
                    path: entry.path.to_string(),
                });
            }
        }
        Ok(declaration)
    }

    /// Reads a centrally kept declaration — the defaults.
    ///
    /// The same shape, one freedom more: here an exclusion may be a pattern.
    /// Centrally, work may be excused; locally it may only be added.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError`] when the TOML does not read, names an edition
    /// this tool does not know, or does not match the shape.
    pub fn parse_central(source: &str) -> Result<Self, ReadError> {
        Self::read_edition(source)?;
        let declaration: Self = toml::from_str(source)?;
        // As a table, uniqueness came for free: TOML forbids the same key
        // twice. A list does not, so the rule stands here now.
        let mut seen = std::collections::BTreeSet::new();
        for entry in &declaration.exclude {
            if !seen.insert(&entry.path) {
                return Err(ReadError::DuplicateExclusion {
                    pattern: entry.path.to_string(),
                });
            }
        }
        Ok(declaration)
    }
}
