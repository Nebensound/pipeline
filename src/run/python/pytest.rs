//! The tests: the project's own pytest, from its lockfile, run by coverage.py
//! laid over the project's environment, measuring lines and branches below
//! the unit.
//!
//! A file an `[[exclude]]` entry takes out is not collected. pytest writes no
//! cache into the checkout, and the report it writes is read for the tests
//! that failed, each a finding on its file.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::{PYTEST_THERE, Paths, Programs, command, has_extension, in_environment, prefixed, uv};
use crate::run::{Session, relative};

/// The line of pytest's header that names its rootdir.
static ROOTDIR: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^rootdir: (.*?)(?:, (?:configfile|testpaths|plugins): .*)?$").ok()
});

/// A line of a traceback that names a file and a line.
static TRACED: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^(.+):(\d+): ").ok());

/// The rootdir pytest names in its header, if it prints one.
#[must_use]
pub fn rootdir(output: &str) -> Option<String> {
    ROOTDIR.as_ref().and_then(|pattern| {
        output.lines().find_map(|line| {
            pattern
                .captures(line)
                .map(|captures| captures[1].to_owned())
        })
    })
}

/// A path written with `/`, with `.` and empty parts left out and each `..`
/// taking the part before it away, as Python's `os.path.normpath` writes it.
#[must_use]
pub fn normalized(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let joined = parts.join("/");
    match (path.starts_with('/'), joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".to_owned(),
        (false, false) => joined,
    }
}

/// The folders the paths of pytest's report start at: the rootdir it names,
/// or, where the repository's `addopts` hide the header (`-q`,
/// `--no-header`), the folder of the configuration pytest read, the unit's
/// folder or one above it, up to the repository's root. Both are absolute
/// paths without links, the unit's folder below the root.
#[must_use]
pub fn candidates(rootdir: Option<String>, unit: &Path, root: &Path) -> Vec<PathBuf> {
    if let Some(rootdir) = rootdir {
        return vec![PathBuf::from(rootdir)];
    }
    let mut folders = Vec::new();
    let mut folder = Some(unit.to_path_buf());
    while let Some(here) = folder {
        let last = here == root;
        folder = here.parent().map(Path::to_path_buf).filter(|_| !last);
        folders.push(here);
    }
    folders
}

/// One test that failed or errored, as a finding: its file from the
/// repository root and its line where the report names them, and what is
/// said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    /// The file and line, if the report names a file.
    pub place: Option<(String, u64)>,
    /// What is said: the test, how it ended, and the first line of why.
    pub message: String,
}

/// The tests pytest's XML report (`junit_family=xunit1`) names as failed or
/// errored.
/// Each report path is taken from the first of `candidates` where it names a
/// file; its line is the one the traceback ends in, when that is in the file,
/// else the line of the test.
///
/// # Errors
///
/// The parser's message, when the report is no XML.
pub fn failed(report: &str, candidates: &[PathBuf], root: &Path) -> Result<Vec<Failed>, String> {
    let document = roxmltree::Document::parse(report).map_err(|error| error.to_string())?;
    let mut found = Vec::new();
    for case in document
        .descendants()
        .filter(|node| node.has_tag_name("testcase"))
    {
        for (outcome, what) in [("failure", "failed"), ("error", "errored")] {
            let Some(element) = case.children().find(|child| child.has_tag_name(outcome)) else {
                continue;
            };
            let test: Vec<&str> = [case.attribute("classname"), case.attribute("name")]
                .into_iter()
                .flatten()
                .filter(|part| !part.is_empty())
                .collect();
            let place = case
                .attribute("file")
                .filter(|file| !file.is_empty())
                .map(|file| {
                    let fallback = candidates
                        .first()
                        .map_or(PathBuf::from(file), |folder| folder.join(file));
                    let located = candidates
                        .iter()
                        .map(|folder| folder.join(file))
                        .find(|path| path.is_file())
                        .unwrap_or(fallback);
                    let real = fs::canonicalize(&located).unwrap_or(located);
                    let path = relative(root, &real).to_string_lossy().into_owned();
                    let mut line = case
                        .attribute("line")
                        .and_then(|line| line.parse::<u64>().ok())
                        .unwrap_or(0)
                        + 1;
                    let traced = TRACED.as_ref().and_then(|pattern| {
                        element
                            .text()
                            .unwrap_or_default()
                            .lines()
                            .rev()
                            .find_map(|text| {
                                let captures = pattern.captures(text)?;
                                (normalized(&captures[1]) == normalized(file))
                                    .then(|| captures[2].parse::<u64>().ok())
                                    .flatten()
                            })
                    });
                    if let Some(traced) = traced {
                        line = traced;
                    }
                    (path, line)
                });
            let reason = element
                .attribute("message")
                .unwrap_or_default()
                .trim()
                .lines()
                .next()
                .map_or_else(String::new, |first| format!(": {first}"));
            found.push(Failed {
                place,
                message: format!("pytest: {} {what}{reason}", test.join(".")),
            });
        }
    }
    Ok(found)
}

