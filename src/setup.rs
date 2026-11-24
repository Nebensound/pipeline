//! `nebensound-repo setup`: get a computer ready to run the checks of a
//! repository.
//!
//! The checks call external programs, which the register pins, each with the
//! runtime it needs, if any, and so do some builds. For the checks the
//! repository's declaration starts, or the checks and builds it is named,
//! `setup` finds every program:
//!
//! - By default it uses a program that is on the computer already and says
//!   where its version differs from the pinned one; it installs the pinned
//!   version of what is missing, and nothing that is present.
//! - With `--pinned` it installs the pinned version of every program into its
//!   own folder, whatever the computer has, and leaves the computer's programs
//!   alone. This is the mode the gate runs in: the blocks of the checks that
//!   run from the tool call it for their own check. A program without a build
//!   for this platform cannot be installed, so it fails here even when the
//!   computer has it.
//! - With `--check` it changes nothing and says what is missing or differs.
//!
//! Pinned copies go into the tool's folder in the user's cache directory, one
//! folder per program and version, and are verified by their digest before
//! anything of them is used. A runtime is installed only when it is missing
//! and someone agrees: in a terminal by answering yes, otherwise by passing
//! `--with-runtimes`. Without either, nothing at all is installed. A runtime
//! on the computer that is too old for a program that needs it counts as
//! missing; one that can run those programs is used whatever its version, in
//! every mode.
//!
//! A runtime is needed, and held against the lowest version a program on it
//! runs on, only for the programs this run installs on it or starts through
//! it: one that is to be installed, since its runtime's installer puts it in
//! place, and a pinned copy installed with npm, whose command is a script that
//! calls the runtime. A pinned copy installed with pip starts with the Python
//! of its own environment, and a program on the computer that the run leaves as
//! it is starts the way it was installed; neither of them needs the runtime.

pub mod fetch;
pub mod find;
pub mod install;
pub mod unpack;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufRead, IsTerminal as _, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Register;
use crate::names::{BuildName, CheckName, ProgramName, RuntimeName};
use crate::programs::{Download, Installer, Platform, Program, Runtime, at_least};

use self::find::{Found, MARK};
use self::unpack::Packing;
use crate::plan::Plan;

/// What `setup` is to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Use what the computer has, and install the pinned version of what it
    /// lacks.
    Missing,
    /// Install the pinned version of every program into the tool's folder.
    Pinned,
    /// Change nothing, and say what is missing or differs.
    Check,
}

/// How `setup` was called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// What to do.
    pub mode: Mode,
    /// Install a missing runtime without asking.
    pub with_runtimes: bool,
}

/// What `setup` works with: the computer, as data, so that a test can hand
/// in another one.
pub struct Environment {
    /// The computer's operating system and architecture as Rust names them,
    /// such as `macos-x86_64`.
    pub computer: String,
    /// The register's key for this computer, if it has one.
    pub platform: Option<Platform>,
    /// Whether the computer runs Windows, where files are found and installed
    /// differently.
    pub windows: bool,
    /// Where programs are looked for: the `PATH`.
    pub search_path: OsString,
    /// The tool's folder in the user's cache directory, if there is one.
    pub folder: Option<PathBuf>,
    /// The protocols a download may use, in the form curl's `--proto` takes.
    pub protocols: String,
    /// Where a yes or no comes from. [`None`] without a terminal: then
    /// nothing is asked.
    pub answers: Option<Box<dyn BufRead>>,
}

impl Environment {
    /// This computer, as the process sees it. A download may use HTTPS only,
    /// and a question is asked only when both standard input and standard
    /// output are a terminal.
    #[must_use]
    pub fn current() -> Self {
        let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
        let terminal = io::stdin().is_terminal() && io::stdout().is_terminal();
        let answers: Option<Box<dyn BufRead>> = if terminal {
            Some(Box::new(io::stdin().lock()))
        } else {
            None
        };
        Self {
            computer: format!("{os}-{arch}"),
            platform: Platform::of(os, arch),
            windows: cfg!(windows),
            search_path: std::env::var_os("PATH").unwrap_or_default(),
            folder: find::cache_home(os, &|name| std::env::var_os(name))
                .map(|home| home.join("nebensound-repo")),
            protocols: "=https".to_owned(),
            answers,
        }
    }
}

