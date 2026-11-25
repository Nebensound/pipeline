//! Prettier's options, as every check that formats with Prettier hands them
//! over (see [`super`]): the checks for CSS, HTML, JSON, Markdown and YAML.
//!
//! Prettier is started with `--no-config`, so it looks for no file of
//! configuration, reads no `.editorconfig` and loads no plugin, and with
//! `--ignore-path` naming an empty file, so it reads no ignore file either
//! (see `Session::prettier` in `run`). What it is to format with, it is
//! handed on the command line, one flag per option ([`Handed::groups`]). The
//! tool works the options out for each file from two files at the root of
//! the repository, in the order Prettier's documentation gives them:
//!
//! 1. `.editorconfig` ([`super::editorconfig`]): `indent_style`,
//!    `indent_size` and `tab_width`, `max_line_length`, `quote_type` and
//!    `end_of_line`, mapped as Prettier 3.9.6 maps them
//!    ([`from_editorconfig`]).
//! 2. Prettier's own file, the first of the names the register gives
//!    Prettier at the root (`configuration`), in a format of data, laid over
//!    what `.editorconfig` says; then each entry of its `overrides` whose
//!    patterns match the file, in their order, each laid over what came
//!    before it.
//!
//! Each key of the file is carried over ([`CARRIED`]), refused by name with
//! the reason ([`REFUSED`]), or unknown, which fails as well; `$schema`, for an
//! editor, is passed over. A file that holds text in place of a mapping names
//! a shared configuration, a module, and is refused. The lists follow the
//! options Prettier 3.9.6 gives, the version the register pins.
//!
//! An entry of `overrides` holds `files`, `excludeFiles` and `options`, and
//! matches a file as Prettier matches it, over the path from the repository
//! root: a pattern without a `/` matches the file's name, one with a `/` the
//! whole path; `*` stops at a `/`, `**` does not, a leading `!` takes the
//! match back, a leading `./` is taken off; `excludeFiles` takes the files it
//! matches out of the entry, read the same way as the patterns it stands
//! beside ([`Override::matches`]).
//!
//! Prettier would also read a configuration in JavaScript, the `prettier`
//! key of a `package.json` or `package.yaml`, a file of its configuration in
//! any folder below the root, and an `.editorconfig` there. The tool reads
//! none of them and names each but the last ([`beside`], and
//! [`super::locate`] for the names the register gives): every `.editorconfig`
//! is taken out by an `[[exclude]]` entry of the pipeline's defaults, and a
//! file taken out is not named.

use std::collections::BTreeMap;

use globset::{GlobBuilder, GlobMatcher};
use serde_json::Value;

use super::editorconfig::{self, EditorConfig, Property};
use super::{Problem, Unread, Written, locate};
use crate::ignored::Read;
use crate::names::ProgramName;
use crate::path::RelativePath;
use crate::programs::Program;
use crate::register::Register;
use crate::rules::Format;

/// The name Prettier goes by in the register.
pub const NAME: &str = "prettier";

/// What values an option takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// True or false.
    Flag,
    /// A whole number of 0 or more.
    Count,
    /// One of these words.
    Choice(&'static [&'static str]),
}

/// The options of Prettier the checks carry over, each with the values it
/// takes: every option of Prettier 3.9.6 that says how a file is written.
pub const CARRIED: &[(&str, Kind)] = &[
    ("arrowParens", Kind::Choice(&["always", "avoid"])),
    ("bracketSameLine", Kind::Flag),
    ("bracketSpacing", Kind::Flag),
    ("embeddedLanguageFormatting", Kind::Choice(&["auto", "off"])),
    ("endOfLine", Kind::Choice(&["lf", "crlf", "cr", "auto"])),
    (
        "experimentalOperatorPosition",
        Kind::Choice(&["start", "end"]),
    ),
    ("experimentalTernaries", Kind::Flag),
    (
        "htmlWhitespaceSensitivity",
        Kind::Choice(&["css", "strict", "ignore"]),
    ),
    ("insertPragma", Kind::Flag),
    ("jsxBracketSameLine", Kind::Flag),
    ("jsxSingleQuote", Kind::Flag),
    ("objectWrap", Kind::Choice(&["preserve", "collapse"])),
    ("printWidth", Kind::Count),
    ("proseWrap", Kind::Choice(&["always", "never", "preserve"])),
    (
        "quoteProps",
        Kind::Choice(&["as-needed", "consistent", "preserve"]),
    ),
    ("semi", Kind::Flag),
    ("singleAttributePerLine", Kind::Flag),
    ("singleQuote", Kind::Flag),
    ("tabWidth", Kind::Count),
    ("trailingComma", Kind::Choice(&["all", "es5", "none"])),
    ("useTabs", Kind::Flag),
    ("vueIndentScriptAndStyle", Kind::Flag),
];

