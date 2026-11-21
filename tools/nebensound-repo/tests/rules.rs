//! The rules a repository sets differently from the pipeline's default.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::{BTreeMap, BTreeSet};

use nebensound_repo::path::RelativePath;
use nebensound_repo::rules::{Defaults, MARKDOWNLINT, YAMLLINT, report};
use support::{QUIET, analysis, call, repository};

/// What the report says of a Markdownlint rule the pipeline's file does not
/// name.
const MD_UNNAMED: &str = "the pipeline's file does not name it; Markdownlint's own default holds";

/// What it says of such a yamllint rule, beneath which lies yamllint's own
/// default.
const YAML_UNNAMED: &str =
    "the pipeline's file does not name it; what its `extends: default` sets holds";

/// The report for a repository under this declaration that holds these
/// paths, the files among them read from memory, held against these defaults.
fn report_for(
    declaration: &str,
    paths: &[&str],
    files: &[(&str, &str)],
    defaults: Defaults<'_>,
) -> String {
    let mut all: BTreeSet<&str> = paths.iter().copied().collect();
    all.extend(files.iter().map(|(path, _)| *path));
    let all: Vec<&str> = all.into_iter().collect();
    let contents: BTreeMap<String, String> = files
        .iter()
        .map(|(path, content)| ((*path).to_owned(), (*content).to_owned()))
        .collect();
    let analysis = analysis(declaration, &all);
    let read = |path: &RelativePath| {
        contents
            .get(&path.to_string())
            .cloned()
            .ok_or_else(|| "no such file".to_owned())
    };
    report(&analysis, &read, defaults)
}

/// The report for a repository holding these files, and a Markdown file in
/// each of their folders, so that the markdown check reaches every folder, held
/// against these defaults.
fn rules_with(defaults: Defaults<'_>, files: &[(&str, &str)]) -> String {
    let readmes: BTreeSet<String> = files
        .iter()
        .map(|(path, _)| match path.rsplit_once('/') {
            Some((dir, _)) => format!("{dir}/README.md"),
            None => "README.md".to_owned(),
        })
        .collect();
    let mut paths: Vec<&str> = vec![".nebensound/repo.toml", "README.md"];
    paths.extend(readmes.iter().map(String::as_str));
    report_for(QUIET, &paths, files, defaults)
}

/// The same against the defaults compiled into the tool.
fn rules(files: &[(&str, &str)]) -> String {
    rules_with(Defaults::BUILT_IN, files)
}

/// The text after the column, for each line that starts with `name` once the
/// indentation is taken off.
fn after(text: &str, name: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix(name)?;
            rest.starts_with(' ').then(|| rest.trim().to_owned())
        })
        .collect()
}

#[test]
fn without_a_configuration_of_its_own_the_defaults_hold_as_they_are() {
    let text = rules(&[]);

    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        [
            "Rules",
            "",
            "  markdown                                              Markdownlint: the default, \
             nothing laid over it",
            "  yaml                                                  yamllint: the default, \
             nothing laid over it",
        ]
    );
}

#[test]
fn the_defaults_compiled_in_are_the_files_the_blocks_read() {
    assert!(MARKDOWNLINT.contains("\"MD013\": false"), "{MARKDOWNLINT}");
    assert!(YAMLLINT.contains("extends: default"), "{YAMLLINT}");
    assert!(YAMLLINT.contains("line-length: disable"), "{YAMLLINT}");
}

#[test]
fn a_markdownlint_options_file_shows_how_each_rule_stands_to_the_default() {
    let text = rules(&[(
        ".markdownlint-cli2.jsonc",
        "{\n  // One rule different.\n  \"config\": { \"MD041\": false, \"MD013\": false },\n}\n",
    )]);

    assert!(
        text.contains("Markdownlint: the default, with these laid over it"),
        "{text}"
    );
    assert_eq!(
        after(&text, ".markdownlint-cli2.jsonc"),
        ["laid over the default rule by rule"]
    );
    assert_eq!(after(&text, "MD041"), [format!("false ({MD_UNNAMED})")]);
    assert_eq!(after(&text, "MD013"), ["false (the same as the default)"]);
}

#[test]
fn a_rule_the_default_sets_otherwise_is_shown_over_the_default() {
    let text = rules(&[(".markdownlint.json", "{\"MD013\": {\"line_length\": 120}}")]);

    assert_eq!(
        after(&text, "MD013"),
        ["{\"line_length\":120} (over the default false)"]
    );
}

