//! What the tests share: files written in one call, a stand-in program, and a
//! directory made up for a test.

#![allow(dead_code, clippy::expect_used, clippy::unwrap_used, clippy::panic)]

// The tool's tests write their stand-ins the same way, from the same file.
#[path = "../../../tests/support/program.rs"]
pub mod program;

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// Writes every file, with the folders it needs.
pub fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, content) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("the folders");
        fs::write(&path, content).expect("the file");
    }
}

/// A directory for one test.
pub struct Scene {
    pub dir: tempfile::TempDir,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("a temporary directory"),
        }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

/// Every line of `text` with `width` spaces in front.
pub fn indent(text: &str, width: usize) -> String {
    text.lines().fold(String::new(), |mut all, line| {
        let _ = writeln!(all, "{}{line}", " ".repeat(width));
        all
    })
}
