//! Runs an action the way the runner of GitHub Actions runs it, on this
//! machine.
//!
//! The steps of a composite action run in order. Each `if:` is decided, every
//! `${{ … }}` is filled in, each `run:` goes to bash as the runner hands it
//! over (`bash --noprofile --norc -eo pipefail`), and the file commands a step
//! writes (`GITHUB_OUTPUT`, `GITHUB_ENV`, `GITHUB_PATH`) are read back
//! afterwards. A step that calls another composite action runs that action's
//! steps the same way, fetched at the ref the step names; inside it,
//! `success()` and `failure()` look at that action's own steps. So the xtask
//! runs the blocks' own steps, read from their `action.yml`, and keeps no copy
//! of them that could drift.
//!
//! An action that runs on Node.js cannot run here. The few the blocks call
//! have a stand-in below; any other one is refused by name.
//!
//! What a step prints is shown with `    | ` in front. A block announces its
//! findings as workflow commands (`::error file=…::…`); in the gate, where the
//! xtask itself runs as a step, those lines would otherwise become
//! annotations for mistakes a failing case makes on purpose.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead as _, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::action::{Action, Kind, Runs, Step};
use crate::expr::{self, Scope, Value};
use crate::{Error, say};

/// Variables of this process that the tool `setup` builds does not get.
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

/// Prefixes of the variables of this process that the tool `setup` builds
/// does not get, for the same reason.
const CARGO_SETS_PREFIXES: &[&str] = &["CARGO_PKG_", "CARGO_BIN_EXE_"];

/// The variables of this process a step gets; every other one is left out.
///
/// A step on a runner starts from the runner's environment, not from a
/// developer's shell, so a variable that steers a tool there
/// (`CARGO_TERM_COLOR`, `RUSTFLAGS`, `NODE_OPTIONS`, `PYTHONPATH`, `GOFLAGS`,
/// `RUSTUP_TOOLCHAIN`, …) would change what a block prints here and not in
/// the gate. Only what a runner step has too is handed on: where the user's
/// files, toolchains and caches lie, the locale, and how to reach the
/// network.
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

/// Prefixes of the variables of this process a step gets, for the same
/// reason: the locale and where the user's files lie.
const PASSED_ON_PREFIXES: &[&str] = &["LC_", "XDG_"];

/// Default variables the runner gives every step and a block's action reads,
/// each with its name in the `github` context, set the same way locally and in
/// the gate: lychee's action builds a link to the run from the repository and
/// the run's ID, under `set -u` from the variables and in an expression from
/// the context. A case is no repository on GitHub and runs in no workflow
/// run, so these name none.
const RUN: &[(&str, &str, &str)] = &[
    ("GITHUB_REPOSITORY", "repository", "xtask/case"),
    ("GITHUB_REPOSITORY_OWNER", "repository_owner", "xtask"),
    ("GITHUB_RUN_ID", "run_id", "0"),
    ("GITHUB_RUN_NUMBER", "run_number", "0"),
    ("GITHUB_RUN_ATTEMPT", "run_attempt", "1"),
];

/// The `job` context, as far as an action the blocks call reads it: lychee's
/// action links the run's summary to the job's check run. A case runs in no
/// job on GitHub, so it names none. Any other name of `job` is refused, as is
/// any name of `github` and `runner` the xtask does not set: on GitHub it
/// would have a value, and reading it as empty would run a block one way here
/// and another way for every caller. The one exception is
/// `github.event.pull_request`: a case is no pull request, and on GitHub every
/// field of it is empty in a run that is none, such as a push to `main`.
const JOB: &[(&str, &str)] = &[("check_run_id", "0")];

