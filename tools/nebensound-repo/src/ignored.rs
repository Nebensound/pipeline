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
//!   path from the root, the very string `check-markdown` and `check-html`
//!   hand lychee.
//! - Stylelint: `.stylelintignore` at the root, and `ignoreFiles` in the
//!   configuration Stylelint reads in a folder (the first of the names it
//!   looks for, `package.json` only where it holds a `stylelint` key),
//!   relative to that folder, for the files below it that no nearer
//!   configuration governs. A configuration in JavaScript is not read, since
//!   phase 0 runs no code of the repository; it still governs its folder.
//! - html-validate: `.htmlvalidateignore` in every folder, relative to that
//!   folder.
//! - rustfmt: `ignore` in `.rustfmt.toml` or `rustfmt.toml`, in every folder,
//!   relative to that folder, the hidden one first. Which of them rustfmt
//!   applies to a file depends on where it runs and on the target it formats,
//!   so every one is read. rustfmt honours `ignore` only on a nightly
//!   toolchain, which a workspace may pin.
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
use globset::{GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use regex::RegexSet;
use serde::Deserialize;
use yaml_rust2::{Yaml, YamlLoader};

/// A tool a building block runs that has a way of its own to leave files out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// The formatter of `check-css`, `check-html`, `check-json`,
    /// `check-markdown` and `check-yaml`.
    Prettier,
    /// The formatter and linter of `check-toml`.
    Taplo,
    /// The linter of `check-markdown`.
    Markdownlint,
    /// The linter of `check-yaml`.
    Yamllint,
    /// The link checker of `check-html` and `check-markdown`.
    Lychee,
    /// The formatter of `check-rust`.
    Rustfmt,
    /// The linter of `check-css`.
    Stylelint,
    /// The linter of `check-html`.
    HtmlValidate,
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
            Self::Stylelint => "Stylelint",
            Self::HtmlValidate => "html-validate",
        }
    }

    /// The checks whose files it reads in a building block.
    #[must_use]
    pub fn checks(self) -> &'static [&'static str] {
        match self {
            Self::Prettier => &["css", "html", "json", "markdown", "yaml"],
            Self::Taplo => &["toml"],
            Self::Markdownlint => &["markdown"],
            Self::Lychee => &["html", "markdown"],
            Self::Yamllint => &["yaml"],
            Self::Rustfmt => &["rust"],
            Self::Stylelint => &["css"],
            Self::HtmlValidate => &["html"],
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
    /// Stylelint's `ignoreFiles`: globs relative to the folder of its
    /// configuration, read in order, a negated one (`!…`) taking back what
    /// the ones before it matched, as Stylelint reads them. They count for
    /// the files below that folder that no configuration in a folder nearer
    /// to them governs.
    Ordered {
        base: RelativePath,
        globs: Vec<(bool, GlobMatcher, String)>,
        nearer: Vec<RelativePath>,
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
            Self::Ordered {
                base,
                globs,
                nearer,
            } => {
                if !base.contains(file) || nearer.iter().any(|dir| dir.contains(file)) {
                    return None;
                }
                let below = file.segments()[base.segments().len()..].join("/");
                // A list of negated globs alone takes out every file that
                // none of them names.
                let mut why = globs.iter().all(|(negated, _, _)| *negated).then(|| {
                    let written: Vec<String> = globs
                        .iter()
                        .map(|(_, _, written)| format!("`{written}`"))
                        .collect();
                    format!(
                        "since it matches none of the patterns {}",
                        written.join(", ")
                    )
                });
                for (negated, glob, written) in globs {
                    if *negated {
                        if glob.is_match(&below) {
                            why = None;
                        }
                    } else if why.is_none() && glob.is_match(&below) {
                        why = Some(format!("through the pattern `{written}`"));
                    }
                }
                why
            }
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
        if !self.tool.checks().contains(&check.as_str()) {
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

/// The names Stylelint looks for its configuration under in a folder, in its
/// order, as its `cosmiconfig` searches them.
const STYLELINT_CONFIGS: &[&str] = &[
    "package.json",
    ".stylelintrc",
    ".stylelintrc.json",
    ".stylelintrc.yaml",
    ".stylelintrc.yml",
    ".stylelintrc.js",
    ".stylelintrc.ts",
    ".stylelintrc.cjs",
    ".stylelintrc.mjs",
    ".config/stylelintrc",
    ".config/stylelintrc.json",
    ".config/stylelintrc.yaml",
    ".config/stylelintrc.yml",
    ".config/stylelintrc.js",
    ".config/stylelintrc.ts",
    ".config/stylelintrc.cjs",
    ".config/stylelintrc.mjs",
    "stylelint.config.js",
    "stylelint.config.ts",
    "stylelint.config.cjs",
    "stylelint.config.mjs",
];

/// Whether a configuration of Stylelint is written in JavaScript or
/// TypeScript, which phase 0 does not run.
fn script(file: &RelativePath) -> bool {
    let name = file.file_name().unwrap_or_default();
    [".js", ".ts", ".cjs", ".mjs"]
        .iter()
        .any(|ending| name.ends_with(ending))
}

/// A configuration of Stylelint: its file and, unless it is JavaScript, what
/// it holds.
type StylelintConfig<'a> = (&'a RelativePath, Option<Yaml>);

/// The configuration of Stylelint a folder holds, as Stylelint finds it: the
/// first of its names there, `package.json` only where it has a `stylelint`
/// key. With it, the configuration as YAML, or [`None`] for one in
/// JavaScript; JSON reads as YAML.
fn stylelint_config<'a>(
    files: &[&'a RelativePath],
    dir: &RelativePath,
    read: &Read<'_>,
) -> Result<Option<StylelintConfig<'a>>, (&'a RelativePath, String)> {
    for name in STYLELINT_CONFIGS {
        let candidate = name
            .split('/')
            .fold(dir.clone(), |path, segment| path.join(segment));
        let Some(file) = files.iter().find(|file| ***file == candidate).copied() else {
            continue;
        };
        if script(file) {
            return Ok(Some((file, None)));
        }
        let config = read(file)
            .and_then(|source| yaml(&source))
            .map_err(|error| (file, error))?;
        if *name != "package.json" {
            return Ok(Some((file, Some(config))));
        }
        if let Yaml::Hash(_) = &config["stylelint"] {
            return Ok(Some((file, Some(config["stylelint"].clone()))));
        }
    }
    Ok(None)
}

/// What `ignoreFiles` of a configuration of Stylelint takes out, read as
/// Stylelint reads it: text or a list of text, `*` stopping at a slash.
fn stylelint(
    config: &Yaml,
    base: &RelativePath,
    nearer: Vec<RelativePath>,
) -> Result<Option<Matcher>, String> {
    let Some(written) = text_or_list(&config["ignoreFiles"], "ignoreFiles")? else {
        return Ok(None);
    };
    let globs = written
        .into_iter()
        .map(|pattern| {
            let (negated, glob) = match pattern.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, pattern.as_str()),
            };
            GlobBuilder::new(glob)
                .literal_separator(true)
                .build()
                .map(|built| (negated, built.compile_matcher(), pattern.clone()))
                .map_err(text)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if globs.is_empty() {
        return Ok(None);
    }
    Ok(Some(Matcher::Ordered {
        base: base.clone(),
        globs,
        nearer,
    }))
}

/// The `ignoreFiles` of every configuration of Stylelint, each counting for
/// the files below its folder that no nearer configuration governs, and each
/// configuration that could not be read.
fn stylelint_settings(
    files: &[&RelativePath],
    read: &Read<'_>,
) -> (Vec<Setting>, Vec<(RelativePath, String)>) {
    let folders: std::collections::BTreeSet<RelativePath> = files
        .iter()
        .filter(|file| {
            STYLELINT_CONFIGS.iter().any(|name| {
                let segments = name.split('/').count();
                file.segments().len() >= segments
                    && file.segments()[file.segments().len() - segments..].join("/") == *name
            })
        })
        .map(|file| {
            let segments = file.segments();
            let depth = if segments.len() >= 2 && segments[segments.len() - 2] == ".config" {
                2
            } else {
                1
            };
            segments[..segments.len() - depth]
                .iter()
                .fold(RelativePath::root(), |path, segment| path.join(segment))
        })
        .collect();
    let mut configs = Vec::new();
    let mut failures = Vec::new();
    for dir in folders {
        match stylelint_config(files, &dir, read) {
            Ok(Some((file, config))) => configs.push((dir, file.clone(), config)),
            Ok(None) => {}
            Err((file, error)) => failures.push((file.clone(), error)),
        }
    }
    let mut settings = Vec::new();
    for (dir, file, config) in &configs {
        let Some(config) = config else {
            continue;
        };
        let nearer = configs
            .iter()
            .map(|(other, _, _)| other)
            .filter(|other| *other != dir && dir.contains(other))
            .cloned()
            .collect();
        match stylelint(config, dir, nearer) {
            Ok(Some(matcher)) => settings.push(Setting {
                file: file.clone(),
                tool: Tool::Stylelint,
                matcher,
            }),
            Ok(None) => {}
            Err(error) => failures.push((file.clone(), error)),
        }
    }
    (settings, failures)
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
        (&[".stylelintignore"][..], Tool::Stylelint),
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
        // html-validate reads the ignore file of every folder above a file.
        if file.file_name() == Some(".htmlvalidateignore") {
            found.push((*file, Tool::HtmlValidate));
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
        Tool::Stylelint | Tool::HtmlValidate => gitignore(source.lines(), &folder(file)).map(Some),
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
        Tool::Prettier
        | Tool::Taplo
        | Tool::Markdownlint
        | Tool::Yamllint
        | Tool::Lychee
        | Tool::Stylelint
        | Tool::HtmlValidate => {
            "give it an [[exclude]] entry with a reason in .nebensound/repo.toml"
        }
    }
}

/// Every file a tool's own setting takes out and no `[[exclude]]` entry
/// does, and every setting that could not be read. A lockfile `register`
/// names is never one of those files.
#[must_use]
pub fn findings(analysis: &Analysis, register: &Register, read: &Read<'_>) -> Vec<Finding> {
    let nodes = analysis.tree.files();
    let files: Vec<&RelativePath> = nodes.iter().map(|(node, _)| &node.path).collect();

    let unreadable = |file: &RelativePath, tool: Tool, error: String| {
        Finding::error(
            Rule::UnreadableIgnoreFile,
            format!(
                "phase 0 cannot read what this takes out of {}, so it cannot hold it against \
                 the declaration: {error}",
                tool.name()
            ),
        )
        .at(file.clone())
    };
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
            Err(error) => findings.push(unreadable(file, tool, error)),
        }
    }
    let (configured, failures) = stylelint_settings(&files, read);
    settings.extend(configured);
    findings.extend(
        failures
            .into_iter()
            .map(|(file, error)| unreadable(&file, Tool::Stylelint, error)),
    );

    for (node, coverage) in nodes {
        // No building block hands a lockfile to Prettier, and Taplo lints
        // without its configuration, so taking one out takes nothing out of a
        // check.
        if register.is_lockfile(&node.path.to_string()) {
            continue;
        }
        for setting in &settings {
            if let Some(why) = setting.out(&node.path, coverage) {
                findings.push(
                    Finding::error(
                        Rule::UnexcusedIgnore,
                        format!(
                            "{file} takes this file out of {} {why}, and no [[exclude]] entry \
                             does. The pipeline checks it all the same: {remedy}, or let \
                             {file} keep it in",
                            setting.tool.name(),
                            file = setting.file,
                            remedy = remedy(setting.tool),
                        ),
                    )
                    .at(node.path.clone()),
                );
            }
        }
    }
    findings
}
