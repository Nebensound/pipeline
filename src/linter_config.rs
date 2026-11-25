//! A repository's configuration of a linter, read in one place.
//!
//! A reading check starts no code of the repository, and a linter that looks
//! for its own configuration finds ways that load code: a configuration
//! written in JavaScript, a plugin, a module of rules, a configuration that
//! extends another. Refusing such files one by one would mean mirroring every
//! linter's loader. So a linter under this rule never looks for its
//! configuration: the tool reads it, here, and hands the linter what it
//! wrote. The rule is the same for every linter under it:
//!
//! 1. A repository's file is optional. Without one, the pipeline's default
//!    holds. With one, its rules are laid over the default's rule by rule, so
//!    the file names only what differs, and may also switch a rule of the
//!    default off or loosen it; the rules report shows every difference.
//! 2. One file per linter, at the root of the repository, under one of the
//!    names the register gives the program (`configuration`), in a format of
//!    data, so that an editor reads the same file. The first of those names
//!    that stands at the root is read ([`locate`]). A difference for a folder
//!    or a unit is written in that file, as a pattern.
//! 3. The tool carries over only what it knows: rules, options that are data,
//!    and differences by pattern. Everything else is an error by name: a key
//!    it does not know, a key that names code or another file, and a file it
//!    does not read ([`Unread`]): another of the names at the root, one of them
//!    in a folder below it, and one the register names as not read
//!    (`configuration-not-read`), such as a configuration in JavaScript,
//!    wherever it lies.
//!
//! What a linter's file holds and how it is handed over is the linter's own:
//! [`markdownlint`] is the first linter under the rule. The check reads
//! through it and hands the linter the result, the rules report shows the
//! same result ([`crate::rules`]), and phase 0 names every file that is not
//! read ([`findings`]), so what the report shows is what runs. [`stylelint`]
//! follows it the same way in the CSS check: the check hands Stylelint a
//! configuration object the tool built, and every package it loads by its
//! place in the pinned install.
//!
//! The link check's configuration ([`crate::lychee_config`]) follows the same
//! shape and is older: lychee's `lychee.toml` is data by design and lychee
//! reads no other file of configuration once it is handed one, so it names
//! no file as not read, and it has no default of the pipeline to lay it over.
//!
//! A file `[[exclude]]` takes out is never named as not read: the exclusion
//! gives its reason. The one file at the root is read all the same, since it
//! configures the linter, whatever check judges the file itself.
//!
//! The tool reads a file as the linter would, keys in the order they are
//! written ([`Written`]): a linter that reads a configuration's keys in order,
//! as markdownlint does, lets a later key count over an earlier one, so the
//! tool hands the keys on in the order the repository wrote them and never
//! changes what the file means.
//!
//! The linters not named here still read their configuration themselves
//! (see `docs/principles.md`).
//!
//! Phase 0 names through [`findings`] one more kind of file that is not
//! read: the configuration of a program that no check starts any more
//! ([`crate::unread`]), such as a `.yamllint`. Such a program has left the
//! register, so its files cannot stand under `configuration-not-read`, which
//! names the files of a program the pipeline runs. Nothing runs that would
//! follow them, so each is a warning, not an error.

pub mod markdownlint;
pub mod stylelint;

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use yaml_rust2::{Yaml, YamlLoader};

use crate::Analysis;
use crate::annotate::Coverage;
use crate::finding::{Finding, Rule};
use crate::ignored::Read;
use crate::names::ProgramName;
use crate::path::RelativePath;
use crate::programs::Program;
use crate::register::Register;
use crate::rules::{self, Format};

/// A value of a linter's configuration as the file writes it: each mapping
/// keeps its keys in the order they are written. `serde_json`'s own
/// mapping sorts its keys, which would change what a file means to a linter
/// that reads the keys in order; the tool keeps `serde_json` as it is
/// everywhere else, so that the order of everything else it writes stays as
/// it is, and reads a linter's file into this type instead.
#[derive(Debug, Clone, PartialEq)]
pub enum Written {
    /// A mapping, its keys in the order written, each once: a key written
    /// twice keeps its first place and takes its last value, as JavaScript
    /// reads such a mapping.
    Mapping(Vec<(String, Written)>),
    /// A list.
    List(Vec<Written>),
    /// Text, a number, a boolean or null.
    Scalar(Value),
}

