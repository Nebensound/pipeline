//! Rust packages and `[[exclude]]`: out as a whole, or not at all.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use nebensound_repo::finding::{Finding, Position, Rule};
use nebensound_repo::names::Reason;
use nebensound_repo::packages::{Excluded, excluded, findings, to_json};
use nebensound_repo::path::RelativePath;
use support::{analysis, call, repository};

/// A workspace at `tool` with two members, `app` and `vendored`, and a path
/// dependency outside it, `outside`, that is no member.
const WORKSPACE: &[(&str, &str)] = &[
    ("README.md", "# x\n"),
    (
        "tool/Cargo.toml",
        "[workspace]\nmembers = [\"app\", \"vendored\"]\n",
    ),
    ("tool/Cargo.lock", "version = 4\n"),
    (
        "tool/app/Cargo.toml",
        "[package]\nname = \"app\"\n\n[dependencies]\nvendored = { path = \"../vendored\" }\n\
         outside = { path = \"../../outside\" }\n",
    ),
    ("tool/app/src/lib.rs", "//! app\n"),
    ("tool/app/src/generated.rs", "//! generated\n"),
    (
        "tool/vendored/Cargo.toml",
        "[package]\nname = \"vendored\"\n",
    ),
    ("tool/vendored/src/lib.rs", "//! vendored\n"),
    ("tool/vendored/src/more.rs", "//! more\n"),
    ("outside/Cargo.toml", "[package]\nname = \"outside\"\n"),
    ("outside/src/lib.rs", "//! outside\n"),
];

/// The declaration around [`WORKSPACE`], with these entries added.
fn declaring(entries: &str) -> String {
    format!(
        "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.tool]\npath = \"tool\"\npipeline = \
         \"rust-workspace\"\nversioning = \"none\"\n{entries}"
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

fn paths<'a>(files: &[(&'a str, &str)]) -> Vec<&'a str> {
    files.iter().map(|(path, _)| *path).collect()
}

/// The findings for [`WORKSPACE`] plus `extra`, under this declaration.
fn found(declaration: &str, extra: &[(&'static str, &'static str)]) -> Vec<Finding> {
    let mut files = WORKSPACE.to_vec();
    files.extend_from_slice(extra);
    let analysis = analysis(declaration, &paths(&files));
    findings(&analysis, &declaration_path(), declaration, &reader(&files))
}

/// The packages taken out as a whole, under this declaration.
fn out(declaration: &str, extra: &[(&'static str, &'static str)]) -> Vec<Excluded> {
    let mut files = WORKSPACE.to_vec();
    files.extend_from_slice(extra);
    let analysis = analysis(declaration, &paths(&files));
    excluded(&analysis, &reader(&files))
}

fn declaration_path() -> RelativePath {
    RelativePath::new(".nebensound/repo.toml").unwrap()
}

fn path(text: &str) -> RelativePath {
    RelativePath::new(text).unwrap()
}

fn reason() -> Reason {
    Reason::new("code of others, kept as it came").unwrap()
}

#[test]
fn a_single_excluded_rust_file_is_an_error_at_its_entry() {
    let declaration = declaring(&exclude("tool/app/src/generated.rs"));
    let found = found(&declaration, &[]);

    assert_eq!(found.len(), 1, "{found:?}");
    let finding = &found[0];
    assert_eq!(finding.rule, Rule::PartialPackageExclusion);
    assert_eq!(finding.path, Some(declaration_path()));
    // The line of `path = …` under `[[exclude]]`, and the column of its value.
    assert_eq!(
        finding.position,
        Some(Position {
            line: 11,
            column: 8
        })
    );
    let message = &finding.message;
    for part in [
        "`tool/app/src/generated.rs`",
        "the Rust package at tool/app",
        "#[rustfmt::skip]",
        "#[coverage(off)]",
        "#[allow(...)]",
        "exclude its folder tool/app",
    ] {
        assert!(message.contains(part), "{part} is missing: {message}");
    }
    assert!(
        finding
            .to_string()
            .starts_with("error[partial-package-exclusion] at .nebensound/repo.toml:11:8: "),
        "{finding}"
    );
    assert!(out(&declaration, &[]).is_empty());
}

#[test]
fn a_folder_inside_a_package_is_not_the_package() {
    let found = found(&declaring(&exclude("tool/vendored/src")), &[]);

    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0]
            .message
            .contains("takes `tool/vendored/src/lib.rs`, `tool/vendored/src/more.rs` out"),
        "{}",
        found[0].message
    );
}

