//! Which version every unit carries at the commit at hand, from Git alone.
//!
//! The history is the truth, and no version is stored in the tree. The version
//! of every unit at every commit follows from the commits of the line of
//! `main` and the declarations they hold, and from nothing else: no tag is
//! read to decide a version. Tags and releases are what the release stage
//! writes down, records, and phase 0 only holds them against the history
//! ([`Rule::ConflictingTag`]). A unit's records start with its prefix
//! ([`Unit::tag_prefix`]): `vX.Y.Z` for the repository, `<component>-vX.Y.Z`
//! for a component, `<component>-<entry>-vX.Y.Z` for a generated unit, and
//! `-pre.N` behind it for a pre-release.
//!
//! The version is a pure function of the history: every commit on `main`
//! raises on its own, so two runs on `main` at the same time come to different
//! numbers without waiting for each other.
//!
//! - The **line** of `main` is the first-parent line of `HEAD`
//!   ([`History::line`]).
//! - What a commit of the line says about a unit is read from the declaration
//!   that commit holds, resolved with the tree it holds: the unit's path and
//!   its minimum version there. A unit is recognised across commits by its
//!   name. Only a commit whose declaration names the unit and gives it a
//!   version **counts** for it, so the count starts at the first such commit,
//!   from `0.0.0`. A commit whose declaration does not read takes the path,
//!   and whether it counts, from the nearest commit after it on the line whose
//!   declaration reads.
//! - Every commit that counts for a unit and touches it, and that the history
//!   gives a version, is a full release of it. The **base** of a unit at a
//!   commit is the version of its newest such commit before it; `0.0.0`
//!   before the first.
//! - A commit **touches** a unit when it changes a file below the unit's path
//!   as that commit declares it, or the declaration itself. The repository is
//!   touched by every commit. Only the line counts. A merge commit of the
//!   line touches what it changes against its first parent, like any other
//!   commit, and the commits only its other parents reach are not on the line.
//! - The **change** of a commit follows from its message as a Conventional
//!   Commit, read with the commit types that count like `feat` in the
//!   declaration that commit holds (`feat-types`).
//! - A commit that touches the unit is the base raised once per touching
//!   commit since the base, each by its own change.
//! - `minimum-version`, three numbers: the value in force is the highest one
//!   that a touching commit since the base names, the commit at hand
//!   included, so a lowered value has no effect and one a full release has
//!   reached does nothing. It lifts the step at which it comes into force,
//!   and the commits after it go on from there rather than all landing on it.
//! - The **current** version of a unit is the highest version the history
//!   gives any commit of the line up to the commit at hand. It is what a unit
//!   the commit at hand does not touch is written as where a package that
//!   gets a version depends on it.
//!
//! The history is walked once per unit, oldest commit first: every commit that
//! counts is resolved as it resolves when it is the commit at hand, and a
//! released commit moves the base for the commits after it.
//!
//! Reading Git is left to the caller, which hands in a [`History`]. Nothing
//! here runs a process, so every rule is checked on a history built by hand as
//! well as on a real repository.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::finding::{Finding, Rule, UnitName};
use crate::model::Declaration;
use crate::path::RelativePath;
use crate::register::Register;
use crate::resolve::{Unit, resolve};
use crate::tree::Tree;
use crate::version::{Change, FeatTypes, TagVersion, Unconventional, Version, change_of};

/// Where the declaration lies. A commit that changes it touches every unit:
/// what the declaration says about a unit is part of the unit.
pub const DECLARATION: &str = ".nebensound/repo.toml";

/// One commit of the line of `main`.
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
    /// Those of [`Commit::paths`] it adds; for a commit without a parent,
    /// every file it holds.
    pub added: Vec<String>,
    /// Those of [`Commit::paths`] it removes.
    pub removed: Vec<String>,
    /// The object the declaration is held in at this commit, a key of
    /// [`History::declarations`]; [`None`] when the commit has none.
    pub declaration: Option<String>,
}

