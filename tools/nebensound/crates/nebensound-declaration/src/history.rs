//! Which version every unit carries at the commit at hand, from Git alone.
//!
//! Tags are the truth, and no version is stored in the tree. A unit's tags
//! start with its prefix ([`Unit::tag_prefix`]): `vX.Y.Z` for the repository,
//! `<component>-vX.Y.Z` for a component, `<component>-<entry>-vX.Y.Z` for a
//! generated unit, and `-pre.N` behind it for a pre-release.
//!
//! The version is a pure function of the history: every commit on `main`
//! raises on its own, so two runs on `main` at the same time come to different
//! numbers without waiting for each other.
//!
//! - The **base** of a unit is its highest full release on a released commit
//!   that `HEAD` contains; `0.0.0` without one. In one stage a released commit
//!   lies on the first-parent line of `HEAD`. In two stages it is a commit
//!   promoted into `release`, or a commit of that line older than the first
//!   promotion, so that the releases from before a switch keep counting. A
//!   pre-release counts, in either mode, only on a commit of that line.
//! - A commit **touches** a unit when it changes a file below the unit's path
//!   or the declaration itself. The repository is touched by every commit.
//!   Only the first-parent line of `HEAD` counts, and a merge commit touches
//!   nothing.
//! - **One stage**: a commit that touches the unit is the base raised once per
//!   touching commit since the base, each by its own change.
//! - **Two stages**: the target is the base raised once by the highest change
//!   among the touching commits since the base. A touching commit is
//!   `<target>-pre.N`, `N` its position among them; the commit promoted into
//!   `release` is `<target>`.
//! - `minimum-version`: the value in force is the highest one that a touching
//!   commit since the base names, the commit at hand included, so a lowered
//!   value has no effect and one a full release has reached does nothing. In
//!   one stage it lifts the step at which it comes into force, and the commits
//!   after it go on from there rather than all landing on it. In two stages it
//!   lifts the target to its `X.Y.Z`. A pre-release value `X.Y.Z-pre.N` also
//!   sets the number when the target is `X.Y.Z`: the commit at which it comes
//!   into force is at least `-pre.N`, and every commit after it one more than
//!   the one before. In one stage, which gives out no pre-release, such a value
//!   is ignored once the base has reached its `X.Y.Z` and is an error before.
//!
//! Reading Git is left to the caller, which hands in a [`History`]. Nothing
//! here runs a process, so every rule is checked on a history built by hand as
//! well as on a real repository.

use std::collections::{BTreeMap, BTreeSet};

use toml_edit::{Document, Item};

use crate::finding::{Finding, Rule, UnitName};
use crate::model::{Declaration, Release};
use crate::path::RelativePath;
use crate::resolve::{Origin, Unit};
use crate::version::{Change, TagVersion, Version, change_of};

/// Where the declaration lies. A commit that changes it touches every unit:
/// what the declaration says about a unit is part of the unit.
pub const DECLARATION: &str = ".nebensound/repo.toml";

/// One commit of the line `HEAD` stands on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Commit {
    /// Its full hash.
    pub id: String,
    /// The hashes of its parents, the first parent first.
    pub parents: Vec<String>,
    /// Its whole message.
    pub message: String,
    /// The paths it changes against its first parent, or against nothing for
    /// a commit without one. Removed and renamed files count by their old
    /// path as well.
    pub paths: Vec<String>,
    /// The object the declaration is held in at this commit, a key of
    /// [`History::declarations`]; [`None`] when the commit has none.
    pub declaration: Option<String>,
}

impl Commit {
    /// Whether it merges more than one parent.
    #[must_use]
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }

    /// Whether it touches a unit at this path.
    #[must_use]
    pub fn touches(&self, unit: &RelativePath) -> bool {
        !self.is_merge()
            && (unit.is_root()
                || self
                    .paths
                    .iter()
                    .any(|path| path == DECLARATION || below(unit, path)))
    }
}

/// Whether a path as Git writes it lies at or below a unit's path.
///
/// Compared by segment, as everywhere else: `crates/foo` holds
/// `crates/foo/src/lib.rs` and not `crates/foobar`. The path is taken as text
/// rather than as a [`RelativePath`]: a file the repository once held may have
/// had a name the declaration could not write down, and it still changed what
/// lies around it.
fn below(unit: &RelativePath, path: &str) -> bool {
    let mut parts = path.split('/');
    unit.segments()
        .iter()
        .all(|segment| parts.next() == Some(segment.as_str()))
}

