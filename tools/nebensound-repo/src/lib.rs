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
#[cfg(feature = "program")]
pub mod build;
pub mod central;
pub mod conformance;
#[cfg(feature = "program")]
pub mod declare;
#[cfg(feature = "ui")]
pub mod draw;
pub mod edit;
pub mod files;
pub mod finding;
#[cfg(feature = "program")]
pub mod gather;
pub mod history;
pub mod ignored;
pub mod lockfiles;
pub mod lychee_config;
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
#[cfg(feature = "program")]
pub mod release;
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
#[cfg(feature = "program")]
pub mod toolchain;
pub mod tree;
#[cfg(feature = "ui")]
pub mod ui;
#[cfg(feature = "program")]
pub mod unwritten;
pub mod validate;
pub mod version;
pub mod versions;
#[cfg(feature = "program")]
pub mod workflow;
pub mod workspace;
pub mod writer;
#[cfg(feature = "program")]
pub mod xtask;

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
    /// json, markdown, python, rust, toml and yaml.
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
    /// Run one build of one unit, as its building block runs it in the Build
    /// phase: for the build cargo-package. Names the artifact it leaves as
    /// outputs of the step.
    // It starts Cargo and the Rust toolchain and unpacks what it built, so it
    // is part of the feature `program`.
    #[cfg(feature = "program")]
    Build {
        /// The build, as the register names it.
        build: String,
        /// The path of the unit, as the plan names it.
        unit: String,
    },
    /// Write the records of this commit's versions, as the release stage on
    /// `main` does after a push: for every unit the commit touches, a tag on
    /// it and a full GitHub Release. Takes the repository, the token and the
    /// summary of the run from the variables of a workflow run.
    // It calls GitHub through curl, as `setup` downloads, so it is part of
    // the feature `program`.
    #[cfg(feature = "program")]
    Release,
    /// Run phase 0 as the job Declare does, and hand on the plan, the units
    /// that run a check and the units that build as outputs of the step.
    /// Takes what the event says of the repository and its pull request from
    /// the variables of a workflow run, and asks GitHub for the visibility
    /// when the event names none.
    // It may call GitHub through curl, so it is part of the feature `program`.
    #[cfg(feature = "program")]
    Declare,
    /// Report the version of every unit as the job Version does: the table
    /// into the log and as Markdown into the summary of the run, for the pull
    /// request the variables of the run name. After a push to a pre-branch it
    /// says why there is none.
    // It writes into the files of a workflow run, so it is part of the
    // feature `program`, as `release` is.
    #[cfg(feature = "program")]
    ReportVersions,
    /// Fail by name for what the register knows and nobody has written a
    /// building block for yet, as the steps `Not written yet` do.
    // Only a workflow run calls it, so it is part of the feature `program`.
    #[cfg(feature = "program")]
    Unwritten {
        /// What is held against the written blocks.
        #[command(subcommand)]
        what: Unwritten,
    },
    /// Turn the results of the jobs the closing jobs Check and Build wait
    /// for into one.
    // Only a workflow run calls it, so it is part of the feature `program`.
    #[cfg(feature = "program")]
    Gather {
        /// Which closing job.
        #[command(subcommand)]
        what: Gather,
    },
    /// Run the repository's own checks, `cargo xtask check --all`, with the
    /// toolchain the xtask pins.
    // It starts Cargo and rustup, so it is part of the feature `program`.
    #[cfg(feature = "program")]
    Xtask,
    /// Open the interface. Only in a build with the `ui` feature.
    #[cfg(feature = "ui")]
    Ui,
}

/// What `unwritten` holds against the blocks that are written.
#[cfg(feature = "program")]
#[derive(Debug, Clone, Subcommand)]
pub enum Unwritten {
    /// The checks of a unit, as its matrix entry names them.
    Checks {
        /// The path of the unit.
        #[arg(long)]
        unit: String,
        /// Its checks, as a JSON list of names.
        #[arg(long)]
        planned: String,
        /// The checks that have a block, as a JSON list of names.
        #[arg(long)]
        written: String,
    },
    /// The build of a unit, as its matrix entry names it.
    Build {
        /// The path of the unit.
        #[arg(long)]
        unit: String,
        /// Its build.
        #[arg(long)]
        planned: String,
        /// The builds that have a block, as a JSON list of names.
        #[arg(long)]
        written: String,
    },
    /// The version writers the plan names.
    Writers {
        /// The plan `nebensound-repo plan` printed; empty when there is none.
        #[arg(long, value_name = "FILE")]
        plan: PathBuf,
        /// The version writers that have a block, as a JSON list of names.
        #[arg(long)]
        written: String,
    },
    /// The publish targets the plan names.
    Targets {
        /// The plan `nebensound-repo plan` printed; empty when there is none.
        #[arg(long, value_name = "FILE")]
        plan: PathBuf,
        /// The publish targets that have a block, as a JSON list of names.
        #[arg(long)]
        written: String,
    },
}

