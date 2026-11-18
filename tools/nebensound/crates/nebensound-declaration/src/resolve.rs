//! Turning what is written into what actually holds.
//!
//! Defaults, requested defaults and presets each let a value arrive from
//! somewhere other than where it is read, so phase 0 is expected to log the
//! resolved configuration. That is what this module produces.

use std::collections::{BTreeMap, BTreeSet};

use crate::finding::{Finding, Rule, UnitName};
use crate::model::{Declaration, Pipeline, Publish, Versioning};
use crate::names::{
    BuildName, CheckName, ComponentName, EcosystemName, PresetName, Reason, VersionWriterName,
};
use crate::path::RelativePath;
use crate::register::Register;
use crate::tree::Tree;

/// Where a unit came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Someone wrote it down.
    Declared,
    /// A `child-` versioning value brought it into being.
    Generated {
        /// The unit that generated it.
        by: UnitName,
    },
}

/// Where a resolved pipeline came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineSource {
    /// Nothing was said, so nothing runs.
    Default,
    /// A preset from the register.
    Preset(PresetName),
    /// The default `[repo.component]` offers.
    RequestedDefault,
    /// A table written out in place.
    WrittenOut,
    /// Inherited from the unit that generated this one.
    Inherited,
}

/// A pipeline with every name resolved against the register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPipeline {
    /// Where it came from, so a log can say so.
    pub source: PipelineSource,
    /// The version writers that receive the unit's version as a derived copy.
    pub version: Vec<VersionWriterName>,
    /// Checks beyond the base set.
    pub checks: Vec<CheckName>,
    /// The build, if anything is built.
    pub build: Option<BuildName>,
    /// Where the artifact goes.
    pub publish: Publish,
    /// The ecosystems Dependabot watches for it.
    pub dependabot: Vec<EcosystemName>,
}

impl ResolvedPipeline {
    fn empty(source: PipelineSource) -> Self {
        Self {
            source,
            version: Vec::new(),
            checks: Vec::new(),
            build: None,
            publish: Publish::Nothing,
            dependabot: Vec::new(),
        }
    }

    /// The targets this pipeline publishes to.
    #[must_use]
    pub fn targets(&self) -> &[crate::names::TargetName] {
        match &self.publish {
            Publish::Nothing => &[],
            Publish::To(targets) => targets,
        }
    }
}

/// One unit, with everything about it decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// Its name, which is also its tag prefix.
    pub name: UnitName,
    /// Where it lives.
    pub path: RelativePath,
    /// Whether someone wrote it down or something generated it.
    pub origin: Origin,
    /// What runs for it.
    pub pipeline: ResolvedPipeline,
    /// What it says about the versions around it.
    pub versioning: Versioning,
    /// Whether it carries a version of its own.
    pub carries_version: bool,
    /// Checks it allows to find nothing.
    pub allow_unused: BTreeMap<CheckName, Reason>,
}

impl Unit {
    /// Where its version is kept, when it carries one.
    #[must_use]
    pub fn version_file(&self) -> Option<RelativePath> {
        if !self.carries_version {
            return None;
        }
        let base = RelativePath::root().join(".nebensound");
        Some(match &self.name {
            UnitName::Repository => base.join("version"),
            UnitName::Component(name) => base.join("versions").join(name.as_str()),
        })
    }
}

/// Everything the declaration means, and everything wrong with it so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The units, repository first.
    pub units: Vec<Unit>,
    /// What resolving them turned up.
    pub findings: Vec<Finding>,
}

/// Resolves a declaration against the register and the tree.
#[must_use]
pub fn resolve(declaration: &Declaration, register: &Register, tree: &Tree) -> Resolution {
    let mut findings = Vec::new();
    let mut units = vec![repository_unit(declaration, register, &mut findings)];

    for (name, component) in &declaration.component {
        let unit = UnitName::Component(name.clone());
        let versioning = component.versioning.unwrap_or_default();
        let pipeline = resolve_pipeline(
            component.pipeline.as_ref(),
            declaration,
            register,
            &unit,
            &mut findings,
        );
        units.push(Unit {
            name: unit,
            path: component.path.clone(),
            origin: Origin::Declared,
            pipeline,
            versioning,
            carries_version: versioning.covers_self(),
            allow_unused: component.allow_unused.clone(),
        });
    }

    let generated = generate_children(&units, tree, &mut findings);
    units.extend(generated);

    Resolution { units, findings }
}

