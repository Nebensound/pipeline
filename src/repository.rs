//! Where the tree comes from and where the declaration lies.
//!
//! What a repository holds is decided by **Git**. A directory walk would also
//! count what nobody versions — `target/`, `node_modules/`, yesterday's
//! half-finished file — and phase 0 would judge things the repository does not
//! have. Git also holds the history every version is read from, and the tags
//! that record the releases.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{fs, io};

use crate::central::CentralError;
use crate::conformance::{Identity, Told, branch_from_head, name_from_address};
use crate::history::{Commit, Tag};
use crate::path::{PathError, RelativePath};
use crate::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER};
use crate::{Analysis, Central, Declaration, History, ReadError, Tree, analyse};
use thiserror::Error;

use crate::{ignored, linter_config, lockfiles, packages, release_fields};

/// Where the declaration of a repository lies.
pub const DECLARATION: &str = crate::history::DECLARATION;

/// The object an empty file holds, under each hash Git can use: SHA-1 and
/// SHA-256. Fixed by the empty content, so a repository of either kind names
/// it the same way.
const EMPTY_BLOBS: [&[u8]; 2] = [
    b"e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
    b"473a0f4c3be8a93681a267e3b1e9a7dcda1185436fe141f7749120a303721813",
];

/// [`DECLARATION`] as a path in the repository.
fn declaration_path() -> RelativePath {
    RelativePath::root().join(".nebensound").join("repo.toml")
}

/// Reads the declaration from its text.
fn parse(source: &str) -> Result<Declaration, Error> {
    Declaration::parse(source).map_err(|source| Error::Malformed {
        path: DECLARATION.to_owned(),
        source,
    })
}

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
    /// The message of the squash merge `HEAD` stands for, when it is read as
    /// a pull request.
    squash: Option<String>,
}

