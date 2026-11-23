//! The build `docker` of one unit: the image of the unit's
//! `deploy/Dockerfile`, built once for `linux/amd64` into an OCI layout and
//! labelled with the unit's version, identified by its manifest digest,
//! started from exactly that layout as the image configures it until Docker
//! reports it healthy, held to a user other than root, scanned for
//! vulnerabilities, and named for the action that hands it on as an artifact
//! of the run.
//!
//! The steps run in this order, each in a group of the log, and the first that
//! fails ends the build; what the build started is removed after it whatever
//! happened:
//!
//! 1. **The image of the unit.** The Dockerfile is `deploy/Dockerfile` below
//!    the unit's path, and nothing else. Phase 0 already refuses a unit without
//!    one (`build-without-file`), so this is only a safeguard: it fails by name
//!    where phase 0 saw a tracked file the checkout cannot read, such as a
//!    symlink whose target is missing. The version is the unit's version of
//!    the run, as the plan gives it ([`Context::version`]), the title the
//!    unit's name, the repository's name for the repository itself, and the
//!    commit the one the checkout holds, `HEAD`, which on a pull request is the
//!    merge commit GitHub builds for it. The build runs on an `x86_64` Linux
//!    machine only, since it builds and starts images for `linux/amd64` on the
//!    machine itself and reads the user of the container from the host.
//! 2. **A builder of its own**, since the builder Docker brings cannot write an
//!    OCI layout unless its daemon keeps images in containerd. Its `BuildKit` is
//!    the image the register pins as `buildkit`. It starts empty, so nothing an
//!    earlier build left reaches this one.
//! 3. **The image built**, once, for `linux/amd64` only, without a cache,
//!    without provenance and without an SBOM, so that the digest names the
//!    image alone, into an OCI layout: a tar that holds the manifest, the
//!    configuration and the layers as a registry takes them. The build context
//!    is the repository root. It carries four labels and nothing else from
//!    here: the version, the commit, the repository's address (in a workflow
//!    run) and the title. No build argument is passed.
//! 4. **Its digest**: the layout names one image manifest, and the manifest,
//!    each of its layers and the configuration are the files their digests
//!    name. It is unpacked beside the tar for the scan, which reads a layout
//!    only as a folder; the tar stays what is handed on. The digest of the
//!    manifest is the image's identity, the one a registry reports once it is
//!    pushed unchanged from the layout. It goes into the log and into the
//!    summary of the run.
//! 5. **A health check**: an image without `HEALTHCHECK` fails before it is
//!    started, since nothing could show that it serves.
//! 6. **The image loaded** from exactly that layout, and the loaded image held
//!    to it: Docker names its ID, which is the digest of the configuration in
//!    a daemon that keeps images itself and the digest of the manifest in one
//!    that keeps them in containerd.
//! 7. **The image started** as it configures itself: no environment, no
//!    secret, no published port, the default network, no `--user`, and a fresh
//!    named volume at every path it declares with `VOLUME`.
//! 8. **Healthy**: the build waits until Docker reports the container healthy,
//!    unhealthy or stopped, for at most five minutes ([`Machine::patience`](super::Machine::patience)),
//!    and prints its log and what its last health check printed when it is
//!    not healthy.
//! 9. **A user other than root**: the user IDs of the container's first
//!    process, real, effective and saved, read from the host and taken back
//!    into the container's own user namespace through its `uid_map`, where
//!    neither a user name mapped to 0 nor a switch inside the entrypoint can
//!    hide them. A saved ID 0 would let the process switch back to root.
//! 10. **The scan**: Trivy, the program the register pins, scans the layout
//!     and fails the build on every vulnerability of severity HIGH or CRITICAL
//!     that has a fixed version, each named on the Dockerfile. No ignore file
//!     and no configuration file of Trivy is read. Its database of
//!     vulnerabilities is fetched in every run into a folder of the build.
//!
//! The artifact is named after the unit's path, its version and the commit:
//! `container-<path>@<version>@<sha>`, the path percent-encoded as for a crate
//! ([`encoded`]). It holds exactly one file, `image.tar`, the OCI layout. The
//! step's outputs `artifact` and `image` give the name and the file, which
//! stays in the build's folder in the temporary directory once the build
//! passed; everything else the build made or started goes. Nothing is pushed.
//!
//! Docker, its Buildx and Trivy start in the repository root, without the
//! variables of this process that steer them ([`steers`]).

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use super::Context;
use super::cargo_package::encoded;
use crate::path::RelativePath;
use crate::setup::find;
use crate::workflow::{self, Log};

