//! The phase 0 rules that need the tree as well as the declaration.
//!
//! The promise a check makes is total: 100 % of the files with its extensions,
//! inside its scope. These rules are what turns that from a claim into
//! something the run can fail on.

use std::collections::BTreeSet;

use crate::annotate::{Annotated, Coverage, annotate};
use crate::central::{Central, Ruling};
use crate::finding::{Finding, Rule, UnitName};
use crate::model::Declaration;
use crate::names::CheckName;
use crate::resolve::{Resolution, Unit};
use crate::tree::Tree;

/// Checks a resolution against the tree it was resolved for.
#[must_use]
pub fn validate(
    declaration: &Declaration,
    resolution: &Resolution,
    central: &Central,
    tree: &Tree,
) -> Vec<Finding> {
    let annotated = annotate(declaration, resolution, central, tree);
    findings_for(declaration, resolution, central, tree, &annotated)
}

/// The same, with the tree already built: [`analyse`] builds it anyway, and the
/// coverage is read off it rather than decided a second time.
fn findings_for(
    declaration: &Declaration,
    resolution: &Resolution,
    central: &Central,
    tree: &Tree,
    annotated: &Annotated,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    paths_exist(resolution, tree, &mut findings);
    every_file_is_covered(annotated, &mut findings);
    every_check_finds_something(resolution, central, tree, &mut findings);
    suppressions_are_used(resolution, &mut findings);
    exclusions_are_used(declaration, tree, &mut findings);
    inclusions_are_used(declaration, central, tree, &mut findings);
    manifests_are_watched(resolution, central, tree, &mut findings);
    publication_is_reachable(resolution, central, &mut findings);
    version_writers_have_a_version(resolution, &mut findings);
    findings
}

fn paths_exist(resolution: &Resolution, tree: &Tree, findings: &mut Vec<Finding>) {
    for unit in &resolution.units {
        if !tree.holds(&unit.path) {
            findings.push(
                Finding::error(
                    Rule::MissingPath,
                    "this path holds nothing; a unit that names a path the repository does not \
                     have checks and versions nothing",
                )
                .about(unit.name.clone())
                .at(unit.path.clone()),
            );
        }
    }
}

/// The gap in the tree, as a finding. It was decided while annotating; here it
/// is only reported.
fn every_file_is_covered(annotated: &Annotated, findings: &mut Vec<Finding>) {
    for (node, coverage) in annotated.files() {
        match coverage {
            Coverage::Covered(_) | Coverage::Excluded { .. } => {}
            Coverage::Unknown => findings.push(
                Finding::error(
                    Rule::LanguageWithoutCheck,
                    "no check in the register covers this kind of file. Add one centrally, if \
                     need be one that fails until it is written — then the gap reports itself \
                     instead of hiding",
                )
                .at(node.path.clone()),
            ),
            Coverage::Uncovered(covering) => findings.push(
                Finding::error(
                    Rule::UncoveredFile,
                    format!(
                        "the {covering} check would cover this file, and no unit containing it \
                         runs it"
                    ),
                )
                .at(node.path.clone()),
            ),
        }
    }
}

/// Whether a unit's scope holds a file this check would cover.
fn check_finds_something(unit: &Unit, check: &CheckName, central: &Central, tree: &Tree) -> bool {
    tree.files()
        .filter(|file| unit.path.contains(file))
        .any(|file| central.register().check_for(&file.to_string()) == Some(check))
}

fn every_check_finds_something(
    resolution: &Resolution,
    central: &Central,
    tree: &Tree,
    findings: &mut Vec<Finding>,
) {
    for unit in &resolution.units {
        for check in &unit.pipeline.checks {
            if unit.allow_unused.contains_key(check) {
                continue;
            }
            if !check_finds_something(unit, check, central, tree) {
                findings.push(
                    Finding::error(
                        Rule::UnusedCheck,
                        format!(
                            "this unit runs the {check} check and holds no file it covers; either \
                             the check is wrong here, or say why it stays with allow-unused"
                        ),
                    )
                    .about(unit.name.clone())
                    .at(unit.path.clone()),
                );
            }
        }
    }
}

fn suppressions_are_used(resolution: &Resolution, findings: &mut Vec<Finding>) {
    for unit in &resolution.units {
        for check in unit.allow_unused.keys() {
            if !unit.pipeline.checks.contains(check) {
                findings.push(
                    Finding::warning(
                        Rule::PointlessSuppression,
                        format!(
                            "allow-unused silences the {check} check, and this unit does not run it"
                        ),
                    )
                    .about(unit.name.clone()),
                );
            }
        }
    }
}

