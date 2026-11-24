//! markdownlint's configuration, as the Markdown check hands it over and the
//! rules report shows it (see [`super`]).
//!
//! The check calls markdownlint's library with the files and the
//! configuration the tool hands it, and the library reads nothing else. The
//! tool reads the repository's file itself ([`read`]): one of the names the
//! register gives markdownlint at the root, either an options file of
//! markdownlint-cli2 (`.markdownlint-cli2.jsonc` or `.yaml`), whose rules
//! stand under `config` beside the other options, or a file that holds rules
//! only (`.markdownlint.jsonc`, `.json`, `.yaml` or `.yml`). The lists below
//! follow the keys markdownlint-cli2 0.23.2 reads from an options file, the
//! version whose files the register names, and the rules of markdownlint
//! 0.41.1, the version the register pins.
//!
//! Each key of an options file is carried over ([`CARRIED`]), set aside
//! because the check decides it itself ([`SET_ASIDE`]), refused by name with
//! the reason ([`REFUSED`]), or unknown, which fails as well. Each key of the
//! rules, under `config`, in a file of rules only, and under `config` of an
//! entry of `overrides`, is a rule, an alias or a tag markdownlint knows, in
//! any case, or `default`; `$schema`, for an editor, is passed over; `extends`
//! is refused, since it names another file; any other key is unknown.
//!
//! A difference by pattern is an entry of `overrides`, as markdownlint-cli2
//! has it: its `filter` holds patterns over the path from the repository
//! root (`*` stops at a `/`, `**` does not, a leading `!` takes a match
//! back), its `config` holds rules, and its `combine` says how they are laid:
//! `merge` lays them over the file's own rules, `replace` takes their place.
//! The first entry whose `filter` matches a file counts for it. The
//! pipeline's default lies beneath either way: no entry and no file takes it
//! away. The library knows no patterns, so the tool works out the rules of
//! each file ([`Handed::rules_for`]) and hands the files in groups of the
//! same rules ([`Handed::job`]).
//!
//! The rules are laid as markdownlint-cli2 lays them, key by key: a key of the
//! file takes the value of the same key beneath it, and a new key comes after
//! those beneath. markdownlint reads a configuration's keys in order, so where
//! two keys cover the same rule, such as a tag and one of its rules, the later
//! one counts. The tool reads the keys of one file in sorted order, not in the
//! order they are written, so a file that names a rule twice, once by a tag
//! and once by its name, can count differently than an editor reads it.

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde_json::{Map, Value};

use super::locate;
use crate::ignored::Read;
use crate::names::ProgramName;
use crate::path::RelativePath;
use crate::programs::Program;
use crate::register::Register;
use crate::rules;

/// The keys of an options file the check carries over: the rules, the
/// differences by pattern, and two options that are data, the pattern of
/// front matter markdownlint skips and whether comments in a document may
/// switch rules.
pub const CARRIED: &[&str] = &["config", "frontMatter", "noInlineConfig", "overrides"];

/// The keys of an options file the check reads and does not hand on, each
/// with why: which files are linted is what `nebensound-repo files` names,
/// and what is printed is the tool's. Phase 0 holds `ignores` against the
/// declaration (see [`crate::ignored`]).
pub const SET_ASIDE: &[(&str, &str)] = &[
    ("$schema", "names the schema of the file for an editor"),
    (
        "gitignore",
        "the check lints the files `nebensound-repo files` names",
    ),
    (
        "globs",
        "the check lints the files `nebensound-repo files` names",
    ),
    (
        "ignores",
        "the check lints the files `nebensound-repo files` names, and phase 0 holds what it \
         leaves out against the declaration",
    ),
    ("noBanner", "the tool prints what the check finds"),
    ("noProgress", "the tool prints what the check finds"),
    ("showFound", "the tool prints what the check finds"),
];

/// The keys of an options file the check refuses, each with what it would
/// make the linter do: load and run code, read another file, or write the
/// files it lints.
pub const REFUSED: &[(&str, &str)] = &[
    (
        "customRules",
        "names modules of rules, which would be loaded and run, and the Markdown check runs no \
         code of the repository",
    ),
    (
        "fix",
        "makes markdownlint write the files it lints, and the Markdown check only reads them",
    ),
    (
        "markdownItPlugins",
        "names modules of the parser, which would be loaded and run, and the Markdown check runs \
         no code of the repository",
    ),
    (
        "modulePaths",
        "names folders modules would be loaded from, and the Markdown check runs no code of the \
         repository",
    ),
    (
        "outputFormatters",
        "names modules that would print the findings, which would be loaded and run, and the \
         Markdown check runs no code of the repository",
    ),
];

