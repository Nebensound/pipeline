//! Stylelint's configuration, as the CSS check hands it over and the rules
//! report shows it (see [`super`]).
//!
//! The CSS check hands Stylelint a configuration object the tool built, so
//! Stylelint looks for no file of configuration, and every package it loads
//! is named by its place in the pinned install (see `run::css`). The tool
//! reads the repository's file itself ([`read`]): the first of the names the
//! register gives Stylelint that stands at the root, `.stylelintrc` (YAML),
//! `.stylelintrc.json`, `.stylelintrc.yaml` or `.stylelintrc.yml`, or one of
//! them in the folder `.config` there, without the leading dot. A `.json` file
//! is read as JSON, which takes no comment, the others as YAML, as Stylelint
//! read them. The lists below follow the keys of a configuration of Stylelint
//! 18.0.0, the version the register pins.
//!
//! Each key of the file is carried over ([`CARRIED`]), set aside because the
//! check decides it itself ([`SET_ASIDE`]), refused by name with the reason
//! ([`REFUSED`]), or unknown, which fails as well. `defaultSeverity` is
//! carried over as `error` only: `warning` would make every rule that names
//! no severity of its own a warning, which fails nothing, and is refused
//! ([`WARNING`]). Each key of `rules`, at the top and in an entry of
//! `overrides`, is a rule of Stylelint 18.0.0 ([`RULES`]) or of the plugin for
//! SCSS the pipeline's default brings along ([`SCSS_RULES`]); any other key is
//! unknown. The value of a rule is data and is handed over as written:
//! Stylelint checks its options and names a wrong one.
//!
//! A difference by pattern is an entry of `overrides` that holds `files`, its
//! patterns, and `rules`, and may hold `name`, a label; any other key in it
//! fails. Its patterns are read as Stylelint 18.0.0 reads them, from the
//! folder of the file that holds them: from the root for `.stylelintrc*`, and
//! from `.config` for `.config/stylelintrc*`, so that the check and an editor
//! that runs Stylelint on the same file agree ([`Patterns::matches`]). Every
//! entry whose patterns match a file counts for it, in their order.
//!
//! The rules of a file are laid as Stylelint lays them, rule by rule, a later
//! value in place of an earlier one ([`Handed::layer_for`]): the pipeline's
//! default ([`Pinned`]) with the entries of its own `overrides` that match the
//! file, then the repository's rules, then the entries of its `overrides`
//! that match the file. A rule of the plugin for SCSS counts only for a file
//! whose configuration loads the plugin ([`SCSS_PLUGIN_BROUGHT_BY`]), a file
//! read as SCSS; for any other it is left out, wherever it was written.
//! Stylelint takes the value of a rule as a whole and
//! looks each rule up by its name, so the order of the keys carries no
//! meaning; the tool keeps them in the order written all the same.
//!
//! Stylelint would also read the `stylelint` key of a `package.json`, a file
//! of its configuration in any folder below the root, and one written in
//! JavaScript. The tool reads none of them and names each ([`beside`], and
//! [`super::locate`] for the names the register gives).

use globset::{GlobBuilder, GlobMatcher};
use serde_json::Value;

use super::{Problem, Unread, Written, locate};
use crate::ignored::Read;
use crate::names::ProgramName;
use crate::path::RelativePath;
use crate::programs::Program;
use crate::register::Register;
use crate::rules::{self, Format};

/// The name Stylelint goes by in the register.
pub const NAME: &str = "stylelint";

