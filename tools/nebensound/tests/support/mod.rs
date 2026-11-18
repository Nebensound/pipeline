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
