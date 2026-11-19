//! The version writer `cargo`: the version of the plan in the manifests.
//!
//! Cargo cannot take a version from outside: the version of a package stands in
//! its `Cargo.toml` and nowhere else. No manifest in the tree carries one,
//! because tags are the truth. So the version a unit carries at the commit at
//! hand is written into the checkout of the run, immediately before building,
//! and never committed.
//!
//! For every unit of the plan with a version and the writer `cargo`:
//!
//! - `package.version` in the unit's `Cargo.toml` becomes that version;
//! - every path dependency on that package inside its workspace gets that
//!   version as its requirement, since a package whose dependency names no
//!   version cannot be published;
//! - the package's entry in the workspace's `Cargo.lock` gets that version,
//!   and so does every entry that names it together with a version, so that
//!   `--locked` holds. No other entry changes. The same goes for the
//!   `Cargo.lock` of every other workspace that reaches the package through
//!   path dependencies, directly or through other packages, such as a `fuzz/`
//!   folder with a `[workspace]` of its own.
//!
//! The workspace is found the way Cargo finds it: the folder the manifest
//! names in `package.workspace`, else the nearest folder at or above the
//! package whose `Cargo.toml` has a `[workspace]` that does not exclude it,
//! else the package's own folder. A dependent is any `Cargo.toml` Git tracks
//! below that folder; one that does not read as TOML is passed over, since it
//! cannot be a package Cargo builds.
//!
//! Every change goes through `toml_edit`, which keeps comments, blank lines
//! and the order of keys. A file whose text does not change is not written, so
//! a second call changes nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use nebensound_declaration::TagVersion;
use nebensound_declaration::names::VersionWriterName;
use nebensound_declaration::path::RelativePath;
use serde::Deserialize;
use thiserror::Error;
use toml_edit::{ArrayOfTables, DocumentMut, Item, RawString, Table, TableLike, Value};

use crate::ignored::folder;
use crate::plan::{Plan, Versioned};
use crate::repository::Repository;

/// The writer this module is, as the register names it.
pub const CARGO: &str = "cargo";

/// The tables a manifest lists its dependencies in.
const DEPENDENCIES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// One unit that carries a version, as the plan names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    /// Which unit this is, as a reader is told.
    pub unit: String,
    /// Where it lives.
    pub path: RelativePath,
    /// The version of the commit at hand; [`None`] when the commit does not
    /// touch the unit.
    pub version: Option<TagVersion>,
    /// The version writers it selects.
    pub writers: Vec<VersionWriterName>,
}

impl Order {
    /// Every unit of a plan that carries a version.
    #[must_use]
    pub fn of(plan: &Plan) -> Vec<Self> {
        plan.versions
            .iter()
            .map(|Versioned { unit, writers }| Self {
                unit: unit.name.to_string(),
                path: unit.path.clone(),
                version: unit.version,
                writers: writers.clone(),
            })
            .collect()
    }

    /// The same from a plan as `nebensound plan` printed it: what Declare
    /// hands on to a job whose checkout holds no history to resolve a version
    /// from.
    ///
    /// # Errors
    ///
    /// Returns what the text failed on when it is not such a plan.
    pub fn read(plan: &str) -> Result<Vec<Self>, serde_json::Error> {
        let handed: Handed = serde_json::from_str(plan)?;
        Ok(handed
            .versions
            .into_iter()
            .map(|entry| Self {
                unit: entry.name.map_or_else(
                    || "the repository".to_owned(),
                    |name| format!("component {name}"),
                ),
                path: entry.path,
                version: entry.version,
                writers: entry.writers,
            })
            .collect())
    }
}

/// A printed plan, as far as the writers read it.
#[derive(Deserialize)]
struct Handed {
    versions: Vec<HandedVersion>,
}

/// One entry of `versions` in a printed plan.
#[derive(Deserialize)]
struct HandedVersion {
    name: Option<String>,
    path: RelativePath,
    version: Option<TagVersion>,
    writers: Vec<VersionWriterName>,
}

/// Why the versions could not be written.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WriteError {
    /// A file that has to be read could not be.
    #[error("{path} is not readable: {message}")]
    Unreadable {
        /// The file, inside the repository.
        path: String,
        /// What the operating system said.
        message: String,
    },
    /// A file that has to be changed is not TOML.
    #[error("{path} is not TOML: {message}")]
    Malformed {
        /// The file, inside the repository.
        path: String,
        /// What it failed on.
        message: String,
    },
    /// The unit's manifest describes no package.
    #[error(
        "{unit} selects the version writer cargo, and {path} has no [package] with a name: there \
         is no package to give the version to"
    )]
    NoPackage {
        /// Which unit this is.
        unit: String,
        /// Its manifest.
        path: String,
    },
    /// A changed file could not be written back.
    #[error("{path} could not be written: {message}")]
    Unwritable {
        /// The file, inside the repository.
        path: String,
        /// What the operating system said.
        message: String,
    },
}

