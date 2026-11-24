//! The Python check over one uv project, the unit's path: every Python file
//! the unit holds is formatted the way Ruff writes it, passes Ruff and passes
//! Pyright in strict mode; the tests pass with every line and branch covered;
//! and the project's wheel installs into a clean environment, where its
//! commands answer `--help` and its packages import.
//!
//! Like the Rust check it runs code of the repository. The project is a uv
//! project: its `pyproject.toml` lies at the unit's path, and its `uv.lock`
//! there or above it, at the root of the uv workspace it belongs to, as uv
//! finds it. The interpreter is the one `.python-version` names, at the unit or
//! at that root, as `rust-toolchain.toml` names the toolchain of the Rust
//! check; uv installs it from its own downloads and never takes one of the
//! computer. uv, Ruff, Pyright and coverage.py are the programs the register
//! pins for the check, found as for every other check; pytest and its plugins
//! are the project's, from its lockfile, since they have to import its code.
//! coverage.py is laid over the project's environment as the wheel the
//! register pins, which the check fetches into its folder and holds to its
//! digest. Everything uv installs goes into the check's folder in the
//! temporary directory, which the check removes again, and nothing is written
//! into the checkout that outlives the check.
//!
//! The steps run in this order, each in a group of the log, and each one runs
//! even when one before it failed, so that one run reports every finding;
//! only a step that what comes after needs ends the check, or leaves out the
//! steps that need it. Every program runs with the settings below
//! ([`VARIABLES`], [`REMOVED`]): uv keeps its cache, the Python it installs,
//! the links it would put on a `PATH` and the project's environment in the
//! check's folder, takes only a Python it downloads itself, and prints no
//! progress bars; no interpreter writes bytecode into the checkout, and no
//! variable of the computer steers which Python runs or what it imports. Other
//! settings still apply, and the check does not hunt them down: the variables
//! of uv it does not set, such as an index, and uv's configuration files in
//! the user's folders. A hosted runner has none, so they are ways a run on a
//! developer's computer can differ from the gate's, whose result is the one
//! that counts.
//!
//! The files are the ones `files` names for the check and the unit. A file an
//! `[[exclude]]` entry takes out is handed to no tool: Ruff and Pyright get the
//! list, pytest does not collect an excluded file, and coverage does not judge
//! one. An entry that takes out the unit's whole folder leaves nothing to
//! check.
//!
//! Python runs in three places, each because only the interpreter can answer:
//! whether pytest can be imported from the project's environment
//! ([`PYTEST_THERE`]), whether a file nobody measured holds a statement, as
//! Python's own parser reads it ([`EMPTY`]), and whether a package of the
//! wheel imports in the clean environment ([`IMPORTS`]). Everything else is
//! Rust.

pub mod coverage;
pub mod pyright;
pub mod pytest;
pub mod ruff;
pub mod wheel;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::rust::{step, stream};
use super::{Session, Tool, has_extension};
use crate::names::ProgramName;
use crate::path::RelativePath;
use crate::report::shown;
use crate::setup::fetch;
use crate::setup::install::search_path_with;

/// The variables every program of the check gets, each with the folder in the
/// check's folder it names, or its value.
pub const VARIABLES: &[(&str, Setting)] = &[
    ("UV_CACHE_DIR", Setting::Folder("uv-cache")),
    ("UV_PYTHON_INSTALL_DIR", Setting::Folder("python")),
    ("UV_PYTHON_BIN_DIR", Setting::Folder("python-bin")),
    ("UV_PROJECT_ENVIRONMENT", Setting::Folder(ENVIRONMENT)),
    ("UV_PYTHON_PREFERENCE", Setting::Value("only-managed")),
    ("UV_PYTHON_DOWNLOADS", Setting::Value("automatic")),
    ("UV_NO_PROGRESS", Setting::Value("1")),
    ("PYTHONDONTWRITEBYTECODE", Setting::Value("1")),
];

