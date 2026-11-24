//! `nebensound-repo release`: the release stage on `main`.
//!
//! After a push to `main`, every unit the history gives a version of the
//! pushed commit, that is every unit the commit touches, gets its records: a
//! tag on the commit and a GitHub Release, a full one. The release of the
//! repository is the latest, the release of a component never is. A unit the
//! commit does not touch has no version of the run and gets nothing: a tag on
//! this commit with its current version would not stand on the commit the
//! history gives that version.
//!
//! The stage first decides, from Git alone and before anything is written
//! ([`decide`]):
//!
//! - A tag of the version that stands anywhere but on the pushed commit stops
//!   the stage, named, before anything is written: a version belongs to
//!   exactly one commit.
//! - A newer record of the unit that already holds the pushed commit, as after
//!   a re-run of an older run, leaves the unit out, with a sentence, and the
//!   stage stays green.
//! - Otherwise the unit is released, with notes generated against its previous
//!   record, or one fixed sentence for its first release.
//!
//! Tags and releases are records, and they are read here only for what records
//! answer: the previous record and whether a newer one holds the commit. No
//! version is read from them; the versions are the history's.
//!
//! Then it writes, in this order: the tag and the release as a draft of every
//! unit, then the place of the publish targets, while the releases still take
//! files, then every release published. An immutable release takes no file
//! once it is published, and GitHub honours the mark of the latest release
//! only in the call that publishes. A re-run completes what an earlier attempt
//! left undone and changes nothing else: a tag on the pushed commit is kept, a
//! draft the attempt left is taken up, and a published release is left as it
//! is. A published release that is not immutable gets a warning.
//!
//! Everything it does goes into the log, and the summary of the run lists every
//! unit with what it got, or why it got nothing.

pub mod github;

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use crate::console::Console;
use crate::history::Tag;
use crate::repository::Records;
use crate::run::{Level, annotation_at};
use crate::{TagVersion, UnitName, UnitVersion};

pub use github::GitHub;

/// The stage did what it had to.
const OK: u8 = 0;
/// A tag stands in the way, or a call failed.
const FAILED: u8 = 1;

/// What the run hands the stage through its environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// How to reach the repository on GitHub.
    pub github: GitHub,
    /// The file the summary of the run is appended to, if there is one.
    pub summary: Option<PathBuf>,
}

impl Settings {
    /// Reads the settings from the variables of a workflow run:
    /// `GITHUB_REPOSITORY` (`owner/name`), `GITHUB_API_URL`, `GITHUB_TOKEN`,
    /// the token that may write to the repository, which the workflow hands
    /// to this one step, and `GITHUB_STEP_SUMMARY`, which may be missing.
    /// A request may use HTTPS only.
    ///
    /// # Errors
    ///
    /// Returns a sentence naming the variable that is missing, empty, holds a
    /// control character, or does not name a repository as `owner/name`. It
    /// never repeats the token.
    pub fn read(
        variable: &dyn Fn(&str) -> Option<String>,
        search_path: OsString,
    ) -> Result<Self, String> {
        let required = |name: &str| -> Result<String, String> {
            let value = variable(name)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("the release stage needs {name}, and it is not set"))?;
            if value.chars().any(char::is_control) {
                return Err(format!("{name} holds a control character"));
            }
            Ok(value)
        };
        let repository = required("GITHUB_REPOSITORY")?;
        let well_formed = repository.split_once('/').is_some_and(|(owner, name)| {
            !owner.is_empty() && !name.is_empty() && !name.contains('/')
        });
        if !well_formed {
            return Err(format!(
                "GITHUB_REPOSITORY is {repository:?}, and the release stage needs the repository \
                 as owner/name"
            ));
        }
        let api = required("GITHUB_API_URL")?.trim_end_matches('/').to_owned();
        let token = required("GITHUB_TOKEN")?;
        Ok(Self {
            github: GitHub {
                api,
                repository,
                token,
                search_path,
                protocols: "=https".to_owned(),
            },
            summary: variable("GITHUB_STEP_SUMMARY")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
        })
    }
}

/// One unit with a version of this run, and what the stage does with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// Which unit it is.
    pub name: UnitName,
    /// The name its release carries: the repository's name for the
    /// repository, the component's for a component.
    pub display: String,
    /// The tag of the version: the unit's prefix and the version.
    pub tag: String,
    /// The version of the pushed commit.
    pub version: TagVersion,
    /// What the stage does with it.
    pub what: What,
}

/// What the stage does with a unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum What {
    /// Release it, with notes against its previous record, or as its first
    /// release when it has none.
    Release {
        /// The unit's tag with the highest version below this one, on a
        /// commit the pushed commit contains.
        previous: Option<String>,
    },
    /// Leave it out: a newer record already holds the pushed commit.
    Held {
        /// The unit's tag with the highest version above this one, on a
        /// commit that contains the pushed commit.
        newer: String,
    },
    /// Stop the stage: the tag of the version stands elsewhere.
    Conflict {
        /// The commit it stands on; [`None`] when it stands on no commit.
        commit: Option<String>,
    },
}

