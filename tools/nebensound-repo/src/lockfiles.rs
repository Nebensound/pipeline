//! A manifest whose ecosystem knows a lockfile commits that lockfile.
//!
//! What is checked and built should be what was resolved, and only a lockfile
//! Git tracks says what that was. The register names, per ecosystem, the
//! lockfile that belongs to a manifest and where it stands
//! ([`crate::register::Lockfile`]); phase 0 warns, with
//! [`Rule::MissingLockfile`], for every one of them that Git does not track.
//!
//! A manifest that `[[exclude]]` takes out is passed over, like everywhere
//! else. The others that lead to the same lockfile, as the members of a Cargo
//! workspace lead to the `Cargo.lock` of its root, are reported once, at the
//! one nearest the root of the repository: the root's own manifest when it is
//! among them. Taking out only that one moves the finding to the next, so for
//! a workspace the message asks to take out every manifest that leads to the
//! lockfile.
//!
//! It is a **warning**, as a manifest nothing watches is: a missing lockfile
//! leaves what is resolved to the day of the run, and makes no statement of
//! the declaration untrue. Whether a tracked lockfile is up to date is for the
//! check of each language.

use std::collections::BTreeMap;

use toml_edit::DocumentMut;

use crate::Analysis;
use crate::annotate::Coverage;
use crate::finding::{Finding, Rule};
use crate::ignored::{Read, folder};
use crate::path::RelativePath;
use crate::register::{Register, Stands};
use crate::workspace::{self, Manifests};

/// The manifests read so far, each once, through a reader.
struct Parsed<'a, 'r> {
    read: &'a Read<'r>,
    /// [`None`] for a file that is missing or does not read as TOML.
    documents: BTreeMap<RelativePath, Option<DocumentMut>>,
}

impl Manifests for Parsed<'_, '_> {
    fn manifest(&mut self, path: &RelativePath) -> Option<&DocumentMut> {
        let read = self.read;
        self.documents
            .entry(path.clone())
            .or_insert_with(|| read(path).ok().and_then(|text| text.parse().ok()))
            .as_ref()
    }
}

/// One lockfile Git does not track, and where it is reported.
struct Missing<'a> {
    /// The file name of the manifests that lead to it.
    manifest: &'a str,
    /// Where it stands, seen from them.
    stands: Stands,
    /// The one of them nearest the root of the repository, the first in path
    /// order among those as near.
    at: &'a RelativePath,
}

/// Every lockfile the register gives a manifest in the tree and Git does not
/// track: one warning per lockfile, at the manifest nearest the root of the
/// repository that leads to it, naming the lockfile.
#[must_use]
pub fn findings(analysis: &Analysis, register: &Register, read: &Read<'_>) -> Vec<Finding> {
    let nodes = analysis.tree.files();
    let mut manifests = Parsed {
        read,
        documents: BTreeMap::new(),
    };
    let mut missing: BTreeMap<RelativePath, Missing<'_>> = BTreeMap::new();
    let lockfiles = register
        .ecosystem
        .values()
        .filter_map(|ecosystem| ecosystem.lockfile.as_ref());
    for lockfile in lockfiles {
        for (node, coverage) in &nodes {
            if node.path.file_name() != Some(lockfile.manifest.as_str())
                || matches!(coverage, Coverage::Excluded { .. })
            {
                continue;
            }
            let place = folder(&node.path);
            let place = match lockfile.stands {
                Stands::Manifest => place,
                Stands::CargoWorkspaceRoot => workspace::root_of(&mut manifests, &place),
            };
            let expected = place.join(&lockfile.name);
            if nodes.iter().any(|(other, _)| other.path == expected) {
                continue;
            }
            let found = missing.entry(expected).or_insert(Missing {
                manifest: &lockfile.manifest,
                stands: lockfile.stands,
                at: &node.path,
            });
            if node.path.segments().len() < found.at.segments().len() {
                found.at = &node.path;
            }
        }
    }
    missing
        .into_iter()
        .map(|(expected, missing)| {
            Finding::warning(
                Rule::MissingLockfile,
                format!(
                    "the lockfile of this {manifest} is {expected}, {stands}, and Git does not \
                     track it; what is checked and built is then resolved anew in each run. \
                     Commit {expected}, or take out {which} with an [[exclude]] entry and its \
                     reason",
                    manifest = missing.manifest,
                    stands = missing.stands,
                    which = match missing.stands {
                        Stands::Manifest => "the manifest",
                        // The members of the workspace lead to the same
                        // lockfile: taking out the one reported only moves
                        // the finding to the next.
                        Stands::CargoWorkspaceRoot => {
                            "every manifest that leads to it, such as the folder of the workspace,"
                        }
                    },
                ),
            )
            .at(missing.at.clone())
        })
        .collect()
}
