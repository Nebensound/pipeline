//! How a unit is released, and the version below which it never falls.

// A test that cannot read its own fixtures has nothing to say, so it should
// fail loudly instead of carrying a Result through every call. The workspace
// denies these for production code, where an unproven invariant hides in them;
// here the reason travels with the call and the failure is the point.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use nebensound_declaration::finding::UnitName;
use nebensound_declaration::names::{ComponentName, TargetName};
use nebensound_declaration::path::RelativePath;
use nebensound_declaration::register::Register;
use nebensound_declaration::resolve::{
    MinimumVersionSource, ReleaseSource, ResolvedMinimumVersion, ResolvedRelease,
};
use nebensound_declaration::{
    Central, Declaration, Release, TagVersion, Tree, Unit, Version, analyse,
};

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

fn targets(names: &[&str]) -> Vec<TargetName> {
    names
        .iter()
        .map(|name| TargetName::new(name).expect("a target name"))
        .collect()
}

fn by_targets(release: Release, deciding: &[&str]) -> ResolvedRelease {
    ResolvedRelease {
        release,
        source: ReleaseSource::Targets {
            deciding: targets(deciding),
        },
    }
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
    "service/Cargo.toml",
    "service/src/main.rs",
    "service/Dockerfile",
    "docs/a.typ",
    "docs/b.typ",
];

#[test]
fn the_default_follows_the_publish_targets() {
    let units = units(
        r#"
[repo]
pipeline = "rust-workspace"

[component.library]
path     = "crates/library"
pipeline = "rust-library-public"

[component.service]
path     = "service"
pipeline = "rust-service"
"#,
        FILES,
    );

    // A public library goes to crates.io, which releases in two stages; a
    // service goes to ghcr, which releases in one. Neither wrote the field.
    assert_eq!(
        unit(&units, &component("library")).release,
        by_targets(Release::TwoStage, &["crates.io"])
    );
    assert_eq!(
        unit(&units, &component("service")).release,
        by_targets(Release::SingleStage, &["ghcr"])
    );
    // Publishing nothing, the repository is released in one stage.
    assert_eq!(
        unit(&units, &UnitName::Repository).release,
        by_targets(Release::SingleStage, &[])
    );
}

#[test]
fn one_target_in_two_stages_takes_the_unit_to_two_stages() {
    let units = units(
        r#"
[repo]
pipeline = { build = "cargo-package", publish = ["release-assets", "private-registry", "ghcr"] }
"#,
        FILES,
    );

    // Only the targets that decided are named: the one releasing in two.
    assert_eq!(
        unit(&units, &UnitName::Repository).release,
        by_targets(Release::TwoStage, &["private-registry"])
    );

    let units = self::units(
        "[repo]\npipeline = { build = \"docker\", publish = [\"ghcr\", \"pages\"] }\n",
        FILES,
    );
    // When none of them does, every target decided.
    assert_eq!(
        unit(&units, &UnitName::Repository).release,
        by_targets(Release::SingleStage, &["ghcr", "pages"])
    );
}

#[test]
fn a_written_value_wins_over_the_default_and_keeps_it_known() {
    let units = units(
        r#"
[repo]
pipeline = "none"
release  = "two-stage"

[component.library]
path     = "crates/library"
pipeline = "rust-library-public"
release  = "single-stage"
"#,
        FILES,
    );

    assert_eq!(
        unit(&units, &component("library")).release,
        ResolvedRelease {
            release: Release::SingleStage,
            source: ReleaseSource::Written {
                default: Release::TwoStage,
            },
        }
    );
    assert_eq!(
        unit(&units, &UnitName::Repository).release,
        ResolvedRelease {
            release: Release::TwoStage,
            source: ReleaseSource::Written {
                default: Release::SingleStage,
            },
        }
    );
}

#[test]
fn a_minimum_version_is_taken_as_written() {
    let units = units(
        r#"
[repo]
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
            version: TagVersion::stable(Version::new(1, 0, 0)),
            source: MinimumVersionSource::Written,
        })
    );
    assert_eq!(unit(&units, &component("library")).minimum_version, None);
}