impl Repository {
    /// The repository in this directory.
    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            git: OsString::from("git"),
            squash: None,
        }
    }

    /// Read as a pull request: `HEAD`, the merge commit GitHub builds for it,
    /// stands for the commit its squash merge would put on the base branch.
    ///
    /// That commit has one parent, the first parent of `HEAD`, the changes of
    /// `HEAD` against it, and this message: the title and the description of
    /// the pull request, which the squash merge takes. The versions are then
    /// those the merge would get.
    #[must_use]
    pub fn as_pull_request(mut self, message: impl Into<String>) -> Self {
        self.squash = Some(message.into());
        self
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

    /// The commit at hand, `HEAD`, by its full hash.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Git`] when git does not run or names no commit.
    pub fn head(&self) -> Result<String, Error> {
        let printed = self.git_output(&["rev-parse", "--verify", "HEAD^{commit}"])?;
        Ok(String::from_utf8_lossy(&printed).trim().to_owned())
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
        // as part of the path. --stage: each entry names the object it holds,
        // which tells an empty file without reading the working tree.
        let listing = self.git_output(&["ls-files", "-z", "--stage"])?;
        // An entry added with `git add -N` is listed with the empty blob,
        // whatever the file holds. Against the index, only such an entry shows
        // up as added. --no-renames: it must not pair up with a deleted file.
        // --relative: the paths as ls-files prints them, below the root.
        let intended = self.git_output(&[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--relative",
            "--diff-filter=A",
        ])?;
        self.tree_of(&listing, &intended)
    }

    /// The same tree from listings that are already at hand. `listing` is what
    /// `git ls-files -z --stage` prints: `<mode> <object> <stage>`, a tab and
    /// the path, entries separated by a zero byte. `intended` is what
    /// `git diff --name-only -z --diff-filter=A` prints: the entries added
    /// with `git add -N`, separated by a zero byte.
    ///
    /// Raw bytes, not text: git hands out the path as it stands on disk, and
    /// replacing what is not UTF-8 would rename a file quietly — or make two
    /// different names collapse into one.
    ///
    /// A file is empty when it holds the empty blob. A file in conflict is
    /// listed once for each side, and counts as empty only when every side is.
    /// An entry added with `git add -N` holds the empty blob only as a
    /// placeholder, so it never counts as empty.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Git`] when an entry does not carry its object,
    /// [`Error::NotUtf8`] when a path is not text, and [`Error::Listing`] when
    /// it is not a path inside the repository.
    pub fn tree_of(&self, listing: &[u8], intended: &[u8]) -> Result<Tree, Error> {
        let intended: BTreeSet<&[u8]> = intended.split(|byte| *byte == 0).collect();
        let mut files: BTreeMap<RelativePath, bool> = BTreeMap::new();
        for entry in listing.split(|byte| *byte == 0).filter(|e| !e.is_empty()) {
            let unstaged = || Error::Git {
                root: self.root.display().to_string(),
                message: format!(
                    "{:?} is not an entry of `git ls-files --stage`",
                    String::from_utf8_lossy(entry)
                ),
            };
            let tab = entry
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or_else(unstaged)?;
            let (stage, name) = (&entry[..tab], &entry[tab + 1..]);
            let object = stage
                .split(|byte| *byte == b' ')
                .nth(1)
                .ok_or_else(unstaged)?;
            let text = std::str::from_utf8(name).map_err(|_| Error::NotUtf8 {
                shown: String::from_utf8_lossy(name).into_owned(),
            })?;
            let path = RelativePath::new(text).map_err(|source| Error::Listing {
                value: text.to_owned(),
                source,
            })?;
            *files.entry(path).or_insert(true) &=
                EMPTY_BLOBS.contains(&object) && !intended.contains(name);
        }
        let empty: Vec<RelativePath> = files
            .iter()
            .filter(|(_, empty)| **empty)
            .map(|(path, _)| path.clone())
            .collect();
        Ok(Tree::new(files.into_keys()).with_empty(empty))
    }

    /// The declaration as it is written.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Missing`] when there is none, [`Error::Unreadable`]
    /// when it cannot be read, and [`Error::Malformed`] when it does not match
    /// the shape.
    pub fn declaration(&self) -> Result<Declaration, Error> {
        parse(&self.declaration_source()?)
    }

    /// The declaration as text, before it is read.
    fn declaration_source(&self) -> Result<String, Error> {
        match self.read(DECLARATION) {
            Ok(source) => Ok(source),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Err(Error::Missing {
                root: self.root.display().to_string(),
            }),
            Err(source) => Err(Error::Unreadable {
                path: DECLARATION.to_owned(),
                source,
            }),
        }
    }

    /// A file as it stands in the repository, and only that: the declaration,
    /// a tool's setting that phase 0 holds against it, or a manifest a version
    /// writer changes.
    ///
    /// `symlink_metadata` asks about each entry on the way itself rather than
    /// what it may point at, every folder as well as the file. The pull
    /// request chooses the paths of a setting (`ignore-from-file`, `extends`),
    /// and a link, of the file or of a folder before it, could otherwise aim
    /// the read outside the checkout, or at a device that never ends.
    pub(crate) fn read(&self, relative: &str) -> io::Result<String> {
        fs::read_to_string(self.checked(relative)?)
    }

    /// A file as it stands in the repository, as bytes, with the same care as
    /// [`Self::read`]: for a reader that decides itself whether the file is
    /// text, so that it reads the file once.
    pub(crate) fn read_bytes(&self, relative: &str) -> io::Result<Vec<u8>> {
        fs::read(self.checked(relative)?)
    }

    /// Writes a file of the repository back, with the same care as
    /// [`Self::read`]: the version writers change manifests a pull request
    /// chose, and a link among them could otherwise aim the write outside the
    /// checkout.
    pub(crate) fn write(&self, relative: &str, text: &str) -> io::Result<()> {
        self.checked(relative)
            .and_then(|path| fs::write(path, text))
    }

    /// Where a file of the repository lies, once every entry on the way is
    /// known to be no link and the file itself a regular one.
    fn checked(&self, relative: &str) -> io::Result<PathBuf> {
        let mut path = self.root.clone();
        let mut regular = false;
        for segment in relative.split('/') {
            path.push(segment);
            let entry = fs::symlink_metadata(&path)?;
            if entry.is_symlink() {
                return Err(io::Error::other(
                    "it lies behind a link, or is one. It is read as it stands in the \
                     repository; a link could point out of it",
                ));
            }
            regular = entry.is_file();
        }
        if !regular {
            return Err(io::Error::other(
                "it is not a regular file. It is read as it stands in the repository; a \
                 device could never end",
            ));
        }
        Ok(path)
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
    /// library has none of. The version of every unit is resolved from the
    /// history ([`Self::history`]).
    pub fn analyse(&self, central: &Central) -> Result<Analysis, Error> {
        let analysis = self.analyse_files(central)?;
        let history = self.history()?;
        Ok(analysis.with_history(&history, central))
    }

    /// Phase 0 on this repository without the history: everything
    /// [`Self::analyse`] finds about the declaration and the files, and no
    /// version.
    ///
    /// The building blocks ask for their file lists in a job of their own,
    /// whose checkout holds the commit at hand and nothing before it. The
    /// versions were resolved on the whole history in Declare, and no file
    /// list depends on them.
    ///
    /// # Errors
    ///
    /// Returns what reading failed at, as [`Self::analyse`] does.
    pub fn analyse_files(&self, central: &Central) -> Result<Analysis, Error> {
        let source = self.declaration_source()?;
        let declaration = parse(&source)?;
        let tree = self.tree()?;
        let mut analysis = analyse(&declaration, central, &tree);
        let read = self.reader();
        let mut found = ignored::findings(&analysis, central.register(), &read);
        found.extend(lockfiles::findings(&analysis, central.register(), &read));
        found.extend(linter_config::findings(
            &analysis,
            central.register(),
            &read,
        ));
        found.extend(packages::findings(
            &analysis,
            &declaration_path(),
            &source,
            &read,
        ));
        found.extend(release_fields::findings(
            &analysis,
            &declaration_path(),
            &source,
        ));
        let loose = analysis.tree.attach(found);
        analysis.loose.extend(loose);
        Ok(analysis)
    }

    /// What Git says about the commit at hand and the history around it.
    ///
    /// A repository without a commit has an empty history, and a shallow
    /// clone one that says only that it is shallow: what it holds of the
    /// history would give versions that are wrong.
    ///
    /// The line of `main` is the first-parent line of `HEAD`.
    ///
    /// Every option a setting of the person running it could change is given
    /// on the command line, so that the answer does not depend on their
    /// configuration: rename detection would hide the old path of a moved
    /// file, a root commit not shown as a creation would touch nothing, and a
    /// log output encoding other than UTF-8 would garble every commit.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Git`] when git does not run or does not answer, and
    /// [`Error::Malformed`] when the declaration of a commit of the line is
    /// written for an edition this tool does not know.
    pub fn history(&self) -> Result<History, Error> {
        let head = self.commit("HEAD")?;
        if self.git_output(&["rev-parse", "--is-shallow-repository"])? == b"true\n" {
            return Ok(History {
                shallow: true,
                ..History::default()
            });
        }
        let tags = tags(&self.git_output(&["for-each-ref", TAG_FORMAT, "refs/tags/"])?);
        let Some(head) = head else {
            return Ok(History {
                tags,
                ..History::default()
            });
        };
        let mut parents = parents(&self.git_output(&["rev-list", "--parents", head.as_str()])?);
        let mut line = line(&self.log(&[head.as_str()])?);
        if let Some(message) = self.squash.as_deref() {
            squash(&mut line, &mut parents, message);
        }
        let files = String::from_utf8_lossy(&self.git_output(&[
            "ls-tree",
            "-r",
            "-z",
            "--name-only",
            "--full-tree",
            head.as_str(),
        ])?)
        .split('\0')
        .filter(|file| !file.is_empty())
        .map(ToOwned::to_owned)
        .collect();
        let mut declarations = BTreeMap::new();
        // Oldest first, so that an edition this tool does not know is named
        // at the commit that switched it on.
        for commit in line.iter().rev() {
            let Some(object) = commit.declaration.as_deref() else {
                continue;
            };
            if !declarations.contains_key(object) {
                let text =
                    String::from_utf8_lossy(&self.git_output(&["cat-file", "blob", object])?)
                        .into_owned();
                // The edition says how the commit is read, so one this tool
                // does not know stops the reading wherever it stands on the
                // line, not only at `HEAD`. Anything else a declaration of
                // the history fails on is for the library to fall back from.
                if let Err(source @ ReadError::UnknownEdition { .. }) =
                    Declaration::read_edition(&text)
                {
                    return Err(Error::Malformed {
                        path: format!(
                            "{DECLARATION} at commit {}",
                            commit.id.chars().take(12).collect::<String>()
                        ),
                        source,
                    });
                }
                declarations.insert(object.to_owned(), text);
            }
        }
        Ok(History {
            shallow: false,
            line,
            parents,
            tags,
            declarations,
            files,
        })
    }

    /// The records around the commit at hand, as the release stage reads
    /// them: every tag with the commit it stands on, and which of them stand
    /// on a commit that contains `HEAD` or that `HEAD` contains.
    ///
    /// It reads no version: which version a unit gets is the history's
    /// answer ([`Self::history`]). The tags only tell what was written down
    /// already.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Git`] when git does not run or does not answer, and
    /// when `HEAD` names no commit: then there is nothing a record could
    /// stand on.
    pub fn records(&self) -> Result<Records, Error> {
        let head = self
            .commit("HEAD")?
            .ok_or_else(|| self.git_failed("HEAD names no commit"))?;
        let tags = tags(&self.git_output(&["for-each-ref", TAG_FORMAT, "refs/tags/"])?);
        let names = |relation: &str| {
            self.git_output(&[
                "for-each-ref",
                "--format=%(refname)",
                relation,
                head.as_str(),
                "refs/tags/",
            ])
            .map(|listing| tags_named(&listing))
        };
        let holding_head = names("--contains")?;
        let held_by_head = names("--merged")?;
        Ok(Records {
            head,
            tags,
            holding_head,
            held_by_head,
        })
    }

    /// Every tag that stands on a commit `HEAD` contains, a tag on `HEAD`
    /// itself among them: the records a repository made before it called the
    /// pipeline, as `init` reads them. None in a repository without a commit.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Git`] when git does not run or does not answer.
    pub fn tags_held_by_head(&self) -> Result<Vec<Tag>, Error> {
        let Some(head) = self.commit("HEAD")? else {
            return Ok(Vec::new());
        };
        Ok(tags(&self.git_output(&[
            "for-each-ref",
            TAG_FORMAT,
            "--merged",
            head.as_str(),
            "refs/tags/",
        ])?))
    }

    /// The repository's name and default branch: as given, and otherwise as
    /// Git knows them.
    ///
    /// Neither is declared, since GitHub holds both. A run hands them in from
    /// its event. On a developer's machine Git knows the name from the address
    /// of `origin`, and the default branch from `refs/remotes/origin/HEAD`,
    /// which a clone sets. Where Git does not know one, it is
    /// [`Told::Unknown`] with the reason, and nothing is guessed.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Git`] when git does not run or does not answer.
    pub fn identity(
        &self,
        name: Option<&str>,
        default_branch: Option<&str>,
    ) -> Result<Identity, Error> {
        let name = match name {
            Some(name) => Told::Known(name.to_owned()),
            None => match self.answer(&["config", "--get", "remote.origin.url"])? {
                None => Told::Unknown(
                    "Git knows no address of origin, and the call names none with --name"
                        .to_owned(),
                ),
                Some(address) => name_from_address(&address).map_or_else(
                    || {
                        Told::Unknown(format!(
                            "the address of origin, {:?}, ends without a name",
                            address.trim()
                        ))
                    },
                    Told::Known,
                ),
            },
        };
        let default_branch = match default_branch {
            Some(branch) => Told::Known(branch.to_owned()),
            None => match self.answer(&["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"])? {
                None => Told::Unknown(
                    "Git does not know which branch origin/HEAD names (`git remote set-head \
                     origin --auto` tells it), and the call names none with --default-branch"
                        .to_owned(),
                ),
                Some(target) => branch_from_head(&target).map_or_else(
                    || {
                        Told::Unknown(format!(
                            "origin/HEAD names {:?}, which is no branch of origin",
                            target.trim()
                        ))
                    },
                    Told::Known,
                ),
            },
        };
        Ok(Identity {
            name,
            default_branch,
        })
    }

    /// What git prints for a question it may have no answer to: [`None`] when
    /// it exits with 1, which `config --get` and `symbolic-ref --quiet` use
    /// for "there is none", and only for that.
    fn answer(&self, args: &[&str]) -> Result<Option<String>, Error> {
        let output = self.git(args)?;
        match output.status.code() {
            Some(0) => Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned())),
            Some(1) => Ok(None),
            _ => Err(self.git_failed(&String::from_utf8_lossy(&output.stderr))),
        }
    }

    /// The first-parent line from a commit on, as [`line`] reads it.
    fn log(&self, start: &[&str]) -> Result<Vec<u8>, Error> {
        let mut args = vec![
            "-c",
            "log.showRoot=true",
            "log",
            "--first-parent",
            "--diff-merges=first-parent",
            "-z",
            "--raw",
            "--no-abbrev",
            "--no-renames",
            "--no-relative",
            "--ignore-submodules=none",
            "--no-color",
            "--no-notes",
            "--no-show-signature",
            "--encoding=UTF-8",
            "--format=%x00%H%x00%P%x00%B",
        ];
        args.extend(start);
        self.git_output(&args)
    }

    /// The commit a reference names, or [`None`] when it names none.
    fn commit(&self, reference: &str) -> Result<Option<String>, Error> {
        let output = self.git(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{reference}^{{commit}}"),
        ])?;
        match output.status.code() {
            Some(0) => Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            )),
            // `--verify --quiet` says "there is no such commit" this way, and
            // only this way.
            Some(1) => Ok(None),
            _ => Err(self.git_failed(&String::from_utf8_lossy(&output.stderr))),
        }
    }

    /// What git prints, when it succeeds.
    fn git_output(&self, args: &[&str]) -> Result<Vec<u8>, Error> {
        let output = self.git(args)?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(self.git_failed(&String::from_utf8_lossy(&output.stderr)))
        }
    }

    /// One call of git in the repository.
    fn git(&self, args: &[&str]) -> Result<Output, Error> {
        Command::new(&self.git)
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(|error| self.git_failed(&error.to_string()))
    }

    /// The Rust packages `[[exclude]]` takes out as a whole
    /// ([`crate::packages`]), in this repository as phase 0 analysed it.
    #[must_use]
    pub fn excluded_packages(&self, analysis: &Analysis) -> Vec<packages::Excluded> {
        packages::excluded(analysis, &self.reader())
    }

    /// [`Self::read`] as the readers of the tools' settings and the
    /// manifests take it.
    pub(crate) fn reader(&self) -> impl Fn(&RelativePath) -> Result<String, String> + '_ {
        |path: &RelativePath| self.read(&path.to_string()).map_err(|e| e.to_string())
    }

    /// [`Self::read_bytes`] as the search for markers of open tasks takes it
    /// ([`crate::marker`]).
    pub(crate) fn byte_reader(&self) -> impl Fn(&RelativePath) -> Result<Vec<u8>, String> + '_ {
        |path: &RelativePath| {
            self.read_bytes(&path.to_string())
                .map_err(|e| e.to_string())
        }
    }

    fn git_failed(&self, message: &str) -> Error {
        Error::Git {
            root: self.root.display().to_string(),
            message: message.trim().to_owned(),
        }
    }
}