/// The keys the checks refuse, each with what it would make Prettier do:
/// load and run code, read another file, choose how a file is read, or pass
/// a file or a part of it unchecked.
pub const REFUSED: &[(&str, &str)] = &[
    (
        "checkIgnorePragma",
        "would pass every file that holds a `@noformat` comment unchecked",
    ),
    (
        "cursorOffset",
        "places an editor's cursor, and the check formats whole files",
    ),
    (
        "filepath",
        "names another file, by whose name Prettier would choose how to read a file",
    ),
    (
        "parser",
        "chooses how Prettier reads a file, which the check leaves to the file's name, and \
         names a parser a plugin would bring",
    ),
    (
        "pluginSearchDirs",
        "names folders plugins would be loaded from, and the check runs no code of the \
         repository",
    ),
    (
        "plugins",
        "names modules that would be loaded and run, and the check runs no code of the \
         repository",
    ),
    (
        "rangeEnd",
        "would leave the part of every file after it unchecked",
    ),
    (
        "rangeStart",
        "would leave the part of every file before it unchecked",
    ),
    (
        "requirePragma",
        "would pass every file that holds no `@format` comment unchecked",
    ),
];

/// The value of an option, as the tool hands it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Setting {
    /// True or false.
    Flag(bool),
    /// A whole number.
    Count(u64),
    /// No limit: what `max_line_length = off` makes of `printWidth`.
    Unbounded,
    /// One of the words the option takes.
    Choice(String),
}

impl Setting {
    /// The value as the command line writes it.
    fn written(&self) -> String {
        match self {
            Self::Flag(flag) => flag.to_string(),
            Self::Count(count) => count.to_string(),
            Self::Unbounded => "Infinity".to_owned(),
            Self::Choice(word) => word.clone(),
        }
    }
}

/// Options, each by its name, in the order of their names.
pub type Options = BTreeMap<String, Setting>;

/// The flag that hands an option to Prettier: its name in lower case, a `-`
/// before each capital, then `=` and the value, all in one argument.
#[must_use]
pub fn flag(name: &str, setting: &Setting) -> String {
    let mut flag = String::from("--");
    for char in name.chars() {
        if char.is_ascii_uppercase() {
            flag.push('-');
            flag.push(char.to_ascii_lowercase());
        } else {
            flag.push(char);
        }
    }
    format!("{flag}={}", setting.written())
}

