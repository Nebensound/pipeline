//! `nebensound-repo run`: one check over the files it covers in one unit, the
//! way its building block runs it.
//!
//! The Rust and the Python check are the ones that run code of the
//! repository: the Rust check builds and tests a Cargo workspace with the Rust
//! toolchain, which rustup provides ([`rust`](mod@rust)), and the Python check
//! installs and tests a uv project with the interpreter uv provides
//! ([`python`](mod@python)). Each goes its own way. What follows holds for the
//! others.
//!
//! The files are those `files` names for the check and the unit. The programs
//! are those the register names for the check, found the way
//! [`locate`](crate::setup::find::locate) finds them: the pinned copy in the
//! tool's folder first, the one on the computer otherwise. Four of them are
//! called through their internals, so that the repository's rules are laid
//! over the pipeline's default: html-validate, markdownlint-cli2, Stylelint
//! and yamllint. Of those four only the pinned copy is taken, since only its
//! internals are known. A program that is not there fails the check and names
//! `setup`, which installs it; `run` installs nothing.
//!
//! Every program is called with the flags that set its own ignore mechanisms
//! aside: what is not to be checked is not in the list. Each part of a check
//! runs even when one before it failed, so that one run reports every
//! finding.
//!
//! A finding names its file, and the line and column where the program gives
//! them. In a workflow run it is an annotation of the run; elsewhere it is a
//! line that starts with `file:line:col:`. A finding that fails nothing, such
//! as a Hadolint finding below the failure threshold, is a warning. What the
//! programs print stands in the output as well.
//!
//! The gate decides: what counts is the result of the run on GitHub Actions.
//! A run anywhere else comes as close to it as is sensible, and is never the
//! same in every case, so every call of the command `run` outside a workflow
//! run opens with a line that says so ([`OUTSIDE`]), also one that ends in a
//! refusal or an error before a check starts. In a workflow run that line is
//! not written.

pub mod css;
pub mod dockerfile;
pub mod html;
pub mod json;
mod links;
pub mod markdown;
pub mod python;
pub mod rust;
pub mod toml;
pub mod yaml;

use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Read as _, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};

use tempfile::TempDir;

use crate::Register;
use crate::names::ProgramName;
use crate::packages::Excluded;
use crate::path::RelativePath;
use crate::programs::Installer;
use crate::report::shown;
use crate::repository::Repository;
use crate::setup::Environment;
use crate::setup::find;
use crate::setup::install::search_path_with;
use crate::writer::Order;

/// How many bytes of file names one call of a program is handed at most. A
/// command line has a limit, the smallest on Windows; a longer list is handed
/// over in several calls, as `xargs` would.
pub const ARGUMENTS: usize = 30_000;

/// The first line of every call of the command `run` outside a workflow run,
/// written where the call is dispatched, before anything can refuse it. It
/// is neither a
/// finding nor a workflow command, so nothing that reads findings off the
/// output takes it for one.
pub const OUTSIDE: &str = "Not running in GitHub Actions, so the result can differ from the \
                           gate's, which is the one that counts.";

/// One of the checks `run` carries out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// `.css` and `.scss` files.
    Css,
    /// `Dockerfile`, `Containerfile` and `.dockerfile` files.
    Dockerfile,
    /// `.html` and `.htm` files.
    Html,
    /// `.json`, `.jsonc` and `.geojson` files and the JSON lockfiles.
    Json,
    /// `.md` and `.markdown` files.
    Markdown,
    /// `.py` and `.pyi` files, the uv project at the unit's path.
    Python,
    /// `.rs` files, the Cargo workspace at the unit's path.
    Rust,
    /// `.toml` files and the TOML lockfiles.
    Toml,
    /// `.yml` and `.yaml` files.
    Yaml,
}