/// A tag as Git names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// The name without `refs/tags/`.
    pub name: String,
    /// The commit it points at, through any annotated tag in between; [`None`]
    /// when it points at something else.
    pub commit: Option<String>,
}

/// What Git says about the commit at hand and the releases around it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct History {
    /// Whether the clone holds only part of the history.
    pub shallow: bool,
    /// The first-parent line of `HEAD`, `HEAD` first. Empty when the
    /// repository has no commit yet.
    pub line: Vec<Commit>,
    /// The parents of every commit that `HEAD` or `origin/release` contains,
    /// the first parent first.
    pub parents: BTreeMap<String, Vec<String>>,
    /// The commit `origin/release` points at, if the branch exists.
    pub release: Option<String>,
    /// Every tag of the repository.
    pub tags: Vec<Tag>,
    /// The declaration as each commit of the line holds it, by the object it
    /// is held in.
    pub declarations: BTreeMap<String, String>,
}

impl History {
    /// Every commit that this one contains, itself included.
    fn contained_in<'h>(&'h self, start: &'h str) -> BTreeSet<&'h str> {
        let mut seen = BTreeSet::new();
        let mut waiting = vec![start];
        while let Some(commit) = waiting.pop() {
            if seen.insert(commit) {
                waiting.extend(
                    self.parents
                        .get(commit)
                        .into_iter()
                        .flatten()
                        .map(String::as_str),
                );
            }
        }
        seen
    }

    /// The commits promoted into `release`, the newest first: the second
    /// parent of each merge commit on its first-parent line, where that parent
    /// is a commit of `main`, the first-parent line of `HEAD`. Nothing but such
    /// merges enters the branch, so these are the commits of `main` that
    /// became full releases.
    ///
    /// The first-parent line of `release` does not end where the branch was
    /// started: below that commit it goes on through `main`'s own history.
    /// A merge there brought a side branch into `main`, whose tip is no commit
    /// of `main`'s first-parent line, and promoted nothing.
    fn promotions(&self, main: &BTreeSet<&str>) -> Vec<&str> {
        let mut promoted = Vec::new();
        let mut next = self.release.as_deref();
        while let Some(commit) = next {
            let parents = self.parents.get(commit).map_or(&[][..], Vec::as_slice);
            if let [_, second, ..] = parents
                && main.contains(second.as_str())
            {
                promoted.push(second.as_str());
            }
            next = parents.first().map(String::as_str);
        }
        promoted
    }
}

/// The version of one unit at the commit at hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitVersion {
    /// Which unit this is.
    pub name: UnitName,
    /// Where it lives.
    pub path: RelativePath,
    /// What its tags start with.
    pub prefix: String,
    /// Whether it is released in one stage or in two.
    pub release: Release,
    /// Its last full release, `0.0.0` without one.
    pub base: Version,
    /// The version of the commit at hand; [`None`] when the commit does not
    /// touch the unit, or when a finding stands in the way.
    pub version: Option<TagVersion>,
}

/// The versions of a repository, and what is wrong with them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Versions {
    /// One entry per unit that carries a version, in the order given.
    pub units: Vec<UnitVersion>,
    /// What resolving them turned up.
    pub findings: Vec<Finding>,
}

/// What the history says once, for every unit.
struct Context<'h> {
    history: &'h History,
    /// The commit at hand.
    head: Option<&'h Commit>,
    /// What it contains.
    in_head: BTreeSet<&'h str>,
    /// The commits of its first-parent line.
    on_line: BTreeSet<&'h str>,
    /// The commits promoted into `release`.
    promoted: BTreeSet<&'h str>,
    /// What the first commit promoted into `release` contains, the commits
    /// older than the first promotion; [`None`] while nothing is promoted, when
    /// every commit is older than it.
    before_promotion: Option<BTreeSet<&'h str>>,
    /// The declarations the line holds, read; [`None`] for one that does not
    /// read, which names no floor.
    declarations: BTreeMap<&'h str, Option<Declaration>>,
}