/// The rules of Stylelint 18.0.0, the version the register pins, as the
/// package names them in `lib/rules/index.mjs`. The tool knows them from this
/// list, taken from the pinned package, and loads nothing to learn them. A
/// rule the list holds and the pinned Stylelint does not know, Stylelint
/// names itself.
pub const RULES: &[&str] = &[
    "alpha-value-notation",
    "annotation-no-unknown",
    "at-charset-rule-no-invalid",
    "at-rule-allowed-list",
    "at-rule-descriptor-no-unknown",
    "at-rule-descriptor-value-no-unknown",
    "at-rule-disallowed-list",
    "at-rule-empty-line-before",
    "at-rule-no-deprecated",
    "at-rule-no-unknown",
    "at-rule-no-vendor-prefix",
    "at-rule-prelude-no-invalid",
    "at-rule-property-required-list",
    "block-no-empty",
    "block-no-redundant-nested-style-rules",
    "color-function-alias-notation",
    "color-function-notation",
    "color-hex-alpha",
    "color-hex-length",
    "color-named",
    "color-no-hex",
    "color-no-invalid-hex",
    "comment-empty-line-before",
    "comment-no-empty",
    "comment-pattern",
    "comment-whitespace-inside",
    "comment-word-disallowed-list",
    "container-name-pattern",
    "custom-media-pattern",
    "custom-property-empty-line-before",
    "custom-property-no-missing-var-function",
    "custom-property-pattern",
    "declaration-block-no-duplicate-custom-properties",
    "declaration-block-no-duplicate-properties",
    "declaration-block-no-redundant-longhand-properties",
    "declaration-block-no-shorthand-property-overrides",
    "declaration-empty-line-before",
    "declaration-no-important",
    "declaration-property-custom-property-allowed-list",
    "declaration-property-max-values",
    "declaration-property-unit-allowed-list",
    "declaration-property-unit-disallowed-list",
    "declaration-property-value-allowed-list",
    "declaration-property-value-disallowed-list",
    "declaration-property-value-keyword-no-deprecated",
    "declaration-property-value-no-unknown",
    "display-notation",
    "font-family-name-quotes",
    "font-family-no-duplicate-names",
    "font-family-no-missing-generic-family-keyword",
    "font-weight-notation",
    "function-allowed-list",
    "function-calc-no-unspaced-operator",
    "function-disallowed-list",
    "function-linear-gradient-no-nonstandard-direction",
    "function-name-case",
    "function-no-unknown",
    "function-url-no-scheme-relative",
    "function-url-quotes",
    "function-url-scheme-allowed-list",
    "function-url-scheme-disallowed-list",
    "hue-degree-notation",
    "import-notation",
    "keyframe-block-no-duplicate-selectors",
    "keyframe-declaration-no-important",
    "keyframe-selector-notation",
    "keyframes-name-pattern",
    "layer-name-pattern",
    "length-zero-no-unit",
    "lightness-notation",
    "max-nesting-depth",
    "media-feature-name-allowed-list",
    "media-feature-name-disallowed-list",
    "media-feature-name-no-unknown",
    "media-feature-name-no-vendor-prefix",
    "media-feature-name-unit-allowed-list",
    "media-feature-name-value-allowed-list",
    "media-feature-name-value-no-unknown",
    "media-feature-range-notation",
    "media-query-no-invalid",
    "media-type-no-deprecated",
    "named-grid-areas-no-invalid",
    "nesting-selector-no-missing-scoping-root",
    "no-descending-specificity",
    "no-duplicate-at-import-rules",
    "no-duplicate-selectors",
    "no-empty-source",
    "no-invalid-double-slash-comments",
    "no-invalid-position-at-import-rule",
    "no-invalid-position-declaration",
    "no-irregular-whitespace",
    "no-unknown-animations",
    "no-unknown-custom-media",
    "no-unknown-custom-properties",
    "number-max-precision",
    "property-allowed-list",
    "property-disallowed-list",
    "property-layout-mappings",
    "property-no-deprecated",
    "property-no-unknown",
    "property-no-vendor-prefix",
    "relative-selector-nesting-notation",
    "rule-empty-line-before",
    "rule-nesting-at-rule-required-list",
    "rule-selector-property-disallowed-list",
    "selector-anb-no-unmatchable",
    "selector-attribute-name-disallowed-list",
    "selector-attribute-operator-allowed-list",
    "selector-attribute-operator-disallowed-list",
    "selector-attribute-quotes",
    "selector-class-pattern",
    "selector-combinator-allowed-list",
    "selector-combinator-disallowed-list",
    "selector-disallowed-list",
    "selector-id-pattern",
    "selector-max-attribute",
    "selector-max-class",
    "selector-max-combinators",
    "selector-max-compound-selectors",
    "selector-max-id",
    "selector-max-pseudo-class",
    "selector-max-specificity",
    "selector-max-type",
    "selector-max-universal",
    "selector-nested-pattern",
    "selector-no-deprecated",
    "selector-no-invalid",
    "selector-no-qualifying-type",
    "selector-no-unmatchable",
    "selector-no-vendor-prefix",
    "selector-not-notation",
    "selector-pseudo-class-allowed-list",
    "selector-pseudo-class-disallowed-list",
    "selector-pseudo-class-no-unknown",
    "selector-pseudo-element-allowed-list",
    "selector-pseudo-element-colon-notation",
    "selector-pseudo-element-disallowed-list",
    "selector-pseudo-element-no-unknown",
    "selector-type-case",
    "selector-type-no-unknown",
    "shorthand-property-no-redundant-values",
    "string-no-newline",
    "syntax-string-no-invalid",
    "time-min-milliseconds",
    "unit-allowed-list",
    "unit-disallowed-list",
    "unit-layout-mappings",
    "unit-no-unknown",
    "value-keyword-case",
    "value-keyword-layout-mappings",
    "value-no-invalid",
    "value-no-vendor-prefix",
    "value-type-custom-property-required-list",
];