/// The variables that start like those that steer Docker and are kept all
/// the same: they say where the daemon and Docker's own files are and how to
/// reach the daemon, not what a build does.
pub const KEPT: &[&str] = &[
    "DOCKER_HOST",
    "DOCKER_CONTEXT",
    "DOCKER_CONFIG",
    "DOCKER_CERT_PATH",
    "DOCKER_TLS",
    "DOCKER_TLS_VERIFY",
];

/// Whether a variable of this process steers Docker, Buildx, `BuildKit` or
/// Trivy and is taken out of what they inherit: every one whose name starts
/// with `DOCKER_`, `BUILDX_`, `BUILDKIT_` or `TRIVY_`, such as
/// `DOCKER_DEFAULT_PLATFORM`, `BUILDX_GIT_LABELS`, `BUILDKIT_PROGRESS` or
/// `TRIVY_SKIP_DIRS`, and `SOURCE_DATE_EPOCH`, which Buildx hands the build,
/// less [`KEPT`].
#[must_use]
pub fn steers(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    let prefixed = ["DOCKER_", "BUILDX_", "BUILDKIT_", "TRIVY_"]
        .iter()
        .any(|prefix| name.starts_with(prefix));
    (prefixed && !KEPT.contains(&name.as_ref())) || name == "SOURCE_DATE_EPOCH"
}

/// A command that starts `program` in the folder `dir`, with the search path
/// `search_path`, nothing on its input, and every variable of this process
/// but those [`steers`] names.
#[must_use]
pub fn command(program: impl AsRef<OsStr>, dir: &Path, search_path: &OsStr) -> Command {
    let mut command = Command::new(program);
    command
        .env_clear()
        .envs(std::env::vars_os().filter(|(name, _)| !steers(name)))
        .env("PATH", search_path)
        .current_dir(dir)
        .stdin(Stdio::null());
    command
}

/// The name of the artifact of the unit at `path`: see [`encoded`].
#[must_use]
pub fn artifact(path: &str, version: &str, commit: &str) -> String {
    format!("container-{}@{version}@{commit}", encoded(path))
}

/// The one image manifest a layout has to name.
const MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

/// What the build started or made, for the removal at its end.
#[derive(Default)]
struct Started {
    dir: Option<PathBuf>,
    builder: Option<String>,
    id: Option<String>,
    container: Option<String>,
    volumes: Vec<String>,
}

/// The unit's image, as the build carries it from step to step.
struct Found {
    unit: String,
    dockerfile: String,
    version: String,
    title: String,
    revision: String,
    /// The repository's address, known in a workflow run only.
    source: Option<String>,
    artifact: String,
    dir: PathBuf,
    builder: String,
    image: PathBuf,
}

/// What the layout names.
struct Layout {
    digest: String,
    configuration: String,
    config: Value,
    folder: PathBuf,
}

/// Runs the build; whether it passed.
pub(super) fn run(context: &Context<'_>, unit: &RelativePath, log: &mut Log<'_>) -> bool {
    let mut started = Started::default();
    let passed = steps(context, unit, &mut started, log).is_some();
    log.step("Remove what it started", |_| {
        remove(context, &started, passed);
        Some(())
    });
    passed
}

fn steps(
    context: &Context<'_>,
    unit: &RelativePath,
    started: &mut Started,
    log: &mut Log<'_>,
) -> Option<()> {
    let found = log.step("Find the image of the unit", |log| {
        find(context, unit, started, log)
    })?;
    log.step("Start a builder of its own", |log| {
        builder(context, &found, log).then_some(())
    })?;
    log.step("Build the image", |log| {
        build(context, &found, log).then_some(())
    })?;
    let layout = log.step("Read the digest of the image", |log| {
        digest(context, &found, log)
    })?;
    log.step("Require a health check", |log| {
        health_check(&found, &layout, log).then_some(())
    })?;
    let id = log.step("Load the image", |log| load(context, &found, &layout, log))?;
    started.id = Some(id.clone());
    let container = log.step("Start the image", |log| {
        start(context, &id, &layout, started, log)
    })?;
    log.step("Wait until it is healthy", |log| {
        healthy(context, &found, &container, log).then_some(())
    })?;
    log.step("Require a user other than root", |log| {
        user(context, &found, &container, log).then_some(())
    })?;
    log.step("Scan the image", |log| {
        scan(context, &found, &layout, log).then_some(())
    })?;
    match workflow::output(
        context.variables,
        &[
            ("artifact", &found.artifact),
            ("image", &found.image.to_string_lossy()),
        ],
    ) {
        Ok(()) => Some(()),
        Err(message) => {
            log.error(&message);
            None
        }
    }
}