impl Context<'_> {
    /// The minimum version a commit's own declaration names for a unit.
    fn floor(&self, commit: &Commit, unit: &Unit) -> Option<TagVersion> {
        let declaration = commit
            .declaration
            .as_deref()
            .and_then(|object| self.declarations.get(object))
            .and_then(Option::as_ref)?;
        match owner(unit) {
            UnitName::Repository => declaration.repo.minimum_version,
            UnitName::Component(name) => declaration
                .component
                .get(name)
                .and_then(|component| component.minimum_version),
        }
    }

    /// Whether a full release on this commit is one, for a unit released in
    /// this mode.
    fn released(&self, commit: &str, release: Release) -> bool {
        let on_line = self.on_line.contains(commit);
        match release {
            Release::SingleStage => on_line,
            Release::TwoStage => {
                self.promoted.contains(commit)
                    || (on_line
                        && self
                            .before_promotion
                            .as_ref()
                            .is_none_or(|older| older.contains(commit)))
            }
        }
    }
}

/// The unit whose declaration names a unit's floor: a generated unit takes its
/// parent's, as it does from the tree.
fn owner(unit: &Unit) -> &UnitName {
    match &unit.origin {
        Origin::Generated { by } => by,
        Origin::Declared => &unit.name,
    }
}

/// A tag that counts: a release of its unit.
struct Counted<'h> {
    version: TagVersion,
    commit: &'h str,
}

/// Resolves the version of every unit that carries one.
#[must_use]
pub fn versions(units: &[Unit], history: &History) -> Versions {
    let mut findings = Vec::new();
    if history.shallow {
        findings.push(Finding::error(
            Rule::ShallowClone,
            "this clone holds only part of the history, so no version can be told from it: a \
             version counts the commits since the last release, and that release may lie on a \
             commit that is not here. Check the repository out with its whole history and its \
             tags (`fetch-depth: 0`), or run `git fetch --unshallow --tags`",
        ));
        return Versions {
            units: Vec::new(),
            findings,
        };
    }

    let head = history.line.first();
    let on_line: BTreeSet<&str> = history
        .line
        .iter()
        .map(|commit| commit.id.as_str())
        .collect();
    let promotions = history.promotions(&on_line);
    let context = Context {
        history,
        head,
        in_head: head.map_or_else(BTreeSet::new, |head| history.contained_in(&head.id)),
        on_line,
        promoted: promotions.iter().copied().collect(),
        before_promotion: promotions.last().map(|first| history.contained_in(first)),
        declarations: history
            .declarations
            .iter()
            .map(|(object, text)| (object.as_str(), Declaration::parse(text).ok()))
            .collect(),
    };
    let counted = count_tags(units, &context, &mut findings);

    let mut warned = BTreeSet::new();
    let resolved = units
        .iter()
        .zip(&counted)
        .filter(|(unit, _)| unit.carries_version)
        .map(|(unit, tags)| resolve_unit(unit, tags, &context, &mut warned, &mut findings))
        .collect();

    let two_stage = units
        .iter()
        .any(|unit| unit.carries_version && unit.release.release == Release::TwoStage);
    if history.release.is_some() && !two_stage {
        findings.push(Finding::warning(
            Rule::PointlessReleaseBranch,
            "there is a branch origin/release, and no unit is released in two stages, so nothing \
             is ever promoted there. Delete the branch, or release a unit in two stages",
        ));
    }

    Versions {
        units: resolved,
        findings,
    }
}

