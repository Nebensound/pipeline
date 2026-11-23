//! What a step of a workflow run hands the tool, and what the tool hands
//! back: the variables of the run, the files a step writes its outputs and its
//! summary to, and the log of the run.
//!
//! The calls that only a workflow run makes (`declare`, `report-versions`,
//! `build`, `unwritten`, `gather`, `xtask`) read the run from its variables.
//! The workflow names in each step what it hands in, through the step's `env`,
//! never inside a script, so that text from the event is never read as shell:
//!
//! - `NEBENSOUND_PRE_BRANCH`: `true` after a push to a pre-branch, as the
//!   workflow decides it for the whole run.
//! - `NEBENSOUND_PULL_REQUEST`, `NEBENSOUND_TITLE` and `NEBENSOUND_BODY`: the
//!   pull request of the run, empty when there is none.
//! - `NEBENSOUND_NAME`, `NEBENSOUND_DEFAULT_BRANCH` and
//!   `NEBENSOUND_VISIBILITY`: what the event says of the repository.
//!
//! The runner sets the rest for every step: `GITHUB_OUTPUT`,
//! `GITHUB_STEP_SUMMARY`, `GITHUB_REF_NAME`, and for a call of the API
//! `GITHUB_REPOSITORY` and `GITHUB_API_URL`; `GITHUB_TOKEN` is handed to the
//! one step that asks the API.

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{BufRead as _, BufReader, Write};
use std::process::Command;

use crate::run::{annotation, data, plain, property};
use crate::versions::PullRequest;

/// How a call reads the variables of the process: by name, [`None`] for one
/// that is not set. Handed in, so that a test sets the run it needs without
/// changing the variables of the process every test shares.
pub type Variables<'a> = &'a dyn Fn(&str) -> Option<String>;

/// A variable that is set and not empty.
#[must_use]
pub fn given(variables: Variables<'_>, name: &str) -> Option<String> {
    variables(name).filter(|value| !value.is_empty())
}

/// Whether the run follows a push to a pre-branch: `NEBENSOUND_PRE_BRANCH`
/// is `true`. The workflow decides it once for the whole run, and every call
/// that would read the versions of such a branch as those of `main` reads it.
#[must_use]
pub fn pre_branch(variables: Variables<'_>) -> bool {
    variables("NEBENSOUND_PRE_BRANCH").as_deref() == Some("true")
}

/// The pull request of the run, as `NEBENSOUND_PULL_REQUEST`,
/// `NEBENSOUND_TITLE` and `NEBENSOUND_BODY` name it, [`None`] when the run
/// belongs to none: its number, and the message its squash merge would carry,
/// the title and, after an empty line, the description when it has one.
///
/// # Errors
///
/// Returns a sentence when the number is no whole number from 1.
pub fn pull_request(variables: Variables<'_>) -> Result<Option<PullRequest>, String> {
    let Some(number) = given(variables, "NEBENSOUND_PULL_REQUEST") else {
        return Ok(None);
    };
    let number = number
        .parse::<u64>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or_else(|| {
            format!("NEBENSOUND_PULL_REQUEST is {number:?}, and the number of a pull request is a whole number from 1")
        })?;
    let mut message = variables("NEBENSOUND_TITLE").unwrap_or_default();
    if let Some(body) = given(variables, "NEBENSOUND_BODY") {
        message.push_str("\n\n");
        message.push_str(&body);
    }
    Ok(Some(PullRequest { number, message }))
}

/// Appends outputs of the step, `name=value` each, to the file
/// `GITHUB_OUTPUT` names. Outside a workflow run nobody reads them, and
/// nothing is written.
///
/// # Errors
///
/// Returns a sentence when a value holds a line break, which would end it
/// early, or the file cannot be written.
pub fn output(variables: Variables<'_>, pairs: &[(&str, &str)]) -> Result<(), String> {
    let Some(file) = given(variables, "GITHUB_OUTPUT") else {
        return Ok(());
    };
    let mut text = String::new();
    for (name, value) in pairs {
        if value.contains(['\n', '\r']) {
            return Err(format!(
                "the output {name} of the step holds a line break, so it cannot be handed on"
            ));
        }
        let _ = writeln!(text, "{name}={value}");
    }
    append(&file, &text)
        .map_err(|error| format!("the outputs of the step cannot be written to {file}: {error}"))
}

