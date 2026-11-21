//! What the tools' own settings leave out, held against the declaration.
//!
//! A building block checks exactly the files `nebensound-repo files` names, and
//! no block reads a tool's own way of leaving a file out, with one exception:
//! `check-markdown` runs lychee with the repository's `lychee.toml`, for its
//! addresses and status codes, and that file's `exclude_path` comes along. Run
//! by hand, the tools read all of them: a `.prettierignore` that takes out more
//! than the declaration does makes a local run pass where the pipeline fails,
//! and leaves a file out that nobody gave a reason for. So phase 0 reads these
//! settings and reports every file they take out that no `[[exclude]]` entry
//! takes out; for lychee, that also keeps its `exclude_path` from taking a
//! listed file out in the block. A setting that takes out less is fine.
//!
//! Read are the settings that take a whole file out of a tool:
//!
//! - Prettier: `.prettierignore` at the repository root.
//! - Taplo: `include` and `exclude` in `.taplo.toml` or `taplo.toml` at the
//!   root, whichever Taplo reads.
//! - markdownlint-cli2: `ignores` in `.markdownlint-cli2.jsonc` or
//!   `.markdownlint-cli2.yaml`, in every folder, relative to that folder.
//! - yamllint: `ignore` and `ignore-from-file` in `.yamllint`,
//!   `.yamllint.yaml` or `.yamllint.yml` at the root, whichever yamllint reads,
//!   or, where it sets neither, in the configuration its `extends` names.
//! - lychee: `exclude_path` in `lychee.toml` at the root, matched against the
//!   path from the root, the very string `check-markdown` hands lychee.
//! - rustfmt: `ignore` in `.rustfmt.toml` or `rustfmt.toml`, in every folder,
//!   relative to that folder, the hidden one first. Which of them rustfmt
//!   applies to a file depends on where it runs and on the target it formats,
//!   so every one is read. rustfmt honours `ignore` only on a nightly
//!   toolchain, which a workspace may pin.
//! - Pyright: `ignore` in `pyrightconfig.json`, and in the `[tool.pyright]`
//!   table of a `pyproject.toml`, in every folder, relative to that folder.
//!   Pyright reads the configuration of the folder it runs in, so every one
//!   is read. A file it ignores is checked and its findings are dropped.
//! - coverage.py: `omit` in `[run]` and `[report]` of `.coveragerc`, of
//!   `[coverage:run]` and `[coverage:report]` in `setup.cfg` and `tox.ini`,
//!   and of `[tool.coverage.run]` and `[tool.coverage.report]` in a
//!   `pyproject.toml`, in every folder, relative to that folder; which one
//!   coverage.py takes depends on where it runs, so every one is read.
//! - Ruff: a `# ruff: noqa` or `# flake8: noqa` line without rule codes in a
//!   Python file, which takes that file out of Ruff's linter.
//!
//! `check-python` sets Pyright's `ignore` and coverage.py's `omit` aside; Ruff
//! honours the comment wherever it runs.
//!
//! Only a file the tool reads in a building block counts: Prettier's ignore
//! file taking out a Rust file takes out nothing Prettier would have looked at.
//! Nor does a lockfile the register names: the blocks never hand a lockfile to
//! Prettier and lint with Taplo without its configuration, so taking one out
//! takes nothing out of a check. A setting that takes out a lockfile and
//! another file is reported at the other file only.

use std::fmt::Display;

use crate::Analysis;
use crate::annotate::Coverage;
use crate::finding::{Finding, Rule};
use crate::path::RelativePath;
use crate::register::Register;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use regex::RegexSet;
use serde::Deserialize;
use yaml_rust2::{Yaml, YamlLoader};

/// A tool a building block runs that has a way of its own to leave files out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// The formatter of `check-json`, `check-markdown` and `check-yaml`.
    Prettier,
    /// The formatter and linter of `check-toml`.
    Taplo,
    /// The linter of `check-markdown`.
    Markdownlint,
    /// The linter of `check-yaml`.
    Yamllint,
    /// The link checker of `check-markdown`.
    Lychee,
    /// The formatter of `check-rust`.
    Rustfmt,
    /// The type checker of `check-python`.
    Pyright,
    /// The coverage tool of `check-python`.
    Coverage,
    /// The formatter and linter of `check-python`.
    Ruff,
}

