//! Pyright in strict mode, with the repository's configuration laid over it.
//!
//! The configuration is the one Pyright finds for the unit:
//! `pyrightconfig.json` in the unit's folder or the nearest one above it,
//! else a `pyproject.toml` with a `[tool.pyright]` table found the same way.
//! Its rule severities, its Python version and platform, its execution
//! environments and its stub paths stay the repository's choice, like
//! `#[allow]` in Rust. What would look away is set aside: `typeCheckingMode`
//! is `strict` whatever it says, its `ignore`, which would hide every finding
//! of a file, is empty, the files are exactly the list (`include`), and its
//! `exclude` takes none of them out. Its `venvPath` and `venv` name the
//! project's environment, so that imports resolve as installed, and where it
//! names no `pythonVersion`, the project's interpreter gives it. A lower mode
//! written in the configuration or in a file (`# pyright: basic`) is a finding
//! of its own: Pyright would honour the comment, and the configuration would
//! lower the mode wherever Pyright runs by hand.
//!
//! The configuration that is run lies beside the repository's, so that its
//! paths mean what they mean there; it is removed when Pyright is done.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use super::{Interpreter, Paths, Programs, command, upwards};
use crate::run::{Session, ended, relative};

/// A comment that sets Pyright's mode or rules for a file.
static COMMENT: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^\s*#\s*pyright\s*:(.*)$").ok());

/// Pyright's own pattern for a `# type: ignore`, matched against a comment
/// after its first `#`, so that one behind another comment on the line counts
/// as well. The rules it names, if any, are its fourth group.
static BLANKET: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"((^|#)\s*)type:\s*ignore(\s*\[([\s\w,-]*)\]|\s|$)").ok());

/// The name the configuration that is run gets, beside the repository's.
pub const RUN: &str = ".nebensound-check-python-pyright";

/// The pattern `exclude` gets: it names no file.
pub const EXCLUDES_NOTHING: &str = ".nebensound-check-python-excludes-nothing";

/// Pyright's configuration file in a folder, if there is one.
fn json_config(folder: &Path) -> Option<PathBuf> {
    let path = folder.join("pyrightconfig.json");
    path.is_file().then_some(path)
}

/// A `pyproject.toml` in a folder that holds a `[tool.pyright]` table, if
/// there is one.
fn toml_config(folder: &Path) -> Option<PathBuf> {
    let path = folder.join("pyproject.toml");
    fs::read_to_string(&path)
        .ok()
        .and_then(|source| toml::from_str::<toml::Table>(&source).ok())
        .filter(|table| {
            table
                .get("tool")
                .and_then(|tool| tool.get("pyright"))
                .is_some()
        })
        .map(|_| path)
}

/// The configuration Pyright finds for the unit, as an absolute path.
fn configuration(paths: &Paths) -> Option<PathBuf> {
    let folders: Vec<PathBuf> = upwards(&paths.unit)
        .iter()
        .map(|folder| paths.root.join(folder))
        .collect();
    folders
        .iter()
        .find_map(|folder| json_config(folder))
        .or_else(|| folders.iter().find_map(|folder| toml_config(folder)))
}

/// Pyright's settings in its configuration file: JSON with comments and
/// trailing commas, as Pyright reads it, or the `[tool.pyright]` table of a
/// `pyproject.toml`.
///
/// # Errors
///
/// What keeps the settings from being read, or that they are no table.
pub fn settings(path: &Path, source: &str) -> Result<Map<String, Value>, String> {
    let value: Value = if path.extension().is_some_and(|ending| ending == "json") {
        jsonc_parser::parse_to_serde_value(source, &jsonc_parser::ParseOptions::default())
            .map_err(|error| format!("Pyright's configuration cannot be read: {error}"))?
    } else {
        let table: toml::Table = toml::from_str(source)
            .map_err(|error| format!("Pyright's configuration cannot be read: {error}"))?;
        let pyright = table
            .get("tool")
            .and_then(|tool| tool.get("pyright"))
            .cloned()
            .unwrap_or(toml::Value::Boolean(false));
        serde_json::to_value(pyright).unwrap_or(Value::Null)
    };
    match value {
        Value::Object(settings) => Ok(settings),
        _ => Err("Pyright's configuration is not a table of settings".to_owned()),
    }
}