/// The checks a plan starts, each once, in name order.
#[must_use]
pub fn started(plan: &Plan) -> Vec<CheckName> {
    let mut checks: Vec<CheckName> = plan
        .units
        .iter()
        .flat_map(|unit| unit.checks.iter().cloned())
        .collect();
    checks.sort();
    checks.dedup();
    checks
}

/// The checks of the register these names name, each once, in name order;
/// or the first name the register does not know.
///
/// # Errors
///
/// Returns the first name that names no check of the register.
pub fn named(register: &Register, names: &[String]) -> Result<Vec<CheckName>, String> {
    let mut checks = Vec::new();
    for name in names {
        let Some(check) = register.check.keys().find(|known| known.as_str() == name) else {
            return Err(name.clone());
        };
        checks.push(check.clone());
    }
    checks.sort();
    checks.dedup();
    Ok(checks)
}

/// The builds of the register these names name, each once, in name order,
/// and the names left for the checks, in the order they came: those that name
/// no build, and those that name a check as well as a build (such as `typst`),
/// so that both are set up.
#[must_use]
pub fn builds_named(register: &Register, names: &[String]) -> (Vec<BuildName>, Vec<String>) {
    let mut builds = Vec::new();
    let mut rest = Vec::new();
    for name in names {
        let build = register.build.keys().find(|known| known.as_str() == name);
        if let Some(build) = build {
            builds.push(build.clone());
        }
        if build.is_none() || register.check.keys().any(|known| known.as_str() == name) {
            rest.push(name.clone());
        }
    }
    builds.sort();
    builds.dedup();
    (builds, rest)
}

/// Finds or installs the programs of these checks, prints what it did and
/// found, and returns the exit code: 0 when every program is there
/// afterwards, 1 otherwise.
pub fn run(
    register: &Register,
    checks: &[CheckName],
    options: Options,
    env: &mut Environment,
    out: &mut dyn Write,
) -> u8 {
    run_for(register, checks, &[], options, env, out)
}

/// Finds or installs the programs of these checks and builds, as [`run`]
/// does for checks alone.
pub fn run_for(
    register: &Register,
    checks: &[CheckName],
    builds: &[BuildName],
    options: Options,
    env: &mut Environment,
    out: &mut dyn Write,
) -> u8 {
    let mut setup = Setup {
        register,
        checks,
        builds,
        env,
        out,
    };
    match setup.go(options) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(failure) => {
            setup.say(&format!("{failure}\n"));
            1
        }
    }
}

/// One program the checks or builds call, and what there is of it.
struct Wanted<'a> {
    name: &'a ProgramName,
    program: &'a Program,
    /// The runtime it runs on, if it has one.
    runtime: Option<(&'a RuntimeName, &'a Runtime)>,
    /// Whether this run needs that runtime for it: see [`needs`].
    needs_runtime: bool,
    /// The checks and builds that call it.
    callers: Vec<String>,
    found: Found,
}

/// One runtime a program needs in this run, and what there is of it.
struct Needed<'a> {
    name: &'a RuntimeName,
    runtime: &'a Runtime,
    /// The highest of the lowest versions the programs that need it run on,
    /// with the first program that needs that version.
    minimum: Option<(&'a ProgramName, &'a str)>,
    found: Found,
}

/// What there is of every program, and of every runtime this run needs.
struct Survey<'a> {
    runtimes: Vec<Needed<'a>>,
    programs: Vec<Wanted<'a>>,
}

struct Setup<'a, 'e> {
    register: &'a Register,
    checks: &'a [CheckName],
    builds: &'a [BuildName],
    env: &'e mut Environment,
    out: &'e mut dyn Write,
}