#[test]
fn a_generated_unit_inherits_both_from_its_creator() {
    let units = units(
        r#"
[repo]
pipeline = "none"

[component.docs]
path            = "docs"
pipeline        = "typst-document"
versioning      = "child-files"
release         = "two-stage"
minimum-version = "0.1.0"
"#,
        FILES,
    );

    for child in ["a", "b"] {
        let child = unit(&units, &component(child));
        assert_eq!(
            child.release,
            ResolvedRelease {
                release: Release::TwoStage,
                source: ReleaseSource::Inherited {
                    by: component("docs"),
                },
            }
        );
        assert_eq!(
            child.minimum_version,
            Some(ResolvedMinimumVersion {
                version: TagVersion::stable(Version::new(0, 1, 0)),
                source: MinimumVersionSource::Inherited {
                    by: component("docs"),
                },
            })
        );
    }

    // A creator without a minimum version passes none on.
    let units = self::units(
        "[repo]\npipeline = \"none\"\n\n[component.docs]\npath = \"docs\"\npipeline = \
         \"typst-document\"\nversioning = \"child-files\"\n",
        FILES,
    );
    let child = unit(&units, &component("a"));
    assert_eq!(child.minimum_version, None);
    assert_eq!(child.release.release, Release::SingleStage);
}

#[test]
fn a_minimum_version_may_name_a_pre_release() {
    let units = units(
        r#"
[repo]
pipeline = "none"

[component.library]
path            = "crates/library"
pipeline        = "rust-library-public"
minimum-version = "1.0.0-pre.10"
"#,
        FILES,
    );

    assert_eq!(
        unit(&units, &component("library")).minimum_version,
        Some(ResolvedMinimumVersion {
            version: TagVersion::pre(Version::new(1, 0, 0), 10),
            source: MinimumVersionSource::Written,
        })
    );
}

#[test]
fn an_invalid_minimum_version_stops_the_reading_at_its_line_and_column() {
    let message = refusal("[repo]\npipeline = \"none\"\nminimum-version = \"v1.0\"\n");

    assert!(message.contains("line 3, column 19"), "{message}");
    assert!(message.contains("X.Y.Z"), "{message}");
    assert!(message.contains("\"v1.0\""), "{message}");

    // A pre-release reads only as pre.N.
    let message =
        refusal("[repo]\n\n[component.thing]\npath = \".\"\nminimum-version = \"1.0.0-rc.1\"\n");
    assert!(message.contains("line 5"), "{message}");
    assert!(message.contains("\"rc.1\""), "{message}");
    assert!(message.contains("pre.N"), "{message}");

    let message = refusal("[repo]\nminimum-version = 1\n");
    assert!(message.contains("string"), "{message}");
}

#[test]
fn a_release_mode_is_one_of_two_values() {
    let message = refusal("[repo]\nrelease = \"three-stage\"\n");

    assert!(message.contains("line 2"), "{message}");
    assert!(message.contains("single-stage"), "{message}");
    assert!(message.contains("two-stage"), "{message}");
}

#[test]
fn every_target_says_how_a_unit_publishing_there_is_released() {
    let register = Register::builtin().expect("the register this crate was built with");
    let release = |name: &str| {
        register
            .target
            .get(&TargetName::new(name).expect("a name"))
            .unwrap_or_else(|| panic!("the {name} target"))
            .release
    };

    for name in ["crates.io", "private-registry"] {
        assert_eq!(release(name), Release::TwoStage, "{name}");
    }
    for name in ["ghcr", "pages", "release-assets"] {
        assert_eq!(release(name), Release::SingleStage, "{name}");
    }
    assert_eq!(register.target.len(), 5, "every target is named above");

    // A target that leaves it open is refused: it would leave the default of
    // every unit publishing there open as well.
    let message = match Register::parse(
        "[version]\n[check]\n[build]\n[target.t]\naccepts = [\"thing\"]\n[preset]\n[ecosystem]\n",
    ) {
        Ok(_) => panic!("a target without a release mode was accepted"),
        Err(error) => error.to_string(),
    };
    assert!(message.contains("release"), "{message}");
}

#[test]
fn a_release_mode_reads_as_it_is_written() {
    assert_eq!(Release::SingleStage.to_string(), "single-stage");
    assert_eq!(Release::TwoStage.as_str(), "two-stage");
}