/// Which closing job `gather` speaks for, with the results it is handed.
#[cfg(feature = "program")]
#[derive(Debug, Clone, Subcommand)]
pub enum Gather {
    /// The closing job Check.
    Checks {
        /// How the job Declare ended.
        #[arg(long, value_enum)]
        declare: gather::Outcome,
        /// How the jobs of the units ended.
        #[arg(long, value_enum)]
        checks: gather::Outcome,
    },
    /// The closing job Build.
    Builds {
        /// How the job Declare ended.
        #[arg(long, value_enum)]
        declare: gather::Outcome,
        /// How the closing job Check ended.
        #[arg(long, value_enum)]
        check: gather::Outcome,
        /// How the jobs of the builds ended.
        #[arg(long, value_enum)]
        builds: gather::Outcome,
        /// The units Declare handed the build matrix, as JSON.
        #[arg(long)]
        planned: String,
    },
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
    /// The build that was named is not one `build` carries out.
    #[cfg(feature = "program")]
    #[error("`build` carries out the build cargo-package; {build:?} is not one of them")]
    NotBuilt {
        /// What was named.
        build: String,
    },
    /// What the variables of the run hand in does not read.
    #[cfg(feature = "program")]
    #[error("{0}")]
    Run(String),
    /// The check that was named is not one `run` carries out.
    #[cfg(feature = "program")]
    #[error(
        "`run` carries out the checks css, dockerfile, html, json, markdown, python, rust, toml \
         and yaml; {check:?} is not one of them, and its building block runs it"
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
    run_with(
        args,
        register,
        defaults,
        output,
        &|name| std::env::var(name).ok(),
        out,
    )
}

/// The same with the variables of the process read through `variables`.
///
/// The calls only a workflow run makes read the run from its variables
/// ([`workflow`](mod@workflow)): what a step hands in, the files it writes to and the search
/// path of the programs it starts. This form stands beside the others so that
/// a test can hand in the run it needs without changing the variables every
/// test of the process shares.
pub fn run_with<I, T>(
    args: I,
    register: &str,
    defaults: &str,
    output: Output,
    variables: &dyn Fn(&str) -> Option<String>,
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
    dispatch(&cli, register, defaults, output, variables, out)
}

