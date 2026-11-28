//! The HTML check: every `.html` and `.htm` file is formatted the way Prettier
//! writes it, every page carries what a page must carry (the page rules,
//! [`page`]), and its links into the repository lead somewhere.
//!
//! The page rules are the tool's own judgement, on the document as html5ever,
//! linked into the tool, parses it. Prettier and lychee are the programs it
//! starts.

pub mod page;

use std::fs;
use std::path::{Path, PathBuf};

use super::links::{self, Handed};
use super::{Entry, Session};

/// Runs the check; whether it found nothing.
pub(super) fn run(session: &mut Session<'_, '_, '_>) -> bool {
    // A missing program is named and fails the check, and the other parts
    // still run, so that one run reports every finding.
    let prettier = session.tool("prettier", false);
    let lychee = session.tool("lychee", false);
    let handed = links::hand_over(session);
    let mut root = PathBuf::new();
    let Some(temp) = session.folder(|path| {
        root = rooted(path);
        let mut entries: Vec<Entry> = vec![
            ("nothing-ignored".to_owned(), Some(Vec::new())),
            ("root".to_owned(), None),
            (
                "lychee-rooted.toml".to_owned(),
                Some(excluded(&root).into_bytes()),
            ),
        ];
        entries.extend(Handed::entries(handed.as_ref()));
        entries
    }) else {
        return false;
    };
    let files = session.files.clone();
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    // Each part runs even when one before it failed, so that one run reports
    // every finding at once.
    let formatted = prettier
        .is_some_and(|prettier| session.prettier(&prettier, &temp.join("nothing-ignored"), &files));
    let pages = pages(session, &files);
    let site = temp.join("lychee-rooted.toml");
    let links_lead = handed.is_some_and(|handed| {
        lychee.is_some_and(|lychee| links::check(session, &lychee, &handed, &temp, &[&site], &root))
    });
    formatted && pages && links_lead
}

/// The page rules over the files ([`page`]): whether no file breaks one. Each
/// finding is an error at its file, line and column.
fn pages(session: &mut Session<'_, '_, '_>, files: &[&str]) -> bool {
    session.log.line(
        "Page rules: every page has a title, a description, exactly one <h1>, and alternative \
         text on every image",
    );
    let mut passed = true;
    for file in files {
        let Some(bytes) = session.contents(file) else {
            passed = false;
            continue;
        };
        for finding in page::findings(&bytes) {
            session.finding(file, &finding);
            passed = false;
        }
    }
    passed
}

/// The folder a link from the root of a site, such as `/style.css` or
/// `/login`, is resolved below: an empty one in the folder of what the
/// programs are handed, `folder`. Such a link is not checked, since where the
/// site is served and which routes its server answers the declaration does
/// not say. The path is taken without links, as lychee writes the addresses
/// it resolves below it; on macOS the temporary folder lies behind one.
#[must_use]
pub fn rooted(folder: &Path) -> PathBuf {
    fs::canonicalize(folder)
        .unwrap_or(folder.to_path_buf())
        .join("root")
}

/// The second configuration of the link check, behind the repository's: it
/// leaves out every address below `root` (see [`rooted`]), through a pattern
/// that takes every character of the folder's path literally.
#[must_use]
pub fn excluded(root: &Path) -> String {
    let pattern = format!("^file://{}/", regex::escape(&root.to_string_lossy()));
    let mut table = toml::Table::new();
    table.insert(
        "exclude".to_owned(),
        toml::Value::Array(vec![toml::Value::String(pattern)]),
    );
    table.to_string()
}