#[test]
fn a_change_of_the_default_reaches_a_repository_that_did_not_name_the_rule() {
    let files = [(".markdownlint-cli2.yaml", "config:\n  MD041: false\n")];
    let changed = Defaults {
        markdownlint: "{ \"MD013\": false, \"MD041\": false }",
        yamllint: YAMLLINT,
    };

    let before = rules(&files);
    let after_change = rules_with(changed, &files);

    assert_eq!(after(&before, "MD041"), [format!("false ({MD_UNNAMED})")]);
    assert_eq!(
        after(&after_change, "MD041"),
        ["false (the same as the default)"]
    );
}

#[test]
fn every_folder_is_read_the_file_of_rules_first_and_then_the_options_file() {
    let text = rules(&[
        (
            ".markdownlint.jsonc",
            "{ \"$schema\": \"x\", \"extends\": \"base.json\", \"MD007\": { \"indent\": 4 } }",
        ),
        (".markdownlint.yaml", "MD001: false\n"),
        (
            ".markdownlint-cli2.jsonc",
            "{ \"config\": { \"MD041\": false } }",
        ),
        (".markdownlint-cli2.yaml", "config:\n  MD002: false\n"),
        ("docs/.markdownlint.yml", "MD033: false\n"),
        (
            "docs/.markdownlint-cli2.jsonc",
            "{ \"config\": { \"MD044\": false } }",
        ),
        (
            ".github/.markdownlint-cli2.yaml",
            "config:\n  MD041: false\n",
        ),
        ("lib/.markdownlint.cjs", "module.exports = {};\n"),
        ("tools/.markdownlint-cli2.mjs", "export default {};\n"),
    ]);

    let files: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("      ") && !line.starts_with("          "))
        .map(|line| line.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(
        files,
        [
            ".markdownlint.jsonc",
            ".markdownlint-cli2.jsonc",
            ".github/.markdownlint-cli2.yaml",
            "docs/.markdownlint.yml",
            "docs/.markdownlint-cli2.jsonc",
            "lib/.markdownlint.cjs",
            "tools/.markdownlint-cli2.mjs",
        ],
        "{text}"
    );
    assert!(!text.contains("$schema"), "{text}");
    assert!(!text.contains("MD001"), "{text}");
    assert!(!text.contains("MD002"), "{text}");
    assert!(!text.contains("MD044"), "{text}");
    assert_eq!(
        after(&text, "extends"),
        ["base.json: what it sets lies beneath the rules here"]
    );
    assert_eq!(
        after(&text, "MD007"),
        [format!("{{\"indent\":4}} ({MD_UNNAMED})")]
    );
    assert_eq!(
        after(&text, ".github/.markdownlint-cli2.yaml"),
        ["laid over the default and what lies above it, rule by rule"]
    );
    assert_eq!(
        after(&text, "docs/.markdownlint.yml"),
        ["in place of everything above it, the default included"]
    );
    assert_eq!(
        after(&text, "docs/.markdownlint-cli2.jsonc"),
        ["its `config` is not read: the file of rules only in its folder takes its place"]
    );
    assert_eq!(
        after(&text, "lib/.markdownlint.cjs"),
        ["written in JavaScript, which phase 0 does not run"]
    );
    assert_eq!(
        after(&text, "tools/.markdownlint-cli2.mjs"),
        ["written in JavaScript, which phase 0 does not run"]
    );
}

#[test]
fn a_markdownlint_rule_is_found_by_any_of_its_names_and_tags() {
    let text = rules(&[(
        ".markdownlint.jsonc",
        "{ \"line-length\": true, \"md013\": false, \"line_length\": { \"line_length\": 100 }, \
           \"headings\": false, \"default\": true, \"my-rule\": true }",
    )]);

    assert_eq!(
        after(&text, "line-length"),
        ["true (over the default false)"]
    );
    assert_eq!(after(&text, "md013"), ["false (the same as the default)"]);
    assert_eq!(
        after(&text, "line_length"),
        ["{\"line_length\":100} (over the default false)"]
    );
    assert_eq!(after(&text, "headings"), [format!("false ({MD_UNNAMED})")]);
    assert_eq!(after(&text, "default"), [format!("true ({MD_UNNAMED})")]);
    assert_eq!(after(&text, "my-rule"), [format!("true ({MD_UNNAMED})")]);
}

