//! Versions: as a declaration writes them, as a tag carries them, and how a
//! commit raises one.
//!
//! A version is three numbers, `X.Y.Z` ([`Version`]). A tag carries one, and
//! a pre-release appends `-pre.N` to it ([`TagVersion`]). A declaration names
//! a version where a person takes a step by hand, the floor below which a
//! unit's version never falls, and writes it the way a tag carries it.
//!
//! What a commit asks of a version follows from its message, read as a
//! Conventional Commit ([`change_of`]): a breaking change raises the major
//! version, `feat` the minor one, everything else the patch. Below `1.0.0`
//! less is promised, and less is raised ([`Version::raised`]): below `0.1.0`
//! every change is a patch, and from there a breaking change raises the minor
//! version. The steps to `0.1.0` and to `1.0.0` are a person's, taken with
//! `minimum-version`.

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
    /// The string is no version as a tag carries it.
    #[error(
        "a version reads X.Y.Z or X.Y.Z-pre.N: three numbers separated by dots without a \
         leading zero or a prefix, and for a pre-release `-pre.` and a number from 1 without a \
         leading zero; {value:?} is not"
    )]
    TagShape {
        /// The rejected string.
        value: String,
    },
    /// The string names a pre-release other than `pre.N`.
    #[error(
        "{value:?} names the pre-release {identifier:?}, and the only pre-release a version \
         names is pre.N, as in X.Y.Z-pre.3"
    )]
    PreRelease {
        /// The rejected string.
        value: String,
        /// What stands after the `-`.
        identifier: String,
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
    /// `0.0.0`, the base of a unit before its first released commit.
    pub const ZERO: Self = Self::new(0, 0, 0);

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

    /// This version raised once, by this change.
    ///
    /// Below `0.1.0` every change is a patch. From `0.1.0` to below `1.0.0` a
    /// breaking change raises the minor version (`0.3.2` to `0.4.0`), and
    /// everything else, `feat` included, the patch. From `1.0.0` on a breaking
    /// change raises the major version and `feat` the minor one. No change
    /// leads to `1.0.0`: that step is a person's, taken with `minimum-version`.
    ///
    /// [`None`] when the field to raise already holds the largest number a
    /// version can carry: there is no version after it, and wrapping round to
    /// zero would move the version backwards.
    #[must_use]
    pub fn raised(self, change: Change) -> Option<Self> {
        let change = if self < Self::new(0, 1, 0) {
            Change::Patch
        } else if self.major == 0 {
            match change {
                Change::Major => Change::Minor,
                Change::Minor | Change::Patch => Change::Patch,
            }
        } else {
            change
        };
        Some(match change {
            Change::Patch => Self::new(self.major, self.minor, self.patch.checked_add(1)?),
            Change::Minor => Self::new(self.major, self.minor.checked_add(1)?, 0),
            Change::Major => Self::new(self.major.checked_add(1)?, 0, 0),
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A version as a tag carries it: `X.Y.Z` for a full release, `X.Y.Z-pre.N`
/// for a pre-release.
///
/// Ordered the way releases follow each other: by the three numbers, then a
/// pre-release before the full release of the same numbers, then by `N`.
/// `1.0.0-pre.2` comes after `1.0.0-pre.1` and before `1.0.0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TagVersion {
    /// The three numbers.
    pub version: Version,
    /// `N` of a pre-release, counted from 1; [`None`] for a full release.
    pub pre: Option<u64>,
}

impl TagVersion {
    /// A full release.
    #[must_use]
    pub const fn stable(version: Version) -> Self {
        Self { version, pre: None }
    }

    /// The `n`th pre-release of `version`.
    #[must_use]
    pub const fn pre(version: Version, n: u64) -> Self {
        Self {
            version,
            pre: Some(n),
        }
    }

    /// Whether this is a pre-release.
    #[must_use]
    pub const fn is_pre(&self) -> bool {
        self.pre.is_some()
    }

    /// Reads `X.Y.Z` or `X.Y.Z-pre.N`, strictly, and says why not.
    ///
    /// The three numbers as [`Version::parse`] reads them, and `N` digits only,
    /// at least 1 and without a leading zero. Anything else, such as `-rc.1`,
    /// `-pre1` or a build suffix, is refused: a tag that holds it is no release
    /// the pipeline made, and a floor that names it names a version the
    /// pipeline never gives out.
    ///
    /// # Errors
    ///
    /// Returns [`VersionError::PreRelease`] for a pre-release other than
    /// `pre.N`, [`VersionError::TooLarge`] for a number beyond `u64`, and
    /// [`VersionError::TagShape`] for anything else.
    pub fn read(value: &str) -> Result<Self, VersionError> {
        let shape = || VersionError::TagShape {
            value: value.to_owned(),
        };
        let (numbers, suffix) = match value.split_once('-') {
            Some((numbers, suffix)) => (numbers, Some(suffix)),
            None => (value, None),
        };
        let version = Version::parse(numbers).map_err(|error| match error {
            VersionError::TooLarge { .. } => VersionError::TooLarge {
                value: value.to_owned(),
            },
            _ => shape(),
        })?;
        let Some(suffix) = suffix else {
            return Ok(Self::stable(version));
        };
        if suffix.is_empty() {
            return Err(shape());
        }
        let Some(n) = suffix.strip_prefix("pre.") else {
            return Err(VersionError::PreRelease {
                value: value.to_owned(),
                identifier: suffix.to_owned(),
            });
        };
        let digits = !n.is_empty() && n.bytes().all(|byte| byte.is_ascii_digit());
        if !digits || n.starts_with('0') {
            return Err(shape());
        }
        let n = n.parse().map_err(|_| VersionError::TooLarge {
            value: value.to_owned(),
        })?;
        Ok(Self::pre(version, n))
    }

    /// Reads `X.Y.Z` or `X.Y.Z-pre.N` as [`TagVersion::read`] does; [`None`]
    /// for anything else.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::read(value).ok()
    }
}

impl<'de> Deserialize<'de> for TagVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::read(&raw).map_err(de::Error::custom)
    }
}