/// What `.editorconfig` says of a file, as Prettier's options, mapped as
/// Prettier 3.9.6 maps them: `indent_style` gives `useTabs` (`space`: false,
/// `tab`: true, and an `indent_size` of `tab` gives true as well); then
/// `tabWidth` is `indent_size` where `useTabs` came out false, else
/// `tab_width`, each only as a whole number above 0; `max_line_length` gives
/// `printWidth`, `off` no limit; `quote_type` gives `singleQuote` (`single`
/// or `double`); `end_of_line` gives `endOfLine` (`lf`, `crlf` or `cr`).
#[must_use]
pub fn from_editorconfig(config: &EditorConfig, file: &str) -> Options {
    let properties = config.properties(file);
    let get = |name: &str| {
        properties
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    };
    let text = |name: &str| match get(name) {
        Some(Property::Text(text)) => Some(text.as_str()),
        _ => None,
    };
    let mut options = Options::new();
    let use_tabs = match text("indent_style") {
        Some("space") => Some(false),
        Some("tab") => Some(true),
        _ if text("indent_size") == Some("tab") => Some(true),
        _ => None,
    };
    if let Some(use_tabs) = use_tabs {
        options.insert("useTabs".to_owned(), Setting::Flag(use_tabs));
    }
    let size = get("indent_size").and_then(Property::positive);
    let width = get("tab_width").and_then(Property::positive);
    let tab_width = match (use_tabs, size) {
        (Some(false), Some(size)) => Some(size),
        _ => width,
    };
    if let Some(tab_width) = tab_width {
        options.insert("tabWidth".to_owned(), Setting::Count(tab_width));
    }
    let print_width = if text("max_line_length") == Some("off") {
        Some(Setting::Unbounded)
    } else {
        get("max_line_length")
            .and_then(Property::positive)
            .map(Setting::Count)
    };
    if let Some(print_width) = print_width {
        options.insert("printWidth".to_owned(), print_width);
    }
    match text("quote_type") {
        Some("single") => {
            options.insert("singleQuote".to_owned(), Setting::Flag(true));
        }
        Some("double") => {
            options.insert("singleQuote".to_owned(), Setting::Flag(false));
        }
        _ => {}
    }
    if let Some(end @ ("lf" | "crlf" | "cr")) = text("end_of_line") {
        options.insert("endOfLine".to_owned(), Setting::Choice(end.to_owned()));
    }
    options
}

/// One pattern of `files` or `excludeFiles`.
#[derive(Debug, Clone)]
struct Pattern {
    /// As written: a path that is the same text matches whatever the
    /// pattern means, as micromatch, which Prettier matches with, has it.
    written: String,
    /// Whether it holds a `/`: then it matches the whole path, else the
    /// file's name.
    slashes: bool,
    /// Whether a leading `!` takes its match back.
    negated: bool,
    /// The pattern built, without the `!` and a leading `./`.
    glob: GlobMatcher,
}

impl Pattern {
    /// Whether it matches `path`, a path from the repository root, read
    /// against `target`: the path itself, or the file's name.
    fn hits(&self, path: &str, target: &str) -> bool {
        self.written == path || self.glob.is_match(target) != self.negated
    }
}

/// A difference by pattern: an entry of `overrides`.
#[derive(Debug, Clone)]
pub struct Override {
    /// Its `files`.
    files: Vec<Pattern>,
    /// Its `excludeFiles`.
    exclude: Vec<Pattern>,
    /// Its options.
    pub options: Options,
}

impl Override {
    /// Whether it counts for a file, a path from the repository root, as
    /// Prettier decides it: the patterns without a `/` are matched against
    /// the file's name and the others against the whole path, each group
    /// with `excludeFiles` read the same way, and either group may match.
    #[must_use]
    pub fn matches(&self, file: &str) -> bool {
        let name = file.rsplit('/').next().unwrap_or(file);
        let group = |slashes: bool| {
            let target = if slashes { file } else { name };
            self.files
                .iter()
                .filter(|pattern| pattern.slashes == slashes)
                .any(|pattern| pattern.hits(file, target))
                && !self
                    .exclude
                    .iter()
                    .any(|pattern| pattern.hits(file, target))
        };
        group(false) || group(true)
    }
}

/// What a check hands Prettier.
#[derive(Debug, Clone, Default)]
pub struct Handed {
    /// Prettier's own file it was read from; [`None`] where the repository
    /// has none.
    pub file: Option<RelativePath>,
    /// `.editorconfig` at the root, where the repository has one that reads.
    pub editorconfig: Option<EditorConfig>,
    /// The options of the file.
    pub options: Options,
    /// The differences by pattern, in their order.
    pub overrides: Vec<Override>,
}

impl Handed {
    /// Whether the repository configures Prettier at all: without, Prettier
    /// formats with its own defaults, as it did before the tool read its
    /// configuration.
    #[must_use]
    pub fn configured(&self) -> bool {
        self.file.is_some() || self.editorconfig.is_some()
    }