#[test]
fn an_excluded_workspace_member_is_out_as_a_whole() {
    let declaration = declaring(&exclude("tool/vendored"));

    assert!(found(&declaration, &[]).is_empty());
    assert_eq!(
        out(&declaration, &[]),
        [Excluded {
            folder: path("tool/vendored"),
            reason: reason(),
        }]
    );
}

#[test]
fn an_excluded_path_dependency_outside_the_workspace_is_out_as_a_whole() {
    let declaration = declaring(&exclude("outside"));

    assert!(found(&declaration, &[]).is_empty());
    assert_eq!(
        out(&declaration, &[]),
        [Excluded {
            folder: path("outside"),
            reason: reason(),
        }]
    );
}

#[test]
fn a_folder_around_several_packages_takes_each_out() {
    let declaration = declaring(&exclude("tool"));

    assert!(found(&declaration, &[]).is_empty());
    let folders: Vec<String> = out(&declaration, &[])
        .iter()
        .map(|package| package.folder.to_string())
        .collect();
    assert_eq!(folders, ["tool/app", "tool/vendored"]);
}

#[test]
fn a_narrower_entry_inside_an_excluded_package_is_redundant() {
    // The narrower entry wins for its own file, the folder's for the rest:
    // every file of the package is out, so the package is out as a whole,
    // with the reason of the entry that takes out its folder.
    for narrower in ["tool/vendored/src/more.rs", "tool/vendored/Cargo.toml"] {
        let declaration = format!(
            "{}\n[[exclude]]\npath = \"{narrower}\"\nreason = \"generated\"\n",
            declaring(&exclude("tool/vendored"))
        );

        assert!(found(&declaration, &[]).is_empty(), "{narrower}");
        assert_eq!(
            out(&declaration, &[]),
            [Excluded {
                folder: path("tool/vendored"),
                reason: reason(),
            }],
            "{narrower}"
        );
    }
}

#[test]
fn narrower_entries_alone_are_no_exclusion_of_the_package() {
    // Every file is out, but no entry takes out the folder, so a `build.rs`
    // Git tracks there tomorrow would be checked: the entries take out part
    // of the package, and the one that takes out code is reported.
    let declaration = format!(
        "{}{}{}",
        declaring(""),
        exclude("tool/vendored/Cargo.toml"),
        exclude("tool/vendored/src"),
    );
    let found = found(&declaration, &[]);

    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0]
            .message
            .contains("the [[exclude]] entry `tool/vendored/src` takes"),
        "{}",
        found[0].message
    );
    assert!(out(&declaration, &[]).is_empty());
}

#[test]
fn a_file_pulled_back_in_leaves_the_package_partly_excluded() {
    let declaration = format!(
        "{}\n[[include]]\npath = \"tool/vendored/src/more.rs\"\n",
        declaring(&exclude("tool/vendored"))
    );
    let found = found(&declaration, &[]);

    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0]
            .message
            .contains("takes `tool/vendored/src/lib.rs` out"),
        "{}",
        found[0].message
    );
    assert!(out(&declaration, &[]).is_empty());
}

#[test]
fn a_manifest_pulled_back_in_leaves_the_package_partly_excluded() {
    let declaration = format!(
        "{}\n[[include]]\npath = \"tool/vendored/Cargo.toml\"\n",
        declaring(&exclude("tool/vendored"))
    );

    assert_eq!(found(&declaration, &[]).len(), 1);
    assert!(out(&declaration, &[]).is_empty());
}

#[test]
fn a_package_inside_a_package_is_a_package_of_its_own() {
    let extra = [
        (
            "tool/app/nested/Cargo.toml",
            "[package]\nname = \"nested\"\n",
        ),
        ("tool/app/nested/src/lib.rs", "//! nested\n"),
    ];
    let declaration = declaring(&exclude("tool/app/nested"));

    assert!(found(&declaration, &extra).is_empty());
    assert_eq!(
        out(&declaration, &extra),
        [Excluded {
            folder: path("tool/app/nested"),
            reason: reason(),
        }]
    );
}

#[test]
fn a_package_inside_an_excluded_one_that_is_pulled_back_in_stays_in() {
    // `check-rust` leaves out exactly the folders this list names, so the
    // package inside, kept in by the declaration, must not be among them.
    let extra = [
        (
            "tool/vendored/inner/Cargo.toml",
            "[package]\nname = \"inner\"\n",
        ),
        ("tool/vendored/inner/src/lib.rs", "//! inner\n"),
    ];
    let declaration = format!(
        "{}\n[[include]]\npath = \"tool/vendored/inner\"\nreason = \"we maintain it ourselves\"\n",
        declaring(&exclude("tool/vendored"))
    );

    assert!(found(&declaration, &extra).is_empty());
    assert_eq!(
        out(&declaration, &extra),
        [Excluded {
            folder: path("tool/vendored"),
            reason: reason(),
        }]
    );
}