impl Tool {
    /// The name it goes by.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Prettier => "Prettier",
            Self::Taplo => "Taplo",
            Self::Markdownlint => "markdownlint-cli2",
            Self::Yamllint => "yamllint",
            Self::Lychee => "lychee",
            Self::Rustfmt => "rustfmt",
            Self::Pyright => "Pyright",
            Self::Coverage => "coverage.py",
            Self::Ruff => "Ruff",
        }
    }

    /// The checks whose files it reads in a building block.
    #[must_use]
    pub fn checks(self) -> &'static [&'static str] {
        match self {
            Self::Prettier => &["json", "markdown", "yaml"],
            Self::Taplo => &["toml"],
            Self::Markdownlint | Self::Lychee => &["markdown"],
            Self::Yamllint => &["yaml"],
            Self::Rustfmt => &["rust"],
            Self::Pyright | Self::Coverage | Self::Ruff => &["python"],
        }
    }

    /// Whether it reads this file of its checks: coverage.py measures code,
    /// so it reads no stub file (`.pyi`).
    fn reads(self, file: &RelativePath) -> bool {
        match self {
            Self::Coverage => file
                .file_name()
                .and_then(|name| std::path::Path::new(name).extension())
                .is_some_and(|extension| extension == "py"),
            _ => true,
        }
    }
}

/// Patterns as written, so that a finding can quote the one that matched.
struct Globs {
    set: GlobSet,
    written: Vec<String>,
}

/// An `include`: the files a tool reads in place of its default ones.
struct Include {
    globs: Globs,
    /// The ending of the files the tool reads without an `include`. Only such
    /// a file can an `include` leave out: one the tool never reads by default
    /// is not taken out by a setting.
    default: &'static str,
}

/// What a setting takes out.
enum Matcher {
    /// Lines in the syntax of `.gitignore`, relative to a folder.
    Gitignore {
        base: RelativePath,
        lines: Gitignore,
    },
    /// Globs relative to a folder. A file below it is out when it matches one
    /// of `exclude`, or, where there is an `include`, when the tool reads it by
    /// default and it matches none of `include`.
    Globs {
        base: RelativePath,
        include: Option<Include>,
        exclude: Globs,
    },
    /// Regular expressions over the path from the repository root, such as
    /// `docs/a.md`: the string lychee matches them against in the block.
    Regex(RegexSet),
    /// coverage.py's patterns: some over the whole path from the root, the
    /// others relative to a folder.
    Coverage {
        anywhere: Globs,
        base: RelativePath,
        here: Globs,
    },
    /// The file itself, through a line it holds.
    Itself {
        file: RelativePath,
        line: usize,
        written: String,
    },
}

impl Matcher {
    /// Why the file is out, or [`None`] when it is not.
    fn out(&self, file: &RelativePath) -> Option<String> {
        match self {
            Self::Gitignore { base, lines } => {
                if !base.contains(file) {
                    return None;
                }
                let below = file.segments()[base.segments().len()..].join("/");
                match lines.matched_path_or_any_parents(below, false) {
                    Match::Ignore(glob) => Some(format!("through the line `{}`", glob.original())),
                    Match::None | Match::Whitelist(_) => None,
                }
            }
            Self::Globs {
                base,
                include,
                exclude,
            } => {
                if !base.contains(file) {
                    return None;
                }
                let below = file.segments()[base.segments().len()..].join("/");
                if let Some(index) = exclude.set.matches(&below).first() {
                    return Some(format!("through the pattern `{}`", exclude.written[*index]));
                }
                include
                    .as_ref()
                    .filter(|include| {
                        below.ends_with(include.default) && !include.globs.set.is_match(&below)
                    })
                    .map(|_| "since it matches none of the `include` patterns".to_owned())
            }
            Self::Regex(set) => set
                .matches(&file.to_string())
                .iter()
                .next()
                .map(|index| format!("through the pattern `{}`", set.patterns()[index])),
            Self::Coverage {
                anywhere,
                base,
                here,
            } => {
                let path = file.to_string();
                if let Some(index) = anywhere.set.matches(&path).first() {
                    return Some(format!(
                        "through the pattern `{}`",
                        anywhere.written[*index]
                    ));
                }
                if !base.contains(file) {
                    return None;
                }
                let below = file.segments()[base.segments().len()..].join("/");
                here.set
                    .matches(&below)
                    .first()
                    .map(|index| format!("through the pattern `{}`", here.written[*index]))
            }
            Self::Itself {
                file: own,
                line,
                written,
            } => (own == file).then(|| format!("through `{written}` on line {line}")),
        }
    }
}

