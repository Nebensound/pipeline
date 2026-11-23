//! The Rust check over one Cargo workspace, the unit's path: formatting,
//! Clippy, tests in every feature mode, documentation, a release build, the
//! oldest supported Rust version, full coverage, and every published package
//! built and tested from its own contents.
//!
//! Unlike the other checks it runs code of the repository, and it runs it
//! with the Rust toolchain rustup provides: the one the workspace pins in its
//! `rust-toolchain.toml`, with rustfmt and Clippy, the oldest `rust-version`
//! its packages claim, and the nightly toolchain the register pins for
//! coverage, the only place where `#[coverage(off)]` takes effect. The check
//! installs each of them through rustup before it uses it, on a developer's
//! computer as in a workflow run. cargo-llvm-cov is the program the register
//! pins for the check, found as for every other check. None of this is an
//! input, so no repository can tell the check to look away.
//!
//! The steps run in this order, each in a group of the log, and the first
//! that fails ends the check: what comes after a failed build would only
//! repeat its error. Every program runs in the workspace's folder, without
//! the variables of this process that steer Cargo, rustc and rustup
//! ([`steers`]), so that no setting that comes as a variable changes what the
//! check finds. Settings that come as files still apply: a directory override
//! rustup keeps in its settings file, which ranks above `rust-toolchain.toml`,
//! and Cargo's configuration files in the folders above the workspace and in
//! `CARGO_HOME`. A hosted runner has neither.
//!
//! The files are the ones `files` names for the check and the unit. Rust code
//! is excluded by the package, never by the file; phase 0 fails on an
//! `[[exclude]]` entry that takes out part of a package. A package
//! `[[exclude]]` takes out as a whole is out of every step: not formatted, not
//! linted, not tested, not documented, not counted for coverage. It is only
//! compiled where another package depends on it, and rustc still prints its
//! own warnings for it then, which fail nothing. That holds for a workspace
//! member and for a path dependency outside the workspace alike. Each package
//! is judged by its own manifest and code, so a package inside an excluded one
//! that the declaration keeps in with an `[[include]]` stays in.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use super::{Session, Tool, chunks, has_extension};
use crate::path::RelativePath;
use crate::writer;

/// The variables that start like those that steer Cargo, rustc and rustup
/// and are kept all the same: they say where Cargo and the toolchains are,
/// not what they do.
pub const KEPT: &[&str] = &["CARGO_HOME", "RUSTUP_HOME"];

/// The option that keeps rustfmt from passing a file it reaches unchecked:
/// `disable_all_formatting` in the repository's configuration would, and the
/// command line wins over the configuration.
const FORMAT_ALL: [&str; 2] = ["--config", "disable_all_formatting=false"];

/// Whether a variable of this process steers Cargo, rustc, rustdoc or rustup
/// and is taken out of what the programs of the check inherit: every one whose
/// name starts with `CARGO` or `RUST`, such as `RUSTFLAGS`,
/// `CARGO_TARGET_DIR`, `RUSTUP_TOOLCHAIN` or `CARGO_INCREMENTAL`, less
/// [`KEPT`]. The check sets what it needs itself.
#[must_use]
pub fn steers(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    (name.starts_with("CARGO") || name.starts_with("RUST")) && !KEPT.contains(&name.as_ref())
}

/// What `cargo metadata` prints, as far as the check reads it.
#[derive(Debug, Clone, Deserialize)]
pub struct Metadata {
    /// Every package of the build: the local ones and their dependencies.
    pub packages: Vec<Package>,
    /// The IDs of the workspace's members.
    pub workspace_members: Vec<String>,
    /// Where Cargo builds.
    pub target_directory: PathBuf,
    /// How the packages depend on each other.
    pub resolve: Option<Resolve>,
}

/// One package `cargo metadata` names.
#[derive(Debug, Clone, Deserialize)]
pub struct Package {
    /// Its ID.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// Where it comes from; [`None`] for a local package.
    pub source: Option<String>,
    /// Its manifest, as an absolute path.
    pub manifest_path: PathBuf,
    /// Its targets.
    pub targets: Vec<Target>,
    /// The registries it may be published to: [`None`] for every one, empty
    /// for none.
    pub publish: Option<Vec<String>>,
    /// The oldest Rust version it claims to build with.
    pub rust_version: Option<String>,
}

/// One target of a package.
#[derive(Debug, Clone, Deserialize)]
pub struct Target {
    /// Its root file, as an absolute path.
    pub src_path: PathBuf,
    /// The edition it is written in.
    pub edition: String,
}

