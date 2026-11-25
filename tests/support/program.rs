//! The one way a test writes a stand-in program: a file it starts, or lets the
//! code it tests start, in place of a real program.
//!
//! Linux refuses to start a file that any process holds open for writing
//! ("Text file busy"). A test binary runs its tests on many threads, and every
//! process one of them starts begins as a copy of the whole test process, with
//! every file the process holds open at that moment, until it has started its
//! own program. A stand-in the test process wrote itself is therefore busy for
//! as long as a process another thread started during the write has not yet
//! started its own; when the test starts the stand-in in that moment, it
//! fails. Rust opens every file with close-on-exec, which only closes the copy
//! when that process starts its program, and Linux has no way to keep a file
//! out of the copy.
//!
//! So the test process never opens a stand-in for writing. It writes the text
//! into a file of its own elsewhere and has `cp`, a process of its own, write
//! the stand-in from it. `cp` holds the only descriptor that ever writes the
//! stand-in, no process copies it, and Linux closes it when `cp` exits, before
//! the wait for `cp` returns. When [`executable`] returns, no process can hold
//! the stand-in open for writing, whatever the other threads start, and that
//! holds for every caller without a lock.
//!
//! This file is shared by the tests of the tool and of the xtask, so that both
//! write their stand-ins the same way.

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Command;

/// Puts a program with this content at `file`, with the folders it needs, and
/// lets everyone run it. Replaces what was there, which must not be running.
///
/// # Panics
///
/// When the program cannot be written.
pub fn executable(file: &Path, content: impl AsRef<[u8]>) {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder).expect("the folder of a program");
    }
    let mut text = tempfile::NamedTempFile::new().expect("a file for the text of a program");
    text.write_all(content.as_ref())
        .expect("the text of a program");
    let copied = Command::new("cp")
        .arg(text.path())
        .arg(file)
        .output()
        .expect("cp can be started");
    assert!(
        copied.status.success(),
        "cp cannot write {}: {}",
        file.display(),
        String::from_utf8_lossy(&copied.stderr)
    );
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).expect("a program can be run");
}
