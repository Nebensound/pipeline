//! The tools' own ways of leaving a file out, held against the declaration.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use nebensound_repo::finding::{Finding, Rule};
use nebensound_repo::ignored::{Tool, findings};
use nebensound_repo::path::RelativePath;
use support::{QUIET, analysis, call, central, directory, git, repository};

/// The findings for a repository holding these files, read from memory.
fn found(declaration: &str, files: &[(&str, &str)]) -> Vec<Finding> {
    let paths: Vec<&str> = files.iter().map(|(path, _)| *path).collect();
    let contents: BTreeMap<String, String> = files
        .iter()
        .map(|(path, content)| ((*path).to_owned(), (*content).to_owned()))
        .collect();
    let analysis = analysis(declaration, &paths);
    let read = |path: &RelativePath| {
        contents
            .get(&path.to_string())
            .cloned()
            .ok_or_else(|| "no such file".to_owned())
    };
    findings(&analysis, central().register(), &read)
}

/// Each finding as `<rule> <path>`.
fn summary(found: &[Finding]) -> Vec<String> {
    found
        .iter()
        .map(|finding| {
            let path = finding
                .path
                .as_ref()
                .map_or_else(String::new, ToString::to_string);
            format!("{} {path}", finding.rule)
        })
        .collect()
}

/// The files a repository holds besides its declaration and its settings.
const FILES: &[(&str, &str)] = &[
    (".nebensound/repo.toml", QUIET),
    ("LICENSE", "text"),
    ("README.md", "# x\n"),
    ("docs/a.md", "# a\n"),
    ("data.json", "{}\n"),
    ("sub/x.yml", "a: 1\n"),
    ("Cargo.toml", "[package]\n"),
    ("Cargo.lock", "version = 4\n"),
];

fn with(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut files = FILES.to_vec();
    files.extend_from_slice(extra);
    files
}

/// The declaration plus an exclusion for the settings file, so that the only
/// findings are the ones about what it takes out.
fn excusing(file: &str) -> String {
    format!("{QUIET}\n[[exclude]]\npath = \"{file}\"\nreason = \"a setting without a check\"\n")
}

#[test]
fn a_prettierignore_holding_a_star_fails_every_file_prettier_reads() {
    let found = found(
        &excusing(".prettierignore"),
        &with(&[(".prettierignore", "*\n")]),
    );

    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore README.md",
            "unexcused-ignore data.json",
            "unexcused-ignore docs/a.md",
            "unexcused-ignore sub/x.yml",
        ]
    );
    let first = &found[0];
    assert_eq!(first.rule, Rule::UnexcusedIgnore);
    assert!(
        first
            .message
            .contains(".prettierignore takes this file out of Prettier"),
        "{}",
        first.message
    );
    assert!(first.message.contains("the line `*`"), "{}", first.message);
}

#[test]
fn what_an_exclusion_takes_out_may_be_ignored_as_well() {
    let declaration = format!(
        "{}\n[[exclude]]\npath = \"docs\"\nreason = \"drafts\"\n",
        excusing(".prettierignore")
    );
    let found = found(&declaration, &with(&[(".prettierignore", "docs/\n")]));

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_line_that_pulls_a_file_back_in_is_followed() {
    let found = found(
        &excusing(".prettierignore"),
        &with(&[(".prettierignore", "*.md\n!README.md\n")]),
    );

    assert_eq!(summary(&found), ["unexcused-ignore docs/a.md"]);
}

#[test]
fn a_line_that_does_not_parse_is_reported_at_its_file() {
    let found = found(
        &excusing(".prettierignore"),
        &with(&[(".prettierignore", "a[z-a]\n")]),
    );

    assert_eq!(summary(&found), ["unreadable-ignore-file .prettierignore"]);
    assert!(
        found[0].message.contains("Prettier"),
        "{}",
        found[0].message
    );
}

#[test]
fn a_settings_file_that_cannot_be_read_is_reported() {
    let analysis = analysis(QUIET, &["README.md", "lychee.toml"]);
    let read = |_: &RelativePath| Err("it is a link".to_owned());
    let found = findings(&analysis, central().register(), &read);

    assert_eq!(summary(&found), ["unreadable-ignore-file lychee.toml"]);
    assert!(
        found[0].message.contains("it is a link"),
        "{}",
        found[0].message
    );
}

#[test]
fn taplo_takes_out_what_its_exclude_names() {
    let found = found(
        QUIET,
        &with(&[("taplo.toml", "exclude = [\"**/Cargo.toml\"]\n")]),
    );

    assert_eq!(summary(&found), ["unexcused-ignore Cargo.toml"]);
    assert!(found[0].message.contains("Taplo"), "{}", found[0].message);
    assert!(
        found[0].message.contains("`**/Cargo.toml`"),
        "{}",
        found[0].message
    );
}

#[test]
fn taplo_takes_out_what_its_include_does_not_name() {
    let found = found(
        QUIET,
        &with(&[(
            ".taplo.toml",
            "include = [\"taplo.toml\", \".taplo.toml\"]\n",
        )]),
    );

    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore .nebensound/repo.toml",
            "unexcused-ignore Cargo.toml",
        ]
    );
    assert!(
        found[0].message.contains("none of the `include` patterns"),
        "{}",
        found[0].message
    );
}

