//! `nebensound-repo setup`: finding and installing the programs the checks
//! call.
//!
//! Nothing here reaches the network. Every download is a local file, fetched
//! through curl with a `file://` address; the programs and runtimes are shell
//! scripts that print a version or act like an installer. Hence Unix only:
//! the gate runs on Linux.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io::{Cursor, Read as _, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use nebensound_repo::Register;
use nebensound_repo::names::{BuildName, CheckName, ProgramName, RuntimeName};
use nebensound_repo::programs::{Algorithm, Installer, Platform};
use nebensound_repo::setup::{self, Environment, Mode, Options, fetch, find, install, unpack};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use sha2::Digest as _;
use support::{Git, QUIET, call, repository};
use tempfile::TempDir;

/// A computer without programs, a folder of downloads, and a cache.
struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a directory");
        for sub in ["server", "bin", "cache"] {
            fs::create_dir_all(dir.path().join(sub)).expect("a folder");
        }
        let curl = find::search("curl", &std::env::var_os("PATH").unwrap_or_default(), false)
            .expect("curl is installed");
        std::os::unix::fs::symlink(curl, dir.path().join("bin/curl")).expect("curl on the path");
        Self { dir }
    }

    fn path(&self, sub: &str) -> PathBuf {
        self.dir.path().join(sub)
    }

    /// Puts a file where a download can fetch it, and returns its address.
    fn serve(&self, name: &str, bytes: &[u8]) -> String {
        let file = self.path("server").join(name);
        fs::write(&file, bytes).expect("a file to download");
        format!("file://{}", file.display())
    }

    /// Puts a program on the computer.
    fn install(&self, name: &str, text: &str) -> PathBuf {
        let file = self.path("bin").join(name);
        executable(&file, text.as_bytes());
        file
    }

    fn env(&self) -> Environment {
        Environment {
            computer: "linux-x86_64".to_owned(),
            platform: Some(Platform::LinuxX86_64),
            windows: false,
            search_path: self.path("bin").into_os_string(),
            folder: Some(self.path("cache")),
            protocols: "=file".to_owned(),
            answers: None,
        }
    }
}

/// Held by every test of this file for as long as it runs.
///
/// These tests write programs from this process and then start them: the
/// helper [`executable`] does, and so does `setup::run` when it unpacks a
/// download. A process another test starts while such a file is open for
/// writing inherits it until it has started its own program, and Linux then
/// refuses to run the file as busy ("Text file busy"). With one test at a time
/// no other test starts a process during a write.
static ALONE: Mutex<()> = Mutex::new(());

/// Waits until no other test of this file runs. A test that failed while it
/// held the lock leaves nothing behind that the next one would trip over, so
/// its poison is ignored.
fn alone() -> MutexGuard<'static, ()> {
    ALONE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Puts an executable file with these bytes at `file`.
fn executable(file: &Path, bytes: &[u8]) {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).expect("a folder");
    }
    fs::write(file, bytes).expect("a file");
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).expect("executable");
}

/// A program that prints this line, whatever it is asked.
fn prints(line: &str) -> String {
    format!("#!/bin/sh\necho '{line}'\n")
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", hex(&sha2::Sha256::digest(bytes)))
}

fn sha512(bytes: &[u8]) -> String {
    format!("sha512:{}", hex(&sha2::Sha512::digest(bytes)))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).expect("compressed");
    encoder.finish().expect("compressed")
}

fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, path, *bytes)
            .expect("an entry");
    }
    gzip(&builder.into_inner().expect("an archive"))
}

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in entries {
        writer
            .start_file(
                *path,
                zip::write::SimpleFileOptions::default().unix_permissions(0o755),
            )
            .expect("an entry");
        writer.write_all(bytes).expect("written");
    }
    writer.finish().expect("an archive").into_inner()
}

/// A register with the tables every register has, and these entries.
fn register(entries: &str) -> Register {
    Register::parse(&format!(
        "[version]\n[build]\n[target]\n[preset]\n[ecosystem]\n{entries}"
    ))
    .expect("a register")
}

/// A program with one build for Linux on x86.
fn native(name: &str, version: &str, url: &str, digest: &str, executable: &str) -> String {
    format!(
        "[program.{name}]\nversion = \"{version}\"\ncommand = \"{name}\"\n\
         [program.{name}.download.linux-x86_64]\nurl = \"{url}\"\ndigest = \"{digest}\"\n\
         executable = \"{executable}\"\n"
    )
}

/// A program that is one package for a runtime, and runs on it from 4.0.0.
fn packaged(name: &str, runtime: &str, url: &str, digest: &str) -> String {
    packaged_from(name, runtime, "4.0.0", url, digest)
}

/// A program that is one package for a runtime, and runs on it from this
/// version.
fn packaged_from(name: &str, runtime: &str, minimum: &str, url: &str, digest: &str) -> String {
    format!(
        "[program.{name}]\nversion = \"7.8.9\"\ncommand = \"{name}\"\nruntime = \"{runtime}\"\n\
         runtime-minimum = \"{minimum}\"\n\
         [program.{name}.download.any]\nurl = \"{url}\"\ndigest = \"{digest}\"\n"
    )
}

/// A runtime with one build for Linux on x86.
fn runtime(name: &str, installer: &str, url: &str, digest: &str, executable: &str) -> String {
    format!(
        "[runtime.{name}]\ntitle = \"The {name} runtime\"\nversion = \"4.5.6\"\n\
         commands = [\"{name}\"]\ninstaller = \"{installer}\"\n\
         [runtime.{name}.download.linux-x86_64]\nurl = \"{url}\"\ndigest = \"{digest}\"\n\
         executable = \"{executable}\"\n"
    )
}

/// The check `json`, calling these programs.
fn check(programs: &[&str]) -> String {
    let list: Vec<String> = programs.iter().map(|name| format!("\"{name}\"")).collect();
    format!(
        "[check.json]\nextensions = [\".json\"]\nprograms = [{}]\n[check.yaml]\nextensions = \
         [\".yaml\"]\n",
        list.join(", ")
    )
}

fn json() -> Vec<CheckName> {
    vec![CheckName::new("json").expect("a name")]
}

fn run(
    register: &Register,
    mode: Mode,
    with_runtimes: bool,
    env: &mut Environment,
) -> (String, u8) {
    let mut out = Vec::new();
    let code = setup::run(
        register,
        &json(),
        Options {
            mode,
            with_runtimes,
        },
        env,
        &mut out,
    );
    (String::from_utf8(out).expect("UTF-8"), code)
}

/// Three programs: one on the computer in the pinned version, one in
/// another, and one missing.
fn three(fixture: &Fixture) -> Register {
    fixture.install("alpha", &prints("alpha 1.2.3"));
    fixture.install("beta", &prints("beta version 2.1.0"));
    let alpha = prints("alpha 1.2.3 pinned");
    let beta = prints("beta 2.0.0 pinned");
    let gamma = tar_gz(&[("gamma-dir/gamma", prints("gamma 3.0.0").as_bytes())]);
    let entries = [
        check(&["alpha", "beta", "gamma"]),
        native(
            "alpha",
            "1.2.3",
            &fixture.serve("alpha", alpha.as_bytes()),
            &sha256(alpha.as_bytes()),
            "alpha",
        ),
        native(
            "beta",
            "2.0.0",
            &fixture.serve("beta.gz", &gzip(beta.as_bytes())),
            &sha256(&gzip(beta.as_bytes())),
            "beta",
        ),
        native(
            "gamma",
            "3.0.0",
            &fixture.serve("gamma.tar.gz", &gamma),
            &sha512(&gamma),
            "gamma-dir/gamma",
        ),
    ];
    register(&entries.concat())
}

#[test]
fn check_says_what_the_computer_has_and_changes_nothing() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = three(&fixture);

    let (text, code) = run(&register, Mode::Check, false, &mut fixture.env());

    assert_eq!(code, 1, "{text}");
    assert!(text.starts_with("Checks: json\nFolder: "), "{text}");
    let bin = fixture.path("bin");
    assert!(
        text.contains(&format!(
            "  alpha 1.2.3 (json): on the computer, {}\n",
            bin.join("alpha").display()
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "  beta 2.0.0 (json): on the computer, {}, version 2.1.0\n",
            bin.join("beta").display()
        )),
        "{text}"
    );
    assert!(text.contains("  gamma 3.0.0 (json): missing\n"), "{text}");
    assert!(text.contains("Missing: gamma 3.0.0.\n"), "{text}");
    assert!(
        text.contains("Not the pinned version: beta 2.0.0 (the computer's: 2.1.0).\n"),
        "{text}"
    );
    assert!(!text.contains("Runtimes"), "{text}");
    assert!(!fixture.path("cache/programs").exists());
}