/// The tags that count, by unit, in the order of `units`.
///
/// A tag belongs to the unit whose prefix it starts with and whose version
/// reads after it. A tag that reads as no unit's release belongs to the unit
/// with the longest prefix it starts with: `v1.0` is the repository's,
/// `tool-v1` the component `tool`'s.
///
/// A tag that does not count is the warning `unexpected-tag`: one that is no
/// version, one on something other than a commit, a full release on a commit
/// that was not released ([`Context::released`]), and a pre-release on a
/// commit off the first-parent line of `HEAD`. A tag on a commit
/// `HEAD` does not contain is passed over without one: it lies in history this
/// commit does not have, such as a newer release, and whether it counts is not
/// this commit's to judge.
fn count_tags<'h>(
    units: &[Unit],
    context: &Context<'h>,
    findings: &mut Vec<Finding>,
) -> Vec<Vec<Counted<'h>>> {
    let prefixes: Vec<String> = units.iter().map(Unit::tag_prefix).collect();
    let mut counted: Vec<Vec<Counted<'h>>> = units.iter().map(|_| Vec::new()).collect();
    for tag in &context.history.tags {
        let owner = prefixes
            .iter()
            .enumerate()
            .filter_map(|(index, prefix)| {
                let rest = tag.name.strip_prefix(prefix.as_str())?;
                Some((index, prefix, TagVersion::parse(rest)))
            })
            .max_by_key(|(_, prefix, version)| (version.is_some(), prefix.len()));
        let Some((index, prefix, version)) = owner else {
            continue;
        };
        let unit = &units[index];
        if !unit.carries_version {
            continue;
        }
        let unexpected = |why: String| {
            Finding::warning(
                Rule::UnexpectedTag,
                format!(
                    "the tag {:?} carries this unit's prefix and {why}, so it is no release of \
                     the unit and does not count",
                    tag.name
                ),
            )
            .about(unit.name.clone())
        };
        let Some(version) = version else {
            findings.push(unexpected(format!(
                "does not read {prefix}X.Y.Z or {prefix}X.Y.Z-pre.N"
            )));
            continue;
        };
        let Some(commit) = tag.commit.as_deref() else {
            findings.push(unexpected("points at no commit".to_owned()));
            continue;
        };
        if !context.in_head.contains(commit) {
            continue;
        }
        let release = unit.release.release;
        if version.is_pre() {
            // In either mode a pre-release is given to a commit of the
            // first-parent line of main, and only there.
            if !context.on_line.contains(commit) {
                findings.push(unexpected(
                    "is a pre-release on a commit off the first-parent line of HEAD, and only \
                     a commit of that line is ever given a pre-release"
                        .to_owned(),
                ));
                continue;
            }
        } else {
            // Without the branch release nothing can be told of a unit
            // released in two stages; the unit reports that itself.
            if release == Release::TwoStage && context.history.release.is_none() {
                continue;
            }
            if !context.released(commit, release) {
                findings.push(unexpected(
                    match release {
                        Release::SingleStage => {
                            "is a full release on a commit off the first-parent line of HEAD; \
                             the unit is released in one stage, where only a commit of that \
                             line is one"
                        }
                        Release::TwoStage => {
                            "is a full release on a commit that was never promoted into \
                             release; the unit is released in two stages, where only a \
                             promoted commit is one, or a commit of main older than the first \
                             promotion"
                        }
                    }
                    .to_owned(),
                ));
                continue;
            }
        }
        counted[index].push(Counted { version, commit });
    }
    counted
}