/// Appends text to the summary of the run, the file `GITHUB_STEP_SUMMARY`
/// names. Outside a workflow run there is none, and nothing is written.
///
/// # Errors
///
/// Returns a sentence when the file cannot be written.
pub fn summary(variables: Variables<'_>, text: &str) -> Result<(), String> {
    let Some(file) = given(variables, "GITHUB_STEP_SUMMARY") else {
        return Ok(());
    };
    append(&file, text)
        .map_err(|error| format!("the summary of the run cannot be written to {file}: {error}"))
}

fn append(file: &str, text: &str) -> std::io::Result<()> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .and_then(|mut file| file.write_all(text.as_bytes()))
}

/// A notice of the run with a title, escaped the way the runner reads it, as
/// [`annotation`] writes a finding.
#[must_use]
pub fn notice(title: &str, message: &str) -> String {
    format!("::notice title={}::{}", property(title), data(message))
}

/// Where a call writes, and how: in a workflow run every workflow command as
/// it is, elsewhere as a reader takes it in ([`plain`]).
pub struct Log<'o> {
    out: &'o mut dyn Write,
    workflow: bool,
}

impl<'o> Log<'o> {
    /// The log on `out`, of a workflow run or not.
    pub fn new(out: &'o mut dyn Write, workflow: bool) -> Self {
        Self { out, workflow }
    }

    /// Writes one line.
    pub fn line(&mut self, text: &str) {
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

    /// Writes every line of a text.
    pub fn lines(&mut self, text: &str) {
        for line in text.lines() {
            self.line(line);
        }
    }

    /// Writes a finding that names no file.
    pub fn error(&mut self, message: &str) {
        self.line(&annotation(None, None, message));
    }

    /// Writes a finding on a file.
    pub fn error_on(&mut self, file: &str, message: &str) {
        self.line(&annotation(Some(file), None, message));
    }

    /// Runs a step of a call: its name opens a group of the log, which closes
    /// after it, and a step that fails says so below the group.
    pub fn step<T>(&mut self, name: &str, body: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        self.line(&format!("::group::{name}"));
        let done = body(self);
        self.line("::endgroup::");
        if done.is_none() {
            self.line(&format!("{name}: failed"));
        }
        done
    }

    /// Runs a command to its end and writes what it prints on both outputs
    /// into the log, line by line as it comes, after a line that shows the
    /// call: whether it succeeded. A program that cannot be started is written
    /// as a finding and counts as failed.
    pub fn stream(&mut self, mut command: Command, what: &str) -> bool {
        let shown: Vec<String> = std::iter::once(what.to_owned())
            .chain(
                command
                    .get_args()
                    .map(|arg| arg.to_string_lossy().into_owned()),
            )
            .collect();
        self.line(&format!("> {}", shown.join(" ")));
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
            Ok((reader, mut child)) => {
                for line in BufReader::new(reader).split(b'\n').map_while(Result::ok) {
                    self.line(String::from_utf8_lossy(&line).trim_end_matches('\r'));
                }
                child.wait().is_ok_and(|status| status.success())
            }
            Err(error) => {
                self.error(&format!("{what} cannot be started: {error}"));
                false
            }
        }
    }

    /// Runs a command to its end and returns what it printed on standard
    /// output when it succeeded; what it printed on standard error goes into
    /// the log. A program that cannot be started, or that fails, is written
    /// into the log, and nothing is returned.
    pub fn capture(&mut self, mut command: Command, what: &str) -> Option<Vec<u8>> {
        match command.output() {
            Ok(output) => {
                self.lines(&String::from_utf8_lossy(&output.stderr));
                if output.status.success() {
                    Some(output.stdout)
                } else {
                    self.line(&format!("{what} failed ({}).", output.status));
                    None
                }
            }
            Err(error) => {
                self.error(&format!("{what} cannot be started: {error}"));
                None
            }
        }
    }
}
