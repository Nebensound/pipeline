//! The Dockerfile check: every `Dockerfile`, `Containerfile` and `.dockerfile`
//! file is formatted the way dockerfmt writes it and passes Hadolint. Building
//! the image and running it belong to the build, not here.
//!
//! The two parts each run even when the other failed, so that one run reports
//! every finding at once.
//!
//! dockerfmt has no configuration file and is handed no style flag: it reads
//! the repository's `.editorconfig` (`indent_style`, `indent_size`,
//! `insert_final_newline` and its own `space_redirects`), so the repository
//! decides how it formats, as Prettier's configuration does in the JSON check.
//! Every file is checked on its own, and what dockerfmt says about it becomes
//! a finding on it; its full output stands in the log. dockerfmt checks its own
//! work: an instruction whose formatted form `BuildKit` would read differently,
//! or that it cannot verify, it leaves as written and only warns, and a file
//! with nothing else to change then passes its check. That instruction was not
//! checked, so the warning fails the check here, on its line.
//! `# dockerfmt-ignore` on the line before an instruction leaves it as written
//! on purpose, visible in the file.
//!
//! The pipeline brings no Hadolint configuration of its own: Hadolint's own
//! rules hold, and they contradict no other tool of the pipeline. The
//! repository's `.hadolint.yaml` or `.hadolint.yml` at its root is handed over
//! with `--config`, as yamllint's is read at the root only; without one, an
//! empty configuration is. So Hadolint reads none of a user's own files, which
//! it would look for in the home folder, and no `HADOLINT_*` variable: they are
//! taken out of its environment.
//!
//! The repository's per-rule settings stay its rule configuration: `ignored`,
//! `override`, the trusted registries, the label schema, a failure threshold,
//! and the inline and global ignore pragmas, the language's own means like
//! `#[allow]`. What would pass every finding of a file unchecked is set aside:
//! `no-fail`, and a `failure-threshold` of `none` or `ignore`, which leaves
//! Hadolint's default, `info`. A finding below the threshold, such as a style
//! finding by default, is a warning and fails nothing, as a yamllint warning
//! does in the YAML check.
//!
//! Hadolint cannot be told on its command line to set `no-fail` aside, so its
//! exit code decides nothing: the check reads the configuration as Hadolint
//! reads it, from what `--verbose` prints, and judges every finding of the JSON
//! report against the threshold itself. A configuration Hadolint cannot read,
//! which it would pass over with a message and run without, fails the check.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use super::{Session, Tool, ended, without_variables};

/// The configurations of Hadolint the check hands over, in the order it looks
/// for them at the repository root.
pub const CONFIGURATIONS: &[&str] = &[".hadolint.yaml", ".hadolint.yml"];

/// The empty configuration of Hadolint, for a repository without one.
/// Hadolint refuses a file without any YAML document, so it is an empty
/// mapping.
const EMPTY: &str = "{}\n";

/// The settings of Hadolint the check reads from what `--verbose` prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Hadolint could not read the configuration and would run without it.
    pub unreadable: bool,
    /// The failure threshold it names, if it names one that is not empty.
    pub threshold: Option<String>,
    /// Whether `no-fail` is set.
    pub no_fail: bool,
    /// How many output formats the configuration names.
    pub formats: usize,
}

/// Reads the settings from what `hadolint --verbose` printed on its standard
/// error without a Dockerfile: the configuration it would run with.
#[must_use]
pub fn settings(dump: &str) -> Settings {
    let lines = || dump.split_terminator('\n');
    // The output formats are the entries below the line that opens them, up
    // to the first line that is no entry; only the first such list counts.
    let mut formats = 0;
    let mut inside = false;
    for line in lines() {
        if line == "  output format:" {
            inside = true;
        } else if inside && line.starts_with("    - ") {
            formats += 1;
        } else if inside {
            break;
        }
    }
    Settings {
        unreadable: lines().any(|line| line.starts_with("\"Error parsing your config file")),
        threshold: lines()
            .find_map(|line| {
                line.strip_prefix("  failure threshold: ")
                    .filter(|value| value.bytes().all(|byte| byte.is_ascii_lowercase()))
            })
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        no_fail: lines().any(|line| line == "  no fail: True"),
        formats,
    }
}