#[test]
fn the_default_installs_only_what_is_missing() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = three(&fixture);

    let (text, code) = run(&register, Mode::Missing, false, &mut fixture.env());

    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("Installing gamma 3.0.0 from file://"),
        "{text}"
    );
    assert!(!text.contains("Installing alpha"), "{text}");
    assert!(!text.contains("Installing beta"), "{text}");
    let gamma = fixture.path("cache/programs/gamma/3.0.0/gamma-dir/gamma");
    assert!(
        text.contains(&format!(
            "  gamma 3.0.0 (json): pinned, {}\n",
            gamma.display()
        )),
        "{text}"
    );
    assert!(text.contains("Nothing is missing.\n"), "{text}");
    assert!(
        text.contains("Not the pinned version: beta 2.0.0"),
        "{text}"
    );
    assert!(!fixture.path("cache/downloads/gamma-3.0.0").exists());
}

#[test]
fn pinned_installs_every_program_and_leaves_the_computers_alone() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = three(&fixture);

    let (text, code) = run(&register, Mode::Pinned, false, &mut fixture.env());

    assert_eq!(code, 0, "{text}");
    for name in ["alpha", "beta", "gamma"] {
        assert!(text.contains(&format!("Installing {name} ")), "{text}");
    }
    assert!(text.contains("Nothing is missing.\n"), "{text}");
    assert!(!text.contains("Not the pinned version"), "{text}");
    assert_eq!(
        fs::read_to_string(fixture.path("bin/beta")).expect("the computer's beta"),
        prints("beta version 2.1.0")
    );
    let beta = fixture.path("cache/programs/beta/2.0.0/beta");
    let output = Command::new(&beta).output().expect("the pinned beta runs");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "beta 2.0.0 pinned\n"
    );

    // A second run finds everything pinned and installs nothing.
    let (again, code) = run(&register, Mode::Pinned, false, &mut fixture.env());
    assert_eq!(code, 0, "{again}");
    assert!(!again.contains("Installing"), "{again}");

    let (check, code) = run(&register, Mode::Check, false, &mut fixture.env());
    assert_eq!(code, 0, "{check}");
    assert!(check.contains("Nothing is missing.\n"), "{check}");
    assert!(
        check.contains(&format!(
            "  beta 2.0.0 (json): pinned, {}\n",
            beta.display()
        )),
        "{check}"
    );
}

#[test]
fn a_download_with_another_digest_is_refused() {
    let _alone = alone();
    let fixture = Fixture::new();
    let bytes = prints("delta 1.0.0");
    let register = register(
        &[
            check(&["delta"]),
            native(
                "delta",
                "1.0.0",
                &fixture.serve("delta", bytes.as_bytes()),
                &sha256(b"something else"),
                "delta",
            ),
        ]
        .concat(),
    );

    let (text, code) = run(&register, Mode::Pinned, false, &mut fixture.env());

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "delta 1.0.0 was not installed: the download from file://{} has the digest {}, and \
             the register pins {}; it was refused and deleted\n",
            fixture.path("server/delta").display(),
            sha256(bytes.as_bytes()),
            sha256(b"something else")
        )),
        "{text}"
    );
    assert!(text.contains("Missing: delta 1.0.0.\n"), "{text}");
    assert!(!fixture.path("cache/programs/delta").exists());
    assert!(!fixture.path("cache/downloads/delta-1.0.0").exists());
}

#[test]
fn every_packing_is_unpacked() {
    let _alone = alone();
    let fixture = Fixture::new();
    let program = |name: &str| prints(&format!("{name} 1.0.0"));
    let zipped = zip(&[("inner/zipped", program("zipped").as_bytes())]);
    let register = register(
        &[
            check(&["plain", "zipped"]),
            native(
                "plain",
                "1.0.0",
                &fixture.serve("plain-tool", program("plain").as_bytes()),
                &sha256(program("plain").as_bytes()),
                "plain",
            ),
            native(
                "zipped",
                "1.0.0",
                &fixture.serve("zipped.ZIP", &zipped),
                &sha256(&zipped),
                "inner/zipped",
            ),
        ]
        .concat(),
    );

    let (text, code) = run(&register, Mode::Pinned, false, &mut fixture.env());

    assert_eq!(code, 0, "{text}");
    for file in ["plain/1.0.0/plain", "zipped/1.0.0/inner/zipped"] {
        let path = fixture.path("cache/programs").join(file);
        let mode = fs::metadata(&path).expect("installed").permissions().mode();
        assert_eq!(mode & 0o777, 0o755, "{file}");
    }
}

#[test]
fn a_download_that_is_not_what_it_says_is_refused() {
    let _alone = alone();
    let fixture = Fixture::new();
    let without = tar_gz(&[("other", b"x")]);
    let broken_zip = b"not a zip".to_vec();
    let broken_gz = b"not gzip".to_vec();
    let broken_tar = b"not gzip either".to_vec();
    let register = register(
        &[
            check(&["without", "broken-zip", "broken-gz", "broken-tar"]),
            native(
                "without",
                "1.0.0",
                &fixture.serve("without.tgz", &without),
                &sha256(&without),
                "tool",
            ),
            native(
                "broken-zip",
                "1.0.0",
                &fixture.serve("broken.zip", &broken_zip),
                &sha256(&broken_zip),
                "tool",
            ),
            native(
                "broken-gz",
                "1.0.0",
                &fixture.serve("broken.gz", &broken_gz),
                &sha256(&broken_gz),
                "tool",
            ),
            native(
                "broken-tar",
                "1.0.0",
                &fixture.serve("broken.tar.gz", &broken_tar),
                &sha256(&broken_tar),
                "tool",
            ),
        ]
        .concat(),
    );

    let (text, code) = run(&register, Mode::Pinned, false, &mut fixture.env());

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("without 1.0.0 was not installed: ")
            && text.contains("holds no tool, which the register names as its program"),
        "{text}"
    );
    for name in ["broken-zip", "broken-tar"] {
        assert!(
            text.contains(&format!("{name} 1.0.0 was not installed: "))
                && text.contains("cannot be unpacked"),
            "{text}"
        );
    }
    assert!(
        text.contains("broken-gz 1.0.0 was not installed: ")
            && text.contains("cannot be unpacked into"),
        "{text}"
    );
    for name in ["without", "broken-zip", "broken-gz", "broken-tar"] {
        assert!(
            !fixture
                .path("cache/programs")
                .join(name)
                .join("1.0.0")
                .exists(),
            "{name}"
        );
    }
}

#[test]
fn a_computer_without_a_build_is_told_so() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = three(&fixture);
    let mut env = fixture.env();
    env.computer = "plan9-mips".to_owned();
    env.platform = None;

    let (text, code) = run(&register, Mode::Missing, false, &mut env);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(
            "gamma 3.0.0 was not installed: the register has no pinned build for plan9-mips\n"
        ),
        "{text}"
    );

    let mut env = fixture.env();
    env.platform = Some(Platform::MacosAarch64);
    env.computer = "macos-aarch64".to_owned();
    let (text, _) = run(&register, Mode::Missing, false, &mut env);
    assert!(
        text.contains("the register has no pinned build for macos-aarch64"),
        "{text}"
    );
}

#[test]
fn without_a_build_only_the_default_takes_the_computers_copy() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = three(&fixture);
    fixture.install("gamma", &prints("gamma 3.0.0"));
    let without_a_build = || {
        let mut env = fixture.env();
        env.computer = "plan9-mips".to_owned();
        env.platform = None;
        env
    };

    let (text, code) = run(&register, Mode::Missing, false, &mut without_a_build());
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("was not installed"), "{text}");
    assert!(text.contains("Nothing is missing.\n"), "{text}");

    let (text, code) = run(&register, Mode::Pinned, false, &mut without_a_build());
    assert_eq!(code, 1, "{text}");
    for (name, version) in [("alpha", "1.2.3"), ("beta", "2.0.0"), ("gamma", "3.0.0")] {
        assert!(
            text.contains(&format!(
                "{name} {version} was not installed: the register has no pinned build for \
                 plan9-mips\n"
            )),
            "{text}"
        );
    }
    assert!(!fixture.path("cache/programs").exists());
}

#[test]
fn a_download_that_fails_says_why() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = register(
        &[
            check(&["nowhere"]),
            native(
                "nowhere",
                "1.0.0",
                &format!("file://{}", fixture.path("server/nothing").display()),
                &sha256(b""),
                "nowhere",
            ),
        ]
        .concat(),
    );

    let (text, code) = run(&register, Mode::Missing, false, &mut fixture.env());
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("nowhere 1.0.0 was not installed: curl failed"),
        "{text}"
    );

    // Without curl there is nothing to download with.
    fs::remove_file(fixture.path("bin/curl")).expect("curl taken away");
    let (text, code) = run(&register, Mode::Missing, false, &mut fixture.env());
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(
            "nowhere 1.0.0 was not installed: there is no curl on the PATH to download with\n"
        ),
        "{text}"
    );
}

#[test]
fn a_download_over_a_protocol_not_allowed_is_not_made() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = three(&fixture);
    let mut env = fixture.env();
    env.protocols = "=https".to_owned();

    let (text, code) = run(&register, Mode::Missing, false, &mut env);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("gamma 3.0.0 was not installed: curl failed"),
        "{text}"
    );
}

