//! The central register of presets, version writers, checks, builds,
//! targets, and the programs and runtimes the checks and builds call.
//!
//! It is data, not code, and it is compiled into this crate. A repository
//! therefore never holds a copy that could fall behind: adopting a change to
//! the register means raising this crate.

use std::collections::BTreeMap;

use serde::Deserialize;
use thiserror::Error;

use crate::model::{Publish, Release};
use crate::names::{
    ArtifactKind, BuildName, CheckName, EcosystemName, Extension, PresetName, ProgramName,
    RuntimeName, TargetName, VersionWriterName,
};
use crate::path::RelativePath;
use crate::programs::{Program, Runtime, at_least, download_problem, is_version};

/// The vocabulary as it stands in `register.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Register {
    /// Every offered version writer, by name.
    pub version: BTreeMap<VersionWriterName, VersionWriter>,
    /// Every offered check, by name.
    pub check: BTreeMap<CheckName, Check>,
    /// Every offered build, by name.
    pub build: BTreeMap<BuildName, Build>,
    /// Every offered publication target, by name.
    pub target: BTreeMap<TargetName, Target>,
    /// Every offered preset, by name.
    pub preset: BTreeMap<PresetName, Preset>,
    /// Every package ecosystem Dependabot can watch.
    pub ecosystem: BTreeMap<EcosystemName, Ecosystem>,
    /// Every program a check or a build calls, pinned.
    #[serde(default)]
    pub program: BTreeMap<ProgramName, Program>,
    /// Every runtime a program runs on, pinned.
    #[serde(default)]
    pub runtime: BTreeMap<RuntimeName, Runtime>,
}

/// How an ecosystem is recognized.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Ecosystem {
    /// The name Dependabot knows it by.
    pub package_ecosystem: String,
    /// The files it is recognized by in the tree.
    pub manifests: Vec<String>,
}

/// A format-aware writer of the derived copy of a unit's version.
///
/// How the copy is written is the knowledge of its building block
/// `version-<name>`, not of the register. The entry makes the name one a
/// pipeline may select, and says in words what it writes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionWriter {
    /// The derived copy it writes, in words.
    pub copy: String,
}

/// What a check covers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    /// The extensions it promises to cover completely.
    pub extensions: Vec<Extension>,
    /// Whole file names it covers, for the files that have no extension.
    #[serde(default)]
    pub filenames: Vec<String>,
    /// The programs it calls.
    #[serde(default)]
    pub programs: Vec<ProgramName>,
}

/// What a build produces, and what it needs to produce it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Build {
    /// The kind of artifact it leaves behind.
    pub produces: ArtifactKind,
    /// The version writers a unit that declares it must select: the artifact
    /// carries the version they write, and without them whatever version its
    /// sources hold.
    #[serde(default)]
    pub requires_version: Vec<VersionWriterName>,
    /// The files a unit it builds must hold, each relative to the unit's
    /// path: what the build reads and fails without.
    #[serde(default)]
    pub requires_file: Vec<RelativePath>,
    /// The programs it calls, each pinned under `[program.*]`.
    #[serde(default)]
    pub programs: Vec<ProgramName>,
}

/// What a target takes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Target {
    /// The artifact kinds it accepts.
    pub accepts: Vec<ArtifactKind>,
    /// A check the unit must run before it may publish here.
    #[serde(default)]
    pub requires_check: Option<CheckName>,
    /// How a unit that publishes here is released when its declaration does
    /// not say. Required: a target that left it open would leave the default
    /// of every unit publishing there open as well.
    pub release: Release,
}

/// A named pipeline, looked up instead of written out.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    /// The version writers that receive the unit's version as a derived copy.
    #[serde(default)]
    pub version: Vec<VersionWriterName>,
    /// Checks beyond the base set.
    pub check: Vec<CheckName>,
    /// The build, if the preset builds anything.
    #[serde(default)]
    pub build: Option<BuildName>,
    /// Where the artifact goes.
    pub publish: Publish,
    /// The ecosystems Dependabot watches for it.
    #[serde(default)]
    pub dependabot: Vec<EcosystemName>,
}

