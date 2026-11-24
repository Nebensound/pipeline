//! The build `cargo-release` of one unit for one target: the one binary of the
//! Cargo package whose manifest stands at the unit's path, built in release
//! for the target of this machine, started, packed into an archive, and named
//! for the action that hands the archive on as an artifact of the run.
//!
//! The register lists the targets of the build, each with the runner it is
//! built on, and `declare` gives every target a job of its own on that runner.
//! So the target is the machine's own: the one target of the register whose
//! processor and operating system are this machine's ([`platform`]). The build
//! starts the binary it made, and only a machine of the target's platform can
//! do that.
//!
//! The steps run in this order, each in a group of the log, and the first that
//! fails ends the build:
//!
//! 1. **The toolchain the workspace pins, with the target**: the target of
//!    this machine named, then the toolchain installed through rustup from
//!    the workspace's `rust-toolchain.toml`, and the target added to it.
//! 2. **The binary of the unit**: the package whose manifest is the unit's own
//!    `Cargo.toml`, as `cargo metadata` names it, which has to have exactly one
//!    binary. A virtual workspace there has no package of its own, and a
//!    package with no binary or with several has no one binary to hand on;
//!    each fails by name. The version is the unit's version of the run, as the
//!    plan gives it, and the commit the one the checkout holds, `HEAD`.
//! 3. **The binary built**: `cargo build --release --locked` for the target,
//!    with the package's default features, the binary alone.
//! 4. **The binary started**: `<binary> --version` has to end within the time
//!    the machine gives it ([`Machine::answer`](super::Machine::answer)), with
//!    success, and print the version of the run as a word of its own: on a pull
//!    request the one its merge would get, with `-pr.<number>` behind it. The
//!    version writer `cargo` ran before the build in the same job, so the
//!    binary carries that version when it takes it from Cargo.
//! 5. **The archive**: `<binary>-<target>.tar.gz` beside the binary, holding
//!    the binary and the licence of the unit ([`licences`]), each under its
//!    own name at the top, written so that the same files give the same bytes
//!    ([`archive`]). Its SHA-256 goes into the log.
//!
//! The artifact is named after the unit's path, its version, the commit it
//! was built from and the target: `executable-<path>@<version>@<sha>@<target>`,
//! the path percent-encoded as for a crate ([`encoded`]). It holds exactly one
//! file, the archive. The step's outputs `artifact` and `archive` give the name
//! and the file.
//!
//! Every program runs in the unit's folder, under the rule of
//! [`crate::toolchain`], the binary as well: without the variables of this
//! process that steer Cargo, rustc and rustup.
//!
//! No cache: what is built here is the artifact a release hands on.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::Instant;

use flate2::{Compression, GzBuilder};

use super::cargo_package::{call, encoded, metadata, sha256};
use super::{Context, Machine};
use crate::path::RelativePath;
use crate::run::rust::Package;
use crate::workflow::{self, Log};

/// The name of the artifact of the unit at `path`: see [`encoded`].
#[must_use]
pub fn artifact(path: &str, version: &str, commit: &str, target: &str) -> String {
    format!("executable-{}@{version}@{commit}@{target}", encoded(path))
}

pub use crate::register::platform;

/// The one target among `targets` whose platform is the machine's, or why
/// there is none.
///
/// The register refuses two targets of one platform when it is read
/// ([`crate::register::Build::targets`]), so more than one is only possible
/// for targets that come from elsewhere.
///
/// # Errors
///
/// Returns a sentence when no target, or more than one, is for this machine.
pub fn target_for<'a>(
    targets: &'a BTreeMap<String, String>,
    machine: &Machine,
) -> Result<&'a str, String> {
    let this = (machine.arch.as_str(), machine.os.as_str());
    let fitting: Vec<&str> = targets
        .keys()
        .map(String::as_str)
        .filter(|target| platform(target) == Some(this))
        .collect();
    match fitting.as_slice() {
        [target] => Ok(target),
        [] => Err(format!(
            "cargo-release builds for {}, each on a machine of that platform, and this one is {} \
             on {}.",
            listed(targets.keys().map(String::as_str)),
            machine.os,
            machine.arch
        )),
        several => Err(format!(
            "The register lists more than one target of cargo-release for {} on {} ({}), so the \
             build cannot tell which to build.",
            machine.os,
            machine.arch,
            several.join(", ")
        )),
    }
}

