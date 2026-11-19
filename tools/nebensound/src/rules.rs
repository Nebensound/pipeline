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
//!   takes the place of everything above it there, as markdownlint-cli2 reads
//!   it.
//! - yamllint: `rules` in `.yamllint`, `.yamllint.yaml` or `.yamllint.yml` at
//!   the root, whichever yamllint reads.
//!
//! A file written in JavaScript is named, not read: phase 0 runs no code of
//! the repository. What an `extends` names is named, not read.

use std::collections::BTreeSet;

use nebensound_declaration::Analysis;
use nebensound_declaration::path::RelativePath;
use serde_json::{Map, Number, Value};
use yaml_rust2::{Yaml, YamlLoader};

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
            Self::Yamllint => yamllint(Format::Yaml, text),
        }
    }

    /// How a value the repository writes stands to the default's value of
    /// the same rule.
    fn stand(self, value: &Value, default: Option<&Value>) -> Stand {
        let Some(default) = default else {
            return Stand::Unset;
        };
        let same = match self {
            // Markdownlint takes a rule's value as a whole.
            Self::Markdownlint => value == default,
            // yamllint lays the options of a rule over those of the rule
            // beneath, where both are on.
            Self::Yamllint => {
                let base = enabled(default);
                let over = match (enabled(value), base.clone()) {
                    (Value::Object(own), Value::Object(mut under)) => {
                        under.extend(own);
                        Value::Object(under)
                    }
                    (own, _) => own,
                };
                over == base
            }
        };
        if same {
            Stand::Same
        } else {
            Stand::Over(default.clone())
        }
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
    /// The default does not name the rule.
    Unset,
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
fn yamllint(format: Format, source: &str) -> Result<Config, String> {
    let Value::Object(mut top) = parse(format, source)? else {
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
            Kind::Yamllint => yamllint(format, &source),
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
/// folder, the one that holds rules only before the options file, which lies
/// over it.
fn markdownlint_layers(files: &[&RelativePath]) -> Vec<Layer> {
    let folders: BTreeSet<RelativePath> = files
        .iter()
        .filter(|file| {
            let name = file.file_name().unwrap_or_default();
            MARKDOWNLINT_OPTIONS.contains(&name) || MARKDOWNLINT_RULES.contains(&name)
        })
        .map(|file| folder(file))
        .collect();
    let mut layers = Vec::new();
    for dir in folders {
        if let Some(file) = first(files, &dir, MARKDOWNLINT_RULES) {
            layers.push(Layer {
                file: file.clone(),
                place: if dir.is_root() {
                    Place::Over
                } else {
                    Place::Instead
                },
                kind: Kind::MarkdownlintRules,
            });
        }
        if let Some(file) = first(files, &dir, MARKDOWNLINT_OPTIONS) {
            layers.push(Layer {
                file: file.clone(),
                place: Place::Over,
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
        let place = match layer.place {
            Place::Over if folder(&layer.file).is_root() => "laid over the default rule by rule",
            Place::Over => "laid over the default and what lies above it, rule by rule",
            Place::Instead => "in place of everything above it, the default included",
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
            let stand = match program.stand(value, default.rules.get(rule)) {
                Stand::Same => "the same as the default".to_owned(),
                Stand::Over(default) => format!("over the default {default}"),
                Stand::Unset => "the default does not set it".to_owned(),
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
    let mut lines = vec!["Rules".to_owned(), String::new()];
    lines.extend(section(
        Program::Markdownlint,
        defaults.markdownlint,
        &markdownlint_layers(&files),
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