impl Check {
    /// The check of this name, if `run` carries it out.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        match name {
            "css" => Some(Self::Css),
            "dockerfile" => Some(Self::Dockerfile),
            "html" => Some(Self::Html),
            "json" => Some(Self::Json),
            "markdown" => Some(Self::Markdown),
            "python" => Some(Self::Python),
            "rust" => Some(Self::Rust),
            "toml" => Some(Self::Toml),
            "yaml" => Some(Self::Yaml),
            _ => None,
        }
    }

    /// The name the register gives it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Css => "css",
            Self::Dockerfile => "dockerfile",
            Self::Html => "html",
            Self::Json => "json",
            Self::Markdown => "markdown",
            Self::Python => "python",
            Self::Rust => "rust",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
        }
    }

    /// What one of its files is called in a sentence.
    fn noun(self) -> &'static str {
        match self {
            Self::Css => "stylesheet",
            Self::Dockerfile => "Dockerfile",
            Self::Html => "HTML file",
            Self::Json => "JSON file",
            Self::Markdown => "Markdown file",
            Self::Python => "Python file",
            Self::Rust => "Rust file",
            Self::Toml => "TOML file",
            Self::Yaml => "YAML file",
        }
    }

    /// Whether it writes the versions of the plan before it tests a package:
    /// then it reads the history for them when no plan is handed in.
    #[must_use]
    pub fn writes_versions(self) -> bool {
        self == Self::Rust
    }
}

/// What a check runs with.
pub struct Context<'a> {
    /// The register, which names the programs and pins them.
    pub register: &'a Register,
    /// The computer: where programs are looked for, and the tool's folder.
    pub env: &'a Environment,
    /// The repository root, as an absolute path without links: every program
    /// runs there, which is where the repository keeps its configuration and
    /// where a finding has to point.
    pub root: &'a Path,
    /// Where `run` makes the folder of what it hands the programs, which it
    /// removes again.
    pub temp: &'a Path,
    /// Whether this is a workflow run, where a finding is an annotation.
    pub workflow: bool,
    /// What the Rust check reads of the repository beyond its files; the
    /// other checks read none of it.
    pub packages: Packages<'a>,
}

/// What the Rust check reads of the repository beyond its files.
pub struct Packages<'a> {
    /// The repository, into whose manifests the package test writes the
    /// versions.
    pub repository: &'a Repository,
    /// Every file Git tracks, as a path from the repository root: the
    /// manifests the versions are written into, and the `.rs` files of an
    /// excluded package, which coverage leaves out one by one where another
    /// package lies inside it.
    pub tracked: &'a [RelativePath],
    /// The packages `[[exclude]]` takes out as a whole.
    pub excluded: &'a [Excluded],
    /// The versions the package test writes: from the plan handed in, or
    /// resolved from the history.
    pub orders: &'a [Order],
}

/// Runs the check over these files of the unit, prints what the programs
/// print and every finding, and returns the exit code: 0 when nothing was
/// found, 1 otherwise.
pub fn run(
    context: &Context<'_>,
    check: Check,
    unit: &RelativePath,
    files: &[&RelativePath],
    out: &mut dyn Write,
) -> u8 {
    let mut log = Log {
        out,
        workflow: context.workflow,
    };
    let noun = check.noun();
    if files.is_empty() {
        log.line(&format!(
            "No {noun} below '{}'; there is nothing to check.",
            shown(&unit.to_string())
        ));
        return 0;
    }
    log.line(&format!("{} {noun}(s) to check:", files.len()));
    for file in files {
        log.line(&shown(&file.to_string()));
    }
    let mut session = Session {
        context,
        unit: unit.clone(),
        files: files.iter().map(ToString::to_string).collect(),
        temp: None,
        log,
    };
    let passed = match check {
        Check::Css => css::run(&mut session),
        Check::Dockerfile => dockerfile::run(&mut session),
        Check::Html => html::run(&mut session),
        Check::Json => json::run(&mut session),
        Check::Markdown => markdown::run(&mut session),
        Check::Python => python::run(&mut session),
        Check::Rust => rust::run(&mut session),
        Check::Toml => toml::run(&mut session),
        Check::Yaml => yaml::run(&mut session),
    };
    u8::from(!passed)
}

/// Where the output goes, and how a finding is written there.
struct Log<'o> {
    out: &'o mut dyn Write,
    workflow: bool,
}

