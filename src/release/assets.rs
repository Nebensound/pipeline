//! The publish target `release-assets`: the archives a unit's build handed on,
//! attached to the unit's release while it is a draft.
//!
//! The job `Publish` fetches the artifacts of the run's builds into one folder,
//! each artifact in a folder of its own named after it, and hands that folder
//! to `release`. They are data: nothing of them is started.
//!
//! A unit takes archives when its pipeline publishes to `release-assets` and
//! the plan starts its build ([`declared`]). Before anything is written, the
//! stage holds what it was handed against what the release needs ([`found`]):
//!
//! - every target of the build has its artifact, named as the build names it
//!   (`<kind>-<path>@<version>@<commit>@<target>`, the path percent-encoded);
//!   a build without targets has the one artifact without the last part;
//! - every artifact of the unit's build belongs to the version the stage
//!   releases and to the commit at hand, and is for a target of the build;
//! - every such artifact is a folder that holds exactly one plain file, the
//!   archive, whose name names its target and holds no control character;
//! - no two archives of one release carry the same name.
//!
//! Anything else is named, and nothing is written. Artifacts of other units, or
//! of other kinds, are not this target's and are passed over.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::build::cargo_package::{encoded, sha256};
use crate::finding::UnitName;
use crate::path::RelativePath;
use crate::{Analysis, Central, plan};

/// The publish target the release stage carries out itself.
pub const TARGET: &str = "release-assets";

/// A unit whose build's archives go to its release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    /// Which unit it is.
    pub name: UnitName,
    /// Where it lives: its artifacts are named after it.
    pub path: RelativePath,
    /// Its build, as the register names it.
    pub build: String,
    /// The kind of artifact its build produces, the first part of the name of
    /// every artifact it hands on.
    pub produces: String,
    /// The targets the register lists for its build, each with an artifact of
    /// its own; empty for a build without targets, which hands on one.
    pub targets: Vec<String>,
}

/// Every unit that publishes to [`TARGET`] and whose build the plan starts, in
/// the order of the plan.
#[must_use]
pub fn declared(analysis: &Analysis, central: &Central) -> Vec<Declared> {
    let plan = plan::plan(analysis, central);
    plan.units
        .iter()
        .filter_map(|unit| {
            let (build, entry) = unit
                .build
                .as_ref()
                .and_then(|build| central.register().build.get_key_value(build))?;
            plan.versions
                .iter()
                .any(|versioned| {
                    versioned.unit.name == unit.name
                        && versioned
                            .targets
                            .iter()
                            .any(|target| target.as_str() == TARGET)
                })
                .then(|| Declared {
                    name: unit.name.clone(),
                    path: unit.path.clone(),
                    build: build.to_string(),
                    produces: entry.produces.to_string(),
                    targets: entry.targets.keys().cloned().collect(),
                })
        })
        .collect()
}

/// The name of the artifact a build of the unit at `path` hands on for a
/// version, a commit and a target, or without one for a build without
/// targets: as the builds name them.
#[must_use]
pub fn artifact(
    produces: &str,
    path: &RelativePath,
    version: &str,
    commit: &str,
    target: Option<&str>,
) -> String {
    let mut name = format!(
        "{produces}-{}@{version}@{commit}",
        encoded(&path.to_string())
    );
    if let Some(target) = target {
        name.push('@');
        name.push_str(target);
    }
    name
}

/// An archive to attach to a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    /// The name it carries on the release: the name of its file.
    pub name: String,
    /// The file.
    pub file: PathBuf,
    /// Its SHA-256, in small hexadecimal digits.
    pub sha256: String,
}

