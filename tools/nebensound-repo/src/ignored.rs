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
//! - Pyright: `include`, `ignore` and `exclude` in `pyrightconfig.json`, and
//!   in the `[tool.pyright]` table of a `pyproject.toml`, in every folder,
//!   relative to that folder. Pyright reads the configuration of the folder it
//!   runs in, so every one is read. A file it ignores is checked and its
//!   findings are dropped. Without `include` it reads everything below the
//!   folder. The entries of Pyright's default list for `exclude` are no
//!   finding.
//! - coverage.py: `omit` in `[coverage:run]` and `[coverage:report]` of
//!   `.coveragerc`, or of `[run]` and `[report]` there where the prefixed
//!   section does not set it; of `[tool.coverage.run]` and
//!   `[tool.coverage.report]` in `.coveragerc.toml`, or of `[run]` and
//!   `[report]` there where the prefixed table is missing; of `[coverage:run]`
//!   and `[coverage:report]` in `setup.cfg` and `tox.ini`; and of
//!   `[tool.coverage.run]` and `[tool.coverage.report]` in a `pyproject.toml`;
//!   in every folder, relative to that folder. coverage.py takes the first of
//!   these, in this order, that exists, where `setup.cfg`, `tox.ini` and
//!   `pyproject.toml` count only when they hold settings of its own; which
//!   one that is depends on where it runs, so every one is read. In the INI
//!   files, an `omit` under `[DEFAULT]` counts as set in every section the
//!   file has, as Python's configparser hands it on.
//! - Ruff: a `# ruff: noqa` or `# flake8: noqa` line without rule codes in a
//!   Python file, which takes that file out of Ruff's linter; and `exclude`
//!   and `extend-exclude` in `.ruff.toml`, `ruff.toml` or the `[tool.ruff]`
//!   table of a `pyproject.toml`, relative to that folder, for what lies below
//!   it that no nearer configuration governs; `include`, `lint.exclude` and
//!   `format.exclude` in the one that governs a file. What a configuration's
//!   `extend` chain names inside the repository is laid beneath it, as Ruff
//!   does. `exclude` takes the place of Ruff's default list, whose entries
//!   are no finding; `extend-include` only adds to `include`.
//!
//! `check-python` hands Ruff and Pyright the list, sets Pyright's `ignore`
//! and `exclude`, Ruff's exclusions and coverage.py's `omit` aside; Ruff
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
use crate::ruff_config::{self, Next};
use globset::{GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use regex::RegexSet;
use serde::Deserialize;
use toml::Value;
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
            Self::Stylelint => "Stylelint",
            Self::HtmlValidate => "html-validate",
            Self::Pyright => "Pyright",
            Self::Coverage => "coverage.py",
            Self::Ruff => "Ruff",
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
    /// coverage.py's patterns: some over the whole path from the root, the
    /// others relative to a folder.
    Coverage {
        anywhere: Globs,
        base: RelativePath,
        here: Globs,
    },
    /// Ruff's `exclude` and `extend-exclude`.
    Ruff(RuffExclude),
    /// Ruff's `include`, `lint.exclude` or `format.exclude`.
    RuffFile(RuffFile),
    /// Several settings of one file, the first that takes the file out
    /// counting.
    Either(Vec<Matcher>),
    /// What a setting takes out, less what the tool leaves out by default.
    Beyond(Beyond),
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
                    .map(|include| none_of(&include.globs.written))
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
            Self::Ruff(ruff) => ruff.out(file),
            Self::RuffFile(ruff) => ruff.out(file),
            Self::Either(matchers) => matchers.iter().find_map(|matcher| matcher.out(file)),
            Self::Beyond(beyond) => beyond.out(file),
            Self::Itself {
                file: own,
                line,
                written,
            } => (own == file).then(|| format!("through `{written}` on line {line}")),
        }
    }
}

