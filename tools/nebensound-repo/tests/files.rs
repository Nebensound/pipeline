//! The files one check covers in one unit, as a building block takes them.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound_repo::files::{Refusal, files, render};
use nebensound_repo::path::RelativePath;
use support::{analysis, central};

const DECLARATION: &str = r#"
[repo]
audience = "public"
pipeline = "none"

[component.tool]
path     = "tool"
pipeline = "rust-workspace"
versioning = "none"

[[exclude]]
path   = "tool/vendor/schema"
reason = "a vendored copy of someone else's crate, kept as it came"

[[exclude]]
path   = "docs/draft.md"
reason = "a draft that is not published"
"#;

const PATHS: &[&str] = &[
    ".nebensound/repo.toml",
    "README.md",
    "docs/draft.md",
    "docs/guide.md",
    "tool/Cargo.lock",
    "tool/Cargo.toml",
    "tool/README.md",
    "tool/src/main.rs",
    "tool/vendor/schema/Cargo.toml",
    "tool/vendor/schema/src/lib.rs",
];

fn path(text: &str) -> RelativePath {
    RelativePath::new(text).unwrap()
}

fn names(list: &[&RelativePath]) -> Vec<String> {
    list.iter().map(ToString::to_string).collect()
}

#[test]
fn a_check_covers_every_file_below_its_unit_less_what_is_excluded() {
    let analysis = analysis(DECLARATION, PATHS);
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    let central = central();

    let markdown = files(&analysis, &central, "markdown", &RelativePath::root()).unwrap();
    assert_eq!(
        names(&markdown),
        ["README.md", "docs/guide.md", "tool/README.md"]
    );

    let rust = files(&analysis, &central, "rust", &path("tool")).unwrap();
    assert_eq!(names(&rust), ["tool/src/main.rs"]);
}

#[test]
fn a_check_the_plan_does_not_start_there_is_refused() {
    let analysis = analysis(DECLARATION, PATHS);
    let central = central();

    let refusal = files(&analysis, &central, "rust", &RelativePath::root()).unwrap_err();
    assert_eq!(
        refusal,
        Refusal::NotStarted {
            check: "rust".to_owned(),
            unit: RelativePath::root(),
        }
    );
    assert_eq!(
        refusal.to_string(),
        "the plan does not start the rust check at .; `plan` says where it runs"
    );

    let refusal = files(&analysis, &central, "markdown", &path("tool")).unwrap_err();
    assert!(matches!(refusal, Refusal::NotStarted { .. }), "{refusal}");
}

#[test]
fn a_check_the_register_does_not_know_is_refused() {
    let analysis = analysis(DECLARATION, PATHS);
    let refusal = files(&analysis, &central(), "rsut\n", &path("tool")).unwrap_err();

    assert_eq!(refusal, Refusal::UnknownCheck("rsut\n".to_owned()));
    assert_eq!(
        refusal.to_string(),
        r#"the register has no check "rsut\\n""#
    );
}

#[test]
fn the_list_is_printed_one_path_per_line_or_ended_by_zero_bytes() {
    let plain = path("a.md");
    let odd = path("b\nc.md");
    let list = [&plain, &odd];

    assert_eq!(render(&list, false), "a.md\nb\\nc.md\n");
    assert_eq!(render(&list, true), "a.md\0b\nc.md\0");
    assert_eq!(render(&[], false), "");
}

#[test]
fn a_lockfile_goes_to_the_check_its_name_assigns() {
    let lockfiles = &[
        ".nebensound/repo.toml",
        "Cargo.lock",
        "Gemfile.lock",
        "app/uv.lock",
        "composer.lock",
        "flake.lock",
        "web/yarn.lock",
    ];
    let analysis = analysis(
        "[repo]\naudience = \"public\"\npipeline = \"none\"\n",
        lockfiles,
    );
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    let central = central();
    let root = RelativePath::root();

    let toml = files(&analysis, &central, "toml", &root).unwrap();
    assert_eq!(
        names(&toml),
        [".nebensound/repo.toml", "Cargo.lock", "app/uv.lock"]
    );
    let json = files(&analysis, &central, "json", &root).unwrap();
    assert_eq!(names(&json), ["composer.lock", "flake.lock"]);
}
