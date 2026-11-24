//! Control characters in names: refused by phase 0, one rule for the kind.
//!
//! Git allows a line break, an escape or any other control character in the
//! name of a tracked file, and the declaration reads such a character in a
//! path as well. A name like that has no honest use, and every place that
//! hands names on one per line, or as `name=value`, would misread it. So
//! instead of guarding each such place, phase 0 refuses the name itself: an
//! error for each tracked file whose path holds one, and for each path of the
//! declaration that does. The run stops there, so no check is started with it.
//!
//! A control character is one of Unicode's general category `Cc`, as
//! [`char::is_control`] tells it: U+0000 to U+001F and U+007F to U+009F. That
//! is the set [`shown`] writes as an escape whenever it shows a name, so every
//! name this rule lets through is shown as it stands.
//!
//! What phase 0 does with such a name before this finding is written stays
//! safe: Git's lists are read with a zero byte between the names, every
//! report writes a name through [`shown`], and so does the message of a
//! declaration that does not read, where it quotes a path or a line of it.

use crate::finding::{Finding, Rule, UnitName};
use crate::model::Declaration;
use crate::report::shown;
use crate::tree::Tree;

/// Whether the text holds a control character.
#[must_use]
pub fn holds_control(text: &str) -> bool {
    text.chars().any(char::is_control)
}

/// An error for every tracked file whose path holds a control character, at
/// that file, and one for every path of the declaration that does, at the
/// declaration.
#[must_use]
pub fn findings(declaration: &Declaration, tree: &Tree) -> Vec<Finding> {
    let mut findings: Vec<Finding> = tree
        .files()
        .filter(|file| holds_control(&file.to_string()))
        .map(|file| {
            Finding::error(
                Rule::ControlCharacter,
                "the name of this file holds a control character, shown here as an escape. A \
                 tool that takes names one per line, or as the value of a step output, would \
                 misread it; rename the file",
            )
            .at(file.clone())
        })
        .collect();
    let declared = |field: String, path: &str| {
        Finding::error(
            Rule::ControlCharacter,
            format!(
                "{field} is \"{}\", which holds a control character, shown here as an escape. No \
                 tracked file may carry it either; write the path without it",
                shown(path)
            ),
        )
        .at(crate::audience::declaration_path())
    };
    for (name, component) in &declaration.component {
        let path = component.path.to_string();
        if holds_control(&path) {
            findings.push(
                declared(format!("the path of [component.{name}]"), &path)
                    .about(UnitName::Component(name.clone())),
            );
        }
    }
    for (table, entries) in [
        (
            "include",
            declaration
                .include
                .iter()
                .map(|entry| entry.path.to_string())
                .collect::<Vec<_>>(),
        ),
        (
            "exclude",
            declaration
                .exclude
                .iter()
                .map(|entry| entry.path.to_string())
                .collect(),
        ),
    ] {
        for (index, path) in entries.into_iter().enumerate() {
            if holds_control(&path) {
                findings.push(declared(
                    format!("the path of [[{table}]] entry {}", index + 1),
                    &path,
                ));
            }
        }
    }
    findings
}
