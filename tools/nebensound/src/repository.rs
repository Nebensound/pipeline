//! Where the tree comes from and where the declaration lies.
//!
//! What a repository holds is decided by **Git**. A directory walk would also
//! count what nobody versions — `target/`, `node_modules/`, yesterday's
//! half-finished file — and phase 0 would judge things the repository does not
//! have.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, io};

use nebensound_declaration::central::CentralError;
use nebensound_declaration::path::{PathError, RelativePath};
use nebensound_declaration::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER};
use nebensound_declaration::{Analysis, Central, Declaration, ReadError, Tree, analyse};
use thiserror::Error;

use crate::ignored;

/// Where the declaration of a repository lies.
pub const DECLARATION: &str = ".nebensound/repo.toml";

/// Why a repository could not be read.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// Git did not name what the repository holds.
    #[error("git does not list what {root} holds: {message}")]
    Git {
        /// The directory this was about.
        root: String,
        /// What git or the operating system said.
        message: String,
    },
    /// Git named something that is not a path inside the repository.
    #[error("git lists {value:?}, and a declaration cannot name that: {source}")]
    Listing {
        /// The rejected entry.
        value: String,
        /// What it failed on.
        source: PathError,
    },
    /// The declaration is missing.
    #[error(
        "{root} has no .nebensound/repo.toml. Every repository in the organization declares \
         itself there; without the file, \"nothing is published here\" cannot be told apart from \
         \"nobody ever set this up\""
    )]
    Missing {
        /// The directory it is missing from.
        root: String,
    },
    /// Git named a path that is not text.
    #[error(
        "git lists {shown:?}, and that is not UTF-8. A declaration can only name what can be \
         written down"
    )]
    NotUtf8 {
        /// The entry, with what could not be read replaced.
        shown: String,
    },
    /// The declaration could not be read.
    #[error("{path} is not readable: {source}")]
    Unreadable {
        /// The file this was about.
        path: String,
        /// What the operating system said.
        source: io::Error,
    },
    /// The declaration does not match the shape.
    #[error("{path}: {source}")]
    Malformed {
        /// The file this was about.
        path: String,
        /// What it failed on.
        source: ReadError,
    },
    /// The central data compiled into this tool does not read.
    #[error("the built-in register or the built-in defaults do not read: {source}")]
    Central {
        /// What they failed on.
        source: CentralError,
    },
}

/// The vocabulary and the defaults as they are compiled into this tool.
///
/// # Errors
///
/// Returns [`Error::Central`] when either of them does not read, which only
/// happens when the library was built with broken data.
pub fn central() -> Result<Central, Error> {
    central_from(BUILTIN_REGISTER, BUILTIN_DEFAULTS)
}

/// The same from given sources.
///
/// There is no switch for it: the central data is compiled in so that no
/// repository holds a copy that can fall behind. This form stands beside it so
/// that what the tool says when it does not read is a checked message.
///
/// # Errors
///
/// Returns [`Error::Central`] when the register or the defaults do not read.
pub fn central_from(register: &str, defaults: &str) -> Result<Central, Error> {
    Central::parse(register, defaults).map_err(|source| Error::Central { source })
}

/// A repository on disk.
#[derive(Debug, Clone)]
pub struct Repository {
    root: PathBuf,
    git: OsString,
}

impl Repository {
    /// The repository in this directory.
    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            git: OsString::from("git"),
        }
    }

    /// With another git program.
    ///
    /// Only here so that "git is not installed" has a checked message rather
    /// than an assumed one.
    #[must_use]
    pub fn with_git(mut self, program: impl AsRef<OsStr>) -> Self {
        program.as_ref().clone_into(&mut self.git);
        self
    }

    /// Where it lies.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What the repository holds, according to Git.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Git`] when git does not run or the directory is not a
    /// repository, and [`Error::Listing`] when an entry it names is not a path
    /// inside the repository.
    pub fn tree(&self) -> Result<Tree, Error> {
        // -z: git quotes unusual names otherwise, and the quotes would be read
        // as part of the path.
        let listing = Command::new(&self.git)
            .arg("-C")
            .arg(&self.root)
            .args(["ls-files", "-z"])
            .output();
        let output = match listing {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                return Err(self.git_failed(&String::from_utf8_lossy(&output.stderr)));
            }
            Err(error) => return Err(self.git_failed(&error.to_string())),
        };
        self.tree_of(&output.stdout)
    }

    /// The same tree from a listing that is already at hand, entries separated
    /// by a zero byte.
    ///
    /// Raw bytes, not text: git hands out the path as it stands on disk, and
    /// replacing what is not UTF-8 would rename a file quietly — or make two
    /// different names collapse into one.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotUtf8`] when an entry is not text, and
    /// [`Error::Listing`] when it is not a path inside the repository.
    pub fn tree_of(&self, listing: &[u8]) -> Result<Tree, Error> {
        let mut files = Vec::new();
        for entry in listing.split(|byte| *byte == 0).filter(|e| !e.is_empty()) {
            let text = std::str::from_utf8(entry).map_err(|_| Error::NotUtf8 {
                shown: String::from_utf8_lossy(entry).into_owned(),
            })?;
            let path = RelativePath::new(text).map_err(|source| Error::Listing {
                value: text.to_owned(),
                source,
            })?;
            files.push(path);
        }
        Ok(Tree::new(files))
    }

    /// The declaration as it is written.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Missing`] when there is none, [`Error::Unreadable`]
    /// when it cannot be read, and [`Error::Malformed`] when it does not match
    /// the shape.
    pub fn declaration(&self) -> Result<Declaration, Error> {
        let source = match self.read(DECLARATION) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(Error::Missing {
                    root: self.root.display().to_string(),
                });
            }
            Err(source) => {
                return Err(Error::Unreadable {
                    path: DECLARATION.to_owned(),
                    source,
                });
            }
        };
        Declaration::parse(&source).map_err(|source| Error::Malformed {
            path: DECLARATION.to_owned(),
            source,
        })
    }

    /// A file as it stands in the repository, and only that: the declaration,
    /// or a tool's setting that phase 0 holds against it.
    ///
    /// `symlink_metadata` asks about the entry itself rather than what it may
    /// point at. A pull request could otherwise aim the file outside the
    /// checkout, or at a device that never ends.
    fn read(&self, relative: &str) -> io::Result<String> {
        let path = self.root.join(relative);
        if !fs::symlink_metadata(&path)?.is_file() {
            return Err(io::Error::other(
                "it is not a regular file. It is read as it stands in the repository; a link or \
                 a device could point out of it, or never end",
            ));
        }
        fs::read_to_string(&path)
    }

    /// Phase 0 on this repository.
    ///
    /// # Errors
    ///
    /// Returns what reading failed at. What is wrong with the declaration is
    /// not an error here but a finding in the [`Analysis`].
    ///
    /// Beside what the library finds, it holds the tools' own settings against
    /// the declaration ([`crate::ignored`]): reading them is input, which the
    /// library has none of.
    pub fn analyse(&self, central: &Central) -> Result<Analysis, Error> {
        let declaration = self.declaration()?;
        let tree = self.tree()?;
        let mut analysis = analyse(&declaration, central, &tree);
        let read = |path: &RelativePath| self.read(&path.to_string()).map_err(|e| e.to_string());
        let found = ignored::findings(&analysis, &read);
        let loose = analysis.tree.attach(found);
        analysis.loose.extend(loose);
        Ok(analysis)
    }

    fn git_failed(&self, message: &str) -> Error {
        Error::Git {
            root: self.root.display().to_string(),
            message: message.trim().to_owned(),
        }
    }
}
