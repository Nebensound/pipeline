//! Configuration files of programs that no check starts any more.
//!
//! When a check stops starting a program, the configuration a repository
//! wrote for it stays behind and keeps looking as if it counted. Phase 0 names
//! each such file it finds, with [`Rule::UnreadConfiguration`], so that the
//! repository learns that its rules and its ignore lists there change nothing.
//! It does so through [`crate::linter_config::findings`], beside the files of
//! a linter's configuration the tool does not read. The program has left the
//! register, so the files are listed here and not under the register's
//! `configuration-not-read`.
//!
//! The YAML check started yamllint and laid the repository's configuration at
//! the root over the pipeline's default: `.yamllint`, `.yamllint.yaml` or
//! `.yamllint.yml`. Whether a YAML file parses, holds a key twice or names an
//! anchor that does not exist is now the tool's own judgement, and it reads
//! none of them.
//!
//! The HTML check started html-validate, which read the `.htmlvalidate.json`
//! (`.mjs`, `.cjs`, `.js`) of a page's folder and of every folder above it,
//! and phase 0 held the `.htmlvalidateignore` of every folder against the
//! declaration; a flat configuration at the root (`html-validate.config.*`)
//! failed the check. The page rules are now the tool's own judgement, and
//! nothing reads any of these files, in whatever folder.
//!
//! It is a **warning**: the file changes nothing the pipeline does, and a
//! repository that runs the program by hand may keep it.

use crate::Analysis;
use crate::finding::{Finding, Rule};

/// Where a check found a file of a program's configuration.
enum Place {
    /// At the root of the repository only.
    Root,
    /// In every folder.
    Anywhere,
}

use Place::{Anywhere, Root};

/// The files of a program's configuration, each with where the check found
/// it.
type Files = &'static [(&'static str, Place)];

/// The programs, each with the check that started it and its files.
const UNREAD: &[(&str, &str, Files)] = &[
    (
        "yamllint",
        "YAML",
        &[
            (".yamllint", Root),
            (".yamllint.yaml", Root),
            (".yamllint.yml", Root),
        ],
    ),
    (
        "html-validate",
        "HTML",
        &[
            (".htmlvalidate.json", Anywhere),
            (".htmlvalidate.mjs", Anywhere),
            (".htmlvalidate.cjs", Anywhere),
            (".htmlvalidate.js", Anywhere),
            (".htmlvalidateignore", Anywhere),
            ("html-validate.config.mjs", Root),
            ("html-validate.config.cjs", Root),
            ("html-validate.config.js", Root),
            ("html-validate.config.mts", Root),
            ("html-validate.config.cts", Root),
            ("html-validate.config.ts", Root),
        ],
    ),
];

/// Every configuration file of the repository that a check read once and
/// reads no longer.
#[must_use]
pub fn findings(analysis: &Analysis) -> Vec<Finding> {
    let nodes = analysis.tree.files();
    let mut findings = Vec::new();
    for (node, _) in nodes {
        let at_root = node.path.segments().len() == 1;
        let unread = UNREAD.iter().find(|(_, _, files)| {
            files.iter().any(|(name, place)| {
                node.path.file_name() == Some(name) && (at_root || matches!(place, Anywhere))
            })
        });
        if let Some((program, check, _)) = unread {
            findings.push(
                Finding::warning(
                    Rule::UnreadConfiguration,
                    format!(
                        "the {check} check no longer starts {program}, so nothing the pipeline \
                         runs reads this file: its rules and what it leaves out change nothing. \
                         Remove it, or keep it for a {program} run of your own"
                    ),
                )
                .at(node.path.clone()),
            );
        }
    }
    findings
}
