//! `unreleased-requirement`: what a package a pull request would publish to a
//! registry requires by path, and the pull request does not touch, has a
//! release already, or the pull request fails.
//!
//! The fixtures are real Git repositories, built commit by commit, and the
//! pull request is the merge commit GitHub builds for it, read as the job
//! `Version` reads it.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound_repo::names::VersionWriterName;
use nebensound_repo::path::RelativePath;
use nebensound_repo::repository::Repository;
use nebensound_repo::writer::{self, Order};
use support::{Git, call};

/// A repository and three packages, `app` requiring `mid` by path and `mid`
/// requiring `lib` for its tests, `mid` and `lib` libraries for the private
/// registry. `app` takes the pipeline given.
fn declaration(app: &str) -> String {
    format!(
        r#"[repo]
audience = "public"
pipeline = "none"

[component.app]
path     = "app"
pipeline = {app}

[component.mid]
path     = "mid"
pipeline = "rust-library-package"

[component.lib]
path     = "lib"
pipeline = "rust-library-package"
"#
    )
}

/// `app` as a library for the private registry.
const FOR_A_REGISTRY: &str = "\"rust-library-package\"";

const APP: &str = "[package]
name    = \"app\"
edition = \"2024\"

[dependencies]
mid = { path = \"../mid\" }
";

const MID: &str = "[package]
name    = \"mid\"
edition = \"2024\"

[dev-dependencies]
lib = { path = \"../lib\" }
";

const LIB: &str = "[package]\nname    = \"lib\"\nedition = \"2024\"\n";

/// The repository with one commit that gives every unit `0.0.1` and records
/// nothing, as in a repository that called the pipeline before the release
/// stage existed.
fn fixture(app: &str) -> Git {
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", &declaration(app)),
        ("README.md", "# Packages\n"),
        ("app/Cargo.toml", APP),
        ("mid/Cargo.toml", MID),
        ("lib/Cargo.toml", LIB),
    ]);
    git.commit("chore: start");
    git
}

/// The merge commit GitHub builds for a pull request that writes these files,
/// checked out detached on top of `main`.
fn pull_request(git: &Git, files: &[(&str, &str)]) {
    git.run(&["checkout", "--quiet", "-b", "topic"]);
    git.write(files);
    git.commit("wip");
    git.run(&["checkout", "--quiet", "--detach", "main"]);
    git.run(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "Merge topic",
        "topic",
    ]);
}

/// `version` as the job `Version` calls it on the pull request, with these
/// arguments in front.
fn version(git: &Git, args: &[&str]) -> (String, u8) {
    let mut all = vec!["nebensound-repo", "-C", git.root(), "version"];
    all.extend(args);
    all.extend(["--pull-request", "7", "--message", "fix(app): the change"]);
    call(&all)
}

/// The finding about the unreleased `lib`.
const FINDING: &str = "error[unreleased-requirement] component app: the package app goes to \
                       private-registry with this pull request, where packages require each \
                       other by version, and it requires component lib at 0.0.1, its current \
                       version, by path. No tag lib-v0.0.1 records that version, so the package \
                       would require a version that was never released. Touch component lib in \
                       this pull request, with a change below lib or a change of \
                       .nebensound/repo.toml, so that it gets a new version that is released \
                       with the merge\n";

#[test]
fn a_package_for_a_registry_fails_on_a_requirement_without_a_record() {
    let git = fixture(FOR_A_REGISTRY);
    // `mid` has its record; `lib`, which `app` reaches through it, has none.
    git.tag("mid-v0.0.1", &git.head());
    pull_request(&git, &[("app/README.md", "# App\n")]);

    let (text, code) = version(&git, &[]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.starts_with("there are no versions: phase 0 found errors\n"),
        "{text}"
    );
    assert!(text.contains(FINDING), "{text}");
    assert_eq!(text.matches("unreleased-requirement").count(), 1, "{text}");

    let (markdown, code) = version(&git, &["--markdown"]);
    assert_eq!(code, 1, "{markdown}");
    assert!(
        markdown.contains("- error\\[unreleased\\-requirement\\] component app"),
        "{markdown}"
    );

    // Only a pull request is held to it: on `main` the merge has happened,
    // and phase 0 passes there.
    let (text, code) = call(&["nebensound-repo", "-C", git.root(), "version"]);
    assert_eq!(code, 0, "{text}");
    let (text, code) = call(&["nebensound-repo", "-C", git.root(), "check"]);
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("unreleased-requirement"), "{text}");
}