/// Why a register could not be read.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RegisterError {
    /// The TOML did not parse, or did not match the shape above.
    #[error("the register is not readable: {0}")]
    Unreadable(#[from] toml::de::Error),
    /// An entry pointed at a name no other entry defines.
    #[error("{holder} refers to the {kind} {name:?}, which the register does not define")]
    Dangling {
        /// The entry that holds the dangling reference.
        holder: String,
        /// What kind of name it referred to.
        kind: &'static str,
        /// The name.
        name: String,
    },
    /// A program or runtime pins something that is not a version.
    #[error("{holder} pins {version:?}, and a version here is three numbers, such as 1.2.3")]
    Version {
        /// The program or runtime.
        holder: String,
        /// What it pins.
        version: String,
    },
    /// A program or runtime has downloads that do not fit it.
    #[error("{holder} {problem}")]
    Download {
        /// The program or runtime.
        holder: String,
        /// What does not fit.
        problem: &'static str,
    },
    /// A program's lowest runtime version is missing, stray or above the
    /// version the register pins for that runtime.
    #[error("{holder} {problem}")]
    Minimum {
        /// The program.
        holder: String,
        /// What does not fit.
        problem: String,
    },
}

/// Refuses a program on a runtime that names no lowest version of it, one on
/// no runtime that names one, and a lowest version above the version the
/// register pins for the runtime: the pinned runtime could not run it.
fn minimum(
    holder: &ProgramName,
    runtime: Option<(&RuntimeName, &Runtime)>,
    minimum: Option<&str>,
) -> Result<(), RegisterError> {
    let problem = match (runtime, minimum) {
        (None, None) => return Ok(()),
        (Some(_), None) => "runs on a runtime, so it names the lowest version of it that it runs \
                            on (`runtime-minimum`)"
            .to_owned(),
        (None, Some(_)) => "names a `runtime-minimum`, and runs on no runtime".to_owned(),
        (Some((name, runtime)), Some(minimum)) => {
            pinned(&format!("the `runtime-minimum` of {holder}"), minimum)?;
            if at_least(&runtime.version, minimum) {
                return Ok(());
            }
            format!(
                "needs {name} {minimum} or later, and the register pins {name} {}",
                runtime.version
            )
        }
    };
    Err(RegisterError::Minimum {
        holder: holder.to_string(),
        problem,
    })
}

/// Refuses a pinned version that is not three numbers.
fn pinned(holder: &dyn std::fmt::Display, version: &str) -> Result<(), RegisterError> {
    if is_version(version) {
        Ok(())
    } else {
        Err(RegisterError::Version {
            holder: holder.to_string(),
            version: version.to_owned(),
        })
    }
}

/// The register this crate was built with.
pub const BUILTIN_REGISTER: &str = include_str!("../register.toml");

/// The defaults this crate was built with.
pub const BUILTIN_DEFAULTS: &str = include_str!("../defaults.toml");

impl Register {
    /// The register compiled into this crate.
    ///
    /// # Errors
    ///
    /// Returns [`RegisterError`] only when this crate was built with a broken
    /// register, which its own tests rule out.
    pub fn builtin() -> Result<Self, RegisterError> {
        Self::parse(BUILTIN_REGISTER)
    }

    /// Reads a register, and refuses one whose entries point at nothing.
    ///
    /// # Errors
    ///
    /// Returns [`RegisterError`] if the TOML does not parse or a preset, build
    /// or target names something the register does not define.
    pub fn parse(source: &str) -> Result<Self, RegisterError> {
        let register: Self = toml::from_str(source)?;
        register.resolved()?;
        Ok(register)
    }

