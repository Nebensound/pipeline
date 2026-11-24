//! Ruff: its rules for the unit, the formatter and the linter.
//!
//! Ruff's rules are the pipeline's default (`rules/ruff.toml`) beneath the
//! repository's own Ruff configuration, which is laid over it rule by rule,
//! the way Ruff lays a configuration over the one its `extend` names: the
//! repository's `ignore` and `extend-select` change single rules and keep the
//! rest of the default, its `select` starts over, and every other setting it
//! names wins. The configuration is the one Ruff would find for the unit:
//! `.ruff.toml`, `ruff.toml` or a `pyproject.toml` with a `[tool.ruff]`
//! table, in the unit's folder or the nearest one above it, up to the
//! repository root. What its own `extend` names comes along with it, over the
//! default as well. Its paths are read from that configuration's folder,
//! where Ruff runs. Where it names no `target-version`, the one its project's
//! `requires-python` gives is set, as Ruff itself would infer it.
//!
//! Ruff runs in the folder of its configuration and is handed exactly the
//! files of the list: `--no-force-exclude` keeps its `exclude` and
//! `extend-exclude` from taking one of them out. It keeps no cache in the
//! checkout.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use toml::{Table, Value};

use super::{Interpreter, Paths, Programs, at, command, upwards};
use crate::rules::RUFF;
use crate::run::{Session, chunks, ended, relative};

/// The files Ruff takes its configuration from in a folder, in the order it
/// looks for them; a `pyproject.toml` counts only with a `[tool.ruff]` table.
pub const FILES: &[&str] = &[".ruff.toml", "ruff.toml", "pyproject.toml"];

/// The lowest Python a `requires-python` names, as Ruff infers its
/// `target-version` from it.
static REQUIRES: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r">=\s*3\.(\d+)").ok());

/// A variable in a path, as Ruff expands it: `$NAME` or `${NAME}`.
static VARIABLE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\$(\w+|\{[^}]*\})").ok());

/// One finding of Ruff's JSON output, as far as the check reads it.
#[derive(Debug, Clone, Deserialize)]
pub struct Finding {
    /// The file, as Ruff names it: absolute, or from the folder it runs in.
    pub filename: String,
    /// Where in it.
    pub location: Location,
    /// The rule, or `unformatted`; none for a file Ruff cannot read.
    pub code: Option<String>,
    /// What Ruff says.
    pub message: String,
}

/// A place in a file.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Location {
    /// The line, from 1.
    pub row: u64,
    /// The column, from 1.
    pub column: u64,
}

/// Ruff's settings in a file: all of it, or of a `pyproject.toml` its
/// `[tool.ruff]` table, [`None`] where it holds none.
///
/// # Errors
///
/// The parser's message, when the file is no TOML.
pub fn settings(path: &Path, source: &str) -> Result<Option<Table>, String> {
    let mut table: Table = toml::from_str(source).map_err(|error| error.to_string())?;
    if path.file_name().is_none_or(|name| name != "pyproject.toml") {
        return Ok(Some(table));
    }
    Ok(match table.remove("tool") {
        Some(Value::Table(mut tool)) => match tool.remove("ruff") {
            Some(Value::Table(ruff)) => Some(ruff),
            _ => None,
        },
        _ => None,
    })
}

/// A path as Ruff reads the value of an `extend`: a leading `~` is the home
/// folder, and `$NAME` and `${NAME}` are the variable of that name; a
/// variable that is not set stays as it is written.
#[must_use]
pub fn expanded(value: &str, var: &dyn Fn(&str) -> Option<String>) -> String {
    let value = match value.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            var("HOME").map_or_else(|| value.to_owned(), |home| format!("{home}{rest}"))
        }
        _ => value.to_owned(),
    };
    VARIABLE.as_ref().map_or(value.clone(), |pattern| {
        pattern
            .replace_all(&value, |captures: &regex::Captures<'_>| {
                let name = captures[1].trim_start_matches('{').trim_end_matches('}');
                var(name).unwrap_or_else(|| captures[0].to_owned())
            })
            .into_owned()
    })
}

/// The `target-version` Ruff infers from a `requires-python`, such as
/// `py311` for `>=3.11`.
#[must_use]
pub fn target_version(requires: &str) -> Option<String> {
    REQUIRES
        .as_ref()
        .and_then(|pattern| pattern.captures(requires))
        .map(|captures| format!("py3{}", &captures[1]))
}

/// What the check writes for one configuration: the one below it named by
/// `extend`, then its own settings.
#[must_use]
pub fn layer(below: &Path, settings: &Table) -> String {
    let mut table = settings.clone();
    table.insert(
        "extend".to_owned(),
        Value::String(below.to_string_lossy().into_owned()),
    );
    table.to_string()
}

