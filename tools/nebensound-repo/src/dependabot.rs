//! The Dependabot configuration a repository's declaration and the policy
//! give, and how far the file in the tree departs from it.
//!
//! The declaration names the ecosystems each unit has watched; the policy says
//! how every one of them is watched: how often, which updates come grouped, how
//! many pull requests stand open, with which commit prefix and which label. It
//! is kept once, in the central defaults (`[dependabot]` in `defaults.toml`),
//! and no repository's declaration can change it.
//!
//! There is one entry per ecosystem, listing the folders that hold a manifest
//! of it inside a unit that has it watched, so that the limit and the group
//! hold per ecosystem, as the policy means them. A manifest an `[[exclude]]`
//! entry takes out is left out: nobody looks at that folder, so nobody would
//! judge a pull request on it. `github-actions` names no manifest; Dependabot
//! reads the workflows from `/`, and the composite actions from
//! `/.github/actions/*` once the repository holds one.
//!
//! Phase 0 compares the file in the tree only for what this generator sets:
//! entries, directories and the keys of the policy. What else a file holds is
//! tolerated, a key the policy does not set as well as an entry for an
//! ecosystem the generator writes none for. Free of input and output, like the
//! rules of the declaration: the file is handed in through a reader.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use yaml_rust2::yaml::Hash;
use yaml_rust2::{Yaml, YamlEmitter, YamlLoader};

use crate::annotate::Coverage;
use crate::central::Central;
use crate::finding::{Finding, Rule};
use crate::ignored::Read;
use crate::names::EcosystemName;
use crate::path::RelativePath;
use crate::register::Register;
use crate::validate::Analysis;

/// Where Dependabot reads its configuration, in the order a file is looked
/// for: the first one the tree holds is the one compared.
pub const FILES: [&str; 2] = [".github/dependabot.yml", ".github/dependabot.yaml"];

/// The version of Dependabot's configuration format.
const FORMAT: i64 = 2;

/// What `schedule.interval` takes without a cron expression.
const INTERVALS: &[&str] = &[
    "daily",
    "weekly",
    "monthly",
    "quarterly",
    "semiannually",
    "yearly",
];

/// What `update-types` of a group takes.
const UPDATE_TYPES: &[&str] = &["major", "minor", "patch"];

/// How long a commit prefix may be.
const PREFIX_LENGTH: usize = 50;

/// The Dependabot name of the ecosystem whose composite actions are watched
/// beside the workflows.
const ACTIONS: &str = "github-actions";

/// Where the policy is described, for the header of the printed file.
const POLICY_PAGE: &str =
    "https://github.com/Nebensound/pipeline/blob/main/docs/register.md#dependabot-policy";

/// The policy every entry of every `dependabot.yml` follows.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Policy {
    /// How often Dependabot looks: `schedule.interval`.
    pub interval: String,
    /// How many pull requests of one entry stand open at most:
    /// `open-pull-requests-limit`.
    pub open_pull_requests_limit: u32,
    /// The labels every pull request carries: `labels`.
    pub labels: Vec<String>,
    /// The commit prefix of an ecosystem that names none of its own:
    /// `commit-message.prefix`.
    pub commit_prefix: String,
    /// The one group of every entry.
    pub group: Group,
    /// What differs for one ecosystem.
    #[serde(default)]
    pub ecosystem: BTreeMap<EcosystemName, EcosystemPolicy>,
}

/// The group the updates of an entry come together in.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Group {
    /// Its name, the key under `groups`.
    pub name: String,
    /// The kinds of update it takes: `update-types`. The others come alone.
    pub update_types: Vec<String>,
}

/// What differs for one ecosystem.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct EcosystemPolicy {
    /// Its commit prefix, in place of the policy's.
    #[serde(default)]
    pub commit_prefix: Option<String>,
    /// The dependencies that stay out of the group, each its own pull
    /// request: `exclude-patterns` of the group.
    #[serde(default)]
    pub exclude_from_group: Vec<String>,
}

impl Policy {
    /// Says what in the policy Dependabot would not take, or names an
    /// ecosystem the register does not know.
    ///
    /// # Errors
    ///
    /// Returns the problem in words.
    pub fn check(&self, register: &Register) -> Result<(), String> {
        if !INTERVALS.contains(&self.interval.as_str()) {
            return Err(format!(
                "sets the interval {:?}, and Dependabot takes one of {}",
                self.interval,
                INTERVALS.join(", ")
            ));
        }
        if self.group.update_types.is_empty()
            || self
                .group
                .update_types
                .iter()
                .any(|kind| !UPDATE_TYPES.contains(&kind.as_str()))
        {
            return Err(format!(
                "groups {:?}, and a group takes one or more of {}",
                self.group.update_types,
                UPDATE_TYPES.join(", ")
            ));
        }
        let prefixes = std::iter::once(&self.commit_prefix).chain(
            self.ecosystem
                .values()
                .filter_map(|ecosystem| ecosystem.commit_prefix.as_ref()),
        );
        for prefix in prefixes {
            if prefix.is_empty() || prefix.chars().count() > PREFIX_LENGTH {
                return Err(format!(
                    "sets the commit prefix {prefix:?}, and Dependabot takes one of 1 to \
                     {PREFIX_LENGTH} characters"
                ));
            }
        }
        if let Some(unknown) = self
            .ecosystem
            .keys()
            .find(|name| !register.ecosystem.contains_key(*name))
        {
            return Err(format!(
                "names the ecosystem {unknown}, which the register does not know"
            ));
        }
        Ok(())
    }