/// The archives of the unit for its release of `version` from `commit`, out
/// of `folder`, where the artifacts of the run's builds lie; none when there is
/// no folder, as when nothing was handed on. In the order of the targets.
///
/// # Errors
///
/// Returns every reason the archives are not the ones the release needs, one
/// sentence each.
pub fn found(
    folder: Option<&Path>,
    unit: &Declared,
    version: &str,
    commit: &str,
) -> Result<Vec<Archive>, Vec<String>> {
    let entries = match folder {
        None => Vec::new(),
        Some(folder) => match fs::read_dir(folder) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(vec![format!(
                    "the artifacts of the run's builds cannot be read from {}: {error}",
                    folder.display()
                )]);
            }
            Ok(read) => listed(read),
        },
    };
    let expected: Vec<Option<&str>> = if unit.targets.is_empty() {
        vec![None]
    } else {
        unit.targets
            .iter()
            .map(|target| Some(target.as_str()))
            .collect()
    };
    let prefix = format!("{}-{}@", unit.produces, encoded(&unit.path.to_string()));
    let mut problems = Vec::new();
    let mut archives: Vec<(Option<&str>, Archive)> = Vec::new();
    for (name, path, kind) in &entries {
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        let parts: Vec<&str> = rest.split('@').collect();
        let (built, from, target) = match (parts.as_slice(), unit.targets.is_empty()) {
            ([built, from], true) => (*built, *from, None),
            ([built, from, target], false) => (*built, *from, Some(*target)),
            _ => {
                problems.push(format!(
                    "the artifact {name} does not read as one of the build {}, which names its \
                     artifacts {}",
                    unit.build,
                    artifact(
                        &unit.produces,
                        &unit.path,
                        "<version>",
                        "<commit>",
                        (!unit.targets.is_empty()).then_some("<target>")
                    )
                ));
                continue;
            }
        };
        if built != version {
            problems.push(format!(
                "the artifact {name} holds an archive of {built}, and this run releases {version}"
            ));
        } else if from != commit {
            problems.push(format!(
                "the artifact {name} was built from {from}, and this run releases {commit}"
            ));
        } else if !expected.contains(&target) {
            problems.push(format!(
                "the artifact {name} is for {}, which the build {} does not list",
                target.unwrap_or_default(),
                unit.build
            ));
        } else {
            match archive(name, path, *kind, target) {
                Ok(found) => archives.push((target, found)),
                Err(problem) => problems.push(problem),
            }
        }
    }
    for target in &expected {
        let wanted = artifact(&unit.produces, &unit.path, version, commit, *target);
        if !entries.iter().any(|(name, _, _)| *name == wanted) {
            problems.push(match target {
                Some(target) => format!(
                    "the archive for {target} is missing: the run's builds handed on no artifact \
                     {wanted}"
                ),
                None => format!(
                    "the archive is missing: the run's builds handed on no artifact {wanted}"
                ),
            });
        }
    }
    for (index, (_, archive)) in archives.iter().enumerate() {
        if archives[..index]
            .iter()
            .any(|(_, earlier)| earlier.name == archive.name)
        {
            problems.push(format!(
                "two artifacts hold an archive named {}, and a release takes one file of a name",
                archive.name
            ));
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    archives.sort_by_key(|(target, _)| expected.iter().position(|each| each == target));
    Ok(archives.into_iter().map(|(_, archive)| archive).collect())
}

/// The entries of a folder, each with its name, its path and what it is, a
/// link not followed, in the order of their names; an entry that cannot be
/// read is left out.
fn listed(read: fs::ReadDir) -> Vec<(String, PathBuf, Option<fs::FileType>)> {
    let mut entries: Vec<(String, PathBuf, Option<fs::FileType>)> = read
        .flatten()
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
                entry.file_type().ok(),
            )
        })
        .collect();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

/// The one archive in the artifact `name` at `path`, which is a `kind`, for
/// `target`.
fn archive(
    name: &str,
    path: &Path,
    kind: Option<fs::FileType>,
    target: Option<&str>,
) -> Result<Archive, String> {
    let not_one = |what: &str| {
        format!(
            "the artifact {name} {what}, and an artifact of a build is a folder that holds \
             exactly one file, the archive"
        )
    };
    let read = match fs::read_dir(path) {
        Ok(read) if kind.is_some_and(|kind| kind.is_dir()) => read,
        _ => return Err(not_one("is no folder")),
    };
    let inside = listed(read);
    let (asset, file) = match inside.as_slice() {
        [(asset, file, Some(kind))] if kind.is_file() => (asset, file),
        [(_, file, _)] => {
            return Err(not_one(&format!(
                "holds {}, which is no plain file",
                file.display()
            )));
        }
        _ => return Err(not_one(&format!("holds {} entries", inside.len()))),
    };
    if asset.chars().any(char::is_control) || file.file_name().and_then(|n| n.to_str()).is_none() {
        return Err(format!(
            "the artifact {name} holds {}, whose name is no text without control characters",
            file.display()
        ));
    }
    if let Some(target) = target
        && !asset.contains(&format!("-{target}."))
    {
        return Err(format!(
            "the artifact {name} holds {asset}, whose name does not name {target}"
        ));
    }
    let bytes = fs::read(file)
        .map_err(|error| format!("the archive {} cannot be read: {error}", file.display()))?;
    Ok(Archive {
        name: asset.clone(),
        file: file.clone(),
        sha256: sha256(&bytes),
    })
}