/// What lowers the strict mode in one file, each with its line and what is
/// said about it: a `# pyright: basic` or `# pyright: standard` comment, and
/// a `# type: ignore` without rules before the file's first statement, which
/// Pyright takes as suppressing every finding of the file, unless the
/// configuration switches those comments off (`honoured` false).
#[must_use]
pub fn lowered(source: &str, honoured: bool) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut statement = false;
    for (index, text) in source.lines().enumerate() {
        let number = index + 1;
        let stripped = text.trim_start();
        if !statement && !stripped.is_empty() && !stripped.starts_with('#') {
            statement = true;
        }
        let rules = stripped
            .strip_prefix('#')
            .and_then(|rest| BLANKET.as_ref().and_then(|blanket| blanket.captures(rest)))
            .map(|captures| {
                captures
                    .get(4)
                    .map_or("", |rules| rules.as_str())
                    .trim()
                    .to_owned()
            });
        if honoured && !statement && rules.is_some_and(|rules| rules.is_empty()) {
            found.push((
                number,
                "'# type: ignore' before the first statement takes the whole file out of \
                 Pyright, which check-python holds every file to; remove it, and switch single \
                 rules off where they have to be ('# pyright: ignore[reportX]' on the line)"
                    .to_owned(),
            ));
            continue;
        }
        let Some(captures) = COMMENT.as_ref().and_then(|comment| comment.captures(text)) else {
            continue;
        };
        for part in captures[1].split(',').map(str::trim) {
            if matches!(part, "basic" | "standard") {
                found.push((
                    number,
                    format!(
                        "'# pyright: {part}' lowers Pyright's strict mode for this file, which \
                         check-python holds every file to; remove it, and switch single rules \
                         off where they have to be ('# pyright: reportX=false')"
                    ),
                ));
            }
        }
    }
    found
}

/// The settings Pyright is run with: the repository's, with what would look
/// away set aside, the files of the list from the configuration's folder
/// `folder`, and the project's environment.
#[must_use]
pub fn run_settings(
    mut settings: Map<String, Value>,
    folder: &Path,
    files: &[PathBuf],
    environment: &Path,
    version: (u64, u64),
) -> Map<String, Value> {
    let include: Vec<Value> = files
        .iter()
        .map(|file| Value::String(relative(folder, file).to_string_lossy().into_owned()))
        .collect();
    let text = |path: Option<&Path>| {
        Value::String(
            path.map(Path::to_string_lossy)
                .unwrap_or_default()
                .into_owned(),
        )
    };
    settings.insert("typeCheckingMode".to_owned(), Value::from("strict"));
    settings.insert("include".to_owned(), Value::Array(include));
    settings.insert("exclude".to_owned(), Value::from(vec![EXCLUDES_NOTHING]));
    settings.insert("ignore".to_owned(), Value::Array(Vec::new()));
    settings.insert("venvPath".to_owned(), text(environment.parent()));
    settings.insert(
        "venv".to_owned(),
        text(environment.file_name().map(Path::new)),
    );
    settings
        .entry("pythonVersion")
        .or_insert_with(|| Value::String(format!("{}.{}", version.0, version.1)));
    settings
}

/// One finding of Pyright, as it is written: a line for the log, and the
/// finding on its file with its level, when it is an error or a warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The file, from the repository root.
    pub file: String,
    /// The line, from 1.
    pub line: u64,
    /// The column, from 1.
    pub column: u64,
    /// `error`, `warning` or `information`.
    pub severity: String,
    /// What Pyright says, with the rule in front where it names one.
    pub message: String,
    /// The line for the log.
    pub text: String,
}