#[test]
fn taplo_include_leaves_out_no_file_taplo_does_not_read_by_default() {
    let found = found(
        QUIET,
        &with(&[(".taplo.toml", "include = [\"**/*.toml\"]\n")]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_taplo_exclude_naming_a_pipfile_takes_it_out() {
    // A Pipfile is no lockfile: check-toml formats and lints it like any
    // `.toml` file, so leaving it out of Taplo needs a reason.
    let found = found(
        QUIET,
        &with(&[
            ("Pipfile", "[packages]\n"),
            ("taplo.toml", "exclude = [\"Pipfile\"]\n"),
        ]),
    );

    assert_eq!(summary(&found), ["unexcused-ignore Pipfile"]);
}

#[test]
fn taplo_include_leaves_out_no_pipfile() {
    // Taplo reads no Pipfile by default, and check-toml hands it over through
    // standard input, where `include` does not apply.
    let found = found(
        QUIET,
        &with(&[
            ("Pipfile", "[packages]\n"),
            (".taplo.toml", "include = [\"**/*.toml\"]\n"),
        ]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_prettierignore_naming_only_a_json_lockfile_takes_nothing_out() {
    let found = found(
        &excusing(".prettierignore"),
        &with(&[
            ("composer.lock", "{}\n"),
            (".prettierignore", "composer.lock\n"),
        ]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_taplo_exclude_naming_only_a_toml_lockfile_takes_nothing_out() {
    let found = found(
        QUIET,
        &with(&[("taplo.toml", "exclude = [\"Cargo.lock\"]\n")]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_prettierignore_naming_only_a_python_json_lockfile_takes_nothing_out() {
    let found = found(
        &excusing(".prettierignore"),
        &with(&[
            ("Pipfile.lock", "{}\n"),
            (".prettierignore", "Pipfile.lock\n"),
        ]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_pattern_that_matches_only_lockfiles_takes_nothing_out() {
    let found = found(
        QUIET,
        &with(&[
            ("api/uv.lock", "version = 1\n"),
            ("api/poetry.lock", "[metadata]\n"),
            ("web/pdm.lock", "[metadata]\n"),
            ("taplo.toml", "exclude = [\"**/*.lock\"]\n"),
        ]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_prettierignore_naming_a_lockfile_and_another_file_counts_at_the_other_file() {
    let found = found(
        &excusing(".prettierignore"),
        &with(&[
            ("composer.lock", "{}\n"),
            (".prettierignore", "composer.lock\ndata.json\n"),
        ]),
    );

    assert_eq!(summary(&found), ["unexcused-ignore data.json"]);
}

#[test]
fn a_taplo_exclude_naming_a_lockfile_and_another_file_counts_at_the_other_file() {
    let found = found(
        QUIET,
        &with(&[("taplo.toml", "exclude = [\"Cargo.lock\", \"Cargo.toml\"]\n")]),
    );

    assert_eq!(summary(&found), ["unexcused-ignore Cargo.toml"]);
}

#[test]
fn taplo_reads_the_hidden_file_before_the_plain_one() {
    let found = found(
        QUIET,
        &with(&[
            (".taplo.toml", "[formatting]\n"),
            ("taplo.toml", "exclude = [\"**\"]\n"),
        ]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_taplo_file_that_does_not_read_is_reported() {
    for source in ["exclude = ", "exclude = [\"a[\"]", "include = [\"a[\"]"] {
        let found = found(QUIET, &with(&[("taplo.toml", source)]));
        assert_eq!(
            summary(&found),
            ["unreadable-ignore-file taplo.toml"],
            "{source}"
        );
    }
}

#[test]
fn markdownlint_takes_out_what_its_ignores_name_below_its_folder() {
    let found = found(
        &excusing(".markdownlint-cli2.jsonc"),
        &with(&[
            (
                ".markdownlint-cli2.jsonc",
                "{\n  // here\n  \"ignores\": [\"*.md\",],\n}\n",
            ),
            ("docs/.markdownlint-cli2.yaml", "ignores:\n  - a.md\n"),
        ]),
    );

    // `*.md` stops at a slash, so it takes out README.md and not docs/a.md;
    // the folder's own file takes out docs/a.md.
    assert_eq!(
        summary(&found),
        ["unexcused-ignore README.md", "unexcused-ignore docs/a.md"]
    );
    assert!(
        found[0].message.contains("markdownlint-cli2"),
        "{}",
        found[0].message
    );
}

#[test]
fn markdownlint_reads_the_jsonc_file_before_the_yaml_one() {
    let found = found(
        &excusing(".markdownlint-cli2.jsonc"),
        &with(&[
            (".markdownlint-cli2.jsonc", "{}"),
            (".markdownlint-cli2.yaml", "ignores: [\"**\"]\n"),
        ]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn markdownlint_options_without_ignores_take_nothing_out() {
    let found = found(
        QUIET,
        &with(&[
            ("a/.markdownlint-cli2.yaml", "config:\n  MD013: false\n"),
            ("b/.markdownlint-cli2.yaml", ""),
        ]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn markdownlint_options_that_do_not_read_are_reported() {
    for (file, source) in [
        (".markdownlint-cli2.jsonc", "{ \"ignores\": [ }"),
        (".markdownlint-cli2.jsonc", "{ \"ignores\": [\"a[\"] }"),
        (".markdownlint-cli2.yaml", "ignores: [\n"),
        (".markdownlint-cli2.yaml", "ignores: 3\n"),
        (".markdownlint-cli2.yaml", "ignores: [3]\n"),
    ] {
        let found = found(QUIET, &with(&[(file, source)]));
        let expected = format!("unreadable-ignore-file {file}");
        assert!(
            summary(&found).contains(&expected),
            "{source}: {:?}",
            summary(&found)
        );
    }
}

#[test]
fn lychee_takes_out_what_its_exclude_path_matches() {
    let found = found(
        QUIET,
        &with(&[("lychee.toml", "exclude_path = [\"^docs/\"]\n")]),
    );

    assert_eq!(summary(&found), ["unexcused-ignore docs/a.md"]);
    assert!(found[0].message.contains("lychee"), "{}", found[0].message);
}

#[test]
fn lychee_without_exclude_path_takes_nothing_out() {
    let found = found(QUIET, &with(&[("lychee.toml", "max_retries = 2\n")]));

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_lychee_configuration_that_does_not_read_is_reported() {
    for source in ["exclude_path = ", "exclude_path = [\"(\"]"] {
        let found = found(QUIET, &with(&[("lychee.toml", source)]));
        assert_eq!(
            summary(&found),
            ["unreadable-ignore-file lychee.toml"],
            "{source}"
        );
    }
}

#[test]
fn lychee_matches_the_path_from_the_root_as_the_block_hands_it_over() {
    // The block hands lychee `docs/a.md`, and lychee matches `exclude_path`
    // against that string: a pattern that needs a leading `/`, a `./` or the
    // checkout's own folder takes nothing out.
    let kept = found(
        QUIET,
        &with(&[(
            "lychee.toml",
            "exclude_path = [\"/docs/\", \"^\\\\./\", \"^/\", \"^/home/\"]\n",
        )]),
    );
    assert!(kept.is_empty(), "{kept:?}");

    let taken = found(
        QUIET,
        &with(&[("lychee.toml", "exclude_path = [\"(^|/)a\\\\.md$\"]\n")]),
    );
    assert_eq!(summary(&taken), ["unexcused-ignore docs/a.md"]);
}

#[cfg(unix)]
#[test]
fn a_setting_is_not_read_through_a_linked_folder() {
    // Ruff's `extend` names a path the pull request chooses. Through a
    // linked folder it would reach a file outside the checkout, and its text
    // would end up in the log.
    let outside = directory(&[("file.toml", "TOKEN=abc{def\n")]);
    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("ruff.toml", "extend = \"lnk/file.toml\"\n"),
        ("a.yml", "a: 1\n"),
    ]);
    std::os::unix::fs::symlink(outside.path(), dir.path().join("lnk")).expect("a link");
    git(dir.path(), &["add", "--all"]);
    let root = dir.path().to_str().unwrap();

    let (text, code) = call(&["nebensound-repo", "-C", root, "check"]);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("error[unreadable-ignore-file] at ruff.toml: "),
        "{text}"
    );
    assert!(text.contains("behind a link, or is one"), "{text}");
    assert!(!text.contains("TOKEN"), "{text}");
}

#[test]
fn settings_below_the_root_are_not_read_except_markdownlint_and_rustfmt() {
    let found = found(
        QUIET,
        &with(&[
            ("sub/.prettierignore", "*\n"),
            ("sub/taplo.toml", "exclude = [\"**\"]\n"),
            ("sub/lychee.toml", "exclude_path = [\".\"]\n"),
        ]),
    );

    assert!(found.is_empty(), "{found:?}");
}

/// A repository with Rust code: a package at the root and one below it.
fn rust(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut files = with(&[
        ("src/lib.rs", "pub mod gen;\n"),
        ("src/gen/mod.rs", "pub fn f() {}\n"),
        ("crates/a/Cargo.toml", "[package]\n"),
        ("crates/a/src/lib.rs", "pub fn g() {}\n"),
    ]);
    files.extend_from_slice(extra);
    files
}

#[test]
fn rustfmt_takes_out_what_its_ignore_names_below_its_folder() {
    let found = found(
        QUIET,
        &rust(&[
            (".rustfmt.toml", "edition = \"2024\"\nignore = [\"gen\"]\n"),
            ("crates/a/rustfmt.toml", "ignore = [\"/src/lib.rs\"]\n"),
        ]),
    );

    // `gen` matches the folder anywhere below the root; `/src/lib.rs` is
    // anchored at the folder of its own file, so it takes out crates/a's
    // src/lib.rs and not the one at the root.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore crates/a/src/lib.rs",
            "unexcused-ignore src/gen/mod.rs",
        ]
    );
    let message = &found[1].message;
    assert!(
        message.contains(".rustfmt.toml takes this file out of rustfmt through the line `gen`"),
        "{message}"
    );
    assert!(message.contains("#[rustfmt::skip]"), "{message}");
    assert!(message.contains("its whole package"), "{message}");
}

#[test]
fn rustfmt_ignore_counts_only_against_rust_files() {
    let found = found(QUIET, &rust(&[("rustfmt.toml", "ignore = [\"*\"]\n")]));

    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore crates/a/src/lib.rs",
            "unexcused-ignore src/gen/mod.rs",
            "unexcused-ignore src/lib.rs",
        ]
    );
}

#[test]
fn rustfmt_may_ignore_a_package_that_is_excluded_as_a_whole() {
    let declaration =
        format!("{QUIET}\n[[exclude]]\npath = \"crates/a\"\nreason = \"code of others\"\n");
    let found = found(
        &declaration,
        &rust(&[("crates/a/rustfmt.toml", "ignore = [\"*\"]\n")]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn rustfmt_reads_the_hidden_file_before_the_plain_one() {
    let found = found(
        QUIET,
        &rust(&[
            (".rustfmt.toml", "edition = \"2024\"\n"),
            ("rustfmt.toml", "ignore = [\"*\"]\n"),
        ]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_rustfmt_configuration_that_does_not_read_is_reported() {
    for source in [
        "ignore = [\n",
        "ignore = \"src\"\n",
        "ignore = [\"a[z-a]\"]\n",
    ] {
        let found = found(QUIET, &rust(&[("crates/a/rustfmt.toml", source)]));

        assert_eq!(
            summary(&found),
            ["unreadable-ignore-file crates/a/rustfmt.toml"],
            "{source}"
        );
        assert!(found[0].message.contains("rustfmt"), "{}", found[0].message);
    }
}

#[test]
fn every_tool_names_the_checks_it_reads() {
    assert_eq!(
        Tool::Prettier.checks(),
        ["css", "html", "json", "markdown", "yaml"]
    );
    assert_eq!(Tool::Taplo.checks(), ["toml"]);
    assert_eq!(Tool::Markdownlint.checks(), ["markdown"]);
    assert_eq!(Tool::Lychee.checks(), ["html", "markdown"]);
    assert_eq!(Tool::Lychee.name(), "lychee");
    assert_eq!(Tool::Rustfmt.checks(), ["rust"]);
    assert_eq!(Tool::Rustfmt.name(), "rustfmt");
    assert_eq!(Tool::Stylelint.checks(), ["css"]);
    assert_eq!(Tool::Stylelint.name(), "Stylelint");
    assert_eq!(Tool::HtmlValidate.checks(), ["html"]);
    assert_eq!(Tool::HtmlValidate.name(), "html-validate");
    for (tool, name) in [
        (Tool::Pyright, "Pyright"),
        (Tool::Coverage, "coverage.py"),
        (Tool::Ruff, "Ruff"),
    ] {
        assert_eq!(tool.checks(), ["python"]);
        assert_eq!(tool.name(), name);
    }
}

/// A repository with Python code: a project at the root and one in `svc/`.
fn python(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut files = with(&[
        ("src/app.py", "x = 1\n"),
        ("src/old.py", "x = 1\n"),
        ("src/gen/a.py", "x = 1\n"),
        ("src/gen/sub/b.py", "x = 1\n"),
        ("src/types.pyi", "x: int\n"),
        ("tests/test_app.py", "x = 1\n"),
        ("tests/stub.pyi", "x: int\n"),
        ("svc/src/x.py", "x = 1\n"),
        ("svc/gen/c.py", "x = 1\n"),
        ("deep/down/legacy.py", "x = 1\n"),
    ]);
    files.extend_from_slice(extra);
    files
}

#[test]
fn pyright_ignore_takes_out_what_it_names_below_its_folder() {
    let found = found(
        QUIET,
        &python(&[(
            "svc/pyrightconfig.json",
            "{\n  // generated code\n  \"ignore\": [\"./gen/\", \"src/x.py\",],\n}\n",
        )]),
    );

    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore svc/gen/c.py",
            "unexcused-ignore svc/src/x.py",
        ]
    );
    let message = &found[0].message;
    assert!(
        message.contains(
            "svc/pyrightconfig.json takes this file out of Pyright through the pattern `./gen/`"
        ),
        "{message}"
    );
    assert!(
        message.contains("[[exclude]] entry with a reason"),
        "{message}"
    );
}

#[test]
fn pyright_ignore_in_pyproject_takes_out_files_and_stubs() {
    let found = found(
        QUIET,
        &python(&[(
            "pyproject.toml",
            "[project]\nname = \"a\"\n\n[tool.pyright]\nignore = [\"**/*.pyi\", \"src/gen\"]\n",
        )]),
    );

    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore src/gen/a.py",
            "unexcused-ignore src/gen/sub/b.py",
            "unexcused-ignore src/types.pyi",
            "unexcused-ignore tests/stub.pyi",
        ]
    );
}

#[test]
fn pyright_settings_without_ignore_or_exclude_take_nothing_out() {
    for (file, source) in [
        ("pyrightconfig.json", "null\n"),
        ("pyrightconfig.json", "{\"typeCheckingMode\": \"strict\"}\n"),
        ("pyrightconfig.json", "{\"ignore\": []}\n"),
        ("pyrightconfig.json", "{\"exclude\": []}\n"),
        ("pyproject.toml", "[project]\nname = \"a\"\n"),
        ("pyproject.toml", "[tool.ruff]\nline-length = 100\n"),
        ("pyproject.toml", "[tool.pyright]\nstrict = [\"src\"]\n"),
    ] {
        let found = found(QUIET, &python(&[(file, source)]));

        assert!(found.is_empty(), "{source}: {found:?}");
    }
}

#[test]
fn a_pyright_configuration_that_does_not_read_is_reported() {
    for (file, source) in [
        ("pyrightconfig.json", "{\"ignore\": \"src\"}\n"),
        ("pyrightconfig.json", "{\"ignore\": [\"a[z-a]\"]}\n"),
        ("pyrightconfig.json", "{\"exclude\": [\"a[z-a]\"]}\n"),
        ("pyrightconfig.json", "{\"include\": [\"a[z-a]\"]}\n"),
        ("pyproject.toml", "[tool.pyright]\nignore = [\n"),
    ] {
        let found = found(QUIET, &python(&[(file, source)]));

        assert!(
            summary(&found).contains(&format!("unreadable-ignore-file {file}")),
            "{source}: {found:?}"
        );
        assert!(
            found
                .iter()
                .any(|finding| finding.message.contains("Pyright")),
            "{found:?}"
        );
    }
}

#[test]
fn coverage_omit_takes_out_what_coverage_would_not_measure() {
    let found = found(
        &excusing(".coveragerc"),
        &python(&[(
            ".coveragerc",
            "# measured\n[run]\nbranch = True\nomit =\n    */tests/*\n\n    legacy.py\n\
             ; a comment\n[report]\nomit: src/old.py, ./src/gen/*\nskip_empty = true\n\
             exclude_also =\n    if TYPE_CHECKING:\n",
        )]),
    );

    // A stub is never measured, so taking one out takes nothing out.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore deep/down/legacy.py",
            "unexcused-ignore src/gen/a.py",
            "unexcused-ignore src/gen/sub/b.py",
            "unexcused-ignore src/old.py",
            "unexcused-ignore tests/test_app.py",
        ]
    );
    let message = &found[0].message;
    assert!(
        message.contains(
            ".coveragerc takes this file out of coverage.py through the pattern `legacy.py`"
        ),
        "{message}"
    );
}

#[test]
fn coverage_reads_coveragerc_with_and_without_the_prefix() {
    let found = found(
        &excusing(".coveragerc"),
        &python(&[
            (
                ".coveragerc",
                "[coverage:run]\nomit = src/app.py\n[run]\nomit = src/old.py\nbranch = True\n\
                 [report]\nomit = src/gen/*\n[coverage:report]\nskip_empty = true\n",
            ),
            ("svc/.coveragerc", "[coverage:report]\nomit =\n    gen/*\n"),
            ("deep/.coveragerc", "[run]\nomit = down/legacy.py\n"),
        ]),
    );

    // coverage.py picks the section before the option: the first of
    // `[coverage:run]` and `[run]` that exists, and `omit` from that one
    // alone, so a `[coverage:report]` without it hides `[report]`.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore deep/down/legacy.py",
            "unexcused-ignore src/app.py",
            "unexcused-ignore svc/gen/c.py",
        ]
    );
}

#[test]
fn coverage_hands_an_omit_under_default_on_to_every_section() {
    let found = found(
        &excusing(".coveragerc"),
        &python(&[
            (
                ".coveragerc",
                "[DEFAULT]\nomit = src/app.py\n[coverage:run]\nbranch = True\n\
                 [run]\nomit = src/old.py\n",
            ),
            (
                "svc/setup.cfg",
                "[DEFAULT]\nomit = gen/*\n[coverage:report]\nskip_empty = true\n",
            ),
            (
                "deep/tox.ini",
                "[DEFAULT]\nomit = down/legacy.py\n[run]\nbranch = True\n",
            ),
        ]),
    );

    // configparser hands `[DEFAULT]` on to every section that exists, so
    // `[coverage:run]` sets `omit` and wins over `[run]`; in `tox.ini` no
    // section coverage.py reads exists, so the default takes nothing out.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore src/app.py",
            "unexcused-ignore svc/gen/c.py"
        ]
    );
}

#[test]
fn coverage_reads_its_sections_in_setup_cfg_tox_ini_and_pyproject() {
    let found = found(
        &format!(
            "{}\n[[exclude]]\npath = \"setup.cfg\"\nreason = \"x\"\n",
            excusing("svc/tox.ini")
        ),
        &python(&[
            (
                "setup.cfg",
                "[run]\nomit = src/app.py\n[coverage:run]\nomit = src\\old.py\n",
            ),
            ("svc/tox.ini", "[coverage:report]\nomit = gen/*\n"),
            (
                "svc/pyproject.toml",
                "[tool.coverage.run]\nomit = [\"src/x.py\"]\n[tool.coverage.report]\nshow_missing = true\n",
            ),
        ]),
    );

    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore src/old.py",
            "unexcused-ignore svc/gen/c.py",
            "unexcused-ignore svc/src/x.py",
        ]
    );
}

#[test]
fn coverage_reads_coveragerc_toml_with_and_without_the_prefix() {
    let found = found(
        &excusing(".coveragerc.toml"),
        &python(&[
            (
                ".coveragerc.toml",
                "[tool.coverage.run]\nomit = [\"src/app.py\"]\n\n[run]\nomit = [\"src/old.py\"]\n\n\
                 [report]\nomit = [\"src/gen/*\"]\n",
            ),
            ("svc/.coveragerc.toml", "[run]\nomit = [\"gen/*\"]\n"),
            (
                "deep/pyproject.toml",
                "[run]\nomit = [\"down/legacy.py\"]\n[report]\nomit = \"down/legacy.py\"\n",
            ),
        ]),
    );

    // coverage.py looks up `[run]` only where `[tool.coverage.run]` is
    // missing, and only in its own file: a `[run]` in `pyproject.toml` is not
    // its. The file needs no exclusion of its own to be read.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore src/app.py",
            "unexcused-ignore src/gen/a.py",
            "unexcused-ignore src/gen/sub/b.py",
            "unexcused-ignore svc/gen/c.py",
        ]
    );
    let message = &found[0].message;
    assert!(
        message.contains(
            ".coveragerc.toml takes this file out of coverage.py through the pattern `src/app.py`"
        ),
        "{message}"
    );
}

#[test]
fn coverage_patterns_follow_its_own_rules() {
    let found = found(
        &excusing(".coveragerc"),
        &python(&[(
            ".coveragerc",
            "[run]\nomit =\n    **/gen/a.py\n    ?eep/*\n    *eep/*\n    *app.py\n    \
             */svc/gen/*\n    /abs/deep/down/legacy.py\n",
        )]),
    );

    // `**/` and a leading `*/` stand for any folders above, and `*app.py`
    // names a file anywhere. coverage.py matches `?eep/*` and `*eep/*`
    // against an absolute path, which starts with a `/`, so they name no
    // file, and neither does an absolute pattern.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore src/app.py",
            "unexcused-ignore src/gen/a.py",
            "unexcused-ignore svc/gen/c.py",
            "unexcused-ignore tests/test_app.py",
        ]
    );
}

#[test]
fn coverage_settings_without_omit_take_nothing_out() {
    for (file, source) in [
        (".coveragerc", "[run]\nbranch = True\n"),
        (
            ".coveragerc",
            "[coverage:run]\nbranch = True\n[run]\nomit = src/old.py\n",
        ),
        (
            ".coveragerc",
            "[DEFAULT]\nomit = src/old.py\n[ run ]\nbranch = True\n[ coverage:report ]\nomit = src/app.py\n",
        ),
        ("setup.cfg", "[metadata]\nname = a\n    continued\n"),
        ("pyproject.toml", "[project]\nname = \"a\"\n"),
        ("pyproject.toml", "[tool.coverage]\n"),
        ("pyproject.toml", "[tool.coverage.run]\nbranch = true\n"),
        (".coveragerc.toml", "[run]\nbranch = true\n"),
        (
            ".coveragerc.toml",
            "[tool.coverage.run]\nbranch = true\n[run]\nomit = [\"src/app.py\"]\n",
        ),
    ] {
        let found = found(&excusing(file), &python(&[(file, source)]));

        assert!(found.is_empty(), "{source}: {found:?}");
    }
}

#[test]
fn a_coverage_configuration_that_does_not_read_is_reported() {
    for (file, source) in [
        (".coveragerc", "[run\nomit = a.py\n"),
        (".coveragerc", "[run]\nnothing here\n"),
        (".coveragerc", "[run]\nomit = a[z-a].py\n"),
        (".coveragerc", "[run]\nomit = src/a[z-a].py\n"),
        ("pyproject.toml", "[tool.coverage.run]\nomit = \"a.py\"\n"),
        (".coveragerc.toml", "[run]\nomit = \"a.py\"\n"),
        (".coveragerc.toml", "[run\nomit = [\"a.py\"]\n"),
    ] {
        let found = found(&excusing(file), &python(&[(file, source)]));

        assert_eq!(
            summary(&found),
            [format!("unreadable-ignore-file {file}")],
            "{source}"
        );
        assert!(
            found[0].message.contains("coverage.py"),
            "{}",
            found[0].message
        );
    }
}

#[test]
fn a_python_file_that_takes_itself_out_of_ruff_is_reported() {
    let declaration =
        format!("{QUIET}\n[[exclude]]\npath = \"vendor/x.py\"\nreason = \"code of others\"\n");
    let found = found(
        &declaration,
        &python(&[
            ("a.py", "\"\"\"A.\"\"\"\n\n# ruff: noqa\nx = 1\n"),
            ("b.py", "  #  flake8 :  NOQA \n"),
            ("c.py", "# ruff: noqa: F401\nimport os\n"),
            ("d.pyi", "#ruff:noqa\n"),
            ("e.py", "x = 1  # noqa\n# pylint: noqa\n# no colon here\n"),
            ("vendor/x.py", "# ruff: noqa\n"),
            ("notes.md", "# ruff: noqa\n"),
        ]),
    );

    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore a.py",
            "unexcused-ignore b.py",
            "unexcused-ignore d.pyi",
        ]
    );
    let message = &found[0].message;
    assert!(
        message.contains(
            "this file takes itself out of Ruff's linter through `# ruff: noqa` on line 3"
        ),
        "{message}"
    );
    assert!(message.contains("`# ruff: noqa: <code>`"), "{message}");
}

#[test]
fn pyright_exclude_takes_out_what_it_names_below_its_folder() {
    let found = found(
        QUIET,
        &python(&[(
            "svc/pyrightconfig.json",
            "{\"exclude\": [\"gen\", \"./src/x.py\"]}\n",
        )]),
    );

    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore svc/gen/c.py",
            "unexcused-ignore svc/src/x.py",
        ]
    );
    let message = &found[0].message;
    assert!(
        message.contains(
            "svc/pyrightconfig.json takes this file out of Pyright through the pattern `gen`"
        ),
        "{message}"
    );
    assert!(
        message.contains("[[exclude]] entry with a reason"),
        "{message}"
    );
}

#[test]
fn pyright_reads_ignore_and_exclude_of_one_configuration() {
    let found = found(
        QUIET,
        &python(&[(
            "pyproject.toml",
            "[tool.pyright]\nignore = [\"src/old.py\"]\nexclude = [\"src/gen\", \"src/old.py\"]\n",
        )]),
    );

    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore src/gen/a.py",
            "unexcused-ignore src/gen/sub/b.py",
            "unexcused-ignore src/old.py",
        ]
    );
}

#[test]
fn pyright_exclude_beside_a_matching_exclusion_is_no_finding() {
    let declaration = format!(
        "{QUIET}\n[[exclude]]\npath = \"src/gen\"\nreason = \"generated\"\n\n\
         [[exclude]]\npath = \"tests\"\nreason = \"of others\"\n"
    );
    let found = found(
        &declaration,
        &python(&[(
            "pyproject.toml",
            "[tool.pyright]\nexclude = [\"src/gen\", \"tests/*.py\"]\n",
        )]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn what_pyright_excludes_by_default_is_no_finding() {
    let found = found(
        QUIET,
        &python(&[
            (
                "pyproject.toml",
                "[tool.pyright]\nexclude = [\"**/node_modules\", \"**/__pycache__\", \"**/.*\", \
                 \"build\", \"src/*\"]\n",
            ),
            ("node_modules/m.py", "x = 1\n"),
            ("src/__pycache__/c.py", "x = 1\n"),
            (".venv/lib/v.py", "x = 1\n"),
            ("build/b.py", "x = 1\n"),
        ]),
    );

    // `src/*` takes out what lies in `src/`, but `src/__pycache__/` is on
    // Pyright's default list all the same.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore build/b.py",
            "unexcused-ignore src/app.py",
            "unexcused-ignore src/gen/a.py",
            "unexcused-ignore src/gen/sub/b.py",
            "unexcused-ignore src/old.py",
            "unexcused-ignore src/types.pyi",
        ]
    );
}

#[test]
fn ruff_exclude_settings_take_out_what_they_name() {
    let found = found(
        QUIET,
        &python(&[(
            "pyproject.toml",
            "[project]\nname = \"a\"\n\n[tool.ruff]\nline-length = 100\n\
             extend-exclude = [\"gen\", \"deep/*\"]\n",
        )]),
    );

    // A pattern without a `/` names a folder of that name anywhere below,
    // and `*` crosses a `/`.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore deep/down/legacy.py",
            "unexcused-ignore src/gen/a.py",
            "unexcused-ignore src/gen/sub/b.py",
            "unexcused-ignore svc/gen/c.py",
        ]
    );
    let message = &found[1].message;
    assert!(
        message.contains("pyproject.toml takes this file out of Ruff through the pattern `gen`"),
        "{message}"
    );
    assert!(
        message.contains("The pipeline checks it all the same: give it an [[exclude]] entry"),
        "{message}"
    );
}

#[test]
fn ruff_exclude_beside_a_matching_exclusion_is_no_finding() {
    let declaration = format!(
        "{QUIET}\n[[exclude]]\npath = \"src/gen\"\nreason = \"generated\"\n\n\
         [[exclude]]\npath = \"svc/gen\"\nreason = \"generated\"\n"
    );
    let found = found(
        &declaration,
        &python(&[(
            "pyproject.toml",
            "[tool.ruff]\nextend-exclude = [\"gen\"]\n",
        )]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn ruff_patterns_follow_its_own_rules() {
    let found = found(
        &excusing("ruff.toml"),
        &python(&[(
            "ruff.toml",
            "exclude = [\"./src/gen/\", \"sr*old.py\", \"*.pyi\", \"/abs/deep\", \"svc/*.py\"]\n",
        )]),
    );

    // A relative pattern is matched from the folder of the configuration,
    // `*` crossing a `/`; one without a `/` matches the name of the file or
    // of a folder above it; an absolute one names nothing in the repository.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore src/gen/a.py",
            "unexcused-ignore src/gen/sub/b.py",
            "unexcused-ignore src/old.py",
            "unexcused-ignore src/types.pyi",
            "unexcused-ignore svc/gen/c.py",
            "unexcused-ignore svc/src/x.py",
            "unexcused-ignore tests/stub.pyi",
        ]
    );
}

#[test]
fn what_ruff_excludes_by_default_is_no_finding() {
    let found = found(
        QUIET,
        &python(&[
            (
                ".ruff.toml",
                "exclude = [\".venv\", \"node_modules\", \"build\"]\nextend-exclude = [\"*.py\"]\n",
            ),
            ("node_modules/m.py", "x = 1\n"),
            ("pkg/.venv/lib/v.py", "x = 1\n"),
            ("dist/d.py", "x = 1\n"),
            ("build/b.py", "x = 1\n"),
        ]),
    );

    // `*.py` takes out every Python file, but those below a folder on Ruff's
    // default list are no finding, `exclude` replacing that list or not.
    let summary = summary(&found);
    assert!(
        summary.contains(&"unexcused-ignore build/b.py".to_owned()),
        "{summary:?}"
    );
    assert!(
        summary.contains(&"unexcused-ignore src/app.py".to_owned()),
        "{summary:?}"
    );
    for file in ["node_modules/m.py", "pkg/.venv/lib/v.py", "dist/d.py"] {
        assert!(
            !summary.contains(&format!("unexcused-ignore {file}")),
            "{summary:?}"
        );
    }
}

#[test]
fn ruff_takes_the_first_configuration_of_a_folder_and_the_nearest_one_governs() {
    let found = found(
        &excusing("svc/ruff.toml"),
        &python(&[
            (".ruff.toml", "extend-exclude = [\"gen\", \"legacy.py\"]\n"),
            ("ruff.toml", "extend-exclude = [\"src\"]\n"),
            ("svc/ruff.toml", "extend-exclude = [\"src/*\"]\n"),
            ("deep/pyproject.toml", "[project]\nname = \"deep\"\n"),
        ]),
    );

    // `svc/ruff.toml` governs `svc/`, so the root's `gen` does not reach
    // `svc/gen/`; `deep/pyproject.toml` has no `[tool.ruff]`, so the root's
    // configuration governs `deep/`.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore deep/down/legacy.py",
            "unexcused-ignore src/gen/a.py",
            "unexcused-ignore src/gen/sub/b.py",
            "unexcused-ignore svc/src/x.py",
        ]
    );
    assert!(
        found[3]
            .message
            .contains("svc/ruff.toml takes this file out of Ruff through the pattern `src/*`"),
        "{}",
        found[3].message
    );
}

#[test]
fn ruff_checks_the_folder_of_a_nearer_configuration_with_the_one_above() {
    let found = found(
        QUIET,
        &python(&[
            ("pyproject.toml", "[tool.ruff]\nexclude = [\"svc\"]\n"),
            ("svc/pyproject.toml", "[tool.ruff]\nline-length = 100\n"),
        ]),
    );

    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore svc/gen/c.py",
            "unexcused-ignore svc/src/x.py",
        ]
    );
}

#[test]
fn ruff_settings_without_exclude_take_nothing_out() {
    for (file, source) in [
        ("ruff.toml", "line-length = 100\n"),
        ("ruff.toml", "exclude = []\nextend-exclude = []\n"),
        (".ruff.toml", ""),
        ("pyproject.toml", "[tool.ruff]\nline-length = 100\n"),
        ("pyproject.toml", "[tool.ruff.lint]\nselect = [\"ALL\"]\n"),
    ] {
        let found = found(&excusing(file), &python(&[(file, source)]));

        assert!(found.is_empty(), "{source}: {found:?}");
    }
}

#[test]
fn a_ruff_configuration_that_does_not_read_is_reported() {
    for (file, source) in [
        ("ruff.toml", "exclude = \"gen\"\n"),
        (".ruff.toml", "extend-exclude = [\"a[z-a]\"]\n"),
        ("ruff.toml", "exclude = [\n"),
        ("svc/pyproject.toml", "[tool.ruff]\nexclude = 1\n"),
    ] {
        let found = found(&excusing(file), &python(&[(file, source)]));

        assert_eq!(
            summary(&found),
            [format!("unreadable-ignore-file {file}")],
            "{source}"
        );
        assert!(found[0].message.contains("Ruff"), "{}", found[0].message);
    }
}

#[test]
fn a_ruff_configuration_that_cannot_be_read_is_reported() {
    let analysis = analysis(QUIET, &[".nebensound/repo.toml", "a.py", "ruff.toml"]);
    let read = |path: &RelativePath| match path.to_string().as_str() {
        ".nebensound/repo.toml" => Ok(QUIET.to_owned()),
        "a.py" => Ok("x = 1\n".to_owned()),
        _ => Err("it is a link".to_owned()),
    };

    let found = findings(&analysis, central().register(), &read);

    assert_eq!(summary(&found), ["unreadable-ignore-file ruff.toml"]);
    assert!(
        found[0].message.contains("Ruff") && found[0].message.contains("it is a link"),
        "{}",
        found[0].message
    );
}

#[test]
fn a_python_file_that_cannot_be_read_holds_no_ruff_line() {
    let analysis = analysis(QUIET, &[".nebensound/repo.toml", "a.py", "b.py"]);
    let read = |path: &RelativePath| match path.to_string().as_str() {
        ".nebensound/repo.toml" => Ok(QUIET.to_owned()),
        "a.py" => Ok("# ruff: noqa\n".to_owned()),
        _ => Err("not text".to_owned()),
    };

    let found = findings(&analysis, central().register(), &read);

    assert_eq!(summary(&found), ["unexcused-ignore a.py"]);
}

/// A repository with stylesheets and pages: at the root, in `web/`, and in
/// `web/admin/`.
fn web(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut files = with(&[
        ("site.css", "a {}\n"),
        ("web/page.html", "<p>a</p>\n"),
        ("web/theme.scss", "a {}\n"),
        ("web/admin/admin.css", "a {}\n"),
        ("web/admin/index.htm", "<p>a</p>\n"),
    ]);
    files.extend_from_slice(extra);
    files
}

#[test]
fn prettier_and_lychee_read_stylesheets_and_pages_as_well() {
    let found = found(
        &excusing(".prettierignore"),
        &web(&[
            (".prettierignore", "web/\n"),
            (
                "lychee.toml",
                "exclude_path = [\"^site\\\\.css$\", \"admin\"]\n",
            ),
        ]),
    );

    // lychee reads the pages of check-html and no stylesheet.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore web/admin/admin.css",
            "unexcused-ignore web/admin/index.htm",
            "unexcused-ignore web/admin/index.htm",
            "unexcused-ignore web/page.html",
            "unexcused-ignore web/theme.scss",
        ]
    );
}

