//! The rules a repository sets differently from the pipeline's default.
//!
//! Three programs of the checks read a rule configuration that the pipeline
//! gives a default for: Markdownlint in the Markdown check, Stylelint in the
//! CSS check and html-validate in the HTML check.
//! `run` lays the repository's own configuration over that default rule by
//! rule, so the repository's file names only what differs, and a change of the
//! default reaches every repository that did not name the rule. The defaults
//! lie beside this tool in `rules/`: the tool compiles them in, both to hand
//! them to the linters in `run` and to report on them here, so both speak of
//! the same files.
//!
//! Read are the files the checks lay over the default:
//!
//! - Markdownlint: the one file of its configuration at the root, read as
//!   the Markdown check reads it ([`crate::linter_config::markdownlint`]),
//!   with its differences by pattern, and every file of its configuration
//!   the check does not read, and every key it does not hand over, each
//!   with why. The check prints the same lines ([`markdownlint`]).
//! - Stylelint: the one file of its configuration at the root, read as the
//!   CSS check reads it ([`crate::linter_config::stylelint`]), with its
//!   differences by pattern, and every file of its configuration the check
//!   does not read, and every key it does not hand over, each with why. The
//!   check prints the same lines ([`stylelint`]).
//! - html-validate: `rules` in every `.htmlvalidate.json` (`.mjs`, `.cjs`,
//!   `.js`) of a folder that holds a file the html check covers, or holds one
//!   below it, up to a folder whose `.htmlvalidate.json` says `root: true`.
//!   `run` lays them over the default, each nearer one over the ones above
//!   it; one in a folder that says `root: true` lies over the default alone.
//!
//! The two of them are shown only where their check covers a file, and so is
//! Ruff, the fourth program with a default, whose `check-python` lays the
//! repository's configuration over it through Ruff's `extend`: the module
//! `ruff` within this one says what is read of it and how.
//!
//! A file written in JavaScript is named, not read: phase 0 runs no code of
//! the repository. What an `extends` names is named, not read.
//!
//! A rule the pipeline's file does not name keeps what lies beneath that file:
//! Markdownlint's own default for the rule, and for the others what the file's
//! `extends` sets. The report says so rather than comparing with it, since
//! neither program's own defaults are compiled in.

mod ruff;

use std::collections::BTreeSet;

use crate::Analysis;
use crate::annotate::Coverage;
use crate::path::RelativePath;
use serde_json::{Number, Value};
use yaml_rust2::Yaml;

use crate::ignored::{Read, folder};
use crate::linter_config::markdownlint::{Combine, Handed, Problem, Rules};
use crate::linter_config::{self, Written, stylelint as styled};
use crate::register::Register;
use crate::report::{column, shown};

/// The pipeline's default for Markdownlint, as `run markdown` hands it over.
pub const MARKDOWNLINT: &str = include_str!("../rules/markdownlint.jsonc");

/// The pipeline's default for Stylelint, as `run css` hands it over.
pub const STYLELINT: &str = include_str!("../rules/stylelint.yaml");

/// The pipeline's default for html-validate, as `run html` hands it over.
pub const HTML_VALIDATE: &str = include_str!("../rules/html-validate.json");

/// The pipeline's default for Ruff, as `check-python` reads it.
pub const RUFF: &str = include_str!("../rules/ruff.toml");

/// The defaults a repository's rules are held against, as text.
#[derive(Debug, Clone, Copy)]
pub struct Defaults<'a> {
    /// The default for Markdownlint, in JSONC.
    pub markdownlint: &'a str,
    /// The default for Stylelint, in YAML.
    pub stylelint: &'a str,
    /// The default for html-validate, in JSON.
    pub html_validate: &'a str,
    /// The default for Ruff, in TOML.
    pub ruff: &'a str,
}

