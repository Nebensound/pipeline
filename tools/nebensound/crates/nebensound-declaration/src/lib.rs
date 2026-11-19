//! Reads, resolves and checks the `.nebensound/repo.toml` every Nebensound
//! repository carries.
//!
//! The crate is deliberately free of input and output. It is handed a
//! declaration, a register and a list of the paths a repository holds, and it
//! answers with resolved units and findings. Walking a working tree, reading
//! files and printing a report belong to whatever runs it, which is what keeps
//! every rule here testable without a filesystem.

pub mod annotate;
pub mod central;
pub mod edit;
pub mod finding;
pub mod history;
pub mod model;
pub mod names;
pub mod path;
pub mod pattern;
pub mod programs;
pub mod register;
pub mod resolve;
#[cfg(feature = "json-schema")]
pub mod schema;
pub mod tree;
pub mod validate;
pub mod version;

pub use annotate::{Annotated, Coverage, Kind, Node, annotate};
pub use central::{Central, Ruling};
pub use edit::DeclarationFile;
pub use finding::{Finding, Rule, Severity, UnitName};
pub use history::{History, UnitVersion, Versions};
pub use model::{Declaration, Pipeline, Publish, Release, Versioning};
pub use register::Register;
pub use resolve::{Resolution, Unit, resolve};
pub use tree::Tree;
pub use validate::{Analysis, analyse, validate};
pub use version::{TagVersion, Version};

use thiserror::Error;

/// Why a declaration could not be read.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ReadError {
    /// The TOML did not read, or did not match the shape.
    #[error("the declaration is not readable: {0}")]
    Unreadable(#[from] toml::de::Error),
    /// The same pattern stands twice under `[[exclude]]`.
    #[error(
        "{pattern} stands twice under [[exclude]]. A list can say the same thing twice, a \
         statement cannot: of two reasons for the same file, one is untrue"
    )]
    DuplicateExclusion {
        /// The duplicated pattern.
        pattern: String,
    },
    /// A repository excluded a pattern rather than a path.
    #[error(
        "{pattern} takes a whole type or name out of the check. Here a pattern may only add \
         work, never excuse any — write the path, or pull it back in with `include`"
    )]
    WildcardExclusion {
        /// The rejected pattern.
        pattern: String,
    },
}

impl Declaration {
    /// Reads the declaration of a repository.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError`] when the TOML does not read, carries a field the
    /// shape does not have, or writes an exclusion as a pattern.
    pub fn parse(source: &str) -> Result<Self, ReadError> {
        let declaration = Self::parse_central(source)?;
        if let Some(entry) = declaration
            .exclude
            .iter()
            .find(|entry| entry.path.is_wildcard())
        {
            return Err(ReadError::WildcardExclusion {
                pattern: entry.path.to_string(),
            });
        }
        Ok(declaration)
    }

    /// Reads a centrally kept declaration — the defaults.
    ///
    /// The same shape, one freedom more: here an exclusion may be a pattern.
    /// Centrally, work may be excused; locally it may only be added.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError`] when the TOML does not read or does not match the
    /// shape.
    pub fn parse_central(source: &str) -> Result<Self, ReadError> {
        let declaration: Self = toml::from_str(source)?;
        // As a table, uniqueness came for free: TOML forbids the same key
        // twice. A list does not, so the rule stands here now.
        let mut seen = std::collections::BTreeSet::new();
        for entry in &declaration.exclude {
            if !seen.insert(&entry.path) {
                return Err(ReadError::DuplicateExclusion {
                    pattern: entry.path.to_string(),
                });
            }
        }
        Ok(declaration)
    }
}
