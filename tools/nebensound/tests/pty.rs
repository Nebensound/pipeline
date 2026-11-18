//! The interface at a real terminal.
//!
//! Everything else about it is a value and is checked without one. What is left
//! — raw mode, the alternate screen, a blocking read — can only be checked by
//! putting a terminal in front of it. So it gets a pseudo terminal here, the
//! same way a person has one.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::io::{Read as _, Write as _};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use support::{QUIET, repository};

/// How long the interface may take until the first screen stands.
const PATIENCE: Duration = Duration::from_secs(30);

#[test]
fn the_interface_draws_and_quits_on_q() {
    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("README.md", "# x\n"),
        ("notes.xyz", "something\n"),
    ]);

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 110,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("a pseudo terminal");

    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_nebensound"));
    command.arg("ui");
    command.cwd(dir.path());
    // Under cargo-llvm-cov every process writes its own measurement; without
    // this line the child's would stay behind.
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }

    let mut child = pair.slave.spawn_command(command).expect("the tool starts");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("read along");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 || sender.send(buffer[..read].to_vec()).is_err() {
                break;
            }
        }
    });

    let screen = wait_for(&receiver, "Tree");
    assert!(
        screen.contains("Tree"),
        "the first screen is missing: {screen}"
    );
    assert!(screen.contains("Node"), "{screen}");

    let mut writer = pair.master.take_writer().expect("write into it");
    // One key that ends nothing, and then the one that does.
    writer.write_all(b"jq").expect("typed");
    writer.flush().expect("sent");

    let status = child.wait().expect("the tool ends");
    // notes.xyz is a gap, and the interface changes nothing about that: it
    // shows the same result `check` reports.
    assert_eq!(status.exit_code(), 1, "{status:?}");
}

/// Reads along until the text shows up or the patience runs out.
fn wait_for(receiver: &mpsc::Receiver<Vec<u8>>, wanted: &str) -> String {
    let deadline = Instant::now() + PATIENCE;
    let mut screen = String::new();
    while Instant::now() < deadline && !screen.contains(wanted) {
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(chunk) => screen.push_str(&String::from_utf8_lossy(&chunk)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    screen
}
