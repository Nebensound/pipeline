//! The rules a repository sets differently from the pipeline's default.
//!
//! Two programs of the building blocks read a rule configuration that the
//! pipeline gives a default for: Markdownlint in `check-markdown` and yamllint
//! in `check-yaml`. The block lays the repository's own configuration over
//! that default rule by rule, so the repository's file names only what
//! differs, and a change of the default reaches every repository that did not
//! name the rule. The defaults lie beside this tool in `rules/`: the blocks
//! read them from there, and the tool compiles them in, so both speak of the
//! same files.
//!
//! Read are the files the blocks lay over the default:
//!
//! - Markdownlint: in every folder, the first options file markdownlint-cli2
//!   reads there (`.markdownlint-cli2.jsonc`, `.yaml`, `.cjs`, `.mjs`) with
//!   its `config`, and the first file that holds rules only
//!   (`.markdownlint.jsonc`, `.json`, `.yaml`, `.yml`, `.cjs`, `.mjs`). The
//!   block lays the one at the root over the default; one in a folder below
//!   takes the place of everything above it there, and of the `config` of the
//!   options file beside it, as markdownlint-cli2 reads it. markdownlint-cli2
//!   reads the files of the folders that hold what it lints and of the folders
//!   above them, so a folder's files are read only where the markdown check
//!   covers a file in that folder or below it. Whether `[[exclude]]` takes the
//!   configuration file itself out does not matter: the program reads it all
//!   the same.
//! - yamllint: `rules` in `.yamllint`, `.yamllint.yaml` or `.yamllint.yml` at
//!   the root, whichever yamllint reads. yamllint reads it with `PyYAML`, as
//!   YAML 1.1, so a plain `yes`, `no`, `on` or `off` in it is a boolean.
//!
//! A file written in JavaScript is named, not read: phase 0 runs no code of
//! the repository. What an `extends` names is named, not read.
//!
//! A rule the pipeline's file does not name keeps what lies beneath that file:
//! Markdownlint's own default for the rule, and for yamllint what the file's
//! `extends` sets. The report says so rather than comparing with it, since
//! neither program's own defaults are compiled in.

use std::collections::BTreeSet;

use crate::Analysis;
use crate::annotate::Coverage;
use crate::path::RelativePath;
use serde_json::{Map, Number, Value};
use yaml_rust2::parser::{MarkedEventReceiver, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};
use yaml_rust2::{Event, Yaml, YamlLoader};

use crate::ignored::{Read, folder};
use crate::report::{column, shown};

/// The pipeline's default for Markdownlint, as `check-markdown` reads it.
pub const MARKDOWNLINT: &str = include_str!("../rules/markdownlint.jsonc");

/// The pipeline's default for yamllint, as `check-yaml` reads it.
pub const YAMLLINT: &str = include_str!("../rules/yamllint.yaml");

/// The defaults a repository's rules are held against, as text.
#[derive(Debug, Clone, Copy)]
pub struct Defaults<'a> {
    /// The default for Markdownlint, in JSONC.
    pub markdownlint: &'a str,
    /// The default for yamllint, in YAML.
    pub yamllint: &'a str,
}

impl Defaults<'static> {
    /// The defaults compiled into this tool: the files the blocks read.
    pub const BUILT_IN: Self = Self {
        markdownlint: MARKDOWNLINT,
        yamllint: YAMLLINT,
    };
}

/// The options files of markdownlint-cli2, in the order it looks for them.
const MARKDOWNLINT_OPTIONS: &[&str] = &[
    ".markdownlint-cli2.jsonc",
    ".markdownlint-cli2.yaml",
    ".markdownlint-cli2.cjs",
    ".markdownlint-cli2.mjs",
];

/// The files of markdownlint-cli2 that hold rules only, in the order it looks
/// for them.
const MARKDOWNLINT_RULES: &[&str] = &[
    ".markdownlint.jsonc",
    ".markdownlint.json",
    ".markdownlint.yaml",
    ".markdownlint.yml",
    ".markdownlint.cjs",
    ".markdownlint.mjs",
];