/// Everything after the arguments were read.
///
/// Not generic, unlike its caller: it is compiled once, so every test binary
/// runs the same copy of it, and a call that only one of them makes still
/// counts as run.
fn dispatch(
    cli: &Cli,
    register: &str,
    defaults: &str,
    output: Output,
    variables: &dyn Fn(&str) -> Option<String>,
    out: &mut dyn Write,
) -> u8 {
    // Every call of `run` or `build` outside a workflow run opens with the
    // line that the gate decides, also one that ends in a refusal or an error.
    #[cfg(feature = "program")]
    if matches!(
        cli.command,
        Some(Command::Run { .. } | Command::Build { .. })
    ) && output != Output::Workflow
    {
        say(out, &format!("{}\n", run::OUTSIDE));
    }
    // Whether a call makes sense does not depend on what it would have been
    // called on, so the path is checked before the repository is read at all.
    let job = match call(cli, variables) {
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
        #[cfg(feature = "program")]
        Ok(Call::Alone(alone)) => return alone_call(cli, &alone, output, variables, out),
        #[cfg(feature = "program")]
        Ok(Call::Declare(pull_request)) => Deferred::Declare(pull_request),
        Ok(Call::Repository(job)) => Deferred::Job(job),
        Err(failure) => {
            say(out, &format!("{failure}\n"));
            return MISUSE;
        }
    };
    let context = Execution {
        cli,
        register,
        defaults,
        output,
        variables,
    };
    let done = match &job {
        #[cfg(feature = "program")]
        Deferred::Declare(pull_request) => declare(&context, pull_request.as_ref(), out),
        Deferred::Job(job) => execute(&context, job, out),
    };
    match done {
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
    /// A call of a workflow run that needs no phase 0.
    #[cfg(feature = "program")]
    Alone(Alone),
    /// Run phase 0 as Declare does and hand on the plan, for the pull
    /// request if there is one.
    #[cfg(feature = "program")]
    Declare(Option<PullRequest>),
    /// Something about the repository.
    Repository(Job),
}

/// A call that reads the repository.
enum Deferred {
    /// `declare`, which reads it twice.
    #[cfg(feature = "program")]
    Declare(Option<PullRequest>),
    /// Every other one.
    Job(Job),
}

/// A call of a workflow run that needs no phase 0: it reads only what it is
/// handed, or files of the repository it names itself.
#[cfg(feature = "program")]
enum Alone {
    /// Fail for what has no block yet.
    Unwritten(Unwritten),
    /// Turn the results of the jobs a closing job waits for into one.
    Gather(Gather),
    /// Say why a run after a push to this pre-branch resolves no version.
    PreBranch(String),
    /// Run the repository's xtask.
    Xtask,
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
    /// Write the records of this commit's versions.
    #[cfg(feature = "program")]
    Release,
    /// Run a build of a unit.
    #[cfg(feature = "program")]
    Build {
        build: build::Build,
        unit: RelativePath,
    },
    /// Report the versions as the job Version does, for the pull request if
    /// there is one.
    #[cfg(feature = "program")]
    ReportVersions(Option<PullRequest>),
    /// Open the interface.
    #[cfg(feature = "ui")]
    Ui,
}

/// What the call asks for, refusing a path that is not one.
fn call(cli: &Cli, variables: &dyn Fn(&str) -> Option<String>) -> Result<Call, Failure> {
    #[cfg(not(feature = "program"))]
    let _ = variables;
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
        #[cfg(feature = "program")]
        Some(Command::Release) => Job::Release,
        #[cfg(feature = "program")]
        Some(Command::Build { build, unit }) => Job::Build {
            build: build::Build::of(build).ok_or_else(|| Failure::NotBuilt {
                build: build.clone(),
            })?,
            unit: path_of(unit)?,
        },
        #[cfg(feature = "program")]
        Some(Command::Declare) => {
            return Ok(Call::Declare(
                workflow::pull_request(variables).map_err(Failure::Run)?,
            ));
        }
        #[cfg(feature = "program")]
        Some(Command::ReportVersions) => {
            if workflow::pre_branch(variables) {
                return Ok(Call::Alone(Alone::PreBranch(
                    variables("GITHUB_REF_NAME").unwrap_or_default(),
                )));
            }
            Job::ReportVersions(workflow::pull_request(variables).map_err(Failure::Run)?)
        }
        #[cfg(feature = "program")]
        Some(Command::Unwritten { what }) => {
            return Ok(Call::Alone(Alone::Unwritten(what.clone())));
        }
        #[cfg(feature = "program")]
        Some(Command::Gather { what }) => return Ok(Call::Alone(Alone::Gather(what.clone()))),
        #[cfg(feature = "program")]
        Some(Command::Xtask) => return Ok(Call::Alone(Alone::Xtask)),
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
    let pull_request = match job {
        Job::Plan(pull_request) => pull_request.as_ref(),
        _ => versions_of(job),
    };
    match pull_request {
        Some(pull_request) => {
            Repository::at(&cli.repository).as_pull_request(pull_request.message.clone())
        }
        None => Repository::at(&cli.repository),
    }
}

/// The pull request a call reports the versions for, if it is one that does.
fn versions_of(job: &Job) -> Option<&PullRequest> {
    match job {
        Job::Version { pull_request, .. } => pull_request.as_ref(),
        #[cfg(feature = "program")]
        Job::ReportVersions(pull_request) => pull_request.as_ref(),
        _ => None,
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
        #[cfg(feature = "program")]
        Job::Build { .. } => true,
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
    if let Job::Check(_, Some(visibility)) = job {
        return Ok(analysis.with_visibility(*visibility));
    }
    if let Some(pull_request) = versions_of(job)
        && !analysis.failed()
    {
        let found = requirements::findings(repository, &analysis, central, pull_request.number)?;
        analysis.loose.extend(found);
    }
    Ok(analysis)
}

/// What every call about the repository runs with.
struct Execution<'a> {
    cli: &'a Cli,
    register: &'a str,
    defaults: &'a str,
    output: Output,
    #[cfg_attr(not(feature = "program"), allow(dead_code))]
    variables: &'a dyn Fn(&str) -> Option<String>,
}

/// What a call says instead of its answer when phase 0 found errors, if it
/// gives none then.
///
/// A plan read off a declaration with errors would start the wrong things
/// with a straight face, so there is none; what is printed then is a
/// sentence, which no matrix takes for a plan. The same holds for the rest: a
/// file list would check the wrong files, a package could be left out without
/// a valid reason, a version could be written into the wrong package or be
/// one the history does not give, the programs could be those of the wrong
/// checks, and a build could be that of the wrong unit.
fn refusal(job: &Job) -> Option<&'static str> {
    Some(match job {
        Job::Plan(_) => "there is no plan: phase 0 found errors, and `check` says which\n",
        Job::Files { .. } => {
            "there is no file list: phase 0 found errors, and `check` says which\n"
        }
        Job::ExcludedPackages => {
            "there is no list of excluded packages: phase 0 found errors, and `check` says which\n"
        }
        Job::WriteVersions(_) => {
            "no version is written: phase 0 found errors, and `check` says which\n"
        }
        #[cfg(feature = "program")]
        Job::Setup(..) => {
            "there is nothing to set up: phase 0 found errors, and `check` says which\n"
        }
        #[cfg(feature = "program")]
        Job::Run { .. } => "nothing is checked: phase 0 found errors, and `check` says which\n",
        #[cfg(feature = "program")]
        Job::Release => "nothing is released: phase 0 found errors, and `check` says which\n",
        #[cfg(feature = "program")]
        Job::Build { .. } => "nothing is built: phase 0 found errors, and `check` says which\n",
        _ => return None,
    })
}

