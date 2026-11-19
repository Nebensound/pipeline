//! The version writer `cargo`: the version of the plan in the manifests.
//!
//! Cargo cannot take a version from outside: the version of a package stands in
//! its `Cargo.toml` and nowhere else. No manifest in the tree carries one,
//! because tags are the truth. So the version a unit carries at the commit at
//! hand is written into the checkout of the run, immediately before building,
//! and never committed.
//!
//! Every unit of the plan with the writer `cargo` that the commit at hand
//! touches gets the version of the commit, `-pr.<number>` behind it on a pull
//! request. A unit the commit does not touch gets the last version released
//! for it only where a package that gets a version depends on it by path,
//! followed through further path dependencies, so that the requirement names
//! a version that exists. Any other unit gets nothing: its manifest stays as
//! it is, and the package test does not package it. A package that gets a
//! version and depends by path, for its tests as well, on a unit that is
//! neither touched nor ever released is an error that names both: there is no
//! version it could require. A path dependency on a package that carries no
//! version at all, no unit with the writer `cargo`, is left to Cargo, which
//! refuses to package the dependent. With the version:
//!
//! - `package.version` in the unit's `Cargo.toml` becomes that version;
//! - every path dependency on that package, wherever in the repository the
//!   manifest that names it lies, gets that version as its requirement, since
//!   a package whose dependency names no version cannot be published. The
//!   manifest of a unit that gets nothing is left out, apart from the entries
//!   of the `[workspace.dependencies]` it holds as the root of a workspace
//!   that a member that gets a version inherits;
//! - the package's entry in the workspace's `Cargo.lock` gets that version,
//!   and so does every entry that names it together with a version, so that
//!   `--locked` holds. No other entry changes. The same goes for the
//!   `Cargo.lock` of every other workspace that reaches the package through
//!   path dependencies, directly or through other packages: one that contains
//!   it while excluding it, one beside it, or a `fuzz/` folder with a
//!   `[workspace]` of its own.
//!
//! A call can start from the units of one workspace alone, as the package
//! test of one workspace does, so that what another workspace needs cannot
//! fail it. Only a touched unit that belongs to that workspace, as Cargo finds
//! it, gets the version of the commit on its own account; a workspace of its
//! own in a folder below, which the workspace excludes or never names, does
//! not belong to it. Any other unit gets a version only where one that gets a
//! version depends on it by path, the version of the commit when the commit
//! touches it and its last release otherwise.
//!
//! The workspace is found the way Cargo finds it: the folder the manifest
//! names in `package.workspace`, else the nearest folder at or above the
//! package whose `Cargo.toml` has a `[workspace]` that does not exclude it,
//! else the package's own folder. A dependent is any `Cargo.toml` Git tracks,
//! in that workspace or outside it; one that does not read as TOML is passed
//! over, since it cannot be a package Cargo builds.
//!
//! Every change goes through `toml_edit`, which keeps comments, blank lines
//! and the order of keys. A file whose text does not change is not written, so
//! a second call changes nothing.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
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
use crate::versions::of_pull_request;

/// The writer this module is, as the register names it.
pub const CARGO: &str = "cargo";

/// The tables a manifest lists its dependencies in. A published package
/// needs a version for each of them: Cargo drops a path dev-dependency without
/// one from the package, and the tests of the packaged copy that use it no
/// longer compile.
const DEPENDENCIES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// The kind [`dependency_tables`] gives `[workspace.dependencies]`.
const WORKSPACE: &str = "workspace";

/// What a package depends on by path: each folder with the name of the
/// package there.
type Needs = Vec<(RelativePath, String)>;

/// The dependencies a manifest takes from its workspace with
/// `workspace = true`, each by its key and a label that [`shared`] gives back
/// with its folder.
type Inherited = Vec<(String, String)>;

/// One unit that carries a version, as the plan names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    /// Which unit this is, as a reader is told.
    pub unit: String,
    /// Where it lives.
    pub path: RelativePath,
    /// The version of the commit at hand as it is written: on a pull request
    /// the one its squash merge would get, with `-pr.<number>` behind it.
    /// [`None`] when the commit does not touch the unit.
    pub version: Option<String>,
    /// The last version released for the unit: its newest counted tag, a
    /// pre-release included. [`None`] when it was never released.
    pub last: Option<TagVersion>,
    /// The version writers it selects.
    pub writers: Vec<VersionWriterName>,
}

