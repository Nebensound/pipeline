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
    /// A build without a version writer it needs, so that its artifact would
    /// carry a version the run did not write.
    BuildWithoutVersionWriter,
    /// A manifest whose ecosystem no unit has watched.
    UnwatchedManifest,
    /// A watched ecosystem whose manifest does not exist.
    PointlessWatch,
    /// A tag with a unit's prefix that is not one of its releases.
    UnexpectedTag,
    /// A commit message that is not a Conventional Commit.
    UnconventionalCommit,
    /// A clone without the whole history, so no version can be told.
    ShallowClone,
    /// A unit released in two stages, and no branch `origin/release`.
    MissingReleaseBranch,
    /// A unit released in one stage whose newest tag is a pre-release that its
    /// last full release does not hold.
    UnreleasedBeforeSwitch,
    /// A version whose field to raise holds the largest number already.
    ExhaustedVersion,
    /// A branch `origin/release`, and no unit released in two stages.
    PointlessReleaseBranch,
    /// A unit released in one stage whose minimum version is a pre-release.
    PreReleaseMinimum,
    /// A merge into `release` of a commit other than the tip of `main`.
    StalePromotion,
    /// A file a tool's own setting takes out, and no exclusion does.
    UnexcusedIgnore,
    /// A tool's setting that takes files out, and that phase 0 cannot read.
    UnreadableIgnoreFile,
    /// An exclusion that takes code out of a Rust package, and not the whole
    /// package.
    PartialPackageExclusion,
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
            Self::BuildWithoutVersionWriter => "build-without-version-writer",
            Self::UnwatchedManifest => "unwatched-manifest",
            Self::PointlessWatch => "pointless-watch",
            Self::UnexpectedTag => "unexpected-tag",
            Self::UnconventionalCommit => "unconventional-commit",
            Self::ShallowClone => "shallow-clone",
            Self::MissingReleaseBranch => "missing-release-branch",
            Self::UnreleasedBeforeSwitch => "unreleased-before-switch",
            Self::ExhaustedVersion => "exhausted-version",
            Self::PointlessReleaseBranch => "pointless-release-branch",
            Self::PreReleaseMinimum => "pre-release-minimum",
            Self::StalePromotion => "stale-promotion",
            Self::UnexcusedIgnore => "unexcused-ignore",
            Self::UnreadableIgnoreFile => "unreadable-ignore-file",
            Self::PartialPackageExclusion => "partial-package-exclusion",
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
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Built in one piece and written once: writing part by part would put
        // an error path into every line that only a failing formatter could
        // ever take, and there is no such formatter here.
        let unit = self
            .unit
            .as_ref()
            .map_or_else(String::new, |unit| format!(" {unit}"));
        let path = match (&self.path, self.position) {
            (Some(path), Some(Position { line, column })) => format!(" at {path}:{line}:{column}"),
            (Some(path), None) => format!(" at {path}"),
            (None, _) => String::new(),
        };
        f.write_str(&format!(
            "{}[{}]{unit}{path}: {}",
            self.severity, self.rule, self.message
        ))
    }
}
