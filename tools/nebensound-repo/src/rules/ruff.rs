//! Ruff's rules against the pipeline's default, as `check-python` lays a
//! repository's configuration over it.
//!
//! The block takes, for each unit it runs at, the configuration Ruff would find
//! for that unit: in the unit's folder or the nearest one above it, up to the
//! root, the first of `.ruff.toml`, `ruff.toml` and a `pyproject.toml` that
//! holds a `[tool.ruff]` table. It lays that file over the default the way Ruff
//! lays a configuration over the one its `extend` names, and what the file's
//! own `extend` names comes along beneath it, over the default as well. The
//! report reads the same files: each configuration the block would find for a
//! unit that holds a Python file the check covers, then every file its
//! `extend` chain names.
//!
//! Ruff's rule selection is no map from rule to value. `select` and `ignore`
//! name rules and prefixes of them (`D`, `PLR2`, `ALL`), which Ruff resolves by
//! how specific each one is, layer by layer. The report does not resolve them:
//! it shows every entry of the settings that switch rules on or off as the file
//! writes it, and says whether the default names the same entry. A `select`
//! starts over, so its entries are shown as one list, with what it sets aside.
//!
//! Read are `select`, `extend-select`, `ignore`, `extend-ignore`,
//! `per-file-ignores` and `extend-per-file-ignores`, in the `lint` table or,
//! where that table does not name one, at the top, where older configurations
//! write them. An `extend` that leaves the repository, or names a path Ruff
//! would expand (`~`, `$`), is named, not read.

use std::collections::BTreeSet;

use serde_json::Value as Json;
use toml::{Table, Value};

use crate::Analysis;
use crate::annotate::Coverage;
use crate::ignored::{Read, folder};
use crate::path::RelativePath;
use crate::report::{column, shown};

/// The check whose block runs Ruff.
const CHECK: &str = "python";

/// The files Ruff takes its configuration from in a folder, in the order it
/// looks for them.
const FILES: &[&str] = &[".ruff.toml", "ruff.toml", "pyproject.toml"];

/// The settings that switch rules on or off, in the order they are shown.
const SELECTION: &[&str] = &[
    "select",
    "extend-select",
    "ignore",
    "extend-ignore",
    "per-file-ignores",
    "extend-per-file-ignores",
];

/// The value of one of those settings.
enum Setting {
    /// A list of rules and prefixes.
    List(Vec<String>),
    /// Patterns of files, each with the rules and prefixes switched off there.
    Map(Vec<(String, Vec<String>)>),
}

/// What one configuration says of Ruff's rules.
struct Config {
    /// What its `extend` names.
    extend: Option<String>,
    /// The settings it writes, each under the key it is written as.
    selection: Vec<(String, Setting)>,
}

/// The entries of the default the repository's are held against.
struct Baseline {
    select: Vec<String>,
    ignore: Vec<String>,
    per_file: Vec<(String, Vec<String>)>,
}

impl Baseline {
    /// The default, read from the text of the pipeline's `ruff.toml`.
    fn read(text: &str) -> Result<Self, String> {
        let table: Table = toml::from_str(text).map_err(|error| error.to_string())?;
        let mut default = Self {
            select: Vec::new(),
            ignore: Vec::new(),
            per_file: Vec::new(),
        };
        for (key, setting) in config(&table)?.selection {
            match (key.rsplit('.').next(), setting) {
                (Some("select" | "extend-select"), Setting::List(list)) => {
                    default.select.extend(list);
                }
                (_, Setting::List(list)) => default.ignore.extend(list),
                (_, Setting::Map(map)) => default.per_file.extend(map),
            }
        }
        Ok(default)
    }

    /// What it switches off for a pattern, if it names the pattern.
    fn for_pattern(&self, pattern: &str) -> Option<&[String]> {
        self.per_file
            .iter()
            .find(|(own, _)| own == pattern)
            .map(|(_, rules)| rules.as_slice())
    }
}

/// A list as JSON writes it.
fn listed(entries: &[String]) -> String {
    Json::from(entries.to_vec()).to_string()
}

/// A list of text, or why the value is not one.
fn strings(key: &str, value: &Value) -> Result<Vec<String>, String> {
    value
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .ok_or_else(|| format!("`{key}` is not a list of text"))
}