impl Commit {
    /// Whether it touches a unit at this path.
    ///
    /// A merge commit is no exception: its paths are those it changes against
    /// its first parent, so it touches what the branch it merges brings into
    /// the line, exactly as a squash commit of the same changes would.
    #[must_use]
    pub fn touches(&self, unit: &RelativePath) -> bool {
        unit.is_root()
            || self
                .paths
                .iter()
                .any(|path| path == DECLARATION || below(unit, path))
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

/// What Git says about the commit at hand and the history around it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct History {
    /// Whether the clone holds only part of the history.
    pub shallow: bool,
    /// The line of `main`, `HEAD` first: its first-parent line. Empty when
    /// the repository has no commit yet.
    pub line: Vec<Commit>,
    /// The parents of every commit that `HEAD` contains, the first parent
    /// first.
    pub parents: BTreeMap<String, Vec<String>>,
    /// Every tag of the repository: the records the history is held against,
    /// never a source of a version.
    pub tags: Vec<Tag>,
    /// The declaration as each commit of the line holds it, by the object it
    /// is held in.
    pub declarations: BTreeMap<String, String>,
    /// The files `HEAD` holds. With the files each commit of the line adds and
    /// removes, they tell what every commit of it held.
    pub files: Vec<String>,
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
}

/// The first twelve characters of a commit's hash, as a message names it.
fn short(commit: &str) -> String {
    commit.chars().take(12).collect()
}

/// The version of one unit at the commit at hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitVersion {
    /// Which unit this is.
    pub name: UnitName,
    /// Where it lives.
    pub path: RelativePath,
    /// What the names of its records, its tags and releases, start with.
    pub prefix: String,
    /// The highest version the history gives any commit of the line up to
    /// the commit at hand: [`UnitVersion::version`] when there is one.
    /// [`None`] before the first commit that touches the unit. A finding that
    /// stands in the way of the version of the commit at hand leaves what the
    /// commits before it were given. What a dependency the commit at hand
    /// does not touch is required at.
    pub current: Option<TagVersion>,
    /// The version of the commit at hand; [`None`] when the commit does not
    /// touch the unit, or when a finding stands in the way.
    pub version: Option<TagVersion>,
    /// Whether a tag records [`UnitVersion::current`]: one named with the
    /// prefix and that version. It decides no version; it tells whether what
    /// a package published to a registry requires of a unit the commit at
    /// hand does not touch was released ([`crate::requirements`]).
    pub recorded: bool,
}

/// The versions of a repository, and what is wrong with them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Versions {
    /// One entry per unit that carries a version, in the order given.
    pub units: Vec<UnitVersion>,
    /// What resolving them turned up.
    pub findings: Vec<Finding>,
}

/// What the declaration of one commit says about one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Declared {
    /// Where it lives there.
    path: RelativePath,
    /// The minimum version it has there.
    floor: Option<Version>,
    /// Whether it carries a version there.
    carries_version: bool,
}

/// The units the declaration of one commit resolves to, by name. Shared
/// between the commits that come to the same units.
type View = Rc<BTreeMap<UnitName, Declared>>;

/// What a commit of the line declares.
enum At {
    /// It holds no declaration.
    Absent,
    /// It holds one that does not read.
    Unreadable,
    /// It holds one that reads, and it resolves to these units.
    Read(View),
}

/// One declaration as it reads, whichever commits hold it.
enum Read {
    /// It generates no unit, so it resolves to the same units at every commit
    /// that holds it.
    Fixed(View),
    /// It generates units, so what it resolves to depends on the tree of the
    /// commit that holds it.
    Generating {
        declaration: Box<Declaration>,
        /// The paths of the units that generate children.
        generators: Vec<RelativePath>,
    },
}

impl Read {
    /// A declaration as it reads with the register.
    fn new(declaration: Declaration, register: &Register) -> Self {
        let declared = resolve(&declaration, register, &Tree::default()).units;
        let generators: Vec<RelativePath> = declared
            .iter()
            .filter(|unit| unit.versioning.generates_children())
            .map(|unit| unit.path.clone())
            .collect();
        if generators.is_empty() {
            Self::Fixed(view(declared))
        } else {
            Self::Generating {
                declaration: Box::new(declaration),
                generators,
            }
        }
    }
}

/// The entries directly below each generator that a file makes exist, each
/// with whether it is that file itself rather than a folder holding it. They
/// are all that a generator reads from a tree.
fn entries<'a>(
    file: &'a RelativePath,
    generators: &'a [RelativePath],
) -> impl Iterator<Item = (RelativePath, bool)> + 'a {
    generators.iter().filter_map(move |generator| {
        file.segments()
            .strip_prefix(generator.segments())?
            .split_first()
            .map(|(name, below)| (generator.join(name), below.is_empty()))
    })
}

