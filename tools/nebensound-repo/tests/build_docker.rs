//! `nebensound-repo build docker`: the image build of one unit, run from the
//! tool.
//!
//! No Docker daemon and no scanner run here. `docker` and `trivy` are shell
//! scripts that print how they were called and act the way the fixture tells
//! them: `docker buildx build` leaves the layout the fixture made where it is
//! told to, `docker inspect` answers what the fixture wrote, and a call fails
//! when a line of the fixture's `fail` file is part of its arguments. The
//! process table the user is read from is a folder of the fixture. That the
//! real programs build, start and scan what the block did is shown by the
//! cases of the block, which `cargo xtask check --all` runs through `build` in
//! the gate. Hence Unix only: the gate runs on Linux.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use nebensound_repo::Register;
use nebensound_repo::build::docker::{self, artifact, steers};
use nebensound_repo::build::{self, Build, Context, Machine};
use nebensound_repo::output::Output;
use nebensound_repo::path::RelativePath;
use nebensound_repo::programs::Platform;
use nebensound_repo::repository::Repository;
use nebensound_repo::setup::Environment;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use support::{Git, call_with};
use tempfile::TempDir;

/// Held by every test of this file that starts the programs below: Linux
/// refuses to start a file another process holds open for writing.
static ALONE: Mutex<()> = Mutex::new(());

fn alone() -> MutexGuard<'static, ()> {
    ALONE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether a call fails: a line of the fixture's `fail` file is part of what
/// it was called with. Every call is also written to the file `calls`.
const FAILING: &str = r#"here=$(cd "$(dirname "$0")/.." && pwd -P)
echo "$(basename "$0") $*" >> "$here/calls"
failing() {
  [ -f "$here/fail" ] || return 1
  while IFS= read -r pattern; do
    [ -n "$pattern" ] || continue
    case "$1" in *"$pattern"*) return 0 ;; esac
  done < "$here/fail"
  return 1
}
"#;

const DOCKER: &str = r#"
echo "docker $*" >&2
failing "docker $*" && { echo "error: it failed" >&2; exit 1; }
case "$1" in
  version) echo "Docker 29.0.0" ;;
  buildx)
    case "$2" in
      version) echo "github.com/docker/buildx v0.30.0" ;;
      build)
        for arg; do
          case "$arg" in type=oci,dest=*) dest="${arg#type=oci,dest=}" ;; esac
        done
        [ -f "$here/layout.tar" ] && cat "$here/layout.tar" > "$dest" ;;
    esac ;;
  load) cat "$here/loaded" ;;
  image) [ "$2" = inspect ] && cat "$here/id" ;;
  volume)
    if [ "$2" = create ]; then
      echo x >> "$here/volumes"
      echo "volume-$(wc -l < "$here/volumes" | tr -d ' ')"
    fi ;;
  create) echo container-1 ;;
  start) echo "$2" ;;
  logs) echo "the service starts" ;;
  inspect)
    case "$3" in
      *State.Status*)
        echo x >> "$here/looks"
        look=$(wc -l < "$here/looks" | tr -d ' ')
        head -n "$look" "$here/states" | tail -n 1 ;;
      *ExitCode*) cat "$here/exit-code" ;;
      *json*) cat "$here/health" ;;
      *Pid*) cat "$here/pid" ;;
    esac ;;
esac
exit 0
"#;