    fn resolved(&self) -> Result<(), RegisterError> {
        let dangling = |holder: &dyn std::fmt::Display, kind, name: &dyn std::fmt::Display| {
            RegisterError::Dangling {
                holder: holder.to_string(),
                kind,
                name: name.to_string(),
            }
        };

        for (name, check) in &self.check {
            for program in &check.programs {
                if !self.program.contains_key(program) {
                    return Err(dangling(name, "program", program));
                }
            }
        }
        // The runtimes first: a program's lowest runtime version is held
        // against the version its runtime pins, which has to be one itself.
        for (name, runtime) in &self.runtime {
            pinned(name, &runtime.version)?;
            if let Some(problem) = download_problem(&runtime.download, false) {
                return Err(RegisterError::Download {
                    holder: name.to_string(),
                    problem,
                });
            }
        }
        for (name, program) in &self.program {
            let runtime = match &program.runtime {
                Some(runtime) => Some((
                    runtime,
                    self.runtime
                        .get(runtime)
                        .ok_or_else(|| dangling(name, "runtime", runtime))?,
                )),
                None => None,
            };
            pinned(name, &program.version)?;
            if let Some(problem) = download_problem(&program.download, runtime.is_some()) {
                return Err(RegisterError::Download {
                    holder: name.to_string(),
                    problem,
                });
            }
            minimum(name, runtime, program.runtime_minimum.as_deref())?;
        }
        for (name, build) in &self.build {
            for writer in &build.requires_version {
                if !self.version.contains_key(writer) {
                    return Err(dangling(name, "version writer", writer));
                }
            }
            for program in &build.programs {
                if !self.program.contains_key(program) {
                    return Err(dangling(name, "program", program));
                }
            }
        }
        for (name, target) in &self.target {
            if let Some(check) = &target.requires_check
                && !self.check.contains_key(check)
            {
                return Err(dangling(name, "check", check));
            }
        }
        for (name, preset) in &self.preset {
            for writer in &preset.version {
                if !self.version.contains_key(writer) {
                    return Err(dangling(name, "version writer", writer));
                }
            }
            for check in &preset.check {
                if !self.check.contains_key(check) {
                    return Err(dangling(name, "check", check));
                }
            }
            if let Some(build) = &preset.build
                && !self.build.contains_key(build)
            {
                return Err(dangling(name, "build", build));
            }
            for ecosystem in &preset.dependabot {
                if !self.ecosystem.contains_key(ecosystem) {
                    return Err(dangling(name, "ecosystem", ecosystem));
                }
            }
            if let Publish::To(targets) = &preset.publish {
                for target in targets {
                    if !self.target.contains_key(target) {
                        return Err(dangling(name, "target", target));
                    }
                }
            }
        }
        Ok(())
    }

    /// The programs these checks call, each once, in name order.
    #[must_use]
    pub fn programs_of<'a>(
        &'a self,
        checks: impl IntoIterator<Item = &'a CheckName>,
    ) -> Vec<(&'a ProgramName, &'a Program)> {
        let mut names: Vec<&ProgramName> = checks
            .into_iter()
            .filter_map(|check| self.check.get(check))
            .flat_map(|check| &check.programs)
            .collect();
        names.sort();
        names.dedup();
        names
            .into_iter()
            .filter_map(|name| self.program.get_key_value(name))
            .collect()
    }

    /// The ecosystem whose manifest this file is, if it is one.
    #[must_use]
    pub fn ecosystem_of(&self, path: &str) -> Option<&EcosystemName> {
        let name = path.rsplit('/').next().unwrap_or(path);
        self.ecosystem
            .iter()
            .find(|(_, definition)| definition.manifests.iter().any(|known| known == name))
            .map(|(ecosystem, _)| ecosystem)
    }

    /// Whether this file is a lockfile the register names: a whole file name
    /// that a check covers and that ends in `.lock`. A check covers a lockfile
    /// by its name, never by the extension `.lock`, which formats that have
    /// nothing in common share; its building block parses it and leaves its
    /// formatting alone.
    #[must_use]
    pub fn is_lockfile(&self, path: &str) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        // Matched as exactly as the names are, so `.LOCK` is no lockfile.
        name.rsplit_once('.')
            .is_some_and(|(_, ending)| ending == "lock")
            && self
                .check
                .values()
                .any(|definition| definition.filenames.iter().any(|known| known == name))
    }

    /// The check that covers a file, if any check does.
    #[must_use]
    pub fn check_for(&self, path: &str) -> Option<&CheckName> {
        let name = path.rsplit('/').next().unwrap_or(path);
        let extension = Extension::of(path);
        self.check.iter().find_map(|(check, definition)| {
            let by_name = definition.filenames.iter().any(|known| known == name);
            let by_extension = extension
                .as_ref()
                .is_some_and(|found| definition.extensions.contains(found));
            (by_name || by_extension).then_some(check)
        })
    }
}
