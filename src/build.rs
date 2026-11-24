//! `nebensound-repo build`: one build of one unit, the way its building block
//! runs it in the unit's job of the Build phase.
//!
//! The call has the shape of `run`: `build <build> <unit>`, the build as the
//! register names it and the unit's path as the plan names it. The unit has to
//! be one the plan starts that build for; any other is refused before anything
//! runs. What a build needs it works out from the repository at the commit of
//! the run: the version writers have run before it in the same job, so the
//! files carry the version of the run. What it hands on, it names as outputs
//! of the step (`GITHUB_OUTPUT`), for the action that uploads the artifact.
//!
//! The gate decides, as for `run`: a call outside a workflow run opens with the
//! line that says so ([`crate::run::OUTSIDE`]). A build starts Cargo and the
//! Rust toolchain under the rule the Rust check starts them under
//! ([`crate::toolchain`]): without the variables that steer them, and without
//! hunting down the settings that come as files. The image build starts
//! Docker and Trivy under a rule of the same kind ([`docker::steers`]).
//!
//! The builds are `cargo-package` ([`cargo_package`]), `cargo-release`
//! ([`cargo_release`]) and `docker` ([`docker`](mod@docker)).

pub mod cargo_package;
pub mod cargo_release;
pub mod docker;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::Register;
use crate::path::RelativePath;
use crate::repository::Repository;
use crate::setup::Environment;
use crate::workflow::{Log, Variables};

/// One of the builds `build` carries out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    /// The one Cargo package at the unit's path, packaged and tested from its
    /// own contents.
    CargoPackage,
    /// The one binary of the Cargo package at the unit's path, built in
    /// release for the target of this machine, started and archived.
    CargoRelease,
    /// The image of the unit's `deploy/Dockerfile`, built, started, held to a
    /// user other than root and scanned.
    Docker,
}

impl Build {
    /// The build of this name, if `build` carries it out.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        match name {
            "cargo-package" => Some(Self::CargoPackage),
            "cargo-release" => Some(Self::CargoRelease),
            "docker" => Some(Self::Docker),
            _ => None,
        }
    }

    /// The name the register gives it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::CargoPackage => "cargo-package",
            Self::CargoRelease => "cargo-release",
            Self::Docker => "docker",
        }
    }

    /// Whether it takes the unit's version of the run from the plan: the
    /// image build labels the image with it, and the release build holds what
    /// the binary's `--version` prints to it. A crate carries the version the
    /// writer wrote into its manifest instead.
    #[must_use]
    pub fn takes_version(self) -> bool {
        matches!(self, Self::Docker | Self::CargoRelease)
    }
}

/// The machine a build runs on, as far as the image build and the release
/// build ask about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    /// Its operating system, as `std::env::consts::OS` names it.
    pub os: String,
    /// Its processor, as `std::env::consts::ARCH` names it.
    pub arch: String,
    /// Where the kernel shows its processes: `/proc`.
    pub processes: PathBuf,
    /// How long a started image is given to become healthy.
    pub patience: Duration,
    /// How long the build waits between two looks at it.
    pub interval: Duration,
    /// How long a binary the release build made is given to answer
    /// `--version`.
    pub answer: Duration,
}

impl Machine {
    /// This machine: five minutes for an image to become healthy, looked at
    /// every second, and thirty seconds for a binary to answer `--version`.
    #[must_use]
    pub fn current() -> Self {
        Self {
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            processes: PathBuf::from("/proc"),
            patience: Duration::from_secs(300),
            interval: Duration::from_secs(1),
            answer: Duration::from_secs(30),
        }
    }
}

/// What a build runs with.
pub struct Context<'a> {
    /// The repository: the commit the artifact is built from.
    pub repository: &'a Repository,
    /// The repository root, as an absolute path without links.
    pub root: &'a Path,
    /// Where the programs are looked for: the `PATH`.
    pub search_path: &'a OsStr,
    /// Where a build makes the folders it removes again.
    pub temp: &'a Path,
    /// The variables of the run: where the outputs of the step and its
    /// summary go, and which repository it belongs to.
    pub variables: Variables<'a>,
    /// The register: the programs and images a build calls.
    pub register: &'a Register,
    /// This computer, for finding the programs of the register.
    pub env: &'a Environment,
    /// The machine the build runs on.
    pub machine: &'a Machine,
    /// The unit's name, [`None`] for the repository itself.
    pub component: Option<&'a str>,
    /// The unit's version of this run, as the plan gives it: on a pull
    /// request the one its merge would get, with `-pr.<number>` behind it.
    /// [`None`] when the plan gives it none.
    pub version: Option<&'a str>,
}

/// Runs the build of the unit, prints what its programs print, and returns
/// the exit code: 0 when it passed, 1 otherwise.
pub fn run(context: &Context<'_>, build: Build, unit: &RelativePath, log: &mut Log<'_>) -> u8 {
    let passed = match build {
        Build::CargoPackage => cargo_package::run(context, unit, log),
        Build::CargoRelease => cargo_release::run(context, unit, log),
        Build::Docker => docker::run(context, unit, log),
    };
    u8::from(!passed)
}
