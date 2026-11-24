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
//! A unit that publishes to `release-assets` takes the archives its build
//! handed on. Before anything is written they are held against what its
//! release needs ([`assets::found`]): every target of the build is there, and
//! every archive belongs to the version and the commit of the run. Anything
//! else stops the stage, named, and nothing is written, unless the unit's
//! release is published already: it takes no archive, so a re-run after the
//! run's artifacts expired leaves it as it is. A draft still takes them, so a
//! re-run that has to finish a draft needs the artifacts.
//!
//! Then it writes, in this order: the tag and the release as a draft of every
//! unit, then the archives attached to the drafts, while the releases still
//! take files, then every release published. An immutable release takes no
//! file once it is published, and GitHub honours the mark of the latest
//! release only in the call that publishes. A re-run completes what an earlier
//! attempt left undone and changes nothing else: a tag on the pushed commit is
//! kept, a draft the attempt left is taken up, an archive the draft holds with
//! the same digest is kept and one with another is replaced, and a published
//! release is left as it is. A published release that is not immutable gets a
//! warning.
//!
//! Everything it does goes into the log, and the summary of the run lists every
//! unit with what it got, or why it got nothing.

pub mod assets;
pub mod github;

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::console::Console;
use crate::history::Tag;
use crate::repository::Records;
use crate::run::{Level, annotation_at};
use crate::{TagVersion, UnitName, UnitVersion};

pub use assets::{Archive, Declared};
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
    /// The folder the artifacts of the run's builds were fetched into, each in
    /// a folder of its own named after it; [`None`] when none was handed in.
    pub assets: Option<PathBuf>,
}

impl Settings {
    /// Reads the settings from the variables of a workflow run:
    /// `GITHUB_REPOSITORY` (`owner/name`), `GITHUB_API_URL`, `GITHUB_TOKEN`,
    /// the token that may write to the repository, which the workflow hands
    /// to this one step, and `GITHUB_STEP_SUMMARY`, which may be missing.
    /// A request may use HTTPS only. The folder of the artifacts is not a
    /// variable: the call names it.
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
            assets: None,
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

/// Runs the stage: decides, writes, and appends the summary. `declared` names
/// the units that publish to `release-assets` ([`assets::declared`]).
///
/// Returns the exit code: 0 when the stage did what it had to, 1 when a tag
/// stands in the way, the archives are not the ones a release needs, a call
/// failed, or the summary could not be written.
pub fn run(
    versions: &[UnitVersion],
    declared: &[Declared],
    records: &Records,
    settings: &Settings,
    out: &mut Console<'_>,
) -> u8 {
    let mut report = Report {
        out,
        summary: String::from("### The release stage on main\n\n"),
    };
    let handed = Handed {
        declared,
        folder: settings.assets.as_deref(),
    };
    let code = stage(versions, &handed, records, &settings.github, &mut report);
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

/// What the stage was handed for the target `release-assets`.
struct Handed<'a> {
    /// The units that publish there.
    declared: &'a [Declared],
    /// Where the artifacts of the run's builds lie.
    folder: Option<&'a Path>,
}

/// The stage without the summary file.
fn stage(
    versions: &[UnitVersion],
    handed: &Handed<'_>,
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
    let archives = match archives(&releases, handed, head, github, report) {
        Ok(Some(archives)) => archives,
        Ok(None) => {
            report.say(
                "Nothing is released in this run: the archives the builds handed on are not the \
                 ones the releases take.",
            );
            return FAILED;
        }
        Err(message) => {
            report.error(&message);
            return FAILED;
        }
    };
    match write(&releases, &archives, records, github, report) {
        Ok(()) => OK,
        Err(message) => {
            report.error(&message);
            FAILED
        }
    }
}

/// Whether the release of the tag is published already.
fn published(existing: &[github::Listed], tag: &str) -> bool {
    existing
        .iter()
        .any(|release| release.tag == tag && !release.draft)
}

/// The archives each unit attaches to its draft.
type Attached<'p> = Vec<(&'p UnitName, Vec<Archive>)>;

