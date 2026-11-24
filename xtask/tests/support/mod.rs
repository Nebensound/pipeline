//! What the tests share: files written in one call, a directory made up for a
//! test, and a lock that runs the tests of a file one at a time.

#![allow(dead_code, clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Held by every test of a file that writes programs and starts them.
///
/// A process another test starts while such a file is open for writing
/// inherits it until it has started its own program, and Linux then refuses to
/// run the file as busy ("Text file busy"). With one test at a time no other
/// test starts a process during a write.
static ALONE: Mutex<()> = Mutex::new(());

/// Waits until no other test of this file runs. A test that failed while it
/// held the lock leaves nothing behind that the next one would trip over, so
/// its poison is ignored.
pub fn alone() -> MutexGuard<'static, ()> {
    ALONE.lock().unwrap_or_else(PoisonError::into_inner)
}

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
