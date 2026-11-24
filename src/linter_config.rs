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
//! read ([`findings`]), so what the report shows is what runs.
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
//! The other linters still read their configuration themselves (see
//! `docs/principles.md`).

pub mod markdownlint;

use std::collections::BTreeSet;

use crate::Analysis;
use crate::annotate::Coverage;
use crate::finding::{Finding, Rule};
use crate::names::ProgramName;
use crate::path::RelativePath;
use crate::programs::Program;
use crate::register::Register;

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
/// configuration for: each one an error of phase 0
/// ([`Rule::UnreadConfiguration`]), since the linter would follow it
/// wherever it runs by hand, and the check never does.
#[must_use]
pub fn findings(analysis: &Analysis, register: &Register) -> Vec<Finding> {
    let nodes = analysis.tree.files();
    let files: Vec<&RelativePath> = nodes.iter().map(|(node, _)| &node.path).collect();
    let excluded: BTreeSet<&RelativePath> = nodes
        .iter()
        .filter(|(_, coverage)| matches!(coverage, Coverage::Excluded { .. }))
        .map(|(node, _)| &node.path)
        .collect();
    let kept = |file: &RelativePath| !excluded.contains(file);
    register
        .program
        .iter()
        .flat_map(|(name, program)| locate(&files, &kept, name, program).unread)
        .map(|unread| Finding::error(Rule::UnreadConfiguration, unread.why).at(unread.file))
        .collect()
}