/// Names joined by commas, or `no target` when there are none.
fn listed<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let names: Vec<&str> = names.collect();
    if names.is_empty() {
        "no target".to_owned()
    } else {
        names.join(", ")
    }
}

/// Whether a file name is that of a licence: it starts with `LICENSE`,
/// `LICENCE` or `COPYING`, in any case.
fn licence(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    ["LICENSE", "LICENCE", "COPYING"]
        .iter()
        .any(|start| name.starts_with(start))
}

/// The licence of the unit at `unit`: the files whose names are those of a
/// licence, in the first folder from the unit's own up to the repository root
/// that holds any, in the order of their names. Empty when none does.
#[must_use]
pub fn licences(root: &Path, unit: &RelativePath) -> Vec<PathBuf> {
    let mut folder = root.to_path_buf();
    let mut folders = vec![folder.clone()];
    for part in unit.to_string().split('/').filter(|part| *part != ".") {
        folder = folder.join(part);
        folders.push(folder.clone());
    }
    folders
        .iter()
        .rev()
        .map(|folder| {
            let mut found: Vec<PathBuf> = fs::read_dir(folder)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_file()
                        && path
                            .file_name()
                            .is_some_and(|name| licence(&name.to_string_lossy()))
                })
                .collect();
            found.sort();
            found
        })
        .find(|found| !found.is_empty())
        .unwrap_or_default()
}

/// One file of an archive: its name at the top of the archive, its contents
/// and its mode.
pub type Entry = (String, Vec<u8>, u32);

/// A tar archive of these files, compressed with gzip, the same bytes for the
/// same files: each a plain file in the order given, with the modification
/// time 0, the owner and group 0 without names, in the ustar format, and the
/// gzip header without a time, a file name or an operating system.
///
/// # Errors
///
/// Returns the error of the archive, such as a name tar refuses.
pub fn archive(entries: &[Entry]) -> io::Result<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    entries
        .iter()
        .try_for_each(|(name, bytes, mode)| {
            let mut header = tar::Header::new_ustar();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_size(bytes.len() as u64);
            header.set_mode(*mode);
            header.set_mtime(0);
            header.set_uid(0);
            header.set_gid(0);
            builder.append_data(&mut header, name, bytes.as_slice())
        })
        .and_then(|()| builder.into_inner())
        .and_then(|tar| {
            let mut encoder = GzBuilder::new()
                .mtime(0)
                .operating_system(255)
                .write(Vec::new(), Compression::best());
            encoder.write_all(&tar).and_then(|()| encoder.finish())
        })
}

/// The binary of the unit, as the build carries it from step to step.
struct Found {
    name: String,
    binary: String,
    version: String,
    /// The binary Cargo builds.
    built: PathBuf,
    /// The archive the build writes.
    archive: PathBuf,
    artifact: String,
}

/// Runs the build; whether it passed.
pub(super) fn run(context: &Context<'_>, unit: &RelativePath, log: &mut Log<'_>) -> bool {
    let dir = context.root.join(unit.to_string());
    steps(context, unit, &dir, log).is_some()
}