const TRIVY: &str = r#"
echo "trivy $*" >&2
failing "trivy $*" && exit 1
case "$1" in
  --version) echo "Version: 0.76.2" ;;
  image)
    while [ $# -gt 0 ]; do
      [ "$1" = "--output" ] && out="$2"
      shift
    done
    [ -f "$here/report" ] && cat "$here/report" > "$out" ;;
esac
exit 0
"#;

fn executable(file: &Path, script: &str) {
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, script).unwrap();
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold("sha256:".to_owned(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// A tar archive of these files.
fn tar_of(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, path, bytes.as_slice())
            .unwrap();
    }
    builder.into_inner().unwrap()
}

/// What an OCI layout holds, to be changed by a test before it is written.
struct Layout {
    index: Value,
    manifest: Value,
    config: Value,
    layer: Vec<u8>,
    /// Files written as they are, beside the blobs their digests name.
    extra: Vec<(String, Vec<u8>)>,
}

impl Layout {
    /// An image for linux/amd64 labelled 1.2.3, with a health check and two
    /// volumes.
    fn new() -> Self {
        Self {
            index: Value::Null,
            manifest: Value::Null,
            config: json!({
                "os": "linux",
                "architecture": "amd64",
                "config": {
                    "Labels": {"org.opencontainers.image.version": "1.2.3"},
                    "Healthcheck": {"Test": ["CMD", "true"]},
                    "Volumes": {"/data": {}, "/cache": {}}
                }
            }),
            layer: b"the layer".to_vec(),
            extra: Vec::new(),
        }
    }

    fn config_bytes(&self) -> Vec<u8> {
        self.config.to_string().into_bytes()
    }

    fn manifest_of(&self) -> Value {
        if !self.manifest.is_null() {
            return self.manifest.clone();
        }
        json!({
            "schemaVersion": 2,
            "config": {"digest": sha256(&self.config_bytes())},
            "layers": [{"digest": sha256(&self.layer)}]
        })
    }

    fn digests(&self) -> (String, String) {
        (
            sha256(self.manifest_of().to_string().as_bytes()),
            sha256(&self.config_bytes()),
        )
    }

    fn tar(&self) -> Vec<u8> {
        let manifest = self.manifest_of().to_string().into_bytes();
        let index = if self.index.is_null() {
            json!({"manifests": [{
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": sha256(&manifest)
            }]})
        } else {
            self.index.clone()
        };
        let blob = |bytes: &[u8]| {
            (
                format!("blobs/sha256/{}", &sha256(bytes)[7..]),
                bytes.to_vec(),
            )
        };
        let mut entries = vec![
            ("index.json".to_owned(), index.to_string().into_bytes()),
            blob(&manifest),
            blob(&self.config_bytes()),
            blob(&self.layer),
        ];
        entries.extend(self.extra.iter().cloned());
        tar_of(&entries)
    }
}

/// Builds with the fixture as `change` leaves it and expects the build to
/// fail where it reads the layout, with a line that holds `said`.
fn layout_fails(change: &dyn Fn(&Fixture), said: &str) {
    let fixture = Fixture::new();
    change(&fixture);
    let run = Run::new(&fixture);
    fails(&fixture, &run, "Read the digest of the image", said);
}

/// The fixture's layout, changed.
fn changed(change: impl Fn(&mut Layout)) -> impl Fn(&Fixture) {
    move |fixture| {
        let mut layout = Layout::new();
        change(&mut layout);
        fixture.layout(&layout);
    }
}

/// A repository with one commit, the programs above, what they are told and
/// a process table.
struct Fixture {
    base: TempDir,
    git: Git,
}

/// The declaration of the fixture: the component `api` builds docker.
const DECLARATION: &str = r#"[repo]
audience = "public"
pipeline = "none"

[component.api]
path     = "api"
pipeline = { check = ["dockerfile"], build = "docker", dependabot = ["docker"] }
"#;

const DOCKERFILE: &str = "FROM scratch\nUSER 65534\nHEALTHCHECK CMD [\"true\"]\n";

impl Fixture {
    /// Everything as for an image that passes: it loads as its
    /// configuration, becomes healthy at the second look, runs as 1000 inside
    /// a namespace that maps 0 to 100000, and has no vulnerability.
    fn new() -> Self {
        let base = tempfile::tempdir().unwrap();
        let bin = base.path().join("bin");
        executable(
            &bin.join("docker"),
            &format!("#!/bin/sh\n{FAILING}{DOCKER}"),
        );
        executable(&bin.join("trivy"), &format!("#!/bin/sh\n{FAILING}{TRIVY}"));
        let utilities = base.path().join("utilities");
        fs::create_dir_all(&utilities).unwrap();
        let computer = std::env::var_os("PATH").unwrap_or_default();
        for utility in ["basename", "cat", "dirname", "head", "tail", "tr", "wc"] {
            let found = nebensound_repo::setup::find::search(utility, &computer, false).unwrap();
            std::os::unix::fs::symlink(found, utilities.join(utility)).unwrap();
        }
        fs::create_dir_all(base.path().join("tmp")).unwrap();
        let git = Git::new();
        git.write(&[
            (".nebensound/repo.toml", DECLARATION),
            ("api/deploy/Dockerfile", DOCKERFILE),
        ]);
        git.commit("feat: start");
        let fixture = Self { base, git };
        fixture.layout(&Layout::new());
        fixture
            .mark("states", "running starting\nrunning healthy\n")
            .mark("exit-code", "3\n")
            .mark(
                "health",
                r#"{"Status":"unhealthy","Log":[{"Output":"first"},{"Output":"not ready yet\n"}]}"#,
            )
            .mark("pid", "4242\n")
            .mark("report", r#"{"Results":[{"Target":"image"}]}"#);
        fixture.process(
            "4242",
            "Name:\tsleep\nUid:\t101000\t101000\t101000\t101000\n",
        );
        fixture.mark_in("proc/4242/uid_map", "         0     100000      65536\n");
        fixture
    }

    fn root(&self) -> PathBuf {
        fs::canonicalize(self.git.path()).unwrap()
    }

    fn base(&self) -> PathBuf {
        fs::canonicalize(self.base.path()).unwrap()
    }

    /// Writes a file that tells the programs what to do.
    fn mark(&self, name: &str, content: &str) -> &Self {
        self.mark_in(name, content);
        self
    }

    fn mark_in(&self, path: &str, content: &str) {
        let file = self.base.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }

    fn process(&self, pid: &str, status: &str) {
        self.mark_in(&format!("proc/{pid}/status"), status);
    }

    /// Makes the calls that hold one of these fail.
    fn fail(&self, patterns: &[&str]) -> &Self {
        self.mark("fail", &format!("{}\n", patterns.join("\n")))
    }

    /// The layout `docker buildx build` leaves, and the answers of `docker
    /// load` and `docker image inspect` that fit it.
    fn layout(&self, layout: &Layout) -> &Self {
        fs::write(self.base.path().join("layout.tar"), layout.tar()).unwrap();
        let (_, configuration) = layout.digests();
        self.mark("loaded", &format!("Loaded image ID: {configuration}\n"))
            .mark("id", &format!("{configuration}\n"))
    }

    fn search_path(&self) -> OsString {
        std::env::join_paths([self.base().join("bin"), self.base().join("utilities")]).unwrap()
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.base().join("calls")).unwrap_or_default()
    }

    fn outputs(&self) -> String {
        fs::read_to_string(self.base().join("outputs")).unwrap_or_default()
    }
}

/// How a test runs the build.
struct Run<'a> {
    unit: &'a str,
    component: Option<&'a str>,
    version: Option<&'a str>,
    machine: Machine,
    temp: PathBuf,
    variables: BTreeMap<String, String>,
    register: Register,
    workflow: bool,
}

