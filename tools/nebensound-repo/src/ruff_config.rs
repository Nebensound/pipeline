//! Ruff's configuration files, as Ruff finds and reads them: the names it
//! looks for in a folder and the chain a configuration's `extend` names.
//!
//! Phase 0 ([`ignored`](crate::ignored)) and the rules report read the same
//! files the same way, so both take them from here. An `extend` is followed
//! inside the repository only: one that leaves it, or names a path Ruff would
//! expand (`~`, `$`), is not followed, and one that comes back to a file
//! already on the chain is a loop, which Ruff refuses.

use toml::{Table, Value};

use crate::ignored::{Read, folder};
use crate::path::RelativePath;

/// The files Ruff takes its configuration from in a folder, in the order it
/// looks for them; a `pyproject.toml` counts only with a `[tool.ruff]` table.
pub(crate) const FILES: &[&str] = &[".ruff.toml", "ruff.toml", "pyproject.toml"];

/// Ruff's settings in a file: all of it, or of a `pyproject.toml` its
/// `[tool.ruff]` table, [`None`] where it holds none.
pub(crate) fn settings(file: &RelativePath, source: &str) -> Result<Option<Table>, String> {
    let mut table: Table = toml::from_str(source).map_err(|error| error.to_string())?;
    if file.file_name() != Some("pyproject.toml") {
        return Ok(Some(table));
    }
    let Some(Value::Table(mut tool)) = table.remove("tool") else {
        return Ok(None);
    };
    match tool.remove("ruff") {
        None => Ok(None),
        Some(Value::Table(ruff)) => Ok(Some(ruff)),
        Some(_) => Err("`tool.ruff` is not a table".to_owned()),
    }
}

/// The file an `extend` names from the folder of the file it stands in, or
/// [`None`] where it lies outside the repository or Ruff would expand it.
fn resolve(dir: &RelativePath, extend: &str) -> Option<RelativePath> {
    if extend.starts_with(['/', '~']) || extend.contains('$') {
        return None;
    }
    let mut segments: Vec<&str> = dir.segments().iter().map(String::as_str).collect();
    for segment in extend.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            name => segments.push(name),
        }
    }
    RelativePath::new(&segments.join("/")).ok()
}

/// Where a file's `extend` leads.
pub(crate) enum Next {
    /// Outside the repository, or to a path Ruff would expand: not followed.
    Outside,
    /// Back to a file already on the chain, which Ruff refuses.
    Loop,
    /// To the next file of the chain.
    File(RelativePath),
}

/// One file of an `extend` chain.
pub(crate) struct Link {
    /// The file.
    pub(crate) file: RelativePath,
    /// Its Ruff settings, empty for a `pyproject.toml` without a
    /// `[tool.ruff]` table, or why they cannot be read.
    pub(crate) table: Result<Table, String>,
    /// What its `extend` names, as written, and where that leads.
    pub(crate) extend: Option<(String, Next)>,
}

/// The settings of a file and its `extend`, or why they cannot be read.
fn link(file: &RelativePath, read: &Read<'_>) -> Result<(Table, Option<String>), String> {
    let table = read(file)
        .and_then(|source| settings(file, &source))?
        .unwrap_or_default();
    let extend = match table.get("extend") {
        None => None,
        Some(Value::String(path)) => Some(path.clone()),
        Some(_) => return Err("`extend` is not text".to_owned()),
    };
    Ok((table, extend))
}

/// A configuration and every file its `extend` chain names, in that order.
/// The chain ends at a file that cannot be read, at one without `extend`, at
/// an `extend` that leaves the repository and at a loop.
pub(crate) fn chain(top: RelativePath, read: &Read<'_>) -> Vec<Link> {
    let mut links: Vec<Link> = Vec::new();
    let mut file = top;
    loop {
        let (table, extend) = match link(&file, read) {
            Ok((table, extend)) => (Ok(table), extend),
            Err(error) => (Err(error), None),
        };
        let next = extend.map(|written| {
            let next = match resolve(&folder(&file), &written) {
                None => Next::Outside,
                Some(next) if next == file || links.iter().any(|link| link.file == next) => {
                    Next::Loop
                }
                Some(next) => Next::File(next),
            };
            (written, next)
        });
        let follow = match &next {
            Some((_, Next::File(next))) => Some(next.clone()),
            _ => None,
        };
        links.push(Link {
            file,
            table,
            extend: next,
        });
        match follow {
            Some(next) => file = next,
            None => return links,
        }
    }
}