/// The children of a generating declaration as the line is walked back: by
/// each entry, how many files make it exist. A commit of the line rarely adds
/// or removes a child, so the units a declaration resolves to are resolved
/// again only when it does, not at every commit.
struct Tracked<'h> {
    /// The object the declaration is held in.
    object: &'h str,
    generators: &'h [RelativePath],
    children: BTreeMap<(RelativePath, bool), usize>,
    /// What the declaration resolves to with these children; [`None`] once
    /// they have changed.
    view: Option<View>,
}

impl<'h> Tracked<'h> {
    /// The children these files make exist.
    fn new(
        object: &'h str,
        generators: &'h [RelativePath],
        files: &BTreeSet<RelativePath>,
    ) -> Self {
        let mut tracked = Self {
            object,
            generators,
            children: BTreeMap::new(),
            view: None,
        };
        for file in files {
            tracked.count(file, true);
        }
        tracked
    }

    /// Counts a file the tree gains, or one it loses.
    fn count(&mut self, file: &RelativePath, gained: bool) {
        for entry in entries(file, self.generators) {
            let before = self.children.get(&entry).copied().unwrap_or_default();
            let after = if gained {
                before.saturating_add(1)
            } else {
                before.saturating_sub(1)
            };
            if (before == 0) != (after == 0) {
                self.view = None;
            }
            if after == 0 {
                self.children.remove(&entry);
            } else {
                self.children.insert(entry, after);
            }
        }
    }
}

/// Every declaration the line holds, by the object it is held in: [`None`]
/// for one that does not read.
type Parsed<'h> = BTreeMap<&'h str, Option<Declaration>>;

/// Every declaration the line holds, read once.
fn parsed(history: &History) -> Parsed<'_> {
    history
        .declarations
        .iter()
        .map(|(object, text)| (object.as_str(), Declaration::parse(text).ok()))
        .collect()
}

/// What each commit of the line asks of a version, by its position: its
/// message read with the commit types that count like `feat` in the
/// declaration that commit holds, as every field that decides a version is.
///
/// A commit whose declaration does not read takes them from the nearest
/// commit after it on the line whose declaration reads, as it takes a unit's
/// path, and from the declaration at hand while none follows. A
/// commit without a declaration counts for no unit, so what it is read with
/// never decides a version.
fn changes(
    history: &History,
    parsed: &Parsed<'_>,
    at_hand: &FeatTypes,
) -> Vec<Result<Change, Unconventional>> {
    // The line runs from the commit at hand back, so the nearest readable
    // declaration after a commit is the last one met before it.
    let mut later = at_hand;
    history
        .line
        .iter()
        .map(|commit| {
            if let Some(Some(declaration)) = commit
                .declaration
                .as_deref()
                .and_then(|object| parsed.get(object))
            {
                later = &declaration.repo.feat_types;
            }
            change_of(&commit.message, later)
        })
        .collect()
}

/// What each commit of the line declares, in the order of the line.
///
/// Each declaration is resolved with the register and with the tree its
/// commit held, so that the units it generates are those of that commit. The
/// tree of a commit follows from the one after it on the line by taking back
/// what that one added and removed.
fn views(history: &History, parsed: Parsed<'_>, register: &Register) -> Vec<At> {
    let reads: BTreeMap<&str, Option<Read>> = parsed
        .into_iter()
        .map(|(object, declaration)| {
            let read = declaration.map(|declaration| Read::new(declaration, register));
            (object, read)
        })
        .collect();
    // A file whose name no declaration could write down generates nothing.
    let mut files: BTreeSet<RelativePath> = history
        .files
        .iter()
        .filter_map(|file| RelativePath::new(file).ok())
        .collect();
    let mut tracked: Option<Tracked> = None;
    let mut views = Vec::with_capacity(history.line.len());
    for commit in &history.line {
        let read = commit
            .declaration
            .as_deref()
            .map(|object| (object, reads.get(object).and_then(Option::as_ref)));
        views.push(match read {
            None => At::Absent,
            Some((_, None)) => At::Unreadable,
            Some((_, Some(Read::Fixed(view)))) => At::Read(Rc::clone(view)),
            Some((
                object,
                Some(Read::Generating {
                    declaration,
                    generators,
                }),
            )) => {
                if tracked
                    .as_ref()
                    .is_some_and(|tracked| tracked.object != object)
                {
                    tracked = None;
                }
                let tracked =
                    tracked.get_or_insert_with(|| Tracked::new(object, generators, &files));
                let view = tracked.view.get_or_insert_with(|| {
                    let below = files
                        .iter()
                        .filter(|file| generators.iter().any(|generator| generator.contains(file)));
                    view(resolve(declaration, register, &Tree::new(below.cloned())).units)
                });
                At::Read(Rc::clone(view))
            }
        });
        for file in commit
            .added
            .iter()
            .filter_map(|file| RelativePath::new(file).ok())
        {
            if files.remove(&file)
                && let Some(tracked) = &mut tracked
            {
                tracked.count(&file, false);
            }
        }
        for file in commit
            .removed
            .iter()
            .filter_map(|file| RelativePath::new(file).ok())
        {
            if let Some(tracked) = &mut tracked
                && !files.contains(&file)
            {
                tracked.count(&file, true);
            }
            files.insert(file);
        }
    }
    views
}

