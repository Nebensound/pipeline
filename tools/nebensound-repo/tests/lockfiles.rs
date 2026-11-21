//! A manifest whose ecosystem knows a lockfile commits that lockfile.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use nebensound_repo::finding::{Finding, Rule, Severity};
use nebensound_repo::lockfiles::findings;
use nebensound_repo::path::RelativePath;
use nebensound_repo::register::{BUILTIN_REGISTER, Register};
use support::{analysis, call, git, repository};

/// A workspace at `tool` with two members, `app` and `lib`, and no
/// `Cargo.lock`.
const WORKSPACE: &[(&str, &str)] = &[
    ("README.md", "# x\n"),
    (
        "tool/Cargo.toml",
        "[workspace]\nmembers = [\"app\", \"lib\"]\n",
    ),
    ("tool/app/Cargo.toml", "[package]\nname = \"app\"\n"),
    ("tool/app/src/lib.rs", "//! app\n"),
    ("tool/lib/Cargo.toml", "[package]\nname = \"lib\"\n"),
    ("tool/lib/src/lib.rs", "//! lib\n"),
];

/// The declaration around the fixtures, with these entries added.
fn declaring(entries: &str) -> String {
    format!(
        "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.tool]\npath = \
         \"tool\"\npipeline = \"rust-workspace\"\nversioning = \"none\"\n{entries}"
    )
}

fn exclude(path: &str) -> String {
    format!("\n[[exclude]]\npath = \"{path}\"\nreason = \"code of others, kept as it came\"\n")
}

fn reader(files: &[(&str, &str)]) -> impl Fn(&RelativePath) -> Result<String, String> {
    let contents: BTreeMap<String, String> = files
        .iter()
        .map(|(path, content)| ((*path).to_owned(), (*content).to_owned()))
        .collect();
    move |path: &RelativePath| {
        contents
            .get(&path.to_string())
            .cloned()
            .ok_or_else(|| "no such file".to_owned())
    }
}

fn builtin() -> Register {
    Register::builtin().expect("the register this crate was built with")
}

/// The findings for these files under this declaration and register.
fn found_with(register: &Register, declaration: &str, files: &[(&str, &str)]) -> Vec<Finding> {
    let paths: Vec<&str> = files.iter().map(|(path, _)| *path).collect();
    let analysis = analysis(declaration, &paths);
    findings(&analysis, register, &reader(files))
}

/// The findings for [`WORKSPACE`] plus `extra`, under this declaration.
fn found(declaration: &str, extra: &[(&'static str, &'static str)]) -> Vec<Finding> {
    let mut files = WORKSPACE.to_vec();
    files.extend_from_slice(extra);
    found_with(&builtin(), declaration, &files)
}

fn path(text: &str) -> RelativePath {
    RelativePath::new(text).unwrap()
}

#[test]
fn a_workspace_without_its_lockfile_is_warned_about_once_at_its_root() {
    let found = found(&declaring(""), &[]);

    assert_eq!(found.len(), 1, "{found:?}");
    let finding = &found[0];
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.rule, Rule::MissingLockfile);
    assert_eq!(finding.rule.code(), "missing-lockfile");
    assert_eq!(finding.path, Some(path("tool/Cargo.toml")));
    assert_eq!(
        finding.message,
        "the lockfile of this Cargo.toml is tool/Cargo.lock, in the root of the Cargo \
         workspace, and Git does not track it; what is checked and built is then resolved \
         anew in each run. Commit tool/Cargo.lock, or take the manifest out with an \
         [[exclude]] entry and its reason"
    );
}

#[test]
fn a_tracked_lockfile_in_the_root_is_enough_for_every_member() {
    let found = found(&declaring(""), &[("tool/Cargo.lock", "version = 4\n")]);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_lockfile_beside_a_member_is_not_the_one_cargo_reads() {
    let found = found(&declaring(""), &[("tool/app/Cargo.lock", "version = 4\n")]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].message.contains("tool/Cargo.lock"), "{found:?}");
}

#[test]
fn a_package_on_its_own_needs_its_lockfile_beside_it() {
    let files = [
        ("README.md", "# x\n"),
        ("tool/Cargo.toml", "[package]\nname = \"tool\"\n"),
        ("tool/src/main.rs", "fn main() {}\n"),
    ];
    let found = found_with(&builtin(), &declaring(""), &files);

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].path, Some(path("tool/Cargo.toml")));
    assert!(
        found[0].message.contains("is tool/Cargo.lock,"),
        "{found:?}"
    );
}