/// The version of one unit.
fn resolve_unit(
    unit: &Unit,
    counted: &[Counted<'_>],
    context: &Context<'_>,
    warned: &mut BTreeSet<String>,
    findings: &mut Vec<Finding>,
) -> UnitVersion {
    let mut entry = UnitVersion {
        name: unit.name.clone(),
        path: unit.path.clone(),
        prefix: unit.tag_prefix(),
        release: unit.release.release,
        base: Version::ZERO,
        version: None,
    };
    let two_stage = entry.release == Release::TwoStage;
    if two_stage && context.history.release.is_none() {
        findings.push(
            Finding::error(
                Rule::MissingReleaseBranch,
                "this unit is released in two stages, and there is no branch origin/release. Its \
                 full releases are the commits promoted there, so without the branch neither its \
                 last release nor its next version can be told. Create and fetch the branch, or \
                 release the unit in one stage",
            )
            .about(unit.name.clone()),
        );
        return entry;
    }

    let base = counted
        .iter()
        .filter(|tag| !tag.version.is_pre())
        .max_by_key(|tag| tag.version);
    if let Some(base) = base {
        entry.base = base.version.version;
    }
    if !two_stage
        && let Some(newest) = counted.iter().max_by_key(|tag| tag.version)
        && newest.version.is_pre()
    {
        findings.push(
            Finding::error(
                Rule::UnreleasedBeforeSwitch,
                format!(
                    "this unit is released in one stage, and its newest tag {}{} is a \
                     pre-release that never became a full release. Promote it into release \
                     before switching to one stage, so that the versions go on from a full \
                     release",
                    entry.prefix, newest.version
                ),
            )
            .about(unit.name.clone()),
        );
        return entry;
    }

    let Some(head) = context.head else {
        return entry;
    };
    // The touching commits since the base, oldest first: those the commit that
    // carries the base does not contain.
    let released = base.map(|base| context.history.contained_in(base.commit));
    let touching: Vec<&Commit> = context
        .history
        .line
        .iter()
        .rev()
        .filter(|commit| {
            commit.touches(&unit.path)
                && released
                    .as_ref()
                    .is_none_or(|released| !released.contains(commit.id.as_str()))
        })
        .collect();
    let step = Step {
        unit,
        context,
        base: entry.base,
        // A commit that carries the base is that release, and stays it.
        carries_base: base.is_some_and(|base| base.commit == head.id),
    };
    entry.version = if two_stage {
        let promoted = context.promoted.contains(head.id.as_str());
        step.two_stage(head, promoted, &touching, warned, findings)
    } else {
        step.single_stage(head, &touching, warned, findings)
    };
    entry
}

/// What raising one unit's version needs.
struct Step<'a> {
    unit: &'a Unit,
    context: &'a Context<'a>,
    base: Version,
    carries_base: bool,
}

impl Step<'_> {
    /// The value of `minimum-version` in force at each of these commits: the
    /// highest one named by it or a commit before it.
    fn floors(&self, commits: &[&Commit]) -> Vec<Option<TagVersion>> {
        commits
            .iter()
            .scan(None, |in_force, commit| {
                *in_force = (*in_force).max(self.context.floor(commit, self.unit));
                Some(*in_force)
            })
            .collect()
    }

    /// One stage: the base raised once per touching commit, each step lifted
    /// to the floor in force where it is lower.
    fn single_stage(
        &self,
        head: &Commit,
        touching: &[&Commit],
        warned: &mut BTreeSet<String>,
        findings: &mut Vec<Finding>,
    ) -> Option<TagVersion> {
        let mut commits = touching.to_vec();
        commits.push(head);
        let floors = self.floors(&commits);
        // One stage gives out no pre-release, so a floor that names one is
        // something no commit can reach, until a full release has passed it.
        if let Some(Some(floor)) = floors.last()
            && floor.is_pre()
            && floor.version > self.base
        {
            findings.push(self.pre_release_floor(*floor, head));
            return None;
        }
        if !head.touches(&self.unit.path) {
            return None;
        }
        // When the commit carries the base, nothing lies since it and the
        // version is the base as it was released.
        let mut version = self.base;
        for (commit, floor) in touching.iter().zip(floors) {
            let change = change(commit, warned, findings);
            let raised = self.raise(version, change, findings)?;
            version = match floor {
                Some(floor) if !floor.is_pre() => raised.max(floor.version),
                _ => raised,
            };
        }
        Some(TagVersion::stable(version))
    }

    /// The error for a unit released in one stage whose floor in force is a
    /// pre-release, at the line and column of the declaration at hand where
    /// the floor is written, or where the unit's table is when it is not.
    fn pre_release_floor(&self, floor: TagVersion, head: &Commit) -> Finding {
        let place = head
            .declaration
            .as_deref()
            .and_then(|object| self.context.history.declarations.get(object))
            .and_then(|text| locate(text, owner(self.unit)))
            .map_or_else(String::new, |(line, column)| {
                format!("{DECLARATION}, line {line}, column {column}: ")
            });
        Finding::error(
            Rule::PreReleaseMinimum,
            format!(
                "{place}the minimum version in force is {floor}, a pre-release, and this unit is \
                 released in one stage, which gives out no pre-release. Write \
                 minimum-version = \"{}\", or release the unit in two stages",
                floor.version
            ),
        )
        .about(self.unit.name.clone())
    }

    /// Two stages: the base raised once by the highest change since it, as a
    /// pre-release on a touching commit and in full on the promoted one.
    fn two_stage(
        &self,
        head: &Commit,
        promoted: bool,
        touching: &[&Commit],
        warned: &mut BTreeSet<String>,
        findings: &mut Vec<Finding>,
    ) -> Option<TagVersion> {
        let touches = head.touches(&self.unit.path);
        if !promoted && !touches {
            return None;
        }
        if self.carries_base {
            return Some(TagVersion::stable(self.base));
        }
        // Promoted with nothing since the base: there is nothing to release,
        // and the base belongs to another commit.
        if touching.is_empty() {
            return None;
        }
        let highest = touching
            .iter()
            .map(|commit| change(commit, warned, findings))
            .fold(Change::Patch, Change::max);
        let mut commits = touching.to_vec();
        commits.push(head);
        let floors = self.floors(&commits);
        // The highest floor since the base: a floor lowered within the cycle
        // must not take the target below a pre-release already given out.
        let in_force = floors.last().copied().flatten();
        let raised = self.raise(self.base, highest, findings)?;
        let target = in_force.map_or(raised, |floor| raised.max(floor.version));
        if promoted {
            return Some(TagVersion::stable(target));
        }
        // The commit at hand is the newest of the touching commits. Each one
        // is numbered one after the one before it, and a pre-release floor of
        // the target sets the number of the commit it comes into force at.
        let mut number: u64 = 0;
        for floor in &floors[..touching.len()] {
            let at_least = floor
                .filter(|floor| floor.version == target)
                .and_then(|floor| floor.pre)
                .unwrap_or(0);
            let Some(next) = number.checked_add(1) else {
                findings.push(self.exhausted(
                    &TagVersion::pre(target, number),
                    "the number of a pre-release already is the largest one a version can carry",
                ));
                return None;
            };
            number = next.max(at_least);
        }
        Some(TagVersion::pre(target, number))
    }

    /// A version raised once, or the finding that it cannot be raised.
    fn raise(
        &self,
        version: Version,
        change: Change,
        findings: &mut Vec<Finding>,
    ) -> Option<Version> {
        let raised = version.raised(change);
        if raised.is_none() {
            findings.push(self.exhausted(
                &version,
                &format!(
                    "the field a {change:?} change raises already holds the largest number a \
                     version can carry"
                ),
            ));
        }
        raised
    }

    /// The error that a version cannot be raised any further.
    fn exhausted(&self, version: &dyn std::fmt::Display, why: &str) -> Finding {
        Finding::error(
            Rule::ExhaustedVersion,
            format!("{version} cannot be raised any further: {why}"),
        )
        .about(self.unit.name.clone())
    }
}

