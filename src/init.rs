//! `init`: the first draft of a repository's switch to the pipeline.
//!
//! It reads the repository, writes the three files a repository needs to call
//! the pipeline where they are missing, and says what it did. The declaration
//! `.nebensound/repo.toml` follows from what it recognises in the files Git
//! tracks and from the call; the caller `.github/workflows/nebensound.yml` is
//! the documented caller, pinned to the release of the pipeline this tool
//! belongs to; the Dependabot file `.github/dependabot.yml` is what
//! [`dependabot::render`] gives for the declaration in place, the draft
//! included.
//!
//! It asks nothing. Where it cannot know, it writes a [`MARKER`] into the file
//! and names it in its report, and `check` reports every marker left in those
//! three files as the error `unresolved-init-marker` ([`findings`]). It never
//! overwrites a file, so a second run changes nothing. It stages, commits and
//! pushes nothing, starts no program but Git, and uses no network.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::annotate::Coverage;
use crate::audience::Visibility;
use crate::central::Central;
use crate::conformance::{self, Identity, Told};
use crate::dependabot;
use crate::finding::{Finding, Position, Rule};
use crate::history::Tag;
use crate::ignored::Read;
use crate::model::{Declaration, Publish};
use crate::names::{CheckName, EcosystemName, PresetName, TargetName, VersionWriterName};
use crate::path::RelativePath;
use crate::register::{Preset, Register};
use crate::report::shown;
use crate::repository::{self, Repository};
use crate::tree::Tree;
use crate::version::{Change, TagVersion, Version};
use crate::{Analysis, analyse};

/// What marks a decision `init` left open, in every file it writes.
pub const MARKER: &str = "TODO(init)";

/// Where `init` writes the declaration.
pub const DECLARATION: &str = ".nebensound/repo.toml";

/// Where `init` writes the caller of the pipeline.
pub const CALLER: &str = ".github/workflows/nebensound.yml";

/// Where `init` writes the Dependabot configuration.
pub const DEPENDABOT: &str = ".github/dependabot.yml";

/// The other name Dependabot reads its configuration from. Where a file
/// stands under it, `init` writes none under [`DEPENDABOT`].
const DEPENDABOT_YAML: &str = ".github/dependabot.yaml";

/// The files `init` writes, and the only ones `check` looks for a leftover
/// [`MARKER`] in.
pub const WRITTEN: [&str; 3] = [DECLARATION, CALLER, DEPENDABOT];

/// The caller as the README documents it, with `{pin}` where the reference of
/// the workflow ends and `nebensound` as its name.
pub const TEMPLATE: &str = include_str!("init/nebensound.yml");

/// What stands in [`TEMPLATE`] where the pin goes.
const PIN: &str = "{pin}";

/// The pin of a tool that belongs to no release: a reference no branch, tag or
/// commit can have, since a reference holds no `..`, so that a forgotten
/// marker fails the run at once as well as `check`.
pub const UNRESOLVED_PIN: &str = "TODO..init # TODO(init): pin the tag of a release of the pipeline, \
                                  vX.Y.Z; this nebensound-repo belongs to no release";

/// The first line of the declaration, which points an editor at the schema.
const SCHEMA: &str = "#:schema https://raw.githubusercontent.com/Nebensound/pipeline/main/schemas/nebensound-declaration.json";

/// The three lines under it, which point a reader at the reference.
const HEADER: &str = "\
# The declaration of this repository: what it consists of and what the
# pipeline does with it. Every field is described in
# https://github.com/Nebensound/pipeline/blob/main/docs/declaration.md
";

/// Where the documents of the pipeline stand, for the report.
const DOCS: &str = "https://github.com/Nebensound/pipeline/blob/main/docs";

/// The prefix of the repository's records: `vX.Y.Z`.
const REPOSITORY_PREFIX: &str = "v";

