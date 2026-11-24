//! What the crate has to say about a declaration.
//!
//! Every finding carries a stable code. The wording may improve; the code is
//! what a workflow, a suppression or a piece of documentation may refer to.

use std::fmt;

use crate::names::ComponentName;
use crate::path::RelativePath;

/// Whether a finding stops the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The declaration is wrong and phase 0 fails.
    Error,
    /// The declaration is usable but says something that is not true.
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Error => "error",
            Self::Warning => "warning",
        })
    }
}

/// Which unit a finding is about.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnitName {
    /// The repository itself.
    Repository,
    /// A component, declared or generated.
    Component(ComponentName),
}

impl fmt::Display for UnitName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository => f.write_str("the repository"),
            Self::Component(name) => write!(f, "component {name}"),
        }
    }
}

/// The rule a finding belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Rule {
    /// A pipeline named a preset the register does not define.
    UnknownPreset,
    /// A pipeline named a check, build or target the register does not define.
    UnknownRegisterEntry,
    /// `pipeline.repo = true` without anything in `[repo.component]`.
    NoRequestableDefault,
    /// The repository used a versioning value that would leave it without one.
    RepositoryWithoutVersion,
    /// A component path does not exist in the tree.
    MissingPath,
    /// A tracked file's name, or a path in the declaration, that holds a
    /// control character.
    ControlCharacter,
    /// A file whose language the register offers no check for at all.
    LanguageWithoutCheck,
    /// A unit declares children it does not have.
    ChildlessGenerator,
    /// A generated unit could not be named, or was named twice.
    UngeneratableName,
    /// A file no check covers, and no exclusion excuses.
    UncoveredFile,
    /// A check that found nothing to check.
    UnusedCheck,
    /// A silenced check that is not declared anywhere.
    PointlessSuppression,
    /// An exclusion for a path the repository does not hold.
    PointlessExclusion,
    /// An included extension that the register already checks.
    PointlessInclusion,
    /// A publication target the build cannot feed.
    UnreachableTarget,
    /// A target that demands a check the unit does not run.
    TargetDemandsCheck,
    /// Publishing from a unit that carries no version.
    PublishWithoutVersion,
    /// A version writer on a unit that carries no version to copy.
    VersionWriterWithoutVersion,
    /// A `minimum-version` on a unit that carries no version for it to act
    /// on.
    ReleaseFieldWithoutVersion,
    /// A build without a version writer it needs, so that its artifact would
    /// carry a version the run did not write.
    BuildWithoutVersionWriter,
    /// A build without a file it reads in the unit, so that its build job would
    /// fail.
    BuildWithoutFile,
    /// A manifest whose ecosystem no unit has watched.
    UnwatchedManifest,
    /// A watched ecosystem whose manifest does not exist.
    PointlessWatch,
    /// A manifest whose lockfile, as the register names it, Git does not
    /// track.
    MissingLockfile,
    /// A commit message that is not a Conventional Commit.
    UnconventionalCommit,
    /// A clone without the whole history, so no version can be told.
    ShallowClone,
    /// A version whose field to raise holds the largest number already.
    ExhaustedVersion,
    /// A tag that reads as a release of a unit, sits elsewhere than on the
    /// commit the history gives its version, and is not below the unit's
    /// current version.
    ConflictingTag,
    /// A package a pull request would publish to a registry requires by path
    /// a unit the pull request does not touch, and no tag records that unit's
    /// current version.
    UnreleasedRequirement,
    /// A file a tool's own setting takes out, and no exclusion does.
    UnexcusedIgnore,
    /// A tool's setting that takes files out, and that phase 0 cannot read.
    UnreadableIgnoreFile,
    /// An exclusion that takes code out of a Rust package, and not the whole
    /// package.
    PartialPackageExclusion,
    /// A declaration at the commit at hand that does not say for whom the
    /// repository is meant.
    MissingAudience,
    /// A declared audience that does not fit what the run knows of the
    /// repository: `public` exactly when the repository is public.
    MismatchedAudience,
    /// A repository whose name is not lowercase with hyphens.
    NonconformingName,
    /// A repository whose default branch is not `main`.
    NonconformingDefaultBranch,
    /// A file every repository holds, and this one does not.
    MissingMinimumFile,
    /// A Dependabot configuration that is missing, cannot be read, or departs
    /// from what the declaration and the policy give.
    StaleDependabot,
}

