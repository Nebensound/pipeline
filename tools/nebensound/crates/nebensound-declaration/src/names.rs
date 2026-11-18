//! The constrained strings a declaration is made of.
//!
//! Each is a newtype with a validating constructor, so a check name cannot be
//! passed where a build name belongs, and a component name that could never
//! become a Git tag cannot be constructed at all.

use std::fmt;

use serde::{Deserialize, Deserializer, de};
use thiserror::Error;

/// Why a string could not become the name it was meant to be.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum NameError {
    /// The string was empty.
    #[error("a {kind} must not be empty")]
    Empty {
        /// What was being named.
        kind: &'static str,
    },
    /// The string held a character a name may not carry.
    #[error("a {kind} holds lowercase letters, digits, '-' and '.', but {value:?} holds {found:?}")]
    Character {
        /// What was being named.
        kind: &'static str,
        /// The rejected string.
        value: String,
        /// The first character that does not belong.
        found: char,
    },
    /// The string began or ended on a separator.
    #[error("a {kind} starts and ends on a letter or digit, and {value:?} does not")]
    Boundary {
        /// What was being named.
        kind: &'static str,
        /// The rejected string.
        value: String,
    },
    /// The string held two dots in a row, which Git refuses in a reference.
    #[error("a {kind} must not hold '..', and {value:?} does")]
    DoubleDot {
        /// What was being named.
        kind: &'static str,
        /// The rejected string.
        value: String,
    },
}

/// The one rule behind every name here.
///
/// It is deliberately narrower than Git's own reference grammar. Everything
/// admitted below survives a tag, a file name, a shell argument and a URL path
/// segment without quoting, which is what these names are actually used for.
fn parse(kind: &'static str, value: &str) -> Result<String, NameError> {
    if value.is_empty() {
        return Err(NameError::Empty { kind });
    }
    if let Some(found) = value
        .chars()
        .find(|c| !matches!(c, 'a'..='z' | '0'..='9' | '-' | '.'))
    {
        return Err(NameError::Character {
            kind,
            value: value.to_owned(),
            found,
        });
    }
    let boundary = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    if !value.starts_with(boundary) || !value.ends_with(boundary) {
        return Err(NameError::Boundary {
            kind,
            value: value.to_owned(),
        });
    }
    if value.contains("..") {
        return Err(NameError::DoubleDot {
            kind,
            value: value.to_owned(),
        });
    }
    Ok(value.to_owned())
}

macro_rules! identifier {
    ($(#[$doc:meta])* $name:ident, $kind:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Accepts the string if it can be this kind of name.
            ///
            /// # Errors
            ///
            /// Returns [`NameError`] describing the first rule the string breaks.
            pub fn new(value: &str) -> Result<Self, NameError> {
                parse($kind, value).map(Self)
            }

            /// The name as written.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                Self::new(&raw).map_err(de::Error::custom)
            }
        }
    };
}

identifier!(
    /// The name of a component, which also becomes its tag prefix.
    ComponentName,
    "component name"
);
identifier!(
    /// The name of a preset in the central register.
    PresetName,
    "preset name"
);
identifier!(
    /// The name of a check in the central register.
    CheckName,
    "check name"
);
identifier!(
    /// The name of a build in the central register.
    BuildName,
    "build name"
);
identifier!(
    /// The name of a publication target in the central register.
    TargetName,
    "target name"
);
identifier!(
    /// The name of an artifact kind, which links a build to a target.
    ArtifactKind,
    "artifact kind"
);
identifier!(
    /// The name of a version writer in the central register.
    VersionWriterName,
    "version writer name"
);
identifier!(
    /// The name of a package ecosystem Dependabot watches.
    EcosystemName,
    "ecosystem name"
);

/// A file extension, written with its leading dot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Extension(String);

impl Extension {
    /// Accepts the string if it is an extension.
    ///
    /// # Errors
    ///
    /// Returns [`NameError`] if the leading dot is missing or the remainder is
    /// not a plain lowercase suffix.
    pub fn new(value: &str) -> Result<Self, NameError> {
        let kind = "file extension";
        let Some(suffix) = value.strip_prefix('.') else {
            return Err(NameError::Boundary {
                kind,
                value: value.to_owned(),
            });
        };
        parse(kind, suffix).map(|suffix| Self(format!(".{suffix}")))
    }

    /// The extension as written, leading dot included.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The extension of a path, if it has one.
    ///
    /// Returns [`None`] for a name without a dot, and for a dotfile such as
    /// `.gitignore`, whose leading dot does not introduce an extension.
    #[must_use]
    pub fn of(path: &str) -> Option<Self> {
        let name = path.rsplit('/').next().unwrap_or(path);
        let (stem, suffix) = name.rsplit_once('.')?;
        if stem.is_empty() {
            return None;
        }
        Self::new(&format!(".{}", suffix.to_ascii_lowercase())).ok()
    }
}

impl fmt::Display for Extension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Extension {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(&raw).map_err(de::Error::custom)
    }
}

/// A reason someone wrote down, for an exclusion or a silenced check.
///
/// It exists as a type so that the reason cannot be forgotten: the file format
/// makes it the value of the entry, and this makes an empty one an error.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Reason(String);

/// Why a reason was rejected.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ReasonError {
    /// The reason held nothing but whitespace.
    #[error("a reason must say something, and this one is blank")]
    Blank,
}

impl Reason {
    /// Accepts the string if it says anything at all.
    ///
    /// # Errors
    ///
    /// Returns [`ReasonError::Blank`] for an empty or whitespace-only string.
    pub fn new(value: &str) -> Result<Self, ReasonError> {
        if value.trim().is_empty() {
            return Err(ReasonError::Blank);
        }
        Ok(Self(value.to_owned()))
    }

    /// The reason as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Reason {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(&raw).map_err(de::Error::custom)
    }
}