/// The release a version of this tool belongs to: a version of three numbers
/// other than `0.0.0`. A build from source prints `0.0.0`, and the build of a
/// pull request or of a pre-release carries more than three numbers; none of
/// them is the tag of a release.
#[must_use]
pub fn release(version: &str) -> Option<Version> {
    Version::parse(version)
        .ok()
        .filter(|version| *version != Version::ZERO)
}

/// The caller, pinned to the tag of `release`, or with [`UNRESOLVED_PIN`]
/// when there is none.
#[must_use]
pub fn caller(release: Option<Version>) -> String {
    let pin = release.map_or_else(
        || UNRESOLVED_PIN.to_owned(),
        |version| format!("v{version}"),
    );
    TEMPLATE.replace(PIN, &pin)
}

/// The highest release of the unit whose records start with `prefix`, as
/// phase 0 reads a tag: `<prefix>X.Y.Z` or `<prefix>X.Y.Z-pre.N`, with the
/// name of the tag that records it.
#[must_use]
pub fn highest<'a>(prefix: &str, tags: &'a [Tag]) -> Option<(TagVersion, &'a str)> {
    tags.iter()
        .filter_map(|tag| {
            let version = TagVersion::parse(tag.name.strip_prefix(prefix)?)?;
            Some((version, tag.name.as_str()))
        })
        .max_by_key(|(version, _)| *version)
}

/// The lowest `minimum-version` that passes a release: its three numbers for
/// a pre-release, the next patch for a full one. [`None`] when no version
/// follows it.
#[must_use]
pub fn passing(release: TagVersion) -> Option<Version> {
    if release.is_pre() {
        Some(release.version)
    } else {
        release.version.raised(Change::Patch)
    }
}

/// Every [`MARKER`] in a text: where it stands, and what the rest of its line
/// says.
fn markers(text: &str) -> Vec<(Position, String)> {
    text.match_indices(MARKER)
        .map(|(offset, _)| {
            let rest = text
                .get(offset + MARKER.len()..)
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or_default();
            (
                Position::at(text, offset),
                rest.trim_start_matches(':').trim().to_owned(),
            )
        })
        .collect()
}

/// The error `unresolved-init-marker` for every [`MARKER`] left in the files
/// `init` writes, among the files Git tracks, with its line and column. A file
/// that cannot be read is passed over: the findings about that file say why.
#[must_use]
pub fn findings<'a>(
    files: impl IntoIterator<Item = &'a RelativePath>,
    read: &Read<'_>,
) -> Vec<Finding> {
    let mut found = Vec::new();
    for path in files {
        if !WRITTEN.contains(&path.to_string().as_str()) {
            continue;
        }
        let Ok(text) = read(path) else {
            continue;
        };
        for (position, rest) in markers(&text) {
            let open = if rest.is_empty() {
                String::new()
            } else {
                format!(": {}", shown(rest.trim_end_matches('.')))
            };
            found.push(
                Finding::error(
                    Rule::UnresolvedInitMarker,
                    format!(
                        "init left a decision open here, and nobody has taken it yet{open}. \
                         Take it, write it in place of the marker and remove the marker"
                    ),
                )
                .at(path.clone())
                .on(position),
            );
        }
    }
    found
}

/// What `init` did, as its report says it, and whether a file could not be
/// written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The report, ended by a line break.
    pub report: String,
    /// Whether a file `init` meant to write could not be written.
    pub failed: bool,
}

/// What the report gathers while `init` runs.
#[derive(Debug, Default)]
struct Report {
    written: Vec<&'static str>,
    left_alone: Vec<String>,
    failed: Vec<String>,
    recognised: Vec<String>,
    markers: Vec<String>,
    uncovered: Vec<String>,
    minimum: Vec<String>,
    workflows: Vec<String>,
}