impl<'a> Setup<'a, '_> {
    /// Writes, and stays quiet when the output is gone: a message about that
    /// would take the same road.
    fn say(&mut self, text: &str) {
        let _ = self.out.write_all(text.as_bytes());
    }

    /// Whether every program is there in the end.
    fn go(&mut self, options: Options) -> Result<bool, String> {
        let folder = self.env.folder.clone().ok_or_else(|| {
            "there is no cache directory to keep pinned programs in: HOME is not set (on \
             Windows, LOCALAPPDATA)"
                .to_owned()
        })?;
        let checks: Vec<String> = self.checks.iter().map(ToString::to_string).collect();
        let builds: Vec<String> = self.builds.iter().map(ToString::to_string).collect();
        let named = if builds.is_empty() {
            String::new()
        } else {
            format!("Builds: {}\n", list(&builds))
        };
        self.say(&format!(
            "Checks: {}\n{named}Folder: {}\n",
            list(&checks),
            folder.display()
        ));
        let survey = self.survey(options.mode, &[]);
        if survey.programs.is_empty() {
            let callers = match (checks.is_empty(), builds.is_empty()) {
                (_, true) => "These checks",
                (true, false) => "These builds",
                (false, false) => "These checks and builds",
            };
            self.say(&format!("\n{callers} call no program the register pins.\n"));
            return Ok(true);
        }
        if options.mode == Mode::Check {
            self.report(&survey);
            return Ok(complete(&survey));
        }

        let mut failed = false;
        // After the installs a program installed with pip no longer needs its
        // runtime; the report still names the runtime this run needed for it.
        let needed: Vec<&ProgramName> = survey
            .programs
            .iter()
            .filter(|wanted| wanted.needs_runtime)
            .map(|wanted| wanted.name)
            .collect();
        let programs: Vec<&Wanted> = survey
            .programs
            .iter()
            .filter(|wanted| installs(&wanted.found, options.mode))
            .collect();
        let runtimes: Vec<&Needed> = survey
            .runtimes
            .iter()
            .filter(|needed| needed.found.missing())
            .collect();
        if !runtimes.is_empty() && !self.agreed(&runtimes, &survey, &folder, options) {
            self.say("Nothing was installed.\n");
            return Ok(false);
        }
        for needed in &runtimes {
            if let Err(failure) = self.install_runtime(needed, &folder) {
                self.say(&format!(
                    "{} {} was not installed: {failure}\n",
                    needed.name, needed.runtime.version
                ));
                failed = true;
            }
        }
        for wanted in programs {
            if let Err(failure) = self.install_program(wanted, &folder) {
                self.say(&format!(
                    "{} {} was not installed: {failure}\n",
                    wanted.name, wanted.program.version
                ));
                failed = true;
            }
        }
        let survey = self.survey(options.mode, &needed);
        self.report(&survey);
        Ok(!failed && complete(&survey))
    }

    /// What there is of the programs and of the runtimes this run needs. A
    /// program in `needed` counts as needing its runtime, whatever is found
    /// for it now.
    fn survey(&self, mode: Mode, needed: &[&ProgramName]) -> Survey<'a> {
        let register = self.register;
        let mut wanted = register.programs_of(self.checks);
        for build in self.builds {
            let programs = register
                .build
                .get(build)
                .map(|definition| definition.programs.as_slice())
                .unwrap_or_default();
            wanted.extend(
                programs
                    .iter()
                    .filter_map(|name| register.program.get_key_value(name)),
            );
        }
        wanted.sort_by_key(|(name, _)| *name);
        wanted.dedup_by_key(|(name, _)| *name);
        let programs: Vec<Wanted<'a>> = wanted
            .into_iter()
            .map(|(name, program)| {
                let checks = self
                    .checks
                    .iter()
                    .filter(|check| {
                        register
                            .check
                            .get(*check)
                            .is_some_and(|definition| definition.programs.contains(name))
                    })
                    .map(ToString::to_string);
                let builds = self
                    .builds
                    .iter()
                    .filter(|build| {
                        register
                            .build
                            .get(*build)
                            .is_some_and(|definition| definition.programs.contains(name))
                    })
                    .map(ToString::to_string);
                let callers = checks.chain(builds).collect();
                let runtime = program
                    .runtime
                    .as_ref()
                    .and_then(|runtime| register.runtime.get_key_value(runtime));
                let found = find::program(register, name, program, self.env);
                Wanted {
                    name,
                    program,
                    runtime,
                    needs_runtime: runtime.is_some_and(|(_, runtime)| {
                        needed.contains(&name) || needs(runtime, &found, mode)
                    }),
                    callers,
                    found,
                }
            })
            .collect();
        let mut runtimes: Vec<Needed<'a>> = Vec::new();
        for (name, runtime) in programs.iter().filter_map(needed_runtime) {
            if !runtimes.iter().any(|needed| needed.name == name) {
                let minimum = highest(
                    programs
                        .iter()
                        .filter(|wanted| needed_runtime(wanted).is_some_and(|(on, _)| on == name))
                        .filter_map(|wanted| {
                            let minimum = wanted.program.runtime_minimum.as_deref();
                            minimum.map(|minimum| (wanted.name, minimum))
                        }),
                );
                let version = minimum.map(|(_, version)| version);
                runtimes.push(Needed {
                    name,
                    runtime,
                    minimum,
                    found: find::runtime(name, runtime, version, self.env),
                });
            }
        }
        runtimes.sort_by_key(|needed| needed.name);
        Survey { runtimes, programs }
    }

    /// Whether the missing runtimes may be installed: by `--with-runtimes`,
    /// or by a yes in a terminal. Without a terminal nothing is asked.
    fn agreed(
        &mut self,
        runtimes: &[&Needed],
        survey: &Survey,
        folder: &Path,
        options: Options,
    ) -> bool {
        let mut text = String::from("\nThese runtimes are missing, and programs need them:\n");
        for needed in runtimes {
            let users: Vec<String> = survey
                .programs
                .iter()
                .filter(|wanted| {
                    needed_runtime(wanted).is_some_and(|(name, _)| name == needed.name)
                })
                .map(|wanted| wanted.name.to_string())
                .collect();
            let source = self
                .env
                .platform
                .and_then(|platform| needed.runtime.download.get(&platform))
                .map_or_else(
                    || format!("no build for {}", self.env.computer),
                    |download| download.url.clone(),
                );
            let _ = writeln!(
                text,
                "  {} {} ({}), for {}: {source}",
                needed.name,
                needed.runtime.version,
                needed.runtime.title,
                list(&users)
            );
            if let (Found::TooOld { path, version }, Some((program, minimum))) =
                (&needed.found, needed.minimum)
            {
                let _ = writeln!(
                    text,
                    "    The computer has {version} ({}); {program} needs {minimum} or later.",
                    path.display()
                );
            }
        }
        self.say(&text);
        if options.with_runtimes {
            return true;
        }
        let Some(answers) = self.env.answers.as_mut() else {
            self.say(
                "There is no terminal to ask in, and --with-runtimes was not given, so no runtime \
                 is installed.\n",
            );
            return false;
        };
        let question = format!(
            "Install them into {}? [y/N] ",
            folder.join("runtimes").display()
        );
        let _ = self.out.write_all(question.as_bytes());
        let _ = self.out.flush();
        let mut answer = String::new();
        let _ = answers.read_line(&mut answer);
        matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    }

    fn install_runtime(&mut self, needed: &Needed, folder: &Path) -> Result<(), String> {
        let download = self.build(&needed.runtime.download, false)?;
        let dir = find::runtime_folder(folder, needed.name, &needed.runtime.version);
        let staging = staging(folder, needed.name.as_str(), &needed.runtime.version);
        self.say(&format!(
            "Installing {} {} from {}\n",
            needed.name, needed.runtime.version, download.url
        ));
        let result = self.native(download, &dir, &staging);
        let _ = fs::remove_dir_all(&staging);
        result
    }

    fn install_program(&mut self, wanted: &Wanted, folder: &Path) -> Result<(), String> {
        let dir = find::program_folder(folder, wanted.name, &wanted.program.version);
        let staging = staging(folder, wanted.name.as_str(), &wanted.program.version);
        let download = self.build(&wanted.program.download, wanted.runtime.is_some())?;
        self.say(&format!(
            "Installing {} {} from {}\n",
            wanted.name, wanted.program.version, download.url
        ));
        let result = match wanted.runtime {
            None => self.native(download, &dir, &staging),
            Some((name, runtime)) => self.package(wanted, download, name, runtime, &dir, &staging),
        };
        let _ = fs::remove_dir_all(&staging);
        result
    }

    /// The build for this computer, or the one package of a program on a
    /// runtime.
    fn build<'d>(
        &self,
        downloads: &'d BTreeMap<Platform, Download>,
        packaged: bool,
    ) -> Result<&'d Download, String> {
        let key = if packaged {
            Some(Platform::Any)
        } else {
            self.env.platform
        };
        key.and_then(|key| downloads.get(&key))
            .ok_or_else(|| format!("the register has no pinned build for {}", self.env.computer))
    }

    /// Downloads, verifies and unpacks a build into `dir`.
    fn native(&self, download: &Download, dir: &Path, staging: &Path) -> Result<(), String> {
        let file = fetch::download(download, staging, self.env)?;
        fresh(dir)?;
        let executable = download.executable.as_deref().unwrap_or_default();
        unpack::unpack(&file, Packing::of(&download.url), dir, executable)
            .and_then(|_| finish(dir))
            .inspect_err(|_| drop(fs::remove_dir_all(dir)))
    }

    /// Downloads and verifies a package and its companions, and installs them
    /// into `dir` together with their runtime's installer.
    fn package(
        &self,
        wanted: &Wanted,
        download: &Download,
        runtime: &RuntimeName,
        definition: &Runtime,
        dir: &Path,
        staging: &Path,
    ) -> Result<(), String> {
        let minimum = wanted.program.runtime_minimum.as_deref();
        let runtime_executable = match find::runtime(runtime, definition, minimum, self.env) {
            Found::Pinned(path) | Found::Computer { path, .. } => path,
            // Only a minimum makes a runtime too old.
            Found::TooOld { version, .. } => {
                return Err(format!(
                    "it needs {runtime} {} or later, and the computer's is {version}",
                    minimum.unwrap_or_default()
                ));
            }
            Found::Missing => return Err(format!("it needs {runtime}, which is missing")),
        };
        let mut files = vec![fetch::download(download, staging, self.env)?];
        for companion in &wanted.program.companions {
            files.push(fetch::download(&companion.download(), staging, self.env)?);
        }
        fresh(dir)?;
        let executable = find::installed(
            definition.installer,
            dir,
            wanted.name,
            wanted.program,
            self.env.windows,
        );
        install::package(
            definition.installer,
            &runtime_executable,
            &files,
            dir,
            self.env,
        )
        .and_then(|()| {
            if executable.is_file() {
                Ok(())
            } else {
                Err(format!(
                    "{} left no {}",
                    definition.installer,
                    executable.display()
                ))
            }
        })
        .and_then(|()| finish(dir))
        .inspect_err(|_| drop(fs::remove_dir_all(dir)))
    }

    fn report(&mut self, survey: &Survey) {
        let mut text = String::new();
        let mut missing: Vec<String> = Vec::new();
        let mut differs: Vec<String> = Vec::new();
        if !survey.runtimes.is_empty() {
            text.push_str("\nRuntimes\n");
            for needed in &survey.runtimes {
                let head = format!("{} {}", needed.name, needed.runtime.version);
                note(
                    &head,
                    &needed.found,
                    &needed.runtime.version,
                    &mut missing,
                    &mut differs,
                );
                let mut line = state(&needed.found, &needed.runtime.version);
                if let (Found::TooOld { .. }, Some((program, minimum))) =
                    (&needed.found, needed.minimum)
                {
                    let _ = write!(line, ": {program} needs {minimum} or later");
                }
                let _ = writeln!(text, "  {head}: {line}");
            }
        }
        text.push_str("\nPrograms\n");
        for wanted in &survey.programs {
            let head = format!("{} {}", wanted.name, wanted.program.version);
            note(
                &head,
                &wanted.found,
                &wanted.program.version,
                &mut missing,
                &mut differs,
            );
            let _ = writeln!(
                text,
                "  {head} ({}): {}",
                wanted.callers.join(", "),
                state(&wanted.found, &wanted.program.version)
            );
        }
        text.push('\n');
        if missing.is_empty() {
            text.push_str("Nothing is missing.\n");
        } else {
            let _ = writeln!(text, "Missing: {}.", missing.join(", "));
        }
        if !differs.is_empty() {
            let _ = writeln!(text, "Not the pinned version: {}.", differs.join(", "));
        }
        self.say(&text);
    }
}