/// What `for-each-ref` prints of a tag for [`tags`]: its name, what it points
/// at, and what an annotated tag points on to.
const TAG_FORMAT: &str =
    "--format=%(refname)%00%(objecttype)%00%(objectname)%00%(*objecttype)%00%(*objectname)";

/// The records of a repository around the commit at hand
/// ([`Repository::records`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Records {
    /// The commit at hand, `HEAD`, by its full hash.
    pub head: String,
    /// Every tag of the repository.
    pub tags: Vec<Tag>,
    /// The names of the tags that stand on a commit containing `HEAD`, a tag
    /// on `HEAD` itself among them.
    pub holding_head: BTreeSet<String>,
    /// The names of the tags that stand on a commit `HEAD` contains, a tag
    /// on `HEAD` itself among them.
    pub held_by_head: BTreeSet<String>,
}

/// The names of the tags `for-each-ref --format=%(refname)` prints, one per
/// line, without `refs/tags/`.
fn tags_named(listing: &[u8]) -> BTreeSet<String> {
    String::from_utf8_lossy(listing)
        .lines()
        .map(|line| line.strip_prefix("refs/tags/").unwrap_or(line).to_owned())
        .collect()
}

/// The tags from `for-each-ref`, one per line, its fields separated by a zero
/// byte. A reference name holds no control character, so the lines cannot be
/// confused.
fn tags(listing: &[u8]) -> Vec<Tag> {
    String::from_utf8_lossy(listing)
        .lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('\0').collect();
            let field = |index: usize| fields.get(index).copied().unwrap_or_default();
            // An annotated tag points at a tag object, which points on.
            let commit = match (field(1), field(3)) {
                ("commit", _) => Some(field(2).to_owned()),
                ("tag", "commit") => Some(field(4).to_owned()),
                _ => None,
            };
            let name = field(0);
            Tag {
                name: name.strip_prefix("refs/tags/").unwrap_or(name).to_owned(),
                commit,
            }
        })
        .collect()
}