/// Reads the repository, writes the three files where they are missing and
/// says what it did. `visibility` is how GitHub shows the repository, as the
/// call names it; `version` is this tool's own.
///
/// # Errors
///
/// Returns what reading the repository failed at: Git does not run, or the
/// folder is no repository. A file that cannot be written is no error but a
/// line of the report, and [`Outcome::failed`].
pub fn run(
    repository: &Repository,
    central: &Central,
    visibility: Option<Visibility>,
    version: &str,
) -> Result<Outcome, repository::Error> {
    let tree = repository.tree()?;
    let tags = repository.tags_held_by_head()?;
    let root = repository.root();
    let read = repository.reader();
    let mut report = Report::default();

    let drafted = !present(root, DECLARATION);
    let in_place = if drafted {
        let text = draft(&tree, &tags, &read, central, visibility, &mut report);
        if put(root, DECLARATION, &text, &mut report) {
            Ok(text)
        } else {
            Err("init could not write its draft".to_owned())
        }
    } else {
        report.left_alone.push(format!(
            "{DECLARATION}: it exists, and init never overwrites a file"
        ));
        read(&path(DECLARATION))
    };
    let analysis = in_place
        .and_then(|text| Declaration::parse(&text).map_err(|error| error.to_string()))
        .map(|declaration| analyse(&declaration, central, &tree));
    if drafted && let Ok(analysis) = &analysis {
        report.uncovered = uncovered(analysis);
    }

    if present(root, CALLER) {
        report.left_alone.push(format!(
            "{CALLER}: it exists, and init never overwrites a file"
        ));
    } else {
        let release = release(version);
        report.recognised.push(release.map_or_else(
            || {
                format!(
                    "pin left open: this tool prints the version {version}, and so belongs to no \
                     release of the pipeline"
                )
            },
            |release| format!("pin v{release}: the release of the pipeline this tool belongs to"),
        ));
        put(root, CALLER, &caller(release), &mut report);
    }

    dependabot_file(root, &analysis, central, &mut report);

    let identity = Identity {
        name: Told::Unknown(String::new()),
        default_branch: Told::Unknown(String::new()),
    };
    report.minimum = conformance::findings(&identity, tree.files())
        .into_iter()
        .filter_map(|finding| finding.path.map(|path| shown(&path.to_string())))
        .collect();
    report.workflows = tree
        .files()
        .filter(|file| workflow(file) && file.to_string() != CALLER)
        .map(|file| shown(&file.to_string()))
        .collect();

    let failed = !report.failed.is_empty();
    Ok(Outcome {
        report: render(&report, visibility),
        failed,
    })
}

/// Writes the Dependabot configuration the declaration in place gives, the
/// draft included, unless one stands under either name Dependabot reads.
fn dependabot_file(
    root: &Path,
    analysis: &Result<Analysis, String>,
    central: &Central,
    report: &mut Report,
) {
    if present(root, DEPENDABOT) {
        report.left_alone.push(format!(
            "{DEPENDABOT}: it exists, and init never overwrites a file"
        ));
        return;
    }
    if present(root, DEPENDABOT_YAML) {
        report.left_alone.push(format!(
            "{DEPENDABOT}: {DEPENDABOT_YAML} exists, which Dependabot reads in its place"
        ));
        return;
    }
    match analysis {
        Ok(analysis) => {
            let text = dependabot::render(&dependabot::entries(analysis, central));
            put(root, DEPENDABOT, &text, report);
        }
        Err(why) => report.left_alone.push(format!(
            "{DEPENDABOT}: no declaration that reads is in place ({}), and the file follows \
             from it; `nebensound-repo dependabot` prints it once one reads",
            shown(why)
        )),
    }
}

/// A path `init` writes, as a path in the repository.
fn path(written: &str) -> RelativePath {
    written
        .split('/')
        .fold(RelativePath::root(), |path, segment| path.join(segment))
}

/// Whether anything stands at this path of the repository: a file, a folder or
/// a link, even one that points nowhere.
fn present(root: &Path, written: &str) -> bool {
    fs::symlink_metadata(root.join(written)).is_ok()
}