impl Log<'_> {
    /// Writes one line. Outside a workflow run, a workflow command is written
    /// the way a reader takes it in: see [`plain`].
    fn line(&mut self, text: &str) {
        let text = if self.workflow {
            Some(text.to_owned())
        } else {
            plain(text)
        };
        if let Some(text) = text {
            let _ = self.out.write_all(text.as_bytes());
            let _ = self.out.write_all(b"\n");
        }
    }

    /// Writes every line of what a program printed.
    fn lines(&mut self, text: &str) {
        for line in text.lines() {
            self.line(line);
        }
    }

    /// Writes a finding: on a file if it names one, at a line and a column
    /// if it gives them.
    fn error(&mut self, file: Option<&str>, position: Option<(&str, Option<&str>)>, message: &str) {
        self.line(&annotation(file, position, message));
    }

    /// Writes a finding that fails nothing, the way [`Log::error`] writes
    /// one that does.
    fn warning(
        &mut self,
        file: Option<&str>,
        position: Option<(&str, Option<&str>)>,
        message: &str,
    ) {
        self.line(&annotation_at(Level::Warning, file, position, message));
    }
}

/// How much a finding weighs: whether it fails the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// It fails the check: the workflow command `error`.
    Error,
    /// It fails nothing: the workflow command `warning`.
    Warning,
}

impl Level {
    /// The name of its workflow command.
    fn command(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

/// A finding as the workflow command `error`, on a file if it names one, at a
/// line and a column if it gives them, escaped the way the runner reads it.
#[must_use]
pub fn annotation(
    file: Option<&str>,
    position: Option<(&str, Option<&str>)>,
    message: &str,
) -> String {
    annotation_at(Level::Error, file, position, message)
}

/// A finding as the workflow command of its level, written as [`annotation`]
/// writes it.
#[must_use]
pub fn annotation_at(
    level: Level,
    file: Option<&str>,
    position: Option<(&str, Option<&str>)>,
    message: &str,
) -> String {
    let level = level.command();
    let Some(file) = file else {
        return format!("::{level}::{}", data(message));
    };
    let mut properties = format!("file={}", property(file));
    if let Some((line, column)) = position {
        let _ = write!(properties, ",line={line}");
        if let Some(column) = column {
            let _ = write!(properties, ",col={column}");
        }
    }
    format!("::{level} {properties}::{}", data(message))
}

/// The message of a workflow command: a line break would end the command, and
/// `%` starts an escape.
fn data(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// A property of a workflow command, where `:` and `,` end the value as well.
fn property(text: &str) -> String {
    data(text).replace(':', "%3A").replace(',', "%2C")
}

/// A line as it is written outside a workflow run.
///
/// An annotation (`::error`, `::warning` or `::notice`) becomes
/// `file:line:col: error: message`, with the parts it does not give left
/// out, its title in brackets behind the level and every escape undone. A
/// group (`::group::name`) becomes its name, and its end nothing, [`None`].
/// Every other line stays as it is.
#[must_use]
pub fn plain(line: &str) -> Option<String> {
    if line == "::endgroup::" {
        return None;
    }
    if let Some(name) = line.strip_prefix("::group::") {
        return Some(unescape(name));
    }
    let Some((level, properties, message)) = command(line) else {
        return Some(line.to_owned());
    };
    let mut place = Vec::new();
    let mut title = String::new();
    for pair in properties.split(',') {
        match pair.split_once('=') {
            Some(("file", value)) => place.insert(0, unescape(value)),
            Some(("line" | "col", value)) => place.push(unescape(value)),
            Some(("title", value)) => title = format!("[{}]", unescape(value)),
            _ => {}
        }
    }
    let message = unescape(message);
    Some(if place.is_empty() {
        format!("{level}{title}: {message}")
    } else {
        format!("{}: {level}{title}: {message}", place.join(":"))
    })
}

/// An annotation, taken apart: its level, its properties (empty when it has
/// none) and its message. Escaped, a property holds no `:`.
fn command(line: &str) -> Option<(&str, &str, &str)> {
    let rest = line.strip_prefix("::")?;
    let level = ["error", "warning", "notice"]
        .into_iter()
        .find(|level| rest.starts_with(level))?;
    let rest = &rest[level.len()..];
    if let Some(message) = rest.strip_prefix("::") {
        return Some((level, "", message));
    }
    let (properties, message) = rest.strip_prefix(' ')?.split_once("::")?;
    (!properties.contains(':')).then_some((level, properties, message))
}

/// Undoes the escapes of a workflow command.
fn unescape(text: &str) -> String {
    text.replace("%0A", "\n")
        .replace("%0D", "\r")
        .replace("%3A", ":")
        .replace("%2C", ",")
        .replace("%25", "%")
}

/// The path from the folder `from` to `to`, both absolute and without links,
/// with `..` for every part of `from` that `to` does not share.
#[must_use]
pub fn relative(from: &Path, to: &Path) -> PathBuf {
    let from: Vec<Component> = from.components().collect();
    let to: Vec<Component> = to.components().collect();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let mut path = PathBuf::new();
    for _ in common..from.len() {
        path.push("..");
    }
    for part in &to[common..] {
        path.push(part);
    }
    path
}

/// The files in groups whose names stay below [`ARGUMENTS`] bytes, each group
/// holding one file at least.
#[must_use]
pub fn chunks<'f>(files: &[&'f str]) -> Vec<Vec<&'f str>> {
    let mut chunks: Vec<Vec<&str>> = Vec::new();
    let mut size = 0;
    for file in files {
        let length = file.len() + 1;
        match chunks.last_mut() {
            Some(chunk) if size + length <= ARGUMENTS => {
                chunk.push(file);
                size += length;
            }
            _ => {
                chunks.push(vec![file]);
                size = length;
            }
        }
    }
    chunks
}

