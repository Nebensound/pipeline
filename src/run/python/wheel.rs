//! The wheel: built the way `uv build` builds it, installed into a clean
//! environment of the same interpreter with the project's locked runtime
//! dependencies, and tried there.
//!
//! The runtime dependencies are those `uv export --frozen --no-default-groups`
//! gives: no dependency group, `dev` or another of `default-groups`; the
//! project itself is left out and added with `--no-deps`. In the clean
//! environment every command of `[project.scripts]` answers `--help` within a
//! minute ([`PATIENCE`]), and every package and module at the top of the wheel
//! imports, from the check's folder outside the checkout, each with nothing on
//! its standard input. A project without a build backend is no installable
//! package and fails.
//!
//! The build installs the build backend `[build-system]` names the way uv
//! builds any project: from the package index, as `requires` asks for it, and
//! without a lockfile, since `uv.lock` pins no build dependency.
//!
//! No version is stored in the tree and the pipeline writes none for Python,
//! so the wheel gets a fixed placeholder version for this step alone:
//! `pyproject.toml` holds `version = "0.0.0"` and no `version` in `dynamic`
//! while the wheel is built, and is put back as committed afterwards. The
//! wheel is never handed on.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use regex::Regex;
use toml::{Table, Value};

use super::{
    IMPORTS, Interpreter, Paths, Programs, call_uv, command, has_extension, in_environment,
};
use crate::run::Session;

/// How long a command of the wheel gets to answer `--help` before it is
/// stopped.
pub const PATIENCE: Duration = Duration::from_secs(60);

/// The placeholder version the wheel is built with.
pub const PLACEHOLDER: &str = "0.0.0";

/// A table header of a TOML file, with an optional comment behind it.
static HEADER: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^\s*\[([^\[\]]+)\]\s*(#.*)?$").ok());

/// The start of an array of tables.
static ARRAY: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^\s*\[\[").ok());

/// A `version` key.
static VERSION: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^\s*version\s*=").ok());

/// A `dynamic` key.
static DYNAMIC: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^\s*dynamic\s*=").ok());

/// Whether a regular expression of this module matches a line.
fn matches(pattern: &LazyLock<Option<Regex>>, line: &str) -> bool {
    pattern
        .as_ref()
        .is_some_and(|pattern| pattern.is_match(line))
}

/// Why `pyproject.toml` cannot be built as a wheel, or [`None`] when it can:
/// it declares no build backend, or `tool.uv.package = false`.
#[must_use]
pub fn no_backend(manifest: &Table) -> bool {
    let package = manifest
        .get("tool")
        .and_then(|tool| tool.get("uv"))
        .and_then(|uv| uv.get("package"));
    !manifest.contains_key("build-system") || package == Some(&Value::Boolean(false))
}

/// `pyproject.toml` with the placeholder version: in its `[project]` table,
/// `version = "0.0.0"` and the `dynamic` it names less `version`, written
/// right below the table's header in place of the lines that name them.
/// [`None`] when the result does not hold the placeholder, as when the file
/// does not write its project's fields in a `[project]` table.
#[must_use]
pub fn placeholder(source: &str, manifest: &Table) -> Option<String> {
    let dynamic: Vec<Value> = manifest
        .get("project")
        .and_then(|project| project.get("dynamic"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.as_str() != Some("version"))
        .cloned()
        .collect();
    let mut out = String::new();
    let mut inside = false;
    let mut skipping = false;
    for line in source.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        let header = HEADER.as_ref().and_then(|pattern| pattern.captures(bare));
        if header.is_some() || matches(&ARRAY, bare) {
            skipping = false;
            inside = header.is_some_and(|header| header[1].trim() == "project");
            out.push_str(line);
            if inside {
                let _ = writeln!(out, "version = \"{PLACEHOLDER}\"");
                let _ = writeln!(out, "dynamic = {}", Value::Array(dynamic.clone()));
            }
            continue;
        }
        if skipping {
            skipping = !line.contains(']');
            continue;
        }
        if inside && matches(&VERSION, bare) {
            continue;
        }
        if inside && matches(&DYNAMIC, bare) {
            skipping = line.contains('[') && !line.contains(']');
            continue;
        }
        out.push_str(line);
    }
    let written = toml::from_str::<Table>(&out).ok()?;
    let project = written.get("project");
    let version = project.and_then(|project| project.get("version"));
    let dynamic_version = project
        .and_then(|project| project.get("dynamic"))
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.as_str() == Some("version"))
        });
    (version.and_then(Value::as_str) == Some(PLACEHOLDER) && !dynamic_version).then_some(out)
}