/// Writes a file `init` writes, records in the report how that went, with
/// every marker it holds, and says whether the file was written.
fn put(root: &Path, written: &'static str, text: &str, report: &mut Report) -> bool {
    match write_new(root, written, text) {
        Ok(()) => {
            report.written.push(written);
            for (position, rest) in markers(text) {
                report.markers.push(format!(
                    "{written}:{}:{}: {rest}",
                    position.line, position.column
                ));
            }
            true
        }
        Err(why) => {
            report.failed.push(format!("{written}: {why}"));
            false
        }
    }
}

/// Writes a new file below the root, with the folders it needs. Every folder
/// on the way that exists has to be a folder and no link, and the file must
/// not exist yet: what stands there is never written over, and a link could
/// point the write out of the repository.
fn write_new(root: &Path, written: &str, text: &str) -> Result<(), String> {
    let (folders, file) = written.rsplit_once('/').unwrap_or(("", written));
    let mut place = root.to_path_buf();
    for folder in folders.split('/') {
        place.push(folder);
        match fs::symlink_metadata(&place) {
            Ok(entry) if entry.is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "{} is no folder but a file or a link, and init writes below folders only",
                    shown(&place.display().to_string())
                ));
            }
            Err(_) => fs::create_dir(&place).map_err(|error| error.to_string())?,
        }
    }
    let target = place.join(file);
    let mut opened = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|error| error.to_string())?;
    fill(&target, &mut opened, text)
}

/// Writes `text` into the file `init` just created at `target`, opened as
/// `opened`. When the write fails, it removes the file again: a file cut off
/// halfway would stay, and every later run would leave it alone as one that
/// exists.
///
/// # Errors
///
/// Returns why the write failed, and why removing the file failed too, if it
/// did.
pub fn fill(target: &Path, opened: &mut impl Write, text: &str) -> Result<(), String> {
    opened.write_all(text.as_bytes()).map_err(|error| {
        fs::remove_file(target).map_or_else(
            |removing| {
                format!(
                    "{error}; what was written of it stays at {}, since removing it failed: \
                     {removing}",
                    shown(&target.display().to_string())
                )
            },
            |()| format!("{error}; init removed what it had written of it"),
        )
    })
}

/// The draft of the declaration, from the tree, the tags `HEAD` contains and
/// the call. What it recognises goes into the report.
fn draft(
    tree: &Tree,
    tags: &[Tag],
    read: &Read<'_>,
    central: &Central,
    visibility: Option<Visibility>,
    report: &mut Report,
) -> String {
    let mut text = format!("{SCHEMA}\n\n{HEADER}\n[repo]\n");
    text.push_str(&audience(visibility, report));
    text.push_str(&floor(tags, report));
    text.push_str(&preset(tree, read, central, report));
    let candidates = candidates(tree);
    if !candidates.is_empty() {
        text.push_str(
            "\n# TODO(init): declare the Cargo packages below that are components.\n\
             # Each can be a component of its own, with a version and releases of its own:\n\
             # declare those that are under [component], each with its path, and remove\n\
             # this comment.\n",
        );
        for candidate in &candidates {
            let _ = writeln!(text, "# - {candidate}");
        }
        report.recognised.push(format!(
            "components left open: the Cargo packages {}",
            candidates.join(", ")
        ));
    }
    text
}