impl Defaults<'static> {
    /// The defaults compiled into this tool: the files `run` hands the
    /// linters.
    pub const BUILT_IN: Self = Self {
        markdownlint: MARKDOWNLINT,
        stylelint: STYLELINT,
        html_validate: HTML_VALIDATE,
        ruff: RUFF,
    };
}

/// The rules of Markdownlint, each with its names and its tags, as
/// markdownlint 0.41.1, the version the register pins, gives them. A key of a configuration names a rule by
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

/// The configuration files of html-validate in a folder, all of which it
/// reads, in this order.
const HTML_VALIDATE_FILES: &[&str] = &[
    ".htmlvalidate.json",
    ".htmlvalidate.mjs",
    ".htmlvalidate.cjs",
    ".htmlvalidate.js",
];

/// A program whose rules have a default of the pipeline.
#[derive(Debug, Clone, Copy)]
enum Program {
    Markdownlint,
    Stylelint,
    HtmlValidate,
}

impl Program {
    /// The check whose block runs it.
    fn check(self) -> &'static str {
        match self {
            Self::Markdownlint => "markdown",
            Self::Stylelint => "css",
            Self::HtmlValidate => "html",
        }
    }

    /// The name it goes by.
    fn name(self) -> &'static str {
        match self {
            Self::Markdownlint => "Markdownlint",
            Self::Stylelint => "Stylelint",
            Self::HtmlValidate => "html-validate",
        }
    }

    /// The rules of its default, read from the text.
    fn default(self, text: &str) -> Result<Config, String> {
        match self {
            Self::Markdownlint => config(linter_config::parse(Format::Jsonc, text)?),
            Self::Stylelint => styled::pinned(text).map(|pinned| Config {
                extends: Some(pinned.base.extends.join(", ")).filter(|names| !names.is_empty()),
                rules: pinned.base.rules,
            }),
            Self::HtmlValidate => rules_of(parse(text)?),
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
                    vec![get(&default.rules, rule)]
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
            // Both take a rule's value as a whole.
            Self::Stylelint | Self::HtmlValidate => {
                standing(get(&default.rules, rule), |under| value == under)
            }
        }
    }

    /// What holds for a rule the pipeline's file does not name.
    fn unset(self, default: &Config) -> String {
        const UNNAMED: &str = "the pipeline's file does not name it";
        match (self, &default.extends) {
            (Self::Markdownlint, _) => format!("{UNNAMED}; Markdownlint's own default holds"),
            (Self::Stylelint | Self::HtmlValidate, Some(extends)) => {
                format!("{UNNAMED}; what its `extends: {extends}` sets holds")
            }
            (Self::Stylelint | Self::HtmlValidate, None) => {
                format!("{UNNAMED}, so it is off")
            }
        }
    }
}

/// The rules of Markdownlint a key of its configuration stands for: the one
/// rule it names, or every rule with the tag it names. None for a key that is
/// neither, such as `default` or a rule markdownlint does not bring along.
/// Markdownlint compares keys in upper case.
pub(crate) fn covered(key: &str) -> Vec<&'static str> {
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

/// The value of a rule, by the key it is written under.
fn get<'a>(rules: &'a [(String, Value)], rule: &str) -> Option<&'a Value> {
    rules
        .iter()
        .find(|(key, _)| key == rule)
        .map(|(_, value)| value)
}

/// What the default gives one rule of Markdownlint: the value of a key that
/// covers it, or else the value of its first `default`, which stands for
/// every rule it does not name. Where several keys cover the rule, the one
/// written last is taken, as Markdownlint reads the keys in the order
/// written.
fn markdownlint_default<'a>(rules: &'a [(String, Value)], rule: &str) -> Option<&'a Value> {
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
    /// Its rules, each with its value: Markdownlint's in the order written,
    /// the others' sorted, since none of them reads two keys for one rule.
    rules: Rules,
}

/// How a file is written, told by its name.
#[derive(Clone, Copy)]
pub(crate) enum Format {
    Jsonc,
    Yaml,
    Script,
}