/// How severe a level of Hadolint is: 1 for `error` down to 4 for `style`,
/// and 5 for anything else.
#[must_use]
pub fn rank(level: &str) -> u8 {
    match level {
        "error" => 1,
        "warning" => 2,
        "info" => 3,
        "style" => 4,
        _ => 5,
    }
}

/// A finding of dockerfmt on `file`, read from one line it printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// The file is not formatted the way dockerfmt writes it.
    Unformatted,
    /// dockerfmt left an instruction as written: the line it names, when that
    /// is a number, and why.
    LeftUnformatted {
        /// The number of the line, as dockerfmt wrote it.
        line: Option<String>,
        /// Why, as dockerfmt said it.
        reason: String,
    },
    /// dockerfmt cannot read the file, at a line where it names one.
    Unreadable {
        /// The number of the line, as dockerfmt wrote it.
        line: Option<String>,
        /// What dockerfmt said.
        detail: String,
    },
    /// A setting of `.editorconfig` dockerfmt cannot use: it formats with its
    /// default for that setting instead.
    Warning(String),
}

/// What a line dockerfmt printed on its standard output says about `file`.
#[must_use]
pub fn said_out(line: &str, file: &str) -> Option<Said> {
    line.strip_prefix(file)
        .is_some_and(|rest| rest == " is not formatted")
        .then_some(Said::Unformatted)
}

/// What a line dockerfmt printed on its standard error says about `file`.
#[must_use]
pub fn said_err(line: &str, file: &str) -> Option<Said> {
    if let Some((rest, reason)) = line
        .strip_prefix(file)
        .and_then(|rest| rest.strip_prefix(':'))
        .and_then(|rest| {
            rest.split_once(": left unformatted: ")
                .map(|(_, reason)| (rest, reason))
        })
    {
        let number = rest.split(':').next().unwrap_or_default();
        let digits = !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit());
        return Some(Said::LeftUnformatted {
            line: digits.then(|| number.to_owned()),
            reason: reason.to_owned(),
        });
    }
    if let Some(detail) = line
        .strip_prefix("dockerfmt: ")
        .and_then(|rest| rest.strip_prefix(file))
        .and_then(|rest| rest.strip_prefix(": "))
    {
        let position = detail
            .strip_prefix("line ")
            .and_then(|rest| rest.split_once(": "))
            .filter(|(number, _)| {
                !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
            });
        return Some(match position {
            Some((number, detail)) => Said::Unreadable {
                line: Some(number.to_owned()),
                detail: detail.to_owned(),
            },
            None => Said::Unreadable {
                line: None,
                detail: detail.to_owned(),
            },
        });
    }
    line.strip_prefix("dockerfmt: warning: ")
        .map(|rest| Said::Warning(rest.to_owned()))
}

/// The keyword of the instruction that starts line `number` of `text`, upper
/// case, the way the instruction is named; nothing when there is no such line
/// or it is blank.
#[must_use]
pub fn instruction(text: &str, number: &str) -> Option<String> {
    let number: usize = number.parse().ok().filter(|number| *number > 0)?;
    text.split('\n')
        .nth(number - 1)?
        .split([' ', '\t'])
        .find(|word| !word.is_empty())
        .map(str::to_ascii_uppercase)
}

/// Runs the check; whether it found nothing.
pub(super) fn run(session: &mut Session<'_, '_, '_>) -> bool {
    // A missing program is named and fails the check, and the part whose
    // program was found still runs, so that one run reports every finding.
    let dockerfmt = session.tool("dockerfmt", false);
    let hadolint = session.tool("hadolint", false);
    let Some(temp) = session.folder(|_| vec![("hadolint.yaml".to_owned(), Some(EMPTY.into()))])
    else {
        return false;
    };
    let files = session.files.clone();
    let formatted = dockerfmt.is_some_and(|dockerfmt| formatted(session, &dockerfmt, &files));
    let linted = hadolint.is_some_and(|hadolint| linted(session, &hadolint, &temp, &files));
    formatted && linted
}