#[test]
fn a_stylelintignore_at_the_root_takes_out_stylesheets_only() {
    let taken = found(
        &excusing(".stylelintignore"),
        &web(&[(".stylelintignore", "admin/\n*.html\n")]),
    );

    assert_eq!(summary(&taken), ["unexcused-ignore web/admin/admin.css"]);
    assert!(
        taken[0].message.contains("Stylelint"),
        "{}",
        taken[0].message
    );

    let below = found(QUIET, &web(&[("web/.stylelintignore", "*\n")]));
    assert!(below.is_empty(), "{below:?}");
}

#[test]
fn stylelint_ignore_files_count_below_their_folder_up_to_a_nearer_configuration() {
    let found = found(
        QUIET,
        &web(&[
            (
                ".stylelintrc.json",
                "{ \"ignoreFiles\": [\"**/*.css\", \"web/*.scss\"] }\n",
            ),
            ("web/admin/.stylelintrc.yaml", "rules: {}\n"),
        ]),
    );

    // The nearer configuration in web/admin/ governs admin.css, and its
    // `ignoreFiles` takes nothing out.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore site.css",
            "unexcused-ignore web/theme.scss"
        ]
    );
    assert!(
        found[0].message.contains("through the pattern `**/*.css`"),
        "{}",
        found[0].message
    );
}

