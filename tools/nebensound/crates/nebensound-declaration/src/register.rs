//! The central register of presets, version writers, checks, builds and
//! targets.
//!
//! It is data, not code, and it is compiled into this crate. A repository
//! therefore never holds a copy that could fall behind: adopting a change to
//! the register means raising this crate.

use std::collections::BTreeMap;

use serde::Deserialize;
use thiserror::Error;

use crate::model::Publish;
use crate::names::{
    ArtifactKind, BuildName, CheckName, EcosystemName, Extension, PresetName, TargetName,
    VersionWriterName,
};

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
}

/// What a build produces.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    /// The kind of artifact it leaves behind.
    pub produces: ArtifactKind,
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

    /// The ecosystem whose manifest this file is, if it is one.
    #[must_use]
    pub fn ecosystem_of(&self, path: &str) -> Option<&EcosystemName> {
        let name = path.rsplit('/').next().unwrap_or(path);
        self.ecosystem
            .iter()
            .find(|(_, definition)| definition.manifests.iter().any(|known| known == name))
            .map(|(ecosystem, _)| ecosystem)
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
