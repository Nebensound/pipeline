//! The tool against real repositories on disk.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound_repo::output::Output;
use nebensound_repo::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER};
use support::{Broken, QUIET, call, directory, repository};

/// A repository with nothing wrong with it.
fn clean() -> tempfile::TempDir {
    repository(&[(".nebensound/repo.toml", QUIET), ("README.md", "# x\n")])
}

/// One with a Rust file nobody checks.
fn broken() -> tempfile::TempDir {
    repository(&[
        (".nebensound/repo.toml", QUIET),
        ("README.md", "# x\n"),
        ("src/main.rs", "fn main() {}\n"),
    ])
}

fn root(dir: &tempfile::TempDir) -> &str {
    dir.path().to_str().expect("a path in UTF-8")
}

#[test]
fn a_clean_repository_reports_nothing() {
    let dir = clean();
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir)]);

    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Resolved configuration"), "{text}");
    assert!(text.contains("Base checks"), "{text}");
    assert!(text.contains("Tree"), "{text}");
    assert!(text.contains("Findings\n\n  none"), "{text}");
    assert!(
        text.contains("2 files, 0 gaps, 0 errors, 0 warnings"),
        "{text}"
    );
}

#[test]
fn an_error_stops_the_run() {
    let dir = broken();
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "check"]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("uncovered-file"), "{text}");
    assert!(text.contains("1 gap"), "{text}");
}

#[test]
fn an_empty_marker_keeps_its_folder_without_an_exclusion() {
    let kept = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("README.md", "# x\n"),
        ("assets/.gitkeep", ""),
        ("logs/.keep", ""),
    ]);
    let (text, code) = call(&["nebensound-repo", "-C", root(&kept), "check"]);
    assert_eq!(code, 0, "{text}");

    // A marker that holds content is a file like any other, and no check
    // covers it.
    let filled = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("README.md", "# x\n"),
        ("assets/.gitkeep", "not empty\n"),
    ]);
    let (text, code) = call(&["nebensound-repo", "-C", root(&filled), "check"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("language-without-check"), "{text}");
    assert!(text.contains("assets/.gitkeep"), "{text}");
}

#[test]
fn without_a_declaration_it_says_which_file_is_missing() {
    let dir = repository(&[("README.md", "# x\n")]);
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir)]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains(".nebensound/repo.toml"), "{text}");
}

#[test]
fn without_git_there_is_no_tree() {
    let dir = directory(&[(".nebensound/repo.toml", QUIET), ("README.md", "# x\n")]);
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir)]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("git does not list"), "{text}");
}

#[test]
fn tree_prints_the_tree_and_nothing_else() {
    let dir = clean();
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "tree"]);

    assert_eq!(code, 0, "{text}");
    assert!(text.starts_with("Tree\n"), "{text}");
    assert!(!text.contains("Resolved configuration"), "{text}");
}

#[test]
fn explain_says_what_holds_for_a_path() {
    let dir = clean();
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "explain", "README.md"]);

    assert_eq!(code, 0, "{text}");
    assert!(text.starts_with("README.md\n"), "{text}");
    assert!(text.contains("Coverage"), "{text}");
    assert!(text.contains("markdown"), "{text}");
}

#[test]
fn explain_only_knows_what_is_in_the_tree() {
    let dir = clean();
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "explain", "nowhere.md"]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("is not in the tree"), "{text}");
}

#[test]
fn explain_takes_only_paths_inside_the_repository() {
    let dir = clean();
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "explain", "/etc/hosts"]);

    assert_eq!(
        code, 2,
        "a wrong call, not a problem with the repository: {text}"
    );
    assert!(text.contains("not a path inside the repository"), "{text}");

    // And it is answered without reading the repository at all.
    let nowhere = directory(&[]);
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root(&nowhere),
        "explain",
        "/etc/hosts",
    ]);
    assert_eq!(code, 2, "{text}");
    assert!(!text.contains("git"), "{text}");
}

#[cfg(feature = "ui")]
#[test]
fn the_interface_needs_a_terminal() {
    // Output::Elsewhere is passed in rather than asked for. Asking under
    // `cargo test` would be answered truthfully by the terminal the run was
    // started from, and this check would take it over.
    let dir = clean();
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "ui"]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("does not go to a terminal"), "{text}");
    assert!(text.contains("`check`"), "{text}");
}