impl Run<'_> {
    /// The component `api` at 1.2.3 on an `x86_64` Linux, in a workflow run on
    /// GitHub.
    fn new(fixture: &Fixture) -> Self {
        let base = fixture.base();
        Self {
            unit: "api",
            component: Some("api"),
            version: Some("1.2.3"),
            machine: Machine {
                os: "linux".to_owned(),
                arch: "x86_64".to_owned(),
                processes: base.join("proc"),
                patience: Duration::from_secs(60),
                interval: Duration::from_millis(1),
            },
            temp: base.join("tmp"),
            variables: BTreeMap::from([
                (
                    "GITHUB_OUTPUT".to_owned(),
                    base.join("outputs").to_string_lossy().into_owned(),
                ),
                (
                    "GITHUB_STEP_SUMMARY".to_owned(),
                    base.join("summary").to_string_lossy().into_owned(),
                ),
                (
                    "GITHUB_SERVER_URL".to_owned(),
                    "https://github.com".to_owned(),
                ),
                ("GITHUB_REPOSITORY".to_owned(), "owner/services".to_owned()),
            ]),
            register: Register::builtin().unwrap(),
            workflow: true,
        }
    }

    /// The exit code and what the build printed.
    fn go(&self, fixture: &Fixture) -> (u8, String) {
        let root = fixture.root();
        let repository = Repository::at(&root);
        let search_path = fixture.search_path();
        let env = Environment {
            computer: "linux-x86_64".to_owned(),
            platform: Platform::of("linux", "x86_64"),
            windows: false,
            search_path: search_path.clone(),
            folder: Some(fixture.base().join("cache")),
            protocols: "=https".to_owned(),
            answers: None,
        };
        let read = |name: &str| self.variables.get(name).cloned();
        let context = Context {
            repository: &repository,
            root: &root,
            search_path: &search_path,
            temp: &self.temp,
            workflow: self.workflow,
            variables: &read,
            register: &self.register,
            env: &env,
            machine: &self.machine,
            component: self.component,
            version: self.version,
        };
        let mut out = Vec::new();
        let code = build::run(
            &context,
            Build::Docker,
            &RelativePath::new(self.unit).unwrap(),
            &mut out,
        );
        (code, String::from_utf8(out).unwrap())
    }
}

fn has(text: &str, line: &str) {
    assert!(
        text.lines().any(|found| found == line),
        "no line {line:?} in\n{text}"
    );
}

/// Builds with the fixture as it is and expects the build to fail in `step`
/// with a line that holds `said`; what was started is removed all the same.
fn fails(fixture: &Fixture, run: &Run<'_>, step: &str, said: &str) -> String {
    let (code, text) = run.go(fixture);
    assert_eq!(code, 1, "{said}: {text}");
    has(&text, &format!("{step}: failed"));
    assert!(text.contains(said), "{said}: {text}");
    assert_eq!(fixture.outputs(), "", "{said}");
    // Nothing of the build's folder stays.
    assert_eq!(
        fs::read_dir(&run.temp).map_or(0, Iterator::count),
        0,
        "{said}: {text}"
    );
    text
}

#[test]
fn the_variables_that_steer_docker_buildx_buildkit_and_trivy_are_taken_out() {
    for name in [
        "DOCKER_DEFAULT_PLATFORM",
        "DOCKER_BUILDKIT",
        "BUILDX_BUILDER",
        "BUILDX_GIT_LABELS",
        "BUILDKIT_PROGRESS",
        "TRIVY_SKIP_DIRS",
        "TRIVY_DB_REPOSITORY",
        "SOURCE_DATE_EPOCH",
    ] {
        assert!(steers(OsStr::new(name)), "{name}");
    }
    for name in
        docker::KEPT
            .iter()
            .copied()
            .chain(["PATH", "HOME", "DOCKER", "CARGO_HOME", "TRIVY"])
    {
        assert!(!steers(OsStr::new(name)), "{name}");
    }
    // What a started program gets: this process's variables less those, and
    // the search path it is given.
    let command = docker::command("docker", Path::new("/"), OsStr::new("/nowhere"));
    let given: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
    assert!(given.iter().all(|(name, _)| !steers(name)));
    assert!(given.contains(&(OsStr::new("PATH"), Some(OsStr::new("/nowhere")))));
    assert_eq!(command.get_current_dir(), Some(Path::new("/")));
}

#[test]
fn the_artifact_carries_the_path_encoded_the_version_and_the_commit() {
    assert_eq!(artifact(".", "1.2.3", "abc"), "container-.@1.2.3@abc");
    assert_eq!(
        artifact("services/a b", "1.2.3-pr.4", "abc"),
        "container-services%2Fa%20b@1.2.3-pr.4@abc"
    );
}

