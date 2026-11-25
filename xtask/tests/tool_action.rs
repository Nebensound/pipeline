//! The cases of the step that fetches the released tool in the action `tool`,
//! the one step that decides something in shell before the tool exists.
//!
//! Each case runs the step's script as a runner does (`bash --noprofile
//! --norc -eo pipefail`), with stand-ins for Git, gh and curl that answer as
//! the case says and write down how they were called, and with nothing else on
//! the `PATH` than the stand-ins and the few programs the step unpacks with.
//! So the case decides what the network and the machine answer: the tags of
//! the pipeline, which gh is installed, how many requests the token has left,
//! whether the release carries the archive and whether it verifies.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use support::Scene;
use yaml_rust2::YamlLoader;

/// The commit the workflow runs at in every case.
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// Another commit, one the case's release does not stand on.
const OTHER: &str = "89abcdef0123456789abcdef0123456789abcdef";

/// The repository of the pipeline in every case.
const REPOSITORY: &str = "Nebensound/pipeline";

/// What the released binary holds in every case.
const BINARY: &str = "#!/bin/sh\necho 'nebensound-repo 0.0.79'\n";

/// The root of this repository.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The script of the step `Fetch the released tool`.
fn script() -> String {
    let text = fs::read_to_string(root().join(".github/actions/tool/action.yml")).unwrap();
    let action = YamlLoader::load_from_str(&text).unwrap()[0].clone();
    action["runs"]["steps"]
        .as_vec()
        .unwrap()
        .iter()
        .find(|step| step["name"].as_str() == Some("Fetch the released tool"))
        .and_then(|step| step["run"].as_str())
        .expect("the action `tool` has a step `Fetch the released tool`")
        .to_owned()
}

/// Where a program lies on the `PATH` of the test.
fn found(program: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join(program))
        .find(|path| path.is_file())
}

/// Writes a stand-in program.
fn program(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Git: `ls-remote` prints the tags the case names, or fails.
const GIT: &str = r#"#!/bin/sh
echo "git $*" >> "$FAKE_LOG"
[ -z "$FAKE_GIT_FAILS" ] || exit 128
printf '%s' "$FAKE_TAGS"
"#;

/// gh: names its version, says how many requests are left (or fails without
/// an answer) and verifies with the exit code the case names. It refuses to
/// work without a token, as gh does.
const GH: &str = r#"#!/bin/sh
echo "gh $*" >> "$FAKE_LOG"
case "$1" in
  --version)
    printf 'gh version %s (2026-11-12)\nhttps://github.com/cli/cli/releases/tag/v%s\n' "$FAKE_GH_VERSION" "$FAKE_GH_VERSION"
    ;;
  api)
    [ -n "$GH_TOKEN" ] || exit 4
    [ -n "$FAKE_REMAINING" ] || exit 1
    echo "$FAKE_REMAINING"
    ;;
  release)
    [ -n "$GH_TOKEN" ] || exit 4
    exit "$FAKE_VERIFY"
    ;;
esac
"#;

