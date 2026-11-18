//! The result of phase 0: the tree, with what holds written at its nodes.
//!
//! A flat list of findings says that a file is not covered, but not which units
//! contain it and why none of them covers it. At the node both stand — and with
//! that, "what is not covered" becomes a filter over the tree rather than a data
//! path of its own, "why is this covered" becomes a query, and "give me
//! everything" becomes the log for phase 0.
//!
//! Input and output stay outside: the tree is a value, not an expression.

use std::collections::{BTreeMap, BTreeSet};

use crate::central::{Central, Ruling};
use crate::finding::{Finding, UnitName};
use crate::model::Declaration;
use crate::names::{CheckName, Reason};
use crate::path::RelativePath;
use crate::pattern::Pattern;
use crate::resolve::Resolution;
use crate::tree::Tree;

/// What holds for a file: whether it is checked, and by what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Coverage {
    /// This check covers it, and a unit containing it runs that check.
    Covered(CheckName),
    /// This check would cover it, and no unit containing it runs that check.
    Uncovered(CheckName),
    /// The register knows no check for this kind of file.
    Unknown,
    /// An entry takes it out, and this is its reason.
    Excluded {
        /// The entry that takes it out.
        pattern: Pattern,
        /// Why.
        reason: Reason,
    },
}

impl Coverage {
    /// Whether the question is settled: checked, or excused with a reason.
    ///
    /// The opposite is the gap — the filter that makes phase 0 fail.
    #[must_use]
    pub fn settled(&self) -> bool {
        matches!(self, Self::Covered(_) | Self::Excluded { .. })
    }
}

/// Whether a node is a file or a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A file, with what holds for it.
    File(Coverage),
    /// A folder, with what lies below it.
    ///
    /// A folder carries no coverage: the promise a check makes is about files.
    Folder(Vec<Node>),
}

/// A path in the tree, with everything that holds for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Where it lies.
    pub path: RelativePath,
    /// The units that contain it — the outermost first.
    pub units: Vec<UnitName>,
    /// The checks in force here: the base set and those of every unit
    /// containing it.
    pub checks: Vec<CheckName>,
    /// The findings that hang at exactly this path.
    pub findings: Vec<Finding>,
    /// File or folder.
    pub kind: Kind,
}

impl Node {
    /// What holds for the file, or [`None`] for a folder.
    #[must_use]
    pub fn coverage(&self) -> Option<&Coverage> {
        match &self.kind {
            Kind::File(coverage) => Some(coverage),
            Kind::Folder(_) => None,
        }
    }

    /// What lies directly below. Nothing, for a file.
    #[must_use]
    pub fn children(&self) -> &[Node] {
        match &self.kind {
            Kind::File(_) => &[],
            Kind::Folder(children) => children,
        }
    }
}

/// The annotated tree.
///
/// It holds the topmost level, not the repository itself: a request with
/// exactly one file answers with exactly one node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotated {
    nodes: Vec<Node>,
}

impl Annotated {
    /// The topmost level, in path order.
    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Every node, a folder before what lies below it.
    #[must_use]
    pub fn walk(&self) -> Vec<&Node> {
        let mut all = Vec::new();
        gather(&self.nodes, &mut all);
        all
    }

    /// Every file, with what holds for it.
    #[must_use]
    pub fn files(&self) -> Vec<(&Node, &Coverage)> {
        self.walk()
            .into_iter()
            .filter_map(|node| match &node.kind {
                Kind::File(coverage) => Some((node, coverage)),
                Kind::Folder(_) => None,
            })
            .collect()
    }

    /// The files whose coverage is not settled — the gap.
    ///
    /// A filter over the tree, not a data path of its own.
    #[must_use]
    pub fn gaps(&self) -> Vec<(&Node, &Coverage)> {
        self.files()
            .into_iter()
            .filter(|(_, coverage)| !coverage.settled())
            .collect()
    }

    /// The node at exactly this path, if there is one.
    #[must_use]
    pub fn at(&self, path: &RelativePath) -> Option<&Node> {
        find(&self.nodes, path)
    }

    /// Hangs the findings on the nodes they belong to, beside what hangs there
    /// already, and returns what hangs on none: what is about the repository as a whole — it has no node of its
    /// own — and what names a path the repository does not have.
    ///
    /// One pass over the tree, not one search per finding: coverage can put a
    /// finding on every file, and searching the tree again for each of them
    /// would cost the tree twice over.
    #[must_use]
    pub fn attach(&mut self, findings: Vec<Finding>) -> Vec<Finding> {
        let mut waiting: BTreeMap<RelativePath, Vec<Finding>> = BTreeMap::new();
        let mut loose = Vec::new();
        for finding in findings {
            match &finding.path {
                Some(path) => waiting.entry(path.clone()).or_default().push(finding),
                None => loose.push(finding),
            }
        }

        hang(&mut self.nodes, &mut waiting);

        // What nobody picked up names a path the tree does not hold.
        loose.extend(waiting.into_values().flatten());
        loose
    }
}