/// Why a file matches none of the patterns of an `include`.
fn none_of(written: &[String]) -> String {
    let mut quoted: Vec<String> = Vec::new();
    for pattern in written {
        let pattern = format!("`{pattern}`");
        if !quoted.contains(&pattern) {
            quoted.push(pattern);
        }
    }
    if quoted.is_empty() {
        "since its `include` names no pattern".to_owned()
    } else {
        format!(
            "since it matches none of the `include` patterns {}",
            quoted.join(", ")
        )
    }
}

/// A pattern of Ruff's: a glob relative to the folder of the file that writes
/// it, `*` crossing a `/`, as Ruff joins it to that folder into an absolute
/// glob. One without a `/` Ruff also matches as it stands, against a name or
/// against the whole path.
struct RuffPattern {
    /// The folder of the file that writes it, gone up by each `../` the
    /// pattern starts with, or [`None`] where that leaves the repository.
    base: Option<RelativePath>,
    glob: GlobMatcher,
    /// Whether it holds no `/`.
    bare: bool,
    written: String,
    /// The file that writes it, where that is not the configuration Ruff
    /// finds but a file its `extend` chain names.
    from: Option<RelativePath>,
}

impl RuffPattern {
    /// Whether it matches this path, as a glob relative to its folder.
    fn below(&self, path: &[String]) -> bool {
        let Some(base) = &self.base else {
            return false;
        };
        let base = base.segments();
        path.len() > base.len()
            && path.starts_with(base)
            && self.glob.is_match(path[base.len()..].join("/"))
    }

    /// Whether it matches the file through the whole path: relative to its
    /// folder, or, without a `/`, against the path from the root, which
    /// stands for the absolute path Ruff matches it against.
    fn whole(&self, file: &RelativePath) -> bool {
        self.below(file.segments()) || (self.bare && self.glob.is_match(format!("/{file}")))
    }

    /// The pattern as a finding names it.
    fn quoted(&self) -> String {
        match &self.from {
            None => format!("the pattern `{}`", self.written),
            Some(from) => format!(
                "the pattern `{}` that it takes from {from} through `extend`",
                self.written
            ),
        }
    }
}

/// Ruff's `exclude` and `extend-exclude`: patterns matched against the file
/// and every folder above it on the way down from the folder of the
/// configuration, as Ruff checks each while it walks. A pattern without a
/// `/` is matched against the name as well. Each of them counts where this
/// configuration governs: below its folder and not below a nearer one, whose
/// folder itself it still checks.
struct RuffExclude {
    base: RelativePath,
    nearer: Vec<RelativePath>,
    patterns: Vec<RuffPattern>,
}

impl RuffExclude {
    /// Why the file is out, or [`None`] when it is not.
    fn out(&self, file: &RelativePath) -> Option<String> {
        let Self {
            base,
            nearer,
            patterns,
        } = self;
        if !base.contains(file) {
            return None;
        }
        let segments = file.segments();
        let start = base.segments().len();
        (start + 1..=segments.len()).find_map(|depth| {
            let path = &segments[..depth];
            // A nearer configuration governs what lies below its folder.
            if nearer
                .iter()
                .any(|dir| dir.segments().len() < depth && path.starts_with(dir.segments()))
            {
                return None;
            }
            let name = &path[depth - 1];
            patterns
                .iter()
                .find(|pattern| {
                    pattern.below(path) || (pattern.bare && pattern.glob.is_match(name))
                })
                .map(|pattern| format!("through {}", pattern.quoted()))
        })
    }
}

/// A setting of Ruff that it reads for each file it finds, in the one
/// configuration that governs the file: the nearest one above it.
struct RuffFile {
    base: RelativePath,
    nearer: Vec<RelativePath>,
    kind: RuffKind,
}