/// What the units of one declaration say, by name.
fn view(units: Vec<Unit>) -> View {
    Rc::new(
        units
            .into_iter()
            .map(|unit| {
                (
                    unit.name,
                    Declared {
                        path: unit.path,
                        floor: unit.minimum_version.map(|floor| floor.version),
                        carries_version: unit.carries_version,
                    },
                )
            })
            .collect(),
    )
}

/// What the history says once, for every unit.
struct Context<'h> {
    history: &'h History,
    /// The commit at hand.
    head: Option<&'h Commit>,
    /// What it contains.
    in_head: BTreeSet<&'h str>,
    /// What each commit of the line declares, by its position.
    views: Vec<At>,
    /// What each commit of the line asks of a version, by its position.
    changes: Vec<Result<Change, Unconventional>>,
}

/// Where a unit lived at a commit that counts for it.
#[derive(Debug, Clone, Copy)]
struct Place<'a> {
    path: &'a RelativePath,
    /// The minimum version it had there.
    floor: Option<Version>,
}

impl Context<'_> {
    /// How each commit of the line counts for a unit, by its position:
    /// [`None`] for one that does not count.
    ///
    /// A commit counts when its declaration names the unit and gives it a
    /// version, with the path and the floor it declares, so a unit's count
    /// starts at the first such commit. A commit without a declaration, or
    /// whose declaration names the unit without a version or not at all, does
    /// not count.
    ///
    /// A commit whose declaration does not read takes the path, and whether
    /// it counts at all, from the nearest commit after it on the line whose
    /// declaration reads, and has no floor. What it counts for is thereby
    /// fixed once such a commit exists: a later move of the unit does not
    /// count it again in another way, so no version is given twice. Where
    /// none follows, the declaration at hand stands in for it.
    fn places<'a>(&'a self, unit: &'a Unit) -> Vec<Option<Place<'a>>> {
        let declared = |view: &'a View| {
            view.get(&unit.name)
                .filter(|declared| declared.carries_version)
        };
        // The line runs from the commit at hand back, so the nearest readable
        // declaration after a commit is the last one met before it.
        let mut later: Option<&'a View> = None;
        self.views
            .iter()
            .map(|at| match at {
                At::Absent => None,
                At::Unreadable => match later {
                    Some(view) => declared(view).map(|declared| Place {
                        path: &declared.path,
                        floor: None,
                    }),
                    None => Some(Place {
                        path: &unit.path,
                        floor: None,
                    }),
                },
                At::Read(view) => {
                    later = Some(view);
                    declared(view).map(|declared| Place {
                        path: &declared.path,
                        floor: declared.floor,
                    })
                }
            })
            .collect()
    }

    /// What the commit at this position asks of a version. A message that is
    /// not a Conventional Commit counts as a patch and is warned about once,
    /// however many units the commit touches.
    fn change(
        &self,
        position: usize,
        warned: &mut BTreeSet<String>,
        findings: &mut Vec<Finding>,
    ) -> Change {
        match &self.changes[position] {
            Ok(change) => *change,
            Err(unconventional) => {
                let commit = &self.history.line[position];
                if warned.insert(commit.id.clone()) {
                    findings.push(Finding::warning(
                        Rule::UnconventionalCommit,
                        format!(
                            "commit {} counts as a patch: {unconventional}",
                            short(&commit.id)
                        ),
                    ));
                }
                Change::Patch
            }
        }
    }
}