/// The findings of Pyright's JSON report, each with its file from the root
/// `root`; one without a file is on the unit `unit`. With the summary line.
#[must_use]
pub fn diagnostics(report: &Value, root: &Path, unit: &str) -> (Vec<Diagnostic>, String) {
    let field = |value: &Value, name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let number = |value: &Value, name: &str| value.get(name).and_then(Value::as_u64).unwrap_or(0);
    let mut found = Vec::new();
    for diagnostic in report
        .get("generalDiagnostics")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let path = field(diagnostic, "file");
        let file = if path.is_empty() {
            unit.to_owned()
        } else {
            let real = fs::canonicalize(&path).unwrap_or(PathBuf::from(&path));
            relative(root, &real).to_string_lossy().into_owned()
        };
        let start = diagnostic
            .get("range")
            .and_then(|range| range.get("start"))
            .cloned()
            .unwrap_or(Value::Null);
        let line = number(&start, "line") + 1;
        let column = number(&start, "character") + 1;
        let rule = field(diagnostic, "rule");
        let said = field(diagnostic, "message");
        let severity = field(diagnostic, "severity");
        let behind = if rule.is_empty() {
            String::new()
        } else {
            format!(" ({rule})")
        };
        let label = if rule.is_empty() {
            "Pyright: ".to_owned()
        } else {
            format!("Pyright {rule}: ")
        };
        found.push(Diagnostic {
            text: format!("{file}:{line}:{column}: {severity}: {said}{behind}"),
            message: format!("{label}{said}"),
            file,
            line,
            column,
            severity,
        });
    }
    let summary = report.get("summary").cloned().unwrap_or(Value::Null);
    let line = format!(
        "Pyright: {} file(s), {} error(s), {} warning(s)",
        number(&summary, "filesAnalyzed"),
        number(&summary, "errorCount"),
        number(&summary, "warningCount")
    );
    (found, line)
}

/// The repository's configuration for Pyright, and the folder the
/// configuration that is run goes into: the folder of the repository's, or
/// the unit's where it has none. A configuration that lowers the mode is a
/// finding, and one that cannot be read ends the step.
fn configured(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
) -> Option<(Map<String, Value>, PathBuf, bool)> {
    let Some(config) = configuration(paths) else {
        session.log.line("Pyright: strict mode");
        return Some((Map::new(), paths.dir.clone(), false));
    };
    let shown = relative(&paths.root, &config)
        .to_string_lossy()
        .into_owned();
    let read = fs::read_to_string(&config)
        .map_err(|error| format!("Pyright's configuration cannot be read: {error}"))
        .and_then(|source| settings(&config, &source));
    let settings = match read {
        Ok(settings) => settings,
        Err(error) => {
            session.log.error(Some(&shown), None, &error);
            return None;
        }
    };
    session
        .log
        .line(&format!("Pyright: strict mode, with {shown} laid over it"));
    let mode = settings
        .get("typeCheckingMode")
        .filter(|mode| mode.as_str() != Some("strict"));
    if let Some(mode) = mode {
        session.log.error(
            Some(&shown),
            None,
            &format!(
                "typeCheckingMode {mode} lowers Pyright's strict mode, which check-python holds \
                 every file to; remove it, and switch single rules off where they have to be"
            ),
        );
    }
    let lowers = mode.is_some();
    let folder = config.parent().unwrap_or(&paths.root).to_path_buf();
    Some((settings, folder, lowers))
}

/// Runs Pyright with these settings, written beside the repository's
/// configuration in `folder` for the run and removed after it: what it
/// printed, or nothing, with why written.
fn pyright(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    programs: &Programs,
    folder: &Path,
    settings: Map<String, Value>,
) -> Option<std::process::Output> {
    let run = folder.join(format!("{RUN}-{}.json", std::process::id()));
    let written = fs::write(&run, Value::Object(settings).to_string());
    let mut command = command(
        paths,
        &programs.pyright.executable,
        &paths.root,
        &programs.pyright.search_path,
    );
    command.args(["--outputjson", "--project"]).arg(&run);
    let output = written
        .map_err(|error| {
            session.log.error(
                None,
                None,
                &format!(
                    "Pyright's configuration cannot be written beside the repository's: {error}"
                ),
            );
        })
        .ok()
        .and_then(|()| session.output(command, "Pyright"));
    let _ = fs::remove_file(&run);
    output
}