#[cfg(not(feature = "ui"))]
#[test]
fn a_build_without_the_interface_does_not_know_the_call() {
    // What CI builds. `ui` is then a wrong call like any other unknown word,
    // refused before the repository is read: there is no half-present command
    // that would have to explain itself.
    let dir = clean();
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "ui"]);

    assert_eq!(code, 2, "{text}");
    assert!(text.contains("ui"), "{text}");
}

#[test]
fn where_the_output_goes_is_answered_from_the_process() {
    assert_eq!(Output::of(true), Output::Terminal);
    assert_eq!(Output::of(false), Output::Elsewhere);
    // Whatever this run has, asking must not fail.
    let _ = Output::current();
}

#[test]
fn help_and_version_are_not_a_failure() {
    let (help, code) = call(&["nebensound-repo", "--help"]);
    assert_eq!(code, 0, "{help}");
    assert!(help.contains("explain"), "{help}");

    let (version, code) = call(&["nebensound-repo", "--version"]);
    assert_eq!(code, 0, "{version}");
}

#[test]
fn an_unknown_command_is_a_misuse() {
    let (text, code) = call(&["nebensound-repo", "nonsense"]);

    assert_eq!(code, 2, "{text}");
}

#[test]
fn an_output_that_broke_away_stays_quiet() {
    let dir = clean();
    let code = nebensound_repo::run(["nebensound-repo", "-C", root(&dir)], &mut Broken);

    assert_eq!(code, 0, "the result does not depend on anyone listening");
}

#[test]
fn broken_built_in_data_says_so() {
    let dir = clean();
    let mut out = Vec::new();
    let code = nebensound_repo::run_against(
        ["nebensound-repo", "-C", root(&dir)],
        "this is not a register",
        "[repo]\naudience = \"public\"\n",
        Output::Elsewhere,
        &mut out,
    );
    let text = String::from_utf8(out).expect("the output is UTF-8");

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("built-in register"), "{text}");
}

#[test]
fn the_built_in_data_is_what_the_binary_uses() {
    // run() reaches for exactly these two; without this the constants could
    // drift apart from what run_against is handed everywhere else.
    assert!(BUILTIN_REGISTER.contains("[check.rust]"), "the register");
    assert!(BUILTIN_DEFAULTS.contains("[[exclude]]"), "the defaults");
}

// The binary is the program, so it is only built with the feature `program`.
#[cfg(feature = "program")]
#[test]
fn the_binary_says_the_same() {
    let dir = broken();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_nebensound-repo"))
        .arg("-C")
        .arg(dir.path())
        .output()
        .expect("the built binary");

    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("uncovered-file"), "{text}");
}

#[test]
fn files_names_what_a_building_block_checks() {
    let dir = clean();
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root(&dir),
        "files",
        "markdown",
        ".",
    ]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(text, "README.md\n");

    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root(&dir),
        "files",
        "-z",
        "markdown",
        ".",
    ]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(text, "README.md\0");
}

#[test]
fn files_refuses_what_the_plan_does_not_start() {
    let dir = clean();
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "files", "rust", "."]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("the plan does not start the rust check"),
        "{text}"
    );

    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root(&dir),
        "files",
        "markdown",
        "/x",
    ]);
    assert_eq!(code, 2, "{text}");
    assert!(
        text.contains("is not a path inside the repository"),
        "{text}"
    );
}

#[test]
fn a_repository_with_errors_gets_no_file_list() {
    let dir = broken();
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root(&dir),
        "files",
        "markdown",
        ".",
    ]);

    assert_eq!(code, 1, "{text}");
    assert_eq!(
        text,
        "there is no file list: phase 0 found errors, and `check` says which\n"
    );
}

#[test]
fn an_ignore_file_that_takes_out_more_than_the_declaration_fails_phase_0() {
    let declaration = format!(
        "{QUIET}\n[[exclude]]\npath = \".prettierignore\"\nreason = \"an ignore file, without a formatter\"\n"
    );
    let dir = repository(&[
        (".nebensound/repo.toml", &declaration),
        ("README.md", "# x\n"),
        (".prettierignore", "*\n"),
    ]);
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "check"]);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("error[unexcused-ignore] at README.md: .prettierignore takes this file out"),
        "{text}"
    );
}

#[test]
fn a_file_an_ignore_setting_names_is_read_from_the_repository() {
    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("README.md", "# x\n"),
        (".yamllint.yaml", "ignore-from-file: .yamlignore\n"),
    ]);
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "check"]);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("error[unreadable-ignore-file] at .yamllint.yaml"),
        "{text}"
    );
    assert!(text.contains(".yamlignore: "), "{text}");
}