#[test]
fn without_a_cache_directory_nothing_is_done() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = three(&fixture);
    let mut env = fixture.env();
    env.folder = None;

    let (text, code) = run(&register, Mode::Check, false, &mut env);

    assert_eq!(code, 1);
    assert_eq!(
        text,
        "there is no cache directory to keep pinned programs in: HOME is not set (on Windows, \
         LOCALAPPDATA)\n"
    );
}

#[test]
fn checks_that_call_no_program_need_nothing() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = register(&check(&[]));
    let mut out = Vec::new();

    let code = setup::run(
        &register,
        &[CheckName::new("yaml").expect("a name")],
        Options {
            mode: Mode::Pinned,
            with_runtimes: false,
        },
        &mut fixture.env(),
        &mut out,
    );

    let text = String::from_utf8(out).expect("UTF-8");
    assert_eq!(code, 0, "{text}");
    assert!(text.starts_with("Checks: yaml\n"), "{text}");
    assert!(
        text.ends_with("\nThese checks call no program the register pins.\n"),
        "{text}"
    );

    let mut out = Vec::new();
    setup::run(
        &register,
        &[],
        Options {
            mode: Mode::Check,
            with_runtimes: false,
        },
        &mut fixture.env(),
        &mut out,
    );
    assert!(
        String::from_utf8(out)
            .expect("UTF-8")
            .starts_with("Checks: none\n")
    );
}

#[test]
fn a_build_gets_the_programs_it_calls() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.install("alpha", &prints("alpha 1.2.3"));
    let register = register(&format!(
        "{}{}[build.scan]\nproduces = \"container\"\nprograms = [\"alpha\"]\n\
         [build.pack]\nproduces = \"crate\"\n[build.yaml]\nproduces = \"crate\"\n",
        check(&["alpha"]),
        native("alpha", "1.2.3", "file:///nowhere", &sha256(b"x"), "alpha"),
    ));
    let names =
        |names: &[&str]| -> Vec<String> { names.iter().map(|name| (*name).to_owned()).collect() };
    let (builds, rest) = setup::builds_named(&register, &names(&["scan", "json", "pack", "scan"]));
    let shown: Vec<String> = builds.iter().map(ToString::to_string).collect();
    assert_eq!(shown, ["pack", "scan"]);
    assert_eq!(rest, ["json"]);
    // A name that is a check and a build sets up both.
    let (builds, rest) = setup::builds_named(&register, &names(&["yaml", "scan"]));
    let shown: Vec<String> = builds.iter().map(ToString::to_string).collect();
    assert_eq!(shown, ["scan", "yaml"]);
    assert_eq!(rest, ["yaml"]);

    let set_up = |checks: &[&str], builds: &[&str], mode| {
        let checks: Vec<CheckName> = checks
            .iter()
            .map(|name| CheckName::new(name).expect("a name"))
            .collect();
        let builds: Vec<BuildName> = builds
            .iter()
            .map(|name| BuildName::new(name).expect("a name"))
            .collect();
        let mut out = Vec::new();
        let code = setup::run_for(
            &register,
            &checks,
            &builds,
            Options {
                mode,
                with_runtimes: false,
            },
            &mut fixture.env(),
            &mut out,
        );
        (String::from_utf8(out).expect("UTF-8"), code)
    };
    let (text, code) = set_up(&[], &["scan"], Mode::Check);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.starts_with("Checks: none\nBuilds: scan\nFolder: "),
        "{text}"
    );
    assert!(text.contains("\n  alpha 1.2.3 (scan): "), "{text}");
    let (text, _) = set_up(&["json"], &["scan"], Mode::Check);
    assert!(text.contains("\n  alpha 1.2.3 (json, scan): "), "{text}");

    let (text, code) = set_up(&[], &["pack"], Mode::Pinned);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.ends_with("\nThese builds call no program the register pins.\n"),
        "{text}"
    );
    let (text, _) = set_up(&["yaml"], &["pack"], Mode::Pinned);
    assert!(
        text.ends_with("\nThese checks and builds call no program the register pins.\n"),
        "{text}"
    );
}

#[test]
fn an_installation_that_broke_off_is_done_again() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = three(&fixture);
    let left = fixture.path("cache/programs/gamma/3.0.0");
    fs::create_dir_all(left.join("gamma-dir")).expect("a folder");
    fs::write(left.join("gamma-dir/gamma"), "half").expect("a file");

    // Without its mark it is no installation.
    let (text, _) = run(&register, Mode::Check, false, &mut fixture.env());
    assert!(text.contains("  gamma 3.0.0 (json): missing\n"), "{text}");

    let (text, code) = run(&register, Mode::Missing, false, &mut fixture.env());
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        fs::read_to_string(left.join("gamma-dir/gamma")).expect("installed"),
        prints("gamma 3.0.0")
    );
}

#[test]
fn a_folder_that_cannot_be_written_stops_the_installation() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = three(&fixture);

    // A file where the folders of the programs would go.
    fs::write(fixture.path("cache/programs"), "").expect("a file");
    let (text, code) = run(&register, Mode::Missing, false, &mut fixture.env());
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("cannot be made: "), "{text}");
    fs::remove_file(fixture.path("cache/programs")).expect("removed");

    // A file where the downloads would go.
    fs::remove_dir_all(fixture.path("cache/downloads")).expect("emptied");
    fs::write(fixture.path("cache/downloads"), "").expect("a file");
    let (text, code) = run(&register, Mode::Missing, false, &mut fixture.env());
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("cannot be made: "), "{text}");
    fs::remove_file(fixture.path("cache/downloads")).expect("removed");

    // A broken-off installation in a folder that cannot be emptied.
    let parent = fixture.path("cache/programs/gamma");
    fs::create_dir_all(parent.join("3.0.0")).expect("a folder");
    fs::write(parent.join("3.0.0/left"), "").expect("a file");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).expect("read-only");
    let (text, code) = run(&register, Mode::Missing, false, &mut fixture.env());
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).expect("writable again");
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("cannot be emptied: "), "{text}");
}

#[test]
fn an_archive_that_takes_the_place_of_the_mark_is_refused() {
    let _alone = alone();
    let fixture = Fixture::new();
    let archive = tar_gz(&[
        ("tool", prints("odd 1.0.0").as_bytes()),
        (".nebensound-installed/inside", b"x"),
    ]);
    let single = prints("lone 1.0.0");
    let register = register(
        &[
            check(&["odd", "lone"]),
            native(
                "odd",
                "1.0.0",
                &fixture.serve("odd.tgz", &archive),
                &sha256(&archive),
                "tool",
            ),
            native(
                "lone",
                "1.0.0",
                &fixture.serve("lone", single.as_bytes()),
                &sha256(single.as_bytes()),
                "no-such-folder/lone",
            ),
        ]
        .concat(),
    );

    let (text, code) = run(&register, Mode::Pinned, false, &mut fixture.env());

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("odd 1.0.0 was not installed: ") && text.contains("cannot be written"),
        "{text}"
    );
    assert!(
        text.contains("lone 1.0.0 was not installed: ") && text.contains("cannot be written"),
        "{text}"
    );
}

/// A runtime archive: the runtime, which prints its version, and an
/// installer beside it, which acts like npm.
fn npm_runtime(npm: &str) -> Vec<u8> {
    tar_gz(&[
        ("rt/bin/rt", prints("rt 4.5.6").as_bytes()),
        ("rt/bin/npm", npm.as_bytes()),
    ])
}

/// An npm that puts a command named like the package file into
/// `node_modules/.bin`.
const NPM: &str = r#"#!/bin/sh
prefix=""
file=""
while [ $# -gt 0 ]; do
  case "$1" in
    --prefix) prefix="$2"; shift 2 ;;
    install|--*) shift ;;
    *) file="$1"; shift ;;
  esac
done
name="${file##*/}"
name="${name%.tgz}"
/bin/mkdir -p "$prefix/node_modules/.bin"
printf '#!/bin/sh\necho "%s 7.8.9"\n' "$name" > "$prefix/node_modules/.bin/$name"
/bin/chmod +x "$prefix/node_modules/.bin/$name"
"#;

/// A Python whose `-m venv` makes a virtual environment whose Python acts
/// like pip: it puts a command named like the wheel into its own folder.
const PYTHON: &str = r##"#!/bin/sh
if [ "$1" = "-m" ] && [ "$2" = "venv" ]; then
  /bin/mkdir -p "$3/bin"
  printf '%s\n' '#!/bin/sh' 'for last; do :; done' 'name="${last##*/}"' 'name="${name%.whl}"' \
    'here="${0%/*}"' 'printf "#!/bin/sh\necho \"%s 7.8.9\"\n" "$name" > "$here/$name"' \
    '/bin/chmod +x "$here/$name"' > "$3/bin/python"
  /bin/chmod +x "$3/bin/python"
  exit 0
fi
echo "py 4.5.6"
"##;

/// A program on the runtime `rt`, and the runtime itself, built with this
/// npm.
fn on_a_runtime(fixture: &Fixture, npm: &str) -> Register {
    let archive = npm_runtime(npm);
    register(
        &[
            check(&["pkg"]),
            packaged(
                "pkg",
                "rt",
                &fixture.serve("pkg.tgz", b"a package"),
                &sha256(b"a package"),
            ),
            runtime(
                "rt",
                "npm",
                &fixture.serve("rt.tar.gz", &archive),
                &sha256(&archive),
                "rt/bin/rt",
            ),
        ]
        .concat(),
    )
}

