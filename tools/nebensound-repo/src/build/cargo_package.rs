//! The build `cargo-package` of one unit: the one Cargo package whose manifest
//! stands at the unit's path, packaged once with `cargo package --locked`, the
//! packaged file tested from its own contents outside the source tree, and
//! named for the action that hands it on as an artifact of the run.
//!
//! The steps run in this order, each in a group of the log, and the first that
//! fails ends the build:
//!
//! 1. **The toolchain the workspace pins**, installed through rustup from its
//!    `rust-toolchain.toml`, read here rather than handed in.
//! 2. **The package of the unit**: the one whose manifest is the unit's own
//!    `Cargo.toml`, as `cargo metadata` names it. A virtual workspace there has
//!    none, and packaging all of its members would hand on artifacts that
//!    belong to other units. The commit is the one the checkout holds, `HEAD`,
//!    which on a pull request is the merge commit GitHub builds for it.
//! 3. **The local packages it depends on**, directly or through each other,
//!    with all features: a path dependency is packaged as a dependency on the
//!    version the writer gave it, and that version is on no registry yet. Each
//!    is patched in from its folder for crates.io, so that Cargo finds it there
//!    and a stranger's crate of the same name cannot stand in for it. Only the
//!    ones it reaches, since a patch the workspace does not use changes its
//!    lockfile, and `--locked` refuses that.
//! 4. **The crate packaged**, without `--no-verify`: Cargo unpacks what it
//!    packed and builds that, so a file the package leaves out that the build
//!    needs fails here. `--allow-dirty`, since the version writer `cargo` ran
//!    before the build in the same job, and Git does not have the version it
//!    wrote.
//! 5. **The crate tested from its own contents**: the very file that is handed
//!    on, unpacked outside the source tree, where neither the workspace nor
//!    its `rust-toolchain.toml` reaches, and tested from what it holds. A file
//!    the package leaves out that a test needs fails here, and so does a
//!    dev-dependency without a version, which `cargo package` drops from the
//!    manifest. The toolchain is named, since the workspace's
//!    `rust-toolchain.toml` no longer applies.
//!
//! The artifact is named after the unit's path, its version and the commit it
//! was built from: `crate-<path>@<version>@<sha>`, the path percent-encoded
//! ([`encoded`]: `.` for the repository itself, `tools%2Ffoo` for
//! `tools/foo`). The encoding is reversible and never writes an `@`, so two
//! units cannot share a name, whoever takes the crate on finds a unit's
//! artifact with the pattern `crate-<path>@*`, and every character an artifact
//! name forbids stays out. It holds exactly one file,
//! `<package>-<version>.crate`. The step's outputs `artifact` and `crate` give
//! the name and the file.
//!
//! No cache: what is built here is the artifact a release hands on.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use flate2::read::GzDecoder;
use sha2::{Digest as _, Sha256};

use super::Context;
use crate::path::RelativePath;
use crate::run::rust::{Metadata, Package, patch, reached};
use crate::workflow::{self, Log};