    /// The options for a file, a path from the repository root: what
    /// `.editorconfig` says of it, the file's options laid over that, then
    /// those of every entry of `overrides` that matches it, in order.
    #[must_use]
    pub fn options_for(&self, file: &str) -> Options {
        let mut options = self
            .editorconfig
            .as_ref()
            .map(|config| from_editorconfig(config, file))
            .unwrap_or_default();
        options.extend(self.options.clone());
        for entry in self.overrides.iter().filter(|entry| entry.matches(file)) {
            options.extend(entry.options.clone());
        }
        options
    }

    /// The files in groups of the same options, in the order of the first
    /// file of each, each group with the flags that hand its options over:
    /// Prettier is started once for each group.
    #[must_use]
    pub fn groups<'f>(&self, files: &[&'f str]) -> Vec<(Vec<String>, Vec<&'f str>)> {
        let mut groups: Vec<(Vec<String>, Vec<&str>)> = Vec::new();
        for file in files {
            let flags: Vec<String> = self
                .options_for(file)
                .iter()
                .map(|(name, setting)| flag(name, setting))
                .collect();
            match groups.iter_mut().find(|(same, _)| *same == flags) {
                Some((_, members)) => members.push(file),
                None => groups.push((flags, vec![file])),
            }
        }
        groups
    }
}

/// Reads Prettier's configuration with the names the register gives its
/// files, as [`read`] does; a register without Prettier names none, and
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

/// Reads Prettier's configuration, the program `name` of the register, among
/// `files`, the files of the repository from its root, through `read`: what
/// the checks hand over, and every problem, each naming its file and key. A
/// problem drops what it is about and nothing else, so the check still
/// formats with the rest, and every problem fails it. `kept` says which files
/// the repository keeps in its checks (see [`super::locate`]).
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
        .chain(beside(files, kept, read))
        .map(|unread| Problem {
            file: unread.file,
            message: unread.why,
        })
        .collect();
    let mut handed = Handed::default();
    if let Some(file) = files
        .iter()
        .find(|file| file.to_string() == editorconfig::NAME)
    {
        match read(file).and_then(|source| editorconfig::parse(&source)) {
            Ok(parsed) => {
                for (section, why) in parsed.unread {
                    problems.push(Problem {
                        file: (*file).clone(),
                        message: format!(
                            "the section `[{section}]` is not handed to Prettier: {why}"
                        ),
                    });
                }
                handed.editorconfig = Some(parsed.config);
            }
            Err(error) => problems.push(Problem {
                file: (*file).clone(),
                message: format!("cannot be read, so nothing of it is handed to Prettier: {error}"),
            }),
        }
    }
    let Some(file) = located.file else {
        return (handed, problems);
    };
    let mut found = Found {
        file,
        version: &program.version,
        problems: Vec::new(),
    };
    match read(file).and_then(|source| parse(file, &source)) {
        Ok(Value::Object(top)) => {
            handed.file = Some(file.clone());
            for (key, value) in top {
                match key.as_str() {
                    "$schema" => {}
                    "overrides" => handed.overrides = found.overrides(value),
                    _ => found.option(&mut handed.options, "", &key, &value),
                }
            }
        }
        // An empty file of YAML holds nothing, and sets nothing.
        Ok(Value::Null) => handed.file = Some(file.clone()),
        Ok(Value::String(module)) => found.problem(&format!(
            "names a shared configuration, `{module}`, a module that would be loaded and run, so \
             nothing of it is handed to Prettier; write the options themselves into the file"
        )),
        Ok(_) => found.problem("holds no mapping, so nothing of it is handed to Prettier"),
        Err(error) => found.problem(&format!(
            "cannot be read, so nothing of it is handed to Prettier: {error}"
        )),
    }
    problems.extend(found.problems);
    (handed, problems)
}