/// One setting, where it stands and what it takes out.
struct Setting {
    file: RelativePath,
    tool: Tool,
    matcher: Matcher,
}

impl Setting {
    /// Why it takes the file out of what its tool reads in a building block,
    /// or [`None`] when it does not.
    fn out(&self, file: &RelativePath, coverage: &Coverage) -> Option<String> {
        let (Coverage::Covered(check) | Coverage::Uncovered(check)) = coverage else {
            return None;
        };
        if !self.tool.checks().contains(&check.as_str()) || !self.tool.reads(file) {
            return None;
        }
        self.matcher.out(file)
    }
}

/// An error, as the text a finding carries.
fn text(error: impl Display) -> String {
    error.to_string()
}

/// `base`: the folder the lines are relative to.
fn gitignore<'a>(
    lines: impl IntoIterator<Item = &'a str>,
    base: &RelativePath,
) -> Result<Matcher, String> {
    let mut builder = GitignoreBuilder::new("");
    for line in lines {
        builder.add_line(None, line).map_err(text)?;
    }
    builder
        .build()
        .map(|lines| Matcher::Gitignore {
            base: base.clone(),
            lines,
        })
        .map_err(text)
}

/// `separator`: whether `*` stops at a `/`, as in markdownlint-cli2, or not,
/// as in Taplo.
fn globs(written: Vec<String>, separator: bool) -> Result<Globs, String> {
    let mut set = GlobSetBuilder::new();
    for pattern in &written {
        set.add(
            GlobBuilder::new(pattern)
                .literal_separator(separator)
                .build()
                .map_err(text)?,
        );
    }
    set.build().map(|set| Globs { set, written }).map_err(text)
}

#[derive(Deserialize)]
struct Taplo {
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}

fn taplo(source: &str) -> Result<Option<Matcher>, String> {
    let config: Taplo = toml::from_str(source).map_err(text)?;
    if config.include.is_none() && config.exclude.is_none() {
        return Ok(None);
    }
    Ok(Some(Matcher::Globs {
        base: RelativePath::root(),
        // Taplo reads `**/*.toml` unless `include` says otherwise, so a
        // `.lock` file can only be taken out through `exclude`.
        include: config
            .include
            .map(|written| {
                globs(written, false).map(|globs| Include {
                    globs,
                    default: ".toml",
                })
            })
            .transpose()?,
        exclude: globs(config.exclude.unwrap_or_default(), false)?,
    }))
}

#[derive(Deserialize)]
struct Rustfmt {
    ignore: Option<Vec<String>>,
}

/// rustfmt reads `ignore` as lines of `.gitignore`, relative to the folder of
/// its configuration file.
fn rustfmt(source: &str, base: &RelativePath) -> Result<Option<Matcher>, String> {
    let config: Rustfmt = toml::from_str(source).map_err(text)?;
    config
        .ignore
        .map(|lines| gitignore(lines.iter().map(String::as_str), base))
        .transpose()
}

#[derive(Deserialize)]
struct Lychee {
    exclude_path: Option<Vec<String>>,
}

