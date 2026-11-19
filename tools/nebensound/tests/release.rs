//! How each unit is released and its minimum version, as the report and
//! `explain` show them: the value, and where it came from.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound::report;
use nebensound_declaration::path::RelativePath;
use support::{Git, analysis, call, central, repository};

/// One unit of each kind: the repository, declared components whose release
/// mode their targets decide or that write it, and generated units.
const KINDS: &str = r#"[repo]
pipeline        = "rust-workspace"
minimum-version = "0.1.0"

[component.library]
path            = "crates/library"
pipeline        = "rust-library-public-package"
minimum-version = "1.0.0"

[component.tool]
path     = "crates/tool"
pipeline = "rust-library-package"
release  = "single-stage"

[component.service]
path     = "service"
pipeline = "rust-service"

[component.docs]
path            = "docs"
pipeline        = "typst-document"
versioning      = "child-files"
minimum-version = "0.2.0"
"#;

const KINDS_FILES: &[(&str, &str)] = &[
    (".nebensound/repo.toml", KINDS),
    ("README.md", "# Kinds\n"),
    ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
    ("Cargo.lock", "version = 4\n"),
    (
        "crates/library/Cargo.toml",
        "[package]\nname = \"library\"\n",
    ),
    ("crates/library/src/lib.rs", "//! A library.\n"),
    ("crates/tool/Cargo.toml", "[package]\nname = \"tool\"\n"),
    ("crates/tool/src/lib.rs", "//! A tool.\n"),
    ("service/Cargo.toml", "[package]\nname = \"service\"\n"),
    ("service/Cargo.lock", "version = 4\n"),
    ("service/Dockerfile", "FROM scratch\n"),
    ("service/src/main.rs", "fn main() {}\n"),
    ("docs/guide.typ", "= Guide\n"),
    ("docs/handbook.typ", "= Handbook\n"),
];

fn paths() -> Vec<&'static str> {
    KINDS_FILES.iter().map(|(path, _)| *path).collect()
}

fn path(value: &str) -> RelativePath {
    RelativePath::new(value).expect("a path in the fixture")
}

fn explained(path: &str) -> String {
    let analysis = analysis(KINDS, &paths());
    report::explain(&analysis, &self::path(path)).expect("the path is in the tree")
}

/// The line of a labelled row that names this unit.
fn row<'a>(text: &'a str, label: &str, unit: &str) -> &'a str {
    text.lines()
        .find(|line| line.trim_start().starts_with(label) && line.contains(&format!("{unit}: ")))
        .unwrap_or_else(|| panic!("no {label} row for {unit}:\n{text}"))
}

#[test]
fn the_fixture_passes_phase_0() {
    let analysis = analysis(KINDS, &paths());

    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
}

#[test]
fn explain_says_how_each_unit_containing_a_path_is_released() {
    let text = explained("crates/library/src/lib.rs");

    assert!(
        row(&text, "Release", "the repository")
            .ends_with("single-stage (the default for a unit that publishes nothing)"),
        "{text}"
    );
    assert!(
        row(&text, "Release", "component library")
            .ends_with("two-stage (the default for publishing to crates.io)"),
        "{text}"
    );
    assert!(
        row(&text, "Minimum version", "the repository").ends_with("0.1.0 (written)"),
        "{text}"
    );
    assert!(
        row(&text, "Minimum version", "component library").ends_with("1.0.0 (written)"),
        "{text}"
    );
    // Units that do not contain the path are not named.
    assert!(!text.contains("component tool"), "{text}");
}

#[test]
fn explain_says_when_a_written_value_wins_over_the_default() {
    let text = explained("crates/tool");

    assert!(
        row(&text, "Release", "component tool")
            .ends_with("single-stage (written, over the default two-stage)"),
        "{text}"
    );
    assert!(
        row(&text, "Minimum version", "component tool").ends_with("none"),
        "{text}"
    );
}

#[test]
fn explain_says_what_a_generated_unit_inherited() {
    let text = explained("docs/guide.typ");

    assert!(
        row(&text, "Release", "component guide")
            .ends_with("single-stage (inherited from component docs)"),
        "{text}"
    );
    assert!(
        row(&text, "Minimum version", "component guide")
            .ends_with("0.2.0 (inherited from component docs)"),
        "{text}"
    );
    assert!(
        row(&text, "Release", "component docs")
            .ends_with("single-stage (the default for publishing to release-assets)"),
        "{text}"
    );
}

#[test]
fn a_written_value_equal_to_the_default_says_so() {
    let analysis = analysis(
        "[repo]\npipeline = \"none\"\nrelease = \"single-stage\"\n",
        &["README.md"],
    );
    let text = report::explain(&analysis, &path("README.md")).expect("the file is in the tree");

    assert!(
        row(&text, "Release", "the repository")
            .ends_with("single-stage (written, the same as the default)"),
        "{text}"
    );
}

#[test]
fn the_configuration_names_both_fields_for_every_unit() {
    let analysis = analysis(KINDS, &paths());
    let text = report::configuration(&analysis, &central());

    // The repository, four declared components and two generated units.
    assert_eq!(text.matches("\n      Release ").count(), 7, "{text}");
    assert_eq!(
        text.matches("\n      Minimum version ").count(),
        7,
        "{text}"
    );
    assert!(
        text.contains("two-stage (the default for publishing to crates.io)"),
        "{text}"
    );
    assert!(
        text.contains("single-stage (the default for publishing to ghcr)"),
        "{text}"
    );
    assert!(
        text.contains("single-stage (written, over the default two-stage)"),
        "{text}"
    );
}

#[test]
fn explain_on_a_repository_shows_both_fields() {
    // The library is released in two stages, which needs the branch release.
    let git = Git::new();
    git.write(KINDS_FILES).commit("feat: one unit of each kind");
    git.promote();

    let (text, code) = call(&["nebensound", "-C", git.root(), "explain", "service"]);

    assert_eq!(code, 0, "{text}");
    assert!(
        row(&text, "Release", "component service")
            .ends_with("single-stage (the default for publishing to ghcr)"),
        "{text}"
    );
}

#[test]
fn an_invalid_minimum_version_fails_phase_0_at_its_line_and_column() {
    let dir = repository(&[
        (
            ".nebensound/repo.toml",
            "[repo]\nminimum-version = \"1.0\"\n",
        ),
        ("README.md", "# Broken\n"),
    ]);
    let root = dir.path().to_str().expect("a path in UTF-8");

    let (text, code) = call(&["nebensound", "-C", root, "check"]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains(".nebensound/repo.toml"), "{text}");
    assert!(text.contains("line 2, column 19"), "{text}");
    assert!(text.contains("X.Y.Z"), "{text}");
    assert!(!text.contains("Resolved configuration"), "{text}");
}