/// dockerfmt over every file, one call each: whether none is unformatted,
/// unreadable or holds an instruction dockerfmt left as written.
fn formatted(session: &mut Session<'_, '_, '_>, dockerfmt: &Tool, files: &[String]) -> bool {
    let mut passed = true;
    for file in files {
        let mut command = session.command(&dockerfmt.executable, &dockerfmt.search_path);
        command.args(["--check", "--", file]);
        let Some(output) = session.output(command, "dockerfmt") else {
            passed = false;
            continue;
        };
        let out = String::from_utf8_lossy(&output.stdout);
        let err = String::from_utf8_lossy(&output.stderr);
        session.log.lines(&out);
        session.log.lines(&err);
        let mut found = false;
        for line in out.split_terminator('\n') {
            if said_out(line, file).is_some() {
                session.log.error(
                    Some(file),
                    None,
                    "not formatted the way dockerfmt writes it; 'dockerfmt --write' fixes it",
                );
                found = true;
            }
        }
        for line in err.split_terminator('\n') {
            match said_err(line, file) {
                Some(Said::LeftUnformatted {
                    line: Some(number),
                    reason,
                }) => {
                    let text = std::fs::read(session.context.root.join(file))
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                        .unwrap_or_default();
                    let keyword = instruction(&text, &number)
                        .map(|keyword| format!("{keyword} "))
                        .unwrap_or_default();
                    session.log.error(
                        Some(file),
                        Some((&number, None)),
                        &format!(
                            "dockerfmt left the {keyword}instruction on this line unformatted \
                             ({reason}), so it was not checked; write it so that dockerfmt can \
                             format it, or put '# dockerfmt-ignore' on the line before it to \
                             leave it as written on purpose"
                        ),
                    );
                    found = true;
                }
                Some(Said::LeftUnformatted { line: None, .. }) => {
                    session.log.error(Some(file), None, line);
                    found = true;
                }
                Some(Said::Unreadable { line, detail }) => {
                    session.log.error(
                        Some(file),
                        line.as_deref().map(|line| (line, None)),
                        &format!("dockerfmt cannot read it: {detail}"),
                    );
                    found = true;
                }
                Some(Said::Warning(warning)) => {
                    session
                        .log
                        .warning(Some(file), None, &format!("dockerfmt is {warning}"));
                }
                Some(Said::Unformatted) | None => {}
            }
        }
        if !output.status.success() && !found {
            session.log.error(
                Some(file),
                None,
                &format!(
                    "dockerfmt failed on it {}; its output stands in the log",
                    ended(output.status)
                ),
            );
            found = true;
        }
        passed &= !found;
    }
    passed
}

/// A command that starts Hadolint at the repository root, without a
/// `HADOLINT_*` variable of this process.
fn hadolint_command(session: &Session<'_, '_, '_>, hadolint: &Tool) -> Command {
    let mut command = session.command(&hadolint.executable, &hadolint.search_path);
    without_variables(&mut command, "HADOLINT_");
    command
}