fn lychee(source: &str) -> Result<Option<Matcher>, String> {
    let config: Lychee = toml::from_str(source).map_err(text)?;
    config
        .exclude_path
        .map(|patterns| RegexSet::new(patterns).map(Matcher::Regex).map_err(text))
        .transpose()
}

#[derive(Deserialize)]
struct Markdownlint {
    #[serde(default)]
    ignores: Vec<String>,
}

fn markdownlint(ignores: Vec<String>, base: &RelativePath) -> Result<Option<Matcher>, String> {
    if ignores.is_empty() {
        return Ok(None);
    }
    Ok(Some(Matcher::Globs {
        base: base.clone(),
        include: None,
        exclude: globs(ignores, true)?,
    }))
}

fn markdownlint_jsonc(source: &str, base: &RelativePath) -> Result<Option<Matcher>, String> {
    let options: Markdownlint =
        jsonc_parser::parse_to_serde_value(source, &jsonc_parser::ParseOptions::default())
            .map_err(text)?;
    markdownlint(options.ignores, base)
}

/// The first document of a YAML file; an empty file is an empty mapping.
fn yaml(source: &str) -> Result<Yaml, String> {
    let documents = YamlLoader::load_from_str(source).map_err(text)?;
    Ok(documents.into_iter().next().unwrap_or(Yaml::BadValue))
}

/// A YAML value that is a list of text, or [`None`] where the key is missing.
fn list(value: &Yaml, key: &str) -> Result<Option<Vec<String>>, String> {
    match value {
        Yaml::BadValue => Ok(None),
        Yaml::Array(items) => items
            .iter()
            .map(|item| match item {
                Yaml::String(text) => Ok(text.clone()),
                _ => Err(format!("`{key}` holds an entry that is not text")),
            })
            .collect::<Result<_, _>>()
            .map(Some),
        _ => Err(format!("`{key}` is not a list of text")),
    }
}

fn markdownlint_yaml(source: &str, base: &RelativePath) -> Result<Option<Matcher>, String> {
    let options = yaml(source)?;
    markdownlint(
        list(&options["ignores"], "ignores")?.unwrap_or_default(),
        base,
    )
}

/// Text or a list of text, as yamllint takes both `ignore` and
/// `ignore-from-file`.
fn text_or_list(value: &Yaml, key: &str) -> Result<Option<Vec<String>>, String> {
    match value {
        Yaml::String(text) => Ok(Some(vec![text.clone()])),
        other => list(other, key),
    }
}

fn yamllint(file: &RelativePath, source: &str, read: &Read<'_>) -> Result<Option<Matcher>, String> {
    let lines = yamllint_ignore(file, source, read, &mut Vec::new())?;
    lines
        .map(|lines| {
            gitignore(
                lines.iter().flat_map(|text| text.lines()),
                &RelativePath::root(),
            )
        })
        .transpose()
}

/// The configurations yamllint ships, which `extends` names without a `/`.
/// None of them sets `ignore`.
const YAMLLINT_BUILT_IN: &[&str] = &["default", "relaxed"];

/// The lines of the `ignore` yamllint takes from this configuration, or
/// [`None`] where it takes none. Where the file sets neither `ignore` nor
/// `ignore-from-file`, yamllint keeps the one of the configuration `extends`
/// names, so that one is read the same way. `seen` holds the configurations
/// already on the way, so that one extending itself is reported instead of
/// read without end.
fn yamllint_ignore(
    file: &RelativePath,
    source: &str,
    read: &Read<'_>,
    seen: &mut Vec<RelativePath>,
) -> Result<Option<Vec<String>>, String> {
    seen.push(file.clone());
    let config = yaml(source)?;
    let ignore = text_or_list(&config["ignore"], "ignore")?;
    let from_files = text_or_list(&config["ignore-from-file"], "ignore-from-file")?;
    match (ignore, from_files) {
        (Some(_), Some(_)) => {
            Err("`ignore` and `ignore-from-file` cannot stand together".to_owned())
        }
        (Some(written), None) => Ok(Some(written)),
        (None, Some(files)) => {
            let mut lines = Vec::new();
            for file in files {
                let path = RelativePath::new(&file).map_err(text)?;
                lines.push(read(&path).map_err(|error| format!("{file}: {error}"))?);
            }
            Ok(Some(lines))
        }
        (None, None) => match &config["extends"] {
            Yaml::BadValue => Ok(None),
            Yaml::String(name) if YAMLLINT_BUILT_IN.contains(&name.as_str()) => Ok(None),
            // yamllint opens any other name as a path, from where it runs: the
            // repository root.
            Yaml::String(name) => {
                let base = RelativePath::new(name).map_err(|error| format!("{name}: {error}"))?;
                if seen.contains(&base) {
                    return Err(format!("`extends` comes back round to {base}"));
                }
                let source = read(&base).map_err(|error| format!("{name}: {error}"))?;
                yamllint_ignore(&base, &source, read, seen)
                    .map_err(|error| format!("{name}: {error}"))
            }
            _ => Err("`extends` is not text".to_owned()),
        },
    }
}