impl Ord for TagVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // A full release sorts after every pre-release of the same numbers.
        let rank = |pre: Option<u64>| pre.map_or((1, 0), |n| (0, n));
        self.version
            .cmp(&other.version)
            .then_with(|| rank(self.pre).cmp(&rank(other.pre)))
    }
}

impl PartialOrd for TagVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for TagVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.pre {
            None => write!(f, "{}", self.version),
            Some(n) => write!(f, "{}-pre.{n}", self.version),
        }
    }
}

/// How much a commit changes, in the order of the version fields it raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Change {
    /// A compatible correction, or anything that is neither of the others.
    Patch,
    /// `feat`: something new, compatible.
    Minor,
    /// `!` after the type, or a `BREAKING CHANGE` footer.
    Major,
}

/// A commit message that is not a Conventional Commit.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error(
    "{header:?} is not a Conventional Commit: it does not read `<type>[(<scope>)][!]: \
     <description>`"
)]
pub struct Unconventional {
    /// The first line of the message.
    pub header: String,
}

/// What a commit message asks of the version.
///
/// The header is `<type>[(<scope>)][!]: <description>`. The type is letters
/// only and compared without regard to case, as the specification allows;
/// `BREAKING CHANGE: ` and `BREAKING-CHANGE: ` count only as written, at the
/// start of a line below the header.
///
/// # Errors
///
/// Returns [`Unconventional`] when the header does not have that form.
pub fn change_of(message: &str) -> Result<Change, Unconventional> {
    let mut lines = message.lines();
    let header = lines.next().unwrap_or_default();
    let unconventional = || Unconventional {
        header: header.to_owned(),
    };

    let (prefix, description) = header.split_once(": ").ok_or_else(unconventional)?;
    if description.trim().is_empty() {
        return Err(unconventional());
    }
    let (prefix, bang) = match prefix.strip_suffix('!') {
        Some(rest) => (rest, true),
        None => (prefix, false),
    };
    let kind = match prefix.split_once('(') {
        None => prefix,
        Some((kind, scope)) => {
            let scope = scope.strip_suffix(')').ok_or_else(unconventional)?;
            if scope.is_empty() || scope.contains(['(', ')']) {
                return Err(unconventional());
            }
            kind
        }
    };
    if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(unconventional());
    }

    let breaking = bang
        || lines.any(|line| {
            line.starts_with("BREAKING CHANGE: ") || line.starts_with("BREAKING-CHANGE: ")
        });
    Ok(if breaking {
        Change::Major
    } else if kind.eq_ignore_ascii_case("feat") {
        Change::Minor
    } else {
        Change::Patch
    })
}