/// The rules of Markdownlint, each with its names and its tags, as
/// markdownlint 0.42.0 gives them. A key of a configuration names a rule by
/// any of its names, or every rule with a tag, in any case.
const MARKDOWNLINT_NAMES: &[(&[&str], &[&str])] = &[
    (&["MD001", "heading-increment"], &["headings"]),
    (&["MD003", "heading-style"], &["headings"]),
    (&["MD004", "ul-style"], &["bullet", "ul"]),
    (&["MD005", "list-indent"], &["bullet", "ul", "indentation"]),
    (&["MD007", "ul-indent"], &["bullet", "ul", "indentation"]),
    (&["MD009", "no-trailing-spaces"], &["whitespace"]),
    (&["MD010", "no-hard-tabs"], &["whitespace", "hard_tab"]),
    (&["MD011", "no-reversed-links"], &["links"]),
    (
        &["MD012", "no-multiple-blanks"],
        &["whitespace", "blank_lines"],
    ),
    (&["MD013", "line-length"], &["line_length"]),
    (&["MD014", "commands-show-output"], &["code"]),
    (
        &["MD018", "no-missing-space-atx"],
        &["headings", "atx", "spaces"],
    ),
    (
        &["MD019", "no-multiple-space-atx"],
        &["headings", "atx", "spaces"],
    ),
    (
        &["MD020", "no-missing-space-closed-atx"],
        &["headings", "atx_closed", "spaces"],
    ),
    (
        &["MD021", "no-multiple-space-closed-atx"],
        &["headings", "atx_closed", "spaces"],
    ),
    (
        &["MD022", "blanks-around-headings"],
        &["headings", "blank_lines"],
    ),
    (&["MD023", "heading-start-left"], &["headings", "spaces"]),
    (&["MD024", "no-duplicate-heading"], &["headings"]),
    (&["MD025", "single-title", "single-h1"], &["headings"]),
    (&["MD026", "no-trailing-punctuation"], &["headings"]),
    (
        &["MD027", "no-multiple-space-blockquote"],
        &["blockquote", "whitespace", "indentation"],
    ),
    (
        &["MD028", "no-blanks-blockquote"],
        &["blockquote", "whitespace"],
    ),
    (&["MD029", "ol-prefix"], &["ol"]),
    (&["MD030", "list-marker-space"], &["ol", "ul", "whitespace"]),
    (&["MD031", "blanks-around-fences"], &["code", "blank_lines"]),
    (
        &["MD032", "blanks-around-lists"],
        &["bullet", "ul", "ol", "blank_lines"],
    ),
    (&["MD033", "no-inline-html"], &["html"]),
    (&["MD034", "no-bare-urls"], &["links", "url"]),
    (&["MD035", "hr-style"], &["hr"]),
    (
        &["MD036", "no-emphasis-as-heading"],
        &["headings", "emphasis"],
    ),
    (
        &["MD037", "no-space-in-emphasis"],
        &["whitespace", "emphasis"],
    ),
    (&["MD038", "no-space-in-code"], &["whitespace", "code"]),
    (&["MD039", "no-space-in-links"], &["whitespace", "links"]),
    (&["MD040", "fenced-code-language"], &["code", "language"]),
    (
        &["MD041", "first-line-heading", "first-line-h1"],
        &["headings"],
    ),
    (&["MD042", "no-empty-links"], &["links"]),
    (&["MD043", "required-headings"], &["headings"]),
    (&["MD044", "proper-names"], &["spelling"]),
    (&["MD045", "no-alt-text"], &["accessibility", "images"]),
    (&["MD046", "code-block-style"], &["code"]),
    (&["MD047", "single-trailing-newline"], &["blank_lines"]),
    (&["MD048", "code-fence-style"], &["code"]),
    (&["MD049", "emphasis-style"], &["emphasis"]),
    (&["MD050", "strong-style"], &["emphasis"]),
    (&["MD051", "link-fragments"], &["links"]),
    (&["MD052", "reference-links-images"], &["images", "links"]),
    (
        &["MD053", "link-image-reference-definitions"],
        &["images", "links"],
    ),
    (&["MD054", "link-image-style"], &["images", "links"]),
    (&["MD055", "table-pipe-style"], &["table"]),
    (&["MD056", "table-column-count"], &["table"]),
    (&["MD058", "blanks-around-tables"], &["table"]),
    (
        &["MD059", "descriptive-link-text"],
        &["accessibility", "links"],
    ),
    (&["MD060", "table-column-style"], &["table"]),
];

/// The configuration files of yamllint, in the order it looks for them.
const YAMLLINT_FILES: &[&str] = &[".yamllint", ".yamllint.yaml", ".yamllint.yml"];