    /// The commit prefix of this ecosystem.
    #[must_use]
    pub fn prefix(&self, ecosystem: &EcosystemName) -> &str {
        self.ecosystem
            .get(ecosystem)
            .and_then(|own| own.commit_prefix.as_deref())
            .unwrap_or(&self.commit_prefix)
    }

    /// The dependencies of this ecosystem that stay out of the group.
    #[must_use]
    pub fn exclude_from_group(&self, ecosystem: &EcosystemName) -> &[String] {
        self.ecosystem
            .get(ecosystem)
            .map_or(&[], |own| own.exclude_from_group.as_slice())
    }
}

/// One entry under `updates`: one ecosystem, the folders it is watched in and
/// the policy applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The name Dependabot knows the ecosystem by.
    pub package_ecosystem: String,
    /// The folders, each from the root and starting with `/`, sorted.
    pub directories: Vec<String>,
    /// `schedule.interval`.
    pub interval: String,
    /// The name of the group.
    pub group: String,
    /// `update-types` of the group.
    pub update_types: Vec<String>,
    /// `exclude-patterns` of the group; written only when there are any.
    pub exclude_patterns: Vec<String>,
    /// `open-pull-requests-limit`.
    pub open_pull_requests_limit: u32,
    /// `commit-message.prefix`.
    pub prefix: String,
    /// `labels`.
    pub labels: Vec<String>,
}

/// The entries the declaration and the policy give, in the order of the
/// register's ecosystems.
///
/// An ecosystem gets an entry when the base set or a unit has it watched and
/// there is a folder to watch: one that holds a manifest of it inside a unit
/// that has it watched, and that no `[[exclude]]` entry takes out.
#[must_use]
pub fn entries(analysis: &Analysis, central: &Central) -> Vec<Entry> {
    let register = central.register();
    let policy = central.dependabot();
    let files = analysis.tree.files();
    let mut entries = Vec::new();
    for (name, ecosystem) in &register.ecosystem {
        let everywhere = central.base_dependabot().contains(name);
        let watched_in = |path: &RelativePath| {
            everywhere
                || analysis
                    .units
                    .iter()
                    .any(|unit| unit.path.contains(path) && unit.pipeline.dependabot.contains(name))
        };
        let mut directories = BTreeSet::new();
        if ecosystem.manifests.is_empty() && watched_in(&RelativePath::root()) {
            directories.insert("/".to_owned());
            if ecosystem.package_ecosystem == ACTIONS
                && files.iter().any(|(node, _)| composite_action(&node.path))
            {
                directories.insert("/.github/actions/*".to_owned());
            }
        }
        for (node, coverage) in &files {
            if matches!(coverage, Coverage::Excluded { .. })
                || register.ecosystem_of(&node.path.to_string()) != Some(name)
                || !watched_in(&node.path)
            {
                continue;
            }
            directories.insert(folder_of(&node.path));
        }
        if directories.is_empty() {
            continue;
        }
        entries.push(Entry {
            package_ecosystem: ecosystem.package_ecosystem.clone(),
            directories: directories.into_iter().collect(),
            interval: policy.interval.clone(),
            group: policy.group.name.clone(),
            update_types: policy.group.update_types.clone(),
            exclude_patterns: policy.exclude_from_group(name).to_vec(),
            open_pull_requests_limit: policy.open_pull_requests_limit,
            prefix: policy.prefix(name).to_owned(),
            labels: policy.labels.clone(),
        });
    }
    entries
}

/// Whether this file is the definition of a composite action of the
/// repository: `.github/actions/<name>/action.yml` or `.yaml`.
fn composite_action(path: &RelativePath) -> bool {
    matches!(
        path.segments(),
        [github, actions, _, file]
            if github == ".github"
                && actions == "actions"
                && (file == "action.yml" || file == "action.yaml")
    )
}

/// The folder a file lies in, as Dependabot names it: from the root, starting
/// with `/`.
fn folder_of(path: &RelativePath) -> String {
    let segments = path.segments();
    let folder = &segments[..segments.len().saturating_sub(1)];
    format!("/{}", folder.join("/"))
}

