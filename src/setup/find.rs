//! Where a program is: the pinned copy in the tool's folder, or one on the
//! computer, and which version that one reports.
//!
//! [`locate`] is what the other calls of the tool ask; `setup` asks the same
//! questions one by one, to say what it found.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::Register;
use crate::names::{ProgramName, RuntimeName};
use crate::programs::{Installer, Program, Runtime, at_least};

use super::Environment;

/// The file a finished installation leaves in its folder, last. A folder
/// without it is an installation that broke off, and counts as none.
pub const MARK: &str = ".nebensound-installed";

/// What was found for a program or runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// The pinned copy in the tool's folder.
    Pinned(PathBuf),
    /// One on the computer, with the version it reports, if it reports one.
    Computer {
        /// Where it is.
        path: PathBuf,
        /// The version it prints, three numbers.
        version: Option<String>,
    },
    /// A runtime on the computer that is older than a program on it needs,
    /// and none that is new enough. It counts as missing.
    TooOld {
        /// Where it is.
        path: PathBuf,
        /// The version it prints, three numbers.
        version: String,
    },
    /// Neither.
    Missing,
}

impl Found {
    /// Whether there is nothing to use: the program or runtime is missing,
    /// or the runtime on the computer is too old.
    #[must_use]
    pub fn missing(&self) -> bool {
        matches!(self, Self::Missing | Self::TooOld { .. })
    }
}

/// Where a program is, for a call that runs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// The file to start.
    pub executable: PathBuf,
    /// Whether it is the pinned copy in the tool's folder.
    pub pinned: bool,
    /// The runtime it runs on, found the same way, when it needs one and one
    /// is there. Its folder belongs in front of the `PATH` of the program.
    pub runtime: Option<PathBuf>,
}

/// Where a program is: the pinned copy when it exists, else the one on the
/// computer; [`None`] when there is neither, or the register does not know
/// the program.
#[must_use]
pub fn locate(register: &Register, name: &ProgramName, env: &Environment) -> Option<Location> {
    let program = register.program.get(name)?;
    let runtime = program.runtime.as_ref().and_then(|name| {
        let minimum = program.runtime_minimum.as_deref();
        match runtime(name, register.runtime.get(name)?, minimum, env) {
            Found::Pinned(path) | Found::Computer { path, .. } => Some(path),
            Found::TooOld { .. } | Found::Missing => None,
        }
    });
    if let Some(executable) = pinned_program(register, name, program, env) {
        return Some(Location {
            executable,
            pinned: true,
            runtime,
        });
    }
    // A library is never looked for on the computer.
    let command = program.command.as_deref()?;
    search(command, &env.search_path, env.windows).map(|executable| Location {
        executable,
        pinned: false,
        runtime,
    })
}

/// What there is of a program.
#[must_use]
pub fn program(
    register: &Register,
    name: &ProgramName,
    program: &Program,
    env: &Environment,
) -> Found {
    if let Some(path) = pinned_program(register, name, program, env) {
        return Found::Pinned(path);
    }
    // A library is never looked for on the computer.
    let Some(command) = program.command.as_deref() else {
        return Found::Missing;
    };
    computer(
        search(command, &env.search_path, env.windows),
        &program.version_args,
        env,
    )
}

/// What there is of a runtime that the programs on it need at least
/// `minimum` of.
///
/// One on the computer counts only when it prints its version, and the
/// search goes on past one that does not: on Windows the Store puts
/// `python3.exe` and `python.exe` on the `PATH` of a computer without Python,
/// and all they print is where to get one. The search also goes on past one
/// older than `minimum`, which cannot run the programs; when no candidate is
/// new enough, the first that is too old is [`Found::TooOld`]. Whatever its
/// version beyond that, a runtime that can run the programs is used.
#[must_use]
pub fn runtime(
    name: &RuntimeName,
    runtime: &Runtime,
    minimum: Option<&str>,
    env: &Environment,
) -> Found {
    if let Some(path) = pinned_runtime(name, runtime, env) {
        return Found::Pinned(path);
    }
    let mut too_old = None;
    for path in runtime
        .commands
        .iter()
        .flat_map(|command| candidates(command, &env.search_path, env.windows))
    {
        let Some(version) = version(&path, &runtime.version_args, &env.search_path) else {
            continue;
        };
        if minimum.is_none_or(|minimum| at_least(&version, minimum)) {
            return Found::Computer {
                path,
                version: Some(version),
            };
        }
        too_old.get_or_insert(Found::TooOld { path, version });
    }
    too_old.unwrap_or(Found::Missing)
}