/// Writes the version of every unit with the writer `cargo` and returns what
/// it did, one line per sentence.
///
/// `manifests` are the `Cargo.toml` files Git tracks: the dependents are
/// looked for among them.
///
/// # Errors
///
/// Returns [`WriteError`] for a manifest or a lockfile that cannot be read,
/// is not TOML or cannot be written back, and for a unit whose manifest holds
/// no package. Every file is read before the first one is written, so only a
/// file that cannot be written back leaves the others written.
pub fn write(
    repository: &Repository,
    manifests: &[RelativePath],
    orders: &[Order],
) -> Result<String, WriteError> {
    let mut files = Files {
        repository,
        open: BTreeMap::new(),
    };
    let mut said = Vec::new();
    for order in orders
        .iter()
        .filter(|order| order.writers.iter().any(|writer| writer.as_str() == CARGO))
    {
        let Some(version) = order.version else {
            said.push(format!(
                "{} carries no version at this commit, so none is written for it.",
                order.unit
            ));
            continue;
        };
        let version = version.to_string();
        let manifest = order.path.join("Cargo.toml");
        let document = files.strict(&manifest)?;
        let name = package_version(document, &version).ok_or_else(|| WriteError::NoPackage {
            unit: order.unit.clone(),
            path: manifest.to_string(),
        })?;
        let root =
            own_root(document, &order.path).unwrap_or_else(|| files.workspace_above(&order.path));
        said.push(format!(
            "{}: the package {name} at {} gets {version} (workspace root: {root}).",
            order.unit, order.path
        ));
        let mut dependents = Vec::new();
        for dependent in manifests.iter().filter(|manifest| root.contains(manifest)) {
            let place = folder(dependent);
            let Some(document) = files.lenient(dependent) else {
                continue;
            };
            let Links {
                mut paths,
                inherited,
            } = require(document, &place, &order.path, &version);
            let own = own_root(document, &place);
            let workspace = own.unwrap_or_else(|| files.workspace_above(&place));
            paths.extend(
                files
                    .lenient(&workspace.join("Cargo.toml"))
                    .map(|manifest| shared(manifest, &workspace, &inherited))
                    .unwrap_or_default(),
            );
            dependents.push((place, workspace, paths));
        }
        // Every workspace that reaches the package through path dependencies,
        // however many steps away, locks it. Such as the `fuzz/` folder `cargo
        // fuzz init` creates, with its own workspace and a path back up to a
        // package that depends on this one.
        let mut workspaces = BTreeSet::from([root.clone()]);
        let mut reached = BTreeSet::from([order.path.clone()]);
        loop {
            let before = reached.len();
            for (place, workspace, paths) in &dependents {
                if !paths.is_disjoint(&reached) && reached.insert(place.clone()) {
                    workspaces.insert(workspace.clone());
                }
            }
            if reached.len() == before {
                break;
            }
        }
        for workspace in workspaces {
            let lockfile = workspace.join("Cargo.lock");
            if fs::symlink_metadata(repository.root().join(lockfile.to_string())).is_ok() {
                lock(files.strict(&lockfile)?, &name, &version);
            } else {
                said.push(format!("There is no {lockfile} to bring in line."));
            }
        }
    }
    let written = files.save()?;
    if written.is_empty() {
        said.push("Nothing to write: every file already holds what it should.".to_owned());
    }
    for path in written {
        said.push(format!("Wrote {path}."));
    }
    Ok(said.join("\n"))
}

/// Sets `package.version` and returns the name of the package, or [`None`]
/// when the manifest holds no package with a name.
fn package_version(manifest: &mut DocumentMut, version: &str) -> Option<String> {
    let package = manifest
        .get_mut("package")
        .and_then(Item::as_table_like_mut)?;
    let name = package
        .get("name")
        .and_then(Item::as_str)
        .map(str::to_owned)?;
    set(package, "version", version);
    Some(name)
}

/// Where the dependencies of a manifest lead.
struct Links {
    /// The folders its path dependencies name.
    paths: BTreeSet<RelativePath>,
    /// The names of the dependencies it takes from its workspace with
    /// `workspace = true`.
    inherited: Vec<String>,
}