/// The programs look for their files by exact name, so the ending is matched
/// exactly too; a name without one, such as `.stylelintrc`, is YAML.
pub(crate) fn format(file: &RelativePath) -> Format {
    let name = file.file_name().unwrap_or_default();
    match name.rsplit_once('.').map(|(_, ending)| ending) {
        Some("cjs" | "mjs" | "js" | "ts") => Format::Script,
        Some("jsonc" | "json") => Format::Jsonc,
        _ => Format::Yaml,
    }
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// The value a file of html-validate's configuration holds, which is JSON
/// with comments. A file in JavaScript is never handed here.
fn parse(source: &str) -> Result<Value, String> {
    jsonc_parser::parse_to_serde_value(source, &jsonc_parser::ParseOptions::default()).map_err(text)
}

/// A YAML value as the JSON value it stands for. A key that is not text is
/// written out as YAML would show it.
pub(crate) fn json(yaml: &Yaml) -> Value {
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
/// in the order written, beside `extends` and the editor's `$schema`.
fn config(value: Written) -> Result<Config, String> {
    let Written::Mapping(mut rules) = value else {
        return Err(NO_MAPPING.to_owned());
    };
    Written::take(&mut rules, "$schema");
    let extends = extends(Written::take(&mut rules, "extends").map(Written::into_value))?;
    let rules = rules
        .into_iter()
        .map(|(key, value)| (key, value.into_value()))
        .collect();
    Ok(Config { extends, rules })
}

/// The rules of a configuration of Stylelint or html-validate: its `rules`,
/// beside `extends`, which names one configuration or a list of them.
fn rules_of(value: Value) -> Result<Config, String> {
    let Value::Object(mut top) = value else {
        return Err(NO_MAPPING.to_owned());
    };
    let extends = match top.remove("extends") {
        None => None,
        Some(Value::String(name)) => Some(name),
        Some(Value::Array(names)) => Some(
            names
                .iter()
                .map(|name| {
                    name.as_str()
                        .ok_or_else(|| "`extends` holds an entry that is not text".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", "),
        ),
        Some(_) => return Err("`extends` is neither text nor a list of text".to_owned()),
    };
    let rules = match top.remove("rules") {
        None => Rules::new(),
        Some(Value::Object(rules)) => rules.into_iter().collect(),
        Some(_) => return Err("`rules` is not a mapping".to_owned()),
    };
    Ok(Config { extends, rules })
}

/// Where a file's rules stand.
#[derive(Clone, Copy)]
enum Place {
    /// Laid over the default, and over what lies above it.
    Over,
    /// Laid over the default, in place of any configuration in a folder
    /// above it: html-validate's in a folder that says `root: true`.
    Nearest,
}

/// One file of html-validate's configuration that is laid over the default.
struct Layer {
    file: RelativePath,
    place: Place,
}

impl Layer {
    /// The folder whose files it is for: its own.
    fn owner(&self) -> RelativePath {
        folder(&self.file)
    }

    /// What it names, or [`None`] for a file in JavaScript.
    fn read(&self, read: &Read<'_>) -> Result<Option<Config>, String> {
        if let Format::Script = format(&self.file) {
            return Ok(None);
        }
        let source = read(&self.file)?;
        rules_of(parse(&source)?).map(Some)
    }
}

/// A file of a folder, by its name.
fn file_in<'a>(
    files: &[&'a RelativePath],
    dir: &RelativePath,
    name: &str,
) -> Option<&'a RelativePath> {
    let wanted = dir.join(name);
    files.iter().find(|file| ***file == wanted).copied()
}

/// Whether a configuration of html-validate says `root: true`, so that
/// html-validate reads nothing above its folder. Only a JSON file is read; one
/// written in JavaScript or one that does not read is taken not to say it.
fn says_root(file: &RelativePath, read: &Read<'_>) -> bool {
    file.file_name() == Some(".htmlvalidate.json")
        && read(file)
            .ok()
            .and_then(|source| parse(&source).ok())
            .and_then(|value| value.get("root").and_then(Value::as_bool))
            == Some(true)
}

/// Every configuration html-validate reads for the files the html check
/// covers: all of its files in the folder of such a file and in each folder
/// above it, up to a folder whose JSON file says `root: true`, the root first.
/// Such a folder's files lie over the default alone.
fn html_validate_layers(
    files: &[&RelativePath],
    pages: &[&RelativePath],
    read: &Read<'_>,
) -> Vec<Layer> {
    let in_folder = |dir: &RelativePath| -> Vec<&RelativePath> {
        HTML_VALIDATE_FILES
            .iter()
            .filter_map(|name| file_in(files, dir, name))
            .collect()
    };
    let mut folders: BTreeSet<RelativePath> = BTreeSet::new();
    let mut roots: BTreeSet<RelativePath> = BTreeSet::new();
    for page in pages {
        let mut dir = folder(page);
        loop {
            let here = in_folder(&dir);
            if !here.is_empty() {
                folders.insert(dir.clone());
            }
            if here.iter().any(|file| says_root(file, read)) {
                roots.insert(dir.clone());
                break;
            }
            if dir.is_root() {
                break;
            }
            dir = folder(&dir);
        }
    }
    folders
        .iter()
        .flat_map(|dir| {
            let place = if roots.contains(dir) && !dir.is_root() {
                Place::Nearest
            } else {
                Place::Over
            };
            in_folder(dir).into_iter().map(move |file| Layer {
                file: file.clone(),
                place,
            })
        })
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
            Place::Over if layer.owner().is_root() => "laid over the default rule by rule",
            Place::Over => "laid over the default and what lies above it, rule by rule",
            Place::Nearest => {
                "laid over the default rule by rule, in place of any configuration above it"
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
        lines.extend(rule_lines(program, &config.rules, &default));
    }
    lines
}

/// One line for every rule, with how it stands to the default.
fn rule_lines(program: Program, rules: &[(String, Value)], default: &Config) -> Vec<String> {
    rules
        .iter()
        .map(|(rule, value)| {
            let stand = match program.stand(rule, value, default) {
                Stand::Same => "the same as the default".to_owned(),
                Stand::Over(under) => format!("over the default {under}"),
                Stand::Unset => program.unset(default),
                Stand::Mixed => {
                    "the default does not give the rules it covers one value".to_owned()
                }
            };
            format!(
                "{}{}",
                column(10, &shown(rule)),
                shown(&format!("{value} ({stand})"))
            )
        })
        .collect()
}

/// The lines of Markdownlint, which the rules report shows and the Markdown
/// check prints, both from what the check hands over: the file laid over the
/// default and how each of its rules stands to it, each difference by
/// pattern the same way, and every problem with the configuration, at its
/// file.
#[must_use]
pub fn markdownlint(default: &str, handed: &Handed, problems: &[Problem]) -> Vec<String> {
    let mut layers = vec![(
        "laid over the default rule by rule".to_owned(),
        handed.rules.as_slice(),
    )];
    for entry in &handed.overrides {
        let place = match entry.combine {
            Combine::Merge => "laid over the default and the rules above, rule by rule",
            Combine::Replace => "laid over the default rule by rule, in place of the rules above",
        };
        layers.push((
            format!("for {}: {place}", patterns(&entry.filter)),
            entry.rules.as_slice(),
        ));
    }
    handed_lines(
        Program::Markdownlint,
        default,
        handed.file.as_ref(),
        &layers,
        problems,
    )
}

/// The lines of Stylelint, which the rules report shows and the CSS check
/// prints, both from what the check hands over: the file laid over the
/// default and how each of its rules stands to it, each difference by
/// pattern the same way, and every problem with the configuration, at its
/// file.
#[must_use]
pub fn stylelint(default: &str, handed: &styled::Handed, problems: &[Problem]) -> Vec<String> {
    let mut layers = vec![(
        "laid over the default rule by rule".to_owned(),
        handed.rules.as_slice(),
    )];
    for entry in &handed.overrides {
        layers.push((
            format!(
                "for {}: laid over the default and the rules above, rule by rule",
                patterns(&entry.patterns.written)
            ),
            entry.rules.as_slice(),
        ));
    }
    handed_lines(
        Program::Stylelint,
        default,
        handed.file.as_ref(),
        &layers,
        problems,
    )
}

/// Patterns as the report shows them, each in backticks.
fn patterns(written: &[String]) -> String {
    written
        .iter()
        .map(|pattern| format!("`{}`", shown(pattern)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The lines of a linter whose configuration the tool reads in one place
/// ([`crate::linter_config`]): the file laid over the default, each of its
/// layers with the place it takes and how each rule stands to the default,
/// and every problem with the configuration, at its file.
fn handed_lines(
    program: Program,
    default: &str,
    file: Option<&RelativePath>,
    layers: &[(String, &[(String, Value)])],
    problems: &[Problem],
) -> Vec<String> {
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
    let mut lines = Vec::new();
    match file {
        None => lines.push(format!("{head}{name}: the default, nothing laid over it")),
        Some(file) => {
            lines.push(format!(
                "{head}{name}: the default, with these laid over it"
            ));
            let file = column(6, &shown(&file.to_string()));
            for (place, rules) in layers {
                if rules.is_empty() {
                    lines.push(format!("{file}{place}; it names no rule"));
                } else {
                    lines.push(format!("{file}{place}"));
                    lines.extend(rule_lines(program, rules, &default));
                }
            }
        }
    }
    lines.extend(problems.iter().map(|problem| {
        format!(
            "{}{}",
            column(6, &shown(&problem.file.to_string())),
            shown(&problem.message)
        )
    }));
    lines
}

/// Which rules the repository sets for each program with a default, and how
/// each stands to that default. `register` names the files of each linter's
/// configuration the tool reads ([`crate::linter_config`]).
#[must_use]
pub fn report(
    analysis: &Analysis,
    read: &Read<'_>,
    defaults: Defaults<'_>,
    register: &Register,
) -> String {
    let nodes = analysis.tree.files();
    let files: Vec<&RelativePath> = nodes.iter().map(|(node, _)| &node.path).collect();
    let excluded: BTreeSet<&RelativePath> = nodes
        .iter()
        .filter(|(_, coverage)| matches!(coverage, Coverage::Excluded { .. }))
        .map(|(node, _)| &node.path)
        .collect();
    let covered = |program: Program| -> Vec<&RelativePath> {
        nodes
            .iter()
            .filter(|(_, coverage)| {
                matches!(coverage, Coverage::Covered(check) if check.as_str() == program.check())
            })
            .map(|(node, _)| &node.path)
            .collect()
    };
    let mut lines = vec!["Rules".to_owned(), String::new()];
    let kept = |file: &RelativePath| !excluded.contains(file);
    let (handed, problems) = linter_config::markdownlint::read_in(register, &files, &kept, read);
    lines.extend(markdownlint(defaults.markdownlint, &handed, &problems));
    if !covered(Program::Stylelint).is_empty() {
        let (handed, problems) = styled::read_in(register, &files, &kept, read);
        lines.extend(stylelint(defaults.stylelint, &handed, &problems));
    }
    let pages = covered(Program::HtmlValidate);
    if !pages.is_empty() {
        lines.extend(section(
            Program::HtmlValidate,
            defaults.html_validate,
            &html_validate_layers(&files, &pages, read),
            read,
        ));
    }
    if let Some(section) = ruff::section(analysis, &files, read, defaults.ruff) {
        lines.extend(section);
    }
    lines.join("\n")
}