/// What a table of Ruff's settings says of its rules.
fn config(table: &Table) -> Result<Config, String> {
    let extend = match table.get("extend") {
        None => None,
        Some(Value::String(path)) => Some(path.clone()),
        Some(_) => return Err("`extend` is not text".to_owned()),
    };
    let lint = match table.get("lint") {
        None => None,
        Some(Value::Table(lint)) => Some(lint),
        Some(_) => return Err("`lint` is not a table".to_owned()),
    };
    let mut selection = Vec::new();
    for name in SELECTION {
        let (key, value) = match lint.and_then(|lint| lint.get(*name)) {
            Some(value) => (format!("lint.{name}"), value),
            None => match table.get(*name) {
                Some(value) => ((*name).to_owned(), value),
                None => continue,
            },
        };
        let setting = if name.ends_with("per-file-ignores") {
            let map = value
                .as_table()
                .ok_or_else(|| format!("`{key}` is not a table"))?;
            Setting::Map(
                map.iter()
                    .map(|(pattern, rules)| {
                        Ok((
                            pattern.clone(),
                            strings(&format!("{key}.{pattern}"), rules)?,
                        ))
                    })
                    .collect::<Result<_, String>>()?,
            )
        } else {
            Setting::List(strings(&key, value)?)
        };
        selection.push((key, setting));
    }
    Ok(Config { extend, selection })
}

/// Ruff's settings in a file: all of it, or of a `pyproject.toml` its
/// `[tool.ruff]` table, [`None`] where it holds none.
fn settings(file: &RelativePath, source: &str) -> Result<Option<Table>, String> {
    let mut table: Table = toml::from_str(source).map_err(|error| error.to_string())?;
    if file.file_name() != Some("pyproject.toml") {
        return Ok(Some(table));
    }
    let Some(Value::Table(mut tool)) = table.remove("tool") else {
        return Ok(None);
    };
    match tool.remove("ruff") {
        None => Ok(None),
        Some(Value::Table(ruff)) => Ok(Some(ruff)),
        Some(_) => Err("`tool.ruff` is not a table".to_owned()),
    }
}

/// The units the block runs at that hold a Python file the check covers: for
/// each such file, the outermost unit holding it that names the check.
fn units(analysis: &Analysis, covered: &[&RelativePath]) -> BTreeSet<RelativePath> {
    let naming: Vec<&RelativePath> = analysis
        .units
        .iter()
        .filter(|unit| {
            unit.pipeline
                .checks
                .iter()
                .any(|check| check.as_str() == CHECK)
        })
        .map(|unit| &unit.path)
        .collect();
    covered
        .iter()
        .filter_map(|file| {
            naming
                .iter()
                .filter(|unit| unit.contains(file))
                .min_by_key(|unit| unit.segments().len())
                .map(|unit| (*unit).clone())
        })
        .collect()
}

/// The configuration Ruff finds for a unit. A `pyproject.toml` that does not
/// read is taken to hold one, so that the report says it cannot be read, as
/// the block stops there.
fn found(files: &[&RelativePath], unit: &RelativePath, read: &Read<'_>) -> Option<RelativePath> {
    let mut dir = unit.clone();
    loop {
        let here = FILES.iter().map(|name| dir.join(name)).find(|file| {
            files.contains(&file)
                && (file.file_name() != Some("pyproject.toml")
                    || read(file)
                        .and_then(|source| settings(file, &source))
                        .map_or(true, |table| table.is_some()))
        });
        if here.is_some() || dir.is_root() {
            return here;
        }
        dir = folder(&dir);
    }
}

/// The file an `extend` names from the folder of the file it stands in, or
/// [`None`] where it lies outside the repository or Ruff would expand it.
fn resolve(dir: &RelativePath, extend: &str) -> Option<RelativePath> {
    if extend.starts_with(['/', '~']) || extend.contains('$') {
        return None;
    }
    let mut segments: Vec<&str> = dir.segments().iter().map(String::as_str).collect();
    for segment in extend.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            name => segments.push(name),
        }
    }
    RelativePath::new(&segments.join("/")).ok()
}

