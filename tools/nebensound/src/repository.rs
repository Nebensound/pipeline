//! Where the tree comes from and where the declaration lies.
//!
//! What a repository holds is decided by **Git**. A directory walk would also
//! count what nobody versions — `target/`, `node_modules/`, yesterday's
//! half-finished file — and phase 0 would judge things the repository does not
//! have. Git also holds the history and the tags every version is read from.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{fs, io};

use nebensound_declaration::central::CentralError;
use nebensound_declaration::history::{Commit, Tag};
use nebensound_declaration::path::{PathError, RelativePath};
use nebensound_declaration::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER};
use nebensound_declaration::{Analysis, Central, Declaration, History, ReadError, Tree, analyse};
use thiserror::Error;

use crate::{ignored, packages};

/// Where the declaration of a repository lies.
pub const DECLARATION: &str = nebensound_declaration::history::DECLARATION;

/// Where the full releases of a unit released in two stages are promoted to.
const RELEASE_BRANCH: &str = "refs/remotes/origin/release";

/// Where every version is given out.
const MAIN_BRANCH: &str = "refs/remotes/origin/main";

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
    /// those the merge would get. A pull request into `release` is merged with
    /// a merge commit and is read as it is.
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
        let mut found = ignored::findings(&analysis, &read);
        found.extend(packages::findings(
            &analysis,
            &declaration_path(),
            &source,
            &read,
        ));
        let loose = analysis.tree.attach(found);
        analysis.loose.extend(loose);
        Ok(analysis)
    }

    /// What Git says about the commit at hand and the releases around it.
    ///
    /// A repository without a commit has an empty history, and a shallow
    /// clone one that says only that it is shallow: what it holds of the
    /// history would give versions that are wrong.
    ///
    /// The line of `main` is the first-parent line of `HEAD`, except at a
    /// merge into `release`: the merge commit GitHub builds for the pull
    /// request from `main`, or the merge itself. Its first parent is a commit
    /// of `release`, and the first-parent line below it holds the merges of
    /// `release` rather than the commits of `main`. There the line is `HEAD`
    /// followed by the first-parent line of the commit of `main` it merges.
    /// Such a merge is told by its parents: the first one on the first-parent
    /// line of `origin/release`, the second one on that of `origin/main`. A
    /// pull request into `main` also merges onto a commit `origin/release` may
    /// stand on, but what it merges is not a commit of `main` yet.
    /// Whether such a merge brings in the tip of `main`, and may promote
    /// anything, is for the library to judge from `origin/main`.
    ///
    /// Every option a setting of the person running it could change is given
    /// on the command line, so that the answer does not depend on their
    /// configuration: rename detection would hide the old path of a moved
    /// file, a root commit not shown as a creation would touch nothing, and a
    /// log output encoding other than UTF-8 would garble every commit.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Git`] when git does not run or does not answer.
    pub fn history(&self) -> Result<History, Error> {
        let head = self.commit("HEAD")?;
        if self.git_output(&["rev-parse", "--is-shallow-repository"])? == b"true\n" {
            return Ok(History {
                shallow: true,
                ..History::default()
            });
        }
        let release = self.commit(RELEASE_BRANCH)?;
        let tags = tags(&self.git_output(&[
            "for-each-ref",
            "--format=%(refname)%00%(objecttype)%00%(objectname)%00%(*objecttype)%00%(*objectname)",
            "refs/tags/",
        ])?);
        let Some(head) = head else {
            return Ok(History {
                release,
                tags,
                ..History::default()
            });
        };
        let main = self.commit(MAIN_BRANCH)?;
        let mut reachable = vec!["rev-list", "--parents", head.as_str()];
        reachable.extend(release.as_deref());
        reachable.extend(main.as_deref());
        let mut parents = parents(&self.git_output(&reachable)?);
        let merged = match (
            parents.get(&head).map(Vec::as_slice),
            release.as_deref(),
            main.as_deref(),
        ) {
            (Some([first, second, ..]), Some(release), Some(main))
                if on_first_parent_line(&parents, release, first)
                    && on_first_parent_line(&parents, main, second) =>
            {
                Some(second.as_str())
            }
            _ => None,
        };
        // Both listings put together read as one: each commit starts with
        // an empty field, and the merge comes first, as `HEAD` does.
        let mut listing = Vec::new();
        if merged.is_some() {
            listing = self.log(&["--max-count=1", head.as_str()])?;
        }
        listing.extend(self.log(&[merged.unwrap_or(head.as_str())])?);
        let into_release = merged.is_some();
        let mut line = line(&listing);
        if let Some(message) = self.squash.as_deref()
            && !into_release
        {
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
        for object in line
            .iter()
            .filter_map(|commit| commit.declaration.as_deref())
        {
            if !declarations.contains_key(object) {
                let text = self.git_output(&["cat-file", "blob", object])?;
                declarations.insert(
                    object.to_owned(),
                    String::from_utf8_lossy(&text).into_owned(),
                );
            }
        }
        Ok(History {
            shallow: false,
            line,
            parents,
            release,
            main,
            tags,
            declarations,
            files,
        })
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

    fn git_failed(&self, message: &str) -> Error {
        Error::Git {
            root: self.root.display().to_string(),
            message: message.trim().to_owned(),
        }
    }
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

/// Whether a commit lies on the first-parent line that starts at another.
fn on_first_parent_line(
    parents: &BTreeMap<String, Vec<String>>,
    start: &str,
    commit: &str,
) -> bool {
    let mut next = Some(start);
    while let Some(current) = next {
        if current == commit {
            return true;
        }
        next = parents
            .get(current)
            .and_then(|parents| parents.first())
            .map(String::as_str);
    }
    false
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