/// What such a setting is.
enum RuffKind {
    /// `include`, beside the `extend-include` that adds to it. Ruff matches
    /// both against the file's whole path. Its default takes every `.py` and
    /// `.pyi` file, all the check for Python covers.
    Include {
        include: Vec<RuffPattern>,
        extend: Vec<RuffPattern>,
    },
    /// `lint.exclude` or `format.exclude`, which takes a file out of one
    /// step of Ruff. Ruff matches it against the file's whole path and its
    /// name, not against the folders above it.
    Step {
        patterns: Vec<RuffPattern>,
        /// The setting, such as `lint.exclude`.
        key: &'static str,
        /// The command that skips the file, and the one that still reads it.
        skips: &'static str,
        reads: &'static str,
    },
}

impl RuffFile {
    /// Why the file is out, or [`None`] when it is not.
    fn out(&self, file: &RelativePath) -> Option<String> {
        if !self.base.contains(file) || self.nearer.iter().any(|dir| dir.contains(file)) {
            return None;
        }
        match &self.kind {
            RuffKind::Include { include, extend } => {
                if include
                    .iter()
                    .chain(extend)
                    .any(|pattern| pattern.whole(file))
                {
                    return None;
                }
                let written: Vec<String> = include
                    .iter()
                    .map(|pattern| pattern.written.clone())
                    .collect();
                let from = include
                    .first()
                    .and_then(|pattern| pattern.from.as_ref())
                    .map(|from| format!(" that it takes from {from} through `extend`"))
                    .unwrap_or_default();
                let added = if extend.is_empty() {
                    ""
                } else {
                    ", nor any of `extend-include`"
                };
                Some(format!("{}{from}{added}", none_of(&written)))
            }
            RuffKind::Step {
                patterns,
                key,
                skips,
                reads,
            } => {
                let name = file.file_name().unwrap_or_default();
                patterns
                    .iter()
                    .find(|pattern| {
                        pattern.whole(file) || (pattern.bare && pattern.glob.is_match(name))
                    })
                    .map(|pattern| {
                        format!(
                            "through {} of `{key}`, which leaves it out of `{skips}` while \
                             `{reads}` still reads it",
                            pattern.quoted()
                        )
                    })
            }
        }
    }
}

/// What a setting takes out, less what the tool leaves out by default: a
/// file with a name on the way below `base` that `defaults` holds.
struct Beyond {
    setting: Box<Matcher>,
    base: RelativePath,
    defaults: fn(&str) -> bool,
}

