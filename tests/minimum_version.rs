//! The version below which a unit's version never falls, and the field
//! `release`, which no declaration writes.

// A test that cannot read its own fixtures has nothing to say, so it should
// fail loudly instead of carrying a Result through every call. The workspace
// denies these for production code, where an unproven invariant hides in them;
// here the reason travels with the call and the failure is the point.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound_repo::finding::UnitName;
use nebensound_repo::names::ComponentName;
use nebensound_repo::path::RelativePath;
use nebensound_repo::resolve::{MinimumVersionSource, ResolvedMinimumVersion};
use nebensound_repo::{Central, Declaration, Tree, Unit, Version, analyse};
use support::{call, repository};

fn tree(paths: &[&str]) -> Tree {
    Tree::new(
        paths
            .iter()
            .map(|path| RelativePath::new(path).expect("a path in the fixture")),
    )
}

fn units(source: &str, paths: &[&str]) -> Vec<Unit> {
    let declaration = Declaration::parse(source).expect("a declaration in the fixture");
    let central = Central::builtin().expect("what is compiled into this crate");
    analyse(&declaration, &central, &tree(paths)).units
}

fn component(name: &str) -> UnitName {
    UnitName::Component(ComponentName::new(name).expect("a name"))
}

fn unit<'a>(units: &'a [Unit], name: &UnitName) -> &'a Unit {
    units
        .iter()
        .find(|unit| &unit.name == name)
        .unwrap_or_else(|| panic!("expected a unit {name}"))
}

fn refusal(source: &str) -> String {
    match Declaration::parse(source) {
        Ok(_) => panic!("expected this declaration to be refused:\n{source}"),
        Err(error) => error.to_string(),
    }
}

const FILES: &[&str] = &[
    "README.md",
    "Cargo.toml",
    "Cargo.lock",
    "crates/library/Cargo.toml",
    "crates/library/src/lib.rs",
    "docs/a.typ",
    "docs/b.typ",
];

#[test]
fn a_minimum_version_is_taken_as_written() {
    let units = units(
        r#"
[repo]
audience = "public"
pipeline        = "none"
minimum-version = "1.0.0"

[component.library]
path     = "crates/library"
pipeline = "rust-library-public"
"#,
        FILES,
    );

    assert_eq!(
        unit(&units, &UnitName::Repository).minimum_version,
        Some(ResolvedMinimumVersion {
            version: Version::new(1, 0, 0),
            source: MinimumVersionSource::Written,
        })
    );
    assert_eq!(unit(&units, &component("library")).minimum_version, None);
}

#[test]
fn a_generated_unit_inherits_the_minimum_version_from_its_creator() {
    let units = units(
        r#"
[repo]
audience = "public"
pipeline = "none"

[component.docs]
path            = "docs"
pipeline        = "typst-document"
versioning      = "child-files"
minimum-version = "0.1.0"
"#,
        FILES,
    );

    for child in ["a", "b"] {
        assert_eq!(
            unit(&units, &component(child)).minimum_version,
            Some(ResolvedMinimumVersion {
                version: Version::new(0, 1, 0),
                source: MinimumVersionSource::Inherited {
                    by: component("docs"),
                },
            })
        );
    }

    // A creator without a minimum version passes none on.
    let units = self::units(
        "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.docs]\npath = \"docs\"\npipeline = \
         \"typst-document\"\nversioning = \"child-files\"\n",
        FILES,
    );
    assert_eq!(unit(&units, &component("a")).minimum_version, None);
}

#[test]
fn an_invalid_minimum_version_stops_the_reading_at_its_line_and_column() {
    let message = refusal("[repo]\npipeline = \"none\"\nminimum-version = \"v1.0\"\n");

    assert!(message.contains("line 3, column 19"), "{message}");
    assert!(message.contains("X.Y.Z"), "{message}");
    assert!(message.contains("\"v1.0\""), "{message}");

    let message = refusal("[repo]\nminimum-version = 1\n");
    assert!(message.contains("string"), "{message}");
}

#[test]
fn a_minimum_version_with_a_pre_release_part_is_a_read_error_that_names_the_field() {
    for value in ["1.0.0-pre.1", "1.0.0-rc.1"] {
        let message = refusal(&format!(
            "[repo]\n\n[component.thing]\npath = \".\"\nminimum-version = \"{value}\"\n"
        ));
        assert!(message.contains("line 5, column 19"), "{message}");
        assert!(message.contains("minimum-version"), "{message}");
        assert!(message.contains("X.Y.Z"), "{message}");
        assert!(message.contains(&format!("\"{value}\"")), "{message}");
    }
}

#[test]
fn a_declaration_that_writes_release_is_a_read_error_that_names_the_field() {
    for source in [
        "[repo]\naudience = \"public\"\nrelease = \"main\"\n",
        "[repo]\naudience = \"public\"\n\n[component.thing]\npath = \".\"\nrelease = \"main\"\n",
    ] {
        let message = refusal(source);
        assert!(message.contains("unknown field `release`"), "{message}");
    }
}

#[test]
fn a_declaration_that_writes_release_stops_phase_0() {
    let dir = repository(&[
        (
            ".nebensound/repo.toml",
            "[repo]\naudience = \"public\"\npipeline = \"none\"\nrelease = \"main\"\n",
        ),
        ("README.md", "# x\n"),
    ]);
    let root = dir.path().to_str().expect("a path in UTF-8");

    let (text, code) = call(&["nebensound-repo", "-C", root, "check"]);
    assert_ne!(code, 0, "{text}");
    assert!(text.contains("unknown field `release`"), "{text}");
    assert!(text.contains("line 4"), "{text}");
}