/// How the packages depend on each other.
#[derive(Debug, Clone, Deserialize)]
pub struct Resolve {
    /// One node per package.
    pub nodes: Vec<Node>,
}

/// What one package depends on.
#[derive(Debug, Clone, Deserialize)]
pub struct Node {
    /// The package.
    pub id: String,
    /// The IDs of the packages it depends on.
    pub dependencies: Vec<String>,
}

/// A local package of the workspace, a member or a path dependency wherever
/// it lies, held against the packages `[[exclude]]` takes out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Local {
    /// Its name.
    pub name: String,
    /// The folder of its manifest, from the repository root, `.` for the
    /// root; an absolute path for one outside the repository.
    pub dir: String,
    /// Whether it is a member of the workspace.
    pub member: bool,
    /// The folders of the local packages inside it.
    pub nested: Vec<String>,
    /// Why `[[exclude]]` takes it out, if it does.
    pub reason: Option<String>,
    /// The root file of each of its targets, with the target's edition.
    pub targets: Vec<(String, String)>,
    /// The oldest Rust version it claims to build with.
    pub rust_version: Option<String>,
}

impl Local {
    /// Whether every step checks it: a member that is not out.
    #[must_use]
    pub fn checked(&self) -> bool {
        self.member && self.reason.is_none()
    }

    /// Whether it is only formatted: a path dependency that is no member
    /// and not out, as `cargo fmt --all` reaches it.
    #[must_use]
    pub fn formatted_only(&self) -> bool {
        !self.member && self.reason.is_none()
    }
}

/// The folder of a manifest from the repository root: `.` for the root
/// itself, the absolute folder for one outside it.
fn folder_of(root: &Path, manifest: &Path) -> String {
    let dir = manifest.parent().unwrap_or(manifest);
    dir.strip_prefix(root).map_or_else(
        |_| dir.to_string_lossy().into_owned(),
        |inside| {
            let parts: Vec<String> = inside
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            if parts.is_empty() {
                ".".to_owned()
            } else {
                parts.join("/")
            }
        },
    )
}

/// Whether `file` lies below the folder `dir`, both from the repository root.
fn below(dir: &str, file: &str) -> bool {
    dir == "." || file.starts_with(&format!("{dir}/"))
}

/// The local packages of the metadata, in its order, each held against the
/// packages `[[exclude]]` takes out (`excluded`, folder and reason) by its
/// folder: a package is out when its folder is named.
#[must_use]
pub fn locals(metadata: &Metadata, root: &Path, excluded: &[(String, String)]) -> Vec<Local> {
    let local: Vec<&Package> = metadata
        .packages
        .iter()
        .filter(|package| package.source.is_none())
        .collect();
    let dirs: Vec<String> = local
        .iter()
        .map(|package| folder_of(root, &package.manifest_path))
        .collect();
    local
        .iter()
        .zip(&dirs)
        .map(|(package, dir)| Local {
            name: package.name.clone(),
            dir: dir.clone(),
            member: metadata.workspace_members.contains(&package.id),
            nested: dirs
                .iter()
                .filter(|other| *other != dir && below(dir, other))
                .cloned()
                .collect(),
            reason: excluded
                .iter()
                .find(|(path, _)| path == dir)
                .map(|(_, reason)| reason.clone()),
            targets: package
                .targets
                .iter()
                .map(|target| {
                    (
                        target.edition.clone(),
                        target.src_path.to_string_lossy().into_owned(),
                    )
                })
                .collect(),
            rust_version: package.rust_version.clone(),
        })
        .collect()
}