/// The file the entries make, with a header that says where it comes from.
///
/// One folder is written as `directory`, several as `directories`.
#[must_use]
pub fn render(entries: &[Entry]) -> String {
    let mut root = Hash::new();
    root.insert(text("version"), Yaml::Integer(FORMAT));
    root.insert(
        text("updates"),
        Yaml::Array(entries.iter().map(entry_yaml).collect()),
    );
    let mut body = String::new();
    // Writing into a String cannot fail, and the document holds nothing the
    // emitter refuses: no alias, no key that is not a string.
    let _ = YamlEmitter::new(&mut body).dump(&Yaml::Hash(root));
    format!(
        "# Written by `nebensound-repo dependabot` from .nebensound/repo.toml and the\n\
         # organization's Dependabot policy, described at\n\
         # {POLICY_PAGE}\n\
         # Phase 0 warns when this file departs from what the command prints.\n\
         {body}\n"
    )
}

fn entry_yaml(entry: &Entry) -> Yaml {
    let mut map = Hash::new();
    map.insert(text("package-ecosystem"), text(&entry.package_ecosystem));
    match entry.directories.as_slice() {
        [one] => {
            map.insert(text("directory"), text(one));
        }
        several => {
            map.insert(text("directories"), texts(several));
        }
    }
    let mut schedule = Hash::new();
    schedule.insert(text("interval"), text(&entry.interval));
    map.insert(text("schedule"), Yaml::Hash(schedule));
    let mut group = Hash::new();
    group.insert(text("update-types"), texts(&entry.update_types));
    if !entry.exclude_patterns.is_empty() {
        group.insert(text("exclude-patterns"), texts(&entry.exclude_patterns));
    }
    let mut groups = Hash::new();
    groups.insert(text(&entry.group), Yaml::Hash(group));
    map.insert(text("groups"), Yaml::Hash(groups));
    map.insert(
        text("open-pull-requests-limit"),
        Yaml::Integer(i64::from(entry.open_pull_requests_limit)),
    );
    let mut commit = Hash::new();
    commit.insert(text("prefix"), text(&entry.prefix));
    map.insert(text("commit-message"), Yaml::Hash(commit));
    map.insert(text("labels"), texts(&entry.labels));
    Yaml::Hash(map)
}

fn text(value: &str) -> Yaml {
    Yaml::String(value.to_owned())
}

fn texts(values: &[String]) -> Yaml {
    Yaml::Array(values.iter().map(|value| text(value)).collect())
}

/// Every place the found file departs from the entries, each named by its key
/// path: a missing entry, a folder an entry does not list, a value of the
/// policy that is missing or different.
///
/// `directory: X` counts as `directories: [X]`, and the folders of every entry
/// for an ecosystem count together; a folder more is tolerated. Lists the
/// policy sets are compared as sets, without regard to order.
#[must_use]
pub fn differences(entries: &[Entry], found: &Yaml) -> Vec<String> {
    let mut found_differences = Vec::new();
    let note = |out: &mut Vec<String>, key: &str, expected: &Yaml, actual: &Yaml| {
        out.extend(compared(key, expected, actual));
    };
    note(
        &mut found_differences,
        "version",
        &Yaml::Integer(FORMAT),
        &found["version"],
    );

    let updates: Vec<(usize, &Yaml)> = found["updates"]
        .as_vec()
        .map(|list| list.iter().enumerate().collect())
        .unwrap_or_default();
    for entry in entries {
        let matching: Vec<&(usize, &Yaml)> = updates
            .iter()
            .filter(|(_, update)| {
                update["package-ecosystem"].as_str() == Some(entry.package_ecosystem.as_str())
            })
            .collect();
        if matching.is_empty() {
            found_differences.push(format!(
                "updates has no entry for {}",
                entry.package_ecosystem
            ));
            continue;
        }
        let listed: BTreeSet<String> = matching
            .iter()
            .flat_map(|(_, update)| folders(update))
            .collect();
        // The folders of every entry for the ecosystem count together, so a
        // missing one is named at all of them.
        let indices = matching
            .iter()
            .map(|(index, _)| index.to_string())
            .collect::<Vec<_>>()
            .join(",");
        for directory in &entry.directories {
            if !listed.contains(directory) {
                found_differences.push(format!("updates[{indices}].directories lacks {directory}"));
            }
        }
        let expected = entry_yaml(entry);
        for (index, update) in matching {
            let at = |key: &str| format!("updates[{index}].{key}");
            note(
                &mut found_differences,
                &at("schedule.interval"),
                &expected["schedule"]["interval"],
                &update["schedule"]["interval"],
            );
            let group = &entry.group;
            note(
                &mut found_differences,
                &at(&format!("groups.{group}.update-types")),
                &expected["groups"][group.as_str()]["update-types"],
                &update["groups"][group.as_str()]["update-types"],
            );
            if !entry.exclude_patterns.is_empty() {
                note(
                    &mut found_differences,
                    &at(&format!("groups.{group}.exclude-patterns")),
                    &expected["groups"][group.as_str()]["exclude-patterns"],
                    &update["groups"][group.as_str()]["exclude-patterns"],
                );
            }
            note(
                &mut found_differences,
                &at("open-pull-requests-limit"),
                &expected["open-pull-requests-limit"],
                &update["open-pull-requests-limit"],
            );
            note(
                &mut found_differences,
                &at("commit-message.prefix"),
                &expected["commit-message"]["prefix"],
                &update["commit-message"]["prefix"],
            );
            note(
                &mut found_differences,
                &at("labels"),
                &expected["labels"],
                &update["labels"],
            );
        }
    }
    found_differences
}