/// The value Prettier's file holds, read by its name as Prettier reads it:
/// `.json` and `.json5` as JSON5, which takes JSON as well, `.toml` as TOML,
/// and the others, `.prettierrc` among them, as YAML, which takes JSON as
/// well.
fn parse(file: &RelativePath, source: &str) -> Result<Value, String> {
    let name = file.file_name().unwrap_or_default();
    let format = match name.rsplit_once('.').map(|(_, ending)| ending) {
        Some("toml") => {
            return toml::from_str::<Value>(source).map_err(|error| error.to_string());
        }
        Some("json" | "json5") => Format::Jsonc,
        _ => Format::Yaml,
    };
    super::parse(format, source).map(Written::into_value)
}

/// The files beside those the register names that Prettier would read and
/// the tool does not, each with why: a `package.json` or `package.yaml` whose
/// `prettier` key holds a configuration, wherever it lies. `kept` says which
/// files the repository keeps in its checks: one an `[[exclude]]` entry takes
/// out is never named, as with [`super::locate`].
///
/// An `.editorconfig` in a folder below the root is not read either, and not
/// named: the pipeline's defaults take every `.editorconfig` out with an
/// `[[exclude]]` entry, and the exclusion gives its reason.
#[must_use]
pub fn beside(
    files: &[&RelativePath],
    kept: &dyn Fn(&RelativePath) -> bool,
    read: &Read<'_>,
) -> Vec<Unread> {
    files
        .iter()
        .filter(|file| kept(file))
        .filter_map(|file| {
            let name = file.file_name()?;
            (matches!(name, "package.json" | "package.yaml") && holds_configuration(file, read))
                .then(|| Unread {
                    file: (*file).clone(),
                    why: format!(
                        "not read: the tool reads the configuration of {NAME} only as data, from \
                         one file at the root of the repository, and not from the `prettier` key \
                         of `{name}`"
                    ),
                })
        })
        .collect()
}

/// Whether a `package.json` or `package.yaml` holds a configuration of
/// Prettier: a `prettier` key whose value JavaScript takes as true, as
/// Prettier asks it. One that does not read holds none, as for Prettier.
fn holds_configuration(file: &RelativePath, read: &Read<'_>) -> bool {
    let format = if file.file_name() == Some("package.json") {
        Format::Jsonc
    } else {
        Format::Yaml
    };
    let value = read(file)
        .and_then(|source| super::parse(format, &source))
        .map(Written::into_value);
    match value.ok().as_ref().and_then(|value| value.get("prettier")) {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::Null) | None => false,
        Some(_) => true,
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

    /// One option, `key`, put into `options` where it is carried over and
    /// its value is one it takes; `at` is where it stands, such as
    /// `overrides[0].options.`, in front of the key a problem names.
    fn option(&mut self, options: &mut Options, at: &str, key: &str, value: &Value) {
        let at = format!("{at}{key}");
        if let Some((_, kind)) = CARRIED.iter().find(|(name, _)| *name == key) {
            match setting(*kind, value) {
                Some(setting) => {
                    options.insert(key.to_owned(), setting);
                }
                None => self.problem(&format!(
                    "`{at}` is not {}, so it is not handed to Prettier",
                    expected(*kind)
                )),
            }
        } else if let Some((_, why)) = REFUSED.iter().find(|(name, _)| *name == key) {
            self.problem(&format!("`{at}` is not handed to Prettier: it {why}"));
        } else {
            self.problem(&format!(
                "`{at}` is not an option of Prettier {} the check knows, so it is not handed to \
                 Prettier",
                self.version
            ));
        }
    }

    /// The entries of `overrides`, each one that can be used.
    fn overrides(&mut self, value: Value) -> Vec<Override> {
        let Value::Array(entries) = value else {
            self.problem("`overrides` is not a list, so it is not handed to Prettier");
            return Vec::new();
        };
        entries
            .into_iter()
            .enumerate()
            .filter_map(|(index, entry)| self.entry(index, entry))
            .collect()
    }

    /// An entry of `overrides`, or [`None`] where it cannot be used, which is
    /// a problem.
    fn entry(&mut self, index: usize, value: Value) -> Option<Override> {
        let at = format!("overrides[{index}]");
        let Value::Object(mut entry) = value else {
            self.problem(&format!(
                "`{at}` is not a mapping, so it is not handed to Prettier"
            ));
            return None;
        };
        let files = entry.remove("files");
        let exclude = entry.remove("excludeFiles");
        let options = entry.remove("options");
        for key in entry.keys() {
            self.problem(&format!(
                "`{at}.{key}` is not a setting of an entry of `overrides` the check knows, so it \
                 is not handed to Prettier"
            ));
        }
        let Some(files) = files else {
            self.problem(&format!(
                "`{at}` has no `files`, so it is not handed to Prettier"
            ));
            return None;
        };
        let patterns = |value: &Value| -> Result<Vec<Pattern>, String> {
            match value {
                Value::String(pattern) => Ok(vec![built(pattern)?]),
                Value::Array(items) => items
                    .iter()
                    .map(|item| match item {
                        Value::String(pattern) => built(pattern),
                        _ => Err("is not a pattern or a list of patterns".to_owned()),
                    })
                    .collect(),
                _ => Err("is not a pattern or a list of patterns".to_owned()),
            }
        };
        let mut built_patterns = |key: &str, value: &Value| match patterns(value) {
            Ok(built) => Some(built),
            Err(reason) => {
                self.problem(&format!(
                    "`{at}.{key}` {reason}, so the entry is not handed to Prettier"
                ));
                None
            }
        };
        let files = built_patterns("files", &files)?;
        let exclude = match exclude {
            Some(exclude) => built_patterns("excludeFiles", &exclude)?,
            None => Vec::new(),
        };
        let mut handed = Options::new();
        match options {
            Some(Value::Object(options)) => {
                for (key, value) in options {
                    self.option(&mut handed, &format!("{at}.options."), &key, &value);
                }
            }
            None => {}
            Some(_) => self.problem(&format!(
                "`{at}.options` is not a mapping, so it is not handed to Prettier"
            )),
        }
        Some(Override {
            files,
            exclude,
            options: handed,
        })
    }
}