/// What `extends` in the rules would make markdownlint do.
const EXTENDS: &str = "names another file of configuration, and the tool reads one file";

/// How the rules of an entry of `overrides` are laid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combine {
    /// Over the rules of the file.
    Merge,
    /// In place of the rules of the file.
    Replace,
}

/// A difference by pattern: an entry of `overrides`.
#[derive(Debug, Clone)]
pub struct Override {
    /// Its patterns, as written.
    pub filter: Vec<String>,
    /// Its rules.
    pub rules: Map<String, Value>,
    /// How its rules are laid.
    pub combine: Combine,
    /// The patterns, built, a leading `!` taken off.
    set: GlobSet,
    /// For each pattern, whether it had a leading `!`.
    negated: Vec<bool>,
}

impl Override {
    /// Whether it counts for a file, a path from the repository root: a
    /// pattern without `!` matches it, or there is none, and no pattern with
    /// `!` matches it.
    #[must_use]
    pub fn matches(&self, file: &str) -> bool {
        let hits = self.set.matches(file);
        let hit = |negated: bool| {
            hits.iter()
                .any(|index| self.negated.get(*index) == Some(&negated))
        };
        (hit(false) || self.negated.iter().all(|negated| *negated)) && !hit(true)
    }
}

/// What the Markdown check hands markdownlint, beside the pipeline's default.
#[derive(Debug, Clone, Default)]
pub struct Handed {
    /// The file it was read from; [`None`] where the repository has none, or
    /// its file holds no mapping.
    pub file: Option<RelativePath>,
    /// The rules of the file.
    pub rules: Map<String, Value>,
    /// The differences by pattern, in their order.
    pub overrides: Vec<Override>,
    /// The pattern of the front matter markdownlint skips, where the file
    /// gives one.
    pub front_matter: Option<String>,
    /// Whether comments in a document are read as configuration.
    pub no_inline_config: bool,
}

/// Something in the configuration that is not handed over, which fails the
/// check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The file it is in.
    pub file: RelativePath,
    /// What is wrong, naming the key.
    pub message: String,
}

impl Handed {
    /// The rules for a file, a path from the repository root, in the order
    /// they are handed over: the default's, then the file's laid over them,
    /// then those of the first entry of `overrides` that matches the file,
    /// laid over the file's or, for `replace`, over the default's alone.
    #[must_use]
    pub fn rules_for(&self, default: &Map<String, Value>, file: &str) -> Vec<(String, Value)> {
        let mut laid: Vec<(String, Value)> = default
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let entry = self.overrides.iter().find(|entry| entry.matches(file));
        if entry.is_none_or(|entry| entry.combine == Combine::Merge) {
            lay(&mut laid, &self.rules);
        }
        if let Some(entry) = entry {
            lay(&mut laid, &entry.rules);
        }
        laid
    }

    /// What the script of the Markdown check is handed, as JSON: the files
    /// in groups of the same rules, each group with its rules, and the two
    /// options.
    #[must_use]
    pub fn job(&self, default: &Map<String, Value>, files: &[String]) -> String {
        let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
        for file in files {
            let config = object(&self.rules_for(default, file));
            match groups.iter_mut().find(|(same, _)| *same == config) {
                Some((_, members)) => members.push(file),
                None => groups.push((config, vec![file])),
            }
        }
        let groups: Vec<String> = groups
            .into_iter()
            .map(|(config, members)| {
                format!("{{\"config\":{config},\"files\":{}}}", Value::from(members))
            })
            .collect();
        format!(
            "{{\"frontMatter\":{},\"noInlineConfig\":{},\"groups\":[{}]}}",
            self.front_matter
                .as_deref()
                .map_or(Value::Null, Value::from),
            self.no_inline_config,
            groups.join(",")
        )
    }
}

/// The rules of the pipeline's default, from its text; an empty set where it
/// does not read, which the rules report names. The default compiled into
/// the tool reads: a test holds it to that.
#[must_use]
pub fn default_rules(text: &str) -> Map<String, Value> {
    match rules::parse(rules::Format::Jsonc, text) {
        Ok(Value::Object(rules)) => rules,
        _ => Map::new(),
    }
}