fn steps(context: &Context<'_>, unit: &RelativePath, dir: &Path, log: &mut Log<'_>) -> Option<()> {
    let target = log.step(
        "Install the toolchain the workspace pins, with the target",
        |log| toolchain(context, dir, log),
    )?;
    let found = log.step("Find the binary of the unit", |log| {
        binary(context, unit, dir, &target, log)
    })?;
    log.step("Build the binary", |log| {
        call(
            context,
            log,
            dir,
            "cargo",
            &[
                "build",
                "--release",
                "--locked",
                "--target",
                &target,
                "--package",
                &found.name,
                "--bin",
                &found.binary,
            ],
        )
        .then_some(())
    })?;
    log.step("Start the binary", |log| {
        started(context, dir, &found, log).then_some(())
    })?;
    log.step("Pack the archive", |log| {
        pack(context, unit, &found, &target, log).then_some(())
    })?;
    match workflow::output(
        context.variables,
        &[
            ("artifact", &found.artifact),
            ("archive", &found.archive.to_string_lossy()),
        ],
    ) {
        Ok(()) => Some(()),
        Err(message) => {
            log.error(&message);
            None
        }
    }
}

/// The target of this machine, and the toolchain the workspace pins installed
/// with it.
fn toolchain(context: &Context<'_>, dir: &Path, log: &mut Log<'_>) -> Option<String> {
    let targets = context
        .register
        .build
        .iter()
        .find(|(name, _)| name.as_str() == super::Build::CargoRelease.name())
        .map(|(_, build)| build.targets.clone())
        .unwrap_or_default();
    let target = match target_for(&targets, context.machine) {
        Ok(target) => target.to_owned(),
        Err(message) => {
            log.error(&message);
            return None;
        }
    };
    log.line(&format!(
        "This machine is {} on {}, so the build is for {target}.",
        context.machine.os, context.machine.arch
    ));
    (call(context, log, dir, "rustup", &["toolchain", "install"])
        && call(context, log, dir, "rustup", &["target", "add", &target])
        && call(context, log, dir, "rustc", &["--version"]))
    .then_some(target)
}

/// The package whose manifest is the unit's own `Cargo.toml`, its one binary,
/// and the name of the artifact.
fn binary(
    context: &Context<'_>,
    unit: &RelativePath,
    dir: &Path,
    target: &str,
    log: &mut Log<'_>,
) -> Option<Found> {
    if !dir.join("Cargo.toml").is_file() {
        log.error(&format!(
            "'{unit}' builds cargo-release but holds no Cargo.toml."
        ));
        return None;
    }
    let metadata = metadata(context, dir, "--no-deps", log)?;
    let manifest = dir.join("Cargo.toml");
    let binaries = |package: &Package| -> Vec<String> {
        let mut names: Vec<String> = package
            .targets
            .iter()
            .filter(|target| target.kind.iter().any(|kind| kind == "bin"))
            .map(|target| target.name.clone())
            .collect();
        names.sort();
        names
    };
    let Some(package) = metadata
        .packages
        .iter()
        .find(|package: &&Package| package.manifest_path == manifest)
    else {
        let mut with: Vec<&str> = metadata
            .packages
            .iter()
            .filter(|package| !binaries(package).is_empty())
            .map(|package| package.name.as_str())
            .collect();
        with.sort_unstable();
        log.error(&format!(
            "The Cargo.toml of '{unit}' is a workspace without a package of its own, so there is \
             no one binary to build; its packages with a binary: {}.",
            if with.is_empty() {
                "none".to_owned()
            } else {
                with.join(", ")
            }
        ));
        return None;
    };
    let mut found = binaries(package);
    if found.len() != 1 {
        log.error(&format!(
            "The package {} of '{unit}' has {} binaries ({}), and cargo-release builds exactly one.",
            package.name,
            found.len(),
            if found.is_empty() {
                "none".to_owned()
            } else {
                found.join(", ")
            }
        ));
        return None;
    }
    let binary = found.remove(0);
    let Some(version) = context.version else {
        log.error(&format!(
            "The plan gives '{unit}' no version of this run, so there is no version for the \
             binary to print."
        ));
        return None;
    };
    let commit = match context.repository.head() {
        Ok(commit) => commit,
        Err(error) => {
            log.error(&format!("The commit of the build is not known: {error}"));
            return None;
        }
    };
    let artifact = artifact(&unit.to_string(), version, &commit, target);
    log.line(&format!(
        "'{unit}' is the binary {binary} of the package {} at {version}; its artifact is \
         '{artifact}'.",
        package.name
    ));
    let release = metadata.target_directory.join(target).join("release");
    Some(Found {
        name: package.name.clone(),
        archive: release.join(format!("{binary}-{target}.tar.gz")),
        built: release.join(&binary),
        binary,
        version: version.to_owned(),
        artifact,
    })
}