/// The numbers of a Rust version, for comparing: `1.70` before `1.100`.
#[must_use]
pub fn numbers(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// The oldest `rust-version` a checked package claims, if any claims one.
#[must_use]
pub fn oldest(locals: &[Local]) -> Option<&str> {
    locals
        .iter()
        .filter(|local| local.checked())
        .filter_map(|local| local.rust_version.as_deref())
        .min_by_key(|version| numbers(version))
}

/// What coverage leaves uncounted, as the regular expression
/// `--ignore-filename-regex` takes, or [`None`] when nothing is left out.
///
/// llvm-cov names every file by its absolute path, so each excluded package's
/// folder is matched as exactly that, from the start. Where another local
/// package lies inside the excluded one, its folder would take that package's
/// files along, so the excluded package's own `.rs` files are matched one by
/// one instead: those of `tracked` in its folder and in none of the packages
/// inside it.
#[must_use]
pub fn uncounted(root: &Path, locals: &[Local], tracked: &[RelativePath]) -> Option<String> {
    let root = root.to_string_lossy();
    let parts: Vec<String> = locals
        .iter()
        .filter(|local| local.reason.is_some())
        .flat_map(|local| {
            if local.nested.is_empty() {
                let dir = if local.dir == "." {
                    root.to_string()
                } else {
                    format!("{root}/{}", local.dir)
                };
                vec![format!("{}/", regex::escape(&dir))]
            } else {
                tracked
                    .iter()
                    .map(ToString::to_string)
                    .filter(|file| {
                        has_extension(file, "rs")
                            && below(&local.dir, file)
                            && !local.nested.iter().any(|dir| below(dir, file))
                    })
                    .map(|file| format!("{}$", regex::escape(&format!("{root}/{file}"))))
                    .collect()
            }
        })
        .collect();
    (!parts.is_empty()).then(|| format!("^({})", parts.join("|")))
}

/// The local packages that the packages named `packaged` reach through their
/// dependencies, directly or through each other, themselves included: each
/// with whether it is one of them, its name and its folder as an absolute
/// path.
#[must_use]
pub fn reached(metadata: &Metadata, packaged: &[String]) -> Vec<(bool, String, String)> {
    let local: Vec<&Package> = metadata
        .packages
        .iter()
        .filter(|package| package.source.is_none())
        .collect();
    let ids: BTreeSet<&str> = local.iter().map(|package| package.id.as_str()).collect();
    let nodes = metadata
        .resolve
        .as_ref()
        .map_or(&[][..], |resolve| &resolve.nodes);
    let mut reached: BTreeSet<&str> = local
        .iter()
        .filter(|package| packaged.contains(&package.name))
        .map(|package| package.id.as_str())
        .collect();
    loop {
        let next: BTreeSet<&str> = nodes
            .iter()
            .filter(|node| reached.contains(node.id.as_str()))
            .flat_map(|node| node.dependencies.iter().map(String::as_str))
            .filter(|id| ids.contains(id))
            .chain(reached.iter().copied())
            .collect();
        if next.len() == reached.len() {
            break;
        }
        reached = next;
    }
    local
        .iter()
        .filter(|package| reached.contains(package.id.as_str()))
        .map(|package| {
            let dir = package
                .manifest_path
                .parent()
                .unwrap_or(&package.manifest_path);
            (
                packaged.contains(&package.name),
                package.name.clone(),
                dir.to_string_lossy().into_owned(),
            )
        })
        .collect()
}

/// The option that lays the package `name` from the folder `dir` over
/// crates.io, as Cargo's `--config` takes it.
#[must_use]
pub fn patch(name: &str, dir: &str) -> String {
    format!(
        "patch.crates-io.{name}.path={}",
        toml::Value::String(dir.to_owned())
    )
}

/// Runs the check; whether it found nothing.
pub(super) fn run(session: &mut Session<'_, '_>) -> bool {
    let dir = session.context.root.join(session.unit.to_string());
    steps(session, &dir).is_some()
}

/// The steps, in their order, up to the first that fails.
fn steps(session: &mut Session<'_, '_>, dir: &Path) -> Option<()> {
    step(session, "Install the toolchain the workspace pins", |s| {
        toolchain(s, dir).then_some(())
    })?;
    let locals = step(session, "Find the packages", |s| packages(s, dir))?;
    if locals.iter().any(|local| local.reason.is_none()) {
        step(session, "Formatting", |s| {
            formatting(s, dir, &locals).then_some(())
        })?;
    }
    if !locals.iter().any(Local::checked) {
        return Some(());
    }
    let without: Vec<String> = locals
        .iter()
        .filter(|local| local.member && local.reason.is_some())
        .map(|local| format!("--exclude={}", local.name))
        .collect();
    let workspace = |command: &[&str], rest: &[&str]| workspace(command, &without, rest);
    // Clippy lints a workspace member it compiles as a dependency of another
    // one all the same, `-D warnings` included, unless it is told
    // `--no-deps`: then it lints only the packages it was asked for. A path
    // dependency outside the workspace it never lints.
    let clippy = ["--all-targets", "--no-deps", "--", "-D", "warnings"];
    for (name, features) in [
        ("Clippy, all features", "--all-features"),
        ("Clippy, no default features", "--no-default-features"),
    ] {
        let mut rest = vec![features];
        rest.extend(clippy);
        let args = workspace(&["clippy"], &rest);
        step(session, name, |s| cargo(s, dir, &args, &[]).then_some(()))?;
    }
    let all = workspace(&["test"], &["--all-features"]);
    let doc = workspace(&["test"], &["--all-features", "--doc"]);
    step(session, "Tests, all features", |s| {
        (cargo(s, dir, &all, &[]) && cargo(s, dir, &doc, &[])).then_some(())
    })?;
    let none = workspace(&["test"], &["--no-default-features"]);
    step(session, "Tests, no default features", |s| {
        cargo(s, dir, &none, &[]).then_some(())
    })?;
    let docs = workspace(&["doc"], &["--all-features", "--no-deps"]);
    step(session, "Documentation without warnings", |s| {
        cargo(s, dir, &docs, &[("RUSTDOCFLAGS", "-D warnings")]).then_some(())
    })?;
    let release = workspace(&["build"], &["--all-features", "--release"]);
    step(session, "Release build", |s| {
        cargo(s, dir, &release, &[]).then_some(())
    })?;
    step(session, "Oldest supported Rust version", |s| {
        oldest_version(s, dir, &locals, &without).then_some(())
    })?;
    let (toolchain, llvm_cov) = step(session, "Coverage tools", |s| coverage_tools(s, dir))?;
    let coverage = workspace(&["llvm-cov"], &["--all-features"]);
    step(
        session,
        "Regions, lines and functions are fully covered",
        |s| covered(s, dir, &locals, coverage, &toolchain, &llvm_cov).then_some(()),
    )?;
    step(
        session,
        "Each published package builds and tests from its own contents",
        |s| package_test(s, dir, &locals).then_some(()),
    )
}

/// The arguments of a Cargo command over the workspace: the command, then
/// `--locked --workspace` and the members that are out (`without`), then the
/// rest.
fn workspace(command: &[&str], without: &[String], rest: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = command.iter().map(|&arg| arg.to_owned()).collect();
    args.extend(["--locked".to_owned(), "--workspace".to_owned()]);
    args.extend(without.iter().cloned());
    args.extend(rest.iter().map(|&arg| arg.to_owned()));
    args
}

/// One step: its name opens a group of the log, which closes after it, and a
/// step that fails says so below the group.
fn step<T>(
    session: &mut Session<'_, '_>,
    name: &str,
    body: impl FnOnce(&mut Session<'_, '_>) -> Option<T>,
) -> Option<T> {
    session.log.line(&format!("::group::{name}"));
    let done = body(session);
    session.log.line("::endgroup::");
    if done.is_none() {
        session.log.line(&format!("{name}: failed"));
    }
    done
}

/// A command that starts `program` in the workspace's folder, with the
/// computer's search path and without the variables [`steers`] names.
fn command(session: &Session<'_, '_>, program: impl AsRef<OsStr>, dir: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(dir)
        .env("PATH", &session.context.env.search_path)
        .stdin(Stdio::null());
    for (name, _) in std::env::vars_os() {
        if steers(&name) {
            command.env_remove(name);
        }
    }
    command
}

/// Runs a command to its end and writes what it prints on both outputs into
/// the log, line by line as it comes: whether it succeeded. A program that
/// cannot be started is written as a finding and counts as failed.
fn stream(session: &mut Session<'_, '_>, mut command: Command, what: &str) -> bool {
    let shown: Vec<String> = std::iter::once(what.to_owned())
        .chain(
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned()),
        )
        .collect();
    session.log.line(&format!("> {}", shown.join(" ")));
    let started = std::io::pipe().and_then(|(reader, writer)| {
        writer.try_clone().and_then(|copy| {
            command.stdout(copy).stderr(writer);
            command.spawn().map(|child| (reader, child))
        })
    });
    // The command holds the writing ends until it is dropped, and the reading
    // below ends only when every writing end is closed.
    drop(command);
    match started {
        Ok((reader, mut child)) => {
            for line in BufReader::new(reader).split(b'\n').map_while(Result::ok) {
                session
                    .log
                    .line(String::from_utf8_lossy(&line).trim_end_matches('\r'));
            }
            child.wait().is_ok_and(|status| status.success())
        }
        Err(error) => {
            session
                .log
                .error(None, None, &format!("{what} cannot be started: {error}"));
            false
        }
    }
}