fn repository_unit(
    declaration: &Declaration,
    register: &Register,
    findings: &mut Vec<Finding>,
) -> Unit {
    let declared = declaration.repo.versioning.unwrap_or_default();
    let versioning = if declared.covers_self() {
        declared
    } else {
        findings.push(
            Finding::error(
                Rule::RepositoryWithoutVersion,
                format!(
                    "the repository declares versioning = \"{declared}\", which would leave it \
                     without a version of its own; it always describes a defined state, so only \
                     the values that include `self` are open to it"
                ),
            )
            .about(UnitName::Repository),
        );
        Versioning::Own
    };

    Unit {
        name: UnitName::Repository,
        path: RelativePath::root(),
        origin: Origin::Declared,
        pipeline: resolve_pipeline(
            declaration.repo.pipeline.as_ref(),
            declaration,
            register,
            &UnitName::Repository,
            findings,
        ),
        versioning,
        carries_version: true,
        allow_unused: declaration.repo.allow_unused.clone(),
    }
}

fn resolve_pipeline(
    pipeline: Option<&Pipeline>,
    declaration: &Declaration,
    register: &Register,
    unit: &UnitName,
    findings: &mut Vec<Finding>,
) -> ResolvedPipeline {
    match pipeline {
        None => ResolvedPipeline::empty(PipelineSource::Default),
        Some(Pipeline::Preset(name)) => from_preset(name, register, unit, findings),
        Some(Pipeline::Explicit(written)) => {
            let mut resolved = ResolvedPipeline::empty(PipelineSource::WrittenOut);
            for writer in &written.version {
                if register.version.contains_key(writer) {
                    resolved.version.push(writer.clone());
                } else {
                    findings.push(unknown_entry(unit, "version writer", writer));
                }
            }
            for check in &written.check {
                if register.check.contains_key(check) {
                    resolved.checks.push(check.clone());
                } else {
                    findings.push(unknown_entry(unit, "check", check));
                }
            }
            for ecosystem in &written.dependabot {
                if register.ecosystem.contains_key(ecosystem) {
                    resolved.dependabot.push(ecosystem.clone());
                } else {
                    findings.push(unknown_entry(unit, "ecosystem", ecosystem));
                }
            }
            if let Some(build) = &written.build {
                if register.build.contains_key(build) {
                    resolved.build = Some(build.clone());
                } else {
                    findings.push(unknown_entry(unit, "build", build));
                }
            }
            resolved.publish = match &written.publish {
                None | Some(Publish::Nothing) => Publish::Nothing,
                Some(Publish::To(targets)) => Publish::To(
                    targets
                        .iter()
                        .filter(|target| {
                            let known = register.target.contains_key(*target);
                            if !known {
                                findings.push(unknown_entry(unit, "target", target));
                            }
                            known
                        })
                        .cloned()
                        .collect(),
                ),
            };
            resolved
        }
        Some(Pipeline::FromRepo) => {
            let offered = declaration
                .repo
                .component
                .as_ref()
                .and_then(|defaults| defaults.pipeline.as_ref());
            match offered {
                None => {
                    findings.push(
                        Finding::error(
                            Rule::NoRequestableDefault,
                            "this asks for the default in [repo.component], and there is none",
                        )
                        .about(unit.clone()),
                    );
                    ResolvedPipeline::empty(PipelineSource::RequestedDefault)
                }
                Some(Pipeline::FromRepo) => {
                    findings.push(
                        Finding::error(
                            Rule::NoRequestableDefault,
                            "the default in [repo.component] asks for itself",
                        )
                        .about(unit.clone()),
                    );
                    ResolvedPipeline::empty(PipelineSource::RequestedDefault)
                }
                Some(offered) => {
                    let mut resolved =
                        resolve_pipeline(Some(offered), declaration, register, unit, findings);
                    resolved.source = PipelineSource::RequestedDefault;
                    resolved
                }
            }
        }
    }
}