#[test]
fn a_markdownlint_tag_over_rules_the_default_treats_apart_says_so() {
    let defaults = Defaults {
        markdownlint: "{ \"MD001\": false, \"heading-style\": false, \"whitespace\": false }",
        yamllint: YAMLLINT,
    };

    let text = rules_with(
        defaults,
        &[(
            ".markdownlint.jsonc",
            "{ \"headings\": false, \"whitespace\": true, \"MD018\": false }",
        )],
    );

    assert_eq!(
        after(&text, "headings"),
        ["false (the default does not give the rules it covers one value)"]
    );
    assert_eq!(
        after(&text, "whitespace"),
        ["true (over the default false)"]
    );
    assert_eq!(after(&text, "MD018"), [format!("false ({MD_UNNAMED})")]);
}

#[test]
fn the_default_of_markdownlint_stands_for_every_rule_it_does_not_name() {
    let defaults = Defaults {
        markdownlint: "{ \"Default\": false, \"MD013\": true }",
        yamllint: YAMLLINT,
    };

    let text = rules_with(
        defaults,
        &[(
            ".markdownlint.jsonc",
            "{ \"MD041\": false, \"MD001\": true, \"MD013\": true }",
        )],
    );

    assert_eq!(after(&text, "MD041"), ["false (the same as the default)"]);
    assert_eq!(after(&text, "MD001"), ["true (over the default false)"]);
    assert_eq!(after(&text, "MD013"), ["true (the same as the default)"]);
}

#[test]
fn a_yamllint_default_without_extends_leaves_a_rule_it_does_not_name_off() {
    let defaults = Defaults {
        markdownlint: MARKDOWNLINT,
        yamllint: "rules:\n  comments: enable\n",
    };

    let text = rules_with(defaults, &[(".yamllint", "rules:\n  hyphens: enable\n")]);

    assert_eq!(
        after(&text, "hyphens"),
        ["\"enable\" (the pipeline's file does not name it, so it is off)"]
    );
}

#[test]
fn a_file_that_names_no_rule_says_so() {
    let text = rules(&[
        (".markdownlint-cli2.jsonc", "{ \"globs\": [] }"),
        ("docs/.markdownlint.json", "{}"),
    ]);

    assert_eq!(
        after(&text, ".markdownlint-cli2.jsonc"),
        ["laid over the default rule by rule; it names no rule"]
    );
    assert_eq!(
        after(&text, "docs/.markdownlint.json"),
        ["in place of everything above it, the default included; it names no rule"]
    );
}

#[test]
fn a_file_that_cannot_be_read_says_why() {
    let text = rules(&[
        ("a/.markdownlint.jsonc", "{ \"MD001\": "),
        ("b/.markdownlint.jsonc", "[1]"),
        ("c/.markdownlint-cli2.jsonc", "[1]"),
        ("c2/.markdownlint-cli2.jsonc", "{"),
        ("d/.markdownlint.jsonc", "{ \"extends\": 1 }"),
        ("e/.markdownlint.yaml", "a: [\n"),
        ("f/.markdownlint.yaml", ""),
        (".yamllint", "rules: [1]\n"),
    ]);

    let unreadable: Vec<String> = text
        .lines()
        .filter(|line| line.contains("cannot be read"))
        .map(|line| line.split_whitespace().next().unwrap().to_owned())
        .collect();
    assert_eq!(
        unreadable,
        [
            "a/.markdownlint.jsonc",
            "b/.markdownlint.jsonc",
            "c/.markdownlint-cli2.jsonc",
            "c2/.markdownlint-cli2.jsonc",
            "d/.markdownlint.jsonc",
            "e/.markdownlint.yaml",
            "f/.markdownlint.yaml",
            ".yamllint",
        ],
        "{text}"
    );
    assert_eq!(
        after(&text, "b/.markdownlint.jsonc"),
        ["cannot be read: it holds no mapping"]
    );
    assert_eq!(
        after(&text, "d/.markdownlint.jsonc"),
        ["cannot be read: `extends` is not text"]
    );
    assert_eq!(
        after(&text, ".yamllint"),
        ["cannot be read: `rules` is not a mapping"]
    );
}

#[test]
fn a_file_the_tree_lists_and_the_disk_does_not_hold_says_so() {
    let analysis = analysis(QUIET, &[".nebensound/repo.toml", ".yamllint.yml"]);
    let read = |_: &RelativePath| Err("gone".to_owned());

    let text = report(&analysis, &read, Defaults::BUILT_IN);

    assert_eq!(after(&text, ".yamllint.yml"), ["cannot be read: gone"]);
}