/// The lines of `[repo]` that say for whom the repository is meant: `public`
/// for a public repository, the only audience that fits it, and a marker
/// otherwise.
fn audience(visibility: Option<Visibility>, report: &mut Report) -> String {
    match visibility {
        Some(Visibility::Public) => {
            report.recognised.push(
                "audience public: the repository is public on GitHub (--visibility public), and \
                 a public repository is meant for the public"
                    .to_owned(),
            );
            "audience = \"public\"\n".to_owned()
        }
        Some(other) => {
            report.recognised.push(format!(
                "audience left open: the repository is {other} on GitHub, which fits internal \
                 and customer alike"
            ));
            "# TODO(init): write audience = \"internal\" or \"customer\".\n\
             # For whom the repository is meant: \"internal\" for the organization alone,\n\
             # \"customer\" when a customer can see it.\n"
                .to_owned()
        }
        None => {
            report.recognised.push(
                "audience left open: the call names no --visibility, and init does not guess it"
                    .to_owned(),
            );
            "# TODO(init): write audience = \"public\", \"internal\" or \"customer\".\n\
             # For whom the repository is meant: \"public\" when it is public on GitHub,\n\
             # \"internal\" for the organization alone, \"customer\" when a customer can see\n\
             # it.\n"
                .to_owned()
        }
    }
}

/// The line of `[repo]` that lifts the repository's versions above the
/// releases its old tags record, if any do.
fn floor(tags: &[Tag], report: &mut Report) -> String {
    let Some((release, tag)) = highest(REPOSITORY_PREFIX, tags) else {
        return String::new();
    };
    let tag = shown(tag);
    if let Some(floor) = passing(release) {
        report.recognised.push(format!(
            "minimum-version {floor}: above {tag}, the highest release the tags HEAD contains \
             record"
        ));
        format!(
            "# Above {tag}, the highest release the old tags record.\n\
             minimum-version = \"{floor}\"\n"
        )
    } else {
        report.recognised.push(format!(
            "minimum-version left open: no version passes the tag {tag}"
        ));
        format!(
            "# TODO(init): delete the tag {tag} if nobody published it.\n\
             # No version follows it, so no minimum-version can pass it.\n"
        )
    }
}

/// The lines of the declaration that name the repository's pipeline.
fn preset(tree: &Tree, read: &Read<'_>, central: &Central, report: &mut Report) -> String {
    let mut text = String::new();
    let fits = fits(tree, read, central.register(), report);
    match fits.split_first() {
        None => {
            let checks: Vec<&str> = central
                .base_checks()
                .iter()
                .map(CheckName::as_str)
                .collect();
            let _ = write!(
                text,
                "# No preset fits what init recognised, so only the base set runs: the checks\n\
                 # {}.\n\
                 pipeline = \"none\"\n",
                checks.join(", ")
            );
            report
                .recognised
                .push("preset none: no preset fits, so only the base set runs".to_owned());
        }
        Some((first, others)) => {
            for other in others {
                let _ = write!(
                    text,
                    "# TODO(init): keep the pipeline below, or write the preset {} instead.\n\
                     # It fits as well: {}.\n",
                    other.name, other.why
                );
                report.recognised.push(format!(
                    "preset {} left open: it fits as well, as {}",
                    other.name, other.why
                ));
            }
            report
                .recognised
                .push(format!("preset {}: {}", first.name, first.why));
            text.push_str(&pipeline(first, report));
        }
    }
    text
}

/// A preset that fits the repository, and why.
struct Fit<'a> {
    name: &'a PresetName,
    preset: &'a Preset,
    why: String,
}

/// The presets that fit the repository, the first the one written: Rust by
/// the manifest at the root, then a Python service by its lockfile and
/// Dockerfile. A preset the register does not hold fits nothing.
fn fits<'a>(
    tree: &Tree,
    read: &Read<'_>,
    register: &'a Register,
    report: &mut Report,
) -> Vec<Fit<'a>> {
    let root = RelativePath::root();
    let manifest = root.join("Cargo.toml");
    let dockerfile = root.join("deploy").join("Dockerfile");
    let mut named: Vec<(&str, String)> = Vec::new();
    if tree.holds(&manifest) {
        match read(&manifest).and_then(|text| {
            text.parse::<toml::Table>()
                .map_err(|error| error.to_string())
        }) {
            Ok(table) => named.push(rust(&table, tree, &dockerfile)),
            Err(why) => report.recognised.push(format!(
                "no Rust preset: Cargo.toml at the root does not read ({})",
                shown(why.trim())
            )),
        }
    }
    if tree.holds(&root.join("uv.lock")) && tree.holds(&dockerfile) {
        named.push((
            "python-service",
            "uv.lock and deploy/Dockerfile stand at the root".to_owned(),
        ));
    }
    named
        .into_iter()
        .filter_map(|(wanted, why)| {
            register
                .preset
                .iter()
                .find(|(name, _)| name.as_str() == wanted)
                .map(|(name, preset)| Fit { name, preset, why })
        })
        .collect()
}

