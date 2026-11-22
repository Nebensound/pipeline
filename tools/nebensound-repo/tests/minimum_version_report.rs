//! The minimum version of each unit, as the report and `explain` show it:
//! the value, and where it came from.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound_repo::path::RelativePath;
use nebensound_repo::report;
use support::{Git, analysis, call, central, repository};

/// One unit of each kind: the repository, declared components with and without
/// a minimum version, and generated units.
const KINDS: &str = r#"[repo]
audience = "public"
pipeline        = "rust-workspace"
minimum-version = "0.1.0"

[component.library]
path            = "crates/library"
pipeline        = "rust-library-public-package"
minimum-version = "1.0.0"

[component.tool]
path     = "crates/tool"
pipeline = "rust-library-package"

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
    ("service/deploy/Dockerfile", "FROM scratch\n"),
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

/// Whether a text holds a row of this label.
fn has_row(text: &str, label: &str) -> bool {
    text.lines()
        .any(|line| line.trim_start().starts_with(&format!("{label} ")))
}

#[test]
fn explain_names_the_minimum_version_of_each_unit_containing_a_path() {
    let text = explained("crates/library/src/lib.rs");

    assert!(
        row(&text, "Minimum version", "the repository").ends_with("0.1.0 (written)"),
        "{text}"
    );
    assert!(
        row(&text, "Minimum version", "component library").ends_with("1.0.0 (written)"),
        "{text}"
    );
    // Units that do not contain the path are not named, and no row says how
    // a unit is released.
    assert!(!text.contains("component tool"), "{text}");
    assert!(!has_row(&text, "Release"), "{text}");
}

#[test]
fn explain_says_none_for_a_unit_without_a_minimum_version() {
    let text = explained("crates/tool");

    assert!(
        row(&text, "Minimum version", "component tool").ends_with("none"),
        "{text}"
    );
}

#[test]
fn explain_says_what_a_generated_unit_inherited() {
    let text = explained("docs/guide.typ");

    assert!(
        row(&text, "Minimum version", "component guide")
            .ends_with("0.2.0 (inherited from component docs)"),
        "{text}"
    );
}

#[test]
fn the_configuration_names_the_minimum_version_of_every_unit() {
    let analysis = analysis(KINDS, &paths());
    let text = report::configuration(&analysis, &central());

    // The repository, four declared components and two generated units.
    assert_eq!(
        text.matches("\n      Minimum version ").count(),
        7,
        "{text}"
    );
    assert!(!has_row(&text, "Release"), "{text}");
}

#[test]
fn explain_on_a_repository_shows_the_minimum_version() {
    let git = Git::new();
    git.write(KINDS_FILES).commit("feat: one unit of each kind");

    let (text, code) = call(&["nebensound-repo", "-C", git.root(), "explain", "service"]);

    assert_eq!(code, 0, "{text}");
    assert!(
        row(&text, "Minimum version", "component service").ends_with("none"),
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

    let (text, code) = call(&["nebensound-repo", "-C", root, "check"]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains(".nebensound/repo.toml"), "{text}");
    assert!(text.contains("line 2, column 19"), "{text}");
    assert!(text.contains("X.Y.Z"), "{text}");
    assert!(!text.contains("Resolved configuration"), "{text}");
}