impl Beyond {
    /// Why the file is out, or [`None`] when it is not.
    fn out(&self, file: &RelativePath) -> Option<String> {
        self.setting.out(file).filter(|_| {
            !file
                .segments()
                .get(self.base.segments().len()..)
                .unwrap_or_default()
                .iter()
                .any(|name| (self.defaults)(name))
        })
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

#[derive(Deserialize)]
struct Pyright {
    include: Option<Vec<String>>,
    ignore: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct PyrightTable {
    tool: Option<PyrightTool>,
}

#[derive(Deserialize)]
struct PyrightTool {
    pyright: Option<Pyright>,
}

/// Whether a name is on Pyright's default list for `exclude`: `**/.*`,
/// `**/node_modules` and `**/__pycache__`.
fn pyright_default(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules" || name == "__pycache__"
}

/// What `include`, `ignore` and `exclude` of a configuration of Pyright take
/// out. Without `include`, Pyright reads everything below the folder of its
/// configuration, so an `include` takes out every file there that none of
/// its entries names. Pyright's default list for `exclude` holds whatever the
/// configuration says, so a file that list leaves out is not taken out by
/// `exclude` or `include`.
fn pyright(config: Pyright, base: &RelativePath) -> Result<Option<Matcher>, String> {
    let beyond = |setting: Matcher| {
        Matcher::Beyond(Beyond {
            setting: Box::new(setting),
            base: base.clone(),
            defaults: pyright_default,
        })
    };
    let paths = |entries: Option<Vec<String>>, include: bool| -> Result<Option<Matcher>, String> {
        let Some(paths) = pyright_paths(entries)? else {
            return Ok(None);
        };
        let (include, exclude) = if include {
            let named = Include {
                globs: paths,
                default: "",
            };
            let nothing = Globs {
                set: GlobSet::empty(),
                written: Vec::new(),
            };
            (Some(named), nothing)
        } else {
            (None, paths)
        };
        Ok(Some(Matcher::Globs {
            base: base.clone(),
            include,
            exclude,
        }))
    };
    let ignore = paths(config.ignore, false)?;
    let exclude = paths(config.exclude, false)?.map(beyond);
    let include = paths(config.include, true)?.map(beyond);
    let matchers: Vec<Matcher> = ignore.into_iter().chain(exclude).chain(include).collect();
    Ok((!matchers.is_empty()).then_some(Matcher::Either(matchers)))
}

/// Pyright reads each entry of `include`, `ignore` and `exclude` as a path
/// relative to the folder of its configuration, a file or a folder with
/// everything below it, where `**` stands for any folders and `*` and `?`
/// stay within one name; `.` is the folder itself. An empty list is none.
fn pyright_paths(entries: Option<Vec<String>>) -> Result<Option<Globs>, String> {
    let Some(entries) = entries.filter(|entries| !entries.is_empty()) else {
        return Ok(None);
    };
    let mut set = GlobSetBuilder::new();
    for entry in &entries {
        let path = entry
            .strip_prefix("./")
            .unwrap_or(entry)
            .trim_end_matches('/');
        let path = if path.is_empty() || path == "." {
            "**"
        } else {
            path
        };
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
    set.build()
        .map(|set| Some(Globs { set, written }))
        .map_err(text)
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
    /// Read in `.coveragerc.toml` alone, and only where the prefixed table
    /// is missing; elsewhere such a table is not coverage.py's.
    run: Option<Value>,
    report: Option<Value>,
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

/// `omit` of `[tool.coverage.run]` and `[tool.coverage.report]` in a TOML
/// file. In coverage.py's own file, `.coveragerc.toml` (`own`), it looks up a
/// section without the prefix as well, `[run]` for `[tool.coverage.run]`,
/// where the prefixed one is missing.
fn coverage_toml(source: &str, own: bool, base: &RelativePath) -> Result<Option<Matcher>, String> {
    let table: CoverageTable = toml::from_str(source).map_err(text)?;
    let sections = table.tool.and_then(|tool| tool.coverage);
    let (run, report) = sections.map_or((None, None), |sections| (sections.run, sections.report));
    let mut patterns = Vec::new();
    for (prefixed, bare) in [(run, table.run), (report, table.report)] {
        let section = match prefixed {
            None if own => bare
                .map(Value::try_into::<CoverageOmit>)
                .transpose()
                .map_err(text)?,
            section => section,
        };
        patterns.extend(
            section
                .and_then(|section| section.omit)
                .into_iter()
                .flatten(),
        );
    }
    coverage(patterns, base)
}

/// `omit` of `[run]` and `[report]` in an INI file, as coverage.py reads it
/// with Python's configparser: a value runs on over the indented lines below
/// its key, and its patterns stand one per line or apart by commas. coverage.py
/// looks the option up under each of `prefixes` in turn, `coverage:run` before
/// `run`, and takes it from the first section that sets it. An `omit` under
/// `[DEFAULT]` counts as set in every section the file has, as configparser
/// hands it on.
fn coverage_ini(
    source: &str,
    prefixes: &[&str],
    base: &RelativePath,
) -> Result<Option<Matcher>, String> {
    let mut omits: Vec<(String, Vec<String>)> = Vec::new();
    let mut sections: Vec<String> = Vec::new();
    let mut section: Option<String> = None;
    let mut collecting = false;
    for (number, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if collecting && let Some((_, patterns)) = omits.last_mut() {
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
            sections.push(name.trim().to_owned());
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
            && let Some(name) = &section
        {
            omits.push((name.clone(), split(value.trim())));
            collecting = true;
        }
    }
    let set = |wanted: &str| {
        omits
            .iter()
            .rev()
            .find(|(section, _)| section == wanted)
            .map(|(_, patterns)| patterns)
    };
    let defaults = set("DEFAULT");
    let mut patterns = Vec::new();
    for name in ["run", "report"] {
        let found = prefixes.iter().find_map(|prefix| {
            let wanted = format!("{prefix}{name}");
            set(&wanted).or_else(|| defaults.filter(|_| sections.contains(&wanted)))
        });
        patterns.extend(found.into_iter().flatten().cloned());
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

/// Ruff's default list for `exclude`, the names it leaves out wherever they
/// stand on the way to a file.
const RUFF_DEFAULT: &[&str] = &[
    ".bzr",
    ".direnv",
    ".eggs",
    ".git",
    ".git-rewrite",
    ".hg",
    ".ipynb_checkpoints",
    ".mypy_cache",
    ".nox",
    ".pants.d",
    ".pyenv",
    ".pytest_cache",
    ".pytype",
    ".ruff_cache",
    ".svn",
    ".tox",
    ".venv",
    ".vscode",
    "__pypackages__",
    "_build",
    "buck-out",
    "dist",
    "node_modules",
    "site-packages",
    "venv",
];

fn ruff_default(name: &str) -> bool {
    RUFF_DEFAULT.contains(&name)
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Ruff {
    exclude: Option<Vec<String>>,
    extend_exclude: Option<Vec<String>>,
    include: Option<Vec<String>>,
    extend_include: Option<Vec<String>>,
    lint: Option<RuffStep>,
    format: Option<RuffStep>,
}

/// The `lint` or `format` table of a configuration of Ruff.
#[derive(Deserialize)]
struct RuffStep {
    exclude: Option<Vec<String>>,
}

/// The settings with which Ruff narrows the files it reads, of one
/// configuration with its `extend` chain laid beneath it, as Ruff lays them:
/// a setting the configuration writes holds over the same one beneath it,
/// and the `extend-` settings add up.
#[derive(Default)]
struct RuffFiles {
    exclude: Option<Vec<RuffPattern>>,
    extend_exclude: Vec<RuffPattern>,
    include: Option<Vec<RuffPattern>>,
    extend_include: Vec<RuffPattern>,
    lint: Option<Vec<RuffPattern>>,
    format: Option<Vec<RuffPattern>>,
}

/// Patterns of Ruff's, relative to `base`, the folder of the file that
/// writes them; `from` is that file where it is not the configuration Ruff
/// finds.
fn ruff_patterns(
    written: Vec<String>,
    base: &RelativePath,
    from: Option<&RelativePath>,
) -> Result<Vec<RuffPattern>, String> {
    written
        .into_iter()
        .map(|written| {
            // Ruff joins the pattern to the folder and resolves each `..`.
            let mut pattern = written.strip_prefix("./").unwrap_or(&written);
            let mut segments = Some(base.segments().to_vec());
            while let Some(rest) = pattern.strip_prefix("../") {
                segments = segments.and_then(|mut segments| segments.pop().map(|_| segments));
                pattern = rest;
            }
            let base = segments.map(|segments| {
                segments
                    .iter()
                    .fold(RelativePath::root(), |path, segment| path.join(segment))
            });
            GlobBuilder::new(pattern.trim_end_matches('/'))
                .build()
                .map(|glob| RuffPattern {
                    base,
                    glob: glob.compile_matcher(),
                    bare: !written.contains('/'),
                    written: written.clone(),
                    from: from.cloned(),
                })
                .map_err(text)
        })
        .collect()
}

/// What a configuration of Ruff and its `extend` chain say of the files Ruff
/// reads. Each pattern is relative to the folder of the file that writes it,
/// as Ruff reads it. An `extend` that leaves the repository is not followed;
/// one that comes back round is an error, as Ruff refuses it.
fn ruff_files(top: &RelativePath, read: &Read<'_>) -> Result<RuffFiles, String> {
    let mut files = RuffFiles::default();
    for link in ruff_config::chain(top.clone(), read) {
        let from = (link.file != *top).then_some(&link.file);
        let named = |error: String| match from {
            Some(file) => format!("{file}: {error}"),
            None => error,
        };
        let config: Ruff = link
            .table
            .and_then(|table| Value::Table(table).try_into().map_err(text))
            .map_err(named)?;
        let base = folder(&link.file);
        let patterns = |written: Vec<String>| ruff_patterns(written, &base, from).map_err(named);
        for (held, written) in [
            (&mut files.exclude, config.exclude),
            (&mut files.include, config.include),
            (&mut files.lint, config.lint.and_then(|lint| lint.exclude)),
            (
                &mut files.format,
                config.format.and_then(|format| format.exclude),
            ),
        ] {
            if held.is_none() {
                *held = written.map(patterns).transpose()?;
            }
        }
        files
            .extend_exclude
            .extend(patterns(config.extend_exclude.unwrap_or_default())?);
        files
            .extend_include
            .extend(patterns(config.extend_include.unwrap_or_default())?);
        if let Some((written, Next::Loop)) = &link.extend {
            return Err(named(format!(
                "`extend = \"{written}\"` comes back round to a file above it, which Ruff \
                 refuses"
            )));
        }
    }
    Ok(files)
}

/// What the settings of a configuration of Ruff take out, each as a setting
/// of its own, so that one taking a file out of the linter and one taking it
/// out of the formatter are both reported. `exclude` takes the place of
/// Ruff's default list, so a file that list leaves out is not taken out by a
/// setting; nor is it by `include`, `lint.exclude` or `format.exclude`, since
/// Ruff never reaches it.
fn ruff(files: RuffFiles, base: &RelativePath, nearer: &[RelativePath]) -> Vec<Matcher> {
    let beyond = |setting: Matcher| {
        Matcher::Beyond(Beyond {
            setting: Box::new(setting),
            base: RelativePath::root(),
            defaults: ruff_default,
        })
    };
    let file = |kind: RuffKind| {
        beyond(Matcher::RuffFile(RuffFile {
            base: base.clone(),
            nearer: nearer.to_vec(),
            kind,
        }))
    };
    let mut matchers = Vec::new();
    let excluded: Vec<RuffPattern> = files
        .exclude
        .into_iter()
        .flatten()
        .chain(files.extend_exclude)
        .collect();
    if !excluded.is_empty() {
        matchers.push(beyond(Matcher::Ruff(RuffExclude {
            base: base.clone(),
            nearer: nearer.to_vec(),
            patterns: excluded,
        })));
    }
    // `extend-include` only adds to what `include` takes, so it never leaves
    // a file out on its own.
    if let Some(include) = files.include {
        matchers.push(file(RuffKind::Include {
            include,
            extend: files.extend_include,
        }));
    }
    for (patterns, key, skips, reads) in [
        (files.lint, "lint.exclude", "ruff check", "ruff format"),
        (files.format, "format.exclude", "ruff format", "ruff check"),
    ] {
        if let Some(patterns) = patterns.filter(|patterns| !patterns.is_empty()) {
            matchers.push(file(RuffKind::Step {
                patterns,
                key,
                skips,
                reads,
            }));
        }
    }
    matchers
}

/// The settings of every configuration of Ruff that narrow the files it
/// reads, each counting where it governs, and each configuration that could
/// not be read.
fn ruff_settings(
    files: &[&RelativePath],
    read: &Read<'_>,
) -> (Vec<Setting>, Vec<(RelativePath, String)>) {
    let mut configs = Vec::new();
    let mut failures = Vec::new();
    for file in files {
        let Some(rank) = ruff_config::FILES
            .iter()
            .position(|name| file.file_name() == Some(name))
        else {
            continue;
        };
        // Ruff takes the first of its names a folder holds.
        let dir = folder(file);
        if ruff_config::FILES[..rank]
            .iter()
            .any(|name| files.contains(&&dir.join(name)))
        {
            continue;
        }
        // A `pyproject.toml` without a `[tool.ruff]` table Ruff passes over.
        match read(file).and_then(|source| ruff_config::settings(file, &source)) {
            Ok(Some(_)) => configs.push((dir, (*file).clone())),
            Ok(None) => {}
            Err(error) => failures.push(((*file).clone(), error)),
        }
    }
    let folders: Vec<RelativePath> = configs.iter().map(|(dir, _)| dir.clone()).collect();
    let mut settings = Vec::new();
    for (dir, file) in configs {
        let nearer: Vec<RelativePath> = folders
            .iter()
            .filter(|other| **other != dir && dir.contains(other))
            .cloned()
            .collect();
        match ruff_files(&file, read) {
            Ok(resolved) => settings.extend(ruff(resolved, &dir, &nearer).into_iter().map(
                |matcher| Setting {
                    file: file.clone(),
                    tool: Tool::Ruff,
                    matcher,
                },
            )),
            Err(error) => failures.push((file, error)),
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
        // Pyright and coverage.py, every file they may read their settings
        // from.
        match file.file_name() {
            Some("pyrightconfig.json") => found.push((*file, Tool::Pyright)),
            Some("pyproject.toml") => {
                found.push((*file, Tool::Pyright));
                found.push((*file, Tool::Coverage));
            }
            Some(".coveragerc" | ".coveragerc.toml" | "setup.cfg" | "tox.ini") => {
                found.push((*file, Tool::Coverage));
            }
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
        Tool::Stylelint | Tool::HtmlValidate => gitignore(source.lines(), &folder(file)).map(Some),
        Tool::Pyright if file.file_name() == Some("pyrightconfig.json") => {
            pyright_json(&source, &folder(file))
        }
        Tool::Pyright => pyright_toml(&source, &folder(file)),
        Tool::Coverage if file.file_name() == Some("pyproject.toml") => {
            coverage_toml(&source, false, &folder(file))
        }
        Tool::Coverage if file.file_name() == Some(".coveragerc.toml") => {
            coverage_toml(&source, true, &folder(file))
        }
        Tool::Coverage if file.file_name() == Some(".coveragerc") => {
            coverage_ini(&source, &["coverage:", ""], &folder(file))
        }
        Tool::Coverage => coverage_ini(&source, &["coverage:"], &folder(file)),
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
        Tool::Prettier
        | Tool::Taplo
        | Tool::Markdownlint
        | Tool::Yamllint
        | Tool::Lychee
        | Tool::Pyright
        | Tool::Coverage
        | Tool::Ruff
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
    let (configured, failures) = ruff_settings(&files, read);
    settings.extend(configured);
    findings.extend(
        failures
            .into_iter()
            .map(|(file, error)| unreadable(&file, Tool::Ruff, error)),
    );

    // A Python file that takes itself out of Ruff. Only the files a check
    // reads are opened; one that cannot be read holds no such line.
    for (node, coverage) in &nodes {
        let python = matches!(coverage, Coverage::Covered(check) | Coverage::Uncovered(check)
            if Tool::Ruff.checks().contains(&check.as_str()));
        if let Some(Ok(Some(matcher))) = python.then(|| read_setting(&node.path, Tool::Ruff, read))
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
                let message = if let Matcher::Itself { .. } = setting.matcher {
                    // Ruff honours the line in the block as well, and single
                    // rules stay the repository's choice.
                    format!(
                        "this file takes itself out of Ruff's linter {why}, and no [[exclude]] \
                         entry does; Ruff leaves it unchecked wherever it runs: switch single \
                         rules off (`# ruff: noqa: <code>`), or {remedy}",
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
