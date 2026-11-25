//! A repository on disk, the way Git sees it.
//!
//! No test case here — only what the others need. As a subdirectory, so that
//! Cargo does not build a test binary of its own from it.

#![allow(clippy::expect_used, dead_code)]

pub mod program;

use std::path::Path;
use std::process::Command;
use std::{fs, io};

use nebensound_repo::output::Output;
use nebensound_repo::path::RelativePath;
use nebensound_repo::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER};
use nebensound_repo::{Analysis, Central, Declaration, Tree, analyse};
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
pub const QUIET: &str = "[repo]\naudience = \"public\"\npipeline = \"none\"\n";

/// The Dependabot configuration `nebensound-repo dependabot` prints for a
/// repository that watches only the base set and holds no composite action.
pub const DEPENDABOT: &str = "\
# Written by `nebensound-repo dependabot` from .nebensound/repo.toml and the
# organization's Dependabot policy, described at
# https://github.com/Nebensound/pipeline/blob/main/docs/register.md#dependabot-policy
# Phase 0 warns when this file departs from what the command prints.
---
version: 2
updates:
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
    groups:
      minor-and-patch:
        update-types:
          - minor
          - patch
        exclude-patterns:
          - Nebensound/pipeline/.github/workflows/nebensound.yml
    open-pull-requests-limit: 5
    commit-message:
      prefix: ci(deps)
";

/// One call of the tool, inside this process.
///
/// Always with [`Output::Elsewhere`]. Under `cargo test` the standard output is
/// the terminal the run was started from, so asking would be answered
/// truthfully — and a check of the `ui` command would take that terminal over.
pub fn call(args: &[&str]) -> (String, u8) {
    let mut out = Vec::new();
    let code = nebensound_repo::run_against(
        args.iter().copied(),
        BUILTIN_REGISTER,
        BUILTIN_DEFAULTS,
        Output::Elsewhere,
        &mut out,
    );
    (String::from_utf8(out).expect("the output is UTF-8"), code)
}

/// One call of the tool, inside this process, with these variables of the
/// run and nothing else of the process's, and this output.
pub fn call_with(args: &[&str], variables: &[(&str, &str)], output: Output) -> (String, u8) {
    let read = |name: &str| {
        variables
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_owned())
    };
    let mut out = Vec::new();
    let code = nebensound_repo::run_with(
        args.iter().copied(),
        BUILTIN_REGISTER,
        BUILTIN_DEFAULTS,
        output,
        &read,
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
    nebensound_repo::repository::central().expect("what is compiled into the tool")
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

/// The lines of a log of a workflow run that stand outside every stopped
/// stretch, in order: the lines the runner reads as they are, so the only ones
/// that can start a command. The lines that stop and resume the processing are
/// left out.
///
/// # Panics
///
/// When the log ends inside a stretch: the tool closes every stretch it opens.
pub fn outside(text: &str) -> Vec<&str> {
    let mut stopped: Option<&str> = None;
    let mut outside = Vec::new();
    for line in text.lines() {
        match stopped {
            Some(token) if line == format!("::{token}::") => stopped = None,
            Some(_) => {}
            None => match line.strip_prefix("::stop-commands::") {
                Some(token) => stopped = Some(token),
                None => outside.push(line),
            },
        }
    }
    assert_eq!(stopped, None, "the log ends inside a stretch:\n{text}");
    outside
}

/// What a log of a workflow run holds besides the lines that stop and resume
/// the runner's processing of workflow commands: every other line, in order,
/// each ended by a line break.
pub fn printed(text: &str) -> String {
    let mut stopped: Option<String> = None;
    let mut kept = String::new();
    for line in text.lines() {
        match &stopped {
            Some(token) if line == format!("::{token}::") => stopped = None,
            None if line.starts_with("::stop-commands::") => {
                stopped = Some(line["::stop-commands::".len()..].to_owned());
            }
            _ => {
                kept.push_str(line);
                kept.push('\n');
            }
        }
    }
    kept
}