fn computer(path: Option<PathBuf>, args: &[String], env: &Environment) -> Found {
    match path {
        Some(path) => {
            let version = version(&path, args, &env.search_path);
            Found::Computer { path, version }
        }
        None => Found::Missing,
    }
}

/// The folder a program's pinned version goes to.
#[must_use]
pub fn program_folder(folder: &Path, name: &ProgramName, version: &str) -> PathBuf {
    folder.join("programs").join(name.as_str()).join(version)
}

/// The folder a runtime's pinned version goes to.
#[must_use]
pub fn runtime_folder(folder: &Path, name: &RuntimeName, version: &str) -> PathBuf {
    folder.join("runtimes").join(name.as_str()).join(version)
}

/// The pinned copy of a program, if it was installed completely.
fn pinned_program(
    register: &Register,
    name: &ProgramName,
    program: &Program,
    env: &Environment,
) -> Option<PathBuf> {
    let dir = program_folder(env.folder.as_deref()?, name, &program.version);
    let executable = program_executable(register, name, program, &dir, env)?;
    // A virtual environment points at the Python it was made with. Once that
    // Python is gone, nothing in it starts, and it counts as no installation:
    // see `environment_starts`.
    let pip = program
        .runtime
        .as_ref()
        .and_then(|runtime| register.runtime.get(runtime))
        .is_some_and(|runtime| runtime.installer == Installer::Pip);
    if pip && !environment_starts(&dir, env.windows) {
        return None;
    }
    finished(&dir, executable)
}

/// The pinned copy of a runtime, if it was installed completely.
fn pinned_runtime(name: &RuntimeName, runtime: &Runtime, env: &Environment) -> Option<PathBuf> {
    let dir = runtime_folder(env.folder.as_deref()?, name, &runtime.version);
    let build = runtime.download.get(&env.platform?)?;
    finished(
        &dir,
        below(
            dir.as_path(),
            build.executable.as_deref().unwrap_or_default(),
        ),
    )
}

fn finished(dir: &Path, executable: PathBuf) -> Option<PathBuf> {
    (dir.join(MARK).is_file() && executable.is_file()).then_some(executable)
}

/// Where the executable of a program lies once its pinned version is in
/// `dir`: inside the build for this platform, or where the runtime's
/// installer puts it; for a library, the file that shows it is there (see
/// [`installed`]). [`None`] when the register has no build for this
/// platform.
#[must_use]
pub fn program_executable(
    register: &Register,
    name: &ProgramName,
    program: &Program,
    dir: &Path,
    env: &Environment,
) -> Option<PathBuf> {
    let Some(runtime) = &program.runtime else {
        let build = program.download.get(&env.platform?)?;
        return Some(below(dir, build.executable.as_deref().unwrap_or_default()));
    };
    let installer = register.runtime.get(runtime)?.installer;
    Some(installed(installer, dir, name, program, env.windows))
}

/// What the installer of a program on a runtime leaves in `dir` that shows
/// the program is there: its command ([`installed_by`]), or, for a library,
/// which has none, the description of its package, `package.json`, where npm
/// puts the package.
#[must_use]
pub fn installed(
    installer: Installer,
    dir: &Path,
    name: &ProgramName,
    program: &Program,
    windows: bool,
) -> PathBuf {
    match &program.command {
        Some(command) => installed_by(installer, dir, command, windows),
        None => dir
            .join("node_modules")
            .join(name.as_str())
            .join("package.json"),
    }
}

/// A path the register writes with `/` between its parts, below `dir`, joined
/// part by part so that it reads right on Windows as well.
#[must_use]
pub fn below(dir: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(dir.to_path_buf(), |path, part| path.join(part))
}

