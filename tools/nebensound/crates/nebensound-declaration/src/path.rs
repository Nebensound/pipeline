//! Repository-relative paths, compared by segment rather than by text.

use std::fmt;

use serde::{Deserialize, Deserializer, de};
use thiserror::Error;

/// Why a string could not become a repository-relative path.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum PathError {
    /// The path was empty.
    #[error("a path must not be empty; write \".\" for the repository itself")]
    Empty,
    /// The path started at the filesystem root.
    #[error("{value:?} is absolute, and a declaration only ever names paths inside the repository")]
    Absolute {
        /// The rejected path.
        value: String,
    },
    /// The path tried to leave the repository, or held an empty segment.
    #[error(
        "{value:?} holds the segment {segment:?}, which does not name anything inside the repository"
    )]
    Segment {
        /// The rejected path.
        value: String,
        /// The offending segment.
        segment: String,
    },
    /// The path used a backslash, which is not a separator here.
    #[error("{value:?} holds a backslash; separate path segments with '/'")]
    Backslash {
        /// The rejected path.
        value: String,
    },
}

/// A path inside the repository, held as its segments.
///
/// Holding segments rather than a string is the whole point: `crates/foo` does
/// not contain `crates/foobar`, although one is a text prefix of the other, and
/// the containment rule in phase 0 turns on exactly that difference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RelativePath {
    segments: Vec<String>,
}

impl RelativePath {
    /// The repository itself, written `.`.
    #[must_use]
    pub fn root() -> Self {
        Self::default()
    }

    /// Accepts the string if it names something inside the repository.
    ///
    /// `.` and the empty-segment forms of it name the repository itself.
    ///
    /// # Errors
    ///
    /// Returns [`PathError`] describing the first rule the string breaks.
    pub fn new(value: &str) -> Result<Self, PathError> {
        if value.is_empty() {
            return Err(PathError::Empty);
        }
        if value.contains('\\') {
            return Err(PathError::Backslash {
                value: value.to_owned(),
            });
        }
        if value.starts_with('/') {
            return Err(PathError::Absolute {
                value: value.to_owned(),
            });
        }
        if value == "." {
            return Ok(Self::root());
        }

        // A trailing slash is a way of writing a directory, so it is dropped
        // rather than rejected; any other empty segment is a mistake. A lone
        // "/" cannot reach this point: it was refused as absolute above.
        let trimmed = value.strip_suffix('/').unwrap_or(value);

        let mut segments = Vec::new();
        for segment in trimmed.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." {
                return Err(PathError::Segment {
                    value: value.to_owned(),
                    segment: segment.to_owned(),
                });
            }
            segments.push(segment.to_owned());
        }
        Ok(Self { segments })
    }

    /// Whether this path is the repository itself.
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    /// The segments, outermost first.
    #[must_use]
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Whether `other` lies at or below this path.
    ///
    /// The repository contains everything, itself included.
    #[must_use]
    pub fn contains(&self, other: &Self) -> bool {
        other.segments.starts_with(&self.segments)
    }

    /// Whether `other` lies directly below this path, one segment deeper.
    #[must_use]
    pub fn is_parent_of(&self, other: &Self) -> bool {
        other.segments.len() == self.segments.len() + 1 && self.contains(other)
    }

    /// The last segment, or [`None`] for the repository itself.
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.segments.last().map(String::as_str)
    }

    /// This path with one more segment below it.
    #[must_use]
    pub fn join(&self, segment: &str) -> Self {
        let mut segments = self.segments.clone();
        segments.push(segment.to_owned());
        Self { segments }
    }
}

impl fmt::Display for RelativePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segments.is_empty() {
            return f.write_str(".");
        }
        f.write_str(&self.segments.join("/"))
    }
}

impl<'de> Deserialize<'de> for RelativePath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(&raw).map_err(de::Error::custom)
    }
}
