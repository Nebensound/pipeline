//! The files a repository holds, as the crate is told about them.
//!
//! Only files are listed. A folder exists exactly when something lies below
//! it, which is also how Git sees it.

use std::collections::BTreeSet;

use crate::path::RelativePath;

/// The paths a repository holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tree {
    files: BTreeSet<RelativePath>,
    /// The files known to be empty. What is not named here may hold content,
    /// so a tree that is told nothing about content treats every file as
    /// holding some.
    empty: BTreeSet<RelativePath>,
}

impl Tree {
    /// Builds a tree from the files a repository holds.
    #[must_use]
    pub fn new(files: impl IntoIterator<Item = RelativePath>) -> Self {
        Self {
            files: files.into_iter().filter(|f| !f.is_root()).collect(),
            empty: BTreeSet::new(),
        }
    }

    /// The same tree, told which of its files are empty. A path it does not
    /// hold as a file is passed over.
    #[must_use]
    pub fn with_empty(mut self, empty: impl IntoIterator<Item = RelativePath>) -> Self {
        self.empty = empty
            .into_iter()
            .filter(|file| self.files.contains(file))
            .collect();
        self
    }

    /// Whether this is a file of the tree that is known to be empty.
    #[must_use]
    pub fn is_empty_file(&self, path: &RelativePath) -> bool {
        self.empty.contains(path)
    }

    /// Every file, in path order.
    pub fn files(&self) -> impl Iterator<Item = &RelativePath> {
        self.files.iter()
    }

    /// Whether anything exists at this path: the file itself, or something
    /// below it. The repository itself exists whenever it holds anything.
    #[must_use]
    pub fn holds(&self, path: &RelativePath) -> bool {
        if path.is_root() {
            return !self.files.is_empty();
        }
        self.files.iter().any(|file| path.contains(file))
    }

    /// The files directly below a path.
    #[must_use]
    pub fn child_files(&self, path: &RelativePath) -> Vec<RelativePath> {
        self.files
            .iter()
            .filter(|file| path.is_parent_of(file))
            .cloned()
            .collect()
    }

    /// The folders directly below a path, in name order.
    #[must_use]
    pub fn child_folders(&self, path: &RelativePath) -> Vec<RelativePath> {
        let depth = path.segments().len();
        self.files
            .iter()
            .filter(|file| path.contains(file) && file.segments().len() > depth + 1)
            .filter_map(|file| file.segments().get(depth).map(|name| path.join(name)))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}
