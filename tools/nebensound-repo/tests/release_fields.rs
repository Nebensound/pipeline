//! `minimum-version` on a unit without a version: a warning at the field, and
//! nothing else.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound_repo::finding::{Finding, Position, Rule, Severity, UnitName};
use nebensound_repo::names::ComponentName;
use nebensound_repo::path::RelativePath;
use nebensound_repo::release_fields::findings;
use support::{analysis, call, repository};

const FILES: &[&str] = &["README.md", "notes/a.md", "notes/b.md"];

/// A declaration whose component `notes` has this `versioning` and writes
/// `minimum-version`.
fn declaring(versioning: &str) -> String {
    format!(
        "[repo]\naudience = \"public\"\nminimum-version = \"1.0.0\"\n\n[component.notes]\npath = \
         \"notes\"\npipeline = \"none\"\nversioning = \"{versioning}\"\nminimum-version = \"0.1.0\"\n"
    )
}

fn declaration_path() -> RelativePath {
    RelativePath::new(".nebensound/repo.toml").unwrap()
}

fn found(source: &str) -> Vec<Finding> {
    findings(&analysis(source, FILES), &declaration_path(), source)
}

fn notes() -> UnitName {
    UnitName::Component(ComponentName::new("notes").unwrap())
}

#[test]
fn a_unit_without_a_version_is_warned_at_the_field() {
    let found = found(&declaring("none"));

    assert_eq!(found.len(), 1, "{found:?}");
    let finding = &found[0];
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.rule, Rule::ReleaseFieldWithoutVersion);
    assert_eq!(finding.unit, Some(notes()));
    assert_eq!(finding.path, Some(declaration_path()));
    // The line of the field, and the column of its value.
    assert_eq!(
        finding.position,
        Some(Position {
            line: 9,
            column: 19
        })
    );
    assert_eq!(
        finding.to_string(),
        "warning[release-field-without-version] component notes at \
         .nebensound/repo.toml:9:19: this unit writes minimum-version and carries no version of \
         its own, nor generates children that would take it over; nothing ever reads the value. \
         Give it a versioning value that includes self, or remove minimum-version"
    );
}

#[test]
fn the_place_is_found_in_an_inline_table_as_well() {
    let minimum = "[repo]\naudience = \"public\"\n\n[component]\nnotes = { path = \"notes\", pipeline = \"none\", \
                   versioning = \"none\", minimum-version = \"2.0.0\" }\n";
    let found_minimum = found(minimum);
    assert_eq!(found_minimum.len(), 1, "{found_minimum:?}");
    assert!(found_minimum[0].message.contains("writes minimum-version"));
    assert_eq!(
        found_minimum[0].position,
        Some(Position {
            line: 5,
            column: 85
        })
    );
}

#[test]
fn a_unit_with_a_version_is_not_warned() {
    for versioning in ["self", "self-and-child-files"] {
        let found = found(&declaring(versioning));
        assert!(found.is_empty(), "{versioning}: {found:?}");
    }
}

#[test]
fn a_unit_that_hands_the_field_to_its_children_is_not_warned() {
    // `child-files` carries no version itself; each generated unit takes the
    // field over, and carries a version it acts on.
    let found = found(&declaring("child-files"));
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_unit_without_a_version_that_does_not_write_the_field_is_not_warned() {
    let source = "[repo]\naudience = \"public\"\n\n[component.notes]\npath = \"notes\"\npipeline = \"none\"\n\
                  versioning = \"none\"\n";
    assert!(found(source).is_empty());
}

#[test]
fn a_declaration_source_that_does_not_read_names_no_place() {
    let declaration = declaring("none");
    let found = findings(&analysis(&declaration, FILES), &declaration_path(), "[repo");

    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].position.is_none());
    assert!(
        found[0].to_string().starts_with(
            "warning[release-field-without-version] component notes at .nebensound/repo.toml: "
        ),
        "{}",
        found[0]
    );
}

#[test]
fn phase_0_reports_the_warning_and_passes() {
    let declaration = declaring("none");
    let dir = repository(&[
        (".nebensound/repo.toml", &declaration),
        ("README.md", "# x\n"),
        ("notes/a.md", "# a\n"),
        ("notes/b.md", "# b\n"),
    ]);
    let root = dir.path().to_str().unwrap();

    let (text, code) = call(&["nebensound-repo", "-C", root, "check"]);
    assert_eq!(code, 0, "a warning stops nothing: {text}");
    assert!(
        text.contains(
            "warning[release-field-without-version] component notes at \
             .nebensound/repo.toml:9:19: "
        ),
        "{text}"
    );
}
