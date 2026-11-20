//! Rust packages and `[[exclude]]`: a package is out as a whole, or not at all.
//!
//! A file with code cannot be taken out of `check-rust` on its own. Cargo
//! compiles a package with all of its files, so leaving one file out of some
//! steps and not of others would make a promise the check cannot keep. Inside a
//! package Rust has its own means for the cases an exclusion is wanted for:
//! `#[rustfmt::skip]`, `#[coverage(off)]` with a justification, `#[allow(...)]`.
//!
//! A whole package can be excluded: the folder that holds its `Cargo.toml`,
//! with a reason. It is meant for code of others inside the repository, such
//! as a vendored copy. `check-rust` then leaves it out of every step, and
//! compiles it only where another package depends on it. A package in a Git
//! submodule is not found: phase 0 sees the files Git tracks, and a submodule
//! is a single entry there.
//!
//! A package is a folder with a `Cargo.toml` that Git tracks and that has a
//! `[package]` table; a manifest that does not read counts as one, since Cargo
//! would refuse it anyway. A `.rs` file belongs to the innermost package around
//! it.

use std::collections::BTreeMap;

use crate::Analysis;
use crate::annotate::Coverage;
use crate::finding::{Finding, Position, Rule};
use crate::names::{Extension, Reason};
use crate::path::RelativePath;
use crate::pattern::Pattern;
use serde::Deserialize;
use serde_json::json;

use crate::ignored::{Read, folder};

/// A package `[[exclude]]` takes out as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Excluded {
    /// The folder that holds its `Cargo.toml`.
    pub folder: RelativePath,
    /// Why, as the entry that takes its folder out says.
    pub reason: Reason,
}

/// One package, with what holds for its manifest and its code.
struct Package<'a> {
    folder: RelativePath,
    manifest: &'a Coverage,
    code: Vec<(&'a RelativePath, &'a Coverage)>,
}

impl Package<'_> {
    /// Whether an entry that takes out the package's whole folder takes this
    /// file out.
    fn whole(&self, coverage: &Coverage) -> bool {
        matches!(
            coverage,
            Coverage::Excluded { pattern: Pattern::Path(path), .. } if path.contains(&self.folder)
        )
    }

    /// The reason it is out as a whole, or [`None`] when it is not.
    ///
    /// It is out when its manifest and every one of its `.rs` files are
    /// excluded, and an entry that takes out its whole folder is what takes
    /// out at least one of them; the reason is that entry's, the first one
    /// found from the manifest on. Which entry wins for each of the other
    /// files does not matter: a narrower one inside the folder is redundant,
    /// as it is for any other file, and no partial exclusion.
    fn excluded(&self) -> Option<&Reason> {
        let files = std::iter::once(self.manifest).chain(self.code.iter().map(|&(_, code)| code));
        let mut found = None;
        for coverage in files {
            let Coverage::Excluded { reason, .. } = coverage else {
                return None;
            };
            if found.is_none() && self.whole(coverage) {
                found = Some(reason);
            }
        }
        found
    }
}

/// Whether this `Cargo.toml` describes a package, not only a workspace.
fn is_package(manifest: &RelativePath, read: &Read<'_>) -> bool {
    read(manifest)
        .ok()
        .and_then(|source| source.parse::<toml::Table>().ok())
        .is_none_or(|table| table.contains_key("package"))
}

/// Every package the repository holds, with its code.
fn packages<'a>(analysis: &'a Analysis, read: &Read<'_>) -> Vec<Package<'a>> {
    let files = analysis.tree.files();
    let mut packages: Vec<Package<'a>> = files
        .iter()
        .filter(|(node, _)| {
            node.path.file_name() == Some("Cargo.toml") && is_package(&node.path, read)
        })
        .map(|(node, coverage)| Package {
            folder: folder(&node.path),
            manifest: coverage,
            code: Vec::new(),
        })
        .collect();
    for (node, coverage) in files {
        let extension = Extension::of(&node.path.to_string());
        if extension.as_ref().map(Extension::as_str) != Some(".rs") {
            continue;
        }
        let owner = packages
            .iter_mut()
            .filter(|package| package.folder.contains(&node.path))
            .max_by_key(|package| package.folder.segments().len());
        if let Some(owner) = owner {
            owner.code.push((&node.path, coverage));
        }
    }
    packages
}