#[test]
fn every_step_runs_in_its_order_and_names_the_image() {
    let _alone = alone();
    let fixture = Fixture::new();
    let run = Run::new(&fixture);
    let (code, text) = run.go(&fixture);
    assert_eq!(code, 0, "{text}");
    let root = fixture.root().display().to_string();
    let head = fixture.git.head();
    let artifact = format!("container-api@1.2.3@{head}");
    let (digest, configuration) = Layout::new().digests();
    let dir = fs::read_dir(&run.temp)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let builder = dir.file_name().unwrap().to_string_lossy().into_owned();
    let image = dir.join("image.tar");
    let mut order = Vec::new();
    for line in [
        "::group::Find the image of the unit",
        &format!("'api' builds api/deploy/Dockerfile as api 1.2.3; its artifact is '{artifact}'."),
        "::group::Start a builder of its own",
        "> docker version --format Docker {{.Server.Version}}",
        "Docker 29.0.0",
        "> docker buildx version",
        &format!(
            "> docker buildx create --name {builder} --driver docker-container --driver-opt \
             image=moby/buildkit:v0.34.2@sha256:\
             c5f8862f52499773af85bc07695b2379ef08e8002352b0df2c4864c91d78dabe --bootstrap"
        ),
        "::group::Build the image",
        &format!(
            "> docker buildx build --builder {builder} --progress plain --platform linux/amd64 \
             --file api/deploy/Dockerfile --no-cache --provenance=false --sbom=false --label \
             org.opencontainers.image.version=1.2.3 --label \
             org.opencontainers.image.revision={head} --label \
             org.opencontainers.image.source=https://github.com/owner/services --label \
             org.opencontainers.image.title=api --output type=oci,dest={} {root}",
            image.display()
        ),
        "::group::Read the digest of the image",
        &format!("The image of 'api' is {digest}, its configuration {configuration}."),
        "::group::Require a health check",
        r#"The image checks its health with ["CMD","true"]."#,
        "::group::Load the image",
        &format!("Loaded image ID: {configuration}"),
        &format!("Loaded {digest} as {configuration}."),
        "::group::Start the image",
        "A fresh volume for /cache: volume-1",
        "A fresh volume for /data: volume-2",
        "> docker start container-1",
        "::group::Wait until it is healthy",
        "The image of 'api' is healthy.",
        "::group::Require a user other than root",
        "The image of 'api' runs with the user ID 1000.",
        "::group::Scan the image",
        "Version: 0.76.2",
        "Trivy finds no vulnerability of severity HIGH or CRITICAL with a fixed version in the \
         image of 'api'.",
        "::group::Remove what it started",
    ] {
        let at = text
            .lines()
            .position(|found| found == line)
            .unwrap_or_else(|| panic!("no line {line:?} in\n{text}"));
        order.push(at);
    }
    assert!(order.is_sorted(), "{text}");
    assert!(!text.contains("failed"), "{text}");
    assert!(!text.contains("::error"), "{text}");
    let calls = fixture.calls();
    assert!(
        calls.contains(&format!(
            "docker create --mount type=volume,source=volume-1,target=/cache --mount \
             type=volume,source=volume-2,target=/data {configuration}\n"
        )),
        "{calls}"
    );
    assert!(
        calls.contains(&format!(
            "trivy image --input {} --config  --ignorefile  --cache-dir {} --scanners vuln \
             --severity HIGH,CRITICAL --ignore-unfixed --exit-code 0 --format json --output {} \
             --no-progress --skip-version-check --disable-telemetry\n",
            dir.join("layout").display(),
            dir.join("trivy-cache").display(),
            dir.join("trivy.json").display()
        )),
        "{calls}"
    );
    removed_and_handed_on(&fixture, &dir, &artifact);
}

/// What the passing build started went again, and the layout stays as the
/// image it names, with the summary.
fn removed_and_handed_on(fixture: &Fixture, dir: &Path, artifact: &str) {
    let calls = fixture.calls();
    let (digest, configuration) = Layout::new().digests();
    let builder = dir.file_name().unwrap().to_string_lossy().into_owned();
    let image = dir.join("image.tar");
    for removed in [
        "docker rm --force container-1\n",
        "docker volume rm volume-1\n",
        "docker volume rm volume-2\n",
        &format!("docker image rm --force {configuration}\n"),
        &format!("docker buildx rm {builder}\n"),
    ] {
        assert!(calls.contains(removed), "{removed}: {calls}");
    }
    // The layout stays, and only the layout, for the action that hands it on.
    let left: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["image.tar"]);
    assert_eq!(
        fixture.outputs(),
        format!("artifact={artifact}\nimage={}\n", image.display())
    );
    assert_eq!(
        fs::read_to_string(fixture.base().join("summary")).unwrap(),
        format!("The image of `api` 1.2.3 is `{digest}`.\n")
    );
}

#[test]
fn the_repository_itself_is_named_after_the_folder_outside_a_workflow_run() {
    let _alone = alone();
    let fixture = Fixture::new();
    // An image without a volume, and a scan that leaves the files behind.
    let mut layout = Layout::new();
    layout.config["config"]["Volumes"] = Value::Null;
    fixture.layout(&layout);
    fs::create_dir_all(fixture.root().join("deploy")).unwrap();
    fs::write(fixture.root().join("deploy/Dockerfile"), DOCKERFILE).unwrap();
    let mut run = Run::new(&fixture);
    run.unit = ".";
    run.component = None;
    run.variables.clear();
    run.workflow = false;
    let (code, text) = run.go(&fixture);
    assert_eq!(code, 0, "{text}");
    let folder = fixture
        .root()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        text.contains(&format!(
            "'.' builds deploy/Dockerfile as {folder} 1.2.3; its artifact is 'container-.@1.2.3@"
        )),
        "{text}"
    );
    has(
        &text,
        "The image declares no VOLUME, so it starts without one.",
    );
    // Without the repository's address there is no label for it.
    assert!(!text.contains("image.source"), "{text}");
    assert!(text.contains("image.title="), "{text}");
    // Outside a workflow run a group is its name.
    has(&text, "Find the image of the unit");
    assert!(!text.contains("::group::"), "{text}");
    // Nothing is written as an output or into a summary.
    assert_eq!(fixture.outputs(), "");
    assert!(!fixture.base().join("summary").exists());

    // A repository named by the run is named after it.
    let mut run = Run::new(&fixture);
    run.unit = ".";
    run.component = None;
    run.variables
        .insert("GITHUB_REPOSITORY".to_owned(), "owner/shop".to_owned());
    let (code, text) = run.go(&fixture);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("'.' builds deploy/Dockerfile as shop 1.2.3"),
        "{text}"
    );
}