#[test]
fn without_a_terminal_and_without_permission_no_runtime_and_nothing_else_is_installed() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_a_runtime(&fixture, NPM);

    let (text, code) = run(&register, Mode::Pinned, false, &mut fixture.env());

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "These runtimes are missing, and programs need them:\n  rt 4.5.6 (The rt runtime), \
             for pkg: file://{}\nThere is no terminal to ask in, and --with-runtimes was not \
             given, so no runtime is installed.\nNothing was installed.\n",
            fixture.path("server/rt.tar.gz").display()
        )),
        "{text}"
    );
    assert!(!fixture.path("cache/runtimes").exists());
    assert!(!fixture.path("cache/programs").exists());

    let (text, code) = run(&register, Mode::Check, false, &mut fixture.env());
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("Runtimes\n  rt 4.5.6: missing\n"), "{text}");
    assert!(text.contains("Missing: rt 4.5.6, pkg 7.8.9.\n"), "{text}");
}

#[test]
fn in_a_terminal_a_runtime_is_installed_on_a_yes_only() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_a_runtime(&fixture, NPM);

    for answer in ["n\n", "", "maybe\n"] {
        let mut env = fixture.env();
        env.answers = Some(Box::new(Cursor::new(answer.as_bytes().to_vec())));
        let (text, code) = run(&register, Mode::Missing, false, &mut env);
        assert_eq!(code, 1, "{text}");
        assert!(
            text.contains(&format!(
                "Install them into {}? [y/N] Nothing was installed.\n",
                fixture.path("cache/runtimes").display()
            )),
            "{text}"
        );
        assert!(!fixture.path("cache/runtimes").exists());
    }

    let mut env = fixture.env();
    env.answers = Some(Box::new(Cursor::new(b"Yes\n".to_vec())));
    let (text, code) = run(&register, Mode::Missing, false, &mut env);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Installing rt 4.5.6 from file://"), "{text}");
    assert!(text.contains("Installing pkg 7.8.9 from file://"), "{text}");
    let rt = fixture.path("cache/runtimes/rt/4.5.6/rt/bin/rt");
    let pkg = fixture.path("cache/programs/pkg/7.8.9/node_modules/.bin/pkg");
    assert!(
        text.contains(&format!("  rt 4.5.6: pinned, {}\n", rt.display())),
        "{text}"
    );
    assert!(
        text.contains(&format!("  pkg 7.8.9 (json): pinned, {}\n", pkg.display())),
        "{text}"
    );
}

/// A program on the runtime `py`, and the runtime itself, which acts like
/// Python.
fn on_python(fixture: &Fixture) -> Register {
    let python = tar_gz(&[("py/bin/py", PYTHON.as_bytes())]);
    register(
        &[
            check(&["wheel-tool"]),
            packaged(
                "wheel-tool",
                "py",
                &fixture.serve("wheel-tool.whl", b"a wheel"),
                &sha256(b"a wheel"),
            ),
            runtime(
                "py",
                "pip",
                &fixture.serve("py.tar.gz", &python),
                &sha256(&python),
                "py/bin/py",
            ),
        ]
        .concat(),
    )
}

#[test]
fn with_runtimes_installs_without_asking_and_pip_gets_an_environment_of_its_own() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_python(&fixture);

    let (text, code) = run(&register, Mode::Pinned, true, &mut fixture.env());

    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("[y/N]"), "{text}");
    // The runtime this run installed for the program stays in the report,
    // although the program installed with pip no longer needs it.
    let py = fixture.path("cache/runtimes/py/4.5.6/py/bin/py");
    assert!(
        text.contains(&format!("Runtimes\n  py 4.5.6: pinned, {}\n", py.display())),
        "{text}"
    );
    let tool = fixture.path("cache/programs/wheel-tool/7.8.9/bin/wheel-tool");
    let output = Command::new(&tool).output().expect("the tool runs");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "wheel-tool 7.8.9\n"
    );
}

#[test]
fn a_virtual_environment_whose_python_is_gone_is_installed_again() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_python(&fixture);
    let (text, code) = run(&register, Mode::Pinned, true, &mut fixture.env());
    assert_eq!(code, 0, "{text}");
    let python = fixture.path("cache/programs/wheel-tool/7.8.9/bin/python");
    fs::remove_file(&python).expect("the environment's Python");

    let (text, code) = run(&register, Mode::Check, false, &mut fixture.env());
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("  wheel-tool 7.8.9 (json): missing\n"),
        "{text}"
    );

    let (text, code) = run(&register, Mode::Missing, false, &mut fixture.env());
    assert_eq!(code, 0, "{text}");
    assert!(python.is_file(), "{text}");
}

#[test]
fn a_windows_environment_starts_only_while_its_python_is_there() {
    let _alone = alone();
    let fixture = Fixture::new();
    let dir = fixture.path("environment");
    let base = fixture.path("python");
    fs::create_dir_all(&base).expect("a folder");
    let config = dir.join("pyvenv.cfg");

    // No launcher.
    assert!(!find::environment_starts(&dir, true));
    executable(&dir.join("Scripts/python.exe"), b"a launcher");
    // A launcher, but no pyvenv.cfg.
    assert!(!find::environment_starts(&dir, true));
    // A pyvenv.cfg that names no home.
    fs::write(&config, "include-system-site-packages = false\n").expect("a file");
    assert!(!find::environment_starts(&dir, true));
    let named = format!(
        "\nversion = 3.12.0\nhome = {}\ninclude-system-site-packages = false\n",
        base.display()
    );
    fs::write(&config, named).expect("a file");
    // The Python it names is gone.
    assert!(!find::environment_starts(&dir, true));
    executable(&base.join("python.exe"), b"python");
    assert!(find::environment_starts(&dir, true));

    // On Linux and macOS the environment's Python alone tells.
    assert!(!find::environment_starts(&dir, false));
    executable(&dir.join("bin/python"), b"python");
    assert!(find::environment_starts(&dir, false));
}

#[test]
fn pip_takes_wheels_only() {
    let _alone = alone();
    let fixture = Fixture::new();
    let runtime = fixture.path("runtime/bin/python3");
    // A Python whose virtual environment's Python writes its arguments down.
    executable(
        &runtime,
        br#"#!/bin/sh
/bin/mkdir -p "$3/bin"
printf '%s\n' '#!/bin/sh' 'echo "$*" > "${0%/*}/arguments"' > "$3/bin/python"
/bin/chmod +x "$3/bin/python"
"#,
    );
    let wheel = fixture.path("tool.whl");
    let dir = fixture.path("dir");

    install::package(
        Installer::Pip,
        &runtime,
        std::slice::from_ref(&wheel),
        &dir,
        &fixture.env(),
    )
    .expect("installed");

    assert_eq!(
        fs::read_to_string(dir.join("bin/arguments")).expect("written"),
        format!(
            "-m pip install --disable-pip-version-check --no-cache-dir --quiet \
             --only-binary=:all: {}\n",
            wheel.display()
        )
    );
}

#[test]
fn a_runtime_that_prints_no_version_is_passed_over() {
    let _alone = alone();
    // On Windows the Store puts `python3.exe` and `python.exe` on the PATH of
    // a computer without Python, which print only where to get one.
    let fixture = Fixture::new();
    let register = on_a_runtime(&fixture, NPM);
    let stub = fixture.path("stub/rt");
    executable(
        &stub,
        b"#!/bin/sh\necho 'rt was not found; install it from the store' >&2\nexit 49\n",
    );
    let mut env = fixture.env();
    env.search_path =
        std::env::join_paths([fixture.path("stub"), fixture.path("bin")]).expect("a search path");
    let pkg = ProgramName::new("pkg").expect("a name");

    // `pkg` is missing, so the run would install it and needs the runtime.
    let (text, code) = run(&register, Mode::Check, false, &mut env);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("Runtimes\n  rt 4.5.6: missing\n"), "{text}");
    let computer = fixture.install("pkg", &prints("pkg 7.8.9"));
    let found = find::locate(&register, &pkg, &env).expect("found");
    assert_eq!(found.runtime, None);

    let rt = fixture.install("rt", &prints("rt 4.5.6"));
    let found = find::locate(&register, &pkg, &env).expect("found");
    assert_eq!(found.runtime, Some(rt.clone()));
    fs::remove_file(computer).expect("the computer's pkg");
    let (text, code) = run(&register, Mode::Check, false, &mut env);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!("  rt 4.5.6: on the computer, {}\n", rt.display())),
        "{text}"
    );
    assert!(text.contains("Missing: pkg 7.8.9.\n"), "{text}");
}