/// The Rust preset the manifest at the root names, and why.
fn rust(manifest: &toml::Table, tree: &Tree, dockerfile: &RelativePath) -> (&'static str, String) {
    let source = RelativePath::root().join("src");
    let binary = tree.holds(&source.join("main.rs"))
        || tree.files().any(|file| source.join("bin").contains(file))
        || manifest.contains_key("bin");
    if !manifest.contains_key("package") {
        (
            "rust-workspace",
            "Cargo.toml at the root holds no package of its own".to_owned(),
        )
    } else if tree.holds(dockerfile) {
        (
            "rust-service",
            "Cargo.toml at the root holds a package, and deploy/Dockerfile stands beside it"
                .to_owned(),
        )
    } else if binary {
        (
            "rust-binary",
            "Cargo.toml at the root holds a package with a binary".to_owned(),
        )
    } else {
        (
            "rust-library",
            "Cargo.toml at the root holds a package without a binary".to_owned(),
        )
    }
}

/// The pipeline of the repository as the declaration writes it: a preset that
/// publishes written out with `publish = false`, since only a person decides
/// to publish, and one that publishes nothing by its name.
fn pipeline(fit: &Fit<'_>, report: &mut Report) -> String {
    let Fit { name, preset, why } = fit;
    let targets = match &preset.publish {
        Publish::To(targets) if !targets.is_empty() => targets,
        _ => return format!("# The preset {name}: {why}.\npipeline = \"{name}\"\n"),
    };
    let targets: Vec<&str> = targets.iter().map(TargetName::as_str).collect();
    report.recognised.push(format!(
        "publication: none. The preset {name} publishes to {}; init wrote it out with publish = \
         false, since only a person decides to publish",
        targets.join(", ")
    ));
    let list = |names: Vec<&str>| {
        let quoted: Vec<String> = names.iter().map(|name| format!("\"{name}\"")).collect();
        format!("[{}]", quoted.join(", "))
    };
    let mut text = format!(
        "\n# The preset {name}, written out with publish = false: nothing is published\n\
         # until a person decides it. To publish, name the targets under publish, or\n\
         # write pipeline = \"{name}\" in [repo] in place of this table.\n\
         [repo.pipeline]\n"
    );
    if !preset.version.is_empty() {
        let writers = preset
            .version
            .iter()
            .map(VersionWriterName::as_str)
            .collect();
        let _ = writeln!(text, "version = {}", list(writers));
    }
    let checks = preset.check.iter().map(CheckName::as_str).collect();
    let _ = writeln!(text, "check = {}", list(checks));
    // Every preset that publishes builds what it publishes; one that does not
    // would get no line here.
    text.push_str(
        &preset
            .build
            .as_ref()
            .map(|build| format!("build = \"{build}\"\n"))
            .unwrap_or_default(),
    );
    text.push_str("publish = false\n");
    let ecosystems = preset
        .dependabot
        .iter()
        .map(EcosystemName::as_str)
        .collect();
    let _ = writeln!(text, "dependabot = {}", list(ecosystems));
    text
}

/// The folders of the Cargo packages below the root: each can be a component
/// of its own.
fn candidates(tree: &Tree) -> Vec<String> {
    tree.files()
        .filter(|file| file.file_name() == Some("Cargo.toml") && file.segments().len() > 1)
        .map(|file| {
            let segments = file.segments();
            shown(&segments[..segments.len() - 1].join("/"))
        })
        .collect()
}