/// The name of a file, after its last `/`.
fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Whether the name of a file ends in this extension, after its last `.`,
/// matched as exactly as the register writes extensions.
fn has_extension(path: &str, extension: &str) -> bool {
    name_of(path)
        .rsplit_once('.')
        .is_some_and(|(_, ending)| ending == extension)
}

/// Whether a file is a lockfile: the register gives the checks lockfiles by
/// name and nothing else that ends in `.lock`, so every such file of a check
/// is one. Its layout is the business of the tool that writes it, so it is
/// parsed and not format-checked; a lockfile the register names later is
/// passed over the same way without a change here.
fn is_lockfile(path: &str) -> bool {
    has_extension(path, "lock")
}

/// Takes every variable whose name starts with `prefix` out of what `command`
/// inherits from this process: a program that reads its settings from such
/// variables then reads none of a developer's or a runner's.
fn without_variables(command: &mut Command, prefix: &str) {
    for (name, _) in std::env::vars_os() {
        if name.as_encoded_bytes().starts_with(prefix.as_bytes()) {
            command.env_remove(name);
        }
    }
}

/// How a program ended, as a message says it: with its exit status, or, in
/// brackets, what ended it when it gave none.
fn ended(status: ExitStatus) -> String {
    status.code().map_or_else(
        || format!("({status})"),
        |code| format!("with exit status {code}"),
    )
}

/// What goes into the folder of what the programs are handed: a file with
/// its content, or a folder.
type Entry = (String, Option<Vec<u8>>);

/// A program found for this run, and how to start it.
struct Tool {
    /// The file to start.
    executable: PathBuf,
    /// The search path it gets: the computer's, with the folder of its
    /// runtime in front when it has one.
    search_path: OsString,
    /// Its runtime, when one was found for it.
    runtime: Option<PathBuf>,
    /// The folder of its pinned copy, when that is the copy found; empty
    /// otherwise.
    folder: PathBuf,
}

/// How a program ended, and what it printed on both outputs as it came.
struct Ran {
    success: bool,
    /// Its exit status, when it gave one.
    code: Option<i32>,
    text: String,
}

impl Ran {
    fn failed() -> Self {
        Self {
            success: false,
            code: None,
            text: String::new(),
        }
    }
}

/// One run of a check.
struct Session<'a, 'o> {
    context: &'a Context<'a>,
    /// The unit, as the plan names it.
    unit: RelativePath,
    /// The files, as paths from the repository root.
    files: Vec<String>,
    /// The folder of what the programs are handed, once it is made.
    temp: Option<TempDir>,
    log: Log<'o>,
}

