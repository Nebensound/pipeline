//! A repository on disk, the way Git sees it.
//!
//! No test case here — only what the others need. As a subdirectory, so that
//! Cargo does not build a test binary of its own from it.

#![allow(clippy::expect_used, dead_code)]

use std::path::Path;
use std::process::Command;
use std::{fs, io};

use nebensound::output::Output;
use nebensound_declaration::path::RelativePath;
use nebensound_declaration::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER};
use nebensound_declaration::{Analysis, Central, Declaration, Tree, analyse};
use tempfile::TempDir;

/// Creates a Git repository holding these files.
///
/// `git add` without a commit is enough: `git ls-files` reads the index, and a
/// commit would need an identity a fixture should not have.
pub fn repository(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("a directory");
    write_all(dir.path(), files);
    git(dir.path(), &["init", "--quiet"]);
    git(dir.path(), &["add", "--all"]);
    dir
}

/// The same files without Git.
pub fn directory(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("a directory");
    write_all(dir.path(), files);
    dir
}

fn write_all(root: &Path, files: &[(&str, &str)]) {
    for (path, content) in files {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("a folder in the fixture");
        }
        fs::write(&full, content).expect("a file in the fixture");
    }
}

/// Runs git in the fixture, for what [`repository`] does not set up.
pub fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git is installed");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The smallest declaration that passes.
pub const QUIET: &str = "[repo]\npipeline = \"none\"\n";

/// One call of the tool, inside this process.
///
/// Always with [`Output::Elsewhere`]. Under `cargo test` the standard output is
/// the terminal the run was started from, so asking would be answered
/// truthfully — and a check of the `ui` command would take that terminal over.
pub fn call(args: &[&str]) -> (String, u8) {
    let mut out = Vec::new();
    let code = nebensound::run_against(
        args.iter().copied(),
        BUILTIN_REGISTER,
        BUILTIN_DEFAULTS,
        Output::Elsewhere,
        &mut out,
    );
    (String::from_utf8(out).expect("the output is UTF-8"), code)
}

/// A sink that never writes.
pub struct Broken;

impl io::Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("nothing goes here"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The central data as it is compiled into the tool.
pub fn central() -> Central {
    nebensound::repository::central().expect("what is compiled into the tool")
}

/// An analysis without a disk: a declaration and a list of paths.
pub fn analysis(source: &str, paths: &[&str]) -> Analysis {
    let declaration = Declaration::parse(source).expect("a declaration in the fixture");
    let tree = Tree::new(
        paths
            .iter()
            .map(|path| RelativePath::new(path).expect("a path in the fixture")),
    );
    analyse(&declaration, &central(), &tree)
}

/// A Git repository with a history, built commit by commit.
///
/// Every call of git here runs without the configuration of whoever runs the
/// tests: no global or system file, a fixed identity and fixed dates, signing
/// off and `main` as the first branch. A fixture that read the developer's
/// configuration would test their machine, not the tool. The tool itself runs
/// git with whatever configuration there is, as it does in a real run.
pub struct Git {
    dir: TempDir,
}

impl Git {
    /// An empty repository on `main`.
    pub fn new() -> Self {
        let git = Self {
            dir: tempfile::tempdir().expect("a directory"),
        };
        git.run(&["init", "--quiet", "--initial-branch=main"]);
        git
    }

    /// Where it lies.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Where it lies, as the tool takes it on its command line.
    pub fn root(&self) -> &str {
        self.path().to_str().expect("a path in UTF-8")
    }

    /// Writes files into the working tree.
    pub fn write(&self, files: &[(&str, &str)]) -> &Self {
        write_all(self.path(), files);
        self
    }

    /// Removes a file from the working tree.
    pub fn remove(&self, path: &str) -> &Self {
        fs::remove_file(self.path().join(path)).expect("a file in the fixture");
        self
    }

    /// Commits everything in the working tree and returns the commit.
    pub fn commit(&self, message: &str) -> String {
        self.run(&["add", "--all"]);
        self.run(&["commit", "--quiet", "--allow-empty", "--message", message]);
        self.head()
    }

    /// The commit `HEAD` names.
    pub fn head(&self) -> String {
        self.run(&["rev-parse", "HEAD"]).trim().to_owned()
    }

    /// A lightweight tag on a commit.
    pub fn tag(&self, name: &str, commit: &str) {
        self.run(&["tag", name, commit]);
    }

    /// Promotes the tip of `main` into `release` the way the pull request from
    /// `main` does, with a merge commit, and makes `origin/release` point at
    /// it. The branch starts at the first commit of `main` when it does not
    /// exist yet. Returns the promoted commit.
    pub fn promote(&self) -> String {
        let promoted = self.head();
        if self.run(&["branch", "--list", "release"]).is_empty() {
            let first = self.run(&["rev-list", "--max-parents=0", "HEAD"]);
            self.run(&["branch", "release", first.trim()]);
        }
        self.run(&["checkout", "--quiet", "release"]);
        self.run(&[
            "merge",
            "--quiet",
            "--no-ff",
            "--message",
            "Promote",
            "main",
        ]);
        self.run(&["checkout", "--quiet", "main"]);
        self.run(&["update-ref", "refs/remotes/origin/release", "release"]);
        promoted
    }

    /// Checks out the merge commit GitHub builds for a pull request from
    /// `main` into `release`: `release` merged with the tip of `main`, `HEAD`
    /// detached on it, and `origin/main` where `main` is. Returns the merge.
    pub fn merge_into_release(&self) -> String {
        self.run(&["update-ref", "refs/remotes/origin/main", "main"]);
        self.run(&["checkout", "--quiet", "--detach", "release"]);
        self.run(&[
            "merge",
            "--quiet",
            "--no-ff",
            "--message",
            "Merge main into release",
            "main",
        ]);
        self.head()
    }

    /// One call of git, without the configuration of whoever runs it.
    pub fn run(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "tag.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
            .output()
            .expect("git is installed");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("git answers in UTF-8")
    }
}