/// The repository a call reads, read: the repository, the central data and
/// phase 0 on it.
fn read(context: &Execution<'_>, job: &Job) -> Result<(Repository, Central, Analysis), Failure> {
    let repository = repository_of(context.cli, job);
    let central = central_from(context.register, context.defaults)?;
    let analysis = analysis_for(&repository, &central, job)?;
    Ok((repository, central, analysis))
}

fn execute(context: &Execution<'_>, job: &Job, out: &mut dyn Write) -> Result<u8, Failure> {
    let output = context.output;
    let (repository, central, mut analysis) = read(context, job)?;
    if analysis.failed()
        && let Some(refusal) = refusal(job)
    {
        say(out, refusal);
        return Ok(FOUND);
    }

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
        Job::Plan(pull_request) => say(out, &plan_of(&analysis, &central, pull_request.as_ref())),
        Job::Files { check, unit, zero } => match files::files(&analysis, &central, check, unit) {
            Ok(list) => say(out, &files::render(&list, *zero)),
            Err(refusal) => {
                say(out, &format!("{refusal}\n"));
                return Ok(FOUND);
            }
        },
        Job::ExcludedPackages => say(
            out,
            &format!(
                "{}\n",
                packages::to_json(&repository.excluded_packages(&analysis))
            ),
        ),
        Job::WriteVersions(writing) => say(
            out,
            &write_versions(&repository, &analysis, &central, writing)?,
        ),
        // A version read off a declaration with errors could be one the
        // history does not give, so what is printed then is every finding.
        Job::Version {
            markdown,
            pull_request,
        } => say(
            out,
            &versions::report(&analysis, *markdown, pull_request.as_ref()),
        ),
        #[cfg(feature = "program")]
        Job::Setup(options, named) => {
            return Ok(set_up(&analysis, &central, *options, named, out));
        }
        #[cfg(feature = "program")]
        Job::Run { check, unit, plan } => {
            let orders = orders(plan.as_ref(), &analysis, &central)?;
            let checked = Checked {
                repository: &repository,
                analysis: &analysis,
                central: &central,
                orders: &orders,
            };
            return Ok(run_check(context.cli, &checked, *check, unit, output, out));
        }
        #[cfg(feature = "program")]
        Job::Release => return release_stage(&repository, &analysis, output, out),
        #[cfg(feature = "program")]
        Job::Build { build, unit } => {
            let at = (&repository, &analysis, &central);
            return Ok(build_unit(context, at, *build, unit, out));
        }
        #[cfg(feature = "program")]
        Job::ReportVersions(pull_request) => {
            return Ok(report_versions(
                context,
                &analysis,
                pull_request.as_ref(),
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

/// `release`: the release stage on the commit at hand, with the settings the
/// variables of the workflow run give. Without them the call is wrong.
#[cfg(feature = "program")]
fn release_stage(
    repository: &Repository,
    analysis: &Analysis,
    output: Output,
    out: &mut dyn Write,
) -> Result<u8, Failure> {
    let settings = match release::Settings::read(
        &|name| std::env::var(name).ok(),
        std::env::var_os("PATH").unwrap_or_default(),
    ) {
        Ok(settings) => settings,
        Err(message) => {
            say(out, &format!("{message}\n"));
            return Ok(MISUSE);
        }
    };
    let records = repository.records()?;
    Ok(release::run(
        &analysis.versions,
        &records,
        &settings,
        output == Output::Workflow,
        out,
    ))
}

/// The search path of the programs a call starts: the `PATH` among the
/// variables of the run.
#[cfg(feature = "program")]
fn search_path(variables: &dyn Fn(&str) -> Option<String>) -> OsString {
    variables("PATH").unwrap_or_default().into()
}

/// A call of a workflow run that needs no phase 0.
#[cfg(feature = "program")]
fn alone_call(
    cli: &Cli,
    alone: &Alone,
    output: Output,
    variables: &dyn Fn(&str) -> Option<String>,
    out: &mut dyn Write,
) -> u8 {
    let mut log = workflow::Log::new(out, output == Output::Workflow);
    let passed = match alone {
        Alone::Unwritten(what) => match unwritten_call(what, &mut log) {
            Ok(passed) => passed,
            Err(message) => {
                log.error(&message);
                return MISUSE;
            }
        },
        Alone::Gather(Gather::Checks { declare, checks }) => {
            gather::checks(*declare, *checks, &mut log)
        }
        Alone::Gather(Gather::Builds {
            declare,
            check,
            builds,
            planned,
        }) => gather::builds(
            &gather::Builds {
                declare: *declare,
                check: *check,
                builds: *builds,
                planned,
                pre_branch: workflow::pre_branch(variables),
            },
            &mut log,
        ),
        Alone::PreBranch(branch) => {
            let line = format!(
                "This run follows a push to the pre-branch `{branch}`. Versions and pre-releases \
                 of pre-branches are not built yet, so no version is resolved, nothing is built \
                 and nothing is published."
            );
            log.line(&workflow::notice("Pre-branch", &line));
            if let Err(message) = workflow::summary(variables, &format!("{line}\n")) {
                log.error(&message);
                return FOUND;
            }
            true
        }
        Alone::Xtask => xtask::run(&cli.repository, &search_path(variables), &mut log),
    };
    if passed { OK } else { FOUND }
}

/// `unwritten`: whether everything the run asks for has a block, or why the
/// call does not read.
#[cfg(feature = "program")]
fn unwritten_call(what: &Unwritten, log: &mut workflow::Log<'_>) -> Result<bool, String> {
    let plan_of = |file: &PathBuf| {
        fs::read_to_string(file)
            .map_err(|error| format!("the plan {} does not read: {error}", file.display()))
    };
    Ok(match what {
        Unwritten::Checks {
            unit,
            planned,
            written,
        } => unwritten::checks(
            unit,
            &unwritten::names(planned, "the checks of the unit")?,
            &unwritten::names(written, "the list of written checks")?,
            log,
        ),
        Unwritten::Build {
            unit,
            planned,
            written,
        } => unwritten::build(
            unit,
            planned,
            &unwritten::names(written, "the list of written builds")?,
            log,
        ),
        Unwritten::Writers { plan, written } => unwritten::named(
            &plan_of(plan)?,
            unwritten::Named::Writers,
            &unwritten::names(written, "the list of written version writers")?,
            log,
        )?,
        Unwritten::Targets { plan, written } => unwritten::named(
            &plan_of(plan)?,
            unwritten::Named::Targets,
            &unwritten::names(written, "the list of written publish targets")?,
            log,
        )?,
    })
}

/// `declare`: phase 0 with what the run knows of the repository, then, when
/// it passed, the plan and the matrices handed on.
#[cfg(feature = "program")]
fn declare(
    context: &Execution<'_>,
    pull_request: Option<&PullRequest>,
    out: &mut dyn Write,
) -> Result<u8, Failure> {
    let run = match declare::Run::read(context.variables) {
        Ok(run) => run,
        Err(message) => {
            say(out, &format!("{}\n", report::shown(&message)));
            return Ok(FOUND);
        }
    };
    let given = IdentityArgs {
        name: run.name.clone(),
        default_branch: run.default_branch.clone(),
    };
    let checked = Job::Check(given.clone(), Some(run.visibility));
    let planned = Job::Plan(pull_request.cloned());
    // Phase 0 as `check` runs it, its report said as soon as it is there, and
    // when it passed the repository read for the plan. One chain, so that a
    // repository that cannot be read stops the call wherever it is found; a
    // read for the plan that fails still leaves the report in the log.
    let read_for_plan = read(context, &checked)
        .and_then(|(repository, central, mut analysis)| {
            check(&repository, &mut analysis, &central, &given, context.output).map(|report| {
                say(out, &report);
                analysis.failed()
            })
        })
        .and_then(|failed| {
            if failed {
                Ok(None)
            } else {
                read(context, &planned).map(Some)
            }
        })?;
    let Some((repository, central, analysis)) = read_for_plan else {
        return Ok(FOUND);
    };
    if analysis.failed() {
        say(
            out,
            "there is no plan: phase 0 found errors, and `check` says which\n",
        );
        return Ok(FOUND);
    }
    let mut plan = plan::plan(&analysis, &central);
    plan.pull_request = pull_request.map(|pull_request| pull_request.number);
    let excluded = repository.excluded_packages(&analysis);
    let mut log = workflow::Log::new(out, context.output == Output::Workflow);
    Ok(
        if declare::hand_on(&plan, &run, &excluded, context.variables, &mut log) {
            OK
        } else {
            FOUND
        },
    )
}

/// `build`: the build of the unit, when the plan starts it there.
#[cfg(feature = "program")]
fn build_unit(
    context: &Execution<'_>,
    (repository, analysis, central): (&Repository, &Analysis, &Central),
    which: build::Build,
    unit: &RelativePath,
    out: &mut dyn Write,
) -> u8 {
    let plan = plan::plan(analysis, central);
    let started = plan.units.iter().any(|planned| {
        planned.path == *unit
            && planned
                .build
                .as_ref()
                .is_some_and(|name| name.as_str() == which.name())
    });
    if !started {
        say(
            out,
            &format!(
                "'{}' does not build {}: the plan starts no such build there\n",
                report::shown(&unit.to_string()),
                which.name()
            ),
        );
        return FOUND;
    }
    // The repository was read, so it is there.
    let root = fs::canonicalize(&context.cli.repository).unwrap_or(context.cli.repository.clone());
    let search_path = search_path(context.variables);
    let settings = build::Context {
        repository,
        root: &root,
        search_path: &search_path,
        temp: &std::env::temp_dir(),
        workflow: context.output == Output::Workflow,
        variables: context.variables,
    };
    build::run(&settings, which, unit, out)
}

/// `report-versions`: the table into the log and as Markdown into the
/// summary of the run. With phase 0 errors both say which, as for `version`,
/// and the step fails.
#[cfg(feature = "program")]
fn report_versions(
    context: &Execution<'_>,
    analysis: &Analysis,
    pull_request: Option<&PullRequest>,
    out: &mut dyn Write,
) -> u8 {
    say(out, &versions::report(analysis, false, pull_request));
    let markdown = versions::report(analysis, true, pull_request);
    if let Err(message) = workflow::summary(context.variables, &markdown) {
        say(out, &format!("{}\n", report::shown(&message)));
        return FOUND;
    }
    if analysis.failed() { FOUND } else { OK }
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
            .and_then(|text| {
                // A step that was handed no plan hands on an empty one.
                if text.trim().is_empty() {
                    return Err("it is empty, so no plan was handed in".to_owned());
                }
                writer::Order::read(&text).map_err(|error| error.to_string())
            })
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
