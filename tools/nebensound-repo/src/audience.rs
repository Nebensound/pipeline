//! For whom a repository is meant, and what phase 0 holds that against.
//!
//! The audience is a fact of the declaration: it is changed by pull request
//! and read from the tree alone, so that it is known on a developer's machine
//! without the network as well. GitHub knows whether a repository is public,
//! not whether a customer sees it, so the declaration says it, and a run holds
//! the part GitHub does know against it: `public` exactly when the repository
//! is public. On a developer's machine nothing is compared.
//!
//! The field takes no part in resolving versions: the declarations of earlier
//! commits are read with it or without it, and only the declaration at hand
//! has to carry it.

use std::fmt;

use clap::ValueEnum;
use serde::Deserialize;

use crate::finding::{Finding, Rule, UnitName};
use crate::history::History;
use crate::model::{Audience, Declaration};
use crate::path::RelativePath;

/// What a run knows of the repository: how GitHub shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Visibility {
    /// Anyone can see it.
    Public,
    /// Only those given access can see it.
    Private,
    /// Every member of the enterprise can see it.
    Internal,
}

impl Visibility {
    /// The value as GitHub writes it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Internal => "internal",
        }
    }
}

impl fmt::Display for Visibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `.nebensound/repo.toml` as a path in the repository, where both findings
/// hang.
fn declaration_path() -> RelativePath {
    RelativePath::root().join(".nebensound").join("repo.toml")
}

/// The finding for a declaration at hand that does not say for whom the
/// repository is meant, if it does not.
#[must_use]
pub fn missing(declaration: &Declaration) -> Option<Finding> {
    if declaration.repo.audience.is_some() {
        return None;
    }
    Some(
        Finding::error(
            Rule::MissingAudience,
            "the declaration does not say for whom the repository is meant. Write `audience` in \
             [repo]: \"public\" for open source of the organization, \"internal\" for the \
             organization alone, \"customer\" where a customer can see it",
        )
        .about(UnitName::Repository)
        .at(declaration_path()),
    )
}

/// The finding for an audience that does not fit how GitHub shows the
/// repository, if it does not: `public` is declared exactly when the
/// repository is public. Without an audience there is nothing to compare;
/// [`missing`] reports that.
#[must_use]
pub fn against(audience: Option<Audience>, visibility: Visibility) -> Option<Finding> {
    let audience = audience?;
    let declared_public = audience == Audience::Public;
    if declared_public == (visibility == Visibility::Public) {
        return None;
    }
    let remedy = if declared_public {
        "make the repository public, or declare \"internal\" or \"customer\""
    } else {
        "declare \"public\", or make the repository private"
    };
    Some(
        Finding::error(
            Rule::MismatchedAudience,
            format!(
                "the declaration says audience = \"{audience}\", and GitHub shows the repository \
                 as {visibility}: \"public\" is declared exactly when the repository is public. \
                 Either {remedy}"
            ),
        )
        .about(UnitName::Repository)
        .at(declaration_path()),
    )
}

/// The top level of a declaration as far as the audience goes. Nothing else
/// of the file is judged here, so a declaration that phase 0 could not read
/// as a whole still says what it names.
#[derive(Deserialize)]
struct AudienceOnly {
    #[serde(default)]
    repo: RepoAudience,
}

/// `[repo]` as far as the audience goes.
#[derive(Deserialize, Default)]
struct RepoAudience {
    #[serde(default)]
    audience: Option<Audience>,
}

/// The audience a declaration names, if its text names a valid one.
fn named(text: &str) -> Option<Audience> {
    toml::from_str::<AudienceOnly>(text).ok()?.repo.audience
}

/// The audience the commit before the commit at hand on the line declares:
/// on a pull request into `main` the tip of `main`, otherwise the commit
/// before on the line of `main`. At a merge from `main` into `release` that
/// is the commit of `main` it merges, not the tip of `release`, so a
/// promotion shows no change: the change was shown on its pull request into
/// `main`. [`None`] when there is no such commit, or its declaration is
/// missing or names no valid audience.
#[must_use]
pub fn previous(history: &History) -> Option<Audience> {
    let object = history.line.get(1)?.declaration.as_deref()?;
    history
        .declarations
        .get(object)
        .map(String::as_str)
        .and_then(named)
}