/// The folders an entry lists, under `directory` or `directories`, each
/// written from the root with a leading `/` and no trailing one.
fn folders(update: &Yaml) -> Vec<String> {
    let single = update["directory"].as_str().into_iter();
    let several = update["directories"]
        .as_vec()
        .into_iter()
        .flatten()
        .filter_map(Yaml::as_str);
    single
        .chain(several)
        .map(|folder| {
            // `./deploy/` names the same folder as `/deploy`.
            let segments: Vec<&str> = folder
                .split('/')
                .filter(|segment| !segment.is_empty() && *segment != ".")
                .collect();
            format!("/{}", segments.join("/"))
        })
        .collect()
}

/// The difference between what the policy sets and what the file holds at
/// one key, or [`None`] when they agree. Lists agree when they hold the same
/// values in any order.
fn compared(key: &str, expected: &Yaml, actual: &Yaml) -> Option<String> {
    let agree = match (expected.as_vec(), actual.as_vec()) {
        (Some(expected), Some(actual)) => {
            expected.iter().collect::<BTreeSet<_>>() == actual.iter().collect::<BTreeSet<_>>()
        }
        _ => expected == actual,
    };
    if agree {
        return None;
    }
    let expected = shown(expected);
    Some(if actual.is_badvalue() {
        format!("{key} is missing, and the policy sets {expected}")
    } else {
        format!("{key} is {}, and the policy sets {expected}", shown(actual))
    })
}

/// A value as a message shows it: a list in brackets.
fn shown(value: &Yaml) -> String {
    match value {
        Yaml::String(text) => text.clone(),
        Yaml::Integer(number) => number.to_string(),
        Yaml::Array(list) => format!(
            "[{}]",
            list.iter().map(shown).collect::<Vec<_>>().join(", ")
        ),
        other => {
            let mut text = String::new();
            // The same as in `render`: a String takes every write.
            let _ = YamlEmitter::new(&mut text).dump(other);
            text.trim_start_matches("---").trim().to_owned()
        }
    }
}

/// The warning `stale-dependabot`, if the Dependabot configuration in the
/// tree is missing, cannot be read, or departs from what the declaration and
/// the policy give. One finding for the file, listing every difference.
#[must_use]
pub fn findings(analysis: &Analysis, central: &Central, read: &Read<'_>) -> Vec<Finding> {
    let files = analysis.tree.files();
    let held = FILES.iter().find_map(|name| {
        files
            .iter()
            .map(|(node, _)| &node.path)
            .find(|path| path.to_string() == *name)
    });
    let Some(path) = held else {
        let written = RelativePath::root().join(".github").join("dependabot.yml");
        return vec![
            Finding::warning(
                Rule::StaleDependabot,
                format!(
                    "the repository holds no Dependabot configuration, which every repository \
                     holds; write the one `nebensound-repo dependabot` prints at {written}"
                ),
            )
            .at(written),
        ];
    };
    let parsed = read(path).and_then(|source| {
        YamlLoader::load_from_str(&source)
            .map(|documents| documents.into_iter().next().unwrap_or(Yaml::Null))
            .map_err(|error| error.to_string())
    });
    let found = match parsed {
        Ok(found) => found,
        Err(reason) => {
            return vec![
                Finding::warning(
                    Rule::StaleDependabot,
                    format!(
                        "the Dependabot configuration cannot be read ({reason}); write the one \
                         `nebensound-repo dependabot` prints in its place"
                    ),
                )
                .at(path.clone()),
            ];
        }
    };
    let differences = differences(&entries(analysis, central), &found);
    if differences.is_empty() {
        return Vec::new();
    }
    vec![
        Finding::warning(
            Rule::StaleDependabot,
            format!(
                "the Dependabot configuration departs from what the declaration and the policy \
                 give: {}. `nebensound-repo dependabot` prints the file they give",
                differences.join("; ")
            ),
        )
        .at(path.clone()),
    ]
}
