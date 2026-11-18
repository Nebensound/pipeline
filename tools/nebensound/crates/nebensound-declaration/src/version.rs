//! A version as a declaration writes it: `X.Y.Z`.
//!
//! Only the three numbers. A declaration names a version where a person takes
//! a step by hand, such as the floor below which a unit's version never falls;
//! a pre-release or build suffix is something the pipeline appends, never
//! something a person writes there.

use std::fmt;

use serde::{Deserialize, Deserializer, de};
use thiserror::Error;

/// Why a string is not a version.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum VersionError {
    /// The string is not three numbers separated by dots.
    #[error(
        "a version is three numbers separated by dots, as X.Y.Z, without a leading zero, a \
         prefix or a suffix; {value:?} is not"
    )]
    Shape {
        /// The rejected string.
        value: String,
    },
    /// A number is too large to be counted.
    #[error("{value:?} holds a number too large to be a version")]
    TooLarge {
        /// The rejected string.
        value: String,
    },
}

/// A version of three numbers: major, minor and patch.
///
/// Ordered the way versions are: by major, then minor, then patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Raised for a change that breaks what a consumer relies on.
    pub major: u64,
    /// Raised for a change that adds something.
    pub minor: u64,
    /// Raised for everything else.
    pub patch: u64,
}

impl Version {
    /// A version from its three numbers.
    #[must_use]
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Reads `X.Y.Z`.
    ///
    /// Each number is digits only, and `0` or starting on a digit other than
    /// `0`: `01.0.0` would read as `1.0.0` here and as something else in a tag
    /// someone compares by text.
    ///
    /// # Errors
    ///
    /// Returns [`VersionError::Shape`] for anything that is not three such
    /// numbers, and [`VersionError::TooLarge`] for a number beyond `u64`.
    pub fn parse(value: &str) -> Result<Self, VersionError> {
        let shape = || VersionError::Shape {
            value: value.to_owned(),
        };
        let mut numbers = [0_u64; 3];
        let mut parts = value.split('.');
        for number in &mut numbers {
            let part = parts.next().ok_or_else(shape)?;
            let digits = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
            if !digits || (part.len() > 1 && part.starts_with('0')) {
                return Err(shape());
            }
            *number = part.parse().map_err(|_| VersionError::TooLarge {
                value: value.to_owned(),
            })?;
        }
        if parts.next().is_some() {
            return Err(shape());
        }
        let [major, minor, patch] = numbers;
        Ok(Self::new(major, minor, patch))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(de::Error::custom)
    }
}