fn exclusions_are_used(declaration: &Declaration, tree: &Tree, findings: &mut Vec<Finding>) {
    for entry in &declaration.exclude {
        if !tree.files().any(|file| entry.path.matches(file)) {
            findings.push(Finding::warning(
                Rule::PointlessExclusion,
                format!(
                    "{} matches nothing, so the excuse ({}) covers nothing",
                    entry.path, entry.reason
                ),
            ));
        }
    }
}

fn inclusions_are_used(
    declaration: &Declaration,
    central: &Central,
    tree: &Tree,
    findings: &mut Vec<Finding>,
) {
    for (position, entry) in declaration.include.iter().enumerate() {
        // The same pattern twice says the same thing, so it is judged once.
        if declaration.include[..position]
            .iter()
            .any(|earlier| earlier.path == entry.path)
        {
            continue;
        }

        // Without this entry: would any file it matches stay excused? If not,
        // it pulls nothing back. Every copy of it has to go — the remaining one
        // would otherwise keep the file checked, and a pattern doing its job
        // would be reported as pointless.
        let mut without = declaration.clone();
        without.include.retain(|other| other.path != entry.path);

        let holds_back = tree
            .files()
            .filter(|file| entry.path.matches(file))
            .any(|file| matches!(central.ruling(&without, file), Ruling::Excluded { .. }));

        if !holds_back {
            findings.push(Finding::warning(
                Rule::PointlessInclusion,
                format!("{} pulls nothing back: nothing takes it out", entry.path),
            ));
        }
    }
}

/// The checks that hold for a unit: its own, those of every unit containing it,
/// and the base set.
fn checks_in_force<'a>(
    resolution: &'a Resolution,
    central: &'a Central,
    unit: &Unit,
) -> BTreeSet<&'a CheckName> {
    let mut checks: BTreeSet<&CheckName> = central.base_checks().iter().collect();
    for other in &resolution.units {
        if other.path.contains(&unit.path) {
            checks.extend(other.pipeline.checks.iter());
        }
    }
    checks
}

/// A manifest belongs watched, and what is watched should exist.
///
/// Both are a **warning**, not an error. A check that finds nothing is an error
/// because its promise is then empty; a missing Dependabot entry leaves
/// dependencies out of date — annoying, but it makes no statement untrue.
fn manifests_are_watched(
    resolution: &Resolution,
    central: &Central,
    tree: &Tree,
    findings: &mut Vec<Finding>,
) {
    for file in tree.files() {
        let Some(ecosystem) = central.register().ecosystem_of(&file.to_string()) else {
            continue;
        };
        let watched = central.base_dependabot().contains(ecosystem)
            || resolution.units.iter().any(|unit| {
                unit.path.contains(file) && unit.pipeline.dependabot.contains(ecosystem)
            });
        if !watched {
            findings.push(
                Finding::warning(
                    Rule::UnwatchedManifest,
                    format!(
                        "this is the manifest of the {ecosystem} ecosystem, and no unit \
                         containing it has it watched"
                    ),
                )
                .at(file.clone()),
            );
        }
    }

    for unit in &resolution.units {
        for ecosystem in &unit.pipeline.dependabot {
            let found = tree
                .files()
                .filter(|file| unit.path.contains(file))
                .any(|file| central.register().ecosystem_of(&file.to_string()) == Some(ecosystem));
            if !found && !central.base_dependabot().contains(ecosystem) {
                findings.push(
                    Finding::warning(
                        Rule::PointlessWatch,
                        format!("this unit has {ecosystem} watched and holds no manifest for it"),
                    )
                    .about(unit.name.clone())
                    .at(unit.path.clone()),
                );
            }
        }
    }
}

/// A version writer copies the unit's canonical version; a unit without one
/// leaves it nothing to copy. A unit that generates children is the exception,
/// as with publishing: the writer applies to each generated unit.
fn version_writers_have_a_version(resolution: &Resolution, findings: &mut Vec<Finding>) {
    for unit in &resolution.units {
        if unit.pipeline.version.is_empty()
            || unit.carries_version
            || unit.versioning.generates_children()
        {
            continue;
        }
        let writers = unit
            .pipeline
            .version
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        findings.push(
            Finding::error(
                Rule::VersionWriterWithoutVersion,
                format!(
                    "this unit selects the version writer {writers} and carries no version of \
                     its own; there is nothing to copy"
                ),
            )
            .about(unit.name.clone()),
        );
    }
}