/// A command that starts Docker with these arguments in the repository root.
fn docker(context: &Context<'_>, args: &[&str]) -> Command {
    let mut command = command("docker", context.root, context.search_path);
    command.args(args);
    command
}

/// Runs Docker with these arguments, its output in the log: whether it
/// succeeded.
fn call(context: &Context<'_>, log: &mut Log<'_>, args: &[&str]) -> bool {
    log.stream(docker(context, args), "docker")
}

/// What Docker prints with these arguments, without the white space around
/// it, when it succeeds; what it prints on standard error goes into the log.
fn answer(context: &Context<'_>, log: &mut Log<'_>, args: &[&str]) -> Option<String> {
    log.capture(docker(context, args), "docker")
        .map(|printed| String::from_utf8_lossy(&printed).trim().to_owned())
}

/// A value as `jq --raw-output` prints it: a string as it is, anything else
/// as JSON, `null` among them.
fn text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), ToOwned::to_owned)
}

/// A file of JSON, read; or nothing, with why written.
fn read_json(file: &Path, log: &mut Log<'_>) -> Option<Value> {
    let read = fs::read(file)
        .map_err(|error| error.to_string())
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()));
    match read {
        Ok(value) => Some(value),
        Err(error) => {
            log.error(&format!(
                "'{}' does not read as JSON: {error}",
                file.display()
            ));
            None
        }
    }
}

/// The repository's name: what the variables of a workflow run give, else
/// the name of the folder it lies in.
fn repository_name(context: &Context<'_>) -> String {
    workflow::given(context.variables, "GITHUB_REPOSITORY").map_or_else(
        || {
            context
                .root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        },
        |repository| repository.rsplit('/').next().unwrap_or_default().to_owned(),
    )
}

/// The Dockerfile, the version, the title, the commit, the artifact's name
/// and the folder of the build.
fn find(
    context: &Context<'_>,
    unit: &RelativePath,
    started: &mut Started,
    log: &mut Log<'_>,
) -> Option<Found> {
    let shown = unit.to_string();
    let dockerfile = if shown == "." {
        "deploy/Dockerfile".to_owned()
    } else {
        format!("{shown}/deploy/Dockerfile")
    };
    if !context.root.join(&dockerfile).is_file() {
        log.error_on(
            &dockerfile,
            &format!("'{shown}' builds docker but holds no deploy/Dockerfile."),
        );
        return None;
    }
    let Some(version) = context.version else {
        log.error(&format!(
            "The plan gives '{shown}' no version of this run, so there is no image to label with \
             it."
        ));
        return None;
    };
    let title = context
        .component
        .map_or_else(|| repository_name(context), ToOwned::to_owned);
    let revision = match context.repository.head() {
        Ok(commit) => commit,
        Err(error) => {
            log.error(&format!("The commit of the build is not known: {error}"));
            return None;
        }
    };
    let artifact = artifact(&shown, version, &revision);
    log.line(&format!(
        "'{shown}' builds {dockerfile} as {title} {version}; its artifact is '{artifact}'."
    ));
    let machine = context.machine;
    if machine.os != "linux" || machine.arch != "x86_64" {
        let this = if machine.os == "linux" {
            machine.arch.clone()
        } else {
            format!("{} on {}", machine.os, machine.arch)
        };
        log.error(&format!(
            "build-docker builds and starts images for linux/amd64 only, on the machine itself, \
             and this one is {this}."
        ));
        return None;
    }
    let dir = match tempfile::Builder::new()
        .prefix("build-docker-")
        .tempdir_in(context.temp)
    {
        Ok(dir) => dir.keep(),
        Err(error) => {
            log.error(&format!(
                "no folder can be made in {} for the image: {error}",
                context.temp.display()
            ));
            return None;
        }
    };
    let builder = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    started.dir = Some(dir.clone());
    started.builder = Some(builder.clone());
    let source = workflow::given(context.variables, "GITHUB_SERVER_URL")
        .zip(workflow::given(context.variables, "GITHUB_REPOSITORY"))
        .map(|(server, repository)| format!("{server}/{repository}"));
    Some(Found {
        unit: shown,
        dockerfile,
        version: version.to_owned(),
        title,
        revision,
        source,
        artifact,
        image: dir.join("image.tar"),
        dir,
        builder,
    })
}

