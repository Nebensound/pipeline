//! Where Cargo finds the workspace a package belongs to.
//!
//! The version writer `cargo` needs it to know which `Cargo.lock` to change,
//! and phase 0 to know where a package's `Cargo.lock` stands
//! ([`crate::lockfiles`]). Both find it here, so that the two cannot disagree.
//!
//! The workspace is found the way Cargo finds it: the package's own folder
//! when its manifest has a `[workspace]`, else the folder it names in
//! `package.workspace`, else the nearest folder above it whose `Cargo.toml`
//! has a `[workspace]` that does not exclude it or names a folder in
//! `package.workspace`, else the package's own folder. A `[workspace]` that
//! excludes the package does not end the search, and an entry of `members`
//! that names the package keeps it in although `exclude` covers it.

use toml_edit::{DocumentMut, Item, Value};

use crate::ignored::folder;
use crate::path::RelativePath;

/// The manifests of a repository as the search reads them.
pub trait Manifests {
    /// The `Cargo.toml` at this path, or [`None`] when there is none or it
    /// does not read as TOML: Cargo cannot build from it, so it ends nothing.
    fn manifest(&mut self, path: &RelativePath) -> Option<&DocumentMut>;
}

/// A path as a manifest writes it, from the folder it lies in; [`None`] when
/// it leads out of the repository.
pub fn resolve(folder: &RelativePath, written: &str) -> Option<RelativePath> {
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

/// The root of the workspace the package at `package` belongs to, as Cargo
/// finds it: what its own manifest says, else the search above it. A folder
/// without a manifest that reads is found the same way, from above.
pub fn root_of(manifests: &mut impl Manifests, package: &RelativePath) -> RelativePath {
    manifests
        .manifest(&package.join("Cargo.toml"))
        .and_then(|manifest| own_root(manifest, package))
        .unwrap_or_else(|| workspace_above(manifests, package))
}

/// The root of the workspace the package at `package` belongs to when its own
/// manifest does not say, searched upwards from the folder above it as Cargo
/// searches: a folder whose manifest has a `[workspace]` that excludes the
/// package is passed over, and the search goes on above it; a package that
/// `members` names literally is never excluded; a manifest without a
/// `[workspace]` that names its workspace in `package.workspace` ends the
/// search there. Nothing found, the package's own folder.
fn workspace_above(manifests: &mut impl Manifests, package: &RelativePath) -> RelativePath {
    let mut above = package.clone();
    while !above.is_root() {
        above = folder(&above);
        let Some(manifest) = manifests.manifest(&above.join("Cargo.toml")) else {
            continue;
        };
        if let Some(workspace) = manifest.get("workspace") {
            // Cargo compares the written entries as paths, without expanding
            // globs, in `members` as in `exclude`.
            let lists = |key: &str| {
                workspace
                    .get(key)
                    .and_then(Item::as_array)
                    .is_some_and(|entries| {
                        entries
                            .iter()
                            .filter_map(Value::as_str)
                            .filter_map(|written| resolve(&above, written))
                            .any(|entry| entry.contains(package))
                    })
            };
            if lists("members") || !lists("exclude") {
                return above;
            }
        } else if let Some(root) = own_root(manifest, &above) {
            return root;
        }
    }
    package.clone()
}