/// Resolves the version of every unit that carries one. The register resolves
/// the declaration each commit of the line held. `feat_types` are those of the
/// declaration at hand, which stands in for a commit whose declaration does
/// not read while no readable one follows it.
#[must_use]
pub fn versions(
    units: &[Unit],
    feat_types: &FeatTypes,
    history: &History,
    register: &Register,
) -> Versions {
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
    let parsed = parsed(history);
    let changes = changes(history, &parsed, feat_types);
    let context = Context {
        history,
        head,
        in_head: head.map_or_else(BTreeSet::new, |head| history.contained_in(&head.id)),
        views: views(history, parsed, register),
        changes,
    };

    let mut warned = BTreeSet::new();
    let resolved: Vec<(usize, Resolved)> = units
        .iter()
        .enumerate()
        .filter(|(_, unit)| unit.carries_version)
        .map(|(index, unit)| {
            (
                index,
                resolve_unit(unit, &context, &mut warned, &mut findings),
            )
        })
        .collect();
    hold_records(units, &resolved, &context, &mut findings);

    Versions {
        units: resolved.into_iter().map(|(_, unit)| unit.entry).collect(),
        findings,
    }
}

/// One unit as the history resolves it.
struct Resolved<'h> {
    entry: UnitVersion,
    /// Every version the history gives a commit of the line up to the commit
    /// at hand, with that commit.
    given: BTreeMap<TagVersion, &'h str>,
}

/// The commits since a unit's last full release.
struct Cycle<'h> {
    /// That release, `0.0.0` before the first.
    base: Version,
    /// The commits since it that touch the unit, oldest first, each with its
    /// position on the line.
    since: Vec<(usize, &'h Commit)>,
}

/// The version of one unit: the history walked from its oldest commit to the
/// one before the commit at hand, quietly, and then the commit at hand.
fn resolve_unit<'h>(
    unit: &Unit,
    context: &Context<'h>,
    warned: &mut BTreeSet<String>,
    findings: &mut Vec<Finding>,
) -> Resolved<'h> {
    let mut entry = UnitVersion {
        name: unit.name.clone(),
        path: unit.path.clone(),
        prefix: unit.tag_prefix(),
        current: None,
        version: None,
        recorded: false,
    };
    let places = context.places(unit);
    let mut cycle = Cycle {
        base: Version::ZERO,
        since: Vec::new(),
    };
    let mut given = BTreeMap::new();
    for position in (1..context.history.line.len()).rev() {
        pass(unit, context, &places, position, &mut cycle, &mut given);
    }
    let Some(head) = context.head else {
        return Resolved { entry, given };
    };

    let step = Step {
        unit,
        context,
        places: &places,
        base: cycle.base,
    };
    if step.touches(0) {
        cycle.since.push((0, head));
    }
    entry.version = step.release(0, &cycle.since, warned, findings);
    if let Some(version) = entry.version {
        given.insert(version, head.id.as_str());
    }
    entry.current = given.keys().next_back().copied();
    entry.recorded = entry.current.is_some_and(|current| {
        let record = format!("{}{current}", entry.prefix);
        context.history.tags.iter().any(|tag| tag.name == record)
    });
    Resolved { entry, given }
}

/// One commit before the commit at hand, resolved as it resolves when it is
/// the commit at hand. What it turns up belongs to that commit, not to the
/// commit at hand, so it is not reported. A released commit moves the base.
fn pass<'h>(
    unit: &Unit,
    context: &Context<'h>,
    places: &[Option<Place<'_>>],
    position: usize,
    cycle: &mut Cycle<'h>,
    given: &mut BTreeMap<TagVersion, &'h str>,
) {
    let Some(place) = places[position] else {
        return;
    };
    let commit = &context.history.line[position];
    if !commit.touches(place.path) {
        return;
    }
    cycle.since.push((position, commit));
    let step = Step {
        unit,
        context,
        places,
        base: cycle.base,
    };
    let (warned, findings) = (&mut BTreeSet::new(), &mut Vec::new());
    if let Some(version) = step.release(position, &cycle.since, warned, findings) {
        given.insert(version, commit.id.as_str());
        cycle.base = version.version;
        cycle.since.clear();
    }
}

/// What raising one unit's version needs.
struct Step<'a> {
    unit: &'a Unit,
    context: &'a Context<'a>,
    /// How each commit of the line counts for the unit.
    places: &'a [Option<Place<'a>>],
    /// Its last full release before the commit resolved.
    base: Version,
}