/// What stands in for an action that runs on Node.js.
#[derive(Clone, Copy, Debug)]
enum StandIn {
    /// Nothing needs to happen here; the text says why.
    Nothing(&'static str),
    /// The action would put a tool of a given version on the PATH. Here the
    /// tool has to be on the PATH already, in that version.
    Version {
        /// The tool.
        program: &'static str,
        /// What makes it print its version.
        argument: &'static str,
        /// The input of the action that names the version.
        input: &'static str,
        /// What stands before the version in what the tool prints.
        prefix: &'static str,
    },
    /// The action would upload the files its input `path` names as an
    /// artifact of the run. Here nothing leaves the machine; each line of the
    /// input has to name a file or folder that exists, as the action with
    /// `if-no-files-found: error` demands.
    Upload,
}

/// The actions on Node.js the blocks call, each with what stands in for it.
const STAND_INS: &[(&str, StandIn)] = &[
    (
        "actions/setup-node",
        StandIn::Version {
            program: "node",
            argument: "--version",
            input: "node-version",
            prefix: "v",
        },
    ),
    (
        "actions/setup-go",
        StandIn::Version {
            program: "go",
            argument: "version",
            input: "go-version",
            prefix: "go",
        },
    ),
    (
        "Swatinem/rust-cache",
        StandIn::Nothing("a cache: without it Cargo builds the same, only from scratch"),
    ),
    (
        "actions/cache/restore",
        StandIn::Nothing("a cache: without it the step it would spare runs"),
    ),
    (
        "actions/cache/save",
        StandIn::Nothing("a cache: nothing is kept for a later run"),
    ),
    ("actions/upload-artifact", StandIn::Upload),
];

/// A program a block's step expects on the `PATH` in a version, where on
/// GitHub an action the step calls would put it there and the xtask stands in
/// with the one on the `PATH` (see [`needs`]).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Need {
    /// The program.
    pub program: &'static str,
    /// The version the step asks for.
    pub version: String,
    argument: &'static str,
    prefix: &'static str,
}

/// What the steps of `action` expect on the `PATH`: every program an action
/// they call would put there, with the version the step names. A version
/// written as an expression is known only when the step runs, so it is left
/// out; the blocks write none.
#[must_use]
pub fn needs(action: &Action) -> Vec<Need> {
    let Runs::Composite(steps) = &action.runs else {
        return Vec::new();
    };
    steps
        .iter()
        .filter_map(|step| match &step.kind {
            Kind::Uses { action, with } => Some((action, with)),
            Kind::Run { .. } => None,
        })
        .filter_map(|(reference, with)| {
            let name = reference
                .split_once('@')
                .map_or(reference.as_str(), |(name, _)| name);
            STAND_INS
                .iter()
                .find_map(|(action, stand_in)| match stand_in {
                    StandIn::Version {
                        program,
                        argument,
                        input,
                        prefix,
                    } if *action == name => with
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case(input))
                        .filter(|(_, version)| !version.contains("${{"))
                        .map(|(_, version)| Need {
                            program,
                            version: version.clone(),
                            argument,
                            prefix,
                        }),
                    _ => None,
                })
        })
        .collect()
}

/// Asks the program `need` names on the `PATH` of `env` for its version. `Ok`
/// carries what it printed when that is the version the step asks for, `Err`
/// what it printed otherwise, or `none` when it printed nothing or cannot be
/// started.
///
/// # Errors
///
/// As described above.
pub fn probe(need: &Need, env: &[(OsString, OsString)]) -> Result<String, String> {
    let printed = Command::new(need.program)
        .arg(need.argument)
        .env_clear()
        .envs(env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    if fits(&printed, need.prefix, &need.version) {
        Ok(printed)
    } else if printed.is_empty() {
        Err("none".to_owned())
    } else {
        Err(printed)
    }
}

/// Whether a word of `printed` is `prefix` and then `wanted`, alone or followed
/// by more parts after a dot: `v24.3.0` fits `24` with the prefix `v`, and
/// `v240.1` does not.
fn fits(printed: &str, prefix: &str, wanted: &str) -> bool {
    printed.split_whitespace().any(|word| {
        word.strip_prefix(prefix)
            .and_then(|version| version.strip_prefix(wanted))
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    })
}

/// The machine the actions run on.
#[derive(Debug)]
pub struct Host {
    /// The environment every step starts from (see [`environment`]).
    pub env: Vec<(OsString, OsString)>,
    /// Where an action `owner/repo@ref` is fetched from: `https://github.com`.
    pub origin: String,
    /// Where fetched actions are kept during the run.
    pub store: PathBuf,
    /// Where `cargo install` in a step installs to (`CARGO_INSTALL_ROOT`),
    /// its `bin` ahead of the inherited `PATH`, so that the cases leave the
    /// programs the developer installed as they are.
    pub installs: PathBuf,
    /// `runner.os`.
    pub os: String,
    /// `runner.arch`.
    pub arch: String,
    /// The bash a `run:` step goes to.
    pub bash: OsString,
    /// The git that fetches actions.
    pub git: OsString,
}

/// What lasts from one step to the next, as within one job.
#[derive(Clone, Debug, Default)]
pub struct Job {
    /// Variables a step added through `GITHUB_ENV`, in order.
    pub env: Vec<(String, String)>,
    /// Directories steps added through `GITHUB_PATH`, the latest first.
    pub path: Vec<String>,
}

/// Where an action runs.
#[derive(Clone, Copy, Debug)]
pub struct Place<'a> {
    /// `GITHUB_WORKSPACE`: the checkout, where every step starts.
    pub workspace: &'a Path,
    /// `RUNNER_TEMP`.
    pub temp: &'a Path,
}

