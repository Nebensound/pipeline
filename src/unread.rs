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
//! It is a **warning**: the file changes nothing the pipeline does, and a
//! repository that runs the program by hand may keep it.

use crate::Analysis;
use crate::finding::{Finding, Rule};

/// The files, at the root of the repository, each with the program that read
/// it and the check that started that program.
const UNREAD: &[(&str, &str, &str)] = &[
    (".yamllint", "yamllint", "YAML"),
    (".yamllint.yaml", "yamllint", "YAML"),
    (".yamllint.yml", "yamllint", "YAML"),
];

/// Every configuration file of the repository that a check read once and
/// reads no longer.
#[must_use]
pub fn findings(analysis: &Analysis) -> Vec<Finding> {
    let nodes = analysis.tree.files();
    let mut findings = Vec::new();
    for (node, _) in nodes {
        let unread = UNREAD.iter().find(|(name, ..)| {
            node.path.segments().len() == 1 && node.path.file_name() == Some(name)
        });
        if let Some((_, program, check)) = unread {
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