/// The unit's path as the artifact's name carries it: every byte other than
/// a letter, a digit, `-`, `_`, `.` and `~` percent-encoded, with capital
/// hexadecimal digits, as jq's `@uri` writes it.
#[must_use]
pub fn encoded(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// The name of the artifact of the unit at `path`: see [`encoded`].
#[must_use]
pub fn artifact(path: &str, version: &str, commit: &str) -> String {
    format!("crate-{}@{version}@{commit}", encoded(path))
}

/// The package of the unit, as the build carries it from step to step.
struct Found {
    name: String,
    version: String,
    /// The file `cargo package` writes.
    crate_file: PathBuf,
    artifact: String,
}

/// Runs the build; whether it passed.
pub(super) fn run(context: &Context<'_>, unit: &RelativePath, log: &mut Log<'_>) -> bool {
    let dir = context.root.join(unit.to_string());
    steps(context, unit, &dir, log).is_some()
}

fn steps(context: &Context<'_>, unit: &RelativePath, dir: &Path, log: &mut Log<'_>) -> Option<()> {
    log.step("Install the toolchain the workspace pins", |log| {
        (call(context, log, dir, "rustup", &["toolchain", "install"])
            && call(context, log, dir, "rustc", &["--version"]))
        .then_some(())
    })?;
    let found = log.step("Find the package of the unit", |log| {
        package(context, unit, dir, log)
    })?;
    let patches = log.step("Find the local packages it depends on", |log| {
        local(context, dir, &found.name, log)
    })?;
    log.step("Package the crate", |log| {
        pack(context, dir, &found, &patches, log).then_some(())
    })?;
    log.step("Test the crate from its own contents", |log| {
        test(context, dir, &found, &patches, log).then_some(())
    })?;
    match workflow::output(
        context.variables,
        &[
            ("artifact", &found.artifact),
            ("crate", &found.crate_file.to_string_lossy()),
        ],
    ) {
        Ok(()) => Some(()),
        Err(message) => {
            log.error(&message);
            None
        }
    }
}

/// A command that starts `program` in `dir` with the search path of the run.
fn command(context: &Context<'_>, program: &str, dir: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(dir)
        .env("PATH", context.search_path)
        .stdin(Stdio::null());
    command
}

/// Runs `program` with these arguments in `dir`, its output in the log:
/// whether it succeeded.
fn call(
    context: &Context<'_>,
    log: &mut Log<'_>,
    dir: &Path,
    program: &str,
    args: &[&str],
) -> bool {
    let mut command = command(context, program, dir);
    command.args(args);
    log.stream(command, program)
}

/// What `cargo metadata` says with these arguments, or nothing, with why
/// written.
fn metadata(context: &Context<'_>, dir: &Path, extra: &str, log: &mut Log<'_>) -> Option<Metadata> {
    let mut command = command(context, "cargo", dir);
    command.args(["metadata", "--locked", "--format-version", "1", extra]);
    let printed = log.capture(command, "cargo metadata")?;
    let read = serde_json::from_slice::<Metadata>(&printed).ok();
    if read.is_none() {
        log.error("cargo metadata printed nothing the build can read.");
    }
    read
}

/// The package whose manifest is the unit's own `Cargo.toml`, and the name of
/// its artifact.
fn package(
    context: &Context<'_>,
    unit: &RelativePath,
    dir: &Path,
    log: &mut Log<'_>,
) -> Option<Found> {
    if !dir.join("Cargo.toml").is_file() {
        log.error(&format!(
            "'{unit}' builds cargo-package but holds no Cargo.toml."
        ));
        return None;
    }
    let metadata = metadata(context, dir, "--no-deps", log)?;
    let manifest = dir.join("Cargo.toml");
    let Some(package) = metadata
        .packages
        .iter()
        .find(|package: &&Package| package.manifest_path == manifest)
    else {
        log.error(&format!(
            "The Cargo.toml of '{unit}' is a workspace without a package of its own, so there is \
             no one crate to build."
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
    let artifact = artifact(&unit.to_string(), &package.version, &commit);
    log.line(&format!(
        "'{unit}' is the package {} {}; its artifact is '{artifact}'.",
        package.name, package.version
    ));
    Some(Found {
        name: package.name.clone(),
        version: package.version.clone(),
        crate_file: metadata
            .target_directory
            .join("package")
            .join(format!("{}-{}.crate", package.name, package.version)),
        artifact,
    })
}

/// The `--config` options that patch in every other local package the
/// package reaches, each from its folder.
fn local(context: &Context<'_>, dir: &Path, name: &str, log: &mut Log<'_>) -> Option<Vec<String>> {
    let metadata = metadata(context, dir, "--all-features", log)?;
    let mut patches = Vec::new();
    for (own, other, folder) in reached(&metadata, &[name.to_owned()]) {
        if !own {
            log.line(&format!("Patched in from its folder: {other} at {folder}"));
            patches.extend(["--config".to_owned(), patch(&other, &folder)]);
        }
    }
    if patches.is_empty() {
        log.line("It depends on no other local package.");
    }
    Some(patches)
}

/// The crate packaged once, and its digest named.
fn pack(
    context: &Context<'_>,
    dir: &Path,
    found: &Found,
    patches: &[String],
    log: &mut Log<'_>,
) -> bool {
    let mut command = command(context, "cargo", dir);
    command
        .args([
            "package",
            "--locked",
            "--allow-dirty",
            "--package",
            &found.name,
        ])
        .args(patches);
    if !log.stream(command, "cargo") {
        return false;
    }
    let shown = found.crate_file.display();
    match fs::read(&found.crate_file) {
        Ok(bytes) => {
            let digest = Sha256::digest(&bytes);
            let mut hex = String::new();
            for byte in digest {
                let _ = write!(hex, "{byte:02x}");
            }
            log.line(&format!("{hex}  {shown}"));
            true
        }
        Err(error) => {
            log.error(&format!(
                "cargo package passed, but '{shown}' is not there: {error}"
            ));
            false
        }
    }
}

/// The crate unpacked outside the source tree and tested from what it holds.
fn test(
    context: &Context<'_>,
    dir: &Path,
    found: &Found,
    patches: &[String],
    log: &mut Log<'_>,
) -> bool {
    let mut active = command(context, "rustup", dir);
    active.args(["show", "active-toolchain"]);
    let toolchain = log.capture(active, "rustup").and_then(|printed| {
        String::from_utf8_lossy(&printed)
            .split_whitespace()
            .next()
            .map(ToOwned::to_owned)
    });
    let Some(toolchain) = toolchain else {
        log.error(
            "rustup names no active toolchain for the workspace, so the crate cannot be tested.",
        );
        return false;
    };
    let destination = match tempfile::Builder::new()
        .prefix("nebensound-repo-build-")
        .tempdir_in(context.temp)
    {
        Ok(destination) => destination,
        Err(error) => {
            log.error(&format!(
                "no folder can be made in {} to unpack the crate into: {error}",
                context.temp.display()
            ));
            return false;
        }
    };
    let shown = found.crate_file.display();
    let unpacked = File::open(&found.crate_file)
        .and_then(|file| tar::Archive::new(GzDecoder::new(file)).unpack(destination.path()));
    if let Err(error) = unpacked {
        log.error(&format!("'{shown}' cannot be unpacked: {error}"));
        return false;
    }
    let folder = format!("{}-{}", found.name, found.version);
    let copy = destination.path().join(&folder);
    let Ok(manifest) = fs::read_to_string(copy.join("Cargo.toml")) else {
        log.error(&format!(
            "'{shown}' does not unpack to {folder}/Cargo.toml."
        ));
        return false;
    };
    if let Some(version) = manifest.lines().find(|line| line.starts_with("version = ")) {
        log.line(&format!("The packaged manifest names {version}."));
    }
    let mut command = command(context, "cargo", &copy);
    command
        .args([format!("+{toolchain}").as_str(), "test", "--all-features"])
        .args(patches);
    if log.stream(command, "cargo") {
        return true;
    }
    log.error(&format!(
        "The crate {} {} fails its tests from its own contents. A file its tests need may be left \
         out of the package, or a dev-dependency may have no version, which cargo package drops.",
        found.name, found.version
    ));
    false
}