/// How an action or a step went.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Whether it succeeded.
    pub success: bool,
    /// Its outputs, by name in lowercase.
    pub outputs: BTreeMap<String, String>,
    /// Every line it printed.
    pub log: Vec<String>,
}

/// The environment a step starts from: the variables of this process that
/// `PASSED_ON` names, and no other: what a runner step has too.
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

/// What a step was run with, besides itself.
struct Frame<'a> {
    context: &'a Context<'a>,
    place: Place<'a>,
    dir: &'a Path,
}

impl Host {
    /// Runs the action in `dir` with the given inputs. A step named in `skip`
    /// is left out; each of them has to be a step of the action.
    ///
    /// The result says whether the action succeeded. An error says that it
    /// could not be run at all: it is no action the xtask can run, or a step
    /// asks for something the xtask does not do.
    ///
    /// # Errors
    ///
    /// As described above.
    pub fn run(
        &self,
        job: &mut Job,
        dir: &Path,
        given: &BTreeMap<String, String>,
        place: Place<'_>,
        skip: &[&str],
        out: &mut dyn Write,
    ) -> Result<Outcome, Error> {
        let action = Action::load(dir)?;
        let steps = match &action.runs {
            Runs::Composite(steps) => steps,
            Runs::Other(using) => {
                return Err(Error::new(format!(
                    "the action in {} runs on `{using}`; the xtask runs composite actions, \
                     and stands in for a few others",
                    dir.display()
                )));
            }
        };
        if let Some(missing) = skip.iter().find(|name| {
            !steps
                .iter()
                .any(|step| step.name.as_deref() == Some(**name))
        }) {
            return Err(Error::new(format!(
                "the xtask leaves out the step `{missing}` of the action in {}, \
                 and it has no such step any more",
                dir.display()
            )));
        }
        let github = self.github(place, dir);
        let runner = self.runner(place);
        let records = BTreeMap::new();
        let none = BTreeMap::new();
        let base = Context {
            inputs: &none,
            steps: &records,
            github: &github,
            runner: &runner,
            failed: false,
        };
        let inputs = inputs(&action, given, &base, dir)?;

        let mut records = BTreeMap::new();
        let mut failed = false;
        let mut log = Vec::new();
        for step in steps {
            let context = Context {
                inputs: &inputs,
                steps: &records,
                github: &github,
                runner: &runner,
                failed,
            };
            let frame = Frame {
                context: &context,
                place,
                dir,
            };
            let (result, outputs) = match self.step(job, step, &frame, skip, out)? {
                Some(outcome) => {
                    log.extend(outcome.log);
                    failed |= !outcome.success;
                    let result = if outcome.success {
                        "success"
                    } else {
                        "failure"
                    };
                    (result, outcome.outputs)
                }
                None => ("skipped", BTreeMap::new()),
            };
            if let Some(id) = &step.id {
                records.insert(id.to_ascii_lowercase(), Record { result, outputs });
            }
        }

        let context = Context {
            inputs: &inputs,
            steps: &records,
            github: &github,
            runner: &runner,
            failed,
        };
        let outputs = action
            .outputs
            .iter()
            .map(|(name, value)| {
                Ok((
                    name.to_ascii_lowercase(),
                    expr::interpolate(value, &context)?,
                ))
            })
            .collect::<Result<_, Error>>()?;
        Ok(Outcome {
            success: !failed,
            outputs,
            log,
        })
    }

