//! `release` and `minimum-version` on a unit that has no version for them.
//!
//! Both fields act on a unit's version: one says how it reaches a full
//! release, the other how low it may be. A unit that carries no version of its
//! own and generates no children to hand them on to has no version for either
//! to act on. The value is accepted and shown in the report, and nothing ever
//! reads it. Phase 0 says so, at the place in the declaration where the field
//! is written, and changes no version.
//!
//! The rule needs the text of the declaration for that place, which is why it
//! stands beside the rules of [`validate`](mod@crate::validate) rather than
//! among them: those are handed the declaration as it was read, without its
//! text.

use toml_edit::{Document, Item};

use crate::Analysis;
use crate::finding::{Finding, Position, Rule, UnitName};
use crate::path::RelativePath;
use crate::resolve::ReleaseSource;

/// A warning for each `release` and `minimum-version` written on a unit that
/// carries no version and generates no children, in that order per unit, at
/// the line and column of the field's value.
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
        // one as well, and takes both fields from the unit that generated it.
        let UnitName::Component(name) = &unit.name else {
            continue;
        };
        if unit.carries_version || unit.versioning.generates_children() {
            continue;
        }
        let written = [
            (
                "release",
                matches!(unit.release.source, ReleaseSource::Written { .. }),
            ),
            ("minimum-version", unit.minimum_version.is_some()),
        ];
        for (field, _) in written.into_iter().filter(|(_, written)| *written) {
            let finding = Finding::warning(
                Rule::ReleaseFieldWithoutVersion,
                format!(
                    "this unit writes {field} and carries no version of its own, nor generates \
                     children that would take it over; nothing ever reads the value. Give it a \
                     versioning value that includes self, or remove {field}"
                ),
            )
            .about(unit.name.clone())
            .at(declaration.clone());
            let place = document
                .as_ref()
                .and_then(|document| document.get("component"))
                .and_then(|components| components.get(name.as_str()))
                .and_then(|table| table.get(field))
                .and_then(Item::span)
                .map(|span| Position::at(source, span.start));
            findings.push(match place {
                Some(position) => finding.on(position),
                None => finding,
            });
        }
    }
    findings
}