#[test]
fn yamllint_lays_the_options_of_a_rule_over_those_of_the_default() {
    let text = rules(&[(
        ".yamllint.yaml",
        "extends: relaxed\n\
         rules:\n\
         \x20 comments:\n\
         \x20   min-spaces-from-content: 1\n\
         \x20 braces: enable\n\
         \x20 truthy:\n\
         \x20   check-keys: true\n\
         \x20 line-length: disable\n\
         \x20 document-start: enable\n\
         \x20 key-ordering: enable\n\
         \x20 indentation: { spaces: 4 }\n",
    )]);

    assert_eq!(
        after(&text, ".yamllint.yaml"),
        ["laid over the default rule by rule"]
    );
    assert_eq!(
        after(&text, "extends"),
        ["relaxed: what it sets lies beneath the rules here"]
    );
    assert_eq!(
        after(&text, "comments"),
        ["{\"min-spaces-from-content\":1} (the same as the default)"]
    );
    assert_eq!(
        after(&text, "braces"),
        ["\"enable\" (the same as the default)"]
    );
    assert_eq!(
        after(&text, "truthy"),
        ["{\"check-keys\":true} (over the default {\"check-keys\":false})"]
    );
    assert_eq!(
        after(&text, "line-length"),
        ["\"disable\" (the same as the default)"]
    );
    assert_eq!(
        after(&text, "document-start"),
        ["\"enable\" (over the default \"disable\")"]
    );
    assert_eq!(
        after(&text, "key-ordering"),
        [format!("\"enable\" ({YAML_UNNAMED})")]
    );
    assert_eq!(
        after(&text, "indentation"),
        [format!("{{\"spaces\":4}} ({YAML_UNNAMED})")]
    );
}

#[test]
fn yamllint_reads_the_first_of_its_files_at_the_root_only() {
    let text = rules(&[
        (".yamllint", "rules:\n  key-ordering: enable\n"),
        (".yamllint.yaml", "rules:\n  empty-lines: disable\n"),
        ("sub/.yamllint", "rules:\n  hyphens: disable\n"),
    ]);

    assert_eq!(
        after(&text, "key-ordering"),
        [format!("\"enable\" ({YAML_UNNAMED})")]
    );
    assert!(!text.contains("empty-lines"), "{text}");
    assert!(!text.contains("hyphens"), "{text}");
}

#[test]
fn every_kind_of_yaml_value_is_shown_as_the_json_it_stands_for() {
    let text = rules(&[(
        ".yamllint.yml",
        "rules:\n\
         \x20 a: 3\n\
         \x20 b: 1.5\n\
         \x20 c: .inf\n\
         \x20 d: ~\n\
         \x20 e: true\n\
         \x20 f: [x, 2]\n\
         \x20 g: { 1: x }\n",
    )]);

    assert_eq!(after(&text, "a"), [format!("3 ({YAML_UNNAMED})")]);
    assert_eq!(after(&text, "b"), [format!("1.5 ({YAML_UNNAMED})")]);
    assert_eq!(after(&text, "c"), [format!("\".inf\" ({YAML_UNNAMED})")]);
    assert_eq!(after(&text, "d"), [format!("null ({YAML_UNNAMED})")]);
    assert_eq!(after(&text, "e"), [format!("true ({YAML_UNNAMED})")]);
    assert_eq!(after(&text, "f"), [format!("[\"x\",2] ({YAML_UNNAMED})")]);
    assert_eq!(
        after(&text, "g"),
        [format!("{{\"1\":\"x\"}} ({YAML_UNNAMED})")]
    );
}

#[test]
fn a_yamllint_file_without_rules_names_none() {
    let text = rules(&[(".yamllint", "locale: C.UTF-8\n")]);

    assert_eq!(
        after(&text, ".yamllint"),
        ["laid over the default rule by rule; it names no rule"]
    );
}

#[test]
fn a_name_that_could_forge_a_line_is_escaped() {
    let text = rules(&[(".markdownlint.json", "{\"MD\\u001b[2J\": \"\\nfake\"}")]);

    assert!(!text.contains('\u{1b}'), "{text}");
    assert_eq!(text.lines().count(), 6, "{text}");
}

#[test]
fn a_default_that_does_not_read_is_named() {
    let broken = Defaults {
        markdownlint: "{",
        yamllint: "- a\n",
    };

    let text = rules_with(broken, &[(".yamllint", "rules: {}\n")]);

    let lines: Vec<&str> = text.lines().skip(2).collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(
        lines[0].ends_with("Markdownlint: the pipeline's default does not read: Unterminated object on line 1 column 2"),
        "{text}"
    );
    assert!(
        lines[1].ends_with("yamllint: the pipeline's default does not read: it holds no mapping"),
        "{text}"
    );
}