#[test]
fn stylelint_reads_ignore_files_in_order_with_negation() {
    let taken = found(
        QUIET,
        &web(&[(
            "web/.stylelintrc",
            "ignoreFiles:\n  - \"**\"\n  - \"!admin/**\"\n",
        )]),
    );
    assert_eq!(summary(&taken), ["unexcused-ignore web/theme.scss"]);

    // A list of negated patterns alone takes out every file none of them
    // names; a single pattern may stand as text.
    let taken = found(
        QUIET,
        &web(&[("web/.stylelintrc.yml", "ignoreFiles:\n  - \"!admin/**\"\n")]),
    );
    assert_eq!(summary(&taken), ["unexcused-ignore web/theme.scss"]);
    assert!(
        taken[0]
            .message
            .contains("since it matches none of the patterns `!admin/**`"),
        "{}",
        taken[0].message
    );
    let taken = found(
        QUIET,
        &web(&[("web/.stylelintrc.yml", "ignoreFiles: admin/*.css\n")]),
    );
    assert_eq!(summary(&taken), ["unexcused-ignore web/admin/admin.css"]);
    let taken = found(
        QUIET,
        &web(&[("web/.stylelintrc.yml", "ignoreFiles: []\n")]),
    );
    assert!(taken.is_empty(), "{taken:?}");
}

