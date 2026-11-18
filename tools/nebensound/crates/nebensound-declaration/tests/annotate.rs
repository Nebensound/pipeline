//! The annotated tree: what stands at a node, and what stays beside it.

// A fixture that cannot read itself has nothing to say. In production code an
// unproven assumption hides in these calls; here the reason travels with the
// call and the failure is the point.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use nebensound_declaration::annotate::Coverage;
use nebensound_declaration::finding::{Rule, UnitName};
use nebensound_declaration::names::{CheckName, ComponentName};
use nebensound_declaration::path::RelativePath;
use nebensound_declaration::{Analysis, Central, Declaration, Tree, analyse};

const FILES: &[&str] = &[
    "README.md",
    "LICENSE",
    "Cargo.toml",
    "Cargo.lock",
    ".nebensound/repo.toml",
    "crates/thing/Cargo.toml",
    "crates/thing/src/lib.rs",
    "doc/logo.png",
    "doc/notes.xyz",
];

const WORKSPACE: &str = "\
[repo]
pipeline = { check = [\"rust\"], dependabot = [\"cargo\"] }

[component.thing]
path     = \"crates/thing\"
pipeline = \"rust-library-package\"
";

fn run(source: &str, paths: &[&str]) -> Analysis {
    let declaration = Declaration::parse(source).expect("a declaration in the fixture");
    let central = Central::builtin().expect("what is compiled into this crate");
    let tree = Tree::new(
        paths
            .iter()
            .map(|path| RelativePath::new(path).expect("a path in the fixture")),
    );
    analyse(&declaration, &central, &tree)
}

fn path(value: &str) -> RelativePath {
    RelativePath::new(value).expect("a path in the fixture")
}

fn check(name: &str) -> CheckName {
    CheckName::new(name).expect("a check name in the fixture")
}

fn component(name: &str) -> UnitName {
    UnitName::Component(ComponentName::new(name).expect("a component name in the fixture"))
}

#[test]
fn one_file_makes_a_tree_with_one_node() {
    let analysis = run("[repo]\n", &["README.md"]);

    assert_eq!(analysis.tree.nodes().len(), 1);
    let node = &analysis.tree.nodes()[0];
    assert_eq!(node.path, path("README.md"));
    assert_eq!(node.coverage(), Some(&Coverage::Covered(check("markdown"))));
    assert!(node.children().is_empty(), "a file has nothing below it");
}

#[test]
fn at_every_node_stands_who_contains_it_and_what_covers_it() {
    let analysis = run(WORKSPACE, FILES);

    let file = analysis
        .tree
        .at(&path("crates/thing/src/lib.rs"))
        .expect("the Rust file is in the tree");
    assert_eq!(
        file.units,
        vec![UnitName::Repository, component("thing")],
        "both units contain it"
    );
    assert!(file.checks.contains(&check("rust")), "{:?}", file.checks);
    assert!(
        file.checks.contains(&check("markdown")),
        "the base set holds here as well: {:?}",
        file.checks
    );
    assert_eq!(file.coverage(), Some(&Coverage::Covered(check("rust"))));

    // The folder carries the same units and no coverage: the promise a check
    // makes is about files.
    let folder = analysis
        .tree
        .at(&path("crates/thing"))
        .expect("the folder is in the tree");
    assert_eq!(folder.units, vec![UnitName::Repository, component("thing")]);
    assert_eq!(folder.coverage(), None);
    assert!(!folder.children().is_empty());
}

#[test]
fn the_units_at_a_node_stand_outermost_first() {
    // The resolution lists components by name. Without ordering by depth, the
    // inner unit would stand before the one containing it, because "a" sorts
    // before "z".
    let analysis = run(
        "\
[repo]
pipeline = { check = [\"rust\"] }

[component.a]
path = \"outer/inner\"

[component.z]
path = \"outer\"
",
        &["README.md", "outer/inner/thing.rs"],
    );

    let file = analysis
        .tree
        .at(&path("outer/inner/thing.rs"))
        .expect("the file is in the tree");
    assert_eq!(
        file.units,
        vec![UnitName::Repository, component("z"), component("a")],
        "the repository contains outer, and outer contains outer/inner"
    );
}

#[test]
fn the_gap_is_a_filter_over_the_tree() {
    let analysis = run(WORKSPACE, FILES);

    let gaps: Vec<String> = analysis
        .tree
        .gaps()
        .into_iter()
        .map(|(node, _)| node.path.to_string())
        .collect();
    assert_eq!(gaps, vec!["doc/notes.xyz".to_owned()]);
    assert_eq!(analysis.tree.files().len(), FILES.len());
}

#[test]
fn an_exclusion_stands_at_the_node_with_its_pattern_and_reason() {
    let analysis = run(WORKSPACE, FILES);

    let node = analysis.tree.at(&path("doc/logo.png")).expect("the image");
    let Some(Coverage::Excluded { pattern, reason }) = node.coverage() else {
        panic!("the image is excused, not {:?}", node.coverage());
    };
    assert_eq!(pattern.to_string(), "*.png");
    assert!(
        !reason.as_str().is_empty(),
        "an exclusion carries its reason"
    );
    assert!(node.coverage().is_some_and(Coverage::settled));
}

#[test]
fn a_file_without_a_check_in_force_stays_open() {
    let analysis = run("[repo]\n", &["README.md", "src/main.rs"]);

    let node = analysis
        .tree
        .at(&path("src/main.rs"))
        .expect("the Rust file");
    assert_eq!(node.coverage(), Some(&Coverage::Uncovered(check("rust"))));
    assert!(!node.coverage().is_some_and(Coverage::settled));
    assert_eq!(
        node.findings.iter().map(|f| f.rule).collect::<Vec<_>>(),
        vec![Rule::UncoveredFile],
        "the finding hangs at the file it is about"
    );
}

#[test]
fn findings_hang_at_the_node_and_the_rest_stays_beside_it() {
    let analysis = run(WORKSPACE, FILES);

    let node = analysis
        .tree
        .at(&path("doc/notes.xyz"))
        .expect("the foreign file");
    assert_eq!(
        node.findings.iter().map(|f| f.rule).collect::<Vec<_>>(),
        vec![Rule::LanguageWithoutCheck]
    );
    assert!(
        analysis
            .loose
            .iter()
            .all(|finding| finding.rule != Rule::LanguageWithoutCheck),
        "what hangs at a file does not stand beside it a second time"
    );
    assert!(analysis.failed());

    let counted: usize = analysis
        .tree
        .walk()
        .iter()
        .map(|node| node.findings.len())
        .sum();
    assert_eq!(analysis.findings().len(), analysis.loose.len() + counted);
}

#[test]
fn a_finding_about_a_folder_hangs_at_the_folder() {
    let analysis = run(
        "\
[repo]
pipeline = { check = [\"rust\"] }

[component.crates]
path     = \"crates\"
pipeline = { check = [\"python\"] }
",
        &["README.md", "crates/thing/src/lib.rs"],
    );

    let folder = analysis.tree.at(&path("crates")).expect("the folder");
    assert_eq!(
        folder.findings.iter().map(|f| f.rule).collect::<Vec<_>>(),
        vec![Rule::UnusedCheck],
        "a check that finds nothing hangs at the unit that runs it"
    );
}

#[test]
fn what_hangs_at_no_path_stays_beside_the_tree() {
    let analysis = run(
        "\
[repo]
pipeline = \"none\"

[component.gone]
path     = \"nowhere\"
pipeline = \"none\"
",
        &["README.md"],
    );

    assert!(
        analysis
            .loose
            .iter()
            .any(|finding| finding.rule == Rule::MissingPath),
        "{:?}",
        analysis.loose
    );
    assert!(
        analysis
            .tree
            .walk()
            .iter()
            .all(|node| node.findings.is_empty()),
        "a path the repository does not have has no node either"
    );
}

#[test]
fn a_query_finds_the_node_or_nothing() {
    let analysis = run(WORKSPACE, FILES);

    assert!(analysis.tree.at(&path("README.md")).is_some());
    assert!(analysis.tree.at(&path("doc")).is_some());
    assert!(
        analysis.tree.at(&path("crates/other")).is_none(),
        "a sibling that does not exist"
    );
    assert!(analysis.tree.at(&path("nowhere/at/all")).is_none());
}