/// A program whose rules have a default of the pipeline.
#[derive(Debug, Clone, Copy)]
enum Program {
    Markdownlint,
    Yamllint,
}

impl Program {
    /// The check whose block runs it.
    fn check(self) -> &'static str {
        match self {
            Self::Markdownlint => "markdown",
            Self::Yamllint => "yaml",
        }
    }

    /// The name it goes by.
    fn name(self) -> &'static str {
        match self {
            Self::Markdownlint => "Markdownlint",
            Self::Yamllint => "yamllint",
        }
    }

    /// The rules of its default, read from the text.
    fn default(self, text: &str) -> Result<Config, String> {
        match self {
            Self::Markdownlint => config(parse(Format::Jsonc, text)?),
            Self::Yamllint => yamllint(text),
        }
    }

    /// How the value the repository writes for a rule stands to what the
    /// default gives that rule.
    fn stand(self, rule: &str, value: &Value, default: &Config) -> Stand {
        match self {
            // Markdownlint takes a rule's value as a whole. A key can name a
            // rule by an alias, or several rules by a tag; what the default
            // gives each of them is compared.
            Self::Markdownlint => {
                let rules = covered(rule);
                let defaults: Vec<Option<&Value>> = if rules.is_empty() {
                    vec![default.rules.get(rule)]
                } else {
                    rules
                        .iter()
                        .map(|name| markdownlint_default(&default.rules, name))
                        .collect()
                };
                if defaults.windows(2).any(|pair| pair.first() != pair.last()) {
                    return Stand::Mixed;
                }
                standing(defaults.first().copied().flatten(), |under| value == under)
            }
            // yamllint lays the options of a rule over those of the rule
            // beneath, where both are on.
            Self::Yamllint => standing(default.rules.get(rule), |under| {
                let base = enabled(under);
                let over = match (enabled(value), base.clone()) {
                    (Value::Object(own), Value::Object(mut under)) => {
                        under.extend(own);
                        Value::Object(under)
                    }
                    (own, _) => own,
                };
                over == base
            }),
        }
    }

    /// What holds for a rule the pipeline's file does not name.
    fn unset(self, default: &Config) -> String {
        const UNNAMED: &str = "the pipeline's file does not name it";
        match (self, &default.extends) {
            (Self::Markdownlint, _) => format!("{UNNAMED}; Markdownlint's own default holds"),
            (Self::Yamllint, Some(extends)) => {
                format!("{UNNAMED}; what its `extends: {extends}` sets holds")
            }
            (Self::Yamllint, None) => format!("{UNNAMED}, so it is off"),
        }
    }
}

/// The rules of Markdownlint a key of its configuration stands for: the one
/// rule it names, or every rule with the tag it names. None for a key that is
/// neither, such as `default` or a rule markdownlint does not bring along.
/// Markdownlint compares keys in upper case.
fn covered(key: &str) -> Vec<&'static str> {
    let key = key.to_uppercase();
    let names = |words: &[&str]| words.iter().any(|word| word.to_ascii_uppercase() == key);
    if let Some((rule, _)) = MARKDOWNLINT_NAMES.iter().find(|(rule, _)| names(rule)) {
        return rule.first().copied().into_iter().collect();
    }
    MARKDOWNLINT_NAMES
        .iter()
        .filter(|(_, tags)| names(tags))
        .filter_map(|(rule, _)| rule.first().copied())
        .collect()
}

/// What the default gives one rule of Markdownlint: the value of a key that
/// covers it, or else the value of its `default`, which stands for every rule
/// it does not name. The pipeline's file names each rule once; where several
/// keys cover it, the one sorted last is taken, as the keys are read in
/// sorted order.
fn markdownlint_default<'a>(rules: &'a Map<String, Value>, rule: &str) -> Option<&'a Value> {
    rules
        .iter()
        .rev()
        .find(|(key, _)| covered(key).contains(&rule))
        .or_else(|| {
            rules
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("default"))
        })
        .map(|(_, value)| value)
}

/// How a value stands to what the default gives the rule, told by `same`.
fn standing(default: Option<&Value>, same: impl FnOnce(&Value) -> bool) -> Stand {
    match default {
        None => Stand::Unset,
        Some(under) if same(under) => Stand::Same,
        Some(under) => Stand::Over(under.clone()),
    }
}