/// Three programs on the runtime `rt`, which run on it from 4.0.0, 4.2.0 and
/// 4.1.0, and the runtime itself, pinned to 4.5.6.
fn on_a_runtime_with_minimums(fixture: &Fixture) -> Register {
    let archive = npm_runtime(NPM);
    let mut entries = vec![check(&["early", "late", "middle"])];
    for (name, minimum) in [("early", "4.0.0"), ("late", "4.2.0"), ("middle", "4.1.0")] {
        let file = format!("{name}.tgz");
        entries.push(packaged_from(
            name,
            "rt",
            minimum,
            &fixture.serve(&file, name.as_bytes()),
            &sha256(name.as_bytes()),
        ));
    }
    entries.push(runtime(
        "rt",
        "npm",
        &fixture.serve("rt.tar.gz", &archive),
        &sha256(&archive),
        "rt/bin/rt",
    ));
    register(&entries.concat())
}

#[test]
fn a_runtime_too_old_for_a_program_on_it_counts_as_missing() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_a_runtime_with_minimums(&fixture);
    let old = fixture.install("rt", &prints("rt 4.1.0"));
    fixture.install("npm", NPM);

    // Too old for `late`, which needs 4.2.0: missing, in every mode.
    let (text, code) = run(&register, Mode::Check, false, &mut fixture.env());
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "Runtimes\n  rt 4.5.6: on the computer, {}, version 4.1.0, too old: late needs \
             4.2.0 or later\n",
            old.display()
        )),
        "{text}"
    );
    assert!(text.contains("Missing: rt 4.5.6, early 7.8.9,"), "{text}");

    // It is listed and asked for like a missing one, and nothing is installed
    // without a yes.
    for mode in [Mode::Missing, Mode::Pinned] {
        let (text, code) = run(&register, mode, false, &mut fixture.env());
        assert_eq!(code, 1, "{text}");
        assert!(
            text.contains(&format!(
                "These runtimes are missing, and programs need them:\n  rt 4.5.6 (The rt \
                 runtime), for early, late, middle: file://{}\n    The computer has 4.1.0 ({}); \
                 late needs 4.2.0 or later.\nThere is no terminal to ask in",
                fixture.path("server/rt.tar.gz").display(),
                old.display()
            )),
            "{text}"
        );
        assert!(text.ends_with("Nothing was installed.\n"), "{text}");
        assert!(!fixture.path("cache").join("runtimes").exists());
    }

    // The other calls get it only for a program it can run.
    let early = ProgramName::new("early").expect("a name");
    let late = ProgramName::new("late").expect("a name");
    fixture.install("early", &prints("early 7.8.9"));
    fixture.install("late", &prints("late 7.8.9"));
    let found = find::locate(&register, &early, &fixture.env()).expect("found");
    assert_eq!(found.runtime, Some(old.clone()));
    let found = find::locate(&register, &late, &fixture.env()).expect("found");
    assert_eq!(found.runtime, None);

    // `--with-runtimes` installs it, for `--pinned`, which installs `late`.
    let (text, code) = run(&register, Mode::Pinned, true, &mut fixture.env());
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Installing rt 4.5.6 from file://"), "{text}");
    assert!(
        text.contains("Installing late 7.8.9 from file://"),
        "{text}"
    );
    let pinned = fixture.path("cache/runtimes/rt/4.5.6/rt/bin/rt");
    assert!(
        text.contains(&format!("  rt 4.5.6: pinned, {}\n", pinned.display())),
        "{text}"
    );
    let found = find::locate(&register, &late, &fixture.env()).expect("found");
    assert_eq!(found.runtime, Some(pinned));
}

#[test]
fn a_runtime_is_held_only_against_the_programs_the_run_installs() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_a_runtime_with_minimums(&fixture);
    let old = fixture.install("rt", &prints("rt 4.1.0"));
    fixture.install("npm", NPM);
    // `late`, which needs 4.2.0, is on the computer; `middle` and `early`,
    // which run on 4.1.0, are not.
    fixture.install("late", &prints("late 7.8.9"));

    let (text, code) = run(&register, Mode::Check, false, &mut fixture.env());
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "Runtimes\n  rt 4.5.6: on the computer, {}, version 4.1.0\n",
            old.display()
        )),
        "{text}"
    );
    assert!(
        text.contains("Missing: early 7.8.9, middle 7.8.9.\n"),
        "{text}"
    );

    let (text, code) = run(&register, Mode::Missing, false, &mut fixture.env());
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("[y/N]"), "{text}");
    assert!(!text.contains("These runtimes are missing"), "{text}");
    assert!(!text.contains("Installing rt"), "{text}");
    assert!(!text.contains("Installing late"), "{text}");
    assert!(
        text.contains("Installing middle 7.8.9 from file://"),
        "{text}"
    );
    assert!(text.contains("Nothing is missing.\n"), "{text}");
    assert!(!fixture.path("cache/runtimes").exists());
}

#[test]
fn programs_on_the_computer_need_no_runtime_in_the_default_mode() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_a_runtime_with_minimums(&fixture);
    // No runtime at all, and every program on it on the computer.
    for name in ["early", "late", "middle"] {
        fixture.install(name, &prints(&format!("{name} 7.8.9")));
    }
    let mut env = fixture.env();
    env.answers = Some(Box::new(Cursor::new(b"n\n".to_vec())));

    for mode in [Mode::Check, Mode::Missing] {
        let (text, code) = run(&register, mode, false, &mut env);
        assert_eq!(code, 0, "{text}");
        assert!(!text.contains("[y/N]"), "{text}");
        assert!(!text.contains("Runtimes"), "{text}");
        assert!(text.contains("Nothing is missing.\n"), "{text}");
    }
    assert!(!fixture.path("cache").join("runtimes").exists());

    // `--pinned` installs them, and that needs the runtime.
    let (text, code) = run(&register, Mode::Pinned, false, &mut env);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("These runtimes are missing, and programs need them:\n  rt 4.5.6"),
        "{text}"
    );
    assert!(text.ends_with("[y/N] Nothing was installed.\n"), "{text}");
}

#[test]
fn pinned_programs_installed_with_pip_need_no_runtime() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_python(&fixture);
    // Installed with a Python of the computer's that the `PATH` then loses,
    // while the environment keeps it.
    executable(&fixture.path("elsewhere/py"), PYTHON.as_bytes());
    let mut env = fixture.env();
    env.search_path = std::env::join_paths([fixture.path("elsewhere"), fixture.path("bin")])
        .expect("a search path");
    let (text, code) = run(&register, Mode::Missing, false, &mut env);
    assert_eq!(code, 0, "{text}");
    assert!(!fixture.path("cache/runtimes").exists(), "{text}");
    let mut env = fixture.env();
    env.answers = Some(Box::new(Cursor::new(b"n\n".to_vec())));

    for mode in [Mode::Check, Mode::Missing, Mode::Pinned] {
        let (text, code) = run(&register, mode, false, &mut env);
        assert_eq!(code, 0, "{text}");
        assert!(!text.contains("[y/N]"), "{text}");
        assert!(!text.contains("Runtimes"), "{text}");
        assert!(text.contains("Nothing is missing.\n"), "{text}");
        assert!(!text.contains("Installing"), "{text}");
    }
}

#[test]
fn pinned_programs_installed_with_npm_start_through_their_runtime() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_a_runtime(&fixture, NPM);
    let (text, code) = run(&register, Mode::Pinned, true, &mut fixture.env());
    assert_eq!(code, 0, "{text}");
    fs::remove_dir_all(fixture.path("cache/runtimes")).expect("the pinned runtime");

    let (text, code) = run(&register, Mode::Check, false, &mut fixture.env());
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("Runtimes\n  rt 4.5.6: missing\n"), "{text}");
    assert!(text.contains("Missing: rt 4.5.6.\n"), "{text}");
}

#[test]
fn a_runtime_that_can_run_the_programs_is_used_whatever_its_version() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_a_runtime_with_minimums(&fixture);
    // Older than the pinned 4.5.6, and new enough for every program.
    let rt = fixture.install("rt", &prints("rt 4.2.0"));
    fixture.install("npm", NPM);

    for (mode, with_runtimes) in [(Mode::Missing, false), (Mode::Pinned, true)] {
        let (text, code) = run(&register, mode, with_runtimes, &mut fixture.env());
        assert_eq!(code, 0, "{text}");
        assert!(!text.contains("Installing rt"), "{text}");
        assert!(
            text.contains(&format!(
                "  rt 4.5.6: on the computer, {}, version 4.2.0\n",
                rt.display()
            )),
            "{text}"
        );
        assert!(
            text.contains("Not the pinned version: rt 4.5.6 (the computer's: 4.2.0).\n"),
            "{text}"
        );
    }
    assert!(!fixture.path("cache/runtimes").exists());
}

#[test]
fn the_search_goes_on_past_a_runtime_that_is_too_old() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_a_runtime_with_minimums(&fixture);
    let older = fixture.path("older/rt");
    executable(&older, prints("rt 3.0.0").as_bytes());
    let mut env = fixture.env();
    env.search_path =
        std::env::join_paths([fixture.path("older"), fixture.path("bin")]).expect("a search path");

    // Two too old: the first is the one named.
    fixture.install("rt", &prints("rt 4.1.0"));
    let (text, code) = run(&register, Mode::Check, false, &mut env);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "  rt 4.5.6: on the computer, {}, version 3.0.0, too old",
            older.display()
        )),
        "{text}"
    );

    // One behind it that is new enough is used.
    let rt = fixture.install("rt", &prints("rt 5.0.0"));
    let (text, _) = run(&register, Mode::Check, false, &mut env);
    assert!(
        text.contains(&format!(
            "  rt 4.5.6: on the computer, {}, version 5.0.0\n",
            rt.display()
        )),
        "{text}"
    );
}

