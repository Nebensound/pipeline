//! What an exclusion or an inclusion may name.
//!
//! A concrete path, a type (`*.png`) or a file name anywhere (`**/LICENSE`).
//! Nothing more: both are recognizable by their prefix, so no glob library is
//! needed and no edge cases nobody reads.

use std::fmt;

use serde::{Deserialize, Deserializer, de};
use thiserror::Error;

use crate::names::{Extension, NameError};
use crate::path::{PathError, RelativePath};

/// Why a string could not become a pattern.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum PatternError {
    /// After `*.` came no valid extension.
    #[error("{value:?} names no type: {source}")]
    Type {
        /// The rejected string.
        value: String,
        /// What was wrong with the extension.
        source: NameError,
    },
    /// After `**/` came no plain name.
    #[error(
        "{value:?} names nothing after `**/`; exactly one path segment goes there, with no `/` in it"
    )]
    Name {
        /// The rejected string.
        value: String,
    },
    /// Neither a pattern nor a valid path.
    #[error("{value:?} is neither a pattern nor a path: {source}")]
    Path {
        /// The rejected string.
        value: String,
        /// What was wrong with the path.
        source: PathError,
    },
}

/// A path, or a pattern over paths.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Pattern {
    /// A concrete path. Matches everything below it.
    Path(RelativePath),
    /// A name anywhere, written `**/<name>`.
    ///
    /// It matches **every** segment: a file of that name as well as everything
    /// below a folder of that name. Otherwise the form would be useless for the
    /// case it is written for — `**/node_modules` means the directory, not a
    /// file of the same name.
    Name(String),
    /// A type, written `*.<extension>`.
    Type(Extension),
}

impl Pattern {
    /// Reads a pattern or a path.
    ///
    /// # Errors
    ///
    /// Returns [`PatternError`] when the string is neither.
    pub fn new(value: &str) -> Result<Self, PatternError> {
        if let Some(suffix) = value.strip_prefix("*.") {
            return Extension::new(&format!(".{suffix}"))
                .map(Self::Type)
                .map_err(|source| PatternError::Type {
                    value: value.to_owned(),
                    source,
                });
        }
        if let Some(name) = value.strip_prefix("**/") {
            if name.is_empty() || name.contains('/') {
                return Err(PatternError::Name {
                    value: value.to_owned(),
                });
            }
            return Ok(Self::Name(name.to_owned()));
        }
        RelativePath::new(value)
            .map(Self::Path)
            .map_err(|source| PatternError::Path {
                value: value.to_owned(),
                source,
            })
    }

    /// Whether this pattern matches the file.
    #[must_use]
    pub fn matches(&self, file: &RelativePath) -> bool {
        match self {
            Self::Path(path) => path.contains(file),
            Self::Name(name) => file.segments().iter().any(|segment| segment == name),
            Self::Type(extension) => Extension::of(&file.to_string()).as_ref() == Some(extension),
        }
    }

    /// How specific the pattern is. Greater beats smaller.
    ///
    /// A concrete path beats every pattern, a longer path a shorter one, and a
    /// name beats a type: it names a place in the tree, the type a class of
    /// files everywhere.
    #[must_use]
    pub fn specificity(&self) -> (u8, usize) {
        match self {
            Self::Path(path) => (2, path.segments().len()),
            Self::Name(_) => (1, 0),
            Self::Type(_) => (0, 0),
        }
    }

    /// Whether it is a pattern and not a concrete path.
    ///
    /// A repository may only use a pattern to add work.
    #[must_use]
    pub fn is_wildcard(&self) -> bool {
        !matches!(self, Self::Path(_))
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(path) => write!(f, "{path}"),
            Self::Name(name) => write!(f, "**/{name}"),
            Self::Type(extension) => write!(f, "*{extension}"),
        }
    }
}

impl<'de> Deserialize<'de> for Pattern {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(&raw).map_err(de::Error::custom)
    }
}