/// yamllint's words for a rule switched on or off, as the values it reads
/// them as.
fn enabled(value: &Value) -> Value {
    match value.as_str() {
        Some("enable") => Value::Object(Map::new()),
        Some("disable") => Value::Bool(false),
        _ => value.clone(),
    }
}

/// How a value stands to the default.
enum Stand {
    /// The default gives the rule the same value.
    Same,
    /// The default gives it this value.
    Over(Value),
    /// The pipeline's file does not name the rule.
    Unset,
    /// The key covers several rules, which the default does not all give the
    /// same value.
    Mixed,
}

/// What a configuration names.
#[derive(Default)]
struct Config {
    /// What its `extends` names.
    extends: Option<String>,
    /// Its rules, each with its value.
    rules: Map<String, Value>,
}

/// How a file is written, told by its name.
#[derive(Clone, Copy)]
enum Format {
    Jsonc,
    Yaml,
    Script,
}

/// The programs look for their files by exact name, so the ending is matched
/// exactly too; `.yamllint` has none and is YAML.
fn format(file: &RelativePath) -> Format {
    let name = file.file_name().unwrap_or_default();
    match name.rsplit_once('.').map(|(_, ending)| ending) {
        Some("cjs" | "mjs") => Format::Script,
        Some("jsonc" | "json") => Format::Jsonc,
        _ => Format::Yaml,
    }
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// The value a file holds. A file in JavaScript is never handed here.
fn parse(format: Format, source: &str) -> Result<Value, String> {
    match format {
        Format::Jsonc => {
            jsonc_parser::parse_to_serde_value(source, &jsonc_parser::ParseOptions::default())
                .map_err(text)
        }
        Format::Yaml | Format::Script => {
            let documents = YamlLoader::load_from_str(source).map_err(text)?;
            Ok(documents.first().map_or(Value::Null, json))
        }
    }
}

/// The words `PyYAML` reads as a boolean in a plain scalar, under YAML 1.1,
/// each with the word YAML 1.2 writes for it.
const YAML_1_1_BOOLEANS: &[(&str, &str)] = &[
    ("yes", "true"),
    ("Yes", "true"),
    ("YES", "true"),
    ("on", "true"),
    ("On", "true"),
    ("ON", "true"),
    ("no", "false"),
    ("No", "false"),
    ("NO", "false"),
    ("off", "false"),
    ("Off", "false"),
    ("OFF", "false"),
];

/// A loader that reads a plain scalar as YAML 1.1 does where 1.2 differs in a
/// boolean, and hands everything else on as it stands. A quoted scalar stays
/// text, as in `PyYAML`.
struct Yaml11(YamlLoader);

impl MarkedEventReceiver for Yaml11 {
    fn on_event(&mut self, event: Event, mark: Marker) {
        let event = match event {
            Event::Scalar(word, TScalarStyle::Plain, anchor, None) => {
                let word = YAML_1_1_BOOLEANS
                    .iter()
                    .find(|(old, _)| *old == word)
                    .map_or(word, |(_, new)| (*new).to_owned());
                Event::Scalar(word, TScalarStyle::Plain, anchor, None)
            }
            other => other,
        };
        self.0.on_event(event, mark);
    }
}

/// The value a configuration of yamllint holds, read as `PyYAML` reads it.
fn parse_yaml_1_1(source: &str) -> Result<Value, String> {
    let mut loader = Yaml11(YamlLoader::default());
    Parser::new_from_str(source)
        .load(&mut loader, true)
        .map_err(text)?;
    // The loader keeps an error of its own, such as a key written twice, to
    // itself; read as it stands, the source names it.
    YamlLoader::load_from_str(source).map_err(text)?;
    Ok(loader.0.documents().first().map_or(Value::Null, json))
}

/// A YAML value as the JSON value it stands for. A key that is not text is
/// written out as YAML would show it.
fn json(yaml: &Yaml) -> Value {
    match yaml {
        Yaml::Hash(hash) => Value::Object(
            hash.iter()
                .map(|(key, value)| {
                    let key = match key {
                        Yaml::String(name) => name.clone(),
                        other => json(other).to_string(),
                    };
                    (key, json(value))
                })
                .collect(),
        ),
        Yaml::Array(items) => Value::Array(items.iter().map(json).collect()),
        Yaml::String(text) => Value::String(text.clone()),
        Yaml::Integer(number) => Value::from(*number),
        Yaml::Real(written) => yaml
            .as_f64()
            .and_then(Number::from_f64)
            .map_or_else(|| Value::String(written.clone()), Value::Number),
        Yaml::Boolean(flag) => Value::Bool(*flag),
        Yaml::Null | Yaml::BadValue | Yaml::Alias(_) => Value::Null,
    }
}

const NO_MAPPING: &str = "it holds no mapping";

/// What an `extends` names.
fn extends(value: Option<Value>) -> Result<Option<String>, String> {
    match value {
        None => Ok(None),
        Some(Value::String(name)) => Ok(Some(name)),
        Some(_) => Err("`extends` is not text".to_owned()),
    }
}

/// Rules in the form Markdownlint reads them: a mapping from rule to value,
/// beside `extends` and the editor's `$schema`.
fn config(value: Value) -> Result<Config, String> {
    let Value::Object(mut rules) = value else {
        return Err(NO_MAPPING.to_owned());
    };
    rules.remove("$schema");
    let extends = extends(rules.remove("extends"))?;
    Ok(Config { extends, rules })
}

/// The rules of an options file of markdownlint-cli2: its `config`.
fn options(format: Format, source: &str) -> Result<Config, String> {
    let Value::Object(mut options) = parse(format, source)? else {
        return Err(NO_MAPPING.to_owned());
    };
    options
        .remove("config")
        .map_or_else(|| Ok(Config::default()), config)
}

/// The rules of a configuration of yamllint: its `rules`, beside `extends`.
fn yamllint(source: &str) -> Result<Config, String> {
    let Value::Object(mut top) = parse_yaml_1_1(source)? else {
        return Err(NO_MAPPING.to_owned());
    };
    let extends = extends(top.remove("extends"))?;
    let rules = match top.remove("rules") {
        None => Map::new(),
        Some(Value::Object(rules)) => rules,
        Some(_) => return Err("`rules` is not a mapping".to_owned()),
    };
    Ok(Config { extends, rules })
}

/// Where a file's rules stand.
#[derive(Clone, Copy)]
enum Place {
    /// Laid over the default, and over what lies above it.
    Over,
    /// In place of everything above it, the default included.
    Instead,
    /// Not in effect: an options file whose folder below the root also holds
    /// a file of rules only, which markdownlint-cli2 takes in place of its
    /// `config`.
    Unread,
}

/// How a file's rules are read.
#[derive(Clone, Copy)]
enum Kind {
    MarkdownlintOptions,
    MarkdownlintRules,
    Yamllint,
}

/// One file of the repository that is laid over the default.
struct Layer {
    file: RelativePath,
    place: Place,
    kind: Kind,
}

impl Layer {
    /// What it names, or [`None`] for a file in JavaScript.
    fn read(&self, read: &Read<'_>) -> Result<Option<Config>, String> {
        let format = format(&self.file);
        if let Format::Script = format {
            return Ok(None);
        }
        let source = read(&self.file)?;
        match self.kind {
            Kind::MarkdownlintOptions => options(format, &source),
            Kind::MarkdownlintRules => config(parse(format, &source)?),
            Kind::Yamllint => yamllint(&source),
        }
        .map(Some)
    }
}

/// The first of these names in the folder.
fn first<'a>(
    files: &[&'a RelativePath],
    dir: &RelativePath,
    names: &[&str],
) -> Option<&'a RelativePath> {
    names.iter().find_map(|name| {
        files
            .iter()
            .find(|file| file.file_name() == Some(name) && folder(file) == *dir)
            .copied()
    })
}