impl Session<'_, '_> {
    /// Makes the folder of what the programs are handed, with the entries
    /// `entries` gives for its path in it, and returns where it is; or writes
    /// why it cannot be made.
    fn folder(&mut self, entries: impl FnOnce(&Path) -> Vec<Entry>) -> Option<PathBuf> {
        let made = tempfile::Builder::new()
            .prefix("nebensound-repo-run-")
            .tempdir_in(self.context.temp)
            .and_then(|dir| {
                entries(dir.path())
                    .iter()
                    .try_for_each(|(name, content)| {
                        let path = dir.path().join(name);
                        match content {
                            Some(content) => fs::write(path, content),
                            None => fs::create_dir(path),
                        }
                    })
                    .map(|()| dir)
            });
        match made {
            Ok(dir) => {
                let path = dir.path().to_path_buf();
                self.temp = Some(dir);
                Some(path)
            }
            Err(error) => {
                self.log.error(
                    None,
                    None,
                    &format!(
                        "no folder can be made in {} for what the programs are handed: {error}",
                        self.context.temp.display()
                    ),
                );
                None
            }
        }
    }

    /// Finds a program, or writes why it cannot be used. `pinned` takes only
    /// the pinned copy, for a program whose internals are called.
    fn tool(&mut self, name: &str, pinned: bool) -> Option<Tool> {
        let register = self.context.register;
        let env = self.context.env;
        let entry = ProgramName::new(name)
            .ok()
            .and_then(|name| register.program.get_key_value(&name));
        let found = entry.and_then(|(key, program)| {
            find::locate(register, key, env)
                .filter(|location| location.pinned || !pinned)
                .map(|location| (key, program, location))
        });
        let Some((key, program, location)) = found else {
            let version =
                entry.map_or_else(String::new, |(_, program)| format!(" {}", program.version));
            let message = if pinned {
                format!(
                    "{name}{version} is not pinned in the tool's folder, and the check calls the \
                     internals of the pinned copy only; `nebensound-repo setup --pinned` installs \
                     it"
                )
            } else {
                format!(
                    "{name}{version} is neither pinned in the tool's folder nor on the computer; \
                     `nebensound-repo setup` installs it"
                )
            };
            self.log.error(None, None, &message);
            return None;
        };
        // A pinned copy installed with npm is a script that starts its
        // runtime; without one it cannot start.
        let npm = program
            .runtime
            .as_ref()
            .and_then(|runtime| register.runtime.get(runtime))
            .is_some_and(|runtime| runtime.installer == Installer::Npm);
        if location.pinned && npm && location.runtime.is_none() {
            self.log.error(
                None,
                None,
                &format!(
                    "{name} {} needs {} {} or later, and there is none; `nebensound-repo setup \
                     --with-runtimes` installs it",
                    program.version,
                    program
                        .runtime
                        .as_ref()
                        .map_or("", |runtime| runtime.as_str()),
                    program.runtime_minimum.as_deref().unwrap_or_default()
                ),
            );
            return None;
        }
        let search_path = match &location.runtime {
            Some(runtime) => search_path_with(runtime, env),
            None => env.search_path.clone(),
        };
        let folder = env
            .folder
            .as_deref()
            .filter(|_| location.pinned)
            .map(|folder| find::program_folder(folder, key, &program.version))
            .unwrap_or_default();
        Some(Tool {
            executable: location.executable,
            search_path,
            runtime: location.runtime,
            folder,
        })
    }