/// Runs `program` with these arguments in the workspace's folder, its output
/// in the log: whether it succeeded.
fn call(session: &mut Session<'_, '_>, dir: &Path, program: &str, args: &[&str]) -> bool {
    let mut command = command(session, program, dir);
    command.args(args);
    stream(session, command, program)
}

/// Runs Cargo with these arguments and variables in the workspace's folder:
/// whether it succeeded.
fn cargo(
    session: &mut Session<'_, '_>,
    dir: &Path,
    args: &[String],
    variables: &[(&str, &str)],
) -> bool {
    let mut command = command(session, "cargo", dir);
    command.args(args).envs(variables.iter().copied());
    stream(session, command, "cargo")
}

/// The toolchain the workspace pins, with rustfmt and Clippy, installed
/// through rustup from its `rust-toolchain.toml`.
fn toolchain(session: &mut Session<'_, '_>, dir: &Path) -> bool {
    call(session, dir, "rustup", &["toolchain", "install"])
        && call(
            session,
            dir,
            "rustup",
            &["component", "add", "rustfmt", "clippy"],
        )
        && call(session, dir, "rustc", &["--version"])
}

/// What `cargo metadata` says about the workspace, with all features, so that
/// an optional path dependency is seen as well; or nothing, with why written.
fn metadata(session: &mut Session<'_, '_>, dir: &Path) -> Option<Metadata> {
    let mut command = command(session, "cargo", dir);
    command.args([
        "metadata",
        "--locked",
        "--format-version",
        "1",
        "--all-features",
    ]);
    let metadata = session.output(command, "cargo").and_then(|output| {
        session.log.lines(&String::from_utf8_lossy(&output.stderr));
        output
            .status
            .success()
            .then(|| serde_json::from_slice::<Metadata>(&output.stdout).ok())
            .flatten()
    });
    if metadata.is_none() {
        session
            .log
            .line("cargo metadata gave nothing the check can read; what it said stands above.");
    }
    metadata
}

