//! The tools' own ways of leaving a file out, held against the declaration.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use nebensound::ignored::{Tool, findings};
use nebensound_declaration::finding::{Finding, Rule};
use nebensound_declaration::path::RelativePath;
use support::{QUIET, analysis};

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
    findings(&analysis, &read)
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
    let found = findings(&analysis, &read);

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
fn taplo_exclude_takes_out_a_lockfile() {
    let found = found(QUIET, &with(&[("taplo.toml", "exclude = [\"*.lock\"]\n")]));

    assert_eq!(summary(&found), ["unexcused-ignore Cargo.lock"]);
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
fn yamllint_takes_out_what_its_ignore_names() {
    for source in ["ignore: |\n  sub/\n", "ignore:\n  - sub/\n"] {
        let found = found(&excusing(".yamllint"), &with(&[(".yamllint", source)]));
        assert_eq!(summary(&found), ["unexcused-ignore sub/x.yml"], "{source}");
        assert!(
            found[0].message.contains("yamllint"),
            "{}",
            found[0].message
        );
    }
}

#[test]
fn yamllint_reads_the_files_its_ignore_from_file_names() {
    let found = found(
        &excusing(".yamllint"),
        &with(&[
            (".yamllint", "ignore-from-file: [.gitignore]\n"),
            (".gitignore", "*.yml\n"),
        ]),
    );

    assert_eq!(summary(&found), ["unexcused-ignore sub/x.yml"]);
}

#[test]
fn yamllint_reads_the_ignore_of_the_configuration_it_extends() {
    let found = found(
        &excusing(".yamllint"),
        &with(&[
            (".yamllint", "extends: ci/yamllint-base\n"),
            ("ci/yamllint-base", "extends: ci/yamllint-root\n"),
            ("ci/yamllint-root", "extends: relaxed\nignore: sub/\n"),
        ]),
    );

    assert_eq!(summary(&found), ["unexcused-ignore sub/x.yml"]);
}

#[test]
fn yamllint_keeps_its_own_ignore_over_the_one_it_extends() {
    let found = found(
        &excusing(".yamllint"),
        &with(&[
            (".yamllint", "extends: base\nignore: docs/\n"),
            ("base", "ignore: \"*\"\n"),
        ]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn yamllint_without_ignore_or_a_file_it_extends_takes_nothing_out() {
    for source in ["rules: {}\n", "extends: default\n", "extends: relaxed\n"] {
        let found = found(QUIET, &with(&[(".yamllint", source)]));
        assert!(found.is_empty(), "{source}: {found:?}");
    }
}

#[test]
fn yamllint_reads_the_first_file_it_would_read() {
    let found = found(
        QUIET,
        &with(&[
            (".yamllint.yaml", "extends: default\n"),
            (".yamllint.yml", "ignore: \"*\"\n"),
        ]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_yamllint_configuration_that_does_not_read_is_reported() {
    for source in [
        "ignore: [\n",
        "ignore: 3\n",
        "ignore: a\nignore-from-file: b\n",
        "ignore-from-file: missing\n",
        "ignore-from-file: 3\n",
        "ignore-from-file: /etc/passwd\n",
        "extends: 3\n",
        "extends: missing\n",
        "extends: /etc/yamllint\n",
        "extends: .yamllint.yaml\n",
        "extends: ci/a\n",
    ] {
        let found = found(
            QUIET,
            &with(&[
                (".yamllint.yaml", source),
                ("ci/a", "extends: ci/b\n"),
                ("ci/b", "ignore: [\n"),
            ]),
        );
        assert_eq!(
            summary(&found),
            ["unreadable-ignore-file .yamllint.yaml"],
            "{source}"
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
fn settings_below_the_root_are_not_read_except_markdownlint() {
    let found = found(
        QUIET,
        &with(&[
            ("sub/.prettierignore", "*\n"),
            ("sub/taplo.toml", "exclude = [\"**\"]\n"),
            ("sub/.yamllint", "ignore: \"*\"\n"),
            ("sub/lychee.toml", "exclude_path = [\".\"]\n"),
        ]),
    );

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn every_tool_names_the_checks_it_reads() {
    assert_eq!(Tool::Prettier.checks(), ["json", "markdown", "yaml"]);
    assert_eq!(Tool::Taplo.checks(), ["toml"]);
    assert_eq!(Tool::Markdownlint.checks(), ["markdown"]);
    assert_eq!(Tool::Yamllint.checks(), ["yaml"]);
    assert_eq!(Tool::Lychee.checks(), ["markdown"]);
    assert_eq!(Tool::Lychee.name(), "lychee");
}