/// curl: writes the case's archive where `-o` says, or fails as for a 404.
const CURL: &str = r#"#!/bin/sh
echo "curl $*" >> "$FAKE_LOG"
[ -n "$FAKE_ASSET" ] || exit 22
while [ $# -gt 0 ]; do
  if [ "$1" = -o ]; then out="$2"; fi
  shift
done
cat "$FAKE_ASSET" > "$out"
"#;

/// What a case sets up.
struct Case {
    os: &'static str,
    arch: &'static str,
    commit: &'static str,
    repository: &'static str,
    tags: String,
    git_fails: bool,
    gh: Option<&'static str>,
    remaining: &'static str,
    asset: bool,
    verifies: bool,
}

impl Case {
    /// A case in which everything is there and verifies, on Linux.
    fn released() -> Self {
        Self {
            os: "Linux",
            arch: "X64",
            commit: COMMIT,
            repository: REPOSITORY,
            tags: format!(
                "{OTHER}\trefs/tags/v0.0.78\n{COMMIT}\trefs/tags/v0.0.79\n{OTHER}\trefs/tags/v0.0.80\n"
            ),
            git_fails: false,
            gh: Some("2.104.0"),
            remaining: "4999",
            asset: true,
            verifies: true,
        }
    }

    /// Runs the step.
    fn run(self) -> Outcome {
        let _alone = support::alone();
        let scene = Scene::new();
        let dir = scene.path();
        let fakes = dir.join("fakes");
        let gh = dir.join("gh");
        let tools = dir.join("tools");
        let temp = dir.join("temp");
        fs::create_dir_all(&tools).unwrap();
        fs::create_dir_all(&temp).unwrap();
        program(&fakes.join("git"), GIT);
        program(&fakes.join("curl"), CURL);
        program(&gh.join("gh"), GH);
        for name in ["tar", "gzip", "mkdir", "cat"] {
            if let Some(path) = found(name) {
                symlink(path, tools.join(name)).unwrap();
            }
        }

        let packed = dir.join("packed");
        support::write(&packed, &[("nebensound-repo", BINARY), ("LICENSE", "EUPL")]);
        let asset = dir.join("asset.tar.gz");
        let status = Command::new(found("tar").expect("tar"))
            .arg("-czf")
            .arg(&asset)
            .arg("-C")
            .arg(&packed)
            .args(["nebensound-repo", "LICENSE"])
            .status()
            .unwrap();
        assert!(status.success());

        let step = dir.join("step.sh");
        fs::write(&step, script()).unwrap();
        let log = dir.join("log");
        let outputs = dir.join("outputs");
        fs::write(&log, "").unwrap();
        fs::write(&outputs, "").unwrap();
        let mut path = vec![fakes, tools];
        if self.gh.is_some() {
            path.insert(0, gh);
        }
        let output = Command::new(found("bash").expect("bash"))
            .args(["--noprofile", "--norc", "-eo", "pipefail"])
            .arg(&step)
            .env_clear()
            .env("PATH", std::env::join_paths(path).unwrap())
            .env("RUNNER_OS", self.os)
            .env("RUNNER_ARCH", self.arch)
            .env("RUNNER_TEMP", &temp)
            .env("GITHUB_OUTPUT", &outputs)
            .env("GH_TOKEN", "a token")
            .env("COMMIT", self.commit)
            .env("REPOSITORY", self.repository)
            .env("FAKE_LOG", &log)
            .env("FAKE_TAGS", &self.tags)
            .env("FAKE_GIT_FAILS", if self.git_fails { "yes" } else { "" })
            .env("FAKE_GH_VERSION", self.gh.unwrap_or_default())
            .env("FAKE_REMAINING", self.remaining)
            .env(
                "FAKE_ASSET",
                if self.asset {
                    asset.as_os_str()
                } else {
                    OsStr::new("")
                },
            )
            .env("FAKE_VERIFY", if self.verifies { "0" } else { "1" })
            .output()
            .unwrap();
        let tool = temp.join("nebensound-repo/nebensound-repo");
        Outcome {
            success: output.status.success(),
            printed: format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
            outputs: fs::read_to_string(&outputs).unwrap(),
            calls: fs::read_to_string(&log).unwrap(),
            tool: fs::read_to_string(&tool).ok(),
            archive: temp
                .join("nebensound-repo-<target>.tar.gz")
                .to_string_lossy()
                .into_owned(),
            home: temp.join("nebensound-repo").to_string_lossy().into_owned(),
        }
    }
}

/// What the step did.
#[derive(Debug)]
struct Outcome {
    success: bool,
    printed: String,
    outputs: String,
    calls: String,
    tool: Option<String>,
    archive: String,
    home: String,
}

impl Outcome {
    /// The step put the verified binary in place and said so, and warned of
    /// nothing.
    fn fetched(&self, target: &str) {
        assert!(self.success, "{self:#?}");
        assert!(
            self.outputs.lines().any(|line| line == "fetched=true"),
            "{self:#?}"
        );
        assert_eq!(self.tool.as_deref(), Some(BINARY), "{self:#?}");
        assert!(!self.printed.contains("::warning"), "{self:#?}");
        let archive = self.archive.replace("<target>", target);
        assert_eq!(
            self.calls
                .lines()
                .filter(|call| !call.starts_with("gh --version") && !call.starts_with("gh api"))
                .collect::<Vec<_>>(),
            [
                format!("git ls-remote --tags --refs https://github.com/{REPOSITORY}"),
                format!(
                    "curl -fsSL --retry 3 -o {archive} \
                     https://github.com/{REPOSITORY}/releases/download/v0.0.79/nebensound-repo-{target}.tar.gz"
                ),
                format!("gh release verify-asset v0.0.79 {archive} --repo {REPOSITORY}"),
            ],
            "{self:#?}"
        );
    }

    /// The step left the tool to the source build, with a warning that names
    /// the reason.
    fn built_from_source(&self, reason: &str) {
        assert!(self.success, "{self:#?}");
        assert!(
            self.printed.lines().any(|line| line
                .starts_with("::warning title=The tool is built from source::")
                && line.contains(reason)),
            "no warning that names {reason:?}: {self:#?}"
        );
        assert!(!self.outputs.contains("fetched="), "{self:#?}");
        assert!(self.tool.is_none(), "{self:#?}");
        assert!(!self.calls.contains("verify-asset"), "{self:#?}");
    }
}

#[test]
fn the_home_of_the_tool_is_named_whatever_follows() {
    for case in [
        Case::released(),
        Case {
            commit: "",
            ..Case::released()
        },
    ] {
        let outcome = case.run();
        assert!(
            outcome
                .outputs
                .lines()
                .any(|line| line == format!("home={}", outcome.home)),
            "{outcome:#?}"
        );
    }
}

#[test]
fn a_released_commit_gets_the_verified_binary_on_linux() {
    Case::released().run().fetched("x86_64-unknown-linux-musl");
}

#[test]
fn a_released_commit_gets_the_verified_binary_on_both_macs() {
    Case {
        os: "macOS",
        arch: "ARM64",
        ..Case::released()
    }
    .run()
    .fetched("aarch64-apple-darwin");
    Case {
        os: "macOS",
        arch: "X64",
        ..Case::released()
    }
    .run()
    .fetched("x86_64-apple-darwin");
}

#[test]
fn a_binary_that_does_not_verify_is_not_run_and_fails_the_step() {
    let outcome = Case {
        verifies: false,
        ..Case::released()
    }
    .run();
    assert!(!outcome.success, "{outcome:#?}");
    assert!(
        outcome.printed.lines().any(|line| line.starts_with(
            "::error title=The released tool does not verify::nebensound-repo-x86_64-unknown-linux-musl.tar.gz \
             of the release v0.0.79 of Nebensound/pipeline does not verify"
        )),
        "{outcome:#?}"
    );
    assert!(!outcome.outputs.contains("fetched="), "{outcome:#?}");
    assert!(outcome.tool.is_none(), "{outcome:#?}");
    assert!(!outcome.printed.contains("::warning"), "{outcome:#?}");
}

#[test]
fn a_commit_without_a_release_is_built_from_source() {
    let outcome = Case {
        tags: format!("{OTHER}\trefs/tags/v0.0.79\n"),
        ..Case::released()
    }
    .run();
    outcome.built_from_source(&format!(
        "the commit {COMMIT} carries no release of Nebensound/pipeline"
    ));
    assert!(!outcome.calls.contains("curl"), "{outcome:#?}");
}

#[test]
fn a_pre_release_or_another_tag_on_the_commit_is_no_release() {
    for tag in [
        "v0.1.0-pre.1",
        "v0.1.0-pre-x.2",
        "nebensound-repo-v0.0.79",
        "v1.2",
    ] {
        Case {
            tags: format!("{COMMIT}\trefs/tags/{tag}\n"),
            ..Case::released()
        }
        .run()
        .built_from_source("carries no release");
    }
}

#[test]
fn a_workflow_that_names_no_commit_or_repository_is_built_from_source() {
    for commit in ["", "0123456", "main"] {
        let outcome = Case {
            commit,
            ..Case::released()
        }
        .run();
        outcome.built_from_source("the workflow names no commit and repository of the pipeline");
        assert!(outcome.calls.is_empty(), "{outcome:#?}");
    }
    for repository in [
        "",
        "Nebensound",
        "a/b/c",
        "a b/c",
        "x/y::error",
        "a/b\n::error::c",
    ] {
        let outcome = Case {
            repository,
            ..Case::released()
        }
        .run();
        outcome.built_from_source("the workflow names no commit and repository of the pipeline");
        assert!(outcome.calls.is_empty(), "{outcome:#?}");
    }
}

#[test]
fn a_system_without_a_released_binary_is_built_from_source() {
    for (os, arch) in [("Linux", "ARM64"), ("Windows", "X64"), ("macOS", "ARM")] {
        let outcome = Case {
            os,
            arch,
            ..Case::released()
        }
        .run();
        outcome.built_from_source(&format!(
            "the pipeline releases no binary for {os} on {arch}"
        ));
        assert!(outcome.calls.is_empty(), "{outcome:#?}");
    }
}

#[test]
fn tags_that_cannot_be_listed_are_built_from_source() {
    Case {
        git_fails: true,
        ..Case::released()
    }
    .run()
    .built_from_source("the tags of Nebensound/pipeline cannot be listed");
}

#[test]
fn a_missing_gh_is_built_from_source() {
    Case {
        gh: None,
        ..Case::released()
    }
    .run()
    .built_from_source("gh is not installed, so the binary of v0.0.79 cannot be verified");
}

#[test]
fn a_gh_older_than_2_93_is_built_from_source_and_2_93_is_enough() {
    for old in ["2.92.9", "2.81.0", "1.99.0", "something"] {
        Case {
            gh: Some(old),
            ..Case::released()
        }
        .run()
        .built_from_source("gh is older than 2.93.0 or does not name its version");
    }
    for enough in ["2.93.0", "2.104.0", "3.0.0"] {
        Case {
            gh: Some(enough),
            ..Case::released()
        }
        .run()
        .fetched("x86_64-unknown-linux-musl");
    }
}

#[test]
fn a_token_near_its_rate_limit_is_built_from_source() {
    for remaining in ["9", "0", "many"] {
        Case {
            remaining,
            ..Case::released()
        }
        .run()
        .built_from_source("the token has fewer than 10 requests to the API left");
    }
    Case {
        remaining: "10",
        ..Case::released()
    }
    .run()
    .fetched("x86_64-unknown-linux-musl");
}

#[test]
fn an_api_gh_cannot_ask_is_built_from_source() {
    Case {
        remaining: "",
        ..Case::released()
    }
    .run()
    .built_from_source("gh cannot ask the API of GitHub");
}

#[test]
fn a_release_without_the_archive_is_built_from_source() {
    Case {
        asset: false,
        ..Case::released()
    }
    .run()
    .built_from_source(
        "the release v0.0.79 of Nebensound/pipeline carries no binary for x86_64-unknown-linux-musl",
    );
}

#[test]
fn the_step_fetches_exactly_the_targets_of_the_build_cargo_release() {
    let register: toml::Table =
        toml::from_str(&fs::read_to_string(root().join("register.toml")).unwrap()).unwrap();
    let mut targets: Vec<String> = register["build"]["cargo-release"]["targets"]
        .as_table()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    targets.sort_unstable();
    assert!(!targets.is_empty());
    let script = script();
    let mut named: Vec<String> = script
        .lines()
        .filter_map(|line| line.split_once(") target=").map(|(_, rest)| rest))
        .filter_map(|rest| rest.split_whitespace().next())
        .map(str::to_owned)
        .collect();
    named.sort_unstable();
    assert_eq!(
        named, targets,
        "the step and the register name other targets"
    );
}
