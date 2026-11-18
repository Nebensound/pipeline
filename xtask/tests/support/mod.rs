//! What the tests share: files written in one call, a host on this machine,
//! and a repository root made up for a test.

#![allow(dead_code, clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::runner::{self, Host, Job, Outcome, Place};

/// Writes every file, with the folders it needs.
pub fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, content) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("the folders");
        fs::write(&path, content).expect("the file");
    }
}

/// A host on this machine that fetches from `origin`.
pub fn host(store: &Path, origin: &str) -> Host {
    Host {
        env: runner::environment(std::env::vars_os().collect()),
        origin: origin.to_owned(),
        store: store.to_path_buf(),
        os: "Linux".to_owned(),
        arch: "X64".to_owned(),
        bash: "bash".into(),
        git: "git".into(),
    }
}

/// A directory for one test, with a workspace and a temporary directory in
/// it.
pub struct Scene {
    pub dir: tempfile::TempDir,
}

impl Scene {
    pub fn new() -> Self {
        let scene = Self {
            dir: tempfile::tempdir().expect("a temporary directory"),
        };
        fs::create_dir_all(scene.workspace()).expect("the workspace");
        fs::create_dir_all(scene.temp()).expect("the temporary directory");
        scene
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn workspace(&self) -> PathBuf {
        self.dir.path().join("workspace")
    }

    pub fn temp(&self) -> PathBuf {
        self.dir.path().join("temp")
    }

    /// Writes an action into `actions/<name>` and returns its directory.
    pub fn action(&self, name: &str, yaml: &str) -> PathBuf {
        let dir = self.dir.path().join("actions").join(name);
        write(&dir, &[("action.yml", yaml)]);
        dir
    }

    pub fn host(&self) -> Host {
        host(&self.dir.path().join("store"), "file:///nowhere")
    }

    /// Runs the action in `dir` with these inputs, on `host`.
    pub fn run_on(
        &self,
        host: &Host,
        job: &mut Job,
        dir: &Path,
        inputs: &[(&str, &str)],
        skip: &[&str],
    ) -> (Result<Outcome, xtask::Error>, String) {
        let inputs: BTreeMap<String, String> = inputs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        let workspace = self.workspace();
        let temp = self.temp();
        let mut out = Vec::new();
        let result = host.run(
            job,
            dir,
            &inputs,
            Place {
                workspace: &workspace,
                temp: &temp,
            },
            skip,
            &mut out,
        );
        (result, String::from_utf8(out).expect("UTF-8"))
    }

    /// Runs the action given as YAML with these inputs.
    pub fn run(
        &self,
        yaml: &str,
        inputs: &[(&str, &str)],
    ) -> (Result<Outcome, xtask::Error>, String) {
        let dir = self.action("main", yaml);
        self.run_on(&self.host(), &mut Job::default(), &dir, inputs, &[])
    }
}

/// Runs git in `dir`, with an identity of its own.
pub fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .status()
        .expect("git starts");
    assert!(status.success(), "git {args:?} in {}", dir.display());
}

/// A composite action of one bash step.
pub fn composite(script: &str) -> String {
    format!(
        "runs:\n  using: composite\n  steps:\n    - shell: bash\n      run: |\n{}",
        indent(script, 8)
    )
}

/// Every line of `text` with `width` spaces in front.
pub fn indent(text: &str, width: usize) -> String {
    text.lines().fold(String::new(), |mut all, line| {
        let _ = writeln!(all, "{}{line}", " ".repeat(width));
        all
    })
}