/// The files markdownlint-cli2 reads, folder by folder, the root first; in a
/// folder, the one that holds rules only before the options file. At the root
/// the block lays the options file's `config` over the file of rules only; in
/// a folder below, markdownlint-cli2 takes the file of rules only and leaves
/// the `config` beside it unread. A folder counts only where a file the check
/// lints lies in it or below it: markdownlint-cli2 reads no other.
fn markdownlint_layers(files: &[&RelativePath], linted: &[&RelativePath]) -> Vec<Layer> {
    let folders: BTreeSet<RelativePath> = files
        .iter()
        .filter(|file| {
            let name = file.file_name().unwrap_or_default();
            MARKDOWNLINT_OPTIONS.contains(&name) || MARKDOWNLINT_RULES.contains(&name)
        })
        .map(|file| folder(file))
        .filter(|dir| linted.iter().any(|file| dir.contains(file)))
        .collect();
    let mut layers = Vec::new();
    for dir in folders {
        let rules = first(files, &dir, MARKDOWNLINT_RULES);
        let below = !dir.is_root();
        if let Some(file) = rules {
            layers.push(Layer {
                file: file.clone(),
                place: if below { Place::Instead } else { Place::Over },
                kind: Kind::MarkdownlintRules,
            });
        }
        if let Some(file) = first(files, &dir, MARKDOWNLINT_OPTIONS) {
            layers.push(Layer {
                file: file.clone(),
                place: if below && rules.is_some() {
                    Place::Unread
                } else {
                    Place::Over
                },
                kind: Kind::MarkdownlintOptions,
            });
        }
    }
    layers
}