/// Writes the pipeline's default and each configuration of the chain over
/// it into the check's folder, the last of the chain first: the file to hand
/// Ruff.
fn write_layers(paths: &Paths, chain: &[Table]) -> std::io::Result<PathBuf> {
    let mut below = paths.temp("ruff-default.toml");
    let mut files = vec![(below.clone(), RUFF.to_owned())];
    for (index, settings) in chain.iter().enumerate().rev() {
        let path = paths.temp(&format!("ruff-{index}.toml"));
        files.push((path.clone(), layer(&below, settings)));
        below = path;
    }
    files
        .iter()
        .try_for_each(|(path, text)| fs::write(path, text))
        .map(|()| below)
}

/// The file a path names, with links resolved where it exists.
fn real(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or(path.to_path_buf())
}

/// The configuration Ruff finds for the unit, in Ruff's order: its folder
/// from the root, its file, and its settings.
fn found(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
) -> Result<Option<(String, PathBuf, Table)>, ()> {
    for folder in upwards(&paths.unit) {
        for name in FILES {
            let path = paths.root.join(at(&folder, name));
            if !path.is_file() {
                continue;
            }
            if let Some(settings) = load(session, paths, &path)? {
                return Ok(Some((folder, path, settings)));
            }
        }
    }
    Ok(None)
}

/// The Ruff settings a file holds, or [`None`] for a `pyproject.toml`
/// without a `[tool.ruff]` table; a file that cannot be read is a finding.
fn load(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    path: &Path,
) -> Result<Option<Table>, ()> {
    let read = match fs::read_to_string(path) {
        Ok(source) => settings(path, &source),
        Err(error) => Err(error.to_string()),
    };
    read.map_err(|error| {
        fail(
            session,
            paths,
            path,
            &format!("Ruff's configuration cannot be read: {error}"),
        );
    })
}

/// A finding on a file of Ruff's configuration.
fn fail(session: &mut Session<'_, '_, '_>, paths: &Paths, path: &Path, text: &str) {
    let name = relative(&paths.root, &real(path));
    session.log.error(Some(&name.to_string_lossy()), None, text);
}

/// The chain of configurations from the one Ruff finds, each without its
/// `extend`, which names the next one.
fn chain(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    mut path: PathBuf,
    mut settings: Table,
) -> Option<Vec<Table>> {
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    loop {
        seen.insert(real(&path));
        let extend = settings.remove("extend");
        chain.push(settings);
        let Some(extend) = extend else {
            return Some(chain);
        };
        let written = extend
            .as_str()
            .map_or_else(|| extend.to_string(), str::to_owned);
        let next = path
            .parent()
            .unwrap_or(&paths.root)
            .join(expanded(&written, &|name| std::env::var(name).ok()));
        if seen.contains(&real(&next)) {
            fail(
                session,
                paths,
                &path,
                &format!("Ruff's `extend` comes back round to {written}"),
            );
            return None;
        }
        if !next.is_file() {
            fail(
                session,
                paths,
                &path,
                &format!("Ruff's `extend` names {written}, which does not exist"),
            );
            return None;
        }
        settings = load(session, paths, &next).ok()?.unwrap_or_default();
        path = next;
    }
}