fn publication_is_reachable(
    resolution: &Resolution,
    central: &Central,
    findings: &mut Vec<Finding>,
) {
    for unit in &resolution.units {
        let targets = unit.pipeline.targets();
        if targets.is_empty() {
            continue;
        }

        // A unit that carries no version but generates children is not
        // publishing anything itself: the publish flows to each generated
        // unit, which is how a folder of documents hangs each document on its
        // own release.
        if !unit.carries_version && !unit.versioning.generates_children() {
            findings.push(
                Finding::error(
                    Rule::PublishWithoutVersion,
                    "this unit publishes and carries no version of its own; without one there is \
                     no identity for anything to appear under",
                )
                .about(unit.name.clone()),
            );
        }

        let produced = unit
            .pipeline
            .build
            .as_ref()
            .and_then(|build| central.register().build.get(build))
            .map(|build| &build.produces);

        for name in targets {
            let Some(target) = central.register().target.get(name) else {
                continue;
            };
            match produced {
                None => findings.push(
                    Finding::error(
                        Rule::UnreachableTarget,
                        format!("this unit publishes to {name} and builds nothing to publish"),
                    )
                    .about(unit.name.clone()),
                ),
                Some(kind) if !target.accepts.contains(kind) => findings.push(
                    Finding::error(
                        Rule::UnreachableTarget,
                        format!("{name} does not take a {kind}, which is what this unit builds"),
                    )
                    .about(unit.name.clone()),
                ),
                Some(_) => {}
            }
            // Not `unit.pipeline.checks`: what counts is what is in force for
            // this unit. A package in a workspace publishes itself, but it is
            // checked by the workspace — the same "there exists" rule as with
            // coverage.
            if let Some(required) = &target.requires_check
                && !checks_in_force(resolution, central, unit).contains(required)
            {
                findings.push(
                    Finding::error(
                        Rule::TargetDemandsCheck,
                        format!(
                            "{name} only takes what the {required} check has been over, and this \
                             unit does not run it"
                        ),
                    )
                    .about(unit.name.clone()),
                );
            }
        }
    }
}

/// Everything the crate knows about one repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    /// The resolved units, repository first.
    pub units: Vec<Unit>,
    /// The annotated tree: every file with what holds for it, and the findings
    /// that hang at exactly that file.
    pub tree: Annotated,
    /// The findings that hang at no path in the tree: what is about the
    /// repository as a whole — it has no node of its own — and what names a
    /// path the repository does not have. A finding about a component hangs at
    /// that component's folder.
    pub loose: Vec<Finding>,
}

impl Analysis {
    /// Everything found: first what hangs at no path, then the nodes in tree
    /// order.
    #[must_use]
    pub fn findings(&self) -> Vec<&Finding> {
        let mut all: Vec<&Finding> = self.loose.iter().collect();
        for node in self.tree.walk() {
            all.extend(&node.findings);
        }
        all
    }

    /// Whether anything found stops the run.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.findings()
            .iter()
            .any(|finding| finding.severity == crate::finding::Severity::Error)
    }
}

/// Resolves a declaration and checks it in one step.
#[must_use]
pub fn analyse(declaration: &Declaration, central: &Central, tree: &Tree) -> Analysis {
    let resolution = crate::resolve::resolve(declaration, central.register(), tree);
    let mut annotated = annotate(declaration, &resolution, central, tree);
    let mut findings = resolution.findings.clone();
    findings.extend(findings_for(
        declaration,
        &resolution,
        central,
        tree,
        &annotated,
    ));
    let loose = annotated.attach(findings);
    Analysis {
        units: resolution.units,
        tree: annotated,
        loose,
    }
}

/// The units a name would have to be unique among, for a caller that wants to
/// place a new component without colliding.
#[must_use]
pub fn taken_names(resolution: &Resolution) -> BTreeSet<&crate::names::ComponentName> {
    resolution
        .units
        .iter()
        .filter_map(|unit| match &unit.name {
            UnitName::Component(name) => Some(name),
            UnitName::Repository => None,
        })
        .collect()
}