/// The local packages, each named with what holds for it.
fn packages(session: &mut Session<'_, '_>, dir: &Path) -> Option<Vec<Local>> {
    let metadata = metadata(session, dir)?;
    let excluded: Vec<(String, String)> = session
        .context
        .packages
        .excluded
        .iter()
        .map(|package| {
            (
                package.folder.to_string(),
                package.reason.as_str().to_owned(),
            )
        })
        .collect();
    let locals = locals(&metadata, session.context.root, &excluded);
    let checked: Vec<&str> = locals
        .iter()
        .filter(|local| local.checked())
        .map(|local| local.name.as_str())
        .collect();
    session
        .log
        .line(&format!("Checked: {}", checked.join(", ")));
    for local in &locals {
        if local.formatted_only() {
            session.log.line(&format!(
                "Formatted only, a path dependency that is no member: {} at {}",
                local.name, local.dir
            ));
        }
        if let Some(reason) = &local.reason {
            let kind = if local.member {
                "a member"
            } else {
                "a path dependency"
            };
            session.log.line(&format!(
                "Left out of every step, {kind}: {} at {}: {reason}",
                local.name, local.dir
            ));
        }
    }
    if checked.is_empty() {
        session
            .log
            .line("Every member of the workspace is excluded, so there is nothing to check.");
    }
    Some(locals)
}

/// The calls that check the formatting of every local package that is not
/// out, and of nothing of one that is, each with `extra` behind it.
///
/// `cargo fmt --all` would take in the excluded ones as well, and `--package`
/// takes members only. So the members that stay go through `cargo fmt
/// --package`, and every other package that stays, a path dependency that is
/// no member, goes to rustfmt the way `cargo fmt --all` hands it over: the
/// root file of each of its targets, with the target's edition, one call per
/// edition, in as many calls as the command line needs.
fn formatters(
    session: &Session<'_, '_>,
    dir: &Path,
    locals: &[Local],
    extra: &[&str],
) -> Vec<Command> {
    let mut commands = Vec::new();
    let members: Vec<String> = locals
        .iter()
        .filter(|local| local.checked())
        .map(|local| format!("--package={}", local.name))
        .collect();
    if !members.is_empty() {
        let mut command = command(session, "cargo", dir);
        command
            .arg("fmt")
            .args(&members)
            .args(["--", "--check"])
            .args(FORMAT_ALL)
            .args(extra);
        commands.push(command);
    }
    let roots: BTreeSet<(&str, &str)> = locals
        .iter()
        .filter(|local| local.formatted_only())
        .flat_map(|local| {
            local
                .targets
                .iter()
                .map(|(edition, path)| (edition.as_str(), path.as_str()))
        })
        .collect();
    let editions: BTreeSet<&str> = roots.iter().map(|(edition, _)| *edition).collect();
    for edition in editions {
        let files: Vec<&str> = roots
            .iter()
            .filter(|(of, _)| *of == edition)
            .map(|(_, path)| *path)
            .collect();
        for chunk in chunks(&files) {
            let mut command = command(session, "rustfmt", dir);
            command
                .args(["--edition", edition, "--check"])
                .args(FORMAT_ALL)
                .args(extra)
                .args(chunk);
            commands.push(command);
        }
    }
    commands
}