/// The packages `[[exclude]]` takes out as a whole, in path order.
#[must_use]
pub fn excluded(analysis: &Analysis, read: &Read<'_>) -> Vec<Excluded> {
    packages(analysis, read)
        .iter()
        .filter_map(|package| {
            package.excluded().map(|reason| Excluded {
                folder: package.folder.clone(),
                reason: reason.clone(),
            })
        })
        .collect()
}

/// The list as it is printed: one line of JSON, for a building block to read.
#[must_use]
pub fn to_json(excluded: &[Excluded]) -> String {
    let entries: Vec<_> = excluded
        .iter()
        .map(|package| json!({ "path": package.folder.to_string(), "reason": package.reason.as_str() }))
        .collect();
    json!(entries).to_string()
}

#[derive(Deserialize)]
struct Spans {
    #[serde(default)]
    exclude: Vec<SpannedExclusion>,
}

#[derive(Deserialize)]
struct SpannedExclusion {
    path: toml::Spanned<Pattern>,
}

/// Where each `[[exclude]]` entry of the declaration writes its path.
///
/// The source is the one the declaration was read from, so it reads here as
/// well; should it not, the findings name no place rather than none at all.
fn positions(source: &str) -> Vec<(Pattern, Position)> {
    toml::from_str::<Spans>(source)
        .map(|spans| spans.exclude)
        .unwrap_or_default()
        .into_iter()
        .map(|entry| {
            let start = entry.path.span().start;
            let before = source.get(..start).unwrap_or_default();
            let line = before.matches('\n').count() + 1;
            let column = before
                .rsplit('\n')
                .next()
                .unwrap_or_default()
                .chars()
                .count()
                + 1;
            (entry.path.into_inner(), Position { line, column })
        })
        .collect()
}

/// Every `[[exclude]]` entry that takes code out of a package and not the
/// whole package: one finding per entry, at the place in the declaration where
/// it writes its path.
///
/// `declaration` is where the declaration lies in the repository, and `source`
/// what it holds.
#[must_use]
pub fn findings(
    analysis: &Analysis,
    declaration: &RelativePath,
    source: &str,
    read: &Read<'_>,
) -> Vec<Finding> {
    let positions = positions(source);
    let mut findings = Vec::new();
    for package in packages(analysis, read) {
        if package.excluded().is_some() {
            continue;
        }
        let mut entries: BTreeMap<&Pattern, Vec<String>> = BTreeMap::new();
        for (file, coverage) in &package.code {
            if let Coverage::Excluded { pattern, .. } = coverage {
                entries
                    .entry(pattern)
                    .or_default()
                    .push(format!("`{file}`"));
            }
        }
        for (pattern, files) in entries {
            let finding = Finding::error(
                Rule::PartialPackageExclusion,
                format!(
                    "the [[exclude]] entry `{pattern}` takes {files} out of the Rust package at \
                     {folder}, and not the whole package. A file with code cannot be excluded \
                     on its own: use what Rust has for it, `#[rustfmt::skip]`, \
                     `#[coverage(off)]` with a justification, or `#[allow(...)]`. To take the \
                     whole package out, as for code of others, exclude its folder {folder} \
                     and pull nothing of it back in",
                    files = files.join(", "),
                    folder = package.folder,
                ),
            )
            .at(declaration.clone());
            let place = positions
                .iter()
                .find(|(written, _)| written == pattern)
                .map(|(_, position)| *position);
            findings.push(match place {
                Some(position) => finding.on(position),
                None => finding,
            });
        }
    }
    findings
}