#[test]
fn what_keeps_the_image_from_being_found_is_named() {
    let _alone = alone();
    let fixture = Fixture::new();
    // No Dockerfile at the unit.
    let mut run = Run::new(&fixture);
    run.unit = "nowhere";
    fails(
        &fixture,
        &run,
        "Find the image of the unit",
        "::error file=nowhere/deploy/Dockerfile::'nowhere' builds docker but holds no \
         deploy/Dockerfile.",
    );

    // No version of the run.
    let mut run = Run::new(&fixture);
    run.version = None;
    fails(
        &fixture,
        &run,
        "Find the image of the unit",
        "::error::The plan gives 'api' no version of this run, so there is no image to label \
         with it.",
    );

    // Another machine.
    for (os, arch, this) in [
        ("linux", "aarch64", "aarch64"),
        ("macos", "aarch64", "macos on aarch64"),
    ] {
        let mut run = Run::new(&fixture);
        run.machine.os = os.to_owned();
        run.machine.arch = arch.to_owned();
        let text = fails(
            &fixture,
            &run,
            "Find the image of the unit",
            &format!(
                "::error::build-docker builds and starts images for linux/amd64 only, on the \
                 machine itself, and this one is {this}."
            ),
        );
        // What it would build is said before.
        assert!(
            text.contains("'api' builds api/deploy/Dockerfile as api"),
            "{text}"
        );
    }

    // No folder for the image.
    let mut run = Run::new(&fixture);
    run.temp = fixture.base().join("layout.tar");
    let (code, text) = run.go(&fixture);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::no folder can be made in ") && text.contains("for the image: "),
        "{text}"
    );

    // A repository without a commit.
    let fixture = Fixture::new();
    fixture.git.run(&["update-ref", "-d", "HEAD"]);
    fails(
        &fixture,
        &Run::new(&fixture),
        "Find the image of the unit",
        "::error::The commit of the build is not known: ",
    );
    // Nothing was started.
    assert_eq!(fixture.calls(), "");
}

#[test]
fn the_first_docker_call_that_fails_ends_the_build() {
    let _alone = alone();
    for (pattern, step) in [
        ("docker version", "Start a builder of its own"),
        ("buildx version", "Start a builder of its own"),
        ("buildx create", "Start a builder of its own"),
        ("buildx build", "Build the image"),
        ("docker load", "Load the image"),
        ("image inspect", "Load the image"),
        ("volume create", "Start the image"),
        ("docker create", "Start the image"),
        ("docker start", "Start the image"),
        ("{{.State.Pid}}", "Require a user other than root"),
        ("trivy --version", "Scan the image"),
        ("trivy image", "Scan the image"),
    ] {
        let fixture = Fixture::new();
        fixture.fail(&[pattern]);
        let run = Run::new(&fixture);
        fails(&fixture, &run, step, "");
        // The builder goes in every case, and so does what was started.
        let calls = fixture.calls();
        assert!(
            calls.contains("docker buildx rm build-docker-"),
            "{pattern}: {calls}"
        );
    }
}

#[test]
fn a_build_that_leaves_no_layout_or_a_broken_one_fails() {
    let _alone = alone();
    let run = layout_fails;
    // No layout at all.
    let fixture = Fixture::new();
    fs::remove_file(fixture.base().join("layout.tar")).unwrap();
    let text = fails(
        &fixture,
        &Run::new(&fixture),
        "Build the image",
        "docker buildx build passed, but '",
    );
    assert!(text.contains("image.tar' is not there."), "{text}");

    run(
        &|fixture| {
            fs::write(fixture.base().join("layout.tar"), "not a tar").unwrap();
        },
        "' cannot be unpacked: ",
    );
    run(
        &|fixture| {
            fs::write(fixture.base().join("layout.tar"), tar_of(&[])).unwrap();
        },
        "index.json' does not read as JSON: ",
    );
    run(
        &changed(|layout| {
            layout.index = json!({"manifests": [{}, {}]});
        }),
        "::error::The layout of 'api' names 2 entries, the first of type null, and one image \
         manifest is what it has to name.",
    );
    run(
        &changed(|layout| layout.index = json!({})),
        "::error::The layout of 'api' names 0 entries, the first of type null",
    );
    run(
        &changed(|layout| {
            layout.index = json!({"manifests": [{
                "mediaType": "application/vnd.oci.image.index.v1+json"
            }]});
        }),
        "the first of type application/vnd.oci.image.index.v1+json, and one image manifest",
    );
    run(
        &changed(|layout| {
            layout.index = json!({"manifests": [{
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": 42
            }]});
        }),
        "::error::The layout of 'api' holds no file with the digest 42.",
    );
}