impl Written {
    /// The same value as `serde_json` holds it, the keys of every mapping
    /// within it sorted. For a value whose keys are looked up by name, such
    /// as the options of one rule, the order carries no meaning.
    #[must_use]
    pub fn into_value(self) -> Value {
        match self {
            Self::Mapping(entries) => Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, value.into_value()))
                    .collect(),
            ),
            Self::List(items) => Value::Array(items.into_iter().map(Self::into_value).collect()),
            Self::Scalar(value) => value,
        }
    }

    /// Takes a key out of a mapping, with its value; [`None`] where the
    /// mapping does not hold it.
    pub(crate) fn take(entries: &mut Vec<(String, Self)>, key: &str) -> Option<Self> {
        let index = entries.iter().position(|(name, _)| name == key)?;
        Some(entries.remove(index).1)
    }
}

/// Adds a key to a mapping as JavaScript does: in place of the same key
/// written before, else last.
fn put(entries: &mut Vec<(String, Written)>, key: String, value: Written) {
    match entries.iter_mut().find(|(name, _)| *name == key) {
        Some(slot) => slot.1 = value,
        None => entries.push((key, value)),
    }
}

impl<'de> Deserialize<'de> for Written {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(WrittenVisitor)
    }
}

/// Reads a [`Written`] value in the order the parser hands its parts over,
/// which is the order of the file.
struct WrittenVisitor;

impl<'de> Visitor<'de> for WrittenVisitor {
    type Value = Written;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a value of JSON")
    }

    fn visit_unit<E>(self) -> Result<Written, E> {
        Ok(Written::Scalar(Value::Null))
    }

    fn visit_bool<E>(self, flag: bool) -> Result<Written, E> {
        Ok(Written::Scalar(Value::Bool(flag)))
    }

    fn visit_i64<E>(self, number: i64) -> Result<Written, E> {
        Ok(Written::Scalar(Value::from(number)))
    }

    fn visit_u64<E>(self, number: u64) -> Result<Written, E> {
        Ok(Written::Scalar(Value::from(number)))
    }

    fn visit_f64<E>(self, number: f64) -> Result<Written, E> {
        Ok(Written::Scalar(Value::from(number)))
    }

    fn visit_str<E>(self, text: &str) -> Result<Written, E> {
        Ok(Written::Scalar(Value::from(text)))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Written, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Written::List(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Written, A::Error> {
        let mut entries = Vec::new();
        while let Some((key, value)) = map.next_entry::<String, Written>()? {
            put(&mut entries, key, value);
        }
        Ok(Written::Mapping(entries))
    }
}

/// The value a file of a linter's configuration holds, its keys in the
/// order written: JSON with comments, or YAML, its values as
/// [`rules::json`] gives them. A file in JavaScript is never handed here. An
/// empty file of YAML holds null.
///
/// # Errors
///
/// Where the file does not read, with the parser's words.
pub(crate) fn parse(format: Format, source: &str) -> Result<Written, String> {
    match format {
        Format::Jsonc => {
            jsonc_parser::parse_to_serde_value(source, &jsonc_parser::ParseOptions::default())
                .map_err(|error| error.to_string())
        }
        Format::Yaml | Format::Script => {
            let documents = YamlLoader::load_from_str(source).map_err(|error| error.to_string())?;
            Ok(documents
                .first()
                .map_or(Written::Scalar(Value::Null), written))
        }
    }
}

/// A YAML value as [`Written`], its mappings in their order. A key that is
/// not text is written out as YAML would show it, as [`rules::json`] does.
fn written(yaml: &Yaml) -> Written {
    match yaml {
        Yaml::Hash(hash) => {
            let mut entries = Vec::new();
            for (key, value) in hash {
                let key = match key {
                    Yaml::String(name) => name.clone(),
                    other => rules::json(other).to_string(),
                };
                put(&mut entries, key, written(value));
            }
            Written::Mapping(entries)
        }
        Yaml::Array(items) => Written::List(items.iter().map(written).collect()),
        other => Written::Scalar(rules::json(other)),
    }
}

/// Something in a linter's configuration that is not handed over, which
/// fails the check that runs the linter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The file it is in.
    pub file: RelativePath,
    /// What is wrong, naming the key.
    pub message: String,
}

