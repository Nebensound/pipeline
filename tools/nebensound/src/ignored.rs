//! What the tools' own settings leave out, held against the declaration.
//!
//! A building block checks exactly the files `nebensound files` names, and no
//! block reads a tool's own way of leaving a file out. Run by hand, the tools
//! still read it: a `.prettierignore` that takes out more than the declaration
//! does makes a local run pass where the pipeline fails, and leaves a file out
//! that nobody gave a reason for. So phase 0 reads these settings and reports
//! every file they take out that no `[[exclude]]` entry takes out. A setting
//! that takes out less is fine.
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
//! - lychee: `exclude_path` in `lychee.toml` at the root.
//!
//! Only a file the tool reads in a building block counts: Prettier's ignore
//! file taking out a Rust file takes out nothing Prettier would have looked at.

use std::fmt::Display;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use nebensound_declaration::Analysis;
use nebensound_declaration::annotate::Coverage;
use nebensound_declaration::finding::{Finding, Rule};
use nebensound_declaration::path::RelativePath;
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
    /// Lines in the syntax of `.gitignore`, relative to the repository root.
    Gitignore(Gitignore),
    /// Globs relative to a folder. A file below it is out when it matches one
    /// of `exclude`, or, where there is an `include`, when the tool reads it by
    /// default and it matches none of `include`.
    Globs {
        base: RelativePath,
        include: Option<Include>,
        exclude: Globs,
    },
    /// Regular expressions over the path.
    Regex(RegexSet),
}

impl Matcher {
    /// Why the file is out, or [`None`] when it is not.
    fn out(&self, file: &RelativePath) -> Option<String> {
        match self {
            Self::Gitignore(lines) => {
                match lines.matched_path_or_any_parents(file.to_string(), false) {
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
        }
    }
}

/// One setting, where it stands and what it takes out.
struct Setting {
    file: RelativePath,
    tool: Tool,
    matcher: Matcher,
}

/// An error, as the text a finding carries.
fn text(error: impl Display) -> String {
    error.to_string()
}

fn gitignore<'a>(lines: impl IntoIterator<Item = &'a str>) -> Result<Matcher, String> {
    let mut builder = GitignoreBuilder::new("");
    for line in lines {
        builder.add_line(None, line).map_err(text)?;
    }
    builder.build().map(Matcher::Gitignore).map_err(text)
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
        .map(|lines| gitignore(lines.iter().flat_map(|text| text.lines())))
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
    }
    found
}

/// The folder a file lies in.
fn folder(file: &RelativePath) -> RelativePath {
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
        Tool::Prettier => gitignore(source.lines()).map(Some),
        Tool::Taplo => taplo(&source),
        Tool::Yamllint => yamllint(file, &source, read),
        Tool::Lychee => lychee(&source),
        Tool::Markdownlint if file.file_name() == Some(".markdownlint-cli2.jsonc") => {
            markdownlint_jsonc(&source, &folder(file))
        }
        Tool::Markdownlint => markdownlint_yaml(&source, &folder(file)),
    }
}

/// Every file a tool's own setting takes out and no `[[exclude]]` entry
/// does, and every setting that could not be read.
#[must_use]
pub fn findings(analysis: &Analysis, read: &Read<'_>) -> Vec<Finding> {
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

    for (node, coverage) in nodes {
        let (Coverage::Covered(check) | Coverage::Uncovered(check)) = coverage else {
            continue;
        };
        for setting in &settings {
            if !setting.tool.checks().contains(&check.as_str()) {
                continue;
            }
            if let Some(why) = setting.matcher.out(&node.path) {
                findings.push(
                    Finding::error(
                        Rule::UnexcusedIgnore,
                        format!(
                            "{file} takes this file out of {} {why}, and no [[exclude]] entry \
                             does. The pipeline checks it all the same: give it an \
                             [[exclude]] entry with a reason in .nebensound/repo.toml, or let \
                             {file} keep it in",
                            setting.tool.name(),
                            file = setting.file,
                        ),
                    )
                    .at(node.path.clone()),
                );
            }
        }
    }
    findings
}