impl Order {
    /// Every unit of a plan that carries a version, for the pull request the
    /// plan is built for if there is one.
    #[must_use]
    pub fn of(plan: &Plan) -> Vec<Self> {
        plan.versions
            .iter()
            .map(|Versioned { unit, writers }| Self {
                unit: unit.name.to_string(),
                path: unit.path.clone(),
                version: unit.version.map(|version| {
                    plan.pull_request.map_or_else(
                        || version.to_string(),
                        |number| of_pull_request(&version, number),
                    )
                }),
                last: unit.last,
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
                version: entry.version.map(|written| written.0),
                last: entry.last,
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
    version: Option<Written>,
    last: Option<TagVersion>,
    writers: Vec<VersionWriterName>,
}

/// A version as a plan hands it on: `X.Y.Z` or `X.Y.Z-pre.N`, and on a pull
/// request `-pr.<number>` behind it.
#[derive(Deserialize)]
#[serde(try_from = "String")]
struct Written(String);

impl TryFrom<String> for Written {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        let merged = text
            .rsplit_once("-pr.")
            .filter(|(_, number)| {
                !number.starts_with('0')
                    && !number.is_empty()
                    && number.bytes().all(|byte| byte.is_ascii_digit())
            })
            .map_or(text.as_str(), |(merged, _)| merged);
        if TagVersion::parse(merged).is_some() {
            Ok(Self(text))
        } else {
            Err(format!(
                "{text:?} is no version: X.Y.Z or X.Y.Z-pre.N, with -pr.<number> behind it on a \
                 pull request"
            ))
        }
    }
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
    /// A package that gets a version depends on a unit that gets none.
    #[error(
        "the package {package} gets {version} and depends by path on the package {dependency} at \
         {path}, and {unit} is neither touched by this commit nor ever released: there is no \
         version of {dependency} that {package} could require. A release of {unit} (its tag) \
         gives it one; until then, a commit that touches {unit} as well passes"
    )]
    UnreleasedDependency {
        /// The package that gets a version.
        package: String,
        /// The version it gets.
        version: String,
        /// The package it depends on, as it names it.
        dependency: String,
        /// Where that package lies.
        path: String,
        /// The unit that package is.
        unit: String,
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
/// looked for among them. `workspace` is a folder of the workspace whose
/// units start the writing: only a touched unit that belongs to the workspace
/// Cargo finds there gets the version of the commit on its own account, and
/// the path dependencies of those are followed wherever they lie. [`None`]
/// starts from every touched unit; the package test of one workspace starts
/// from its own units, so that what another workspace needs cannot fail it.
///
/// # Errors
///
/// Returns [`WriteError`] for a manifest or a lockfile that cannot be read,
/// is not TOML or cannot be written back, for a unit that gets a version and
/// whose manifest holds no package, and for a package that gets a version and
/// depends by path on a unit that is neither touched nor ever released. Every
/// file is read before the first one is written, so only a file that cannot
/// be written back leaves the others written.
pub fn write(
    repository: &Repository,
    manifests: &[RelativePath],
    orders: &[Order],
    workspace: Option<&RelativePath>,
) -> Result<String, WriteError> {
    let mut files = Files {
        repository,
        open: BTreeMap::new(),
    };
    let cargo: Vec<&Order> = orders
        .iter()
        .filter(|order| order.writers.iter().any(|writer| writer.as_str() == CARGO))
        .collect();
    let start = workspace.map(|folder| files.root_of(folder));
    let chosen = choose(&mut files, &cargo, start.as_ref())?;
    let unwritten = unwritten(&cargo, &chosen);
    let mut said = Vec::new();
    for (index, order) in cargo.iter().enumerate() {
        let Some(Chosen {
            name,
            version,
            root,
            touched,
            ..
        }) = chosen.get(&index)
        else {
            said.push(passed_over(order, start.as_ref()));
            continue;
        };
        said.push(announced(order, name, version, root, *touched));
        let mut dependents = Vec::new();
        // A dependent may lie anywhere in the repository: a member of the
        // workspace, a package it excludes, or a workspace of its own beside it.
        for dependent in manifests {
            let place = folder(dependent);
            let Some(document) = files.lenient(dependent) else {
                continue;
            };
            let Links {
                mut paths,
                inherited,
            } = require(
                document,
                &place,
                &order.path,
                version,
                unwritten.get(&place),
            );
            let workspace = files.root_of(&place);
            paths.extend(
                files
                    .lenient(&workspace.join("Cargo.toml"))
                    .map(|manifest| shared(manifest, &workspace, &inherited))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(path, _)| path),
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
                lock(files.strict(&lockfile)?, name, version);
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

/// A unit that gets a version, with its package version already set.
struct Chosen {
    /// The name of its package.
    name: String,
    /// The version it gets.
    version: String,
    /// The root of its workspace.
    root: RelativePath,
    /// Whether the commit at hand touches it, which gives it the version of
    /// the commit rather than its last release.
    touched: bool,
    /// What it depends on by path where it is published.
    needs: Needs,
    /// The keys of the dependencies it inherits from the
    /// `[workspace.dependencies]` of its workspace with `workspace = true`.
    inherits: BTreeSet<String>,
}

/// Picks the units that get a version, by their place in `cargo`, and sets
/// the version of each one's package.
///
/// Every unit the commit touches gets the version of the commit, when `start`
/// is [`Some`] only one whose workspace has that root. Any other unit gets a
/// version only where a package that gets one depends on it by path, followed
/// through the path dependencies of each one it gives a version to: the
/// version of the commit when the commit touches it, its last release
/// otherwise. A package such a unit cannot give a version to, neither touched
/// nor ever released, is an error that names both. A path dependency on a
/// package that is no unit with the writer `cargo` is left to Cargo, which
/// refuses to package a dependent without a version requirement.
fn choose(
    files: &mut Files<'_>,
    cargo: &[&Order],
    start: Option<&RelativePath>,
) -> Result<BTreeMap<usize, Chosen>, WriteError> {
    let mut chosen = BTreeMap::new();
    // Each package given a version whose path dependencies are still to be
    // followed: its name, its version and what it depends on.
    let mut queue = VecDeque::new();
    for (index, order) in cargo.iter().enumerate() {
        let Some(version) = &order.version else {
            continue;
        };
        if start.is_none_or(|start| files.root_of(&order.path) == *start) {
            let unit = files.chosen(order, version.clone(), true)?;
            queue.push_back((unit.name.clone(), unit.version.clone(), unit.needs.clone()));
            chosen.insert(index, unit);
        }
    }
    while let Some((package, version, needs)) = queue.pop_front() {
        for (path, dependency) in needs {
            let Some((found, order)) = cargo
                .iter()
                .enumerate()
                .find(|(found, order)| order.path == path && !chosen.contains_key(found))
            else {
                continue;
            };
            let (given, touched) = match (&order.version, order.last) {
                (Some(touched), _) => (touched.clone(), true),
                (None, Some(last)) => (last.to_string(), false),
                (None, None) => {
                    return Err(WriteError::UnreleasedDependency {
                        package,
                        version,
                        dependency,
                        path: path.to_string(),
                        unit: order.unit.clone(),
                    });
                }
            };
            let unit = files.chosen(order, given, touched)?;
            queue.push_back((unit.name.clone(), unit.version.clone(), unit.needs.clone()));
            chosen.insert(found, unit);
        }
    }
    Ok(chosen)
}

/// The folders of the units that get nothing, whose manifests stay as they
/// are, each with the keys of its `[workspace.dependencies]` that a member
/// that gets a version inherits: the only entries written there, when the
/// unit is the root of that member's workspace.
fn unwritten(
    cargo: &[&Order],
    chosen: &BTreeMap<usize, Chosen>,
) -> BTreeMap<RelativePath, BTreeSet<String>> {
    cargo
        .iter()
        .enumerate()
        .filter(|(index, _)| !chosen.contains_key(index))
        .map(|(_, order)| {
            let keys = chosen
                .values()
                .filter(|unit| unit.root == order.path)
                .flat_map(|unit| unit.inherits.iter().cloned())
                .collect();
            (order.path.clone(), keys)
        })
        .collect()
}

/// What is said about a unit that gets nothing: one the commit touches lies
/// outside the workspace the call starts from, whose root is `start`.
fn passed_over(order: &Order, start: Option<&RelativePath>) -> String {
    if let (Some(_), Some(start)) = (&order.version, start) {
        format!(
            "{} lies outside the workspace at {start}, and no package that gets a version \
             depends on it by path, so nothing is written for it.",
            order.unit
        )
    } else {
        format!(
            "{} is not touched by this commit, and no package that gets a version depends on it \
             by path, so nothing is written for it.",
            order.unit
        )
    }
}

/// What is said about a unit that gets a version: the one of the commit at
/// hand when it touches the unit, the last release otherwise.
fn announced(
    order: &Order,
    name: &str,
    version: &str,
    root: &RelativePath,
    touched: bool,
) -> String {
    if touched {
        format!(
            "{}: the package {name} at {} gets {version} (workspace root: {root}).",
            order.unit, order.path
        )
    } else {
        format!(
            "{} is not touched by this commit: the package {name} at {} gets its last release \
             {version} (workspace root: {root}).",
            order.unit, order.path
        )
    }
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
    /// The dependencies it takes from its workspace with `workspace = true`,
    /// each by its key, twice: [`shared`] takes a key and a label.
    inherited: Inherited,
}

/// Gives every path dependency on `target` the version as its requirement,
/// and returns where the manifest's dependencies lead.
///
/// `folder` is where the manifest lies, which its paths are relative to.
/// `only` is [`Some`] for the manifest of a unit with the writer `cargo` that
/// gets no version: it stays as it is, apart from the entries of the
/// `[workspace.dependencies]` it holds as the root of a workspace whose keys
/// `only` names, those that a member that gets a version inherits.
fn require(
    manifest: &mut DocumentMut,
    folder: &RelativePath,
    target: &RelativePath,
    version: &str,
    only: Option<&BTreeSet<String>>,
) -> Links {
    let mut links = Links {
        paths: BTreeSet::new(),
        inherited: Vec::new(),
    };
    for (kind, table) in dependency_tables(manifest) {
        for (name, dependency) in table.iter_mut() {
            let Some(dependency) = dependency.as_table_like_mut() else {
                continue;
            };
            if dependency.get("workspace").and_then(Item::as_bool) == Some(true) {
                links
                    .inherited
                    .push((name.get().to_owned(), name.get().to_owned()));
            }
            let Some(path) = dependency
                .get("path")
                .and_then(Item::as_str)
                .and_then(|path| resolve(folder, path))
            else {
                continue;
            };
            let wanted = only.is_none_or(|keys| kind == WORKSPACE && keys.contains(name.get()));
            if &path == target && wanted {
                set(dependency, "version", version);
            }
            links.paths.insert(path);
        }
    }
    links
}

/// Every table a manifest lists dependencies in, each with its kind: one of
/// [`DEPENDENCIES`], also per platform (`[target.'cfg(unix)'.dependencies]`),
/// and [`WORKSPACE`] for `[workspace.dependencies]`, where the dependencies
/// inherited with `workspace = true` are written once at the root.
fn dependency_tables(manifest: &mut DocumentMut) -> Vec<(&'static str, &mut dyn TableLike)> {
    let kind_of = |key: &str| DEPENDENCIES.into_iter().find(|kind| *kind == key);
    let mut tables: Vec<(&'static str, &mut dyn TableLike)> = Vec::new();
    let tables_in = manifest
        .iter_mut()
        .filter_map(|(key, item)| item.as_table_like_mut().map(|table| (key, table)));
    for (key, table) in tables_in {
        match key.get() {
            "target" => {
                for (_, platform) in table.iter_mut() {
                    tables.extend(
                        platform
                            .as_table_like_mut()
                            .into_iter()
                            .flat_map(|platform| {
                                platform.iter_mut().filter_map(|(kind, deps)| {
                                    kind_of(kind.get()).zip(deps.as_table_like_mut())
                                })
                            }),
                    );
                }
            }
            WORKSPACE => tables.extend(
                table
                    .get_mut("dependencies")
                    .and_then(Item::as_table_like_mut)
                    .map(|dependencies| (WORKSPACE, dependencies)),
            ),
            kind => tables.extend(kind_of(kind).map(|kind| (kind, table))),
        }
    }
    tables
}

/// The folders the `[workspace.dependencies]` of a workspace's manifest name
/// by path for the dependencies `inherited`, each given by its key and a
/// label that comes back with its folder. A rename with `package = "..."`
/// lives in the workspace's entry, so when that entry has one, its name comes
/// back instead of the label.
fn shared(manifest: &DocumentMut, workspace: &RelativePath, inherited: &Inherited) -> Needs {
    let dependencies = manifest
        .get("workspace")
        .and_then(|table| table.get("dependencies"))
        .and_then(Item::as_table_like);
    inherited
        .iter()
        .filter_map(|(key, label)| {
            dependencies
                .and_then(|dependencies| dependencies.get(key))
                .and_then(|dependency| {
                    let name = dependency
                        .get("package")
                        .and_then(Item::as_str)
                        .unwrap_or(label);
                    dependency
                        .get("path")
                        .and_then(Item::as_str)
                        .and_then(|written| resolve(workspace, written))
                        .map(|path| (path, name.to_owned()))
                })
        })
        .collect()
}

/// What a package depends on by path where it is published, the
/// dependencies of its tests included: each folder its manifest names with
/// the name of the package there, and each dependency it inherits with
/// `workspace = true` by its key and that name, for [`shared`] to find.
///
/// `folder` is where the manifest lies, which its paths are relative to.
fn published(manifest: &mut DocumentMut, folder: &RelativePath) -> (Needs, Inherited) {
    let mut needs = Vec::new();
    let mut inherited = Vec::new();
    for (_, table) in dependency_tables(manifest)
        .into_iter()
        .filter(|(kind, _)| *kind != WORKSPACE)
    {
        let dependencies = table.iter().filter_map(|(key, dependency)| {
            dependency
                .as_table_like()
                .map(|dependency| (key, dependency))
        });
        for (key, dependency) in dependencies {
            // Renamed with `package = "..."`, the key is not the name.
            let name = dependency
                .get("package")
                .and_then(Item::as_str)
                .unwrap_or(key)
                .to_owned();
            if dependency.get("workspace").and_then(Item::as_bool) == Some(true) {
                inherited.push((key.to_owned(), name));
            } else {
                needs.extend(
                    dependency
                        .get("path")
                        .and_then(Item::as_str)
                        .and_then(|written| resolve(folder, written))
                        .map(|path| (path, name)),
                );
            }
        }
    }
    (needs, inherited)
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

    /// The unit `order` given `version`: its package's version set, and what
    /// it depends on by path where it is published.
    fn chosen(
        &mut self,
        order: &Order,
        version: String,
        touched: bool,
    ) -> Result<Chosen, WriteError> {
        let manifest = order.path.join("Cargo.toml");
        let document = self.strict(&manifest)?;
        let name = package_version(document, &version).ok_or_else(|| WriteError::NoPackage {
            unit: order.unit.clone(),
            path: manifest.to_string(),
        })?;
        let (mut needs, inherited) = published(document, &order.path);
        let root = self.root_of(&order.path);
        needs.extend(
            self.lenient(&root.join("Cargo.toml"))
                .map(|manifest| shared(manifest, &root, &inherited))
                .unwrap_or_default(),
        );
        Ok(Chosen {
            name,
            version,
            root,
            touched,
            needs,
            inherits: inherited.into_iter().map(|(key, _)| key).collect(),
        })
    }

    /// The root of the workspace the package at `package` belongs to, as Cargo
    /// finds it: what its own manifest says, else [`Self::workspace_above`].
    /// A folder without a manifest that reads is found the same way, from
    /// above.
    fn root_of(&mut self, package: &RelativePath) -> RelativePath {
        self.lenient(&package.join("Cargo.toml"))
            .and_then(|manifest| own_root(manifest, package))
            .unwrap_or_else(|| self.workspace_above(package))
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