impl Planned {
    /// Whether its release is the latest one: only the repository's is.
    #[must_use]
    pub fn latest(&self) -> bool {
        self.name == UnitName::Repository
    }

    /// The title of its release: its name and its version.
    #[must_use]
    pub fn title(&self) -> String {
        format!("{} {}", self.display, self.version)
    }
}

/// What the stage does with every unit that has a version of this run, in the
/// order given, from Git alone. `repository` is the repository's name, which
/// its releases carry.
///
/// A tag is a unit's record when what follows the unit's prefix reads as a
/// version as strictly as phase 0 reads it, a pre-release among them; any
/// other tag that starts with the prefix is passed over.
#[must_use]
pub fn decide(versions: &[UnitVersion], records: &Records, repository: &str) -> Vec<Planned> {
    versions
        .iter()
        .filter_map(|unit| {
            let version = unit.version?;
            let tag = format!("{}{version}", unit.prefix);
            let display = match &unit.name {
                UnitName::Repository => repository.to_owned(),
                UnitName::Component(name) => name.to_string(),
            };
            let own = records.tags.iter().find(|record| record.name == tag);
            let what = if let Some(record) =
                own.filter(|record| record.commit.as_deref() != Some(records.head.as_str()))
            {
                What::Conflict {
                    commit: record.commit.clone(),
                }
            } else if let Some(newer) = nearest(records, &unit.prefix, |found| {
                found > version
                    && records
                        .holding_head
                        .contains(&found_name(&unit.prefix, found))
            }) {
                What::Held { newer }
            } else {
                What::Release {
                    previous: nearest(records, &unit.prefix, |found| {
                        found < version
                            && records
                                .held_by_head
                                .contains(&found_name(&unit.prefix, found))
                    }),
                }
            };
            Some(Planned {
                name: unit.name.clone(),
                display,
                tag,
                version,
                what,
            })
        })
        .collect()
}

/// The name of a unit's record of a version.
fn found_name(prefix: &str, version: TagVersion) -> String {
    format!("{prefix}{version}")
}

/// Of the unit's records whose version `wanted` accepts, the one with the
/// highest version.
fn nearest(records: &Records, prefix: &str, wanted: impl Fn(TagVersion) -> bool) -> Option<String> {
    records
        .tags
        .iter()
        .filter_map(|Tag { name, .. }| {
            let version = TagVersion::parse(name.strip_prefix(prefix)?)?;
            wanted(version).then_some(version)
        })
        .max()
        .map(|version| found_name(prefix, version))
}

/// Runs the stage: decides, writes, and appends the summary.
///
/// Returns the exit code: 0 when the stage did what it had to, 1 when a tag
/// stands in the way, a call failed, or the summary could not be written.
pub fn run(
    versions: &[UnitVersion],
    records: &Records,
    settings: &Settings,
    out: &mut Console<'_>,
) -> u8 {
    let mut report = Report {
        out,
        summary: String::from("### The release stage on main\n\n"),
    };
    let code = stage(versions, records, &settings.github, &mut report);
    let Some(path) = &settings.summary else {
        return code;
    };
    let appended = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(report.summary.as_bytes()));
    match appended {
        Ok(()) => code,
        Err(error) => {
            let message = format!(
                "the summary of the run cannot be written to {}: {error}",
                path.display()
            );
            report.error(&message);
            FAILED
        }
    }
}

/// The stage without the summary file.
fn stage(
    versions: &[UnitVersion],
    records: &Records,
    github: &GitHub,
    report: &mut Report<'_, '_>,
) -> u8 {
    let name = github
        .repository
        .rsplit_once('/')
        .map_or(github.repository.as_str(), |(_, name)| name);
    let planned = decide(versions, records, name);
    let head = &records.head;
    let mut conflicts = false;
    let mut releases = Vec::new();
    for unit in &planned {
        let label = &unit.name;
        let tag = &unit.tag;
        match &unit.what {
            What::Conflict { commit } => {
                let place = commit.as_ref().map_or_else(
                    || "no commit".to_owned(),
                    |commit| format!("the commit {commit}"),
                );
                report.error(&format!(
                    "The tag {tag} stands on {place}, and the history gives {} of {label} to \
                     {head}, the commit pushed. A version belongs to exactly one commit, so \
                     nothing is released in this run. Delete the tag if nobody published it, or \
                     raise minimum-version past it, as for the finding conflicting-tag.",
                    unit.version
                ));
                conflicts = true;
            }
            What::Held { newer } => report.say(&format!(
                "{label}: the newer release {newer} already holds {head}, so {tag} is not \
                 released."
            )),
            What::Release { previous } => {
                let what = if unit.latest() {
                    "a full release, the latest"
                } else {
                    "a full release, not the latest"
                };
                let notes = previous.as_ref().map_or_else(
                    || "its first release".to_owned(),
                    |previous| format!("with notes since {previous}"),
                );
                report.say(&format!("{label}: {tag} on {head}, {what}, {notes}."));
                releases.push((unit, previous.as_deref()));
            }
        }
    }
    if conflicts {
        return FAILED;
    }
    if releases.is_empty() {
        report.say("Nothing is released in this run.");
        return OK;
    }
    match write(&releases, records, github, report) {
        Ok(()) => OK,
        Err(message) => {
            report.error(&message);
            FAILED
        }
    }
}