/// Gives every path dependency on `target` the version as its requirement,
/// and returns where the manifest's dependencies lead.
///
/// `folder` is where the manifest lies, which its paths are relative to.
fn require(
    manifest: &mut DocumentMut,
    folder: &RelativePath,
    target: &RelativePath,
    version: &str,
) -> Links {
    let mut tables: Vec<&mut dyn TableLike> = Vec::new();
    let tables_in = manifest
        .iter_mut()
        .filter_map(|(key, item)| item.as_table_like_mut().map(|table| (key, table)));
    for (key, table) in tables_in {
        match key.get() {
            kind if DEPENDENCIES.contains(&kind) => tables.push(table),
            // Per platform: `[target.'cfg(unix)'.dependencies]`.
            "target" => {
                for (_, platform) in table.iter_mut() {
                    tables.extend(
                        platform
                            .as_table_like_mut()
                            .into_iter()
                            .flat_map(|platform| {
                                platform.iter_mut().filter_map(|(kind, deps)| {
                                    DEPENDENCIES
                                        .contains(&kind.get())
                                        .then(|| deps.as_table_like_mut())
                                        .flatten()
                                })
                            }),
                    );
                }
            }
            // Inherited with `workspace = true`, written once at the root.
            "workspace" => tables.extend(
                table
                    .get_mut("dependencies")
                    .and_then(Item::as_table_like_mut),
            ),
            _ => {}
        }
    }
    let mut links = Links {
        paths: BTreeSet::new(),
        inherited: Vec::new(),
    };
    for table in tables {
        for (name, dependency) in table.iter_mut() {
            let Some(dependency) = dependency.as_table_like_mut() else {
                continue;
            };
            if dependency.get("workspace").and_then(Item::as_bool) == Some(true) {
                links.inherited.push(name.get().to_owned());
            }
            let Some(path) = dependency
                .get("path")
                .and_then(Item::as_str)
                .and_then(|path| resolve(folder, path))
            else {
                continue;
            };
            if &path == target {
                set(dependency, "version", version);
            }
            links.paths.insert(path);
        }
    }
    links
}

/// The folders the `[workspace.dependencies]` of a workspace's manifest name
/// by path for the dependencies `inherited`.
fn shared(
    manifest: &DocumentMut,
    workspace: &RelativePath,
    inherited: &[String],
) -> Vec<RelativePath> {
    let dependencies = manifest
        .get("workspace")
        .and_then(|table| table.get("dependencies"))
        .and_then(Item::as_table_like);
    inherited
        .iter()
        .filter_map(|name| {
            dependencies
                .and_then(|dependencies| dependencies.get(name))
                .and_then(|dependency| dependency.get("path"))
                .and_then(Item::as_str)
        })
        .filter_map(|written| resolve(workspace, written))
        .collect()
}

/// Gives the package's entry in a lockfile the version, and every entry that
/// names it together with its old version the new one.
///
/// The package's entry is the one with its name and no `source`: a package of
/// the workspace, not one from a registry or a Git repository. An entry names
/// another one by its name alone where that is unique, and by name and version
/// where it is not.
fn lock(lockfile: &mut DocumentMut, name: &str, version: &str) {
    let mut packages: Vec<&mut Table> = lockfile
        .get_mut("package")
        .and_then(Item::as_array_of_tables_mut)
        .into_iter()
        .flat_map(ArrayOfTables::iter_mut)
        .collect();
    let mut old = Vec::new();
    for package in &mut packages {
        if package.get("name").and_then(Item::as_str) == Some(name)
            && !package.contains_key("source")
        {
            old.extend(
                package
                    .get("version")
                    .and_then(Item::as_str)
                    .map(str::to_owned),
            );
            set(*package, "version", version);
        }
    }
    let named = |entry: &Value| {
        entry
            .as_str()
            .and_then(|entry| entry.split_once(' '))
            .is_some_and(|(entry, was)| entry == name && old.iter().any(|old| old == was))
    };
    for package in packages {
        if let Some(dependencies) = package.get_mut("dependencies").and_then(Item::as_array_mut) {
            for index in 0..dependencies.len() {
                if dependencies.get(index).is_some_and(named) {
                    dependencies.replace(index, format!("{name} {version}"));
                }
            }
        }
    }
}

/// Sets a key to a string, keeping what stands around the old value. Nothing
/// changes when it holds that string already.
fn set(table: &mut dyn TableLike, key: &str, text: &str) {
    match table.get_mut(key) {
        Some(Item::Value(value)) => {
            if value.as_str() != Some(text) {
                let decor = value.decor().clone();
                *value = Value::from(text);
                *value.decor_mut() = decor;
            }
        }
        // `version.workspace = true`, which a written version replaces.
        Some(item) => *item = Item::Value(Value::from(text)),
        // Last in the table. In an inline table the space before the closing
        // brace stands behind the value that was last, and moves along.
        None => {
            let mut value = Value::from(text);
            if let Some(last) = table
                .iter_mut()
                .last()
                .and_then(|(_, item)| item.as_value_mut())
                && let Some(space) = last
                    .decor()
                    .suffix()
                    .and_then(RawString::as_str)
                    .filter(|suffix| suffix.trim().is_empty())
                    .map(str::to_owned)
            {
                last.decor_mut().set_suffix("");
                value.decor_mut().set_suffix(space);
            }
            table.insert(key, Item::Value(value));
        }
    }
}