/// How many files Pyright's JSON report says it analysed; none where it
/// printed no report.
#[must_use]
pub fn analysed(stdout: &[u8]) -> u64 {
    serde_json::from_slice::<Value>(stdout)
        .ok()
        .and_then(|report| report.get("summary")?.get("filesAnalyzed")?.as_u64())
        .unwrap_or(0)
}

/// The files of the list Pyright leaves out although it is handed them by
/// name, each run alone with the same settings: a file it analyses none of is
/// a finding of its own. Measured: Pyright passes over a file in a folder whose
/// name starts with a dot, even when its configuration names it.
fn left_out(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    programs: &Programs,
    folder: &Path,
    settings: &Map<String, Value>,
) {
    for file in &session.files.clone() {
        let mut alone = settings.clone();
        alone.insert(
            "include".to_owned(),
            Value::from(vec![
                relative(folder, &paths.root.join(file))
                    .to_string_lossy()
                    .into_owned(),
            ]),
        );
        let none = pyright(session, paths, programs, folder, alone)
            .is_some_and(|output| analysed(&output.stdout) == 0);
        if none {
            session.log.error(
                Some(file),
                None,
                "Pyright did not analyse it, although check-python handed it over by name, as \
                 Pyright does with a file in a folder whose name starts with a dot; move it out \
                 of such a folder, or take it out with an [[exclude]] entry and its reason",
            );
        }
    }
}

/// Every file passes Pyright in strict mode.
pub(super) fn check(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    programs: &Programs,
    python: &Interpreter,
) -> bool {
    let Some((settings, folder, mut failed)) = configured(session, paths) else {
        return false;
    };
    let honoured = settings.get("enableTypeIgnoreComments") != Some(&Value::Bool(false));
    for file in &session.files.clone() {
        let source = fs::read(paths.root.join(file)).unwrap_or_default();
        for (line, message) in lowered(&String::from_utf8_lossy(&source), honoured) {
            session
                .log
                .error(Some(file), Some((&line.to_string(), None)), &message);
            failed = true;
        }
    }
    let files: Vec<PathBuf> = session
        .files
        .iter()
        .map(|file| paths.root.join(file))
        .collect();
    let settings = run_settings(
        settings,
        &folder,
        &files,
        &paths.environment(),
        python.version,
    );
    let output = pyright(session, paths, programs, &folder, settings.clone());
    let Some(output) = output else {
        return false;
    };
    session.log.lines(&String::from_utf8_lossy(&output.stderr));
    let Ok(report) = serde_json::from_slice::<Value>(&output.stdout) else {
        session.log.lines(&String::from_utf8_lossy(&output.stdout));
        session.log.error(
            None,
            None,
            &format!(
                "Pyright could not check the files (it ended {}); its message stands in the log",
                ended(output.status)
            ),
        );
        return false;
    };
    let (found, summary) = diagnostics(&report, &paths.root, &paths.unit);
    for diagnostic in &found {
        session.log.line(&diagnostic.text);
        let line = diagnostic.line.to_string();
        let column = diagnostic.column.to_string();
        let position = Some((line.as_str(), Some(column.as_str())));
        match diagnostic.severity.as_str() {
            "error" => {
                session
                    .log
                    .error(Some(&diagnostic.file), position, &diagnostic.message);
                failed = true;
            }
            "warning" => session
                .log
                .warning(Some(&diagnostic.file), position, &diagnostic.message),
            _ => {}
        }
    }
    session.log.line(&summary);
    let handed = session.files.len() as u64;
    let counted = analysed(&output.stdout);
    if counted != handed {
        session.log.error(
            None,
            None,
            &format!(
                "Pyright analysed {counted} of the {handed} file(s) check-python handed it; each \
                 one it left out is named"
            ),
        );
        left_out(session, paths, programs, &folder, &settings);
        failed = true;
    }
    if !matches!(output.status.code(), Some(0 | 1)) {
        session.log.error(
            None,
            None,
            &format!(
                "Pyright ended {}; its message stands in the log",
                ended(output.status)
            ),
        );
        failed = true;
    }
    !failed
}