/// Writes the records of the units to release: every tag and draft, then
/// every release published.
fn write(
    releases: &[(&Planned, Option<&str>)],
    records: &Records,
    github: &GitHub,
    report: &mut Report<'_, '_>,
) -> Result<(), String> {
    let head = &records.head;
    let existing = github.releases()?;
    let mut drafts = Vec::new();
    for (unit, previous) in releases {
        let label = &unit.name;
        let tag = &unit.tag;
        let release = |draft: bool| {
            existing
                .iter()
                .find(|release| release.tag == *tag && release.draft == draft)
        };
        if release(false).is_some() {
            report.line(&format!(
                "{label}: the release {tag} is published already; nothing is changed."
            ));
            continue;
        }
        // A tag of the version that stands elsewhere stopped the stage
        // already, so one that is there stands on the pushed commit.
        if records.tags.iter().any(|record| record.name == *tag) {
            report.line(&format!("{label}: the tag {tag} stands on {head} already."));
        } else {
            github.create_tag(tag, head)?;
            report.line(&format!("{label}: tagged {head} as {tag}."));
        }
        let id = if let Some(draft) = release(true) {
            report.line(&format!(
                "{label}: the draft of {tag} that an earlier attempt left is taken up."
            ));
            draft.id
        } else {
            let notes = match previous {
                Some(previous) => github.generate_notes(tag, previous)?,
                None => format!(
                    "The first release of {}, so there is no earlier release to list the \
                     changes against.",
                    unit.display
                ),
            };
            let id = github.create_draft(tag, head, &unit.title(), &notes)?;
            report.line(&format!("{label}: drafted the release {tag}."));
            id
        };
        drafts.push((*unit, id));
    }

    // The publish targets come here, while the releases are drafts that still
    // take files. Every target without a building block fails Check before
    // anything is built, so no unit that reaches the stage names one yet; the
    // first block sets how it is called.

    if drafts.is_empty() {
        report.line("No release is left to publish.");
        return Ok(());
    }
    report
        .summary
        .push_str("\n### Published\n\n| Unit | Release | Kind |\n| ---- | ------- | ---- |\n");
    let mut warnings = Vec::new();
    for (unit, id) in drafts {
        let label = &unit.name;
        let tag = &unit.tag;
        github.publish(id, unit.latest())?;
        let shown = github.release(id)?;
        let kind = if unit.latest() {
            "full release, latest"
        } else {
            "full release"
        };
        let url = &shown.url;
        report.line(&format!("{label}: published {tag}, {kind}: {url}"));
        let _ = writeln!(report.summary, "| {label} | [{tag}]({url}) | {kind} |");
        if !shown.immutable {
            let message = format!(
                "The release {tag} is published and is not immutable: immutable releases are \
                 switched off in {}, so its tag can still be moved or deleted and its assets \
                 changed. Switch them on in the settings of the repository.",
                github.repository
            );
            report.warning(&message);
            warnings.push(message);
        }
    }
    if !warnings.is_empty() {
        report.summary.push('\n');
        for warning in warnings {
            let _ = writeln!(report.summary, "**Warning:** {warning}");
        }
    }
    Ok(())
}

/// Where the stage writes: the log, and the summary of the run.
struct Report<'o, 'c> {
    out: &'o mut Console<'c>,
    summary: String,
}

impl Report<'_, '_> {
    /// A line of the log, as text.
    fn line(&mut self, text: &str) {
        self.out.line(text);
    }

    /// A line of the log that the summary lists as well.
    fn say(&mut self, text: &str) {
        self.line(text);
        let _ = writeln!(self.summary, "- {text}");
    }

    /// An error, as an annotation of the run.
    fn error(&mut self, text: &str) {
        self.out
            .annotate(&annotation_at(Level::Error, None, None, text));
    }

    /// A warning, as an annotation of the run.
    fn warning(&mut self, text: &str) {
        self.out
            .annotate(&annotation_at(Level::Warning, None, None, text));
    }
}