#[derive(Deserialize)]
struct Pyright {
    ignore: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct PyrightTable {
    tool: Option<PyrightTool>,
}

#[derive(Deserialize)]
struct PyrightTool {
    pyright: Option<Pyright>,
}

/// Pyright reads each entry of `ignore` as a path relative to the folder of
/// its configuration, a file or a folder with everything below it, where
/// `**` stands for any folders and `*` and `?` stay within one name.
fn pyright(config: Pyright, base: &RelativePath) -> Result<Option<Matcher>, String> {
    let Some(entries) = config.ignore.filter(|entries| !entries.is_empty()) else {
        return Ok(None);
    };
    let mut set = GlobSetBuilder::new();
    for entry in &entries {
        let path = entry
            .strip_prefix("./")
            .unwrap_or(entry)
            .trim_end_matches('/');
        for pattern in [path.to_owned(), format!("{path}/**")] {
            set.add(
                GlobBuilder::new(&pattern)
                    .literal_separator(true)
                    .build()
                    .map_err(text)?,
            );
        }
    }
    // Each entry stands twice in the set, as itself and with what lies below.
    let written = entries
        .iter()
        .flat_map(|entry| [entry.clone(), entry.clone()])
        .collect();
    Ok(Some(Matcher::Globs {
        base: base.clone(),
        include: None,
        exclude: Globs {
            set: set.build().map_err(text)?,
            written,
        },
    }))
}

fn pyright_json(source: &str, base: &RelativePath) -> Result<Option<Matcher>, String> {
    let config: Option<Pyright> =
        jsonc_parser::parse_to_serde_value(source, &jsonc_parser::ParseOptions::default())
            .map_err(text)?;
    config.map_or(Ok(None), |config| pyright(config, base))
}

fn pyright_toml(source: &str, base: &RelativePath) -> Result<Option<Matcher>, String> {
    let table: PyrightTable = toml::from_str(source).map_err(text)?;
    match table.tool.and_then(|tool| tool.pyright) {
        Some(config) => pyright(config, base),
        None => Ok(None),
    }
}

/// coverage.py's patterns, as `globset` reads them. A pattern without a `/`
/// names a file anywhere. coverage.py matches the others against the
/// absolute path of a file, from its start: one that starts with a wildcard
/// as it is, so it names a file only where it starts with `*/` or `**/`,
/// any folders above; any other one relative to the folder of its
/// configuration, where coverage.py runs, and an absolute one names no file
/// of the repository. `*` stays within one name, `**/` stands for any
/// folders and a trailing `/*` for anything below.
fn coverage(patterns: Vec<String>, base: &RelativePath) -> Result<Option<Matcher>, String> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut anywhere = Vec::new();
    let mut here = Vec::new();
    for written in patterns {
        let pattern = written.replace('\\', "/");
        let pattern = pattern.strip_prefix("./").unwrap_or(&pattern).to_owned();
        let pattern = match pattern.strip_suffix("/*") {
            Some(stem) => format!("{stem}/**"),
            None => pattern,
        };
        if !pattern.contains('/') {
            anywhere.push((format!("**/{pattern}"), written));
        } else if pattern.starts_with(['*', '?']) {
            // Matched against an absolute path from its start, such a
            // pattern names a file only where its wildcards stand for the
            // folders above it.
            let stars = pattern.len() - pattern.trim_start_matches('*').len();
            if let Some(rest) = pattern[stars..].strip_prefix('/').filter(|_| stars > 0) {
                anywhere.push((format!("**/{rest}"), written));
            }
        } else if !pattern.starts_with('/') {
            here.push((pattern, written));
        }
    }
    let build = |patterns: Vec<(String, String)>| -> Result<Globs, String> {
        let mut set = GlobSetBuilder::new();
        let mut written = Vec::new();
        for (pattern, original) in patterns {
            set.add(
                GlobBuilder::new(&pattern)
                    .literal_separator(true)
                    .build()
                    .map_err(text)?,
            );
            written.push(original);
        }
        set.build().map(|set| Globs { set, written }).map_err(text)
    };
    Ok(Some(Matcher::Coverage {
        anywhere: build(anywhere)?,
        base: base.clone(),
        here: build(here)?,
    }))
}