/// A pattern of `files` or `excludeFiles`, built as micromatch reads it; an
/// error says why it cannot be.
fn built(written: &str) -> Result<Pattern, String> {
    let slashes = written.contains('/');
    let rest = from_root(written);
    let (negated, glob) = match rest.strip_prefix('!') {
        Some(rest) if !rest.starts_with('(') => (true, from_root(rest)),
        _ => (false, rest),
    };
    let group = ['*', '?', '+', '@', '!']
        .iter()
        .any(|lead| glob.contains(&format!("{lead}(")));
    if group {
        return Err(format!(
            "holds `{written}`, a group, `(...)`, a form of pattern the tool does not read"
        ));
    }
    let glob = GlobBuilder::new(glob)
        .literal_separator(true)
        .build()
        .map_err(|error| format!("holds a pattern that does not read: {error}"))?
        .compile_matcher();
    Ok(Pattern {
        written: written.to_owned(),
        slashes,
        negated,
        glob,
    })
}

/// A pattern without the `./` it starts with, however often it is written.
fn from_root(mut pattern: &str) -> &str {
    while let Some(rest) = pattern.strip_prefix("./") {
        pattern = rest;
    }
    pattern
}

/// The value an option of this kind is handed, where `value` is one it
/// takes.
fn setting(kind: Kind, value: &Value) -> Option<Setting> {
    match (kind, value) {
        (Kind::Flag, Value::Bool(flag)) => Some(Setting::Flag(*flag)),
        (Kind::Count, Value::Number(number)) => number
            .as_f64()
            .and_then(editorconfig::whole)
            .map(Setting::Count),
        (Kind::Choice(words), Value::String(word)) if words.contains(&word.as_str()) => {
            Some(Setting::Choice(word.clone()))
        }
        _ => None,
    }
}

/// What an option of this kind takes, in words.
fn expected(kind: Kind) -> String {
    match kind {
        Kind::Flag => "true or false".to_owned(),
        Kind::Count => "a whole number of 0 or more".to_owned(),
        Kind::Choice(words) => format!(
            "one of {}",
            words
                .iter()
                .map(|word| format!("`{word}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