/// The configuration yamllint reads at the root.
fn yamllint_layers(files: &[&RelativePath]) -> Vec<Layer> {
    first(files, &RelativePath::root(), YAMLLINT_FILES)
        .map(|file| Layer {
            file: file.clone(),
            place: Place::Over,
            kind: Kind::Yamllint,
        })
        .into_iter()
        .collect()
}

/// The lines of one program: what lies over its default, and how every rule
/// stands to it.
fn section(program: Program, default: &str, layers: &[Layer], read: &Read<'_>) -> Vec<String> {
    let head = column(2, program.check());
    let name = program.name();
    let default = match program.default(default) {
        Ok(default) => default,
        Err(error) => {
            return vec![format!(
                "{head}{name}: the pipeline's default does not read: {}",
                shown(&error)
            )];
        }
    };
    if layers.is_empty() {
        return vec![format!("{head}{name}: the default, nothing laid over it")];
    }
    let mut lines = vec![format!(
        "{head}{name}: the default, with these laid over it"
    )];
    for layer in layers {
        let file = column(6, &shown(&layer.file.to_string()));
        let place = match layer.place {
            Place::Over if folder(&layer.file).is_root() => "laid over the default rule by rule",
            Place::Over => "laid over the default and what lies above it, rule by rule",
            Place::Instead => "in place of everything above it, the default included",
            Place::Unread => {
                lines.push(format!(
                    "{file}its `config` is not read: the file of rules only in its folder \
                     takes its place"
                ));
                continue;
            }
        };
        let config = match layer.read(read) {
            Ok(Some(config)) => config,
            Ok(None) => {
                lines.push(format!(
                    "{file}written in JavaScript, which phase 0 does not run"
                ));
                continue;
            }
            Err(error) => {
                lines.push(format!("{file}cannot be read: {}", shown(&error)));
                continue;
            }
        };
        if config.extends.is_none() && config.rules.is_empty() {
            lines.push(format!("{file}{place}; it names no rule"));
            continue;
        }
        lines.push(format!("{file}{place}"));
        if let Some(extends) = &config.extends {
            lines.push(format!(
                "{}{}: what it sets lies beneath the rules here",
                column(10, "extends"),
                shown(extends)
            ));
        }
        for (rule, value) in &config.rules {
            let stand = match program.stand(rule, value, &default) {
                Stand::Same => "the same as the default".to_owned(),
                Stand::Over(under) => format!("over the default {under}"),
                Stand::Unset => program.unset(&default),
                Stand::Mixed => {
                    "the default does not give the rules it covers one value".to_owned()
                }
            };
            lines.push(format!(
                "{}{}",
                column(10, &shown(rule)),
                shown(&format!("{value} ({stand})"))
            ));
        }
    }
    lines
}

/// Which rules the repository sets for each program with a default, and how
/// each stands to that default.
#[must_use]
pub fn report(analysis: &Analysis, read: &Read<'_>, defaults: Defaults<'_>) -> String {
    let nodes = analysis.tree.files();
    let files: Vec<&RelativePath> = nodes.iter().map(|(node, _)| &node.path).collect();
    let linted: Vec<&RelativePath> = nodes
        .iter()
        .filter(|(_, coverage)| {
            matches!(coverage, Coverage::Covered(check)
                if check.as_str() == Program::Markdownlint.check())
        })
        .map(|(node, _)| &node.path)
        .collect();
    let mut lines = vec!["Rules".to_owned(), String::new()];
    lines.extend(section(
        Program::Markdownlint,
        defaults.markdownlint,
        &markdownlint_layers(&files, &linted),
        read,
    ));
    lines.extend(section(
        Program::Yamllint,
        defaults.yamllint,
        &yamllint_layers(&files),
        read,
    ));
    lines.join("\n")
}