#[test]
fn a_package_the_workspace_excludes_needs_a_lockfile_of_its_own() {
    let found = found(
        &declaring(""),
        &[
            ("tool/Cargo.lock", "version = 4\n"),
            (
                "tool/Cargo.toml",
                "[workspace]\nmembers = [\"app\", \"lib\"]\nexclude = [\"fuzz\"]\n",
            ),
            ("tool/fuzz/Cargo.toml", "[package]\nname = \"fuzz\"\n"),
            ("tool/fuzz/src/lib.rs", "//! fuzz\n"),
        ],
    );

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].path, Some(path("tool/fuzz/Cargo.toml")));
    assert!(
        found[0].message.contains("tool/fuzz/Cargo.lock"),
        "{found:?}"
    );
}

#[test]
fn the_finding_stands_at_the_manifest_nearest_the_root_whatever_the_path_order() {
    // `A` sorts before `Cargo.toml`, so the member comes first and the root's
    // manifest has to take its place.
    let files = [
        ("README.md", "# x\n"),
        ("tool/Cargo.toml", "[workspace]\nmembers = [\"A\"]\n"),
        ("tool/A/Cargo.toml", "[package]\nname = \"a\"\n"),
        ("tool/A/src/lib.rs", "//! a\n"),
    ];
    let found = found_with(&builtin(), &declaring(""), &files);

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].path, Some(path("tool/Cargo.toml")));
}

#[test]
fn a_manifest_that_does_not_read_ends_no_search() {
    // The root does not read as TOML, so it is no workspace: each member is
    // its own root, and needs its own lockfile.
    let found = found(&declaring(""), &[("tool/Cargo.toml", "[workspace\n")]);

    let at: Vec<_> = found.iter().filter_map(|f| f.path.clone()).collect();
    assert_eq!(
        at,
        [
            path("tool/Cargo.toml"),
            path("tool/app/Cargo.toml"),
            path("tool/lib/Cargo.toml"),
        ],
        "{found:?}"
    );
}

#[test]
fn an_excluded_workspace_is_passed_over() {
    let found = found(&declaring(&exclude("tool")), &[]);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_lockfile_that_stands_beside_its_manifest_is_looked_for_there() {
    let source =
        BUILTIN_REGISTER.replace("stands = \"cargo-workspace-root\"", "stands = \"manifest\"");
    assert_ne!(
        source, BUILTIN_REGISTER,
        "the register names where Cargo.lock stands"
    );
    let register = Register::parse(&source).expect("a register with the lockfile beside");

    let found = found_with(
        &register,
        &declaring(""),
        &[WORKSPACE, &[("tool/Cargo.lock", "version = 4\n")]].concat(),
    );

    let at: Vec<_> = found.iter().filter_map(|f| f.path.clone()).collect();
    assert_eq!(
        at,
        [path("tool/app/Cargo.toml"), path("tool/lib/Cargo.toml")],
        "{found:?}"
    );
    assert!(
        found[0]
            .message
            .contains("is tool/app/Cargo.lock, beside the manifest,"),
        "{found:?}"
    );
}

#[test]
fn an_ecosystem_without_a_lockfile_asks_for_none() {
    let files = [
        ("README.md", "# x\n"),
        ("requirements.txt", "requests==2.0.0\n"),
    ];
    let found = found_with(
        &builtin(),
        "[repo]\naudience = \"public\"\npipeline = \"none\"\n",
        &files,
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn phase_zero_warns_about_a_lockfile_git_does_not_track() {
    let mut files = WORKSPACE.to_vec();
    let declaration = declaring("");
    files.push((".nebensound/repo.toml", &declaration));
    files.push(("tool/Cargo.lock", "version = 4\n"));
    let dir = repository(&files);
    let root = dir.path().to_str().expect("a path in UTF-8");

    // On disk, and not tracked: that is what the warning is about.
    git(
        dir.path(),
        &["rm", "--cached", "--quiet", "tool/Cargo.lock"],
    );
    let (text, _) = call(&["nebensound-repo", "-C", root, "check"]);
    assert!(
        text.contains(
            "warning[missing-lockfile] at tool/Cargo.toml: the lockfile of this \
                       Cargo.toml is tool/Cargo.lock"
        ),
        "{text}"
    );

    git(dir.path(), &["add", "tool/Cargo.lock"]);
    let (text, _) = call(&["nebensound-repo", "-C", root, "check"]);
    assert!(!text.contains("missing-lockfile"), "{text}");
}
