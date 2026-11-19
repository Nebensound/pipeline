//! The declaration exactly as it is written, before anything is resolved.
//!
//! Unknown fields are an error rather than a warning. A misspelled `publsh`
//! would otherwise mean, quietly, that nothing is published.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use crate::names::{
    BuildName, CheckName, ComponentName, EcosystemName, PresetName, Reason, TargetName,
    VersionWriterName,
};
use crate::path::RelativePath;
use crate::pattern::Pattern;
use crate::version::TagVersion;

/// The contents of `.nebensound/repo.toml`.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    /// What is pulled back into the check — paths and patterns.
    #[serde(default)]
    pub include: Vec<Inclusion>,
    /// The repository itself, which is a unit like any other.
    ///
    /// It is required: a file without it would not be a statement, and the
    /// point of demanding the file everywhere is to tell "nothing is published
    /// here" apart from "nobody ever set this up".
    pub repo: Repo,
    /// Components, keyed by the name that also becomes their tag prefix.
    #[serde(default)]
    pub component: BTreeMap<ComponentName, Component>,
    /// What falls out of the coverage check, each with its reason.
    ///
    /// In the declaration of a repository only concrete paths are allowed here:
    /// a pattern would excuse a whole class of files, and extensions are
    /// decided by the central list.
    #[serde(default)]
    pub exclude: Vec<Exclusion>,
}

/// An entry that pulls something back into the check.
///
/// The reason is optional here: adding work is harmless and does not have to
/// justify itself. The same table shape all the same, so that both directions
/// look alike and an entry can carry one more field later.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inclusion {
    /// What it applies to.
    pub path: Pattern,
    /// Why, if anyone wrote it down.
    #[serde(default)]
    pub reason: Option<Reason>,
}

/// An entry that takes something out of the check.
///
/// Two fields instead of a key against a value: the entry names something and
/// justifies it, and both belong visibly named. Being a table also keeps it
/// open — were a third field ever to arrive, the shape would not have to be
/// rebuilt for it.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exclusion {
    /// What it applies to.
    ///
    /// In a repository's own file only a concrete path, so that is what the
    /// schema of that file allows; the central defaults may name a pattern.
    #[cfg_attr(feature = "json-schema", schemars(with = "RelativePath"))]
    pub path: Pattern,
    /// Why nobody looks.
    pub reason: Reason,
}

/// The repository as a unit.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Repo {
    /// What runs for it.
    #[serde(default)]
    pub pipeline: Option<Pipeline>,
    /// Which units around it carry a version.
    #[serde(default)]
    pub versioning: Option<Versioning>,
    /// Whether it is released in one stage or in two. Left out, its publish
    /// targets decide.
    #[serde(default)]
    pub release: Option<Release>,
    /// The version it gets is never lower than this: `X.Y.Z`, or
    /// `X.Y.Z-pre.N` for a pre-release of a unit released in two stages.
    #[serde(default)]
    pub minimum_version: Option<TagVersion>,
    /// Checks that may find nothing without failing, each with its reason.
    #[serde(default)]
    pub allow_unused: BTreeMap<CheckName, Reason>,
    /// The default a component can ask for with `pipeline.repo = true`.
    #[serde(default)]
    pub component: Option<ComponentDefaults>,
}

/// What `[repo.component]` offers. Only a pipeline, and only on request.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ComponentDefaults {
    /// The pipeline a component receives when it asks for it.
    #[serde(default)]
    pub pipeline: Option<Pipeline>,
}

/// A part of the repository that is a unit of its own.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Component {
    /// Where it lives. May name a single file when exactly that file is the
    /// unit.
    pub path: RelativePath,
    /// What runs for it.
    #[serde(default)]
    pub pipeline: Option<Pipeline>,
    /// Which units around it carry a version.
    #[serde(default)]
    pub versioning: Option<Versioning>,
    /// Whether it is released in one stage or in two. Left out, its publish
    /// targets decide.
    #[serde(default)]
    pub release: Option<Release>,
    /// The version it gets is never lower than this: `X.Y.Z`, or
    /// `X.Y.Z-pre.N` for a pre-release of a unit released in two stages.
    #[serde(default)]
    pub minimum_version: Option<TagVersion>,
    /// Checks that may find nothing without failing, each with its reason.
    #[serde(default)]
    pub allow_unused: BTreeMap<CheckName, Reason>,
}

/// How a unit's version reaches a full release.
///
/// Written down rather than read off the repository: whether a branch
/// `release` exists says nothing, since a deleted branch must not turn the next
/// merge into a full release.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Release {
    /// A merge to `main` is a full release.
    #[serde(rename = "single-stage")]
    SingleStage,
    /// A merge to `main` is a pre-release, and a pull request from `main` into
    /// `release` makes the full release.
    #[serde(rename = "two-stage")]
    TwoStage,
}

impl Release {
    /// The value as written in the file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SingleStage => "single-stage",
            Self::TwoStage => "two-stage",
        }
    }
}