impl Step<'_> {
    /// Whether the commit at this position counts for the unit and touches
    /// it.
    fn touches(&self, position: usize) -> bool {
        self.places[position]
            .is_some_and(|place| self.context.history.line[position].touches(place.path))
    }

    /// The version of the commit at this position: the base raised once per
    /// touching commit since it, each step lifted to the floor in force where
    /// it is lower. [`None`] when the commit is not the newest of the touching
    /// commits, which is when it does not touch the unit.
    fn release(
        &self,
        at: usize,
        touching: &[(usize, &Commit)],
        warned: &mut BTreeSet<String>,
        findings: &mut Vec<Finding>,
    ) -> Option<TagVersion> {
        if touching.last().is_none_or(|(position, _)| *position != at) {
            return None;
        }
        // The value of `minimum-version` in force at each commit: the highest
        // one named by it or a commit before it since the base.
        touching
            .iter()
            .scan(None, |in_force, (position, _)| {
                *in_force = (*in_force).max(self.places[*position].and_then(|place| place.floor));
                Some((*position, *in_force))
            })
            .try_fold(self.base, |version, (position, floor)| {
                let change = self.context.change(position, warned, findings);
                let raised = self.raise(version, change, findings)?;
                Some(floor.map_or(raised, |floor| raised.max(floor)))
            })
            .map(TagVersion::stable)
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
            findings.push(
                Finding::error(
                    Rule::ExhaustedVersion,
                    format!(
                        "{version} cannot be raised any further: the field a {change:?} change \
                         raises already holds the largest number a version can carry"
                    ),
                )
                .about(self.unit.name.clone()),
            );
        }
        raised
    }
}

/// Holds the records against the history: every tag that reads as a release
/// of a unit and sits on a commit the commit at hand contains.
///
/// A tag belongs to the unit whose prefix it starts with and whose version
/// reads after it, `<prefix>X.Y.Z` or `<prefix>X.Y.Z-pre.N`; no two units
/// share a prefix, and a version holds no `v`, so at most one unit reads it.
/// A tag that reads as no unit's release, one on something other than a
/// commit, and one on a commit the commit at hand does not contain are passed
/// over: whatever they record is not this commit's to judge.
///
/// A tag on exactly the commit to which the history gives its version is a
/// faithful record and passes. Any other such tag whose version is not lower
/// than the unit's current version is the error `conflicting-tag`: a release
/// of the unit could meet it, and a tag never moves a version. A tag below
/// stays unnoticed, so the tags of a repository from before it called the
/// pipeline do no harm below its `minimum-version`.
fn hold_records(
    units: &[Unit],
    resolved: &[(usize, Resolved<'_>)],
    context: &Context<'_>,
    findings: &mut Vec<Finding>,
) {
    let prefixes: Vec<String> = units.iter().map(Unit::tag_prefix).collect();
    for tag in &context.history.tags {
        let Some((index, version)) = prefixes.iter().enumerate().find_map(|(index, prefix)| {
            let rest = tag.name.strip_prefix(prefix.as_str())?;
            TagVersion::parse(rest).map(|version| (index, version))
        }) else {
            continue;
        };
        let Some((entry, given)) = resolved
            .iter()
            .find(|(unit, _)| *unit == index)
            .map(|(_, resolved)| (&resolved.entry, &resolved.given))
        else {
            continue;
        };
        let Some(commit) = tag.commit.as_deref() else {
            continue;
        };
        if !context.in_head.contains(commit) || given.get(&version) == Some(&commit) {
            continue;
        }
        if entry.current.is_some_and(|current| version < current) {
            continue;
        }
        let holder = given.get(&version).map_or_else(
            || "to no commit".to_owned(),
            |other| format!("to the commit {}", short(other)),
        );
        let current = entry
            .current
            .map_or_else(|| "none yet".to_owned(), |current| current.to_string());
        findings.push(
            Finding::error(
                Rule::ConflictingTag,
                format!(
                    "the tag {:?} reads as the release {version} of this unit and sits on the \
                     commit {}, and the history gives {version} {holder}. A tag is a record and \
                     never moves a version, and this one is not lower than the unit's current \
                     version ({current}), so a release of the unit could meet it. Delete the \
                     tag if nobody published it, or {}",
                    tag.name,
                    short(commit),
                    floor_past(version)
                ),
            )
            .about(entry.name.clone()),
        );
    }
}

/// The way past a conflicting tag through `minimum-version`, which names three
/// numbers. Past a pre-release tag its three numbers are the lowest value that
/// works, past a full one only three numbers above it do.
fn floor_past(tag: TagVersion) -> String {
    if tag.is_pre() {
        format!("raise minimum-version to {} or higher", tag.version)
    } else {
        format!("raise minimum-version above {tag}")
    }
}