fn from_preset(
    name: &PresetName,
    register: &Register,
    unit: &UnitName,
    findings: &mut Vec<Finding>,
) -> ResolvedPipeline {
    let Some(preset) = register.preset.get(name) else {
        let known = register
            .preset
            .keys()
            .map(PresetName::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        findings.push(
            Finding::error(
                Rule::UnknownPreset,
                format!("the register has no preset {name:?}; it offers {known}"),
            )
            .about(unit.clone()),
        );
        return ResolvedPipeline::empty(PipelineSource::Preset(name.clone()));
    };
    ResolvedPipeline {
        source: PipelineSource::Preset(name.clone()),
        version: preset.version.clone(),
        checks: preset.check.clone(),
        build: preset.build.clone(),
        publish: preset.publish.clone(),
        dependabot: preset.dependabot.clone(),
    }
}

fn unknown_entry(unit: &UnitName, kind: &str, name: &dyn std::fmt::Display) -> Finding {
    Finding::error(
        Rule::UnknownRegisterEntry,
        format!("the register has no {kind} {name}"),
    )
    .about(unit.clone())
}

fn generate_children(declared: &[Unit], tree: &Tree, findings: &mut Vec<Finding>) -> Vec<Unit> {
    let mut taken: BTreeSet<ComponentName> = declared
        .iter()
        .filter_map(|unit| match &unit.name {
            UnitName::Component(name) => Some(name.clone()),
            UnitName::Repository => None,
        })
        .collect();
    let mut generated = Vec::new();

    for parent in declared {
        if !parent.versioning.generates_children() {
            continue;
        }
        let children = if parent.versioning.covers_child_files() {
            tree.child_files(&parent.path)
        } else {
            tree.child_folders(&parent.path)
        };
        if children.is_empty() {
            findings.push(
                Finding::warning(
                    Rule::ChildlessGenerator,
                    format!(
                        "versioning = \"{}\" promises children to version, and there are none \
                         directly below this path",
                        parent.versioning
                    ),
                )
                .about(parent.name.clone())
                .at(parent.path.clone()),
            );
            continue;
        }
        for child in children {
            match child_name(&child) {
                None => findings.push(
                    Finding::error(
                        Rule::UngeneratableName,
                        "this child cannot be turned into a name that would survive a Git tag; \
                         write it out as a component with a name of its own",
                    )
                    .about(parent.name.clone())
                    .at(child.clone()),
                ),
                Some(name) if taken.contains(&name) => findings.push(
                    Finding::error(
                        Rule::UngeneratableName,
                        format!(
                            "this child would be named {name}, and that name is already taken; \
                             two units cannot share a tag prefix"
                        ),
                    )
                    .about(parent.name.clone())
                    .at(child.clone()),
                ),
                Some(name) => {
                    taken.insert(name.clone());
                    generated.push(Unit {
                        name: UnitName::Component(name),
                        path: child,
                        origin: Origin::Generated {
                            by: parent.name.clone(),
                        },
                        // A generated unit has nowhere to say anything, so it
                        // inherits everything from the unit that made it.
                        pipeline: ResolvedPipeline {
                            source: PipelineSource::Inherited,
                            ..parent.pipeline.clone()
                        },
                        versioning: Versioning::Own,
                        carries_version: true,
                        allow_unused: parent.allow_unused.clone(),
                    });
                }
            }
        }
    }
    generated
}

/// The name a generated unit gets: the child's own name without its extension.
fn child_name(path: &RelativePath) -> Option<ComponentName> {
    // Only ever called with a child of some unit, so the root — the one path
    // without a last segment — cannot arrive here. An empty name is refused
    // below rather than assumed away.
    let last = path.file_name().unwrap_or_default();
    let stem = match last.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => last,
    };
    let mut cleaned = String::with_capacity(stem.len());
    for character in stem.chars() {
        match character.to_ascii_lowercase() {
            c @ ('a'..='z' | '0'..='9') => cleaned.push(c),
            _ => cleaned.push('-'),
        }
    }
    ComponentName::new(cleaned.trim_matches('-')).ok()
}