/// Whether this run puts the program in place: in the default mode, and in
/// the one that only says what that mode would do, when it is missing; with
/// `--pinned`, unless its pinned copy is there.
fn installs(found: &Found, mode: Mode) -> bool {
    match mode {
        Mode::Pinned => !matches!(found, Found::Pinned(_)),
        Mode::Missing | Mode::Check => found.missing(),
    }
}

/// Whether this run needs the runtime of a program on it: when it installs
/// the program through the runtime's installer, or when the program's pinned
/// copy starts through the runtime. npm puts a script in place that calls the
/// runtime on the `PATH`; pip puts the program into an environment of its own,
/// which starts with that environment's Python. A program on the computer that
/// the run leaves as it is starts the way it was installed.
fn needs(runtime: &Runtime, found: &Found, mode: Mode) -> bool {
    let started_through = match runtime.installer {
        Installer::Npm => true,
        Installer::Pip => false,
    };
    installs(found, mode) || (started_through && matches!(found, Found::Pinned(_)))
}

/// The runtime of a program, if this run needs it for the program.
fn needed_runtime<'a>(wanted: &Wanted<'a>) -> Option<(&'a RuntimeName, &'a Runtime)> {
    wanted.runtime.filter(|_| wanted.needs_runtime)
}

/// Whether nothing is missing.
fn complete(survey: &Survey) -> bool {
    survey
        .runtimes
        .iter()
        .map(|needed| &needed.found)
        .chain(survey.programs.iter().map(|wanted| &wanted.found))
        .all(|found| !found.missing())
}