/// Where a declaration writes the `minimum-version` of a unit, as a line and a
/// column counted from 1, or where it writes the unit's table when it names
/// none there. The column is that of the value, as a reading error gives it.
fn locate(text: &str, unit: &UnitName) -> Option<(usize, usize)> {
    // A step that finds nothing leaves the finding without a place; it does
    // not stand in the way of the finding itself.
    let document = Document::parse(text).ok();
    let table = document.as_ref().and_then(|document| match unit {
        UnitName::Repository => document.get("repo"),
        UnitName::Component(name) => document
            .get("component")
            .and_then(|components| components.get(name.as_str())),
    });
    table
        .and_then(|table| {
            table
                .get("minimum-version")
                .and_then(Item::span)
                .or_else(|| table.span())
        })
        .and_then(|span| text.get(..span.start))
        .map(|before| {
            let line = before.matches('\n').count() + 1;
            let column = before
                .rsplit('\n')
                .next()
                .unwrap_or_default()
                .chars()
                .count()
                + 1;
            (line, column)
        })
}

/// What a commit asks of the version. A message that is not a Conventional
/// Commit counts as a patch and is warned about once, however many units the
/// commit touches.
fn change(commit: &Commit, warned: &mut BTreeSet<String>, findings: &mut Vec<Finding>) -> Change {
    change_of(&commit.message).unwrap_or_else(|unconventional| {
        if warned.insert(commit.id.clone()) {
            let short: String = commit.id.chars().take(12).collect();
            findings.push(Finding::warning(
                Rule::UnconventionalCommit,
                format!("commit {short} counts as a patch: {unconventional}"),
            ));
        }
        Change::Patch
    })
}