/// Whether a name is one Python takes as an identifier.
#[must_use]
pub fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
}

/// The commands an `entry_points.txt` names under `[console_scripts]`.
#[must_use]
pub fn console_scripts(text: &str) -> Vec<String> {
    let mut inside = false;
    let mut commands = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(['#', ';']) || line.starts_with([' ', '\t']) {
            continue;
        }
        if let Some(section) = trimmed
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            inside = section == "console_scripts";
            continue;
        }
        if let Some((name, _)) = trimmed.split_once(['=', ':']).filter(|_| inside) {
            commands.push(name.trim().to_owned());
        }
    }
    commands
}

/// The names at the top of a wheel that import: each package folder and
/// each module, a `.py` file or a compiled one, whose name is an identifier;
/// its metadata and data are none of them.
#[must_use]
pub fn tops(names: &[String]) -> Vec<String> {
    let mut tops: Vec<String> = names
        .iter()
        .filter_map(|name| {
            let (first, rest) = name.split_once('/').unwrap_or((name, ""));
            if has_extension(first, "dist-info") || has_extension(first, "data") {
                return None;
            }
            let module = if rest.is_empty() {
                first
                    .strip_suffix(".py")
                    .or_else(|| {
                        (has_extension(first, "so") || has_extension(first, "pyd"))
                            .then(|| first.split('.').next().unwrap_or_default())
                    })
                    .unwrap_or_default()
            } else {
                first
            };
            identifier(module).then(|| module.to_owned())
        })
        .collect();
    tops.sort();
    tops.dedup();
    tops
}

/// What a wheel offers to be tried: the commands its `entry_points.txt`
/// names, and the names at its top that import, each sorted.
///
/// # Errors
///
/// What keeps the wheel from being read.
pub fn offered(wheel: &Path) -> Result<(Vec<String>, Vec<String>), String> {
    read_wheel(wheel).map_err(|error| error.to_string())
}

/// [`offered`], with the error of the archive.
fn read_wheel(wheel: &Path) -> zip::result::ZipResult<(Vec<String>, Vec<String>)> {
    let mut archive = zip::ZipArchive::new(File::open(wheel)?)?;
    // A name is read as UTF-8 and otherwise as CP437, which decodes every
    // byte, so no name fails; `flatten` takes the names out of their results.
    let names: Vec<String> = archive
        .file_names()
        .flatten()
        .map(Cow::into_owned)
        .collect();
    let mut commands = Vec::new();
    for name in &names {
        let parts: Vec<&str> = name.split('/').collect();
        if let [info, "entry_points.txt"] = parts[..]
            && has_extension(info, "dist-info")
        {
            let mut text = String::new();
            archive.by_name(name).and_then(|mut file| {
                file.read_to_string(&mut text)
                    .map_err(zip::result::ZipError::from)
            })?;
            commands.extend(console_scripts(&text));
        }
    }
    commands.sort();
    Ok((commands, tops(&names)))
}