#[test]
fn stylelint_takes_the_first_configuration_of_a_folder() {
    // package.json counts only with a `stylelint` key; the hidden folder
    // `.config` comes after the files in the folder itself.
    let found = found(
        QUIET,
        &web(&[
            ("package.json", "{ \"name\": \"site\" }\n"),
            (".config/stylelintrc.yaml", "ignoreFiles: \"*.css\"\n"),
            (
                "web/package.json",
                "{ \"stylelint\": { \"ignoreFiles\": [\"*.scss\"] } }\n",
            ),
            ("web/.stylelintrc.json", "{ \"ignoreFiles\": [\"**\"] }\n"),
            ("web/admin/package.json", "{ \"name\": \"admin\" }\n"),
        ]),
    );
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore site.css",
            "unexcused-ignore web/theme.scss"
        ]
    );
}

#[test]
fn a_stylelint_configuration_in_javascript_is_not_read_and_still_governs_its_folder() {
    let found = found(
        QUIET,
        &web(&[
            (".stylelintrc.json", "{ \"ignoreFiles\": [\"**/*.css\"] }\n"),
            (
                "web/stylelint.config.mjs",
                "export default { ignoreFiles: [\"**\"] };\n",
            ),
        ]),
    );

    assert_eq!(summary(&found), ["unexcused-ignore site.css"]);
}