/// A file of a linter's configuration that the tool does not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unread {
    /// The file, from the repository root.
    pub file: RelativePath,
    /// Why it is not read, a sentence that starts with "not read".
    pub why: String,
}

/// Where a linter's configuration stands in the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located<'a> {
    /// The one file the tool reads, if the repository has one.
    pub file: Option<&'a RelativePath>,
    /// Every other file of the linter's configuration, which the tool does
    /// not read, in the order of `files`.
    pub unread: Vec<Unread>,
}

/// Finds the configuration of `program`, called `name`, among `files`, the
/// files of the repository from its root: the first of its names under
/// `configuration` that stands at the root, and every other file of its
/// configuration, which is not read. `kept` says whether a file is one the
/// repository keeps in its checks: one an `[[exclude]]` entry takes out is
/// never named as not read.
#[must_use]
pub fn locate<'a>(
    files: &[&'a RelativePath],
    kept: &dyn Fn(&RelativePath) -> bool,
    name: &ProgramName,
    program: &Program,
) -> Located<'a> {
    let at_root = |wanted: &str| {
        files
            .iter()
            .copied()
            .find(|file| file.to_string() == wanted)
    };
    let file = program
        .configuration
        .iter()
        .find_map(|wanted| at_root(wanted));
    let names = program
        .configuration
        .iter()
        .map(|wanted| format!("`{wanted}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let unread = files
        .iter()
        .filter(|candidate| Some(**candidate) != file)
        .filter_map(|candidate| {
            let path = candidate.to_string();
            let listed = |list: &[String]| {
                list.iter()
                    .any(|wanted| path == *wanted || path.ends_with(&format!("/{wanted}")))
            };
            let why = if listed(&program.configuration_not_read) {
                format!(
                    "not read: the tool reads the configuration of {name} only as data, from one \
                     file at the root of the repository ({names}), and never runs this one"
                )
            } else if !listed(&program.configuration) {
                return None;
            } else if program.configuration.contains(&path) {
                format!(
                    "not read: the tool reads the configuration of {name} from one file at the \
                     root of the repository, and that is `{}`",
                    file.map(ToString::to_string).unwrap_or_default()
                )
            } else {
                format!(
                    "not read: the tool reads the configuration of {name} from one file at the \
                     root of the repository ({names}), and from no folder below it; a difference \
                     for a folder belongs in that file, as a pattern"
                )
            };
            if !kept(candidate) {
                return None;
            }
            Some(Unread {
                file: (*candidate).clone(),
                why,
            })
        })
        .collect();
    Located { file, unread }
}

/// Every file of a linter's configuration in the repository that the tool
/// does not read, for every program the register names files of its
/// configuration for, and beside them the other files Stylelint would read
/// ([`stylelint::beside`]), read through `read` where their content decides
/// it: each one an error of phase 0 ([`Rule::UnreadConfiguration`]), since
/// the linter would follow it wherever it runs by hand, and the check never
/// does. After them, each configuration of a program that no check starts
/// any more, a warning ([`crate::unread::findings`]).
#[must_use]
pub fn findings(analysis: &Analysis, register: &Register, read: &Read<'_>) -> Vec<Finding> {
    let nodes = analysis.tree.files();
    let files: Vec<&RelativePath> = nodes.iter().map(|(node, _)| &node.path).collect();
    let excluded: BTreeSet<&RelativePath> = nodes
        .iter()
        .filter(|(_, coverage)| matches!(coverage, Coverage::Excluded { .. }))
        .map(|(node, _)| &node.path)
        .collect();
    let kept = |file: &RelativePath| !excluded.contains(file);
    let mut found: Vec<Finding> = register
        .program
        .iter()
        .flat_map(|(name, program)| {
            let mut unread = locate(&files, &kept, name, program).unread;
            if name.as_str() == stylelint::NAME {
                unread.extend(stylelint::beside(&files, &kept, read));
            }
            unread
        })
        .map(|unread| Finding::error(Rule::UnreadConfiguration, unread.why).at(unread.file))
        .collect();
    found.extend(crate::unread::findings(analysis));
    found
}