/// Ruff's configuration for the unit, written into the check's folder: the
/// file to hand Ruff and the folder Ruff runs in, from the root.
pub(super) fn configure(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
) -> Option<(PathBuf, String)> {
    let found = found(session, paths).ok()?;
    let (folder, own, mut chain) = match found {
        Some((folder, path, settings)) => {
            let own = relative(&paths.root, &real(&path));
            (folder, Some(own), chain(session, paths, path, settings)?)
        }
        None => (paths.unit.clone(), None, Vec::new()),
    };
    if !chain
        .iter()
        .any(|settings| settings.contains_key("target-version"))
    {
        let requires = fs::read_to_string(paths.dir.join("pyproject.toml"))
            .ok()
            .and_then(|source| toml::from_str::<Table>(&source).ok())
            .and_then(|table| {
                table
                    .get("project")
                    .and_then(|project| project.get("requires-python"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        if let Some(target) = target_version(&requires) {
            if chain.is_empty() {
                chain.push(Table::new());
            }
            chain[0].insert("target-version".to_owned(), Value::String(target));
        }
    }
    let below = match write_layers(paths, &chain) {
        Ok(below) => below,
        Err(error) => {
            session.log.error(
                None,
                None,
                &format!("check-python cannot write Ruff's configuration: {error}"),
            );
            return None;
        }
    };
    match own {
        Some(own) => session.log.line(&format!(
            "Ruff's rules: the pipeline default, and laid over it {}",
            own.display()
        )),
        None => session.log.line("Ruff's rules: the pipeline default"),
    }
    Some((below, folder))
}

/// A file of the list as Ruff names it, from the folder it runs in.
#[must_use]
pub fn inside(folder: &str, file: &str) -> String {
    if folder == "." {
        file.to_owned()
    } else {
        file.strip_prefix(&format!("{folder}/"))
            .unwrap_or(file)
            .to_owned()
    }
}

/// A file Ruff names, from the repository root: an absolute one below the
/// root, or one from the folder it runs in. Absolute is what the platform
/// calls absolute, so `C:\...` on Windows as well; the result is separated
/// by `/` on every platform.
#[must_use]
pub fn from_root(root: &Path, folder: &str, filename: &str) -> String {
    let path = Path::new(filename);
    if path.is_absolute() {
        path.strip_prefix(root).map_or_else(
            |_| filename.to_owned(),
            |inside| {
                inside
                    .iter()
                    .map(OsStr::to_string_lossy)
                    .collect::<Vec<_>>()
                    .join("/")
            },
        )
    } else {
        at(folder, filename)
    }
}

/// Runs Ruff with these arguments over the files of the list, in as many
/// calls as the command line needs: whether each call ended with an exit
/// status of at most `worst` and printed its findings as JSON, and the
/// findings.
fn ruff(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    programs: &Programs,
    python: &Interpreter,
    args: &[&str],
    worst: i32,
) -> Option<(bool, Vec<Finding>)> {
    let folder = paths.root.join(&python.ruff_folder);
    let files: Vec<String> = session
        .files
        .iter()
        .map(|file| inside(&python.ruff_folder, file))
        .collect();
    let names: Vec<&str> = files.iter().map(String::as_str).collect();
    let mut clean = true;
    let mut findings = Vec::new();
    for chunk in chunks(&names) {
        let mut command = command(
            paths,
            &programs.ruff.executable,
            &folder,
            &programs.ruff.search_path,
        );
        command
            .args(args)
            .args([
                "--no-cache",
                "--no-force-exclude",
                "--output-format",
                "json",
            ])
            .arg("--config")
            .arg(&python.ruff_config)
            .arg("--")
            .args(chunk.iter().map(OsString::from));
        let output = session.output(command, "Ruff")?;
        session.log.lines(&String::from_utf8_lossy(&output.stderr));
        let report = serde_json::from_slice::<Vec<Finding>>(&output.stdout).ok();
        let within = output.status.code().is_some_and(|code| code <= worst);
        let Some(report) = report.filter(|_| within) else {
            session.log.lines(&String::from_utf8_lossy(&output.stdout));
            session
                .log
                .error(None, None, &unusable(args[0], output.status));
            return Some((false, Vec::new()));
        };
        clean &= output.status.success();
        findings.extend(report);
    }
    Some((clean, findings))
}

/// What is said when Ruff's subcommand gave nothing the check can read.
fn unusable(subcommand: &str, status: std::process::ExitStatus) -> String {
    let what = if subcommand == "format" {
        "Ruff's formatter could not check the files"
    } else {
        "Ruff could not lint the files"
    };
    format!(
        "{what} (it ended {}); its message stands in the log",
        ended(status)
    )
}

/// Writes a finding of Ruff as a line and as a finding on its file.
fn report(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    folder: &str,
    finding: &Finding,
    message: &str,
) {
    let file = from_root(&paths.root, folder, &finding.filename);
    let (row, column) = (finding.location.row, finding.location.column);
    session.log.line(&format!(
        "{file}:{row}:{column}: {} {}",
        finding.code.as_deref().unwrap_or("error"),
        finding.message
    ));
    session.log.error(
        Some(&file),
        Some((&row.to_string(), Some(&column.to_string()))),
        message,
    );
}

/// Every file is formatted the way Ruff writes it. A file that is not is
/// named at its first change; one Ruff cannot read at the place it stops.
pub(super) fn format(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    programs: &Programs,
    python: &Interpreter,
) -> bool {
    let Some((clean, findings)) = ruff(session, paths, programs, python, &["format", "--check"], 2)
    else {
        return false;
    };
    for finding in &findings {
        let message = if finding.code.as_deref() == Some("unformatted") {
            "not formatted the way Ruff writes it; 'ruff format' fixes it".to_owned()
        } else {
            format!("Ruff cannot format it: {}", finding.message)
        };
        report(session, paths, &python.ruff_folder, finding, &message);
    }
    if findings.is_empty() && clean {
        session
            .log
            .line("Every file is formatted the way Ruff writes it.");
        return true;
    }
    false
}

/// Every file passes Ruff. Every finding becomes a finding on its line,
/// naming the rule. A rule the repository switches off for one line (`#
/// noqa: <code>`) stays its choice, like `#[allow]` in Rust; a `# ruff:
/// noqa` that takes a whole file out is held against the declaration by
/// phase 0.
pub(super) fn lint(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    programs: &Programs,
    python: &Interpreter,
) -> bool {
    let Some((_, findings)) = ruff(session, paths, programs, python, &["check", "--no-fix"], 1)
    else {
        return false;
    };
    for finding in &findings {
        let rule = finding
            .code
            .as_deref()
            .map_or_else(String::new, |code| format!("{code}: "));
        let message = format!("Ruff {rule}{}", finding.message);
        report(session, paths, &python.ruff_folder, finding, &message);
    }
    if findings.is_empty() {
        session.log.line("No finding.");
        return true;
    }
    false
}