#[derive(Deserialize)]
struct CoverageTable {
    tool: Option<CoverageTool>,
}

#[derive(Deserialize)]
struct CoverageTool {
    coverage: Option<CoverageSections>,
}

#[derive(Deserialize)]
struct CoverageSections {
    run: Option<CoverageOmit>,
    report: Option<CoverageOmit>,
}

#[derive(Deserialize)]
struct CoverageOmit {
    omit: Option<Vec<String>>,
}

fn coverage_toml(source: &str, base: &RelativePath) -> Result<Option<Matcher>, String> {
    let table: CoverageTable = toml::from_str(source).map_err(text)?;
    let sections = table.tool.and_then(|tool| tool.coverage);
    let patterns = sections
        .into_iter()
        .flat_map(|sections| [sections.run, sections.report])
        .flatten()
        .filter_map(|section| section.omit)
        .flatten()
        .collect();
    coverage(patterns, base)
}

/// `omit` of the named sections of an INI file, as coverage.py reads it with
/// Python's configparser: a value runs on over the indented lines below its
/// key, and its patterns stand one per line or apart by commas.
fn coverage_ini(
    source: &str,
    sections: &[&str],
    base: &RelativePath,
) -> Result<Option<Matcher>, String> {
    let mut patterns = Vec::new();
    let mut section: Option<String> = None;
    let mut collecting = false;
    for (number, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if collecting {
                patterns.extend(split(trimmed));
            }
            continue;
        }
        collecting = false;
        if let Some(name) = trimmed.strip_prefix('[') {
            let Some(name) = name.strip_suffix(']') else {
                return Err(format!(
                    "line {}: a section header that does not close",
                    number + 1
                ));
            };
            section = Some(name.trim().to_owned());
            continue;
        }
        let Some((key, value)) = trimmed.split_once(['=', ':']) else {
            return Err(format!(
                "line {}: neither a section, a key nor a value",
                number + 1
            ));
        };
        if key.trim().eq_ignore_ascii_case("omit")
            && section
                .as_deref()
                .is_some_and(|name| sections.contains(&name))
        {
            patterns.extend(split(value.trim()));
            collecting = true;
        }
    }
    coverage(patterns, base)
}