/// The binary started with `--version`: whether it ended in time, with
/// success, and printed the version of the run as a word of its own.
fn started(context: &Context<'_>, dir: &Path, found: &Found, log: &mut Log<'_>) -> bool {
    let printed = match tempfile::Builder::new()
        .prefix("nebensound-repo-version-")
        .tempfile_in(context.temp)
    {
        Ok(printed) => printed,
        Err(error) => {
            log.error(&format!(
                "no file can be made in {} for what the binary prints: {error}",
                context.temp.display()
            ));
            return false;
        }
    };
    let shown = found.built.display().to_string();
    log.line(&format!("> {shown} --version"));
    let mut command = crate::toolchain::command(&found.built, dir, context.search_path);
    command.arg("--version");
    let spawned = printed
        .as_file()
        .try_clone()
        .and_then(|out| printed.as_file().try_clone().map(|err| (out, err)))
        .and_then(|(out, err)| command.stdout(out).stderr(err).spawn());
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            log.error(&format!("'{shown}' cannot be started: {error}"));
            return false;
        }
    };
    let machine = context.machine;
    let deadline = Instant::now() + machine.answer;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            _ if Instant::now() < deadline => std::thread::sleep(machine.interval),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let text = fs::read(printed.path())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    log.lines(&text);
    let binary = &found.binary;
    match status {
        None => log.error(&format!(
            "'{binary} --version' did not end within {:?}.",
            machine.answer
        )),
        Some(status) if !status.success() => {
            log.error(&format!("'{binary} --version' failed ({status})."));
        }
        Some(_) if !text.split_whitespace().any(|word| word == found.version) => {
            log.error(&format!(
                "'{binary} --version' does not print the version of this run, {}.",
                found.version
            ));
        }
        Some(_) => return true,
    }
    false
}

/// The archive of the binary and the licence written, and its digest named.
fn pack(
    context: &Context<'_>,
    unit: &RelativePath,
    found: &Found,
    target: &str,
    log: &mut Log<'_>,
) -> bool {
    let licences = licences(context.root, unit);
    if licences.is_empty() {
        log.error(&format!(
            "Neither '{unit}' nor a folder above it holds a licence, a file whose name starts \
             with LICENSE, LICENCE or COPYING, to put beside the binary."
        ));
        return false;
    }
    let mut files = vec![(found.binary.clone(), found.built.clone(), 0o755)];
    for licence in licences {
        let name = licence
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        log.line(&format!("Beside the binary: {}", licence.display()));
        files.push((name, licence, 0o644));
    }
    let shown = found.archive.display();
    let written = files
        .into_iter()
        .map(|(name, path, mode)| fs::read(&path).map(|bytes| (name, bytes, mode)))
        .collect::<io::Result<Vec<Entry>>>()
        .and_then(|entries| archive(&entries))
        .and_then(|bytes| {
            File::create(&found.archive)
                .and_then(|mut file| file.write_all(&bytes))
                .map(|()| bytes)
        });
    match written {
        Ok(bytes) => {
            log.line(&format!("{}  {shown}", sha256(&bytes)));
            log.line(&format!(
                "The archive for {target} holds the binary {} and the licence.",
                found.binary
            ));
            true
        }
        Err(error) => {
            log.error(&format!("'{shown}' cannot be written: {error}"));
            false
        }
    }
}