/// Lays rules over others, key by key, as markdownlint-cli2 does: a key
/// written beneath takes the new value in its place, a new key comes last.
fn lay(laid: &mut Vec<(String, Value)>, over: &Map<String, Value>) {
    for (key, value) in over {
        match laid.iter_mut().find(|(beneath, _)| beneath == key) {
            Some(slot) => slot.1 = value.clone(),
            None => laid.push((key.clone(), value.clone())),
        }
    }
}

/// Rules as a JSON object, its keys in their order.
fn object(rules: &[(String, Value)]) -> String {
    let entries: Vec<String> = rules
        .iter()
        .map(|(key, value)| format!("{}:{value}", Value::from(key.as_str())))
        .collect();
    format!("{{{}}}", entries.join(","))
}

/// The name markdownlint goes by in the register.
pub const NAME: &str = "markdownlint";

/// Reads markdownlint's configuration with the names the register gives its
/// files, as [`read`] does; a register without markdownlint names none, and
/// then nothing is read.
#[must_use]
pub fn read_in(
    register: &Register,
    files: &[&RelativePath],
    kept: &dyn Fn(&RelativePath) -> bool,
    source: &Read<'_>,
) -> (Handed, Vec<Problem>) {
    match register
        .program
        .iter()
        .find(|(name, _)| name.as_str() == NAME)
    {
        Some((name, program)) => read(files, kept, name, program, source),
        None => (Handed::default(), Vec::new()),
    }
}

/// Reads markdownlint's configuration, the program `name` of the register,
/// among `files`, the files of the repository from its root, through `read`:
/// what the check hands over, and every problem, each naming its file and
/// key. A problem drops what it is about and nothing else, so the check
/// still lints with the rest, and every problem fails it. `kept` says which
/// files the repository keeps in its checks (see [`super::locate`]).
#[must_use]
pub fn read(
    files: &[&RelativePath],
    kept: &dyn Fn(&RelativePath) -> bool,
    name: &ProgramName,
    program: &Program,
    read: &Read<'_>,
) -> (Handed, Vec<Problem>) {
    let located = locate(files, kept, name, program);
    let mut problems: Vec<Problem> = located
        .unread
        .into_iter()
        .map(|unread| Problem {
            file: unread.file,
            message: unread.why,
        })
        .collect();
    let mut handed = Handed::default();
    let Some(file) = located.file else {
        return (handed, problems);
    };
    let mut found = Found {
        file,
        version: &program.version,
        problems: Vec::new(),
    };
    let value = read(file).and_then(|source| rules::parse(rules::format(file), &source));
    match value {
        Ok(Value::Object(top)) => {
            handed.file = Some(file.clone());
            if file
                .file_name()
                .is_some_and(|name| name.starts_with(".markdownlint-cli2."))
            {
                options(&mut found, &mut handed, top);
            } else {
                handed.rules = found.rules(Value::Object(top), "");
            }
        }
        // An empty file of YAML holds nothing, and so lays nothing over the
        // default.
        Ok(Value::Null) => handed.file = Some(file.clone()),
        Ok(_) => found.problem("holds no mapping, so nothing of it is handed to markdownlint"),
        Err(error) => found.problem(&format!(
            "cannot be read, so nothing of it is handed to markdownlint: {error}"
        )),
    }
    problems.extend(found.problems);
    (handed, problems)
}

/// The file being read, and what is wrong in it.
struct Found<'a> {
    file: &'a RelativePath,
    version: &'a str,
    problems: Vec<Problem>,
}

