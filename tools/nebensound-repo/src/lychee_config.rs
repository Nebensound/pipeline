//! lychee's configuration, as the link check hands it over and phase 0 reads
//! it.
//!
//! The link check of the Markdown and the HTML check never hands lychee the
//! repository's `lychee.toml` as it stands: lychee would start a command
//! `preprocess` names for every file it reads, end without checking on
//! `dump`, and read or write files that other keys name. So the tool reads
//! the file key by key ([`hand_over`]) and writes the configuration lychee
//! gets itself, from a positive list: every key of [`CARRIED`] is carried
//! over as a plain setting of the check, every key of [`REFUSED`] fails the
//! check by name with the reason, and so does every other key, one the tool
//! does not know. The lists follow the keys lychee 0.24.2 reads from its
//! configuration file, the version the register pins.
//!
//! `exclude_path` is not handed over: lychee matches it against the path it
//! is handed, and the link check hands it absolute paths, so the tool takes
//! out what it matches itself, matched against the path from the repository
//! root as phase 0 matches it ([`crate::ignored`]), and both build it here
//! ([`exclude_path`]). The rules report reads no lychee configuration.
//!
//! lychee also reads `.lycheeignore` from the folder it starts in, one
//! address pattern per line, and no flag stops that. The tool reads it at the
//! repository root instead ([`ignore_lines`]), adds its patterns to
//! `exclude`, and starts lychee in a folder of its own, where there is none.

use regex::RegexSet;
use toml::{Table, Value};

/// The keys the link check carries over as they are: which addresses it
/// leaves out or remaps, which answers it accepts, how a link into a folder
/// or without an extension is resolved, how requests are made, and how
/// lychee prints. The command line of the check sets `offline`,
/// `include_fragments`, `hidden`, `no_ignore`, `no_progress` and `format`
/// itself, and lychee lets its command line win over a configuration, so the
/// repository's value of these changes nothing.
pub const CARRIED: &[&str] = &[
    "accept",
    "accept_timeouts",
    "archive",
    "basic_auth",
    "cache_exclude_status",
    "default_extension",
    "exclude",
    "exclude_all_private",
    "exclude_link_local",
    "exclude_loopback",
    "exclude_private",
    "extensions",
    "fallback_extensions",
    "format",
    "github_token",
    "header",
    "hidden",
    "host_concurrency",
    "host_request_interval",
    "host_stats",
    "hosts",
    "include",
    "include_fragments",
    "include_mail",
    "include_verbatim",
    "include_wikilinks",
    "index_files",
    "insecure",
    "max_cache_age",
    "max_concurrency",
    "max_redirects",
    "max_retries",
    "method",
    "min_tls",
    "mode",
    "no_ignore",
    "no_progress",
    "offline",
    "remap",
    "require_https",
    "retry_wait_time",
    "scheme",
    "threads",
    "timeout",
    "user_agent",
    "verbose",
];

/// The key the tool applies itself, from the repository root: see the
/// module.
pub const EXCLUDE_PATH: &str = "exclude_path";

/// The keys of lychee's configuration the link check refuses, each with what
/// it would make lychee do: start a command, end or pass without checking,
/// or read or write a file the check does not hand it.
pub const REFUSED: &[(&str, &str)] = &[
    (
        "base_url",
        "makes lychee resolve the links of a file against another address, which it does not \
         check offline",
    ),
    ("cache", "makes lychee read and write a cache file"),
    (
        "cookie_jar",
        "makes lychee read and write a file of cookies",
    ),
    (
        "dump",
        "makes lychee print the links and end without checking them",
    ),
    (
        "dump_inputs",
        "makes lychee print the files and end without checking them",
    ),
    (
        "exclude_file",
        "makes lychee read the addresses it leaves out from another file",
    ),
    (
        "files_from",
        "makes lychee read the files it checks from another file",
    ),
    (
        "generate",
        "makes lychee print a page and end without checking",
    ),
    (
        "glob_ignore_case",
        "makes lychee read files whose names differ in case from the ones it is handed",
    ),
    ("output", "makes lychee write its report to another file"),
    (
        "preprocess",
        "makes lychee start a command for every file it reads, and the link check starts no \
         program of the repository",
    ),
    (
        "root_dir",
        "makes lychee resolve a link from the root of a site in another folder",
    ),
    (
        "skip_missing",
        "makes lychee pass a file it cannot read without checking it",
    ),
    (
        "suggest",
        "makes lychee ask a web archive about every broken link, a request the offline check \
         does not make",
    ),
];

