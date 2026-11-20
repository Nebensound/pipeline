//! Changing a declaration without losing what someone wrote in it.
//!
//! `repo.toml` is handwritten and carries comments that explain decisions, so
//! a tool that rewrites it by serialising the typed model would quietly throw
//! those away. Every change here goes through `toml_edit`, which keeps the
//! formatting and the comments as they are.

use std::fmt;

use thiserror::Error;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

use crate::names::{ComponentName, Reason};
use crate::path::RelativePath;
use crate::pattern::Pattern;

/// Why a declaration could not be changed.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EditError {
    /// The file did not parse as TOML.
    #[error("the declaration is not TOML: {0}")]
    Unparsable(#[from] toml_edit::TomlError),
    /// The entry is already there, and this would have overwritten it.
    #[error("{what} {name:?} is already declared; change it rather than adding it again")]
    AlreadyPresent {
        /// What kind of entry it is.
        what: &'static str,
        /// Its name.
        name: String,
    },
    /// A table that has to be a table is something else.
    #[error("{path} is {found} in this file, and it has to be a table")]
    NotATable {
        /// Where the problem is.
        path: &'static str,
        /// What was found instead.
        found: &'static str,
    },
    /// A list of tables that has to be one is something else.
    ///
    /// Separate from [`EditError::NotATable`] so that the advice matches the
    /// shape: `[exclude]` written as a table is exactly what is refused here,
    /// and being told to write a table would send the reader back to it.
    #[error("{path} is {found} in this file, and it has to be a list of tables")]
    NotAListOfTables {
        /// Where the problem is.
        path: &'static str,
        /// What was found instead.
        found: &'static str,
    },
}

/// A declaration held as the document it is, ready to be changed.
#[derive(Debug, Clone)]
pub struct DeclarationFile {
    document: DocumentMut,
}

impl DeclarationFile {
    /// Reads the file, keeping its formatting and comments.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::Unparsable`] if the text is not TOML. It does not
    /// check the declaration against the format; use [`crate::Declaration`]
    /// for that.
    pub fn parse(source: &str) -> Result<Self, EditError> {
        Ok(Self {
            document: source.parse::<DocumentMut>()?,
        })
    }

    /// An empty declaration that says the repository publishes nothing.
    ///
    /// This is the file every repository starts with, and it is deliberately
    /// not empty: an empty file could not be told apart from a forgotten one.
    #[must_use]
    pub fn minimal() -> Self {
        let mut document = DocumentMut::new();
        let mut repo = Table::new();
        repo["pipeline"] = value("none");
        document["repo"] = Item::Table(repo);
        Self { document }
    }

    /// Adds a component.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::AlreadyPresent`] if a component of that name is
    /// already declared, and [`EditError::NotATable`] if `[component]` is
    /// something other than a table.
    pub fn add_component(
        &mut self,
        name: &ComponentName,
        path: &RelativePath,
    ) -> Result<(), EditError> {
        let components = self.table("component")?;
        if components.contains_key(name.as_str()) {
            return Err(EditError::AlreadyPresent {
                what: "component",
                name: name.to_string(),
            });
        }
        let mut component = Table::new();
        component["path"] = value(path.to_string());
        components.insert(name.as_str(), Item::Table(component));
        Ok(())
    }

    /// Excludes a path from the coverage check, with its reason.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::AlreadyPresent`] if the path is already excluded,
    /// and [`EditError::NotAListOfTables`] if `exclude` is something other than
    /// a list of tables.
    pub fn exclude(&mut self, pattern: &Pattern, reason: &Reason) -> Result<(), EditError> {
        let written = pattern.to_string();
        let item = self
            .document
            .entry("exclude")
            .or_insert(Item::ArrayOfTables(ArrayOfTables::new()));
        let Some(entries) = item.as_array_of_tables_mut() else {
            return Err(EditError::NotAListOfTables {
                path: "exclude",
                found: item.type_name(),
            });
        };
        if entries
            .iter()
            .any(|entry| entry.get("path").and_then(Item::as_str) == Some(written.as_str()))
        {
            return Err(EditError::AlreadyPresent {
                what: "the path",
                name: written,
            });
        }
        let mut entry = Table::new();
        entry["path"] = value(written);
        entry["reason"] = value(reason.as_str());
        entries.push(entry);
        Ok(())
    }

    /// Pulls a path or a pattern back into the check.
    ///
    /// Writing the same entry twice is not an error: the list says what is
    /// checked, and saying it twice says the same thing.
    ///
    /// The reason is optional: adding work does not have to justify itself.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::NotAListOfTables`] when `include` stands there as
    /// something other than a list of tables.
    pub fn include(&mut self, pattern: &Pattern, reason: Option<&Reason>) -> Result<(), EditError> {
        let written = pattern.to_string();
        let item = self
            .document
            .entry("include")
            .or_insert(Item::ArrayOfTables(ArrayOfTables::new()));
        let Some(entries) = item.as_array_of_tables_mut() else {
            return Err(EditError::NotAListOfTables {
                path: "include",
                found: item.type_name(),
            });
        };
        if entries
            .iter()
            .any(|entry| entry.get("path").and_then(Item::as_str) == Some(written.as_str()))
        {
            return Ok(());
        }
        let mut entry = Table::new();
        entry["path"] = value(written);
        if let Some(reason) = reason {
            entry["reason"] = value(reason.as_str());
        }
        entries.push(entry);
        Ok(())
    }

    fn table(&mut self, name: &'static str) -> Result<&mut Table, EditError> {
        let item = self
            .document
            .entry(name)
            .or_insert(Item::Table(Table::new()));
        let found = item.type_name();
        item.as_table_mut()
            .ok_or(EditError::NotATable { path: name, found })
    }
}

impl fmt::Display for DeclarationFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.document)
    }
}