fn hang(nodes: &mut [Node], waiting: &mut BTreeMap<RelativePath, Vec<Finding>>) {
    for node in nodes {
        if let Some(found) = waiting.remove(&node.path) {
            node.findings.extend(found);
        }
        if let Kind::Folder(children) = &mut node.kind {
            hang(children, waiting);
        }
    }
}

fn gather<'a>(nodes: &'a [Node], all: &mut Vec<&'a Node>) {
    for node in nodes {
        all.push(node);
        gather(node.children(), all);
    }
}

fn find<'a>(nodes: &'a [Node], path: &RelativePath) -> Option<&'a Node> {
    nodes.iter().find_map(|node| {
        if node.path == *path {
            return Some(node);
        }
        match &node.kind {
            Kind::Folder(children) if node.path.contains(path) => find(children, path),
            _ => None,
        }
    })
}

/// Builds the annotated tree, still without findings.
///
/// The findings arrive with [`Annotated::attach`], because they only come into
/// being once the tree stands.
#[must_use]
pub fn annotate(
    declaration: &Declaration,
    resolution: &Resolution,
    central: &Central,
    tree: &Tree,
) -> Annotated {
    // The files arrive sorted, so everything below one folder stands together.
    // Asking the tree for each folder's children instead would walk the whole
    // file set once per folder.
    let files: Vec<&RelativePath> = tree.files().collect();
    Annotated {
        nodes: below(
            &RelativePath::root(),
            &files,
            declaration,
            resolution,
            central,
        ),
    }
}

fn below(
    prefix: &RelativePath,
    files: &[&RelativePath],
    declaration: &Declaration,
    resolution: &Resolution,
    central: &Central,
) -> Vec<Node> {
    let depth = prefix.segments().len();
    let mut nodes = Vec::new();
    let mut rest = files;

    while let Some(first) = rest.first() {
        let name = first.segments()[depth].clone();
        let run = rest.partition_point(|file| file.segments()[depth] == name);
        let (group, remainder) = rest.split_at(run);
        rest = remainder;

        let path = prefix.join(&name);
        // A file and a folder of the same name cannot both exist in Git. The
        // split is here so that a hand-built tree holding both is answered
        // rather than indexed past its end.
        let (here, deeper): (Vec<_>, Vec<_>) = group
            .iter()
            .partition(|file| file.segments().len() == depth + 1);

        for file in here {
            let checks = checks_at(file, resolution, central);
            let coverage = coverage_of(file, &checks, declaration, central);
            nodes.push(Node {
                path: (*file).clone(),
                units: units_at(file, resolution),
                checks: checks.into_iter().cloned().collect(),
                findings: Vec::new(),
                kind: Kind::File(coverage),
            });
        }
        if !deeper.is_empty() {
            let children = below(&path, &deeper, declaration, resolution, central);
            nodes.push(Node {
                units: units_at(&path, resolution),
                checks: checks_at(&path, resolution, central)
                    .into_iter()
                    .cloned()
                    .collect(),
                findings: Vec::new(),
                kind: Kind::Folder(children),
                path,
            });
        }
    }
    nodes
}

fn units_at(path: &RelativePath, resolution: &Resolution) -> Vec<UnitName> {
    let mut containing: Vec<_> = resolution
        .units
        .iter()
        .filter(|unit| unit.path.contains(path))
        .collect();
    // Outermost first: a unit that contains another stands on a shorter path.
    // The resolution lists components by name, so without this a nested unit
    // whose name sorts first would stand before the one containing it. Equal
    // depth keeps the order the resolution has.
    containing.sort_by_key(|unit| unit.path.segments().len());
    containing
        .into_iter()
        .map(|unit| unit.name.clone())
        .collect()
}

/// The checks in force at a path: the base set, which needs declaring nowhere,
/// and those of every unit containing it.
fn checks_at<'a>(
    path: &RelativePath,
    resolution: &'a Resolution,
    central: &'a Central,
) -> BTreeSet<&'a CheckName> {
    let mut checks: BTreeSet<&CheckName> = central.base_checks().iter().collect();
    for unit in &resolution.units {
        if unit.path.contains(path) {
            checks.extend(unit.pipeline.checks.iter());
        }
    }
    checks
}

fn coverage_of(
    file: &RelativePath,
    checks: &BTreeSet<&CheckName>,
    declaration: &Declaration,
    central: &Central,
) -> Coverage {
    if let Ruling::Excluded { pattern, reason } = central.ruling(declaration, file) {
        return Coverage::Excluded {
            pattern: pattern.clone(),
            reason: reason.clone(),
        };
    }
    match central.register().check_for(&file.to_string()) {
        None => Coverage::Unknown,
        Some(check) if checks.contains(check) => Coverage::Covered(check.clone()),
        Some(check) => Coverage::Uncovered(check.clone()),
    }
}
