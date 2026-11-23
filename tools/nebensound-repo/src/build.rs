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
//! hunting down the settings that come as files.
//!
//! `cargo-package` is the first build ([`cargo_package`]).

pub mod cargo_package;

use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;

use crate::path::RelativePath;
use crate::repository::Repository;
use crate::workflow::{Log, Variables};

/// One of the builds `build` carries out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    /// The one Cargo package at the unit's path, packaged and tested from its
    /// own contents.
    CargoPackage,
}

impl Build {
    /// The build of this name, if `build` carries it out.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        match name {
            "cargo-package" => Some(Self::CargoPackage),
            _ => None,
        }
    }

    /// The name the register gives it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::CargoPackage => "cargo-package",
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
    /// Whether this is a workflow run, where a finding is an annotation.
    pub workflow: bool,
    /// The variables of the run: where the outputs of the step go.
    pub variables: Variables<'a>,
}

/// Runs the build of the unit, prints what its programs print, and returns
/// the exit code: 0 when it passed, 1 otherwise.
pub fn run(context: &Context<'_>, build: Build, unit: &RelativePath, out: &mut dyn Write) -> u8 {
    let mut log = Log::new(out, context.workflow);
    let passed = match build {
        Build::CargoPackage => cargo_package::run(context, unit, &mut log),
    };
    u8::from(!passed)
}