#[test]
fn a_package_on_a_runtime_too_old_and_not_installed_is_not_installed() {
    let _alone = alone();
    let fixture = Fixture::new();
    let mut entries = on_a_runtime_with_minimums(&fixture);
    // No build of the runtime for this computer.
    for runtime in entries.runtime.values_mut() {
        let build = runtime
            .download
            .remove(&Platform::LinuxX86_64)
            .expect("a build");
        runtime.download.insert(Platform::WindowsX86_64, build);
    }
    fixture.install("rt", &prints("rt 4.1.0"));
    fixture.install("npm", NPM);

    let (text, code) = run(&entries, Mode::Missing, true, &mut fixture.env());

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("rt 4.5.6 was not installed: the register has no pinned build"),
        "{text}"
    );
    assert!(
        text.contains(
            "late 7.8.9 was not installed: it needs rt 4.2.0 or later, and the computer's is \
             4.1.0\n"
        ),
        "{text}"
    );
    // `early` runs on 4.1.0, so it was installed with the computer's.
    assert!(
        fixture
            .path("cache/programs/early/7.8.9/node_modules/.bin/early")
            .is_file(),
        "{text}"
    );
}

#[test]
fn a_runtime_on_the_computer_installs_the_package() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_a_runtime(&fixture, NPM);
    fixture.install("rt", &prints("rt 5.0.0"));
    fixture.install("npm", NPM);

    let (text, code) = run(&register, Mode::Missing, false, &mut fixture.env());

    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("Installing rt"), "{text}");
    assert!(
        text.contains("Not the pinned version: rt 4.5.6 (the computer's: 5.0.0).\n"),
        "{text}"
    );
    assert!(
        fixture
            .path("cache/programs/pkg/7.8.9/node_modules/.bin/pkg")
            .is_file()
    );
}

#[test]
fn an_installer_that_fails_or_leaves_nothing_is_reported() {
    let _alone = alone();
    let fixture = Fixture::new();
    let failing = on_a_runtime(&fixture, "#!/bin/sh\necho 'no registry here' >&2\nexit 1\n");
    let (text, code) = run(&failing, Mode::Missing, true, &mut fixture.env());
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(
            "pkg 7.8.9 was not installed: npm failed (exit status: 1): no registry here\n"
        ),
        "{text}"
    );
    assert!(!fixture.path("cache/programs/pkg/7.8.9").exists());

    let fixture = Fixture::new();
    let idle = on_a_runtime(&fixture, "#!/bin/sh\nexit 0\n");
    let (text, code) = run(&idle, Mode::Missing, true, &mut fixture.env());
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "pkg 7.8.9 was not installed: npm left no {}\n",
            fixture
                .path("cache/programs/pkg/7.8.9/node_modules/.bin/pkg")
                .display()
        )),
        "{text}"
    );

    // A runtime without npm beside it.
    let fixture = Fixture::new();
    let lonely = tar_gz(&[("rt/bin/rt", prints("rt 4.5.6").as_bytes())]);
    let register = register(
        &[
            check(&["pkg"]),
            packaged("pkg", "rt", &fixture.serve("pkg.tgz", b"p"), &sha256(b"p")),
            runtime(
                "rt",
                "npm",
                &fixture.serve("rt.tgz", &lonely),
                &sha256(&lonely),
                "rt/bin/rt",
            ),
        ]
        .concat(),
    );
    let (text, code) = run(&register, Mode::Missing, true, &mut fixture.env());
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("pkg 7.8.9 was not installed: npm cannot be started: "),
        "{text}"
    );
}

/// An npm that installs the first package file it is handed as a command,
/// and writes down the package files it was handed, in order.
const NPM_WRITES_DOWN: &str = r#"#!/bin/sh
prefix=""
files=""
while [ $# -gt 0 ]; do
  case "$1" in
    --prefix) prefix="$2"; shift 2 ;;
    install|--*) shift ;;
    *) files="$files ${1##*/}"; shift ;;
  esac
done
set -- $files
name="${1%.tgz}"
/bin/mkdir -p "$prefix/node_modules/.bin"
printf '#!/bin/sh\necho "%s 7.8.9"\n' "$name" > "$prefix/node_modules/.bin/$name"
/bin/chmod +x "$prefix/node_modules/.bin/$name"
echo "$files" > "$prefix/handed"
"#;

#[test]
fn a_package_is_installed_together_with_its_companions() {
    let _alone = alone();
    let fixture = Fixture::new();
    let archive = npm_runtime(NPM_WRITES_DOWN);
    let companion = |name: &str, bytes: &[u8], digest: &str| {
        format!(
            "[[program.pkg.companion]]\nname = \"{name}\"\nversion = \"1.0.0\"\n\
             url = \"{}\"\ndigest = \"{digest}\"\n",
            fixture.serve(&format!("{name}.tgz"), bytes)
        )
    };
    let entries = |second: String| {
        [
            check(&["pkg"]),
            packaged(
                "pkg",
                "rt",
                &fixture.serve("pkg.tgz", b"a package"),
                &sha256(b"a package"),
            ),
            companion("config", b"a config", &sha256(b"a config")),
            second,
            runtime(
                "rt",
                "npm",
                &fixture.serve("rt.tar.gz", &archive),
                &sha256(&archive),
                "rt/bin/rt",
            ),
        ]
        .concat()
    };

    let complete = register(&entries(companion(
        "syntax",
        b"a syntax",
        &sha512(b"a syntax"),
    )));
    let (text, code) = run(&complete, Mode::Missing, true, &mut fixture.env());
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        fs::read_to_string(fixture.path("cache/programs/pkg/7.8.9/handed")).expect("written"),
        " pkg.tgz config.tgz syntax.tgz\n"
    );

    // A companion with another digest stops the installation of its program.
    let other = Fixture::new();
    let refused = register(&entries(companion(
        "syntax",
        b"a syntax",
        &sha256(b"other"),
    )));
    let (text, code) = run(&refused, Mode::Missing, true, &mut other.env());
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("pkg 7.8.9 was not installed: the download from "),
        "{text}"
    );
    assert!(!other.path("cache/programs/pkg/7.8.9").exists(), "{text}");
}

#[test]
fn a_package_needs_its_runtime_and_its_digest() {
    let _alone = alone();
    let fixture = Fixture::new();
    let archive = npm_runtime(NPM);
    // A runtime with no build for this computer cannot be installed, and so
    // neither can the package on it.
    let mut entries = [
        check(&["pkg"]),
        packaged("pkg", "rt", &fixture.serve("pkg.tgz", b"p"), &sha256(b"p")),
        runtime(
            "rt",
            "npm",
            &fixture.serve("rt.tgz", &archive),
            &sha256(&archive),
            "rt/bin/rt",
        ),
    ]
    .concat();
    entries = entries.replace("download.linux-x86_64", "download.windows-x86_64");
    let unbuildable = register(&entries);

    let (text, code) = run(&unbuildable, Mode::Missing, true, &mut fixture.env());

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("  rt 4.5.6 (The rt runtime), for pkg: no build for linux-x86_64\n"),
        "{text}"
    );
    assert!(
        text.contains(
            "rt 4.5.6 was not installed: the register has no pinned build for linux-x86_64\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("pkg 7.8.9 was not installed: it needs rt, which is missing\n"),
        "{text}"
    );

    // A package with another digest is refused like any download.
    let fixture = Fixture::new();
    let archive = npm_runtime(NPM);
    let register = register(
        &[
            check(&["pkg"]),
            packaged("pkg", "rt", &fixture.serve("pkg.tgz", b"p"), &sha256(b"q")),
            runtime(
                "rt",
                "npm",
                &fixture.serve("rt.tgz", &archive),
                &sha256(&archive),
                "rt/bin/rt",
            ),
        ]
        .concat(),
    );
    let (text, code) = run(&register, Mode::Missing, true, &mut fixture.env());
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("pkg 7.8.9 was not installed: the download from"),
        "{text}"
    );
    assert!(text.contains("it was refused and deleted"), "{text}");
}

#[test]
fn a_virtual_environment_that_cannot_be_made_is_reported() {
    let _alone = alone();
    let fixture = Fixture::new();
    let python = tar_gz(&[("py/bin/py", b"#!/bin/sh\necho 'no venv' >&2\nexit 2\n")]);
    let register = register(
        &[
            check(&["wheel-tool"]),
            packaged(
                "wheel-tool",
                "py",
                &fixture.serve("w.whl", b"w"),
                &sha256(b"w"),
            ),
            runtime(
                "py",
                "pip",
                &fixture.serve("py.tgz", &python),
                &sha256(&python),
                "py/bin/py",
            ),
        ]
        .concat(),
    );

    let (text, code) = run(&register, Mode::Missing, true, &mut fixture.env());

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(
            "wheel-tool 7.8.9 was not installed: python -m venv failed (exit status: 2): no venv\n"
        ),
        "{text}"
    );
}