#[test]
fn check_prints_the_rules_after_the_configuration() {
    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("README.md", "# x\n"),
        (
            ".markdownlint-cli2.jsonc",
            "{ \"config\": { \"MD041\": false } }\n",
        ),
    ]);
    let root = dir.path().to_str().unwrap();

    let (text, code) = call(&["nebensound-repo", "-C", root, "check"]);

    assert_eq!(code, 0, "{text}");
    let configuration = text.find("Resolved configuration").unwrap();
    let rules = text.find("\nRules\n").unwrap();
    let tree = text.find("\nTree\n").unwrap();
    assert!(configuration < rules && rules < tree, "{text}");
    assert_eq!(after(&text, "MD041"), [format!("false ({MD_UNNAMED})")]);
}

#[test]
fn a_yamllint_file_that_cannot_be_read_says_why() {
    for (source, why) in [
        ("a: [\n", "cannot be read: while parsing"),
        (
            "extends: [default]\n",
            "cannot be read: `extends` is not text",
        ),
        ("- rules\n", "cannot be read: it holds no mapping"),
        (
            "rules: {}\nrules: {}\n",
            "cannot be read: String(\"rules\"): duplicated key in mapping",
        ),
    ] {
        let text = rules(&[(".yamllint", source)]);

        let shown = after(&text, ".yamllint");
        assert!(shown[0].starts_with(why), "{text}");
    }
}

#[test]
fn markdownlint_files_where_the_check_lints_nothing_are_not_read() {
    let declaration = "[repo]\naudience = \"public\"\n\
                       pipeline = \"none\"\n\
                       \n\
                       [[exclude]]\n\
                       path   = \"vendor\"\n\
                       reason = \"a vendored copy, kept as it came\"\n\
                       \n\
                       [[exclude]]\n\
                       path   = \"docs/.markdownlint.json\"\n\
                       reason = \"kept out of the json check\"\n";
    let files = [
        (
            "vendor/.markdownlint-cli2.jsonc",
            "{ \"config\": { \"MD001\": false } }",
        ),
        ("vendor/sub/.markdownlint.json", "{ \"MD003\": false }"),
        ("assets/.markdownlint.json", "{ \"MD004\": false }"),
        ("docs/.markdownlint.json", "{ \"MD005\": false }"),
        ("guide/.markdownlint.json", "{ \"MD007\": false }"),
    ];
    let paths = [
        ".nebensound/repo.toml",
        "README.md",
        "vendor/README.md",
        "vendor/sub/notes.md",
        "assets/logo.png",
        "docs/index.md",
        "guide/part/one.md",
    ];

    let text = report_for(declaration, &paths, &files, Defaults::BUILT_IN);

    for gone in ["vendor/", "assets/", "MD001", "MD003", "MD004"] {
        assert!(!text.contains(gone), "{gone}: {text}");
    }
    assert_eq!(
        after(&text, "docs/.markdownlint.json"),
        ["in place of everything above it, the default included"]
    );
    assert_eq!(
        after(&text, "guide/.markdownlint.json"),
        ["in place of everything above it, the default included"]
    );
}

#[test]
fn without_a_file_the_markdown_check_lints_nothing_is_laid_over_its_default() {
    let text = report_for(
        "[repo]\naudience = \"public\"\npipeline = \"none\"\n",
        &[".nebensound/repo.toml"],
        &[(".markdownlint.json", "{ \"MD041\": false }")],
        Defaults::BUILT_IN,
    );

    assert!(
        text.contains("Markdownlint: the default, nothing laid over it"),
        "{text}"
    );
}

#[test]
fn yamllint_reads_its_configuration_as_yaml_1_1() {
    let text = rules(&[(
        ".yamllint",
        "rules:\n\
         \x20 truthy:\n\
         \x20   check-keys: no\n\
         \x20 document-start: Off\n\
         \x20 key-ordering: { a: YES, b: on, c: 'no', d: \"off\", e: !!str yes }\n",
    )]);

    assert_eq!(
        after(&text, "truthy"),
        ["{\"check-keys\":false} (the same as the default)"]
    );
    assert_eq!(
        after(&text, "document-start"),
        ["false (the same as the default)"]
    );
    assert_eq!(
        after(&text, "key-ordering"),
        [format!(
            "{{\"a\":true,\"b\":true,\"c\":\"no\",\"d\":\"off\",\"e\":\"yes\"}} ({YAML_UNNAMED})"
        )]
    );
}

#[test]
fn markdownlint_reads_yaml_as_1_2() {
    let text = rules(&[(".markdownlint.yaml", "MD013: no\n")]);

    assert_eq!(after(&text, "MD013"), ["\"no\" (over the default false)"]);
}