/// The configuration Hadolint is handed, the failure threshold it holds and how
/// many output formats it names, read from what Hadolint prints with
/// `--verbose`; or nothing, with the finding written, when Hadolint cannot read
/// the configuration or does not say how it runs.
fn configuration(
    session: &mut Session<'_, '_, '_>,
    hadolint: &Tool,
    temp: &Path,
) -> Option<(OsString, String, usize)> {
    let own = CONFIGURATIONS
        .iter()
        .copied()
        .find(|name| session.is_file(name));
    let config: OsString = if let Some(own) = own {
        session
            .log
            .line(&format!("Rules: Hadolint's own, with {own}"));
        own.into()
    } else {
        session.log.line("Rules: Hadolint's own");
        temp.join("hadolint.yaml").into()
    };
    let shown = config.to_string_lossy().into_owned();

    // Without a Dockerfile, Hadolint prints the configuration it would run
    // with and stops.
    let mut command = hadolint_command(session, hadolint);
    command
        .arg("--verbose")
        .arg("--config")
        .arg(&config)
        .stdout(Stdio::null());
    let output = session.output(command, "Hadolint")?;
    let dump = String::from_utf8_lossy(&output.stderr).into_owned();
    let settings = settings(&dump);
    if settings.unreadable {
        session.log.lines(&dump);
        session.log.error(
            Some(&shown),
            None,
            "Hadolint cannot read this configuration and would run without it; its message \
             stands in the log",
        );
        return None;
    }
    let Some(threshold) = settings.threshold else {
        session.log.lines(&dump);
        session.log.error(
            None,
            None,
            &format!(
                "Hadolint printed no failure threshold for {shown}; the configuration it runs \
                 with is not known"
            ),
        );
        return None;
    };
    let threshold = if threshold == "ignore" {
        session.log.line(
            "A failure threshold of none or ignore is set aside; Hadolint's default, info, holds.",
        );
        "info".to_owned()
    } else {
        threshold
    };
    if settings.no_fail {
        session
            .log
            .line("no-fail is set aside: a finding at or above the threshold fails the check.");
    }
    session.log.line(&format!("Failure threshold: {threshold}"));
    Some((config, threshold, settings.formats))
}

/// Hadolint over every file, one call each, with the repository's
/// configuration or the empty one in `temp`: whether no finding is at or above
/// the failure threshold.
fn linted(
    session: &mut Session<'_, '_, '_>,
    hadolint: &Tool,
    temp: &Path,
    files: &[String],
) -> bool {
    let Some((config, threshold, formats)) = configuration(session, hadolint, temp) else {
        return false;
    };
    // Every output format the configuration names is written as well, before
    // the JSON report the command line asks for, and each goes to the next
    // `--output`: those of the configuration go into files nobody reads.
    let report = temp.join("report");
    let mut outputs: Vec<OsString> = Vec::new();
    for index in 0..formats {
        outputs.push("--output".into());
        outputs.push(temp.join(format!("discarded-{index}")).into());
    }
    outputs.push("--output".into());
    outputs.push(report.clone().into());

    let mut passed = true;
    for file in files {
        let _ = std::fs::remove_file(&report);
        let mut command = hadolint_command(session, hadolint);
        command
            .arg("--config")
            .arg(&config)
            .args(["--no-color", "--format", "json"])
            .args(&outputs)
            .args(["--", file]);
        let Some(output) = session.output(command, "Hadolint") else {
            passed = false;
            continue;
        };
        session.log.lines(&String::from_utf8_lossy(&output.stdout));
        session.log.lines(&String::from_utf8_lossy(&output.stderr));
        let findings = std::fs::read(&report)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        let Some(Value::Array(findings)) = findings else {
            session.log.error(
                Some(file),
                None,
                "Hadolint gave no report for it; its output stands in the log",
            );
            passed = false;
            continue;
        };
        // Every finding is an annotation on its line: an error at or above
        // the threshold, a warning below it.
        for finding in &findings {
            let line = text(&finding["line"]);
            let column = text(&finding["column"]);
            let message = format!(
                "{} ({}): {}",
                text(&finding["code"]),
                text(&finding["level"]),
                text(&finding["message"])
            );
            let position = Some((line.as_str(), Some(column.as_str())));
            if rank(finding["level"].as_str().unwrap_or_default()) <= rank(&threshold) {
                session.log.error(Some(file), position, &message);
                passed = false;
            } else {
                session.log.warning(Some(file), position, &message);
            }
        }
    }
    passed
}

/// A value of the report as text: a string as it is, anything else as JSON
/// writes it.
fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