/// The patterns of one line of a list in an INI file.
fn split(line: &str) -> Vec<String> {
    line.split(',')
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The line of a Python file that takes it out of Ruff's linter as a whole:
/// `# ruff: noqa` or `# flake8: noqa` without rule codes.
fn noqa(file: &RelativePath, source: &str) -> Option<Matcher> {
    source.lines().enumerate().find_map(|(index, line)| {
        let comment = line.trim().strip_prefix('#')?.trim();
        let (tool, rest) = comment.split_once(':')?;
        let tool = tool.trim().to_ascii_lowercase();
        (["ruff", "flake8"].contains(&tool.as_str()) && rest.trim().eq_ignore_ascii_case("noqa"))
            .then(|| Matcher::Itself {
                file: file.clone(),
                line: index + 1,
                written: line.trim().to_owned(),
            })
    })
}

/// Reads a file of the repository, as it stands there.
pub type Read<'a> = dyn Fn(&RelativePath) -> Result<String, String> + 'a;

/// The first of these names the repository holds at its root.
fn first<'a>(files: &[&'a RelativePath], names: &[&str]) -> Option<&'a RelativePath> {
    names.iter().find_map(|name| {
        files
            .iter()
            .find(|file| file.segments().len() == 1 && file.file_name() == Some(name))
            .copied()
    })
}

/// Where the settings stand, with the tool each is for and how it is read.
fn places<'a>(files: &[&'a RelativePath]) -> Vec<(&'a RelativePath, Tool)> {
    let mut found = Vec::new();
    for (names, tool) in [
        (&[".prettierignore"][..], Tool::Prettier),
        (&[".taplo.toml", "taplo.toml"][..], Tool::Taplo),
        (
            &[".yamllint", ".yamllint.yaml", ".yamllint.yml"][..],
            Tool::Yamllint,
        ),
        (&["lychee.toml"][..], Tool::Lychee),
    ] {
        if let Some(file) = first(files, names) {
            found.push((file, tool));
        }
    }
    // markdownlint-cli2 reads one options file in every folder, the JSONC one
    // before the YAML one.
    for file in files {
        let jsonc = file.file_name() == Some(".markdownlint-cli2.jsonc");
        let yaml = file.file_name() == Some(".markdownlint-cli2.yaml")
            && !files.contains(&&folder(file).join(".markdownlint-cli2.jsonc"));
        if jsonc || yaml {
            found.push((*file, Tool::Markdownlint));
        }
        // rustfmt as well, the hidden file before the plain one.
        let hidden = file.file_name() == Some(".rustfmt.toml");
        let plain = file.file_name() == Some("rustfmt.toml")
            && !files.contains(&&folder(file).join(".rustfmt.toml"));
        if hidden || plain {
            found.push((*file, Tool::Rustfmt));
        }
        // Pyright and coverage.py, every file they may read their settings
        // from.
        match file.file_name() {
            Some("pyrightconfig.json") => found.push((*file, Tool::Pyright)),
            Some("pyproject.toml") => {
                found.push((*file, Tool::Pyright));
                found.push((*file, Tool::Coverage));
            }
            Some(".coveragerc" | "setup.cfg" | "tox.ini") => found.push((*file, Tool::Coverage)),
            _ => {}
        }
    }
    found
}

/// The folder a file lies in.
pub(crate) fn folder(file: &RelativePath) -> RelativePath {
    let segments = file.segments();
    segments[..segments.len() - 1]
        .iter()
        .fold(RelativePath::root(), |path, segment| path.join(segment))
}

fn read_setting(
    file: &RelativePath,
    tool: Tool,
    read: &Read<'_>,
) -> Result<Option<Matcher>, String> {
    let source = read(file)?;
    match tool {
        Tool::Prettier => gitignore(source.lines(), &RelativePath::root()).map(Some),
        Tool::Rustfmt => rustfmt(&source, &folder(file)),
        Tool::Taplo => taplo(&source),
        Tool::Yamllint => yamllint(file, &source, read),
        Tool::Lychee => lychee(&source),
        Tool::Markdownlint if file.file_name() == Some(".markdownlint-cli2.jsonc") => {
            markdownlint_jsonc(&source, &folder(file))
        }
        Tool::Markdownlint => markdownlint_yaml(&source, &folder(file)),
        Tool::Pyright if file.file_name() == Some("pyrightconfig.json") => {
            pyright_json(&source, &folder(file))
        }
        Tool::Pyright => pyright_toml(&source, &folder(file)),
        Tool::Coverage if file.file_name() == Some("pyproject.toml") => {
            coverage_toml(&source, &folder(file))
        }
        Tool::Coverage if file.file_name() == Some(".coveragerc") => {
            coverage_ini(&source, &["run", "report"], &folder(file))
        }
        Tool::Coverage => {
            coverage_ini(&source, &["coverage:run", "coverage:report"], &folder(file))
        }
        Tool::Ruff => Ok(noqa(file, &source)),
    }
}