#[test]
fn a_stylelint_configuration_that_does_not_read_is_reported() {
    for (file, source) in [
        (".stylelintrc.json", "{ \"ignoreFiles\": [ }"),
        (".stylelintrc.yaml", "ignoreFiles: 3\n"),
        (".stylelintrc.yaml", "ignoreFiles: [\"a[\"]\n"),
        ("package.json", "{ \"stylelint\": "),
    ] {
        let found = found(QUIET, &web(&[(file, source)]));
        assert_eq!(
            summary(&found),
            [format!("unreadable-ignore-file {file}")],
            "{source}"
        );
        assert!(
            found[0].message.contains("Stylelint"),
            "{}",
            found[0].message
        );
    }
}

#[test]
fn html_validate_ignore_files_count_in_every_folder() {
    let found = found(
        QUIET,
        &web(&[
            ("web/.htmlvalidateignore", "admin/\n"),
            ("web/admin/.htmlvalidateignore", "*.css\n"),
        ]),
    );

    // Only the pages count, and every ignore file relative to its folder.
    assert_eq!(summary(&found), ["unexcused-ignore web/admin/index.htm"]);
    assert!(
        found[0].message.contains("html-validate"),
        "{}",
        found[0].message
    );
}

#[test]
fn ruff_lint_exclude_takes_a_file_out_of_the_linter_alone() {
    let found = found(
        QUIET,
        &python(&[(
            "pyproject.toml",
            "[tool.ruff.lint]\nexclude = [\"src/gen/*\", \"old.py\", \"tests\"]\n",
        )]),
    );

    // Ruff matches `lint.exclude` against the file's path and name only, so
    // a folder's name (`tests`) takes nothing out.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore src/gen/a.py",
            "unexcused-ignore src/gen/sub/b.py",
            "unexcused-ignore src/old.py",
        ]
    );
    let message = &found[0].message;
    assert!(
        message.contains(
            "pyproject.toml takes this file out of Ruff through the pattern `src/gen/*` of \
             `lint.exclude`, which leaves it out of `ruff check` while `ruff format` still \
             reads it"
        ),
        "{message}"
    );
    assert!(
        message.contains("give it an [[exclude]] entry"),
        "{message}"
    );
}