/// Whether a file is a workflow: a YAML file directly in `.github/workflows`.
fn workflow(file: &RelativePath) -> bool {
    matches!(
        file.segments(),
        [github, workflows, name]
            if github == ".github"
                && workflows == "workflows"
                && Path::new(name)
                    .extension()
                    .is_some_and(|extension| extension == "yml" || extension == "yaml")
    )
}

/// The files no check of the draft covers, by the check that would cover
/// them, or by none where the register knows no check for them: what phase 0
/// fails on until a check runs for them or an `[[exclude]]` gives a reason.
fn uncovered(analysis: &Analysis) -> Vec<String> {
    let mut by_check: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (node, coverage) in analysis.tree.files() {
        let check = match coverage {
            Coverage::Uncovered(check) => format!("the check {check}"),
            Coverage::Unknown => "no check of the register".to_owned(),
            Coverage::Covered(_) | Coverage::Excluded { .. } => continue,
        };
        by_check
            .entry(check)
            .or_default()
            .push(shown(&node.path.to_string()));
    }
    by_check
        .into_iter()
        .map(|(check, files)| {
            let shown: Vec<&str> = files.iter().take(3).map(String::as_str).collect();
            let more = if files.len() > 3 { ", …" } else { "" };
            format!(
                "{check}, {} file(s): {}{more}",
                files.len(),
                shown.join(", ")
            )
        })
        .collect()
}

/// The report, from what was gathered.
fn render(report: &Report, visibility: Option<Visibility>) -> String {
    let mut text =
        String::from("nebensound-repo init: the first draft of the switch to the pipeline\n");
    let sections: [(&str, Vec<String>); 8] = [
        (
            "Written",
            report
                .written
                .iter()
                .map(|path| (*path).to_owned())
                .collect(),
        ),
        ("Left alone", report.left_alone.clone()),
        ("Not written", report.failed.clone()),
        ("Recognised", report.recognised.clone()),
        (
            "Decisions left open, each marked TODO(init) in its file",
            report.markers.clone(),
        ),
        ("Files no check covers yet", report.uncovered.clone()),
        (
            "Minimum files the repository does not hold",
            report.minimum.clone(),
        ),
        ("Workflows init did not touch", report.workflows.clone()),
    ];
    for (title, lines) in &sections {
        if lines.is_empty() {
            continue;
        }
        let _ = write!(text, "\n{title}:\n");
        for line in lines {
            let _ = writeln!(text, "  {line}");
        }
    }
    let mut steps = Vec::new();
    if !report.markers.is_empty() {
        steps.push(
            "Take the decision each marker names, write it in its place and remove the marker. \
             `nebensound-repo check` reports every marker left as unresolved-init-marker."
                .to_owned(),
        );
    }
    if !report.written.is_empty() {
        steps.push(format!(
            "Add what init wrote, since phase 0 reads only the files Git tracks: git add {}",
            report.written.join(" ")
        ));
    }
    let given = visibility.map_or_else(String::new, |visibility| {
        format!(" --visibility {visibility}")
    });
    steps.push(format!(
        "Run nebensound-repo check{given} and fix what it finds."
    ));
    if !report.workflows.is_empty() {
        steps.push(
            "Decide for each workflow init did not touch whether the pipeline replaces it; init \
             deletes none."
                .to_owned(),
        );
    }
    steps.push(
        "Open a pull request with the change. nebensound / Declare fails on it while a marker \
         is left."
            .to_owned(),
    );
    text.push_str("\nNext steps:\n");
    for (number, step) in steps.iter().enumerate() {
        let _ = writeln!(text, "  {}. {step}", number + 1);
    }
    let _ = write!(
        text,
        "\nFor the organization owner, on the day the repository switches:\n  \
         - the merge settings of {DOCS}/repository-settings.md#every-repository-of-the-organization\n  \
         - the steps of {DOCS}/organization/README.md#the-day-a-repository-switches\n"
    );
    text
}