    /// A command that starts `program` at the repository root with this
    /// search path.
    fn command(&self, program: &Path, search_path: &OsStr) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(self.context.root)
            .env("PATH", search_path)
            .stdin(Stdio::null());
        command
    }

    /// Runs a program to its end: whether it succeeded, and what it printed
    /// on both outputs in the order it printed it. A program that cannot be
    /// started is written as a finding and counts as failed.
    fn capture(&mut self, mut command: Command, what: &str) -> Ran {
        let started = std::io::pipe().and_then(|(reader, writer)| {
            writer.try_clone().and_then(|copy| {
                command.stdout(copy).stderr(writer);
                command.spawn().map(|child| (reader, child))
            })
        });
        // The command holds the writing ends until it is dropped, and the
        // reading below ends only when every writing end is closed.
        drop(command);
        match started {
            Ok((mut reader, mut child)) => {
                let mut bytes = Vec::new();
                let _ = reader.read_to_end(&mut bytes);
                let status = child.wait().ok();
                Ran {
                    success: status.is_some_and(|status| status.success()),
                    code: status.and_then(|status| status.code()),
                    text: String::from_utf8_lossy(&bytes).into_owned(),
                }
            }
            Err(error) => {
                self.log
                    .error(None, None, &format!("{what} cannot be started: {error}"));
                Ran::failed()
            }
        }
    }

    /// Runs a program to its end and keeps what it printed on each output
    /// apart, with its exit status. A program that cannot be started is
    /// written as a finding, and nothing is returned.
    fn output(&mut self, mut command: Command, what: &str) -> Option<Output> {
        match command.output() {
            Ok(output) => Some(output),
            Err(error) => {
                self.log
                    .error(None, None, &format!("{what} cannot be started: {error}"));
                None
            }
        }
    }

    /// Runs a tool with these arguments, and with a file of the repository
    /// on its standard input if one is given.
    fn execute(&mut self, tool: &Tool, args: &[OsString], input: Option<&str>, what: &str) -> Ran {
        let mut command = self.command(&tool.executable, &tool.search_path);
        command.args(args);
        if let Some(file) = input {
            match File::open(self.context.root.join(file)) {
                Ok(file) => {
                    command.stdin(file);
                }
                Err(error) => {
                    self.log
                        .error(Some(file), None, &format!("cannot be read: {error}"));
                    return Ran::failed();
                }
            }
        }
        self.capture(command, what)
    }

    /// Runs a tool over these files, in as many calls as the command line
    /// needs: whether every call succeeded, and what they printed.
    fn over(&mut self, tool: &Tool, args: &[OsString], files: &[&str], what: &str) -> Ran {
        let mut all = Ran {
            success: true,
            code: None,
            text: String::new(),
        };
        for chunk in chunks(files) {
            let mut call: Vec<OsString> = args.to_vec();
            call.push("--".into());
            call.extend(chunk.iter().map(OsString::from));
            let ran = self.execute(tool, &call, None, what);
            all.success &= ran.success;
            all.text.push_str(&ran.text);
        }
        all
    }

    /// Whether a path from the repository root is a file.
    fn is_file(&self, path: &str) -> bool {
        self.context.root.join(path).is_file()
    }

    /// Prettier over these files: they are formatted the way it writes them.
    /// `ignore` is an empty file in the folder of what the programs are
    /// handed.
    ///
    /// Prettier reads the repository's own configuration, so a repository
    /// decides how it formats. It reads no ignore file: by default it would
    /// read `.prettierignore` and `.gitignore` and silently pass what they
    /// name, so it is handed an empty one instead, and it leaves out no
    /// `node_modules` either. Its configuration could still pass a whole file
    /// unchecked, through `requirePragma` (a file without `@format`),
    /// `checkIgnorePragma` (a file with `@noformat`) or a range (`rangeStart`,
    /// `rangeEnd`) that leaves the file out, `overrides` included; the flags
    /// set all of them aside, since the command line wins over the
    /// configuration. Without colours, so that the lines read the same
    /// everywhere.
    ///
    /// Prettier names an unformatted file as `[warn] <file>` and one it
    /// cannot parse as `[error] <file>: <message>`; each becomes a finding on
    /// its file, and the line stays in the output as it is.
    fn prettier(&mut self, tool: &Tool, ignore: &Path, files: &[&str]) -> bool {
        let args: Vec<OsString> = vec![
            "--check".into(),
            "--no-color".into(),
            "--with-node-modules".into(),
            "--ignore-path".into(),
            ignore.into(),
            "--no-require-pragma".into(),
            "--no-check-ignore-pragma".into(),
            "--range-start=0".into(),
            "--range-end=Infinity".into(),
        ];
        let ran = self.over(tool, &args, files, "Prettier");
        for line in ran.text.lines() {
            self.log.line(line);
            if let Some(file) = line
                .strip_prefix("[warn] ")
                .filter(|file| self.is_file(file))
            {
                self.log
                    .error(Some(file), None, "not formatted the way Prettier writes it");
            } else if let Some((file, message)) = line
                .strip_prefix("[error] ")
                .and_then(|rest| rest.split_once(": "))
                .filter(|(file, message)| {
                    !file.contains(':') && !message.is_empty() && self.is_file(file)
                })
            {
                self.log.error(
                    Some(file),
                    None,
                    &format!("Prettier cannot read it: {message}"),
                );
            }
        }
        ran.success
    }
}