/// The rules of stylelint-scss 7.5.0, the plugin the pipeline's default
/// brings along for `.scss` through `stylelint-config-standard-scss`, as the
/// plugin names them in `src/rules/index.js`; a configuration writes each as
/// `scss/<name>`. The register pins the configuration that brings it, not the
/// plugin itself.
pub const SCSS_RULES: &[&str] = &[
    "at-each-key-value-single-line",
    "at-else-closing-brace-newline-after",
    "at-else-closing-brace-space-after",
    "at-else-empty-line-before",
    "at-else-if-parentheses-space-before",
    "at-extend-no-missing-placeholder",
    "at-function-named-arguments",
    "at-function-parentheses-space-before",
    "at-function-pattern",
    "at-if-closing-brace-newline-after",
    "at-if-closing-brace-space-after",
    "at-if-no-null",
    "at-import-partial-extension-allowed-list",
    "at-import-partial-extension-disallowed-list",
    "at-mixin-argumentless-call-parentheses",
    "at-mixin-named-arguments",
    "at-mixin-no-risky-nesting-selector",
    "at-mixin-parentheses-space-before",
    "at-mixin-pattern",
    "at-root-no-redundant",
    "at-rule-conditional-no-parentheses",
    "at-rule-no-unknown",
    "at-use-no-redundant-alias",
    "at-use-no-unnamespaced",
    "block-no-redundant-nesting",
    "comment-no-empty",
    "comment-no-loud",
    "declaration-nested-properties",
    "declaration-nested-properties-no-divided-groups",
    "declaration-property-value-no-unknown",
    "dimension-no-non-numeric-values",
    "dollar-variable-colon-newline-after",
    "dollar-variable-colon-space-after",
    "dollar-variable-colon-space-before",
    "dollar-variable-default",
    "dollar-variable-empty-line-after",
    "dollar-variable-empty-line-before",
    "dollar-variable-first-in-block",
    "dollar-variable-no-missing-interpolation",
    "dollar-variable-no-namespaced-assignment",
    "dollar-variable-pattern",
    "double-slash-comment-empty-line-before",
    "double-slash-comment-inline",
    "double-slash-comment-whitespace-inside",
    "function-calculation-no-interpolation",
    "function-color-channel",
    "function-color-relative",
    "function-disallowed-list",
    "function-no-unknown",
    "function-quote-no-quoted-strings-inside",
    "function-unquote-no-unquoted-strings-inside",
    "load-no-partial-leading-underscore",
    "load-partial-extension",
    "map-keys-quotes",
    "media-feature-value-dollar-variable",
    "no-dollar-variables",
    "no-duplicate-dollar-variables",
    "no-duplicate-load-rules",
    "no-duplicate-mixins",
    "no-global-function-names",
    "no-unused-private-members",
    "operator-no-newline-after",
    "operator-no-newline-before",
    "operator-no-unspaced",
    "partial-no-import",
    "percent-placeholder-pattern",
    "property-no-unknown",
    "selector-class-pattern",
    "selector-nest-combinators",
    "selector-no-redundant-nesting-selector",
    "selector-no-union-class-name",
];

/// The package of the pipeline's default that brings the plugin for SCSS
/// along, for the files it extends. A file whose configuration extends it
/// not, a `.css` file, has no rule of the plugin: Stylelint would report one
/// handed to it as unknown, even one switched off, so a rule of
/// [`SCSS_RULES`] counts only where this package is extended
/// ([`Handed::layer_for`]).
pub const SCSS_PLUGIN_BROUGHT_BY: &str = "stylelint-config-standard-scss";