#[test]
fn a_layout_whose_files_are_not_what_their_digests_name_fails() {
    let _alone = alone();
    let run = layout_fails;
    let missing = format!("sha256:{}", "0".repeat(64));
    run(
        &changed(|layout| {
            layout.index = json!({"manifests": [{
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": format!("sha256:{}", "0".repeat(64))
            }]});
        }),
        &format!("::error::The layout of 'api' holds no file with the digest {missing}."),
    );
    for digest in [
        format!("sha256:{}", "A".repeat(64)),
        "sha256:../../../index.json".to_owned(),
        "sha512:00".to_owned(),
    ] {
        run(
            &changed(move |layout| {
                layout.index = json!({"manifests": [{
                    "mediaType": "application/vnd.oci.image.manifest.v1+json",
                    "digest": digest.clone()
                }]});
            }),
            "holds no file with the digest sha",
        );
    }
    // A manifest that is no JSON, under its digest.
    run(
        &|fixture| {
            let mut layout = Layout::new();
            let text = b"not JSON".to_vec();
            layout.index = json!({"manifests": [{
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": sha256(&text)
            }]});
            layout
                .extra
                .push((format!("blobs/sha256/{}", &sha256(&text)[7..]), text));
            fixture.layout(&layout);
        },
        "' does not read as JSON: ",
    );
    // A file whose content is not its digest.
    run(
        &|fixture| {
            let layout = Layout::new();
            let layer = &sha256(&layout.layer)[7..];
            let mut changed = Layout::new();
            changed
                .extra
                .push((format!("blobs/sha256/{layer}"), b"something else".to_vec()));
            fixture.layout(&changed);
        },
        &format!(
            "::error::The layout of 'api' holds no file with the digest {}.",
            sha256(b"the layer")
        ),
    );
    run(
        &changed(|layout| {
            layout.manifest = json!({"config": {}, "layers": []});
        }),
        "::error::The layout of 'api' holds no file with the digest null.",
    );
    run(
        &changed(|layout| {
            let config = b"not JSON".to_vec();
            layout.manifest = json!({"config": {"digest": sha256(&config)}});
            layout
                .extra
                .push((format!("blobs/sha256/{}", &sha256(&config)[7..]), config));
        }),
        "' does not read as JSON: ",
    );
    run(
        &changed(|layout| layout.config["architecture"] = json!("arm64")),
        "::error::The image of 'api' is for linux/arm64 and labelled with the version '1.2.3', \
         and it has to be for linux/amd64 and labelled 1.2.3.",
    );
    run(
        &changed(|layout| layout.config = json!({})),
        "::error::The image of 'api' is for / and labelled with the version '', and it has to \
         be for linux/amd64 and labelled 1.2.3.",
    );
    // A summary that cannot be written.
    let fixture = Fixture::new();
    let mut run = Run::new(&fixture);
    run.variables.insert(
        "GITHUB_STEP_SUMMARY".to_owned(),
        fixture.base().to_string_lossy().into_owned(),
    );
    fails(
        &fixture,
        &run,
        "Read the digest of the image",
        "::error::the summary of the run cannot be written to ",
    );
}

#[test]
fn an_image_without_a_health_check_is_not_started() {
    let _alone = alone();
    for test in [Value::Null, json!([]), json!(["NONE"]), json!("CMD true")] {
        let fixture = Fixture::new();
        let mut layout = Layout::new();
        layout.config["config"]["Healthcheck"]["Test"] = test.clone();
        fixture.layout(&layout);
        fails(
            &fixture,
            &Run::new(&fixture),
            "Require a health check",
            "::error file=api/deploy/Dockerfile::The image of 'api' has no HEALTHCHECK, so \
             nothing shows that it starts and serves. Give api/deploy/Dockerfile a HEALTHCHECK \
             instruction.",
        );
        assert!(!fixture.calls().contains("docker load"), "{test}");
    }
}

#[test]
fn the_loaded_image_has_to_be_the_one_the_layout_names() {
    let _alone = alone();
    // Docker names no image.
    let fixture = Fixture::new();
    fixture.mark("loaded", "something else\n");
    fails(
        &fixture,
        &Run::new(&fixture),
        "Load the image",
        "::error::Docker loaded the layout of 'api' and named no image.",
    );

    // Another image.
    let fixture = Fixture::new();
    fixture.mark("id", "sha256:other\n");
    let (digest, configuration) = Layout::new().digests();
    fails(
        &fixture,
        &Run::new(&fixture),
        "Load the image",
        &format!(
            "::error::Docker loaded the image sha256:other from the layout of 'api', and the \
             layout names {digest} with the configuration {configuration}."
        ),
    );

    // A daemon that keeps images in containerd names the manifest, by name.
    let fixture = Fixture::new();
    fixture
        .mark(
            "loaded",
            "Loaded image ID: sha256:first\nLoaded image: api:latest\n",
        )
        .mark("id", &format!("{digest}\n"));
    let (code, text) = Run::new(&fixture).go(&fixture);
    assert_eq!(code, 0, "{text}");
    assert!(
        fixture
            .calls()
            .contains("docker image inspect --format {{.Id}} api:latest\n"),
        "{text}"
    );
    has(&text, &format!("Loaded {digest} as {digest}."));
}