/// rustfmt in check mode over everything that stays: whether every file is
/// formatted the way `cargo fmt` writes it.
///
/// rustfmt names every file that differs on standard output, by its absolute
/// path, and each one fails the step, as an annotation on the file when it is
/// one of the check's files. On standard error it warns or says what keeps it
/// from formatting. rustfmt reads the repository's own configuration, so a
/// repository decides how it formats; its `ignore`, which takes single files
/// out on a nightly toolchain, cannot be set aside on the command line, and
/// phase 0 fails on every file it takes out. On a failure the differences
/// follow, for the log.
fn formatting(session: &mut Session<'_, '_>, dir: &Path, locals: &[Local]) -> bool {
    let mut success = true;
    let mut named = String::new();
    for command in formatters(session, dir, locals, &["--files-with-diff"]) {
        if let Some(output) = session.output(command, "rustfmt") {
            success &= output.status.success();
            named.push_str(&String::from_utf8_lossy(&output.stdout));
            session.log.lines(&String::from_utf8_lossy(&output.stderr));
        } else {
            success = false;
        }
    }
    if success {
        session
            .log
            .line("Every file is formatted the way cargo fmt writes it.");
        return true;
    }
    if named.trim().is_empty() {
        session
            .log
            .line("Formatting failed and named no file; what rustfmt said stands above.");
    }
    let prefix = format!("{}/", session.context.root.display());
    for line in named.lines() {
        match line
            .strip_prefix(&prefix)
            .filter(|file| session.files.iter().any(|known| known == file))
        {
            Some(file) => {
                session.log.error(
                    Some(file),
                    None,
                    "not formatted the way cargo fmt writes it",
                );
            }
            None => session.log.line(line),
        }
    }
    for command in formatters(session, dir, locals, &[]) {
        stream(session, command, "rustfmt");
    }
    false
}

/// The workspace built with the oldest `rust-version` a checked package
/// claims, installed through rustup with the minimal profile. A workspace
/// that claims none has nothing to prove here.
fn oldest_version(
    session: &mut Session<'_, '_>,
    dir: &Path,
    locals: &[Local],
    without: &[String],
) -> bool {
    let Some(version) = oldest(locals) else {
        session
            .log
            .line("No package claims a rust-version, so there is no oldest version to build with.");
        return true;
    };
    session.log.line(&format!(
        "Building with {version}, the oldest version a package claims."
    ));
    let toolchain = format!("+{version}");
    call(
        session,
        dir,
        "rustup",
        &["toolchain", "install", version, "--profile", "minimal"],
    ) && cargo(
        session,
        dir,
        &workspace(&[&toolchain, "build"], without, &["--all-features"]),
        &[],
    )
}

/// The nightly toolchain the register pins for coverage, installed through
/// rustup with `llvm-tools-preview`, and cargo-llvm-cov as the register pins
/// it.
///
/// On a nightly toolchain cargo-llvm-cov sets the cfg `coverage_nightly`,
/// and only there does `#[coverage(off)]` exist. So a workspace marks a
/// function it justifies leaving out with
/// `#[cfg_attr(coverage_nightly, coverage(off))]`, enables the attribute at
/// the crate root with
/// `#![cfg_attr(coverage_nightly, feature(coverage_attribute))]`, declares
/// the cfg in its manifest with `unexpected_cfgs = { level = "...", check-cfg
/// = ["cfg(coverage_nightly)"] }` under `[workspace.lints.rust]` or
/// `[lints.rust]` (otherwise rustc reports every such `cfg_attr` as
/// `unexpected_cfgs`, which Clippy and rustdoc, both run with `-D warnings`
/// here, turn into an error), and builds unchanged on its own toolchain.
fn coverage_tools(session: &mut Session<'_, '_>, dir: &Path) -> Option<(String, Tool)> {
    let pinned = session
        .context
        .register
        .check
        .iter()
        .find(|(name, _)| name.as_str() == "rust")
        .and_then(|(_, check)| check.coverage_toolchain.clone());
    let Some(toolchain) = pinned else {
        session.log.error(
            None,
            None,
            "the register pins no coverage toolchain for the rust check",
        );
        return None;
    };
    let installed = call(
        session,
        dir,
        "rustup",
        &[
            "toolchain",
            "install",
            &toolchain,
            "--profile",
            "minimal",
            "--component",
            "llvm-tools-preview",
        ],
    ) && call(
        session,
        dir,
        "rustc",
        &[&format!("+{toolchain}"), "--version"],
    );
    if !installed {
        return None;
    }
    session
        .tool("cargo-llvm-cov", false)
        .map(|tool| (toolchain, tool))
}