/// The latest of these versions, each with what needs it, if there is any;
/// of equal ones the first.
fn highest<'v, T>(versions: impl Iterator<Item = (T, &'v str)>) -> Option<(T, &'v str)> {
    versions.reduce(
        |kept, next| {
            if at_least(kept.1, next.1) { kept } else { next }
        },
    )
}

/// What was found, in words.
fn state(found: &Found, pinned: &str) -> String {
    match found {
        Found::Pinned(path) => format!("pinned, {}", path.display()),
        Found::Computer { path, version } => match version {
            Some(version) if version == pinned => format!("on the computer, {}", path.display()),
            Some(version) => format!("on the computer, {}, version {version}", path.display()),
            None => format!(
                "on the computer, {}, which prints no version",
                path.display()
            ),
        },
        Found::TooOld { path, version } => {
            format!(
                "on the computer, {}, version {version}, too old",
                path.display()
            )
        }
        Found::Missing => "missing".to_owned(),
    }
}

/// Adds what was found to the list of what is missing or differs.
fn note(
    head: &str,
    found: &Found,
    pinned: &str,
    missing: &mut Vec<String>,
    differs: &mut Vec<String>,
) {
    match found {
        Found::Missing | Found::TooOld { .. } => missing.push(head.to_owned()),
        Found::Computer { version, .. } if version.as_deref() != Some(pinned) => {
            differs.push(format!(
                "{head} (the computer's: {})",
                version.as_deref().unwrap_or("unknown")
            ));
        }
        Found::Pinned(_) | Found::Computer { .. } => {}
    }
}