/// Whether a key of `rules` names a rule the tool carries over: one of
/// [`RULES`], or one of [`SCSS_RULES`] written `scss/<name>`.
#[must_use]
pub fn is_rule(key: &str) -> bool {
    RULES.contains(&key)
        || key
            .strip_prefix("scss/")
            .is_some_and(|name| SCSS_RULES.contains(&name))
}

/// The keys of a configuration the check carries over: the rules, the
/// differences by pattern, and the options that are data and fail nothing
/// that would fail without them: what the severity of a rule without one is,
/// whether comments in a stylesheet may switch rules off, what such comments
/// start with, whether such comments are reported, and what the syntax knows
/// beyond the standard.
pub const CARRIED: &[&str] = &[
    "configurationComment",
    "defaultSeverity",
    "ignoreDisables",
    "languageOptions",
    "overrides",
    "reportDescriptionlessDisables",
    "reportInvalidScopeDisables",
    "reportNeedlessDisables",
    "reportUnscopedDisables",
    "rules",
];

/// The keys of a configuration the check reads and does not hand on, each
/// with why. Phase 0 holds `ignoreFiles` against the declaration (see
/// [`crate::ignored`]).
pub const SET_ASIDE: &[(&str, &str)] = &[
    ("$schema", "names the schema of the file for an editor"),
    (
        "allowEmptyInput",
        "the check lints the files `nebensound-repo files` names",
    ),
    ("cache", "the check keeps no cache between runs"),
    (
        "ignoreFiles",
        "the check lints the files `nebensound-repo files` names, and phase 0 holds what it \
         leaves out against the declaration",
    ),
    (
        "maxWarnings",
        "the check fails on every error and on no warning, as Stylelint's command line does \
         without it",
    ),
    (
        "validate",
        "the check has Stylelint check the options of every rule",
    ),
];

/// The keys of a configuration the check refuses, each with what it would
/// make Stylelint do: load and run code, read another file, or pass a finding
/// unreported.
pub const REFUSED: &[(&str, &str)] = &[
    (
        "customSyntax",
        "names a module that would read the stylesheets, which would be loaded and run, and the \
         CSS check runs no code of the repository",
    ),
    (
        "extends",
        "names further configuration, a module or another file, which would be loaded, and the \
         tool reads one file",
    ),
    (
        "fix",
        "makes Stylelint fix what it can and report only the rest, so that a finding it can fix \
         would pass unreported",
    ),
    (
        "formatter",
        "names a module that would print the findings, and the tool prints what the check finds",
    ),
    (
        "plugins",
        "names modules of rules, which would be loaded and run, and the CSS check runs no code of \
         the repository",
    ),
    (
        "processors",
        "names modules that would rewrite what is linted, which would be loaded and run, and the \
         CSS check runs no code of the repository",
    ),
    (
        "quiet",
        "would drop every warning from what the check shows",
    ),
    (
        "referenceFiles",
        "names further files to read, and modules to read them with, and the tool reads one file",
    ),
];

/// Why `defaultSeverity: warning` is refused.
pub const WARNING: &str = "`defaultSeverity: warning` is not handed to Stylelint: it would make \
                           every rule that names no severity of its own a warning, which fails \
                           nothing, and so switch the failing of the CSS check off";

/// Rules, each key with its value, in the order they are written.
pub type Rules = Vec<(String, Value)>;

/// One pattern of an entry of `overrides`, built.
#[derive(Debug, Clone)]
struct Pattern {
    /// Whether it had a leading `!`.
    negated: bool,
    /// The pattern without `!`, joined onto the folder of the file that holds
    /// it, as a path from the root; [`None`] where it climbs above the root,
    /// so that it matches no file there.
    glob: Option<GlobMatcher>,
    /// The pattern as written, without a leading `./`, matched against a
    /// file's name; [`None`] for one with `!`.
    by_name: Option<GlobMatcher>,
}

/// The patterns of an entry of `overrides`.
#[derive(Debug, Clone)]
pub struct Patterns {
    /// As written.
    pub written: Vec<String>,
    /// Built.
    built: Vec<Pattern>,
}