#[test]
fn every_unit_reached_without_a_record_is_named() {
    let git = fixture(FOR_A_REGISTRY);
    pull_request(&git, &[("app/README.md", "# App\n")]);

    let (text, code) = version(&git, &[]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains(FINDING), "{text}");
    assert!(
        text.contains(
            "it requires component mid at 0.0.1, its current version, by path. No tag \
                       mid-v0.0.1 records"
        ),
        "{text}"
    );
    assert_eq!(text.matches("unreleased-requirement").count(), 2, "{text}");
}

#[test]
fn the_same_history_passes_with_the_records() {
    let git = fixture(FOR_A_REGISTRY);
    git.tag("mid-v0.0.1", &git.head());
    git.tag("lib-v0.0.1", &git.head());
    pull_request(&git, &[("app/README.md", "# App\n")]);

    let (text, code) = version(&git, &[]);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn the_same_history_passes_when_the_pull_request_touches_the_required_unit() {
    // A change below the unit's path.
    let git = fixture(FOR_A_REGISTRY);
    git.tag("mid-v0.0.1", &git.head());
    pull_request(
        &git,
        &[("app/README.md", "# App\n"), ("lib/README.md", "# Lib\n")],
    );
    let (text, code) = version(&git, &[]);
    assert_eq!(code, 0, "{text}");

    // A change of the declaration, which touches every unit.
    let git = fixture(FOR_A_REGISTRY);
    let changed = format!("{}# Touched.\n", declaration(FOR_A_REGISTRY));
    pull_request(&git, &[(".nebensound/repo.toml", &changed)]);
    let (text, code) = version(&git, &[]);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn the_same_history_passes_when_the_package_has_no_such_target() {
    // A release asset needs no record of what the package requires.
    let git = fixture(
        "{ version = [\"cargo\"], check = [], build = \"cargo-release\", publish = \
         [\"release-assets\"] }",
    );
    pull_request(&git, &[("app/README.md", "# App\n")]);

    let (text, code) = version(&git, &[]);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn a_manifest_that_does_not_read_fails_the_call() {
    let git = fixture(FOR_A_REGISTRY);
    pull_request(&git, &[("app/Cargo.toml", "[package\n")]);

    let (text, code) = version(&git, &[]);
    assert_eq!(code, 1, "{text}");
    assert!(text.starts_with("app/Cargo.toml is not TOML: "), "{text}");

    // The same for a unit reached on the way that holds no package.
    let git = fixture(FOR_A_REGISTRY);
    pull_request(
        &git,
        &[("app/README.md", "# App\n"), ("mid/Cargo.toml", "[lib]\n")],
    );
    let (text, code) = version(&git, &[]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.starts_with(
            "component mid selects the version writer cargo, and mid/Cargo.toml has no [package]"
        ),
        "{text}"
    );
}

#[test]
fn the_writer_starts_nothing_from_a_unit_it_gives_no_version_of_the_commit() {
    let git = fixture(FOR_A_REGISTRY);
    let repository = Repository::at(git.path());
    let order = |writers: &[&str], version: Option<&str>| Order {
        unit: "component app".to_owned(),
        path: RelativePath::new("app").expect("a path"),
        version: version.map(ToOwned::to_owned),
        current: Some("0.0.1".to_owned()),
        writers: writers
            .iter()
            .map(|writer| VersionWriterName::new(writer).expect("a name"))
            .collect(),
    };

    // Not a unit of the writer `cargo`.
    let orders = [order(&[], Some("0.0.2"))];
    assert_eq!(writer::requirements(&repository, &orders, 0).unwrap(), None);
    // Not touched by the commit.
    let orders = [order(&["cargo"], None)];
    assert_eq!(writer::requirements(&repository, &orders, 0).unwrap(), None);
    // Touched, with nothing it requires among the orders.
    let orders = [order(&["cargo"], Some("0.0.2"))];
    let required = writer::requirements(&repository, &orders, 0)
        .unwrap()
        .expect("requirements");
    assert_eq!(required.package, "app");
    assert!(required.untouched.is_empty());
}