/// Why pytest failed, by its exit status, where it named no test.
#[must_use]
pub fn reason(status: Option<i32>) -> &'static str {
    match status {
        Some(2) => "the run was interrupted, as by a test module that cannot be collected",
        Some(3) => "pytest itself failed",
        Some(4) => "its command line or configuration is not usable",
        Some(5) => "it collected no test",
        _ => "a test failed",
    }
}

/// The tests pass.
pub(super) fn check(
    session: &mut Session<'_, '_, '_>,
    paths: &Paths,
    programs: &Programs,
    taken: &[String],
) -> bool {
    let environment = in_environment(&paths.environment(), "python", paths.windows);
    let mut asked = command(paths, &environment, &paths.dir, &programs.uv.search_path);
    asked.args(["-I", "-c", PYTEST_THERE]);
    let there = session
        .output(asked, "the project's Python")
        .is_some_and(|output| output.status.success());
    if !there {
        session.log.error(
            Some(&paths.manifest),
            None,
            "pytest is not in the project's environment: check-python runs the tests with the \
             pytest the project locks; add it to its dev dependency group (uv add --dev pytest)",
        );
        return false;
    }
    let junit = paths.temp("junit.xml");
    let mut run = uv(paths, programs);
    run.args(["run", "--no-sync", "--with"])
        .arg(&programs.coverage)
        .args(["coverage", "run", "--branch"])
        .arg(prefixed("--source=", &paths.dir))
        .arg(prefixed("--data-file=", &paths.temp("coverage-data")))
        .args([
            "-m",
            "pytest",
            "-p",
            "no:cacheprovider",
            "-o",
            "junit_family=xunit1",
        ])
        .arg(prefixed("--junit-xml=", &junit))
        .args(
            taken
                .iter()
                .filter(|file| has_extension(file, "py"))
                .map(|file| prefixed("--ignore=", &paths.root.join(file))),
        );
    let ran = session.capture(run, "uv");
    session.log.lines(&ran.text);
    let found = match fs::read_to_string(&junit) {
        Ok(report) => {
            let folders = candidates(rootdir(&ran.text), &paths.dir, &paths.root);
            match failed(&report, &folders, &paths.root) {
                Ok(found) => found,
                Err(error) => {
                    session.log.error(
                        None,
                        None,
                        &format!("pytest's report cannot be read: {error}"),
                    );
                    return false;
                }
            }
        }
        Err(_) => Vec::new(),
    };
    for test in &found {
        match &test.place {
            Some((file, line)) => {
                session
                    .log
                    .error(Some(file), Some((&line.to_string(), None)), &test.message);
            }
            None => session.log.error(None, None, &test.message),
        }
    }
    if !ran.success && found.is_empty() {
        let code = ran.code.map_or("none".to_owned(), |code| code.to_string());
        session.log.error(
            Some(&paths.manifest),
            None,
            &format!(
                "pytest failed with exit status {code}: {}; its output stands in the log",
                reason(ran.code)
            ),
        );
    }
    ran.success
}