#[test]
fn ruff_format_exclude_takes_a_file_out_of_the_formatter_alone() {
    let found = found(
        &excusing("ruff.toml"),
        &python(&[("ruff.toml", "[format]\nexclude = [\"*.pyi\"]\n")]),
    );

    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore src/types.pyi",
            "unexcused-ignore tests/stub.pyi",
        ]
    );
    assert!(
        found[0].message.contains(
            "through the pattern `*.pyi` of `format.exclude`, which leaves it out of \
             `ruff format` while `ruff check` still reads it"
        ),
        "{}",
        found[0].message
    );
}

#[test]
fn a_file_out_of_both_steps_of_ruff_is_reported_for_each() {
    let found = found(
        &excusing("ruff.toml"),
        &python(&[(
            "ruff.toml",
            "[lint]\nexclude = [\"src/old.py\"]\n\n[format]\nexclude = [\"old.py\"]\n",
        )]),
    );

    assert_eq!(
        summary(&found),
        ["unexcused-ignore src/old.py", "unexcused-ignore src/old.py"]
    );
    assert!(found[0].message.contains("`lint.exclude`"), "{found:?}");
    assert!(found[1].message.contains("`format.exclude`"), "{found:?}");
}

#[test]
fn ruff_lint_and_format_exclude_beside_a_matching_exclusion_are_no_finding() {
    let declaration = format!(
        "{QUIET}\n[[exclude]]\npath = \"src/gen\"\nreason = \"generated\"\n\n\
         [[exclude]]\npath = \"src/types.pyi\"\nreason = \"generated\"\n\n\
         [[exclude]]\npath = \"tests/stub.pyi\"\nreason = \"generated\"\n"
    );
    let found = found(
        &declaration,
        &python(&[(
            "pyproject.toml",
            "[tool.ruff.lint]\nexclude = [\"src/gen/*\"]\n\n\
             [tool.ruff.format]\nexclude = [\"*.pyi\"]\n",
        )]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn ruff_include_narrower_than_its_default_takes_out_what_it_leaves() {
    let found = found(
        &excusing("ruff.toml"),
        &python(&[("ruff.toml", "include = [\"src/*.py\", \"*.pyi\"]\n")]),
    );

    // The default takes every `.py` and `.pyi` file; `src/*.py` crosses a
    // `/`, and `*.pyi`, without a `/`, matches the whole path.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore deep/down/legacy.py",
            "unexcused-ignore svc/gen/c.py",
            "unexcused-ignore svc/src/x.py",
            "unexcused-ignore tests/test_app.py",
        ]
    );
    assert!(
        found[0].message.contains(
            "ruff.toml takes this file out of Ruff since it matches none of the `include` \
             patterns `src/*.py`, `*.pyi`, and no [[exclude]] entry does"
        ),
        "{}",
        found[0].message
    );
}

#[test]
fn ruff_include_beside_a_matching_exclusion_is_no_finding() {
    let declaration = format!(
        "{}\n[[exclude]]\npath = \"tests\"\nreason = \"of others\"\n\n\
         [[exclude]]\npath = \"svc\"\nreason = \"of others\"\n\n\
         [[exclude]]\npath = \"deep\"\nreason = \"of others\"\n",
        excusing("ruff.toml")
    );
    let found = found(
        &declaration,
        &python(&[("ruff.toml", "include = [\"src/**\"]\n")]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn ruff_extend_include_widens_include_and_never_narrows() {
    // `extend-include` alone only adds to the default.
    let found_alone = found(
        &excusing("ruff.toml"),
        &python(&[("ruff.toml", "extend-include = [\"src/app.py\"]\n")]),
    );
    assert!(found_alone.is_empty(), "{found_alone:?}");

    let found = found(
        &excusing("ruff.toml"),
        &python(&[(
            "ruff.toml",
            "include = [\"src/**\"]\nextend-include = [\"tests/*\", \"legacy.py\"]\n",
        )]),
    );

    // A bare `legacy.py` matches no whole path, so it adds nothing.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore deep/down/legacy.py",
            "unexcused-ignore svc/gen/c.py",
            "unexcused-ignore svc/src/x.py",
        ]
    );
    assert!(
        found[0].message.contains(
            "since it matches none of the `include` patterns `src/**`, nor any of \
             `extend-include`"
        ),
        "{}",
        found[0].message
    );
}

#[test]
fn an_empty_ruff_include_takes_out_every_file() {
    let found = found(
        &excusing("svc/ruff.toml"),
        &python(&[("svc/ruff.toml", "include = []\n")]),
    );

    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore svc/gen/c.py",
            "unexcused-ignore svc/src/x.py",
        ]
    );
    assert!(
        found[0]
            .message
            .contains("since its `include` names no pattern"),
        "{}",
        found[0].message
    );
}

#[test]
fn ruff_reads_include_and_lint_exclude_in_the_nearest_configuration() {
    let found = found(
        &excusing("svc/ruff.toml"),
        &python(&[
            (
                "pyproject.toml",
                "[tool.ruff]\ninclude = [\"src/*\", \"tests/*\", \"deep/*\"]\n\n\
                 [tool.ruff.lint]\nexclude = [\"*.py\"]\n",
            ),
            ("svc/ruff.toml", "line-length = 100\n"),
            ("node_modules/m.py", "x = 1\n"),
        ]),
    );

    // `svc/ruff.toml` governs `svc/` with Ruff's default; below a folder on
    // Ruff's default list nothing is reported.
    let summary = summary(&found);
    assert!(
        summary
            .iter()
            .all(|line| !line.contains("svc/") && !line.contains("node_modules")),
        "{summary:?}"
    );
    assert!(
        summary.contains(&"unexcused-ignore src/app.py".to_owned()),
        "{summary:?}"
    );
}