/// Names joined for a sentence.
fn list(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(", ")
    }
}

/// Where a download waits while it is checked and unpacked.
fn staging(folder: &Path, name: &str, version: &str) -> PathBuf {
    folder.join("downloads").join(format!("{name}-{version}"))
}

/// Empties the folder of an installation, which an earlier one that broke off
/// may have left.
fn fresh(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(cannot("cannot be emptied", dir))?;
    }
    fs::create_dir_all(dir).map_err(cannot("cannot be made", dir))
}

/// Marks an installation as complete, last of all.
fn finish(dir: &Path) -> Result<(), String> {
    let mark = dir.join(MARK);
    fs::write(&mark, "").map_err(cannot("cannot be written", &mark))
}

/// The sentence for a file or folder that something cannot be done with.
fn cannot<'p>(what: &'p str, path: &'p Path) -> impl FnOnce(io::Error) -> String + 'p {
    move |error| format!("{} {what}: {error}", path.display())
}

/// Runs a program to its end. Returns what it printed when it succeeds, and
/// a sentence with what it printed when it cannot be started or fails.
fn execute(command: &mut Command, what: &str) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|error| format!("{what} cannot be started: {error}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        Ok(text)
    } else {
        Err(format!(
            "{what} failed ({}): {}",
            output.status,
            text.trim()
        ))
    }
}