#[test]
fn an_image_that_never_becomes_healthy_shows_its_log() {
    let _alone = alone();
    // Docker reports it unhealthy.
    let fixture = Fixture::new();
    fixture.mark("states", "running starting\nrunning unhealthy\n");
    let text = fails(
        &fixture,
        &Run::new(&fixture),
        "Wait until it is healthy",
        "::error file=api/deploy/Dockerfile::The image of 'api' never became healthy: Docker \
         reports it unhealthy.",
    );
    for line in [
        "The log of the container:",
        "the service starts",
        "What its last health check printed:",
        "not ready yet",
    ] {
        has(&text, line);
    }
    assert!(!text.contains("first"), "{text}");

    // Not healthy in time.
    let fixture = Fixture::new();
    fixture
        .mark("states", "running\n")
        .mark("health", r#"{"Status":"starting"}"#);
    let mut run = Run::new(&fixture);
    run.machine.patience = Duration::from_millis(30);
    let text = fails(
        &fixture,
        &run,
        "Wait until it is healthy",
        "never became healthy: it is not healthy after five minutes.",
    );
    has(&text, "nothing");

    // Stopped.
    let fixture = Fixture::new();
    fixture.mark("states", "exited\n").mark("health", "null");
    let text = fails(
        &fixture,
        &Run::new(&fixture),
        "Wait until it is healthy",
        "never became healthy: it stopped with the exit code 3.",
    );
    has(&text, "nothing");

    // Gone: what Docker cannot say is not said.
    let fixture = Fixture::new();
    fixture
        .mark("states", "running starting\n")
        .fail(&["{{.State.Status}}", "{{json .State.Health}}"]);
    let text = fails(
        &fixture,
        &Run::new(&fixture),
        "Wait until it is healthy",
        "never became healthy: it stopped with the exit code 3.",
    );
    has(&text, "What its last health check printed:");
    assert!(!text.lines().any(|line| line == "nothing"), "{text}");

    // Its exit code unknown, nothing more is said.
    let fixture = Fixture::new();
    fixture
        .mark("states", "dead\n")
        .fail(&["{{.State.ExitCode}}"]);
    let text = fails(
        &fixture,
        &Run::new(&fixture),
        "Wait until it is healthy",
        "",
    );
    assert!(!text.contains("never became healthy"), "{text}");
    assert!(!text.contains("The log of the container:"), "{text}");
}

#[test]
fn an_image_that_runs_as_root_fails_whatever_name_its_user_has() {
    let _alone = alone();
    let unreadable = "::error::The user of the container of 'api' cannot be read from the host.";
    let cases: [(&str, &str, Option<&str>, &str); 12] = [
        // Root inside the container, as the uid_map shows it.
        (
            "4242",
            "Uid:\t100000\t100000\t100000\t100000\n",
            None,
            "::error file=api/deploy/Dockerfile::The image of 'api' runs as root: its first \
             process has the user ID 0. Give api/deploy/Dockerfile a USER other than root.",
        ),
        // The saved ID alone would let it switch back.
        (
            "4242",
            "Uid:\t101000\t101000\t100000\t101000\n",
            None,
            "runs as root",
        ),
        (
            "0",
            "",
            None,
            "::error::The container of 'api' stopped before its user could be read.",
        ),
        ("", "", None, unreadable),
        ("42a", "", None, unreadable),
        ("7", "", None, unreadable),
        ("4242", "Name:\tsleep\n", None, unreadable),
        ("4242", "Uid:\t1000\t1000\n", None, unreadable),
        ("4242", "Uid:\tx\t1000\t1000\n", None, unreadable),
        // An ID outside the map, and a map with lines that are none.
        (
            "4242",
            "Uid:\t5\t5\t5\n",
            Some("0 100000 65536\nx y z\n1 2\n"),
            unreadable,
        ),
        // A saved ID outside the map.
        ("4242", "Uid:\t101000\t101000\t5\t5\n", None, unreadable),
        // Without a uid_map.
        ("4243", "Uid:\t1000\t1000\t1000\n", None, unreadable),
    ];
    for (pid, status, map, said) in cases {
        let fixture = Fixture::new();
        fixture.mark("pid", &format!("{pid}\n"));
        if !status.is_empty() {
            fixture.process(pid, status);
        }
        if let Some(map) = map {
            fixture.mark_in(&format!("proc/{pid}/uid_map"), map);
        }
        fails(
            &fixture,
            &Run::new(&fixture),
            "Require a user other than root",
            said,
        );
    }

    // An ID mapped near the end of the numbers is no overflow.
    let fixture = Fixture::new();
    fixture.mark_in("proc/4242/uid_map", &format!("{} 0 4294967296\n", u64::MAX));
    fixture.process("4242", "Uid:\t0\t1\t1\n");
    fails(
        &fixture,
        &Run::new(&fixture),
        "Require a user other than root",
        unreadable,
    );
}

#[test]
fn the_scan_names_every_vulnerability_with_a_fixed_version() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.mark(
        "report",
        &json!({"Results": [
            {"Target": "image (alpine 3.10.0)", "Vulnerabilities": [
                {"VulnerabilityID": "CVE-2019-14697", "Severity": "CRITICAL", "PkgName": "musl",
                 "InstalledVersion": "1.1.22-r2", "FixedVersion": "1.1.22-r3"},
                {"VulnerabilityID": "CVE-2019-14697", "Severity": "CRITICAL", "PkgName": "musl",
                 "InstalledVersion": "1.1.22-r2", "FixedVersion": "1.1.22-r3"},
                {"VulnerabilityID": "CVE-2000-1", "Severity": "HIGH", "PkgName": "zlib",
                 "InstalledVersion": "1"}
            ]},
            {"Target": "Node.js"}
        ]})
        .to_string(),
    );
    let text = fails(
        &fixture,
        &Run::new(&fixture),
        "Scan the image",
        "::error file=api/deploy/Dockerfile::The image of 'api' has 2 vulnerabilities of \
         severity HIGH or CRITICAL with a fixed version.",
    );
    let first = text
        .find(
            "::error file=api/deploy/Dockerfile::CVE-2000-1 (HIGH) in zlib 1, fixed in null, in \
             image (alpine 3.10.0)",
        )
        .unwrap();
    let second = text
        .find(
            "::error file=api/deploy/Dockerfile::CVE-2019-14697 (CRITICAL) in musl 1.1.22-r2, \
             fixed in 1.1.22-r3, in image (alpine 3.10.0)",
        )
        .unwrap();
    assert!(first < second, "{text}");

    // A report that is none.
    let fixture = Fixture::new();
    fs::remove_file(fixture.base().join("report")).unwrap();
    fails(
        &fixture,
        &Run::new(&fixture),
        "Scan the image",
        "trivy.json' does not read as JSON: ",
    );

    // No Trivy: not pinned, and not on the computer.
    let fixture = Fixture::new();
    fs::remove_file(fixture.base().join("bin/trivy")).unwrap();
    fails(
        &fixture,
        &Run::new(&fixture),
        "Scan the image",
        "::error::trivy 0.76.2 is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup` installs it",
    );
}