/// What takes a file out instead of the tool's own setting.
fn remedy(tool: Tool) -> &'static str {
    match tool {
        // A single file with code cannot be excluded, only its whole package.
        Tool::Rustfmt => {
            "mark its code #[rustfmt::skip], exclude its whole package (the folder \
             with its Cargo.toml) with an [[exclude]] entry and a reason in \
             .nebensound/repo.toml"
        }
        // Single rules stay the repository's choice.
        Tool::Ruff => {
            "switch single rules off (`# ruff: noqa: <code>`), or give it an [[exclude]] entry \
             with a reason in .nebensound/repo.toml"
        }
        Tool::Prettier
        | Tool::Taplo
        | Tool::Markdownlint
        | Tool::Yamllint
        | Tool::Lychee
        | Tool::Pyright
        | Tool::Coverage => "give it an [[exclude]] entry with a reason in .nebensound/repo.toml",
    }
}

/// Every file a tool's own setting takes out and no `[[exclude]]` entry
/// does, and every setting that could not be read. A lockfile `register`
/// names is never one of those files.
#[must_use]
pub fn findings(analysis: &Analysis, register: &Register, read: &Read<'_>) -> Vec<Finding> {
    let nodes = analysis.tree.files();
    let files: Vec<&RelativePath> = nodes.iter().map(|(node, _)| &node.path).collect();

    let mut findings = Vec::new();
    let mut settings = Vec::new();
    for (file, tool) in places(&files) {
        match read_setting(file, tool, read) {
            Ok(Some(matcher)) => settings.push(Setting {
                file: file.clone(),
                tool,
                matcher,
            }),
            Ok(None) => {}
            Err(error) => findings.push(
                Finding::error(
                    Rule::UnreadableIgnoreFile,
                    format!(
                        "phase 0 cannot read what this takes out of {}, so it cannot hold it \
                         against the declaration: {error}",
                        tool.name()
                    ),
                )
                .at(file.clone()),
            ),
        }
    }
    // A Python file that takes itself out of Ruff. Only the files a check
    // reads are opened; one that cannot be read holds no such line.
    for (node, coverage) in &nodes {
        let python = matches!(coverage, Coverage::Covered(check) | Coverage::Uncovered(check)
            if Tool::Ruff.checks().contains(&check.as_str()));
        if let Some(matcher) = python
            .then(|| read(&node.path).ok())
            .flatten()
            .and_then(|source| noqa(&node.path, &source))
        {
            settings.push(Setting {
                file: node.path.clone(),
                tool: Tool::Ruff,
                matcher,
            });
        }
    }

    for (node, coverage) in nodes {
        // No building block hands a lockfile to Prettier, and Taplo lints
        // without its configuration, so taking one out takes nothing out of a
        // check.
        if register.is_lockfile(&node.path.to_string()) {
            continue;
        }
        for setting in &settings {
            if let Some(why) = setting.out(&node.path, coverage) {
                let message = if setting.tool == Tool::Ruff {
                    // Ruff honours the line in the block as well.
                    format!(
                        "this file takes itself out of Ruff's linter {why}, and no [[exclude]] \
                         entry does; Ruff leaves it unchecked wherever it runs: {remedy}",
                        remedy = remedy(setting.tool),
                    )
                } else {
                    format!(
                        "{file} takes this file out of {} {why}, and no [[exclude]] entry \
                         does. The pipeline checks it all the same: {remedy}, or let \
                         {file} keep it in",
                        setting.tool.name(),
                        file = setting.file,
                        remedy = remedy(setting.tool),
                    )
                };
                findings.push(Finding::error(Rule::UnexcusedIgnore, message).at(node.path.clone()));
            }
        }
    }
    findings
}