/// Regions, lines and functions fully covered, measured by cargo-llvm-cov on
/// the coverage toolchain, without the files of an excluded package
/// ([`uncounted`]).
fn covered(
    session: &mut Session<'_, '_>,
    dir: &Path,
    locals: &[Local],
    mut args: Vec<String>,
    toolchain: &str,
    llvm_cov: &Tool,
) -> bool {
    if let Some(pattern) = uncounted(
        session.context.root,
        locals,
        session.context.packages.tracked,
    ) {
        session.log.line(&format!("Not counted: {pattern}"));
        args.extend(["--ignore-filename-regex".to_owned(), pattern]);
    }
    for gate in [
        "--fail-under-lines",
        "--fail-under-regions",
        "--fail-under-functions",
    ] {
        args.extend([gate.to_owned(), "100".to_owned()]);
    }
    let mut command = command(session, &llvm_cov.executable, dir);
    command
        .env("PATH", &llvm_cov.search_path)
        .env("RUSTUP_TOOLCHAIN", toolchain)
        .args(&args);
    stream(session, command, "cargo-llvm-cov")
}

/// Every package published from here and checked, built and tested from its
/// own packaged contents, with the versions written first and every manifest
/// and lockfile put back afterwards, byte for byte, so that what comes after
/// sees the files as they are committed.
fn package_test(session: &mut Session<'_, '_>, dir: &Path, locals: &[Local]) -> bool {
    let repository = session.context.packages.repository;
    let saved: Vec<(String, String)> = session
        .context
        .packages
        .tracked
        .iter()
        .filter(|path| matches!(path.file_name(), Some("Cargo.toml" | "Cargo.lock")))
        .filter_map(|path| {
            let path = path.to_string();
            repository.read(&path).ok().map(|text| (path, text))
        })
        .collect();
    let passed = verify(session, dir, locals);
    let mut restored = true;
    for (path, text) in &saved {
        if repository.read(path).ok().as_ref() == Some(text) {
            continue;
        }
        if let Err(error) = repository.write(path, text) {
            session.log.error(
                Some(path),
                None,
                &format!("cannot be put back as it is committed: {error}"),
            );
            restored = false;
        }
    }
    passed && restored
}