#[test]
fn locate_prefers_the_pinned_copy() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_a_runtime(&fixture, NPM);
    let pkg = ProgramName::new("pkg").expect("a name");

    assert_eq!(find::locate(&register, &pkg, &fixture.env()), None);
    assert_eq!(
        find::locate(
            &register,
            &ProgramName::new("unknown").expect("a name"),
            &fixture.env()
        ),
        None
    );

    let computer = fixture.install("pkg", &prints("pkg 7.0.0"));
    let rt = fixture.install("rt", &prints("rt 5.0.0"));
    fixture.install("npm", NPM);
    let found = find::locate(&register, &pkg, &fixture.env()).expect("found");
    assert_eq!(found.executable, computer);
    assert!(!found.pinned);
    assert_eq!(found.runtime, Some(rt));

    let (text, code) = run(&register, Mode::Pinned, true, &mut fixture.env());
    assert_eq!(code, 0, "{text}");
    let found = find::locate(&register, &pkg, &fixture.env()).expect("found");
    assert_eq!(
        found.executable,
        fixture.path("cache/programs/pkg/7.8.9/node_modules/.bin/pkg")
    );
    assert!(found.pinned);
    // The computer has the runtime, so none was pinned.
    assert_eq!(found.runtime, Some(fixture.path("bin/rt")));
}

#[test]
fn a_program_is_found_only_where_it_can_be_run() {
    let _alone = alone();
    let fixture = Fixture::new();
    let bin = fixture.path("bin");
    fs::write(bin.join("readable"), "#!/bin/sh\n").expect("a file");
    fs::create_dir_all(bin.join("folder")).expect("a folder");
    let windows = fixture.install("tool.cmd", "");

    assert_eq!(find::search("readable", bin.as_os_str(), false), None);
    assert_eq!(find::search("folder", bin.as_os_str(), false), None);
    assert_eq!(find::search("tool", bin.as_os_str(), false), None);
    assert_eq!(find::search("tool", bin.as_os_str(), true), Some(windows));

    // A folder that is no absolute path is never searched.
    let mut relative = OsString::from("bin:");
    relative.push(bin.as_os_str());
    assert_eq!(
        find::search("curl", &OsString::from("bin"), false),
        None,
        "relative to wherever the tool runs"
    );
    assert_eq!(
        find::search("curl", &relative, false),
        Some(bin.join("curl"))
    );
}

#[test]
fn a_version_is_three_numbers() {
    let _alone = alone();
    for (text, version) in [
        ("prettier 3.9.6\n", Some("3.9.6")),
        ("v24.22.3", Some("24.22.3")),
        ("jq-1.7.1", Some("1.7.1")),
        (
            "markdownlint-cli2 v0.23.2 (markdownlint v0.41.1)",
            Some("0.23.2"),
        ),
        ("Version: 2.5.15", Some("2.5.15")),
        ("1.2.3.4", Some("1.2.3")),
        ("...1.2..3 then 4.5.6.", Some("4.5.6")),
        ("1.2 only", None),
        ("", None),
    ] {
        assert_eq!(find::version_in(text).as_deref(), version, "{text:?}");
    }
}

#[test]
fn a_program_that_prints_no_version_is_said_to() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = register(
        &[
            check(&["mute"]),
            native("mute", "1.0.0", "file:///nowhere", &sha256(b""), "mute"),
        ]
        .concat(),
    );
    fixture.install("mute", "#!/nonexistent/interpreter\n");

    let (text, code) = run(&register, Mode::Check, false, &mut fixture.env());

    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains(&format!(
            "  mute 1.0.0 (json): on the computer, {}, which prints no version\n",
            fixture.path("bin/mute").display()
        )),
        "{text}"
    );
    assert!(
        text.contains("Not the pinned version: mute 1.0.0 (the computer's: unknown).\n"),
        "{text}"
    );
}

#[test]
fn the_cache_directory_follows_each_operating_system() {
    let _alone = alone();
    let vars = |name: &str| -> Option<OsString> {
        match name {
            "HOME" => Some("/home/someone".into()),
            "LOCALAPPDATA" => Some("/c/Users/someone/AppData/Local".into()),
            "XDG_CACHE_HOME" => Some("/var/cache/someone".into()),
            _ => None,
        }
    };
    assert_eq!(
        find::cache_home("macos", &vars),
        Some(PathBuf::from("/home/someone/Library/Caches"))
    );
    assert_eq!(
        find::cache_home("windows", &vars),
        Some(PathBuf::from("/c/Users/someone/AppData/Local"))
    );
    assert_eq!(
        find::cache_home("linux", &vars),
        Some(PathBuf::from("/var/cache/someone"))
    );
    let home_only = |name: &str| (name == "HOME").then(|| OsString::from("/home/someone"));
    assert_eq!(
        find::cache_home("linux", &home_only),
        Some(PathBuf::from("/home/someone/.cache"))
    );
    let relative = |_: &str| Some(OsString::from("relative"));
    assert_eq!(find::cache_home("linux", &relative), None);
    assert_eq!(find::cache_home("macos", &|_| None), None);
}

#[test]
fn windows_keeps_its_programs_elsewhere() {
    let _alone = alone();
    let dir = Path::new("/pinned");
    assert_eq!(
        find::installed_by(Installer::Npm, dir, "prettier", true),
        dir.join("node_modules/.bin/prettier.cmd")
    );
    assert_eq!(
        find::installed_by(Installer::Pip, dir, "yamllint", true),
        dir.join("Scripts/yamllint.exe")
    );
    assert_eq!(
        find::installed_by(Installer::Pip, dir, "yamllint", false),
        dir.join("bin/yamllint")
    );
    assert_eq!(
        find::below(dir, "package/biome.exe"),
        dir.join("package").join("biome.exe")
    );

    let fixture = Fixture::new();
    let register = register(&check(&[]));
    let mut env = fixture.env();
    env.windows = true;
    env.platform = Some(Platform::WindowsX86_64);
    let program = nebensound_repo::programs::Program {
        version: "1.0.0".to_owned(),
        command: "tool".to_owned(),
        version_args: vec![],
        runtime: Some(RuntimeName::new("absent").expect("a name")),
        runtime_minimum: Some("1.0.0".to_owned()),
        download: std::collections::BTreeMap::new(),
        companions: Vec::new(),
    };
    assert_eq!(
        find::program_executable(&register, &program, dir, &env),
        None
    );
}

#[test]
fn installers_find_their_runtime_first_on_the_path() {
    let _alone = alone();
    let fixture = Fixture::new();
    let runtime = fixture.path("runtime/bin/node");
    executable(&runtime, prints("v1.0.0").as_bytes());
    // An npm that writes the PATH it was given.
    executable(
        &fixture.path("runtime/bin/npm"),
        b"#!/bin/sh\nfor last; do :; done\nprintf '%s' \"$PATH\" > \"$last\"\n",
    );
    let out = fixture.path("path.txt");
    install::package(
        Installer::Npm,
        &runtime,
        std::slice::from_ref(&out),
        &fixture.path("dir"),
        &fixture.env(),
    )
    .expect("installed");
    assert_eq!(
        fs::read_to_string(&out).expect("written"),
        format!(
            "{}:{}",
            fixture.path("runtime/bin").display(),
            fixture.path("bin").display()
        )
    );

    let mut env = fixture.env();
    env.windows = true;
    let out = [out];
    let failure = install::package(Installer::Npm, &runtime, &out, &fixture.path("dir"), &env)
        .expect_err("there is no npm.cmd");
    assert!(failure.starts_with("npm cannot be started: "), "{failure}");
    let failure = install::package(Installer::Pip, &runtime, &out, &fixture.path("dir"), &env)
        .expect_err("node makes no virtual environment with a Windows layout");
    assert!(failure.starts_with("pip cannot be started: "), "{failure}");
}

#[test]
fn a_download_is_saved_under_a_safe_name_and_packed_as_its_name_says() {
    let _alone = alone();
    for (url, name) in [
        (
            "https://example.org/a/tool-1.0.0.tar.gz",
            "tool-1.0.0.tar.gz",
        ),
        (
            "https://example.org/a/cpython-3.12%2B1.tgz",
            "cpython-3.122B1.tgz",
        ),
        ("https://example.org/a/..", "download"),
        ("https://example.org/a/", "download"),
        ("plain", "plain"),
    ] {
        assert_eq!(fetch::file_name(url), name, "{url}");
    }
    for (url, packing) in [
        ("x.tar.gz", unpack::Packing::TarGz),
        ("x.TGZ", unpack::Packing::TarGz),
        ("x.gz", unpack::Packing::Gzip),
        ("x.zip", unpack::Packing::Zip),
        ("x.exe", unpack::Packing::Plain),
    ] {
        assert_eq!(unpack::Packing::of(url), packing, "{url}");
    }
}