impl Patterns {
    /// Whether they match a file, a path from the repository root, as
    /// Stylelint 18.0.0 matches the patterns of an entry of `overrides`
    /// (`compileOverrideMatchers`, with picomatch, dots included): one of
    /// them matches it. A pattern is joined onto the folder of the file that
    /// holds it, as Node's `path.join` joins them, and matches the path from
    /// the root, where `*` stops at a `/` and `**` does not. Every pattern
    /// without `!` also matches the file's name as written, so one without a
    /// `/` matches in any folder. One with a leading `!` matches every path
    /// its pattern does not, so that beside others it takes nothing back.
    #[must_use]
    pub fn matches(&self, file: &str) -> bool {
        let name = file.rsplit('/').next().unwrap_or(file);
        self.built.iter().any(|pattern| {
            let at_path = pattern
                .glob
                .as_ref()
                .is_some_and(|glob| glob.is_match(file));
            if pattern.negated {
                !at_path
            } else {
                at_path
                    || pattern
                        .by_name
                        .as_ref()
                        .is_some_and(|glob| glob.is_match(name))
            }
        })
    }
}

/// A pattern without the `./` it starts with, however often it is written.
fn from_root(mut pattern: &str) -> &str {
    while let Some(rest) = pattern.strip_prefix("./") {
        pattern = rest;
    }
    pattern
}

/// A pattern joined onto `base`, a folder from the root, as Node's
/// `path.join` joins them: an empty segment and `.` are dropped, `..` takes
/// the segment before it off, and a trailing `/` stays. [`None`] where `..`
/// climbs above the root. A pattern that starts with `/` is a path of its
/// own and is not joined, as with `path.join`'s caller in Stylelint.
fn joined(base: &[String], pattern: &str) -> Option<String> {
    if pattern.starts_with('/') {
        return Some(pattern.to_owned());
    }
    let mut segments: Vec<&str> = base.iter().map(String::as_str).collect();
    for segment in pattern.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    let mut path = segments.join("/");
    if pattern.ends_with('/') {
        path.push('/');
    }
    Some(path)
}

/// A pattern built as a glob, where `*` stops at a `/`.
fn glob(pattern: &str) -> Result<GlobMatcher, String> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|error| format!("holds a pattern that does not read: {error}"))
}