    /// Runs one step, unless it is left out or its condition says no.
    fn step(
        &self,
        job: &mut Job,
        step: &Step,
        frame: &Frame<'_>,
        skip: &[&str],
        out: &mut dyn Write,
    ) -> Result<Option<Outcome>, Error> {
        let title = step.title();
        if step
            .name
            .as_deref()
            .is_some_and(|name| skip.contains(&name))
        {
            say(out, &format!("  - {title}: left out by the xtask\n"));
            return Ok(None);
        }
        let runs = match &step.condition {
            Some(condition) => expr::condition(condition, frame.context)?,
            None => !frame.context.failed,
        };
        if !runs {
            say(out, &format!("  - {title}: skipped\n"));
            return Ok(None);
        }
        say(out, &format!("  > {title}\n"));
        let outcome = match &step.kind {
            Kind::Run { script, shell } => {
                self.script(job, step, script, shell.as_deref(), frame, out)?
            }
            Kind::Uses { action, with } => self.uses(job, action, with, frame, out)?,
        };
        if !outcome.success {
            say(out, &format!("  x {title}: failed\n"));
        }
        Ok(Some(outcome))
    }

    fn github(&self, place: Place<'_>, dir: &Path) -> BTreeMap<&'static str, String> {
        let mut github = BTreeMap::from([
            ("workspace", place.workspace.display().to_string()),
            ("action_path", dir.display().to_string()),
            // No step is handed a token: the blocks need none, and nothing a
            // case holds should run with one.
            ("token", String::new()),
            ("server_url", self.origin.clone()),
        ]);
        github.extend(
            RUN.iter()
                .map(|(_, name, value)| (*name, (*value).to_owned())),
        );
        github
    }

    fn runner(&self, place: Place<'_>) -> BTreeMap<&'static str, String> {
        BTreeMap::from([
            ("temp", place.temp.display().to_string()),
            ("os", self.os.clone()),
            ("arch", self.arch.clone()),
        ])
    }

    /// A command that runs in the environment a step of the action in `dir`
    /// gets, with the file commands in `files`.
    fn command(
        &self,
        program: &OsString,
        job: &Job,
        place: Place<'_>,
        dir: &Path,
        files: &Path,
    ) -> Command {
        let mut command = Command::new(program);
        command
            .env_clear()
            .envs(self.env.iter().map(|(name, value)| (name, value)));
        command.envs(job.env.iter().map(|(name, value)| (name, value)));
        let inherited = self
            .env
            .iter()
            .find(|(name, _)| name == "PATH")
            .map(|(_, value)| value.to_string_lossy().into_owned());
        let installed = self.installs.join("bin").display().to_string();
        let path: Vec<String> = job
            .path
            .iter()
            .cloned()
            .chain(std::iter::once(installed))
            .chain(inherited)
            .collect();
        command
            .env("PATH", path.join(":"))
            .env("CARGO_INSTALL_ROOT", &self.installs)
            .envs(RUN.iter().map(|(variable, _, value)| (variable, value)))
            .env("CI", "true")
            .env("GITHUB_ACTIONS", "true")
            .env("GITHUB_SERVER_URL", &self.origin)
            .env("GITHUB_WORKSPACE", place.workspace)
            .env("GITHUB_ACTION_PATH", dir)
            .env("RUNNER_TEMP", place.temp)
            .env("RUNNER_OS", &self.os)
            .env("RUNNER_ARCH", &self.arch)
            .env("GITHUB_OUTPUT", files.join("output"))
            .env("GITHUB_ENV", files.join("env"))
            .env("GITHUB_PATH", files.join("path"))
            .env("GITHUB_STATE", files.join("state"))
            .env("GITHUB_STEP_SUMMARY", files.join("summary"))
            .current_dir(place.workspace);
        command
    }

    fn script(
        &self,
        job: &mut Job,
        step: &Step,
        script: &str,
        shell: Option<&str>,
        frame: &Frame<'_>,
        out: &mut dyn Write,
    ) -> Result<Outcome, Error> {
        if shell != Some("bash") {
            return Err(Error::new(format!(
                "the step `{}` of the action in {} runs on the shell {}; the xtask runs bash",
                step.title(),
                frame.dir.display(),
                shell.unwrap_or("of the runner's choice")
            )));
        }
        let env = pairs(&step.env, frame.context)?;
        let directory = match &step.working_directory {
            Some(directory) => frame
                .place
                .workspace
                .join(expr::interpolate(directory, frame.context)?),
            None => frame.place.workspace.to_path_buf(),
        };
        let body = expr::interpolate(script, frame.context)?;
        let files = tempfile::Builder::new()
            .prefix("step-")
            .tempdir_in(frame.place.temp)
            .and_then(|files| fs::write(files.path().join("script.sh"), body).map(|()| files))
            .map_err(|error| {
                Error::new(format!(
                    "cannot prepare a step in {}: {error}",
                    frame.place.temp.display()
                ))
            })?;

        let mut command = self.command(&self.bash, job, frame.place, frame.dir, files.path());
        command
            .args(["--noprofile", "--norc", "-eo", "pipefail"])
            .arg(files.path().join("script.sh"))
            .envs(env)
            .current_dir(directory);
        let (success, log) = stream(command, out)?;

        let read = |name: &str| fs::read_to_string(files.path().join(name)).unwrap_or_default();
        let outputs = file_commands(&read("output"), "GITHUB_OUTPUT")?
            .into_iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value))
            .collect();
        job.env.extend(file_commands(&read("env"), "GITHUB_ENV")?);
        for line in read("path").lines().filter(|line| !line.trim().is_empty()) {
            job.path.insert(0, line.to_owned());
        }
        Ok(Outcome {
            success,
            outputs,
            log,
        })
    }