/// A builder of its own, with the `BuildKit` the register pins.
fn builder(context: &Context<'_>, found: &Found, log: &mut Log<'_>) -> bool {
    let Some(image) = context
        .register
        .image
        .iter()
        .find(|(name, _)| name.as_str() == "buildkit")
        .map(|(_, image)| image.reference())
    else {
        log.error("The register pins no image buildkit, so no builder can be started.");
        return false;
    };
    call(
        context,
        log,
        &["version", "--format", "Docker {{.Server.Version}}"],
    ) && call(context, log, &["buildx", "version"])
        && call(
            context,
            log,
            &[
                "buildx",
                "create",
                "--name",
                &found.builder,
                "--driver",
                "docker-container",
                "--driver-opt",
                &format!("image={image}"),
                "--bootstrap",
            ],
        )
}

/// The image built into the OCI layout.
fn build(context: &Context<'_>, found: &Found, log: &mut Log<'_>) -> bool {
    let mut labels = vec![
        format!("org.opencontainers.image.version={}", found.version),
        format!("org.opencontainers.image.revision={}", found.revision),
    ];
    if let Some(source) = &found.source {
        labels.push(format!("org.opencontainers.image.source={source}"));
    }
    labels.push(format!("org.opencontainers.image.title={}", found.title));
    let output = format!("type=oci,dest={}", found.image.display());
    let root = context.root.to_string_lossy();
    let mut args = vec![
        "buildx",
        "build",
        "--builder",
        &found.builder,
        "--progress",
        "plain",
        "--platform",
        "linux/amd64",
        "--file",
        &found.dockerfile,
        "--no-cache",
        "--provenance=false",
        "--sbom=false",
    ];
    for label in &labels {
        args.extend(["--label", label.as_str()]);
    }
    args.extend(["--output", &output, &root]);
    if !call(context, log, &args) {
        return false;
    }
    if fs::metadata(&found.image).is_ok_and(|metadata| metadata.len() > 0) {
        return true;
    }
    log.error(&format!(
        "docker buildx build passed, but '{}' is not there.",
        found.image.display()
    ));
    false
}