/// The patterns of `files`, built: a pattern or a list of them that holds one
/// at least, each a pattern that reads. `base` is the folder of the file that
/// holds them, from the root.
fn patterns(value: Written, base: &[String]) -> Result<Patterns, String> {
    let not = || "is neither a pattern nor a list of patterns".to_owned();
    let written: Vec<String> = match value.into_value() {
        Value::String(one) => vec![one],
        Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                Value::String(pattern) => Ok(pattern),
                _ => Err(not()),
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(not()),
    };
    if written.is_empty() {
        return Err("holds no pattern".to_owned());
    }
    let built = written
        .iter()
        .map(|pattern| {
            let (negated, rest) = match pattern.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, pattern.as_str()),
            };
            Ok(Pattern {
                negated,
                glob: joined(base, rest).as_deref().map(glob).transpose()?,
                by_name: if negated {
                    None
                } else {
                    Some(glob(from_root(rest))?)
                },
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(Patterns { written, built })
}

/// Lays rules over others, key by key, as Stylelint does with JavaScript's
/// spread: a key written beneath takes the new value in its place, a new key
/// comes last, in the order written.
fn lay(laid: &mut Rules, over: &[(String, Value)]) {
    for (key, value) in over {
        match laid.iter_mut().find(|(beneath, _)| beneath == key) {
            Some(slot) => slot.1 = value.clone(),
            None => laid.push((key.clone(), value.clone())),
        }
    }
}

/// A part of the pipeline's default, or what it comes to for a file: the
/// packages it extends, the package it reads the files with, and its rules.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layer {
    /// The configurations it extends, by package, in order.
    pub extends: Vec<String>,
    /// The syntax it reads the files with, by package.
    pub custom_syntax: Option<String>,
    /// Its rules, in the order written.
    pub rules: Rules,
}

/// The pipeline's default (`rules/stylelint.yaml`), as the tool reads it:
/// what it extends and its rules, and the entries of its `overrides`. The
/// packages it names are those the register pins with Stylelint, named by
/// package; the CSS check hands each to Stylelint by its place in the pinned
/// install.
#[derive(Debug, Clone, Default)]
pub struct Pinned {
    /// What holds for every file.
    pub base: Layer,
    /// The entries of `overrides`, each with its patterns.
    pub overrides: Vec<(Patterns, Layer)>,
}

/// Reads the pipeline's default from its text. It holds `extends`,
/// `customSyntax` and `rules`, at the top and in an entry of `overrides`,
/// which holds `files` as well.
///
/// # Errors
///
/// Where it does not read, or holds anything else, naming what; the default
/// compiled into the tool reads, and a test holds it to that.
pub fn pinned(text: &str) -> Result<Pinned, String> {
    let Written::Mapping(mut top) = super::parse(Format::Yaml, text)? else {
        return Err("it holds no mapping".to_owned());
    };
    let overrides = match Written::take(&mut top, "overrides") {
        None => Vec::new(),
        Some(Written::List(entries)) => entries
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                let at = format!("overrides[{index}]");
                let Written::Mapping(mut entry) = entry else {
                    return Err(format!("`{at}` is not a mapping"));
                };
                let files = Written::take(&mut entry, "files")
                    .ok_or_else(|| format!("`{at}` has no `files`"))?;
                let patterns =
                    patterns(files, &[]).map_err(|reason| format!("`{at}.files` {reason}"))?;
                Ok((patterns, layer(entry, &format!("{at}."))?))
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("`overrides` is not a list".to_owned()),
    };
    Ok(Pinned {
        base: layer(top, "")?,
        overrides,
    })
}

/// A part of the pipeline's default; `at` is where it stands, in front of
/// every key an error names.
fn layer(entries: Vec<(String, Written)>, at: &str) -> Result<Layer, String> {
    let mut layer = Layer::default();
    for (key, value) in entries {
        match (key.as_str(), value) {
            ("extends", Written::Scalar(Value::String(name))) => layer.extends = vec![name],
            ("extends", Written::List(names)) => {
                layer.extends = names
                    .into_iter()
                    .map(|name| match name {
                        Written::Scalar(Value::String(name)) => Ok(name),
                        _ => Err(format!("`{at}extends` holds an entry that is not text")),
                    })
                    .collect::<Result<_, _>>()?;
            }
            ("customSyntax", Written::Scalar(Value::String(name))) => {
                layer.custom_syntax = Some(name);
            }
            ("rules", Written::Mapping(rules)) => {
                layer.rules = rules
                    .into_iter()
                    .map(|(rule, value)| (rule, value.into_value()))
                    .collect();
            }
            ("extends" | "customSyntax" | "rules", _) => {
                return Err(format!("`{at}{key}` does not hold what it takes"));
            }
            _ => {
                return Err(format!(
                    "`{at}{key}` is no key the tool reads in its default"
                ));
            }
        }
    }
    Ok(layer)
}

/// A difference by pattern: an entry of `overrides`.
#[derive(Debug, Clone)]
pub struct Override {
    /// Its patterns.
    pub patterns: Patterns,
    /// Its rules, in the order written.
    pub rules: Rules,
}

/// What the CSS check hands Stylelint, beside the pipeline's default.
#[derive(Debug, Clone, Default)]
pub struct Handed {
    /// The file it was read from; [`None`] where the repository has none, or
    /// its file holds no mapping.
    pub file: Option<RelativePath>,
    /// The rules of the file, in the order written.
    pub rules: Rules,
    /// The options it carries over beside the rules ([`CARRIED`]), in the
    /// order written.
    pub options: Vec<(String, Value)>,
    /// The differences by pattern, in their order.
    pub overrides: Vec<Override>,
}

impl Handed {
    /// What Stylelint is handed for a file, a path from the repository root:
    /// the packages of the pipeline's default it extends and the syntax it
    /// reads the file with, from the default and the entries of the default's
    /// `overrides` that match the file, each later one appended to what it
    /// extends and in place of the syntax; and the rules, laid as [`super`]
    /// says. A rule of the plugin for SCSS, written `scss/<name>`, is left
    /// out where the file does not extend [`SCSS_PLUGIN_BROUGHT_BY`], which
    /// loads the plugin, wherever it was written: such a rule counts only for
    /// the files read as SCSS.
    #[must_use]
    pub fn layer_for(&self, pinned: &Pinned, file: &str) -> Layer {
        let mut laid = pinned.base.clone();
        for (patterns, layer) in &pinned.overrides {
            if patterns.matches(file) {
                laid.extends.extend(layer.extends.iter().cloned());
                if layer.custom_syntax.is_some() {
                    laid.custom_syntax.clone_from(&layer.custom_syntax);
                }
                lay(&mut laid.rules, &layer.rules);
            }
        }
        lay(&mut laid.rules, &self.rules);
        for entry in &self.overrides {
            if entry.patterns.matches(file) {
                lay(&mut laid.rules, &entry.rules);
            }
        }
        if !laid
            .extends
            .iter()
            .any(|name| name == SCSS_PLUGIN_BROUGHT_BY)
        {
            laid.rules.retain(|(rule, _)| !rule.starts_with("scss/"));
        }
        laid
    }

    /// What the script of the CSS check is handed, as JSON: the files in
    /// groups of the same configuration, each group with the packages it
    /// extends and the syntax, by package, and the configuration itself: the
    /// options, in the order written, and the rules.
    #[must_use]
    pub fn job(&self, pinned: &Pinned, files: &[String]) -> String {
        let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
        for file in files {
            let layer = self.layer_for(pinned, file);
            let mut entries: Vec<String> = self
                .options
                .iter()
                .map(|(key, value)| format!("{}:{value}", Value::from(key.as_str())))
                .collect();
            entries.push(format!("\"rules\":{}", object(&layer.rules)));
            let group = format!(
                "\"extends\":{},\"customSyntax\":{},\"config\":{{{}}}",
                Value::from(layer.extends),
                layer.custom_syntax.map_or(Value::Null, Value::from),
                entries.join(",")
            );
            match groups.iter_mut().find(|(same, _)| *same == group) {
                Some((_, members)) => members.push(file),
                None => groups.push((group, vec![file])),
            }
        }
        let groups: Vec<String> = groups
            .into_iter()
            .map(|(group, members)| format!("{{{group},\"files\":{}}}", Value::from(members)))
            .collect();
        format!("{{\"groups\":[{}]}}", groups.join(","))
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

/// Reads Stylelint's configuration with the names the register gives its
/// files, as [`read`] does; a register without Stylelint names none, and then
/// nothing is read.
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

/// Reads Stylelint's configuration, the program `name` of the register,
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
    match read(file).and_then(|source| parse(file, &source)) {
        Ok(Written::Mapping(top)) => {
            handed.file = Some(file.clone());
            keys(&mut found, &mut handed, top);
        }
        // An empty file of YAML holds nothing, and so lays nothing over the
        // default.
        Ok(Written::Scalar(Value::Null)) => handed.file = Some(file.clone()),
        Ok(_) => found.problem("holds no mapping, so nothing of it is handed to Stylelint"),
        Err(error) => found.problem(&format!(
            "cannot be read, so nothing of it is handed to Stylelint: {error}"
        )),
    }
    problems.extend(found.problems);
    (handed, problems)
}

/// The value a file of Stylelint's configuration holds, its keys in the
/// order written, read as Stylelint read it: a `.json` file as JSON, which
/// takes no comment, any other as YAML.
fn parse(file: &RelativePath, source: &str) -> Result<Written, String> {
    match rules::format(file) {
        Format::Jsonc => serde_json::from_str(source).map_err(|error| error.to_string()),
        Format::Yaml | Format::Script => super::parse(Format::Yaml, source),
    }
}

/// The keys of the file.
fn keys(found: &mut Found<'_>, handed: &mut Handed, top: Vec<(String, Written)>) {
    for (key, value) in top {
        match key.as_str() {
            "rules" => handed.rules = found.rules(value, ""),
            "overrides" => match value {
                Written::List(entries) => {
                    handed.overrides = entries
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, entry)| found.entry(index, entry))
                        .collect();
                }
                _ => found.problem("`overrides` is not a list, so it is not handed to Stylelint"),
            },
            "defaultSeverity" => match value.into_value() {
                Value::String(word) if word == "error" => {
                    handed.options.push((key, Value::String(word)));
                }
                Value::String(word) if word == "warning" => found.problem(WARNING),
                _ => found.problem(
                    "`defaultSeverity` is neither `error` nor `warning`, so it is not handed to \
                     Stylelint",
                ),
            },
            _ if CARRIED.contains(&key.as_str()) => handed.options.push((key, value.into_value())),
            _ if SET_ASIDE.iter().any(|(name, _)| *name == key) => {}
            _ => match REFUSED.iter().find(|(name, _)| *name == key) {
                Some((_, why)) => {
                    found.problem(&format!("`{key}` is not handed to Stylelint: it {why}"));
                }
                None => found.problem(&format!(
                    "`{key}` is not a setting the CSS check knows, so it is not handed to Stylelint"
                )),
            },
        }
    }
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

    /// The rules of a value, each key checked, in the order written; `at` is
    /// where it stands, such as `overrides[0].`, in front of every key a
    /// problem names.
    fn rules(&mut self, value: Written, at: &str) -> Rules {
        let Written::Mapping(written) = value else {
            self.problem(&format!(
                "`{at}rules` is not a mapping of rules, so it is not handed to Stylelint"
            ));
            return Rules::new();
        };
        let mut rules = Rules::new();
        for (key, value) in written {
            if is_rule(&key) {
                rules.push((key, value.into_value()));
            } else {
                self.problem(&format!(
                    "`{at}rules.{key}` is no rule of Stylelint {} or of its plugin for SCSS, so it \
                     is not handed to Stylelint",
                    self.version
                ));
            }
        }
        rules
    }

    /// An entry of `overrides`, or [`None`] where it cannot be used, which is
    /// a problem. Any key beside `files`, `rules` and `name` is a problem,
    /// and the rest of the entry is still handed over.
    fn entry(&mut self, index: usize, value: Written) -> Option<Override> {
        let at = format!("overrides[{index}]");
        let Written::Mapping(mut entry) = value else {
            self.problem(&format!(
                "`{at}` is not a mapping, so it is not handed to Stylelint"
            ));
            return None;
        };
        let files = Written::take(&mut entry, "files");
        let rules = Written::take(&mut entry, "rules");
        Written::take(&mut entry, "name");
        for (key, _) in &entry {
            match REFUSED.iter().find(|(name, _)| name == key) {
                Some((_, why)) => {
                    self.problem(&format!(
                        "`{at}.{key}` is not handed to Stylelint: it {why}"
                    ));
                }
                None => self.problem(&format!(
                    "`{at}.{key}` is not handed to Stylelint: an entry of `overrides` carries \
                     rules for patterns, and nothing else"
                )),
            }
        }
        let Some(files) = files else {
            self.problem(&format!(
                "`{at}` has no `files`, so it is not handed to Stylelint"
            ));
            return None;
        };
        let base = self.file.segments();
        let patterns = match patterns(files, &base[..base.len().saturating_sub(1)]) {
            Ok(patterns) => patterns,
            Err(reason) => {
                self.problem(&format!(
                    "`{at}.files` {reason}, so the entry is not handed to Stylelint"
                ));
                return None;
            }
        };
        let rules = rules.map_or_else(Rules::new, |rules| self.rules(rules, &format!("{at}.")));
        Some(Override { patterns, rules })
    }
}