    fn uses(
        &self,
        job: &mut Job,
        reference: &str,
        with: &[(String, String)],
        frame: &Frame<'_>,
        out: &mut dyn Write,
    ) -> Result<Outcome, Error> {
        let inputs: BTreeMap<String, String> = pairs(with, frame.context)?
            .into_iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value))
            .collect();
        let remote = reference.split_once('@').filter(|(name, _)| {
            !name.starts_with('.') && !name.contains(':') && !name.starts_with('$')
        });
        let Some((name, version)) = remote else {
            return Err(Error::new(format!(
                "the action `{reference}` is not one the xtask fetches: it fetches `owner/repo@ref`"
            )));
        };
        if let Some((_, stand_in)) = STAND_INS.iter().find(|(action, _)| *action == name) {
            return Ok(self.stand_in(*stand_in, reference, &inputs, job, frame, out));
        }
        let dir = self.fetch(name, version)?;
        self.run(job, &dir, &inputs, frame.place, &[], out)
    }

    /// The directory of the action `owner/repo[/path]` at `version`, fetched
    /// once per run. `version` is a branch, a tag or a full commit SHA, as a
    /// workflow may name it: the ref is fetched, which takes all three, where
    /// `git clone --branch` takes no SHA. A fetch that fails leaves nothing
    /// behind, so a later step does not take the empty checkout for the action.
    fn fetch(&self, name: &str, version: &str) -> Result<PathBuf, Error> {
        let mut parts = name.splitn(3, '/');
        let (owner, repository) = (parts.next().unwrap_or_default(), parts.next());
        let Some(repository) = repository else {
            return Err(Error::new(format!(
                "the action `{name}@{version}` names no repository"
            )));
        };
        let checkout = self.store.join(owner).join(repository).join(version);
        if !checkout.is_dir() {
            let git = |args: &[&str]| {
                let mut all: Vec<OsString> = vec!["-C".into(), checkout.clone().into()];
                all.extend(args.iter().map(OsString::from));
                crate::quietly(&self.git, &all)
            };
            let origin = format!("{}/{owner}/{repository}", self.origin);
            let fetched = crate::quietly(
                &self.git,
                &["init".into(), "--quiet".into(), checkout.clone().into()],
            )
            .and_then(|()| git(&["fetch", "--quiet", "--depth", "1", &origin, version]))
            .and_then(|()| git(&["checkout", "--quiet", "FETCH_HEAD"]));
            if let Err(error) = fetched {
                let _ = fs::remove_dir_all(&checkout);
                return Err(error);
            }
        }
        Ok(parts
            .next()
            .map_or(checkout.clone(), |path| checkout.join(path)))
    }

    fn stand_in(
        &self,
        stand_in: StandIn,
        reference: &str,
        inputs: &BTreeMap<String, String>,
        job: &Job,
        frame: &Frame<'_>,
        out: &mut dyn Write,
    ) -> Outcome {
        let mut line = |text: String, success: bool| {
            say(out, &format!("    | {text}\n"));
            Outcome {
                success,
                outputs: BTreeMap::new(),
                log: vec![text],
            }
        };
        match stand_in {
            StandIn::Nothing(why) => line(format!("{reference} is left out here: {why}"), true),
            StandIn::Version {
                program,
                argument,
                input,
                prefix,
            } => {
                let wanted = inputs.get(input).cloned().unwrap_or_default();
                let mut command = self.command(
                    &program.into(),
                    job,
                    frame.place,
                    frame.dir,
                    frame.place.temp,
                );
                command.arg(argument);
                let printed = command
                    .stderr(Stdio::null())
                    .output()
                    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
                    .unwrap_or_default();
                if fits(&printed, prefix, &wanted) {
                    line(
                        format!("{reference} stands in with `{program}` from the PATH: {printed}"),
                        true,
                    )
                } else {
                    line(
                        format!(
                            "{reference} would put {program} {wanted} on the PATH; the xtask \
                             takes the one on the PATH instead, and that is {:?}. Put {program} \
                             {wanted} on the PATH.",
                            if printed.is_empty() { "none" } else { &printed }
                        ),
                        false,
                    )
                }
            }
            StandIn::Upload => {
                let name = inputs.get("name").cloned().unwrap_or_default();
                let paths: Vec<&str> = inputs
                    .get("path")
                    .map(String::as_str)
                    .unwrap_or_default()
                    .lines()
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
                    .collect();
                let missing: Vec<&str> = paths
                    .iter()
                    .copied()
                    .filter(|path| !frame.place.workspace.join(path).exists())
                    .collect();
                if paths.is_empty() || !missing.is_empty() {
                    line(
                        format!(
                            "{reference} would upload the artifact `{name}`, and finds no file \
                             at {}",
                            if paths.is_empty() {
                                "all: no path is named".to_owned()
                            } else {
                                missing.join(", ")
                            }
                        ),
                        false,
                    )
                } else {
                    line(
                        format!(
                            "{reference} would upload the artifact `{name}`: {}; here it stays \
                             on this machine",
                            paths.join(", ")
                        ),
                        true,
                    )
                }
            }
        }
    }
}