/// A path as a manifest writes it, from the folder it lies in; [`None`] when
/// it leads out of the repository.
fn resolve(folder: &RelativePath, written: &str) -> Option<RelativePath> {
    if written.starts_with('/') {
        return None;
    }
    let mut segments: Vec<&str> = folder.segments().iter().map(String::as_str).collect();
    for segment in written.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            segment => segments.push(segment),
        }
    }
    Some(
        segments
            .into_iter()
            .fold(RelativePath::root(), |path, segment| path.join(segment)),
    )
}

/// The root of the workspace as the package's own manifest says it: its own
/// folder for a manifest with a `[workspace]`, the folder `package.workspace`
/// names, or [`None`] when it says neither.
fn own_root(manifest: &DocumentMut, package: &RelativePath) -> Option<RelativePath> {
    if manifest.contains_key("workspace") {
        return Some(package.clone());
    }
    manifest
        .get("package")
        .and_then(|table| table.get("workspace"))
        .and_then(Item::as_str)
        .and_then(|written| resolve(package, written))
}

/// One file read for changing, and its text as it was read.
struct Open {
    original: String,
    document: DocumentMut,
}

/// The files read so far, each once, so that the changes of several units to
/// one file add up.
struct Files<'a> {
    repository: &'a Repository,
    /// [`None`] for a file that was looked for leniently and does not read.
    open: BTreeMap<RelativePath, Option<Open>>,
}

impl Files<'_> {
    /// A file that has to read.
    fn strict(&mut self, path: &RelativePath) -> Result<&mut DocumentMut, WriteError> {
        let repository = self.repository;
        let slot = self.open.entry(path.clone()).or_default();
        let opened = match slot.take() {
            Some(opened) => opened,
            None => open(repository, path)?,
        };
        Ok(&mut slot.insert(opened).document)
    }

    /// A file that may be missing or no TOML: [`None`] then.
    fn lenient(&mut self, path: &RelativePath) -> Option<&mut DocumentMut> {
        let repository = self.repository;
        self.open
            .entry(path.clone())
            .or_insert_with(|| open(repository, path).ok())
            .as_mut()
            .map(|opened| &mut opened.document)
    }

    /// The root of the workspace the package at `package` belongs to when its
    /// own manifest does not say: the nearest folder above it whose manifest
    /// has a `[workspace]` that does not exclude it, else its own folder.
    fn workspace_above(&mut self, package: &RelativePath) -> RelativePath {
        let mut above = package.clone();
        while !above.is_root() {
            above = folder(&above);
            if let Some(workspace) = self
                .lenient(&above.join("Cargo.toml"))
                .and_then(|manifest| manifest.get("workspace"))
            {
                let excluded = workspace
                    .get("exclude")
                    .and_then(Item::as_array)
                    .is_some_and(|excluded| {
                        excluded
                            .iter()
                            .filter_map(Value::as_str)
                            .filter_map(|written| resolve(&above, written))
                            .any(|excluded| excluded.contains(package))
                    });
                return if excluded { package.clone() } else { above };
            }
        }
        package.clone()
    }

    /// Writes back every file whose text changed and returns them.
    fn save(self) -> Result<Vec<RelativePath>, WriteError> {
        let mut written = Vec::new();
        let opened = self
            .open
            .into_iter()
            .filter_map(|(path, opened)| opened.map(|opened| (path, opened)));
        for (path, opened) in opened {
            let text = opened.document.to_string();
            if text != opened.original {
                self.repository
                    .write(&path.to_string(), &text)
                    .map_err(|error| WriteError::Unwritable {
                        path: path.to_string(),
                        message: error.to_string(),
                    })?;
                written.push(path);
            }
        }
        Ok(written)
    }
}

/// Reads one file for changing.
fn open(repository: &Repository, path: &RelativePath) -> Result<Open, WriteError> {
    let original = repository
        .read(&path.to_string())
        .map_err(|error| WriteError::Unreadable {
            path: path.to_string(),
            message: error.to_string(),
        })?;
    let document = original
        .parse::<DocumentMut>()
        .map_err(|error| WriteError::Malformed {
            path: path.to_string(),
            message: error.to_string(),
        })?;
    Ok(Open { original, document })
}
