//! `minimum-version` on a unit that has no version for it.
//!
//! The field acts on a unit's version: it says how low the version may be. A
//! unit that carries no version of its own and generates no children to hand
//! it on to has no version for it to act on. The value is accepted and shown
//! in the report, and nothing ever reads it. Phase 0 says so, at the place in
//! the declaration where the field is written, and changes no version.
//!
//! The rule needs the text of the declaration for that place, which is why it
//! stands beside the rules of [`validate`](mod@crate::validate) rather than
//! among them: those are handed the declaration as it was read, without its
//! text.

use toml_edit::{Document, Item};

use crate::Analysis;
use crate::finding::{Finding, Position, Rule, UnitName};
use crate::path::RelativePath;

/// A warning for each `minimum-version` written on a unit that carries no
/// version and generates no children, at the line and column of the field's
/// value.
///
/// `declaration` is where the declaration lies in the repository, and `source`
/// what it holds. Should the text not read, the findings name no place rather
/// than none at all.
#[must_use]
pub fn findings(analysis: &Analysis, declaration: &RelativePath, source: &str) -> Vec<Finding> {
    let document = Document::parse(source).ok();
    let mut findings = Vec::new();
    for unit in &analysis.units {
        // The repository always carries a version; a generated unit carries
        // one as well, and takes the field from the unit that generated it.
        let UnitName::Component(name) = &unit.name else {
            continue;
        };
        if unit.carries_version
            || unit.versioning.generates_children()
            || unit.minimum_version.is_none()
        {
            continue;
        }
        let finding = Finding::warning(
            Rule::ReleaseFieldWithoutVersion,
            "this unit writes minimum-version and carries no version of its own, nor generates \
             children that would take it over; nothing ever reads the value. Give it a \
             versioning value that includes self, or remove minimum-version",
        )
        .about(unit.name.clone())
        .at(declaration.clone());
        let place = document
            .as_ref()
            .and_then(|document| document.get("component"))
            .and_then(|components| components.get(name.as_str()))
            .and_then(|table| table.get("minimum-version"))
            .and_then(Item::span)
            .map(|span| Position::at(source, span.start));
        findings.push(match place {
            Some(position) => finding.on(position),
            None => finding,
        });
    }
    findings
}