/// Every pair with its value filled in.
fn pairs(
    pairs: &[(String, String)],
    context: &Context<'_>,
) -> Result<Vec<(String, String)>, Error> {
    pairs
        .iter()
        .map(|(name, value)| Ok((name.clone(), expr::interpolate(value, context)?)))
        .collect()
}

/// The inputs an action runs with: what was given, else the default, else
/// nothing. An input the action does not declare is an error.
fn inputs(
    action: &Action,
    given: &BTreeMap<String, String>,
    context: &Context<'_>,
    dir: &Path,
) -> Result<BTreeMap<String, String>, Error> {
    let given: BTreeMap<String, &String> = given
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value))
        .collect();
    if let Some(unknown) = given.keys().find(|name| {
        !action
            .inputs
            .iter()
            .any(|input| input.name.eq_ignore_ascii_case(name))
    }) {
        return Err(Error::new(format!(
            "the action in {} has no input `{unknown}`",
            dir.display()
        )));
    }
    action
        .inputs
        .iter()
        .map(|input| {
            let name = input.name.to_ascii_lowercase();
            let value = match (given.get(&name), &input.default) {
                (Some(value), _) => (*value).clone(),
                (None, Some(default)) => expr::interpolate(default, context)?,
                (None, None) => String::new(),
            };
            Ok((name, value))
        })
        .collect()
}