#[test]
fn a_rust_file_in_no_package_is_no_concern_here() {
    // `tool/Cargo.toml` describes a workspace and no package, so a file beside
    // it belongs to none: Cargo compiles nothing of it.
    let extra = [("tool/stray.rs", "//! stray\n")];
    let declaration = declaring(&exclude("tool/stray.rs"));

    assert!(found(&declaration, &extra).is_empty());
    assert!(out(&declaration, &extra).is_empty());
}

#[test]
fn a_manifest_that_does_not_read_counts_as_a_package() {
    let files: Vec<(&str, &str)> = WORKSPACE
        .iter()
        .copied()
        .filter(|(path, _)| *path != "tool/Cargo.toml")
        .chain([
            ("tool/Cargo.toml", "[workspace"),
            ("tool/stray.rs", "//! x\n"),
        ])
        .collect();
    let declaration = declaring(&exclude("tool/stray.rs"));
    let analysis = analysis(&declaration, &paths(&files));
    let found = findings(
        &analysis,
        &declaration_path(),
        &declaration,
        &reader(&files),
    );

    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].message.contains("the Rust package at tool,"));

    // Nor does one that is not there.
    let missing = findings(&analysis, &declaration_path(), &declaration, &|_| {
        Err("gone".to_owned())
    });
    assert_eq!(missing.len(), 1, "{missing:?}");
}

#[test]
fn a_declaration_source_that_does_not_read_names_no_place() {
    let declaration = declaring(&exclude("tool/app/src/generated.rs"));
    let analysis = analysis(&declaration, &paths(WORKSPACE));
    let found = findings(&analysis, &declaration_path(), "[repo", &reader(WORKSPACE));

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].position, None);
    assert!(
        found[0]
            .to_string()
            .starts_with("error[partial-package-exclusion] at .nebensound/repo.toml: "),
        "{}",
        found[0]
    );
}

#[test]
fn the_column_counts_characters() {
    // A path with a character of two bytes before the value, and a value on
    // the first line.
    let declaration = "exclude = [{ reason = \"ä\", path = \"tool/app/src/generated.rs\" }]\n\
                       [repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.tool]\npath = \"tool\"\n\
                       pipeline = \"rust-workspace\"\nversioning = \"none\"\n";
    let found = found(declaration, &[]);

    assert_eq!(
        found[0].position,
        Some(Position {
            line: 1,
            column: 35
        })
    );
}

#[test]
fn the_list_is_one_line_of_json() {
    let list = [
        Excluded {
            folder: path("tool/vendored"),
            reason: reason(),
        },
        Excluded {
            folder: path("outside"),
            reason: Reason::new("a \"quoted\"\nreason").unwrap(),
        },
    ];

    assert_eq!(
        to_json(&list),
        r#"[{"path":"tool/vendored","reason":"code of others, kept as it came"},{"path":"outside","reason":"a \"quoted\"\nreason"}]"#
    );
    assert_eq!(to_json(&[]), "[]");
}

#[test]
fn the_tool_prints_the_excluded_packages() {
    let mut files: Vec<(&str, &str)> = WORKSPACE.to_vec();
    let declaration = format!(
        "{}{}{}",
        declaring(""),
        exclude("tool/vendored"),
        exclude("outside")
    );
    files.push((".nebensound/repo.toml", &declaration));
    let dir = repository(&files);
    let root = dir.path().to_str().unwrap();

    let (text, code) = call(&["nebensound-repo", "-C", root, "excluded-packages"]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        text,
        "[{\"path\":\"outside\",\"reason\":\"code of others, kept as it came\"},\
         {\"path\":\"tool/vendored\",\"reason\":\"code of others, kept as it came\"}]\n"
    );
}

#[test]
fn the_tool_prints_no_list_and_names_the_place_when_phase_0_fails() {
    let mut files: Vec<(&str, &str)> = WORKSPACE.to_vec();
    let declaration = declaring(&exclude("tool/app/src/generated.rs"));
    files.push((".nebensound/repo.toml", &declaration));
    let dir = repository(&files);
    let root = dir.path().to_str().unwrap();

    let (text, code) = call(&["nebensound-repo", "-C", root, "excluded-packages"]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(
        text,
        "there is no list of excluded packages: phase 0 found errors, and `check` says which\n"
    );

    let (text, code) = call(&["nebensound-repo", "-C", root, "check"]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("error[partial-package-exclusion] at .nebensound/repo.toml:11:8: "),
        "{text}"
    );
}