/// The parents of every commit, from `rev-list --parents`: one commit per
/// line, followed by its parents.
fn parents(listing: &[u8]) -> BTreeMap<String, Vec<String>> {
    String::from_utf8_lossy(listing)
        .lines()
        .map(|line| {
            let mut ids = line.split(' ').map(ToOwned::to_owned);
            (ids.next().unwrap_or_default(), ids.collect())
        })
        .collect()
}

/// Makes the first commit of the line the squash merge it stands for: one
/// parent, its first, and the message given. Its changes are already those
/// against its first parent. What only its other parents bring in is no longer
/// contained in it, as it will not be in the commit the merge puts on the base
/// branch, so a tag there is passed over.
fn squash(line: &mut [Commit], parents: &mut BTreeMap<String, Vec<String>>, message: &str) {
    // The line starts at `HEAD`, which exists here, and `rev-list` named its
    // parents; nothing is left to decide.
    for head in line.iter_mut().take(1) {
        head.parents.truncate(1);
        message.clone_into(&mut head.message);
        parents
            .entry(head.id.clone())
            .and_modify(|own| own.truncate(1));
    }
}

/// The first-parent line from `log --raw`.
///
/// Every commit starts with an empty field, then its hash, its parents and its
/// message. Then each file it changed follows in two fields: what changed —
/// both modes, both objects and a letter — the first one behind a newline, and
/// then the path. A path is never empty and a message holds no zero byte, so
/// an empty field after the message can only start the next commit.
///
/// Which object holds the declaration at a commit follows from the line: the
/// object a commit changes it to, otherwise what its first parent holds.
fn line(listing: &[u8]) -> Vec<Commit> {
    let text = String::from_utf8_lossy(listing);
    let mut fields = text.split('\0').peekable();
    let mut commits = Vec::new();
    // Per commit, what it did to the declaration: set it to an object, or
    // removed it.
    let mut declared: Vec<Option<Option<String>>> = Vec::new();
    while fields.next().is_some() {
        let (Some(id), Some(parents), Some(message)) =
            (fields.next(), fields.next(), fields.next())
        else {
            break;
        };
        let mut paths = Vec::new();
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut declaration = None;
        while let Some(change) = fields.next_if(|field| !field.is_empty()) {
            let path = fields.next().unwrap_or_default();
            let words: Vec<&str> = change.split(' ').collect();
            if path == DECLARATION {
                // The fourth word is the object after the change; a removed
                // file has none, written as zeros.
                let object = words.get(3).copied().unwrap_or_default();
                declaration = Some(
                    Some(object.to_owned())
                        .filter(|object| object.bytes().any(|byte| byte != b'0')),
                );
            }
            // The fifth word is the letter: renames are off, so a file comes
            // or goes only as `A` or `D`.
            match words.get(4).copied() {
                Some("A") => added.push(path.to_owned()),
                Some("D") => removed.push(path.to_owned()),
                _ => {}
            }
            paths.push(path.to_owned());
        }
        declared.push(declaration);
        commits.push(Commit {
            id: id.to_owned(),
            parents: parents.split_whitespace().map(ToOwned::to_owned).collect(),
            message: message.to_owned(),
            paths,
            added,
            removed,
            declaration: None,
        });
    }
    // Oldest first: the first commit of the line is a creation, so what it
    // holds is what it adds.
    let mut current = None;
    for (commit, change) in commits.iter_mut().zip(declared).rev() {
        if let Some(change) = change {
            current = change;
        }
        commit.declaration.clone_from(&current);
    }
    commits
}