/// The lines of one setting, each entry against the default.
fn entries(key: &str, setting: &Setting, default: &Baseline) -> Vec<String> {
    let line = |name: &str, text: String| format!("{}{}", column(10, &shown(name)), shown(&text));
    let select = listed(&default.select);
    let name = key.rsplit('.').next().unwrap_or(key);
    match setting {
        Setting::List(list) if name == "select" => vec![line(
            key,
            format!(
                "{} (starts over: the `select` and `ignore` beneath it no longer hold, the \
                 default's `select` {select} included)",
                listed(list)
            ),
        )],
        Setting::List(list) if name == "extend-select" => list
            .iter()
            .map(|entry| {
                let stand = if default.ignore.contains(entry) {
                    "over the default, whose `ignore` names it".to_owned()
                } else if default.select.contains(entry) {
                    "the same as the default: its `select` names it".to_owned()
                } else {
                    format!(
                        "the default names it in neither `select` nor `ignore`; its `select` \
                         is {select}"
                    )
                };
                line(entry, format!("on by `{key}` ({stand})"))
            })
            .collect(),
        Setting::List(list) => list
            .iter()
            .map(|entry| {
                let stand = if default.ignore.contains(entry) {
                    "the same as the default: its `ignore` names it".to_owned()
                } else {
                    format!("the default's `ignore` does not name it; its `select` is {select}")
                };
                line(entry, format!("off by `{key}` ({stand})"))
            })
            .collect(),
        Setting::Map(map) => {
            let mut lines = Vec::new();
            if name == "per-file-ignores" {
                let gone: Vec<String> = default
                    .per_file
                    .iter()
                    .filter(|(pattern, _)| map.iter().all(|(own, _)| own != pattern))
                    .map(|(pattern, _)| pattern.clone())
                    .collect();
                lines.push(line(
                    key,
                    if gone.is_empty() {
                        "starts over, in place of the default's".to_owned()
                    } else {
                        format!(
                            "starts over: the default's for {} no longer hold",
                            listed(&gone)
                        )
                    },
                ));
            }
            for (pattern, rules) in map {
                let under = default.for_pattern(pattern);
                for rule in rules {
                    let stand = match under {
                        Some(under) if under.contains(rule) => "the same as the default".to_owned(),
                        Some(under) => format!("the default switches off {} there", listed(under)),
                        None => "the default names no such pattern".to_owned(),
                    };
                    lines.push(line(
                        rule,
                        format!(
                            "off in {} by `{key}` ({stand})",
                            Json::from(pattern.as_str())
                        ),
                    ));
                }
            }
            lines
        }
    }
}

/// The lines of one configuration the block finds and of every file its
/// `extend` chain names.
fn chain(top: RelativePath, read: &Read<'_>, default: &Baseline) -> Vec<String> {
    let mut lines = Vec::new();
    let mut seen: Vec<RelativePath> = Vec::new();
    let mut file = top;
    loop {
        let head = column(6, &shown(&file.to_string()));
        let place = seen.last().map_or_else(
            || "laid over the default rule by rule".to_owned(),
            |over| format!("laid over the default beneath {over}, whose `extend` names it"),
        );
        let read_config = read(&file)
            .and_then(|source| settings(&file, &source))
            .and_then(|table| config(&table.unwrap_or_default()));
        let config = match read_config {
            Ok(config) => config,
            Err(error) => {
                lines.push(format!("{head}cannot be read: {}", shown(&error)));
                return lines;
            }
        };
        if config.extend.is_none() && config.selection.is_empty() {
            lines.push(format!(
                "{}{}",
                head,
                shown(&format!("{place}; it names no rule"))
            ));
            return lines;
        }
        lines.push(format!("{head}{}", shown(&place)));
        for (key, setting) in &config.selection {
            lines.extend(entries(key, setting, default));
        }
        let Some(extend) = config.extend else {
            return lines;
        };
        let next = resolve(&folder(&file), &extend);
        let what = match &next {
            None => "lies outside the repository: named, not read",
            Some(next) if *next == file || seen.contains(next) => {
                "comes back round to a file above it, which Ruff refuses"
            }
            Some(_) => "what it names lies beneath the rules here",
        };
        lines.push(format!(
            "{}{}",
            column(10, "extend"),
            shown(&format!("{}: {what}", Json::from(extend.as_str())))
        ));
        match next {
            Some(next) if next != file && !seen.contains(&next) => {
                seen.push(file);
                file = next;
            }
            _ => return lines,
        }
    }
}

/// The section of Ruff, or [`None`] where the check covers no Python file.
pub(super) fn section(
    analysis: &Analysis,
    files: &[&RelativePath],
    read: &Read<'_>,
    default: &str,
) -> Option<Vec<String>> {
    let covered: Vec<&RelativePath> = analysis
        .tree
        .files()
        .into_iter()
        .filter(
            |(_, coverage)| matches!(coverage, Coverage::Covered(check) if check.as_str() == CHECK),
        )
        .map(|(node, _)| &node.path)
        .collect();
    if covered.is_empty() {
        return None;
    }
    let head = column(2, CHECK);
    let default = match Baseline::read(default) {
        Ok(default) => default,
        Err(error) => {
            return Some(vec![format!(
                "{head}Ruff: the pipeline's default does not read: {}",
                shown(&error)
            )]);
        }
    };
    let configs: BTreeSet<RelativePath> = units(analysis, &covered)
        .iter()
        .filter_map(|unit| found(files, unit, read))
        .collect();
    if configs.is_empty() {
        return Some(vec![format!(
            "{head}Ruff: the default, nothing laid over it"
        )]);
    }
    let mut lines = vec![format!("{head}Ruff: the default, with these laid over it")];
    for config in configs {
        lines.extend(chain(config, read, &default));
    }
    Some(lines)
}
