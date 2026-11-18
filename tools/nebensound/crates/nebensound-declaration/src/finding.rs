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
    /// A manifest whose ecosystem no unit has watched.
    UnwatchedManifest,
    /// A watched ecosystem whose manifest does not exist.
    PointlessWatch,
    /// A file a tool's own setting takes out, and no exclusion does.
    UnexcusedIgnore,
    /// A tool's setting that takes files out, and that phase 0 cannot read.
    UnreadableIgnoreFile,
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
            Self::UnwatchedManifest => "unwatched-manifest",
            Self::PointlessWatch => "pointless-watch",
            Self::UnexcusedIgnore => "unexcused-ignore",
            Self::UnreadableIgnoreFile => "unreadable-ignore-file",
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
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
        let path = self
            .path
            .as_ref()
            .map_or_else(String::new, |path| format!(" at {path}"));
        f.write_str(&format!(
            "{}[{}]{unit}{path}: {}",
            self.severity, self.rule, self.message
        ))
    }
}