/// The variables no program of the check inherits: they would steer which
/// Python runs or what it imports.
pub const REMOVED: &[&str] = &[
    "PYTHONPATH",
    "PYTHONHOME",
    "PYTHONSTARTUP",
    "VIRTUAL_ENV",
    "CONDA_PREFIX",
];

/// The folder of the project's environment in the check's folder.
const ENVIRONMENT: &str = "environment";

/// What a variable of [`VARIABLES`] is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// A folder in the check's folder, by its name there.
    Folder(&'static str),
    /// A value.
    Value(&'static str),
}

/// The Python that tells whether pytest can be imported from an
/// environment: its exit status is 0 when it can. Only the interpreter knows
/// what it imports.
pub const PYTEST_THERE: &str =
    "import importlib.util, sys; sys.exit(importlib.util.find_spec(\"pytest\") is None)";

/// The Python that tells whether the file it is handed holds no statement
/// beyond its docstring: its exit status is 0 when it holds none, and not 0
/// when it holds one or cannot be read or parsed. Python's own parser is the
/// one that decides what a statement is.
pub const EMPTY: &str = "import ast, sys\n\
with open(sys.argv[1], \"rb\") as file:\n    body = ast.parse(file.read()).body\n\
if body and isinstance(body[0], ast.Expr) and isinstance(body[0].value, ast.Constant):\n    \
body = body[1:]\n\
sys.exit(1 if body else 0)\n";

/// The Python that imports the module it is handed: the smoke test of a
/// package of the wheel.
pub const IMPORTS: &str = "import importlib, sys; importlib.import_module(sys.argv[1])";

/// Where the check works: the unit, and its folder in the temporary directory.
pub(super) struct Paths {
    /// The repository root, as an absolute path without links.
    root: PathBuf,
    /// The unit's folder, as an absolute path.
    dir: PathBuf,
    /// The unit, as the plan names it.
    unit: String,
    /// The unit's `pyproject.toml`, from the repository root.
    manifest: String,
    /// The check's folder in the temporary directory.
    temp: PathBuf,
    /// Whether the computer runs Windows, where an environment keeps its
    /// programs elsewhere.
    windows: bool,
}

impl Paths {
    /// A file or folder in the check's folder.
    fn temp(&self, name: &str) -> PathBuf {
        self.temp.join(name)
    }

    /// The project's environment.
    fn environment(&self) -> PathBuf {
        self.temp(ENVIRONMENT)
    }
}

/// The programs of the check.
pub(super) struct Programs {
    uv: Tool,
    ruff: Tool,
    pyright: Tool,
    /// The wheel of coverage.py, in the check's folder.
    coverage: PathBuf,
    /// The lowest Python coverage.py runs on, and its version, as the
    /// register pins them.
    coverage_needs: (String, String),
}

/// The project's interpreter, and how Ruff is configured for the unit.
pub(super) struct Interpreter {
    /// The interpreter uv installed.
    python: PathBuf,
    /// Its version, major and minor.
    version: (u64, u64),
    /// The configuration Ruff is handed.
    ruff_config: PathBuf,
    /// The folder Ruff runs in, from the repository root.
    ruff_folder: String,
}

/// A file or folder in a folder, as a path from the root: `name` itself in
/// the root.
#[must_use]
pub fn at(folder: &str, name: &str) -> String {
    if folder == "." {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

/// The folder above a path from the root, `.` for a path in the root, and
/// [`None`] for the root itself.
#[must_use]
pub fn above(path: &str) -> Option<String> {
    if path == "." {
        return None;
    }
    Some(
        path.rsplit_once('/')
            .map_or(".", |(parent, _)| parent)
            .to_owned(),
    )
}

/// The folder and every folder above it up to the root, nearest first.
#[must_use]
pub fn upwards(folder: &str) -> Vec<String> {
    std::iter::successors(Some(folder.to_owned()), |folder| above(folder)).collect()
}

/// What Git tracks below the unit with an extension of the check and the
/// list of the check's files does not hold: what an `[[exclude]]` entry takes
/// out.
#[must_use]
pub fn excluded(files: &[String], tracked: &[RelativePath], unit: &RelativePath) -> Vec<String> {
    tracked
        .iter()
        .filter(|path| unit.contains(path))
        .map(ToString::to_string)
        .filter(|path| {
            (has_extension(path, "py") || has_extension(path, "pyi")) && !files.contains(path)
        })
        .collect()
}

/// The numbers of a version a program prints, such as `Python 3.14.0`: the
/// major and minor version, [`None`] when it names none.
#[must_use]
pub fn major_minor(text: &str) -> Option<(u64, u64)> {
    let version = text.split_whitespace().find(|word| {
        word.chars()
            .next()
            .is_some_and(|first| first.is_ascii_digit())
    })?;
    let mut numbers = version
        .split(|c: char| !c.is_ascii_digit())
        .map(str::parse::<u64>);
    match (numbers.next(), numbers.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => Some((major, minor)),
        _ => None,
    }
}

/// A program in an environment uv made in `dir`.
#[must_use]
pub fn in_environment(dir: &Path, program: &str, windows: bool) -> PathBuf {
    if windows {
        dir.join("Scripts").join(format!("{program}.exe"))
    } else {
        dir.join("bin").join(program)
    }
}

/// Runs the check; whether it found nothing.
pub(super) fn run(session: &mut Session<'_, '_, '_>) -> bool {
    let unit = session.unit.to_string();
    let taken = excluded(
        &session.files,
        session.context.packages.tracked,
        &session.unit,
    );
    if !taken.is_empty() {
        session
            .log
            .line("Taken out by an [[exclude]] entry, and handed to no tool:");
        for file in &taken {
            session.log.line(&shown(file));
        }
    }
    // The project the files belong to. pyproject.toml is read where the unit
    // lies, as Cargo.toml is for the Rust check; a folder of loose scripts
    // would otherwise leave the tests and the 100 % coverage out.
    let manifest = at(&unit, "pyproject.toml");
    let tracked = session
        .context
        .packages
        .tracked
        .iter()
        .any(|path| path.to_string() == manifest);
    if !tracked {
        session.log.error(
            None,
            None,
            &format!(
                "check-python checks a uv project, and '{unit}', the unit's path, holds no \
                 pyproject.toml that Git tracks; write one there (uv init), or exclude the \
                 Python files with an [[exclude]] entry and a reason"
            ),
        );
        return false;
    }
    let Some(temp) = session.folder(|_| Vec::new()) else {
        return false;
    };
    let paths = Paths {
        root: session.context.root.to_path_buf(),
        dir: session.context.root.join(&unit),
        unit,
        manifest,
        temp,
        windows: session.context.env.windows,
    };
    steps(session, &paths, &taken)
}

/// The steps, in their order.
fn steps(session: &mut Session<'_, '_, '_>, paths: &Paths, taken: &[String]) -> bool {
    let Some(programs) = step(session, "Find uv, Ruff, Pyright and coverage.py", |s| {
        programs(s, paths)
    }) else {
        return false;
    };
    let Some(python) = step(session, "Install the project's Python", |s| {
        interpreter(s, paths, &programs)
    }) else {
        return false;
    };
    let synced = step(session, "Install the project", |s| {
        sync(s, paths, &programs).then_some(())
    })
    .is_some();
    let mut passed = synced;
    passed &= step(session, "Every Python file is formatted", |s| {
        ruff::format(s, paths, &programs, &python).then_some(())
    })
    .is_some();
    passed &= step(session, "Every Python file passes Ruff", |s| {
        ruff::lint(s, paths, &programs, &python).then_some(())
    })
    .is_some();
    if !synced {
        return false;
    }
    passed &= step(
        session,
        "Every Python file passes Pyright in strict mode",
        |s| pyright::check(s, paths, &programs, &python).then_some(()),
    )
    .is_some();
    passed &= step(session, "The tests pass", |s| {
        pytest::check(s, paths, &programs, taken).then_some(())
    })
    .is_some();
    passed &= step(session, "Every line and branch is covered", |s| {
        coverage::check(s, paths, &programs, &python).then_some(())
    })
    .is_some();
    passed &= step(session, "The wheel installs and runs", |s| {
        wheel::check(s, paths, &programs, &python).then_some(())
    })
    .is_some();
    passed
}

/// A command that starts `program` in `dir` with this search path, with the
/// settings of [`VARIABLES`] and without those of [`REMOVED`], and with
/// nothing on its standard input.
fn command(paths: &Paths, program: impl AsRef<OsStr>, dir: &Path, search: &OsStr) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(dir)
        .env("PATH", search)
        .stdin(Stdio::null());
    for name in REMOVED {
        command.env_remove(name);
    }
    for (name, setting) in VARIABLES {
        match setting {
            Setting::Folder(folder) => command.env(name, paths.temp(folder)),
            Setting::Value(value) => command.env(name, value),
        };
    }
    command
}

/// A command that starts uv in the unit's folder.
fn uv(paths: &Paths, programs: &Programs) -> Command {
    command(
        paths,
        &programs.uv.executable,
        &paths.dir,
        &programs.uv.search_path,
    )
}

/// Runs uv with these arguments in the unit's folder, its output in the log:
/// whether it succeeded.
fn call_uv(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    programs: &Programs,
    args: &[&OsStr],
) -> bool {
    let mut command = uv(paths, programs);
    command.args(args);
    stream(session, command, "uv")
}

/// An option with a path behind it, as one argument.
fn prefixed(option: &str, path: &Path) -> std::ffi::OsString {
    let mut argument = std::ffi::OsString::from(option);
    argument.push(path);
    argument
}

/// What a program printed on standard output, trimmed, when it succeeded;
/// what it printed on standard error goes into the log.
fn printed(session: &mut Session<'_, '_, '_>, command: Command, what: &str) -> Option<String> {
    let output = session.output(command, what)?;
    session.log.lines(&String::from_utf8_lossy(&output.stderr));
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// uv, Ruff and Pyright as the register pins them and `setup` finds them,
/// and the wheel of coverage.py, fetched into the check's folder and held to
/// the digest the register pins.
fn programs(session: &mut Session<'_, '_, '_>, paths: &Paths) -> Option<Programs> {
    let uv = session.tool("uv", false);
    let ruff = session.tool("ruff", false);
    let pyright = session.tool("pyright", false);
    let coverage = coverage_wheel(session, paths);
    let (Some(uv), Some(ruff), Some(pyright), Some((coverage, needs))) =
        (uv, ruff, pyright, coverage)
    else {
        return None;
    };
    let mut shown = true;
    for (tool, what) in [(&uv, "uv"), (&ruff, "ruff"), (&pyright, "pyright")] {
        let mut version = command(paths, &tool.executable, &paths.root, &tool.search_path);
        version.arg("--version");
        shown &= stream(session, version, what);
    }
    session.log.line(&format!(
        "coverage.py {}, as {}",
        needs.1,
        coverage.display()
    ));
    shown.then_some(Programs {
        uv: Tool {
            search_path: search_path_with(&uv.executable, session.context.env),
            ..uv
        },
        ruff,
        pyright,
        coverage,
        coverage_needs: needs,
    })
}

/// The wheel of coverage.py the register pins, fetched into the check's
/// folder under its own name, which uv reads it by, and held to its digest;
/// with the lowest Python it runs on and its version.
fn coverage_wheel(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
) -> Option<(PathBuf, (String, String))> {
    let register = session.context.register;
    let pinned = ProgramName::new("coverage")
        .ok()
        .and_then(|name| register.program.get(&name))
        .and_then(|program| {
            program
                .download
                .values()
                .next()
                .map(|download| (program, download))
        });
    let Some((program, download)) = pinned else {
        session.log.error(
            None,
            None,
            "the register pins no wheel of coverage.py for the python check",
        );
        return None;
    };
    match fetch::download(download, &paths.temp, session.context.env) {
        Ok(wheel) => Some((
            wheel,
            (
                program.runtime_minimum.clone().unwrap_or_default(),
                program.version.clone(),
            ),
        )),
        Err(error) => {
            session.log.error(
                None,
                None,
                &format!("coverage.py {} cannot be fetched: {error}", program.version),
            );
            None
        }
    }
}

/// The interpreter `.python-version` names, at the unit or at the root of
/// its uv workspace, installed by uv, and Ruff's configuration for the unit.
fn interpreter(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    programs: &Programs,
) -> Option<Interpreter> {
    let workspace = upwards(&paths.unit)
        .into_iter()
        .find(|folder| paths.root.join(at(folder, "uv.lock")).is_file());
    let Some(workspace) = workspace else {
        session.log.error(
            Some(&paths.manifest),
            None,
            &format!(
                "uv finds no uv.lock at '{}' or above it: check-python installs the project as \
                 its lockfile pins it (uv sync --locked); write it with 'uv lock' and commit it",
                paths.unit
            ),
        );
        return None;
    };
    session.log.line(&format!("The uv workspace: {workspace}"));
    let pin = [
        at(&paths.unit, ".python-version"),
        at(&workspace, ".python-version"),
    ]
    .into_iter()
    .find(|candidate| paths.root.join(candidate).is_file());
    let Some(pin) = pin else {
        session.log.error(
            Some(&paths.manifest),
            None,
            &format!(
                "neither '{}' nor the root of its uv workspace holds a .python-version: it names \
                 the interpreter check-python installs, as rust-toolchain.toml names the \
                 toolchain of check-rust; write it with 'uv python pin' and commit it",
                paths.unit
            ),
        );
        return None;
    };
    let named = std::fs::read_to_string(paths.root.join(&pin)).unwrap_or_default();
    session.log.line(&format!(
        "The interpreter: {}, as {pin} names it",
        shown(named.lines().next().unwrap_or_default())
    ));
    let installed = call_uv(
        session,
        paths,
        programs,
        &[
            OsStr::new("python"),
            OsStr::new("install"),
            OsStr::new("--no-bin"),
        ],
    );
    if !installed {
        return None;
    }
    let mut find = uv(paths, programs);
    find.args(["python", "find"]);
    let python = PathBuf::from(printed(session, find, "uv")?);
    let mut asked = command(paths, &python, &paths.dir, &programs.uv.search_path);
    asked.arg("--version");
    let said = printed(session, asked, "python")?;
    session.log.line(&said);
    let (minimum, version) = &programs.coverage_needs;
    let wanted = major_minor(minimum).unwrap_or_default();
    let version_found = major_minor(&said).filter(|found| *found >= wanted);
    let Some(numbers) = version_found else {
        session.log.error(
            Some(&pin),
            None,
            &format!(
                "check-python measures coverage with coverage.py {version}, which runs on Python \
                 {minimum} or later, and this names {said}"
            ),
        );
        return None;
    };
    let (ruff_config, ruff_folder) = ruff::configure(session, paths)?;
    Some(Interpreter {
        python,
        version: numbers,
        ruff_config,
        ruff_folder,
    })
}

/// The project as its lockfile pins it, with its default dependency groups,
/// into an environment in the check's folder. A lockfile that no longer
/// matches `pyproject.toml` fails here, as it would for anyone else.
fn sync(session: &mut Session<'_, '_, '_>, paths: &Paths, programs: &Programs) -> bool {
    if call_uv(
        session,
        paths,
        programs,
        &[OsStr::new("sync"), OsStr::new("--locked")],
    ) {
        return true;
    }
    session.log.error(
        Some(&paths.manifest),
        None,
        "uv cannot install the project as its uv.lock pins it (uv sync --locked); its message \
         stands in the log",
    );
    false
}