/// The archives of every unit to release that publishes to
/// `release-assets`, held against what its release needs; [`None`] when they
/// are not the ones, each reason named.
///
/// A unit whose archives are refused is looked up among the releases, a
/// read, so still nothing is written: a release published already takes no
/// archive, so a re-run after the run's artifacts expired leaves it as it is.
///
/// # Errors
///
/// Returns a sentence when the releases cannot be read.
fn archives<'p>(
    releases: &[(&'p Planned, Option<&str>)],
    handed: &Handed<'_>,
    head: &str,
    github: &GitHub,
    report: &mut Report<'_, '_>,
) -> Result<Option<Attached<'p>>, String> {
    let mut archives = Vec::new();
    let mut refusals = Vec::new();
    for (unit, _) in releases {
        let Some(declared) = handed
            .declared
            .iter()
            .find(|declared| declared.name == unit.name)
        else {
            continue;
        };
        match assets::found(handed.folder, declared, &unit.version.to_string(), head) {
            Ok(found) => {
                let names: Vec<&str> = found.iter().map(|archive| archive.name.as_str()).collect();
                report.line(&format!(
                    "{}: the archives for {} are {}.",
                    unit.name,
                    unit.tag,
                    names.join(", ")
                ));
                archives.push((&unit.name, found));
            }
            Err(problems) => refusals.push((*unit, problems)),
        }
    }
    if refusals.is_empty() {
        return Ok(Some(archives));
    }
    let existing = github.releases()?;
    let mut refused = false;
    for (unit, problems) in refusals {
        let label = &unit.name;
        if published(&existing, &unit.tag) {
            report.line(&format!(
                "{label}: the release {} is published already and takes no archive.",
                unit.tag
            ));
            continue;
        }
        for problem in problems {
            report.error(&format!("{label}: {problem}."));
        }
        refused = true;
    }
    Ok((!refused).then_some(archives))
}

/// Writes the records of the units to release: every tag and draft, then the
/// archives attached to the drafts, then every release published.
fn write(
    releases: &[(&Planned, Option<&str>)],
    archives: &[(&UnitName, Vec<Archive>)],
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
        if published(&existing, tag) {
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
        let left = existing
            .iter()
            .find(|release| release.tag == *tag && release.draft);
        let draft = if let Some(draft) = left {
            report.line(&format!(
                "{label}: the draft of {tag} that an earlier attempt left is taken up."
            ));
            github::Draft {
                id: draft.id,
                upload: draft.upload.clone(),
            }
        } else {
            let notes = match previous {
                Some(previous) => github.generate_notes(tag, previous)?,
                None => format!(
                    "The first release of {}, so there is no earlier release to list the \
                     changes against.",
                    unit.display
                ),
            };
            let draft = github.create_draft(tag, head, &unit.title(), &notes)?;
            report.line(&format!("{label}: drafted the release {tag}."));
            draft
        };
        drafts.push((*unit, draft));
    }

    // The publish targets, while the releases are drafts that still take
    // files: the archives of `release-assets`. Every other target fails Check
    // before anything is built, so no unit that reaches the stage names one.
    for (unit, draft) in &drafts {
        if let Some((_, found)) = archives.iter().find(|(name, _)| **name == unit.name) {
            attach(github, unit, draft, found, report)?;
        }
    }

    if drafts.is_empty() {
        report.line("No release is left to publish.");
        return Ok(());
    }
    report
        .summary
        .push_str("\n### Published\n\n| Unit | Release | Kind |\n| ---- | ------- | ---- |\n");
    let mut warnings = Vec::new();
    for (unit, draft) in drafts {
        let label = &unit.name;
        let tag = &unit.tag;
        github.publish(draft.id, unit.latest())?;
        let shown = github.release(draft.id)?;
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

/// Attaches the archives to the draft of the unit: an asset of the same name
/// and digest that the draft holds is kept, one with another digest, or whose
/// upload broke off, is replaced, and every other archive is uploaded.
fn attach(
    github: &GitHub,
    unit: &Planned,
    draft: &github::Draft,
    archives: &[Archive],
    report: &mut Report<'_, '_>,
) -> Result<(), String> {
    let label = &unit.name;
    let tag = &unit.tag;
    let present = github.assets(draft.id)?;
    for archive in archives {
        let name = &archive.name;
        let digest = format!("sha256:{}", archive.sha256);
        let held = present.iter().find(|asset| asset.name == *name);
        if let Some(asset) = held {
            if asset.uploaded && asset.digest.as_deref() == Some(digest.as_str()) {
                report.line(&format!(
                    "{label}: the draft of {tag} holds {name} with the same digest already."
                ));
                continue;
            }
            github.delete_asset(asset.id)?;
        }
        let uploaded = github.upload(&draft.upload, name, &archive.file)?;
        // An answer without a digest proves nothing, so it fails as well, as
        // the same asset fails the check of a re-run above.
        let given = uploaded.digest.as_deref().unwrap_or("none");
        if !uploaded.uploaded || uploaded.name != *name || given != digest {
            return Err(format!(
                "GitHub holds the upload of {name} to the draft of {tag} as {} with the digest \
                 {given}, {}, and the archive is {name} with {digest}",
                uploaded.name,
                if uploaded.uploaded {
                    "uploaded"
                } else {
                    "not uploaded"
                }
            ));
        }
        let done = if held.is_some() {
            "replaced"
        } else {
            "attached"
        };
        report.line(&format!(
            "{label}: {done} {name} on the draft of {tag}, {digest}."
        ));
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