/// Where an installer puts the command of a package it installed into `dir`.
#[must_use]
pub fn installed_by(installer: Installer, dir: &Path, command: &str, windows: bool) -> PathBuf {
    match (installer, windows) {
        (Installer::Npm, false) => dir.join("node_modules").join(".bin").join(command),
        (Installer::Npm, true) => dir
            .join("node_modules")
            .join(".bin")
            .join(format!("{command}.cmd")),
        (Installer::Pip, false) => dir.join("bin").join(command),
        (Installer::Pip, true) => dir.join("Scripts").join(format!("{command}.exe")),
    }
}

/// The Python of a virtual environment made in `dir`.
#[must_use]
pub fn environment_python(dir: &Path, windows: bool) -> PathBuf {
    if windows {
        dir.join("Scripts").join("python.exe")
    } else {
        dir.join("bin").join("python")
    }
}

/// Whether the virtual environment in `dir` still starts: its Python is
/// there, and so is the Python it was made with.
///
/// On Linux and macOS the environment's Python is a link to that Python and
/// is no file once it is gone. On Windows it is a copy of a launcher that
/// starts the `python.exe` in the folder `home` names in `pyvenv.cfg`, so
/// that folder must still hold one.
#[must_use]
pub fn environment_starts(dir: &Path, windows: bool) -> bool {
    environment_python(dir, windows).is_file()
        && (!windows || base_home(dir).is_some_and(|home| home.join("python.exe").is_file()))
}

/// The folder of the Python a virtual environment in `dir` was made with, as
/// its `pyvenv.cfg` names it.
fn base_home(dir: &Path) -> Option<PathBuf> {
    let config = std::fs::read_to_string(dir.join("pyvenv.cfg")).ok()?;
    config.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "home").then(|| PathBuf::from(value.trim()))
    })
}

/// The first file on the search path that is this command and can be run.
///
/// On Windows a command is found by its file with `.exe`, `.cmd` or `.bat`
/// behind the name. A folder that is not an absolute path is skipped: it
/// would be read relative to wherever the tool happens to run.
#[must_use]
pub fn search(command: &str, search_path: &OsStr, windows: bool) -> Option<PathBuf> {
    candidates(command, search_path, windows).next()
}

/// Every file on the search path that is this command and can be run, in
/// the order of the search path.
fn candidates<'a>(
    command: &'a str,
    search_path: &'a OsStr,
    windows: bool,
) -> impl Iterator<Item = PathBuf> + 'a {
    let suffixes: &[&str] = if windows {
        &[".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    std::env::split_paths(search_path)
        .filter(|dir| dir.is_absolute())
        .flat_map(move |dir| {
            suffixes
                .iter()
                .map(move |suffix| dir.join(format!("{command}{suffix}")))
        })
        .filter(|candidate| runnable(candidate))
}

fn runnable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    #[cfg(unix)]
    let runnable = {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let runnable = true;
    metadata.is_file() && runnable
}

/// The version a program prints when it is started with these arguments: the
/// first three numbers joined by dots in what it writes. [`None`] when it
/// cannot be started or prints no version.
#[must_use]
pub fn version(executable: &Path, args: &[String], search_path: &OsStr) -> Option<String> {
    let output = Command::new(executable)
        .args(args)
        .env("PATH", search_path)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    version_in(&text)
}

/// The first version in a text: the first three numbers of the first run of
/// at least three numbers joined by dots.
#[must_use]
pub fn version_in(text: &str) -> Option<String> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|word| word.trim_matches('.'))
        .find_map(|word| {
            let parts: Vec<&str> = word.split('.').collect();
            (parts.len() >= 3 && parts[..3].iter().all(|part| !part.is_empty()))
                .then(|| parts[..3].join("."))
        })
}

/// The user's cache directory on this operating system, from the variables of
/// the environment: `~/Library/Caches` on macOS, `%LOCALAPPDATA%` on Windows,
/// `$XDG_CACHE_HOME` or `~/.cache` elsewhere. Only an absolute path counts.
#[must_use]
pub fn cache_home(os: &str, var: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let absolute = |name: &str| {
        var(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    match os {
        "macos" => absolute("HOME").map(|home| home.join("Library").join("Caches")),
        "windows" => absolute("LOCALAPPDATA"),
        _ => {
            absolute("XDG_CACHE_HOME").or_else(|| absolute("HOME").map(|home| home.join(".cache")))
        }
    }
}