impl Rule {
    /// The stable code for this rule.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::UnknownPreset => "unknown-preset",
            Self::UnknownRegisterEntry => "unknown-register-entry",
            Self::NoRequestableDefault => "no-requestable-default",
            Self::RepositoryWithoutVersion => "repository-without-version",
            Self::MissingPath => "missing-path",
            Self::ControlCharacter => "control-character",
            Self::LanguageWithoutCheck => "language-without-check",
            Self::ChildlessGenerator => "childless-generator",
            Self::UngeneratableName => "ungeneratable-name",
            Self::UncoveredFile => "uncovered-file",
            Self::UnusedCheck => "unused-check",
            Self::PointlessSuppression => "pointless-suppression",
            Self::PointlessExclusion => "pointless-exclusion",
            Self::PointlessInclusion => "pointless-inclusion",
            Self::UnreachableTarget => "unreachable-target",
            Self::TargetDemandsCheck => "target-demands-check",
            Self::PublishWithoutVersion => "publish-without-version",
            Self::VersionWriterWithoutVersion => "version-writer-without-version",
            Self::ReleaseFieldWithoutVersion => "release-field-without-version",
            Self::BuildWithoutVersionWriter => "build-without-version-writer",
            Self::BuildWithoutFile => "build-without-file",
            Self::UnwatchedManifest => "unwatched-manifest",
            Self::PointlessWatch => "pointless-watch",
            Self::MissingLockfile => "missing-lockfile",
            Self::UnconventionalCommit => "unconventional-commit",
            Self::ShallowClone => "shallow-clone",
            Self::ExhaustedVersion => "exhausted-version",
            Self::ConflictingTag => "conflicting-tag",
            Self::UnreleasedRequirement => "unreleased-requirement",
            Self::UnexcusedIgnore => "unexcused-ignore",
            Self::UnreadableIgnoreFile => "unreadable-ignore-file",
            Self::PartialPackageExclusion => "partial-package-exclusion",
            Self::MissingAudience => "missing-audience",
            Self::MismatchedAudience => "mismatched-audience",
            Self::NonconformingName => "nonconforming-name",
            Self::NonconformingDefaultBranch => "nonconforming-default-branch",
            Self::MissingMinimumFile => "missing-minimum-file",
            Self::StaleDependabot => "stale-dependabot",
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Where in its file a finding points, both counted from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    /// The line.
    pub line: usize,
    /// The column, in characters.
    pub column: usize,
}

impl Position {
    /// The line and column of a byte offset in a text, the column counted in
    /// characters. An offset past the end or inside a character points at the
    /// start of the text.
    #[must_use]
    pub fn at(source: &str, offset: usize) -> Self {
        let before = source.get(..offset).unwrap_or_default();
        let line = before.matches('\n').count() + 1;
        let column = before
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .chars()
            .count()
            + 1;
        Self { line, column }
    }
}

/// One thing the crate has to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Whether it stops the run.
    pub severity: Severity,
    /// Which rule it belongs to.
    pub rule: Rule,
    /// Which unit it is about, when it is about one.
    pub unit: Option<UnitName>,
    /// Which path it is about, when it is about one.
    pub path: Option<RelativePath>,
    /// Where in that file, when it points at a place in it.
    pub position: Option<Position>,
    /// What is wrong, in a sentence.
    pub message: String,
}

impl Finding {
    /// A finding that stops the run.
    #[must_use]
    pub fn error(rule: Rule, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            rule,
            unit: None,
            path: None,
            position: None,
            message: message.into(),
        }
    }

    /// A finding that does not.
    #[must_use]
    pub fn warning(rule: Rule, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            rule,
            unit: None,
            path: None,
            position: None,
            message: message.into(),
        }
    }

    /// Attaches the unit this is about.
    #[must_use]
    pub fn about(mut self, unit: UnitName) -> Self {
        self.unit = Some(unit);
        self
    }

    /// Attaches the path this is about.
    #[must_use]
    pub fn at(mut self, path: RelativePath) -> Self {
        self.path = Some(path);
        self
    }

    /// Attaches the place in that file.
    #[must_use]
    pub fn on(mut self, position: Position) -> Self {
        self.position = Some(position);
        self
    }

    /// What it is about, as the report names it: the unit and the path, each
    /// behind a space, or nothing.
    #[must_use]
    pub fn place(&self) -> String {
        let unit = self
            .unit
            .as_ref()
            .map_or_else(String::new, |unit| format!(" {unit}"));
        let path = match (&self.path, self.position) {
            (Some(path), Some(Position { line, column })) => format!(" at {path}:{line}:{column}"),
            (Some(path), None) => format!(" at {path}"),
            (None, _) => String::new(),
        };
        format!("{unit}{path}")
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Built in one piece and written once: writing part by part would put
        // an error path into every line that only a failing formatter could
        // ever take, and there is no such formatter here.
        f.write_str(&format!(
            "{}[{}]{}: {}",
            self.severity,
            self.rule,
            self.place(),
            self.message
        ))
    }
}