/// The files beside those the register names that Stylelint would read and
/// the tool does not, each with why: a `package.json` whose `stylelint` key
/// holds a configuration, wherever it lies. `kept` says which files the
/// repository keeps in its checks: one an `[[exclude]]` entry takes out is
/// never named, as with [`super::locate`].
#[must_use]
pub fn beside(
    files: &[&RelativePath],
    kept: &dyn Fn(&RelativePath) -> bool,
    read: &Read<'_>,
) -> Vec<Unread> {
    files
        .iter()
        .filter(|file| kept(file))
        .filter(|file| file.file_name() == Some("package.json") && holds_configuration(file, read))
        .map(|file| Unread {
            file: (*file).clone(),
            why: format!(
                "not read: the tool reads the configuration of {NAME} only as data, from one file \
                 at the root of the repository, and not from the `stylelint` key of `package.json`"
            ),
        })
        .collect()
}

/// Whether a `package.json` holds a configuration of Stylelint: a `stylelint`
/// key that is not null, as Stylelint's loader asks it. One that does not
/// read holds none.
fn holds_configuration(file: &RelativePath, read: &Read<'_>) -> bool {
    read(file)
        .ok()
        .and_then(|source| serde_json::from_str::<Value>(&source).ok())
        .and_then(|value| value.get("stylelint").cloned())
        .is_some_and(|value| !value.is_null())
}