/// What the link check hands lychee.
#[derive(Debug, Clone)]
pub struct Handed {
    /// The configuration lychee reads: the carried keys, and in `exclude` the
    /// patterns of `.lycheeignore` behind the repository's own.
    pub settings: Table,
    /// `exclude_path`, which the tool applies to the paths from the root.
    pub exclude_path: Option<RegexSet>,
}

/// Reads `lychee.toml` (`config`, empty where there is none) and
/// `.lycheeignore` (`ignore`, empty where there is none) into what the link
/// check hands lychee, or every reason it hands nothing: the file is no TOML,
/// or a key is refused or unknown, or a key the tool reads itself holds what
/// it cannot read. Each reason names its key.
///
/// # Errors
///
/// Every reason the configuration cannot be handed over: the refused and
/// unknown keys in their order, then what is wrong with `exclude_path` and
/// `exclude`.
pub fn hand_over(config: &str, ignore: &str) -> Result<Handed, Vec<String>> {
    let table: Table =
        toml::from_str(config).map_err(|error| vec![format!("is no TOML: {error}")])?;
    let mut reasons = Vec::new();
    let mut settings = Table::new();
    for (key, value) in &table {
        if CARRIED.contains(&key.as_str()) {
            settings.insert(key.clone(), value.clone());
        } else if let Some((_, why)) = REFUSED.iter().find(|(name, _)| name == key) {
            reasons.push(format!("`{key}` is not handed to lychee: it {why}"));
        } else if key != EXCLUDE_PATH {
            reasons.push(format!(
                "`{key}` is not a setting the link check knows, so it is not handed to lychee"
            ));
        }
    }
    let exclude_path = exclude_path(&table).unwrap_or_else(|reason| {
        reasons.push(reason);
        None
    });
    let mut exclude = match settings.remove("exclude") {
        None => Vec::new(),
        Some(value) => texts(&value, "exclude").unwrap_or_else(|reason| {
            reasons.push(reason);
            Vec::new()
        }),
    };
    exclude.extend(ignore_lines(ignore));
    if !exclude.is_empty() {
        settings.insert(
            "exclude".to_owned(),
            Value::Array(exclude.into_iter().map(Value::String).collect()),
        );
    }
    if reasons.is_empty() {
        Ok(Handed {
            settings,
            exclude_path,
        })
    } else {
        Err(reasons)
    }
}

/// `exclude_path` of a lychee configuration, as lychee builds it: a set of
/// regular expressions, [`None`] where the key is missing.
///
/// # Errors
///
/// Why it cannot be read: it is no list of text, or a pattern does not
/// compile.
pub fn exclude_path(table: &Table) -> Result<Option<RegexSet>, String> {
    table
        .get(EXCLUDE_PATH)
        .map(|value| {
            let patterns = texts(value, EXCLUDE_PATH)?;
            RegexSet::new(patterns).map_err(|error| format!("`{EXCLUDE_PATH}`: {error}"))
        })
        .transpose()
}

/// The patterns of a `.lycheeignore`, as lychee reads them: every line that
/// is not empty and does not start with `#` after white space at its start.
#[must_use]
pub fn ignore_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.is_empty() && !line.trim_start().starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// A value that is a list of text.
fn texts(value: &Value, key: &str) -> Result<Vec<String>, String> {
    let not = || format!("`{key}` is not a list of text");
    value
        .as_array()
        .ok_or_else(not)?
        .iter()
        .map(|item| item.as_str().map(str::to_owned).ok_or_else(not))
        .collect()
}