#[test]
fn ruff_follows_extend_and_reads_each_pattern_from_its_own_folder() {
    let found = found(
        &excusing("svc/ruff.toml"),
        &python(&[
            ("svc/ruff.toml", "extend = \"../shared/ruff.toml\"\n"),
            (
                "shared/ruff.toml",
                "extend = \"base.toml\"\nextend-exclude = [\"gen\", \"src/*\"]\n",
            ),
            (
                "shared/base.toml",
                "extend-exclude = [\"../svc/src/x.py\", \"../../svc/*\"]\n\n\
                 [lint]\nexclude = [\"y.py\"]\n",
            ),
            ("svc/lib/y.py", "x = 1\n"),
        ]),
    );

    // `gen` without a `/` names a folder anywhere below `svc/`; `src/*`
    // lies below `shared/`, so it names nothing in `svc/`; `..` goes up from
    // `shared/`, and above the root it names nothing.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore svc/gen/c.py",
            "unexcused-ignore svc/lib/y.py",
            "unexcused-ignore svc/src/x.py",
        ]
    );
    assert!(
        found[0].message.contains(
            "svc/ruff.toml takes this file out of Ruff through the pattern `gen` that it \
             takes from shared/ruff.toml through `extend`"
        ),
        "{}",
        found[0].message
    );
    assert!(
        found[1].message.contains(
            "the pattern `y.py` that it takes from shared/base.toml through `extend` of \
             `lint.exclude`"
        ),
        "{}",
        found[1].message
    );
    assert!(
        found[2].message.contains(
            "the pattern `../svc/src/x.py` that it takes from shared/base.toml through `extend`"
        ),
        "{}",
        found[2].message
    );
}

#[test]
fn a_setting_the_configuration_writes_holds_over_the_one_it_extends() {
    let found = found(
        &excusing("ruff.toml"),
        &python(&[
            (
                "ruff.toml",
                "extend = \"base.toml\"\nexclude = []\ninclude = [\"**/*.py\", \"**/*.pyi\"]\n",
            ),
            (
                "base.toml",
                "exclude = [\"src\"]\ninclude = [\"src/*\"]\n\n[format]\nexclude = [\"svc/*\"]\n",
            ),
        ]),
    );

    // `exclude` and `include` of `ruff.toml` hold; `format.exclude` comes
    // from `base.toml`.
    assert_eq!(
        summary(&found),
        [
            "unexcused-ignore svc/gen/c.py",
            "unexcused-ignore svc/src/x.py",
        ]
    );
    assert!(found[0].message.contains("`format.exclude`"), "{found:?}");
}

#[test]
fn an_inherited_include_names_its_file() {
    let found = found(
        &excusing("ruff.toml"),
        &python(&[
            ("ruff.toml", "extend = \"./base.toml\"\n"),
            (
                "base.toml",
                "include = [\"src/**\", \"tests/**\", \"svc/**\"]\n",
            ),
        ]),
    );

    assert_eq!(summary(&found), ["unexcused-ignore deep/down/legacy.py"]);
    assert!(
        found[0].message.contains(
            "none of the `include` patterns `src/**`, `tests/**`, `svc/**` that it takes \
             from base.toml through `extend`"
        ),
        "{}",
        found[0].message
    );
}

#[test]
fn ruff_extend_beside_a_matching_exclusion_is_no_finding() {
    let declaration = format!(
        "{}\n[[exclude]]\npath = \"src/gen\"\nreason = \"generated\"\n\n\
         [[exclude]]\npath = \"svc/gen\"\nreason = \"generated\"\n",
        excusing("ruff.toml")
    );
    let found = found(
        &declaration,
        &python(&[
            ("ruff.toml", "extend = \"base.toml\"\n"),
            ("base.toml", "extend-exclude = [\"gen\"]\n"),
        ]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn an_extend_outside_the_repository_is_not_followed() {
    for source in [
        "extend = \"../outside.toml\"\n",
        "extend = \"/etc/ruff.toml\"\n",
        "extend = \"~/ruff.toml\"\n",
        "extend = \"$HOME/ruff.toml\"\n",
    ] {
        let found = found(&excusing("ruff.toml"), &python(&[("ruff.toml", source)]));

        assert!(found.is_empty(), "{source}: {found:?}");
    }
}

#[test]
fn an_extend_chain_that_does_not_read_is_reported() {
    for (files, why) in [
        (
            &[
                ("ruff.toml", "extend = \"a.toml\"\n"),
                ("a.toml", "extend = \"ruff.toml\"\n"),
            ][..],
            "a.toml: `extend = \"ruff.toml\"` comes back round to a file above it, which Ruff \
             refuses",
        ),
        (
            &[("ruff.toml", "extend = \"./ruff.toml\"\n")][..],
            "`extend = \"./ruff.toml\"` comes back round",
        ),
        (
            &[("ruff.toml", "extend = \"missing.toml\"\n")][..],
            "missing.toml: no such file",
        ),
        (
            &[
                ("ruff.toml", "extend = \"a.toml\"\n"),
                ("a.toml", "include = \"src\"\n"),
            ][..],
            "a.toml: ",
        ),
        (
            &[
                ("ruff.toml", "extend = \"a.toml\"\n"),
                ("a.toml", "exclude = [\"a[z-a]\"]\n"),
            ][..],
            "a.toml: ",
        ),
        (&[("ruff.toml", "extend = 3\n")][..], "`extend` is not text"),
        (
            &[("ruff.toml", "extend-include = [\"a[z-a]\"]\n")][..],
            "invalid range",
        ),
        (
            &[("ruff.toml", "[format]\nexclude = [\"a[z-a]\"]\n")][..],
            "invalid range",
        ),
        (&[("ruff.toml", "lint = 3\n")][..], "lint"),
    ] {
        let found = found(&excusing("ruff.toml"), &python(files));

        assert_eq!(
            summary(&found),
            ["unreadable-ignore-file ruff.toml"],
            "{files:?}"
        );
        assert!(found[0].message.contains(why), "{}", found[0].message);
    }
}

#[test]
fn pyright_include_takes_out_what_none_of_its_entries_names() {
    let found = found(
        QUIET,
        &python(&[(
            "svc/pyrightconfig.json",
            "{\"include\": [\"src\", \"./gen/*.pyi\"]}\n",
        )]),
    );

    // Without `include` Pyright reads everything below the folder of its
    // configuration; nothing outside that folder is its business.
    assert_eq!(summary(&found), ["unexcused-ignore svc/gen/c.py"]);
    assert!(
        found[0].message.contains(
            "svc/pyrightconfig.json takes this file out of Pyright since it matches none of \
             the `include` patterns `src`, `./gen/*.pyi`"
        ),
        "{}",
        found[0].message
    );
}

#[test]
fn pyright_include_beside_a_matching_exclusion_is_no_finding() {
    let declaration = format!(
        "{QUIET}\n[[exclude]]\npath = \"tests\"\nreason = \"of others\"\n\n\
         [[exclude]]\npath = \"svc\"\nreason = \"of others\"\n\n\
         [[exclude]]\npath = \"deep\"\nreason = \"of others\"\n"
    );
    let found = found(
        &declaration,
        &python(&[("pyproject.toml", "[tool.pyright]\ninclude = [\"src\"]\n")]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn pyright_include_of_everything_or_nothing_takes_nothing_out() {
    for source in [
        "[tool.pyright]\ninclude = [\".\"]\n",
        "[tool.pyright]\ninclude = [\"./\"]\n",
        "[tool.pyright]\ninclude = [\"**\"]\n",
        "[tool.pyright]\ninclude = []\n",
        "[tool.pyright]\ninclude = [\"src\", \"tests\", \"svc\", \"deep\"]\n",
    ] {
        let found = found(
            QUIET,
            &python(&[("pyproject.toml", source), ("node_modules/m.py", "x = 1\n")]),
        );

        assert!(found.is_empty(), "{source}: {found:?}");
    }
}

#[test]
fn pyright_include_leaves_what_its_default_list_leaves_out_unreported() {
    let found = found(
        QUIET,
        &python(&[
            (
                "pyproject.toml",
                "[tool.pyright]\ninclude = [\"src\", \"tests\", \"svc\"]\n",
            ),
            ("node_modules/m.py", "x = 1\n"),
            (".venv/lib/v.py", "x = 1\n"),
        ]),
    );

    assert_eq!(summary(&found), ["unexcused-ignore deep/down/legacy.py"]);
}