/// The package test proper: see [`package_test`].
///
/// Only what is published, and not excluded. A package with `publish = false`
/// has no package anyone could build from the outside, and `cargo package
/// --workspace` fails on it instead of skipping it.
///
/// The versions first, as the version writer cargo writes them, the same call
/// as `write-versions`: no manifest carries one, since the history is the
/// truth, and a package without a version is not published (Cargo's
/// `publish` defaults to false then), so it would not be verified. The
/// writing starts from the units of this workspace alone, as Cargo finds it,
/// and follows their path dependencies wherever they lie, so that what
/// another workspace of the repository needs cannot fail this one.
///
/// All in one `cargo package`. A package that depends on another one
/// published from here by path names it by version in its package, and that
/// version is on no registry yet. Packaged together, Cargo lays the packages
/// of the others over crates.io, but it still takes the highest version that
/// matches from both: a stranger's crate of the same name wins whenever
/// crates.io has a higher one, or, on a pull request, the same version
/// without the suffix. So each package packaged here is patched in from its
/// folder as well, which leaves crates.io nothing to offer for it, and so is
/// every other local package they reach, a path dependency that is no member
/// or a member an `[[exclude]]` entry takes out: only the ones they reach,
/// since a patch the workspace does not use changes its lockfile, and
/// `--locked` refuses that. `--allow-dirty`, since Git does not have the
/// versions just written.
///
/// Each package is copied out of the workspace, where its
/// `rust-toolchain.toml` no longer applies, so the workspace's toolchain is
/// named, and each copy is tested with the copies of its siblings and the
/// other local packages patched in. A patch a copy does not need only draws a
/// warning.
fn verify(session: &mut Session<'_, '_>, dir: &Path, locals: &[Local]) -> bool {
    let packages = &session.context.packages;
    let manifests: Vec<RelativePath> = packages
        .tracked
        .iter()
        .filter(|path| path.file_name() == Some("Cargo.toml"))
        .cloned()
        .collect();
    let written = writer::write(
        packages.repository,
        &manifests,
        packages.orders,
        Some(&session.unit),
    );
    match written {
        Ok(said) => session.log.lines(&said),
        Err(error) => {
            session.log.line(&error.to_string());
            return false;
        }
    }
    let Some(metadata) = metadata(session, dir) else {
        return false;
    };
    let checked: Vec<&str> = locals
        .iter()
        .filter(|local| local.checked())
        .map(|local| local.name.as_str())
        .collect();
    let published: Vec<(String, String)> = metadata
        .packages
        .iter()
        .filter(|package| {
            package.source.is_none()
                && package.publish.as_ref().is_none_or(|to| !to.is_empty())
                && checked.contains(&package.name.as_str())
        })
        .map(|package| (package.name.clone(), package.version.clone()))
        .collect();
    if published.is_empty() {
        session
            .log
            .line("Nothing is published from here, so there is nothing to verify.");
        return true;
    }
    let names: Vec<String> = published.iter().map(|(name, _)| name.clone()).collect();
    let mut own = Vec::new();
    let mut others = Vec::new();
    for (inside, name, folder) in reached(&metadata, &names) {
        own.extend(["--config".to_owned(), patch(&name, &folder)]);
        if !inside {
            session.log.line(&format!(
                "Patched in from its folder, not packaged here: {name} at {folder}"
            ));
            others.extend(["--config".to_owned(), patch(&name, &folder)]);
        }
    }
    let mut args = vec![
        "package".to_owned(),
        "--locked".to_owned(),
        "--allow-dirty".to_owned(),
    ];
    for name in &names {
        args.extend(["--package".to_owned(), name.clone()]);
    }
    args.extend(own);
    cargo(session, dir, &args, &[]) && test_copies(session, dir, &metadata, &published, &others)
}

/// Each package `cargo package` packaged (`published`, name and version)
/// copied out of the workspace and tested there, with the copies of its
/// siblings and the patches in `others` laid over crates.io.
fn test_copies(
    session: &mut Session<'_, '_>,
    dir: &Path,
    metadata: &Metadata,
    published: &[(String, String)],
    others: &[String],
) -> bool {
    let Some(temp) = session.folder(|_| Vec::new()) else {
        return false;
    };
    let mut copies = Vec::new();
    for (name, version) in published {
        let from = metadata
            .target_directory
            .join("package")
            .join(format!("{name}-{version}"));
        let to = temp.join(format!("{name}-{version}"));
        if let Err(error) = copy(&from, &to) {
            session.log.error(
                None,
                None,
                &format!(
                    "the package {name} {version} cannot be copied out of {}: {error}",
                    from.display()
                ),
            );
            return false;
        }
        copies.push((name.clone(), to));
    }
    let mut active = command(session, "rustup", dir);
    active.args(["show", "active-toolchain"]);
    let toolchain = session.output(active, "rustup").and_then(|output| {
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            .map(|name| format!("+{name}"))
    });
    let Some(toolchain) = toolchain else {
        session
            .log
            .line("rustup names no active toolchain for the workspace, so no copy can be tested.");
        return false;
    };
    for (name, copy) in &copies {
        let mut args = vec![
            toolchain.clone(),
            "test".to_owned(),
            "--all-features".to_owned(),
        ];
        for (other, path) in &copies {
            if other != name {
                args.extend(["--config".to_owned(), patch(other, &path.to_string_lossy())]);
            }
        }
        args.extend(others.iter().cloned());
        if !cargo(session, copy, &args, &[]) {
            return false;
        }
    }
    session.log.line(&format!(
        "Verified {} package(s) outside the source tree.",
        copies.len()
    ));
    true
}

/// Copies the folder `from` with everything in it to `to`, which must not
/// exist yet.
fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir(to)
        .and_then(|()| fs::read_dir(from))
        .and_then(|mut entries| {
            // An entry that cannot be read ends the copy with its error, so
            // that no package is tested from an incomplete copy.
            entries.try_for_each(|entry| {
                entry.and_then(|entry| {
                    let path = entry.path();
                    let target = to.join(entry.file_name());
                    if path.is_dir() {
                        copy(&path, &target)
                    } else {
                        fs::copy(&path, &target).map(drop)
                    }
                })
            })
        })
}