/// The file of the layout with the digest `digest`, when it is there and has
/// that digest; or nothing, with why written.
fn blob(folder: &Path, digest: &str, unit: &str, log: &mut Log<'_>) -> Option<PathBuf> {
    let file = digest
        .strip_prefix("sha256:")
        .filter(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .map(|hex| folder.join("blobs").join("sha256").join(hex));
    let matches = file.as_ref().is_some_and(|file| {
        let mut hasher = Sha256::new();
        File::open(file)
            .and_then(|mut opened| std::io::copy(&mut opened, &mut hasher))
            .is_ok_and(|_| {
                let hex = hasher
                    .finalize()
                    .iter()
                    .fold(String::new(), |mut hex, byte| {
                        let _ = write!(hex, "{byte:02x}");
                        hex
                    });
                digest.ends_with(&hex)
            })
    });
    if matches {
        return file;
    }
    log.error(&format!(
        "The layout of '{unit}' holds no file with the digest {digest}."
    ));
    None
}

/// The layout unpacked beside the tar, the digest of its one image manifest
/// and of its configuration, and the configuration, held to the platform and
/// the version.
fn digest(context: &Context<'_>, found: &Found, log: &mut Log<'_>) -> Option<Layout> {
    let unit = &found.unit;
    let folder = found.dir.join("layout");
    let unpacked = fs::create_dir_all(&folder)
        .and_then(|()| File::open(&found.image))
        .and_then(|tar| tar::Archive::new(tar).unpack(&folder));
    if let Err(error) = unpacked {
        log.error(&format!(
            "'{}' cannot be unpacked: {error}",
            found.image.display()
        ));
        return None;
    }
    let index = read_json(&folder.join("index.json"), log)?;
    let manifests = index["manifests"].as_array().map_or(0, Vec::len);
    let media = text(&index["manifests"][0]["mediaType"]);
    if manifests != 1 || media != MANIFEST {
        log.error(&format!(
            "The layout of '{unit}' names {manifests} entries, the first of type {media}, and one \
             image manifest is what it has to name."
        ));
        return None;
    }
    let digest = text(&index["manifests"][0]["digest"]);
    let manifest = blob(&folder, &digest, unit, log)?;
    let manifest = read_json(&manifest, log)?;
    for layer in manifest["layers"].as_array().into_iter().flatten() {
        blob(&folder, &text(&layer["digest"]), unit, log)?;
    }
    let configuration = text(&manifest["config"]["digest"]);
    let config = blob(&folder, &configuration, unit, log)?;
    let config = read_json(&config, log)?;
    let platform = format!(
        "{}/{}",
        config["os"].as_str().unwrap_or_default(),
        config["architecture"].as_str().unwrap_or_default()
    );
    let labelled = config["config"]["Labels"]["org.opencontainers.image.version"]
        .as_str()
        .unwrap_or_default();
    if platform != "linux/amd64" || labelled != found.version {
        log.error(&format!(
            "The image of '{unit}' is for {platform} and labelled with the version '{labelled}', \
             and it has to be for linux/amd64 and labelled {}.",
            found.version
        ));
        return None;
    }
    log.line(&format!(
        "The image of '{unit}' is {digest}, its configuration {configuration}."
    ));
    let summary = format!("The image of `{unit}` {} is `{digest}`.\n", found.version);
    if let Err(message) = workflow::summary(context.variables, &summary) {
        log.error(&message);
        return None;
    }
    Some(Layout {
        digest,
        configuration,
        config,
        folder,
    })
}

/// Whether the image configures a health check.
fn health_check(found: &Found, layout: &Layout, log: &mut Log<'_>) -> bool {
    let test = match &layout.config["config"]["Healthcheck"]["Test"] {
        Value::Array(test) => test.clone(),
        _ => Vec::new(),
    };
    if test.first().is_none_or(|first| first == "NONE") {
        log.error_on(
            &found.dockerfile,
            &format!(
                "The image of '{}' has no HEALTHCHECK, so nothing shows that it starts and \
                 serves. Give {} a HEALTHCHECK instruction.",
                found.unit, found.dockerfile
            ),
        );
        return false;
    }
    log.line(&format!(
        "The image checks its health with {}.",
        Value::Array(test)
    ));
    true
}

/// The layout loaded into Docker, held to what it names: the ID of the
/// loaded image.
fn load(
    context: &Context<'_>,
    found: &Found,
    layout: &Layout,
    log: &mut Log<'_>,
) -> Option<String> {
    let unit = &found.unit;
    let image = found.image.to_string_lossy();
    let loaded = answer(context, log, &["load", "--input", &image])?;
    log.lines(&loaded);
    let Some(reference) = loaded
        .lines()
        .filter_map(|line| {
            line.strip_prefix("Loaded image ID: ")
                .or_else(|| line.strip_prefix("Loaded image: "))
        })
        .next_back()
    else {
        log.error(&format!(
            "Docker loaded the layout of '{unit}' and named no image."
        ));
        return None;
    };
    let id = answer(
        context,
        log,
        &["image", "inspect", "--format", "{{.Id}}", reference],
    )?;
    if id != layout.configuration && id != layout.digest {
        log.error(&format!(
            "Docker loaded the image {id} from the layout of '{unit}', and the layout names {} \
             with the configuration {}.",
            layout.digest, layout.configuration
        ));
        return None;
    }
    log.line(&format!("Loaded {} as {id}.", layout.digest));
    Some(id)
}

/// The loaded image started, with a fresh volume at every path it declares:
/// the container.
fn start(
    context: &Context<'_>,
    id: &str,
    layout: &Layout,
    started: &mut Started,
    log: &mut Log<'_>,
) -> Option<String> {
    let mut paths: Vec<String> = layout.config["config"]["Volumes"]
        .as_object()
        .map(|volumes| volumes.keys().cloned().collect())
        .unwrap_or_default();
    paths.sort();
    let mut mounts = Vec::new();
    for path in &paths {
        let volume = answer(context, log, &["volume", "create"])?;
        started.volumes.push(volume.clone());
        log.line(&format!("A fresh volume for {path}: {volume}"));
        mounts.push(format!("type=volume,source={volume},target={path}"));
    }
    if paths.is_empty() {
        log.line("The image declares no VOLUME, so it starts without one.");
    }
    let mut args = vec!["create"];
    for mount in &mounts {
        args.extend(["--mount", mount.as_str()]);
    }
    args.push(id);
    let container = answer(context, log, &args)?;
    started.container = Some(container.clone());
    call(context, log, &["start", &container]).then_some(container)
}

/// Whether the container becomes healthy in time. When it does not, its log
/// and what its last health check printed are written.
fn healthy(context: &Context<'_>, found: &Found, container: &str, log: &mut Log<'_>) -> bool {
    let unit = &found.unit;
    let machine = context.machine;
    let deadline = Instant::now() + machine.patience;
    let state = [
        "inspect",
        "--format",
        "{{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{end}}",
        container,
    ];
    let reason = loop {
        let seen = answer(context, log, &state).unwrap_or_default();
        let mut words = seen.split_whitespace();
        match (words.next(), words.next()) {
            (Some("running"), Some("healthy")) => {
                log.line(&format!("The image of '{unit}' is healthy."));
                return true;
            }
            (Some("running"), Some("unhealthy")) => break "Docker reports it unhealthy".to_owned(),
            (Some("running"), _) if Instant::now() < deadline => {
                std::thread::sleep(machine.interval);
            }
            (Some("running"), _) => break "it is not healthy after five minutes".to_owned(),
            _ => {
                let exit = ["inspect", "--format", "{{.State.ExitCode}}", container];
                let Some(code) = answer(context, log, &exit) else {
                    return false;
                };
                break format!("it stopped with the exit code {code}");
            }
        }
    };
    log.line("The log of the container:");
    log.stream(docker(context, &["logs", container]), "docker");
    log.line("What its last health check printed:");
    let health = ["inspect", "--format", "{{json .State.Health}}", container];
    if let Some(health) = answer(context, log, &health)
        .and_then(|printed| serde_json::from_str::<Value>(&printed).ok())
    {
        let last = health["Log"]
            .as_array()
            .and_then(|checks| checks.last())
            .and_then(|check| check["Output"].as_str());
        log.lines(last.unwrap_or("nothing"));
    }
    log.error_on(
        &found.dockerfile,
        &format!("The image of '{unit}' never became healthy: {reason}."),
    );
    false
}

/// The user IDs of a process, real, effective and saved, as `processes`
/// shows them, taken back into its own user namespace through its
/// `uid_map`; or nothing when one of them cannot be read.
fn ids(processes: &Path, pid: &str) -> Option<[u64; 3]> {
    if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let folder = processes.join(pid);
    let status = fs::read_to_string(folder.join("status")).ok()?;
    let map = fs::read_to_string(folder.join("uid_map")).ok()?;
    let line = status.lines().find_map(|line| line.strip_prefix("Uid:"))?;
    let seen: Vec<u64> = line
        .split_whitespace()
        .take(3)
        .map(|id| id.parse().ok())
        .collect::<Option<_>>()?;
    let [real, effective, saved] = seen[..] else {
        return None;
    };
    Some([
        inside(&map, real)?,
        inside(&map, effective)?,
        inside(&map, saved)?,
    ])
}

/// A user ID of the host as the user namespace with this `uid_map` sees it.
fn inside(map: &str, id: u64) -> Option<u64> {
    map.lines().find_map(|line| {
        let fields: Vec<u64> = line
            .split_whitespace()
            .map(|field| field.parse().ok())
            .collect::<Option<_>>()?;
        let [inner, outer, count] = fields[..] else {
            return None;
        };
        if outer <= id && id - outer < count {
            inner.checked_add(id - outer)
        } else {
            None
        }
    })
}

/// Whether the first process of the container runs as a user other than
/// root.
fn user(context: &Context<'_>, found: &Found, container: &str, log: &mut Log<'_>) -> bool {
    let unit = &found.unit;
    let pid = ["inspect", "--format", "{{.State.Pid}}", container];
    let Some(pid) = answer(context, log, &pid) else {
        return false;
    };
    if pid == "0" {
        log.error(&format!(
            "The container of '{unit}' stopped before its user could be read."
        ));
        return false;
    }
    let Some([real, effective, saved]) = ids(&context.machine.processes, &pid) else {
        log.error(&format!(
            "The user of the container of '{unit}' cannot be read from the host."
        ));
        return false;
    };
    if real == 0 || effective == 0 || saved == 0 {
        log.error_on(
            &found.dockerfile,
            &format!(
                "The image of '{unit}' runs as root: its first process has the user ID 0. Give \
                 {} a USER other than root.",
                found.dockerfile
            ),
        );
        return false;
    }
    log.line(&format!(
        "The image of '{unit}' runs with the user ID {effective}."
    ));
    true
}

/// Trivy, the pinned copy in the tool's folder first, else the one on the
/// computer; or nothing, with why written.
fn trivy(context: &Context<'_>, log: &mut Log<'_>) -> Option<PathBuf> {
    let entry = context
        .register
        .program
        .iter()
        .find(|(name, _)| name.as_str() == "trivy");
    let location = entry.and_then(|(name, _)| find::locate(context.register, name, context.env));
    if let Some(location) = location {
        return Some(location.executable);
    }
    let version = entry.map_or_else(String::new, |(_, program)| format!(" {}", program.version));
    log.error(&format!(
        "trivy{version} is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup docker` installs it"
    ));
    None
}

/// Whether Trivy finds no vulnerability of severity HIGH or CRITICAL with a
/// fixed version in the layout.
fn scan(context: &Context<'_>, found: &Found, layout: &Layout, log: &mut Log<'_>) -> bool {
    let Some(trivy) = trivy(context, log) else {
        return false;
    };
    let mut version = command(&trivy, context.root, context.search_path);
    version.arg("--version");
    if !log.stream(version, "trivy") {
        return false;
    }
    let report = found.dir.join("trivy.json");
    let mut scan = command(&trivy, context.root, context.search_path);
    scan.args(["image", "--input"])
        .arg(&layout.folder)
        .args(["--config", "", "--ignorefile", "", "--cache-dir"])
        .arg(found.dir.join("trivy-cache"))
        .args([
            "--scanners",
            "vuln",
            "--severity",
            "HIGH,CRITICAL",
            "--ignore-unfixed",
            "--exit-code",
            "0",
            "--format",
            "json",
            "--output",
        ])
        .arg(&report)
        .args([
            "--no-progress",
            "--skip-version-check",
            "--disable-telemetry",
        ]);
    if !log.stream(scan, "trivy") {
        return false;
    }
    let Some(report) = read_json(&report, log) else {
        return false;
    };
    let mut seen = BTreeSet::new();
    for result in report["Results"].as_array().into_iter().flatten() {
        let target = text(&result["Target"]);
        for vulnerability in result["Vulnerabilities"].as_array().into_iter().flatten() {
            seen.insert(format!(
                "{} ({}) in {} {}, fixed in {}, in {target}",
                text(&vulnerability["VulnerabilityID"]),
                text(&vulnerability["Severity"]),
                text(&vulnerability["PkgName"]),
                text(&vulnerability["InstalledVersion"]),
                text(&vulnerability["FixedVersion"]),
            ));
        }
    }
    let unit = &found.unit;
    if seen.is_empty() {
        log.line(&format!(
            "Trivy finds no vulnerability of severity HIGH or CRITICAL with a fixed version in \
             the image of '{unit}'."
        ));
        return true;
    }
    for finding in &seen {
        log.error_on(&found.dockerfile, finding);
    }
    log.error_on(
        &found.dockerfile,
        &format!(
            "The image of '{unit}' has {} vulnerabilities of severity HIGH or CRITICAL with a \
             fixed version.",
            seen.len()
        ),
    );
    false
}

/// Removes what the build started on the machine, whatever happened, so that
/// a machine that runs it more than once, as the test cases do, keeps
/// nothing of it: the container, its volumes, the loaded image, the builder
/// and the build's folder, less the layout when the build passed, which the
/// action that hands it on still reads.
fn remove(context: &Context<'_>, started: &Started, passed: bool) {
    let quietly = |args: &[&str]| {
        let _ = docker(context, args).output();
    };
    if let Some(container) = &started.container {
        quietly(&["rm", "--force", container]);
    }
    for volume in &started.volumes {
        quietly(&["volume", "rm", volume]);
    }
    if let Some(id) = &started.id {
        quietly(&["image", "rm", "--force", id]);
    }
    if let Some(builder) = &started.builder {
        quietly(&["buildx", "rm", builder]);
    }
    let Some(dir) = &started.dir else {
        return;
    };
    if !passed {
        let _ = fs::remove_dir_all(dir);
        return;
    }
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if entry.file_name() != "image.tar" {
            let _ = fs::remove_dir_all(&path).or_else(|_| fs::remove_file(&path));
        }
    }
}