#[test]
fn a_file_that_cannot_be_read_has_no_digest() {
    let _alone = alone();
    let fixture = Fixture::new();
    let nothing = fixture.path("nothing");
    assert!(fetch::digest_of(&nothing, Algorithm::Sha256).is_err());
    let failure = unpack::unpack(&nothing, unpack::Packing::Plain, &fixture.path("dir"), "x")
        .expect_err("nothing to unpack");
    assert!(failure.contains("cannot be read"), "{failure}");
}

#[test]
fn the_checks_of_a_plan_are_named_once() {
    let _alone = alone();
    let analysis = support::analysis(
        "[repo]\naudience = \"public\"\npipeline = \"rust-workspace\"\n",
        &["README.md", "src/lib.rs"],
    );
    let plan = nebensound_repo::plan::plan(&analysis, &support::central());
    let names: Vec<String> = setup::started(&plan)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(names, ["json", "markdown", "rust", "toml", "yaml"]);
}

#[test]
fn the_call_refuses_a_contradiction_and_a_broken_declaration() {
    let _alone = alone();
    let (text, code) = call(&["nebensound-repo", "setup", "--check", "--pinned"]);
    assert_eq!(code, 2, "{text}");
    let (text, code) = call(&["nebensound-repo", "setup", "--check", "--with-runtimes"]);
    assert_eq!(code, 2, "{text}");

    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("src/main.rs", "fn main() {}\n"),
    ]);
    let root = dir.path().to_str().expect("UTF-8");
    let (text, code) = call(&["nebensound-repo", "-C", root, "setup"]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(
        text,
        "there is nothing to set up: phase 0 found errors, and `check` says which\n"
    );
}

/// A folder holding only git, for a computer that has nothing else.
fn only_git(fixture: &Fixture) -> PathBuf {
    let git = find::search("git", &std::env::var_os("PATH").unwrap_or_default(), false)
        .expect("git is installed");
    let dir = fixture.path("git-only");
    fs::create_dir_all(&dir).expect("a folder");
    std::os::unix::fs::symlink(git, dir.join("git")).expect("git on the path");
    dir
}

#[test]
fn the_binary_checks_this_computer() {
    let _alone = alone();
    let fixture = Fixture::new();
    let repo = repository(&[(".nebensound/repo.toml", QUIET), ("README.md", "# x\n")]);
    let home = fixture.path("home");
    fs::create_dir_all(&home).expect("a home");

    let output = Command::new(env!("CARGO_BIN_EXE_nebensound-repo"))
        .args(["setup", "--check"])
        .current_dir(repo.path())
        .env("PATH", only_git(&fixture))
        .env("HOME", &home)
        .env_remove("XDG_CACHE_HOME")
        .output()
        .expect("the built binary");

    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(
        text.starts_with("Checks: json, markdown, toml, yaml\n"),
        "{text}"
    );
    assert!(
        text.contains(&format!("Folder: {}", home.display())),
        "{text}"
    );
    assert!(
        text.contains("  prettier 3.9.6 (json, markdown, yaml): missing\n"),
        "{text}"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_nebensound-repo"))
        .args(["setup", "--check"])
        .current_dir(repo.path())
        .env("PATH", only_git(&Fixture::new()))
        .env_remove("HOME")
        .env_remove("XDG_CACHE_HOME")
        .output()
        .expect("the built binary");
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("there is no cache directory"), "{text}");
}

#[test]
fn the_programs_do_not_depend_on_the_history() {
    let _alone = alone();
    // A unit job's checkout holds only the commit at hand: the history is an
    // error there, and it says nothing about which checks the plan starts.
    let git = Git::new();
    git.write(&[(".nebensound/repo.toml", QUIET), ("README.md", "# x\n")]);
    git.commit("chore: start");
    git.write(&[("README.md", "# x\n\nMore.\n")]);
    git.commit("docs: more");
    let fixture = Fixture::new();
    let shallow = fixture.path("shallow");
    git.run(&[
        "clone",
        "--quiet",
        "--depth",
        "1",
        &format!("file://{}", git.root()),
        shallow.to_str().expect("a path in UTF-8"),
    ]);
    let root = shallow.to_str().expect("a path in UTF-8");
    let (text, code) = call(&["nebensound-repo", "-C", root, "check"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("error[shallow-clone]"), "{text}");

    let home = fixture.path("home");
    fs::create_dir_all(&home).expect("a home");
    let output = Command::new(env!("CARGO_BIN_EXE_nebensound-repo"))
        .args(["setup", "--check"])
        .current_dir(&shallow)
        .env("PATH", only_git(&fixture))
        .env("HOME", &home)
        .env_remove("XDG_CACHE_HOME")
        .output()
        .expect("the built binary");
    let text = String::from_utf8_lossy(&output.stdout);
    // Missing programs, not a refusal.
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(
        text.starts_with("Checks: json, markdown, toml, yaml\n"),
        "{text}"
    );
    assert!(
        text.contains("  prettier 3.9.6 (json, markdown, yaml): missing\n"),
        "{text}"
    );
}

/// How long the tool may take to ask.
const PATIENCE: Duration = Duration::from_secs(30);

#[test]
fn in_a_terminal_the_binary_asks_before_it_installs_a_runtime() {
    let _alone = alone();
    let fixture = Fixture::new();
    let repo = repository(&[(".nebensound/repo.toml", QUIET), ("README.md", "# x\n")]);
    let home = fixture.path("home");
    fs::create_dir_all(&home).expect("a home");

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 200,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("a pseudo terminal");
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_nebensound-repo"));
    command.arg("setup");
    command.cwd(repo.path());
    command.env("PATH", only_git(&fixture));
    command.env("HOME", &home);
    command.env_remove("XDG_CACHE_HOME");
    // Under cargo-llvm-cov every process writes its own measurement.
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

    let screen = wait_for(&receiver, "[y/N]");
    assert!(
        screen.contains("node 24.22.3 (Node.js), for markdownlint-cli2, prettier"),
        "{screen}"
    );
    let mut writer = pair.master.take_writer().expect("write into it");
    writer.write_all(b"n\r").expect("typed");
    writer.flush().expect("sent");
    let screen = wait_for(&receiver, "Nothing was installed.");
    assert!(screen.contains("Nothing was installed."), "{screen}");

    let status = child.wait().expect("the tool ends");
    assert_eq!(status.exit_code(), 1, "{status:?}");
    assert!(!home.join(".cache/nebensound-repo/runtimes").exists());
    assert!(
        !home
            .join("Library/Caches/nebensound-repo/runtimes")
            .exists()
    );
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

#[test]
fn a_package_whose_folder_cannot_be_made_is_reported() {
    let _alone = alone();
    let fixture = Fixture::new();
    let register = on_a_runtime(&fixture, NPM);
    fs::write(fixture.path("cache/programs"), "").expect("a file");

    let (text, code) = run(&register, Mode::Missing, true, &mut fixture.env());

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("pkg 7.8.9 was not installed: "), "{text}");
    assert!(text.contains("cannot be made: "), "{text}");
    assert!(text.contains("  rt 4.5.6: pinned, "), "{text}");
}

#[test]
fn a_file_that_cannot_be_read_is_not_verified() {
    let _alone = alone();
    let fixture = Fixture::new();
    // A folder opens on Unix, and fails on the first read.
    let folder = fixture.path("server");
    for algorithm in [Algorithm::Sha256, Algorithm::Sha512] {
        assert!(fetch::digest_of(&folder, algorithm).is_err());
    }
    let digest = nebensound_repo::programs::Digest::new(&sha256(b"")).expect("a digest");
    let failure = fetch::verify(&folder, "file:///server", &digest).expect_err("unreadable");
    assert!(failure.contains("cannot be read"), "{failure}");
}

#[test]
fn locate_answers_without_a_cache_and_without_the_runtime_in_the_register() {
    let _alone = alone();
    let fixture = Fixture::new();
    let mut register = on_a_runtime(&fixture, NPM);
    let pkg = ProgramName::new("pkg").expect("a name");
    let computer = fixture.install("pkg", &prints("pkg 7.0.0"));
    let rt = fixture.install("rt", &prints("rt 5.0.0"));

    let mut env = fixture.env();
    env.folder = None;
    let found = find::locate(&register, &pkg, &env).expect("found");
    assert_eq!(
        (found.executable, found.runtime),
        (computer.clone(), Some(rt))
    );

    let mut env = fixture.env();
    env.platform = None;
    let found = find::locate(&register, &pkg, &env).expect("found");
    assert_eq!(found.executable, computer);

    register.runtime.clear();
    let found = find::locate(&register, &pkg, &fixture.env()).expect("found");
    assert_eq!((found.executable, found.runtime), (computer, None));
}

#[test]
fn setup_pinned_is_a_call_of_its_own() {
    let _alone = alone();
    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("src/main.rs", "fn main() {}\n"),
    ]);
    let root = dir.path().to_str().expect("UTF-8");
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root,
        "setup",
        "--pinned",
        "--with-runtimes",
    ]);
    assert_eq!(code, 1, "{text}");
    assert!(text.starts_with("there is nothing to set up"), "{text}");
}