/// Starts a command with its standard output and error in one stream, shows
/// every line and keeps it, and says whether it succeeded.
fn stream(mut command: Command, out: &mut dyn Write) -> Result<(bool, Vec<String>), Error> {
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

/// The pairs a step wrote to `GITHUB_OUTPUT` or `GITHUB_ENV`: `name=value`,
/// or `name<<delimiter` with the value on the lines up to the delimiter.
///
/// # Errors
///
/// When a line is neither, or a delimiter is never reached.
pub fn file_commands(text: &str, what: &str) -> Result<Vec<(String, String)>, Error> {
    let mut found = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.is_empty() {
            continue;
        }
        let heredoc = match (line.find('='), line.find("<<")) {
            (Some(equals), Some(heredoc)) => heredoc < equals,
            (None, Some(_)) => true,
            _ => false,
        };
        if heredoc {
            let (name, delimiter) = line.split_once("<<").unwrap_or_default();
            let mut value: Vec<&str> = Vec::new();
            loop {
                match lines.next() {
                    Some(next) if next == delimiter => break,
                    Some(next) => value.push(next),
                    None => {
                        return Err(Error::new(format!(
                            "{what} opens `{name}` with the delimiter `{delimiter}` and never closes it"
                        )));
                    }
                }
            }
            found.push((name.to_owned(), value.join("\n")));
        } else if let Some((name, value)) = line.split_once('=') {
            found.push((name.to_owned(), value.to_owned()));
        } else {
            return Err(Error::new(format!(
                "a line of {what} reads neither `name=value` nor `name<<delimiter`: {line:?}"
            )));
        }
    }
    Ok(found)
}

/// The value a name of `github`, `runner` or `job` has, or the refusal when
/// the xtask sets none for it (see [`JOB`]).
fn known(value: Option<String>, path: &[String]) -> Result<String, Error> {
    value.ok_or_else(|| unknown(path))
}

fn unknown(path: &[String]) -> Error {
    Error::new(format!(
        "`{}` names no value the xtask knows",
        path.join(".")
    ))
}

/// What a step recorded under its `id`.
struct Record {
    result: &'static str,
    outputs: BTreeMap<String, String>,
}

/// What the expressions of one step can read.
struct Context<'a> {
    inputs: &'a BTreeMap<String, String>,
    steps: &'a BTreeMap<String, Record>,
    github: &'a BTreeMap<&'static str, String>,
    runner: &'a BTreeMap<&'static str, String>,
    failed: bool,
}

impl Scope for Context<'_> {
    fn lookup(&self, path: &[String]) -> Result<Value, Error> {
        let lowered: Vec<String> = path
            .iter()
            .map(|segment| segment.to_ascii_lowercase())
            .collect();
        let segments: Vec<&str> = lowered.iter().map(String::as_str).collect();
        let found = match segments.as_slice() {
            ["inputs", name] => self.inputs.get(*name).cloned(),
            ["steps", id, "outcome" | "conclusion"] => {
                self.steps.get(*id).map(|record| record.result.to_owned())
            }
            ["steps", id, "outputs", name] => self
                .steps
                .get(*id)
                .and_then(|record| record.outputs.get(*name).cloned()),
            // A case is no pull request, so it has none of its fields.
            ["github", "event", "pull_request", ..] => None,
            ["github", name] => Some(known(self.github.get(*name).cloned(), path)?),
            ["runner", name] => Some(known(self.runner.get(*name).cloned(), path)?),
            ["job", name] => Some(known(
                JOB.iter()
                    .find(|(known, _)| known == name)
                    .map(|(_, value)| (*value).to_owned()),
                path,
            )?),
            [context, ..] if ["inputs", "steps", "github", "runner", "job"].contains(context) => {
                return Err(unknown(path));
            }
            _ => {
                return Err(Error::new(format!(
                    "`{}` reads a context the xtask does not know",
                    path.join(".")
                )));
            }
        };
        Ok(found.map_or(Value::Null, Value::String))
    }

    fn failed(&self) -> bool {
        self.failed
    }
}