impl Found<'_> {
    fn problem(&mut self, message: &str) {
        self.problems.push(Problem {
            file: self.file.clone(),
            message: message.to_owned(),
        });
    }

    fn unknown(&mut self, key: &str) {
        self.problem(&format!(
            "`{key}` is not a setting the Markdown check knows, so it is not handed to markdownlint"
        ));
    }

    /// The rules of a value, each key checked; `at` is where it stands, such
    /// as `config.`, in front of every key a problem names.
    fn rules(&mut self, value: Value, at: &str) -> Map<String, Value> {
        let Value::Object(written) = value else {
            let at = at.trim_end_matches('.');
            self.problem(&format!(
                "`{at}` is not a mapping of rules, so it is not handed to markdownlint"
            ));
            return Map::new();
        };
        let mut rules = Map::new();
        for (key, value) in written {
            if key == "$schema" {
                continue;
            }
            if key == "extends" {
                self.problem(&format!(
                    "`{at}extends` is not handed to markdownlint: it {EXTENDS}"
                ));
            } else if key.eq_ignore_ascii_case("default") || !rules::covered(&key).is_empty() {
                rules.insert(key, value);
            } else {
                self.problem(&format!(
                    "`{at}{key}` is no rule, alias or tag of markdownlint {}, so it is not handed \
                     to markdownlint",
                    self.version
                ));
            }
        }
        rules
    }

    /// An entry of `overrides`, or [`None`] where it cannot be used, which is
    /// a problem.
    fn entry(&mut self, index: usize, value: Value) -> Option<Override> {
        let at = format!("overrides[{index}]");
        let Value::Object(mut entry) = value else {
            self.problem(&format!(
                "`{at}` is not a mapping, so it is not handed to markdownlint"
            ));
            return None;
        };
        let filter = entry.remove("filter");
        let config = entry.remove("config");
        let combine = entry.remove("combine");
        for key in entry.keys() {
            self.unknown(&format!("{at}.{key}"));
        }
        let missing = [
            ("filter", &filter),
            ("config", &config),
            ("combine", &combine),
        ]
        .into_iter()
        .filter(|(_, value)| value.is_none())
        .map(|(key, _)| format!("`{key}`"))
        .collect::<Vec<_>>();
        if !missing.is_empty() {
            self.problem(&format!(
                "`{at}` has no {}, so it is not handed to markdownlint",
                missing.join(", ")
            ));
            return None;
        }
        let rules = self.rules(config.unwrap_or_default(), &format!("{at}.config."));
        let combine = match combine.unwrap_or_default().as_str() {
            Some("merge") => Combine::Merge,
            Some("replace") => Combine::Replace,
            _ => {
                self.problem(&format!(
                    "`{at}.combine` is neither `merge` nor `replace`, so the entry is not handed \
                     to markdownlint"
                ));
                return None;
            }
        };
        let filter = match patterns(filter.unwrap_or_default()) {
            Ok(filter) => filter,
            Err(reason) => {
                self.problem(&format!(
                    "`{at}.filter` {reason}, so the entry is not handed to markdownlint"
                ));
                return None;
            }
        };
        Some(Override {
            rules,
            combine,
            ..filter
        })
    }
}

/// The keys of an options file of markdownlint-cli2.
fn options(found: &mut Found<'_>, handed: &mut Handed, top: Map<String, Value>) {
    for (key, value) in top {
        match key.as_str() {
            "config" => handed.rules = found.rules(value, "config."),
            "frontMatter" => match value {
                Value::String(pattern) => handed.front_matter = Some(pattern),
                _ => {
                    found.problem("`frontMatter` is not text, so it is not handed to markdownlint")
                }
            },
            "noInlineConfig" => match value {
                Value::Bool(flag) => handed.no_inline_config = flag,
                _ => found.problem(
                    "`noInlineConfig` is neither true nor false, so it is not handed to \
                     markdownlint",
                ),
            },
            "overrides" => match value {
                Value::Array(entries) => {
                    handed.overrides = entries
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, entry)| found.entry(index, entry))
                        .collect();
                }
                _ => {
                    found.problem("`overrides` is not a list, so it is not handed to markdownlint")
                }
            },
            _ if SET_ASIDE.iter().any(|(name, _)| *name == key) => {}
            _ => match REFUSED.iter().find(|(name, _)| *name == key) {
                Some((_, why)) => {
                    found.problem(&format!("`{key}` is not handed to markdownlint: it {why}"));
                }
                None => found.unknown(&key),
            },
        }
    }
}

/// The patterns of a `filter`, built: a list of text that holds one at
/// least, each a pattern that reads.
fn patterns(value: Value) -> Result<Override, String> {
    let not = || "is not a list of patterns".to_owned();
    let written: Vec<String> = value
        .as_array()
        .ok_or_else(not)?
        .iter()
        .map(|item| item.as_str().map(str::to_owned).ok_or_else(not))
        .collect::<Result<_, _>>()?;
    if written.is_empty() {
        return Err("holds no pattern".to_owned());
    }
    let unreadable = |error: globset::Error| format!("holds a pattern that does not read: {error}");
    let mut set = GlobSetBuilder::new();
    let mut negated = Vec::new();
    for pattern in &written {
        let glob = pattern.strip_prefix('!');
        negated.push(glob.is_some());
        set.add(
            GlobBuilder::new(glob.unwrap_or(pattern))
                .literal_separator(true)
                .build()
                .map_err(unreadable)?,
        );
    }
    set.build().map_err(unreadable).map(|set| Override {
        filter: written,
        rules: Map::new(),
        combine: Combine::Merge,
        set,
        negated,
    })
}