#[test]
fn a_register_without_the_pinned_images_or_programs_starts_nothing() {
    let _alone = alone();
    let source = nebensound_repo::register::BUILTIN_REGISTER
        .replace("images        = [\"buildkit\"]\n", "")
        .replace("[image.buildkit]", "[image.other]")
        .replace("programs      = [\"trivy\"]\n", "")
        .replace("[program.trivy", "[program.scanner");
    let register = Register::parse(&source).unwrap();
    let fixture = Fixture::new();
    let mut run = Run::new(&fixture);
    run.register = register.clone();
    let text = fails(
        &fixture,
        &run,
        "Start a builder of its own",
        "::error::The register pins no image buildkit, so no builder can be started.",
    );
    assert!(!text.contains("> docker"), "{text}");

    // The scan cannot name a version it does not know.
    let fixture = Fixture::new();
    let mut run = Run::new(&fixture);
    run.register = Register::parse(
        &nebensound_repo::register::BUILTIN_REGISTER
            .replace("programs      = [\"trivy\"]\n", "")
            .replace("[program.trivy", "[program.scanner"),
    )
    .unwrap();
    fails(
        &fixture,
        &run,
        "Scan the image",
        "::error::trivy is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup` installs it",
    );
}

#[test]
fn outputs_that_cannot_be_written_fail_the_build_at_its_end() {
    let _alone = alone();
    let fixture = Fixture::new();
    let mut run = Run::new(&fixture);
    run.variables.insert(
        "GITHUB_OUTPUT".to_owned(),
        fixture.base().to_string_lossy().into_owned(),
    );
    let (code, text) = run.go(&fixture);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::the outputs of the step cannot be written to "),
        "{text}"
    );
    // Nothing is handed on, so nothing stays.
    assert_eq!(fs::read_dir(&run.temp).unwrap().count(), 0);
}

#[test]
fn the_call_takes_the_version_from_the_plan_or_the_history() {
    let _alone = alone();
    let fixture = Fixture::new();
    // Neither machine gets past Docker here: elsewhere than on an x86_64
    // Linux the build refuses, and there `docker version` fails.
    fixture.fail(&["docker version"]);
    let root = fixture.root().display().to_string();
    let search_path = fixture.search_path();
    let temp = fixture.base().join("tmp");
    let plan = fixture.base().join("plan.json");
    fs::write(
        &plan,
        r#"{"versions":[{"kind":"component","name":"api","path":"api","version":"1.2.3-pr.4","writers":[]}]}"#,
    )
    .unwrap();
    let variables = [
        ("PATH", search_path.to_str().unwrap()),
        ("RUNNER_TEMP", temp.to_str().unwrap()),
    ];
    let call = |args: &[&str]| {
        let mut all = vec!["nebensound-repo", "-C", &root, "build"];
        all.extend(args);
        call_with(&all, &variables, Output::Workflow)
    };

    let (text, code) = call(&["--plan", plan.to_str().unwrap(), "docker", "--", "api"]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("'api' builds api/deploy/Dockerfile as api 1.2.3-pr.4; its artifact is"),
        "{text}"
    );
    assert_eq!(fs::read_dir(&temp).unwrap().count(), 0, "{text}");

    // Without a plan, from the history.
    let (text, code) = call(&["docker", "api"]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("'api' builds api/deploy/Dockerfile as api ")
            && text.contains("; its artifact is 'container-api@"),
        "{text}"
    );

    // A plan that does not read builds nothing.
    let missing = fixture.base().join("no-plan.json");
    let (text, code) = call(&["--plan", missing.to_str().unwrap(), "docker", "api"]);
    assert_ne!(code, 0, "{text}");
    assert!(text.contains("no-plan.json does not read: "), "{text}");
    assert!(!text.contains("builds api/deploy/Dockerfile"), "{text}");

    // A plan without the unit gives it no version.
    fs::write(&plan, r#"{"versions":[]}"#).unwrap();
    let (text, code) = call(&["--plan", plan.to_str().unwrap(), "docker", "api"]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::The plan gives 'api' no version of this run"),
        "{text}"
    );

    // A unit that builds no image is refused.
    let (text, code) = call(&["docker", "."]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(
        text,
        "'.' does not build docker: the plan starts no such build there\n"
    );

    // The repository itself is named after the repository of the run.
    fixture.git.write(&[
        (
            ".nebensound/repo.toml",
            "[repo]\naudience = \"public\"\npipeline = { check = [\"dockerfile\"], build = \
             \"docker\", dependabot = [\"docker\"] }\n",
        ),
        ("deploy/Dockerfile", DOCKERFILE),
    ]);
    fixture.git.remove("api/deploy/Dockerfile");
    fixture.git.commit("feat: one image");
    let variables = [
        ("PATH", search_path.to_str().unwrap()),
        ("GITHUB_REPOSITORY", "owner/shop"),
    ];
    let (text, code) = call_with(
        &["nebensound-repo", "-C", &root, "build", "docker", "."],
        &variables,
        Output::Workflow,
    );
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("'.' builds deploy/Dockerfile as shop "),
        "{text}"
    );
}