impl fmt::Display for Release {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which units around a declaring unit carry a version.
///
/// Every value names the whole effect rather than half of it, so that reading
/// one tells you whether the declaring unit itself is included.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
pub enum Versioning {
    /// Only this unit.
    #[default]
    #[serde(rename = "self")]
    Own,
    /// This unit and each folder directly below it.
    #[serde(rename = "self-and-child-folders")]
    OwnAndChildFolders,
    /// This unit and each file directly below it.
    #[serde(rename = "self-and-child-files")]
    OwnAndChildFiles,
    /// Only the folders directly below it.
    #[serde(rename = "child-folders")]
    ChildFolders,
    /// Only the files directly below it.
    #[serde(rename = "child-files")]
    ChildFiles,
    /// None of them; only units written out separately carry a version.
    #[serde(rename = "none")]
    Nothing,
}

impl Versioning {
    /// Whether the declaring unit itself carries a version.
    #[must_use]
    pub fn covers_self(self) -> bool {
        matches!(
            self,
            Self::Own | Self::OwnAndChildFolders | Self::OwnAndChildFiles
        )
    }

    /// Whether folders directly below carry one.
    #[must_use]
    pub fn covers_child_folders(self) -> bool {
        matches!(self, Self::OwnAndChildFolders | Self::ChildFolders)
    }

    /// Whether files directly below carry one.
    #[must_use]
    pub fn covers_child_files(self) -> bool {
        matches!(self, Self::OwnAndChildFiles | Self::ChildFiles)
    }

    /// Whether this value makes the unit generate children of its own.
    #[must_use]
    pub fn generates_children(self) -> bool {
        self.covers_child_folders() || self.covers_child_files()
    }

    /// The value as written in the file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Own => "self",
            Self::OwnAndChildFolders => "self-and-child-folders",
            Self::OwnAndChildFiles => "self-and-child-files",
            Self::ChildFolders => "child-folders",
            Self::ChildFiles => "child-files",
            Self::Nothing => "none",
        }
    }
}

impl fmt::Display for Versioning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What runs for a unit.
///
/// Either a preset name or the table, and nothing in between: a preset with
/// partial overrides would let one value arrive from three directions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pipeline {
    /// A preset from the central register, looked up by name.
    Preset(PresetName),
    /// Whatever `[repo.component]` offers.
    FromRepo,
    /// Everything written out.
    Explicit(ExplicitPipeline),
}

/// A pipeline written out rather than named.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
// Read through `RawPipeline`; the schema describes what that accepts.
#[cfg_attr(
    feature = "json-schema",
    derive(schemars::JsonSchema),
    schemars(deny_unknown_fields, default)
)]
pub struct ExplicitPipeline {
    /// The version writers that receive the unit's version as a derived copy.
    ///
    /// A list, like `check`: one unit can carry more than one copy.
    pub version: Vec<VersionWriterName>,
    /// Checks beyond the base set every repository runs.
    pub check: Vec<CheckName>,
    /// The build that produces the artifact.
    pub build: Option<BuildName>,
    /// Where the artifact goes.
    pub publish: Option<Publish>,
    /// The ecosystems Dependabot watches for this unit.
    ///
    /// A list, because a unit can have several — a service has its language
    /// ecosystem and its container.
    pub dependabot: Vec<EcosystemName>,
}

/// Where a unit's artifact goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Publish {
    /// Nowhere, said out loud.
    Nothing,
    /// One or more targets from the central register.
    To(Vec<TargetName>),
}

/// The pipeline table as serde sees it, before the combinations are judged.
///
/// Letting serde read the table rather than walking it by hand is what makes a
/// misspelled key report as an unknown field, with the ones it could have been.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPipeline {
    repo: Option<bool>,
    version: Option<Vec<VersionWriterName>>,
    check: Option<Vec<CheckName>>,
    build: Option<BuildName>,
    publish: Option<Publish>,
    dependabot: Option<Vec<EcosystemName>>,
}

impl<'de> Deserialize<'de> for Pipeline {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(PipelineVisitor)
    }
}

struct PipelineVisitor;

impl<'de> Visitor<'de> for PipelineVisitor {
    type Value = Pipeline;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a preset name, `{ repo = true }`, or a table of version, check, build, publish \
             and dependabot",
        )
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        PresetName::new(value)
            .map(Pipeline::Preset)
            .map_err(E::custom)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        let raw = RawPipeline::deserialize(MapAccessDeserializer::new(map))?;
        let written_out = raw.version.is_some()
            || raw.check.is_some()
            || raw.build.is_some()
            || raw.publish.is_some()
            || raw.dependabot.is_some();

        match (raw.repo, written_out) {
            (Some(_), true) => Err(de::Error::custom(
                "a pipeline is either `repo = true` or a table written out; \
                 a requested default with parts overridden is not a form this format has",
            )),
            (Some(true), false) => Ok(Pipeline::FromRepo),
            (Some(false), false) => Err(de::Error::custom(
                "`repo = false` says nothing; leave the pipeline out to take the default",
            )),
            (None, _) => Ok(Pipeline::Explicit(ExplicitPipeline {
                version: raw.version.unwrap_or_default(),
                check: raw.check.unwrap_or_default(),
                build: raw.build,
                publish: raw.publish,
                dependabot: raw.dependabot.unwrap_or_default(),
            })),
        }
    }
}

impl<'de> Deserialize<'de> for Publish {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(PublishVisitor)
    }
}

struct PublishVisitor;

impl<'de> Visitor<'de> for PublishVisitor {
    type Value = Publish;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("`false`, or a list of target names")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        if value {
            return Err(E::custom(
                "`publish = true` names no target; write the targets as a list",
            ));
        }
        Ok(Publish::Nothing)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        Vec::deserialize(SeqAccessDeserializer::new(seq)).map(Publish::To)
    }
}