/// Starts a command with its output going to `log` and waits for it, at
/// most `patience`: how it ended, or [`None`] when it did not end in time and
/// was stopped.
///
/// On Unix the command leads a process group of its own, and a command that
/// is stopped takes that group with it, as `timeout` stopped it: a launcher
/// that started a server and waits for it leaves no server running, holding
/// `log` open.
///
/// # Errors
///
/// When the command cannot be started or waited for.
pub fn answer(
    mut command: Command,
    log: &Path,
    patience: Duration,
) -> std::io::Result<Option<ExitStatus>> {
    let file = File::create(log)?;
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child: Child = file.try_clone().and_then(|copy| {
        command.stdout(copy).stderr(file);
        command.spawn()
    })?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(None) => {}
            ended => return ended,
        }
        if start.elapsed() >= patience {
            #[cfg(unix)]
            stop_group(&child);
            return child.kill().and_then(|()| child.wait()).map(|_| None);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Kills the process group `child` leads, and with it whatever the command
/// started. Its result is left aside: it fails only when no process of the
/// group is left, and the command itself is killed right after, where an
/// error that matters shows.
#[cfg(unix)]
fn stop_group(child: &Child) {
    use rustix::process::{Pid, Signal, kill_process_group};
    let _ = kill_process_group(Pid::from_child(child), Signal::KILL);
}

/// What is said about a command that did not answer `--help` as it should,
/// by how it ended; [`None`] when it answered.
#[must_use]
pub fn unanswered(command: &str, ended: Option<ExitStatus>) -> Option<String> {
    match ended {
        None => Some(format!(
            "smoke test of the wheel: '{command} --help' did not answer within {} seconds in a \
             clean environment and was stopped; a command that starts a service has to parse \
             its command line first",
            PATIENCE.as_secs()
        )),
        Some(status) if status.success() => None,
        Some(status) => Some(format!(
            "smoke test of the wheel: '{command} --help' failed {} in a clean environment; its \
             output stands in the log",
            crate::run::ended(status)
        )),
    }
}

/// A clean environment of the project's interpreter, with the locked runtime
/// dependencies and the wheel without its own: whether it was made.
fn install(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    programs: &Programs,
    python: &Interpreter,
    wheel: &Path,
) -> bool {
    let clean = paths.temp("clean");
    let clean_python = in_environment(&clean, "python", paths.windows);
    call_uv(
        session,
        paths,
        programs,
        &[
            OsStr::new("venv"),
            OsStr::new("--quiet"),
            OsStr::new("--python"),
            python.python.as_os_str(),
            clean.as_os_str(),
        ],
    ) && call_uv(
        session,
        paths,
        programs,
        &[
            OsStr::new("pip"),
            OsStr::new("install"),
            OsStr::new("--quiet"),
            OsStr::new("--python"),
            clean_python.as_os_str(),
            OsStr::new("--requirement"),
            paths.temp("requirements.txt").as_os_str(),
        ],
    ) && call_uv(
        session,
        paths,
        programs,
        &[
            OsStr::new("pip"),
            OsStr::new("install"),
            OsStr::new("--quiet"),
            OsStr::new("--python"),
            clean_python.as_os_str(),
            OsStr::new("--no-deps"),
            wheel.as_os_str(),
        ],
    )
}

/// The wheel installs and runs.
pub(super) fn check(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    programs: &Programs,
    python: &Interpreter,
) -> bool {
    let Some(wheel) = build(session, paths, programs) else {
        return false;
    };
    let clean = paths.temp("clean");
    let clean_python = in_environment(&clean, "python", paths.windows);
    if !install(session, paths, programs, python, &wheel) {
        return false;
    }
    let (commands, imports) = match offered(&wheel) {
        Ok(offered) => offered,
        Err(error) => {
            session.log.error(
                Some(&paths.manifest),
                None,
                &format!("the wheel cannot be read: {error}"),
            );
            return false;
        }
    };
    let mut passed = true;
    for name in &commands {
        session.log.line(&format!("$ {name} --help"));
        let mut run = command(
            paths,
            in_environment(&clean, name, paths.windows),
            &paths.temp,
            &programs.uv.search_path,
        );
        run.arg("--help");
        let message = match said(session, paths, run) {
            Ok(ended) => unanswered(name, ended),
            Err(error) => Some(format!(
                "smoke test of the wheel: '{name} --help' cannot be started in a clean \
                 environment: {error}"
            )),
        };
        if let Some(message) = message {
            session.log.error(Some(&paths.manifest), None, &message);
            passed = false;
        }
    }
    for name in &imports {
        session.log.line(&format!("$ python -c 'import {name}'"));
        let mut run = command(paths, &clean_python, &paths.temp, &programs.uv.search_path);
        run.args(["-I", "-c", IMPORTS, name]);
        let imported = said(session, paths, run)
            .is_ok_and(|ended| ended.is_some_and(|status| status.success()));
        if !imported {
            session.log.error(
                Some(&paths.manifest),
                None,
                &format!(
                    "smoke test of the wheel: 'import {name}' failed in a clean environment; its \
                     output stands in the log"
                ),
            );
            passed = false;
        }
    }
    if commands.is_empty() && imports.is_empty() {
        session.log.error(
            Some(&paths.manifest),
            None,
            "smoke test of the wheel: it holds no command and no package to try",
        );
        passed = false;
    }
    passed
}

/// Runs a command of the smoke test with [`PATIENCE`] and writes what it
/// printed into the log: how it ended.
///
/// # Errors
///
/// When it cannot be started.
fn said(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    command: Command,
) -> std::io::Result<Option<ExitStatus>> {
    let log = paths.temp("smoke-test.log");
    let ended = answer(command, &log, PATIENCE);
    let text = fs::read(&log).unwrap_or_default();
    session.log.lines(&String::from_utf8_lossy(&text));
    ended
}

/// The runtime dependencies as the lockfile pins them, the wheel built with
/// the placeholder version, and `pyproject.toml` put back as committed: the
/// wheel, or nothing, with why written.
fn build(session: &mut Session<'_, '_, '_>, paths: &Paths, programs: &Programs) -> Option<PathBuf> {
    let requirements = paths.temp("requirements.txt");
    let mut export = super::uv(paths, programs);
    export
        .args([
            "export",
            "--frozen",
            "--no-default-groups",
            "--no-emit-project",
            "--no-editable",
            "--no-header",
            "--output-file",
        ])
        .arg(&requirements);
    super::printed(session, export, "uv")?;
    let path = paths.dir.join("pyproject.toml");
    let read = fs::read_to_string(&path).map_err(|error| error.to_string());
    let parsed = read.and_then(|committed| {
        toml::from_str::<Table>(&committed)
            .map(|manifest| (committed, manifest))
            .map_err(|error| error.to_string())
    });
    let (committed, manifest) = match parsed {
        Ok(read) => read,
        Err(error) => {
            session.log.error(
                Some(&paths.manifest),
                None,
                &format!("pyproject.toml cannot be read: {error}"),
            );
            return None;
        }
    };
    if no_backend(&manifest) {
        session.log.error(
            Some(&paths.manifest),
            None,
            "pyproject.toml declares no build backend ([build-system]), or `tool.uv.package = \
             false`, so no wheel can be built: check-python checks every Python unit as an \
             installable package; declare one, such as uv_build",
        );
        return None;
    }
    let Some(changed) = placeholder(&committed, &manifest) else {
        session.log.error(
            Some(&paths.manifest),
            None,
            "check-python cannot give the wheel its placeholder version: it writes `version` \
             into the [project] table, and this pyproject.toml does not hold its project's \
             fields in one",
        );
        return None;
    };
    placed(session, paths, programs, &path, &committed, &changed)
}

/// The wheel built with `changed`, the placeholder version, in `path`, and
/// `path` put back to `committed` afterwards: the wheel, or nothing, with why
/// written.
fn placed(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    programs: &Programs,
    path: &Path,
    committed: &str,
    changed: &str,
) -> Option<PathBuf> {
    let dist = paths.temp("dist");
    let written = match fs::write(path, changed) {
        Ok(()) => true,
        Err(error) => {
            session.log.error(
                Some(&paths.manifest),
                None,
                &format!("cannot be given the placeholder version {PLACEHOLDER}: {error}"),
            );
            false
        }
    };
    let built = written && {
        session.log.line(&format!(
            "The wheel gets the placeholder version {PLACEHOLDER}."
        ));
        call_uv(
            session,
            paths,
            programs,
            &[
                OsStr::new("build"),
                OsStr::new("--wheel"),
                OsStr::new("--out-dir"),
                dist.as_os_str(),
            ],
        )
    };
    // Written only where it differs: a placeholder that could not be written
    // left it as committed, unless the write broke off halfway.
    let restored = fs::read_to_string(path).is_ok_and(|text| text == committed)
        || match fs::write(path, committed) {
            Ok(()) => true,
            Err(error) => {
                session.log.error(
                    Some(&paths.manifest),
                    None,
                    &format!("cannot be put back as it is committed: {error}"),
                );
                false
            }
        };
    let mut wheels: Vec<PathBuf> = fs::read_dir(&dist)
        .into_iter()
        .flatten()
        .map_while(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ending| ending == "whl"))
        .collect();
    wheels.sort();
    let wheel = wheels.into_iter().next().filter(|_| built);
    let Some(wheel) = wheel.filter(|_| restored) else {
        if written && restored {
            session.log.error(
                Some(&paths.manifest),
                None,
                "uv build cannot build the wheel; its message stands in the log",
            );
        }
        return None;
    };
    session.log.line(&format!(
        "Built {}",
        wheel.file_name().unwrap_or_default().to_string_lossy()
    ));
    Some(wheel)
}
