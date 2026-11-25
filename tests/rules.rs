//! The rules a repository sets differently from the pipeline's default.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::{BTreeMap, BTreeSet};

use nebensound_repo::path::RelativePath;
use nebensound_repo::register::Register;
use nebensound_repo::rules::{Defaults, HTML_VALIDATE, MARKDOWNLINT, RUFF, STYLELINT, report};
use support::{QUIET, analysis, call, repository};

/// What the report says of a Markdownlint rule the pipeline's file does not
/// name.
const MD_UNNAMED: &str = "the pipeline's file does not name it; Markdownlint's own default holds";

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
    report(&analysis, &read, defaults, &register())
}

/// The register compiled into the tool.
fn register() -> Register {
    Register::builtin().expect("the register")
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
        ]
    );
}

#[test]
fn the_defaults_compiled_in_are_the_files_the_blocks_read() {
    assert!(MARKDOWNLINT.contains("\"MD013\": false"), "{MARKDOWNLINT}");
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
        stylelint: STYLELINT,
        html_validate: HTML_VALIDATE,
        ruff: RUFF,
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
fn only_the_first_file_at_the_root_is_read_and_every_other_one_is_named() {
    let text = rules(&[
        (
            ".markdownlint-cli2.jsonc",
            "{ \"$schema\": \"x\", \"config\": { \"$schema\": \"y\", \"MD041\": false } }",
        ),
        (".markdownlint.yaml", "MD001: false\n"),
        ("docs/.markdownlint.yml", "MD033: false\n"),
        ("lib/.markdownlint.cjs", "module.exports = {};\n"),
        (".markdownlint-cli2.mjs", "export default {};\n"),
    ]);

    let files: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("      ") && !line.starts_with("          "))
        .map(|line| line.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(
        files,
        [
            ".markdownlint-cli2.jsonc",
            ".markdownlint-cli2.mjs",
            ".markdownlint.yaml",
            "docs/.markdownlint.yml",
            "lib/.markdownlint.cjs",
        ],
        "{text}"
    );
    assert!(!text.contains("$schema"), "{text}");
    assert!(!text.contains("MD001"), "{text}");
    assert!(!text.contains("MD033"), "{text}");
    assert_eq!(after(&text, "MD041"), [format!("false ({MD_UNNAMED})")]);
    let javascript = after(&text, "lib/.markdownlint.cjs");
    assert!(
        javascript[0].starts_with(
            "not read: the tool reads the configuration of markdownlint only as data, from one \
             file at the root of the repository (`.markdownlint-cli2.jsonc`, "
        ),
        "{text}"
    );
    assert!(javascript[0].ends_with("and never runs this one"), "{text}");
    assert_eq!(after(&text, ".markdownlint-cli2.mjs"), javascript);
    assert_eq!(
        after(&text, ".markdownlint.yaml"),
        [
            "not read: the tool reads the configuration of markdownlint from one file at the root \
          of the repository, and that is `.markdownlint-cli2.jsonc`"
        ]
    );
    let below = after(&text, "docs/.markdownlint.yml");
    assert!(
        below[0].ends_with(
            "and from no folder below it; a difference for a folder belongs in that file, as a \
             pattern"
        ),
        "{text}"
    );
}

#[test]
fn a_difference_by_pattern_is_shown_under_the_file() {
    let text = rules(&[(
        ".markdownlint-cli2.yaml",
        "config:\n  MD033: false\n\
         overrides:\n\
         \x20 - filter: [\".github/**\", \"!.github/x.md\"]\n\
         \x20   combine: merge\n\
         \x20   config: { MD041: false }\n\
         \x20 - filter: [\"docs/**\"]\n\
         \x20   combine: replace\n\
         \x20   config: {}\n",
    )]);

    assert_eq!(
        after(&text, ".markdownlint-cli2.yaml"),
        [
            "laid over the default rule by rule",
            "for `.github/**`, `!.github/x.md`: laid over the default and the rules above, rule \
             by rule",
            "for `docs/**`: laid over the default rule by rule, in place of the rules above; it \
             names no rule",
        ],
        "{text}"
    );
    assert_eq!(after(&text, "MD033"), [format!("false ({MD_UNNAMED})")]);
    assert_eq!(after(&text, "MD041"), [format!("false ({MD_UNNAMED})")]);
}

#[test]
fn a_key_that_is_not_handed_over_is_named_at_its_file() {
    let text = rules(&[(
        ".markdownlint-cli2.jsonc",
        "{ \"customRules\": [\"./rule.mjs\"], \"config\": { \"my-rule\": true } }",
    )]);

    assert_eq!(
        after(&text, ".markdownlint-cli2.jsonc"),
        [
            "laid over the default rule by rule; it names no rule",
            "`customRules` is not handed to markdownlint: it names modules of rules, which would \
             be loaded and run, and the Markdown check runs no code of the repository",
            "`config.my-rule` is no rule, alias or tag of markdownlint 0.41.1, so it is not \
             handed to markdownlint",
        ],
        "{text}"
    );
}

#[test]
fn a_register_without_markdownlint_reads_nothing_for_it() {
    let analysis = analysis(QUIET, &[".nebensound/repo.toml", ".markdownlint.json"]);
    let read = |_: &RelativePath| Ok("{ \"MD041\": false }".to_owned());
    let mut register = register();
    register
        .program
        .retain(|name, _| name.as_str() != "markdownlint");

    let text = report(&analysis, &read, Defaults::BUILT_IN, &register);

    assert!(
        text.contains("Markdownlint: the default, nothing laid over it"),
        "{text}"
    );
    assert!(!text.contains("MD041"), "{text}");
}

#[test]
fn a_markdownlint_rule_is_found_by_any_of_its_names_and_tags() {
    let text = rules(&[(
        ".markdownlint.jsonc",
        "{ \"line-length\": true, \"md013\": false, \"line_length\": { \"line_length\": 100 }, \
           \"headings\": false, \"default\": true }",
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
}

#[test]
fn a_markdownlint_tag_over_rules_the_default_treats_apart_says_so() {
    let defaults = Defaults {
        markdownlint: "{ \"MD001\": false, \"heading-style\": false, \"whitespace\": false }",
        stylelint: STYLELINT,
        html_validate: HTML_VALIDATE,
        ruff: RUFF,
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
        stylelint: STYLELINT,
        html_validate: HTML_VALIDATE,
        ruff: RUFF,
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
fn the_rules_of_markdownlint_are_read_in_the_order_written() {
    // In the default the rule comes after its tag, so the rule counts, as
    // markdownlint 0.41.1 resolves it. Read in sorted order, the tag would
    // come last and give `MD025` the value `false`.
    let defaults = Defaults {
        markdownlint: "{ \"headings\": false, \"MD025\": true }",
        stylelint: STYLELINT,
        html_validate: HTML_VALIDATE,
        ruff: RUFF,
    };

    let text = rules_with(
        defaults,
        &[(
            ".markdownlint.yaml",
            "single-title: true\nheadings: false\nMD001: false\n",
        )],
    );

    assert_eq!(
        after(&text, "single-title"),
        ["true (the same as the default)"]
    );
    assert_eq!(after(&text, "MD001"), ["false (the same as the default)"]);
    // The report shows the file's rules in the order they count in.
    let names: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("          "))
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert_eq!(names, ["single-title", "headings", "MD001"], "{text}");
}

#[test]
fn a_file_that_names_no_rule_says_so() {
    let text = rules(&[(".markdownlint-cli2.jsonc", "{ \"globs\": [] }")]);

    assert_eq!(
        after(&text, ".markdownlint-cli2.jsonc"),
        ["laid over the default rule by rule; it names no rule"]
    );
}

#[test]
fn a_file_that_cannot_be_read_says_why() {
    for (file, source, why) in [
        (
            ".markdownlint.jsonc",
            "{ \"MD001\": ",
            "cannot be read, so nothing of it is handed to markdownlint: ",
        ),
        (
            ".markdownlint.jsonc",
            "[1]",
            "holds no mapping, so nothing of it is handed to markdownlint",
        ),
        (
            ".markdownlint-cli2.jsonc",
            "{ \"config\": [1] }",
            "`config` is not a mapping of rules, so it is not handed to markdownlint",
        ),
        (
            ".markdownlint.jsonc",
            "{ \"extends\": \"base.json\" }",
            "`extends` is not handed to markdownlint: it names another file of configuration, \
             and the tool reads one file",
        ),
        (
            ".markdownlint.yaml",
            "a: [\n",
            "cannot be read, so nothing of it is handed to markdownlint: ",
        ),
    ] {
        let text = rules(&[(file, source)]);

        let shown = after(&text, file);
        assert!(
            shown.iter().any(|line| line.starts_with(why)),
            "{file}: {text}"
        );
    }
}

#[test]
fn an_empty_file_of_yaml_lays_nothing_over_the_default() {
    let text = rules(&[(".markdownlint.yaml", "")]);

    assert_eq!(
        after(&text, ".markdownlint.yaml"),
        ["laid over the default rule by rule; it names no rule"]
    );
}

#[test]
fn a_file_the_tree_lists_and_the_disk_does_not_hold_says_so() {
    let analysis = analysis(
        QUIET,
        &[".nebensound/repo.toml", ".markdownlint.yml", "README.md"],
    );
    let read = |_: &RelativePath| Err("gone".to_owned());

    let text = report(&analysis, &read, Defaults::BUILT_IN, &register());

    assert_eq!(
        after(&text, ".markdownlint.yml"),
        ["cannot be read, so nothing of it is handed to markdownlint: gone"]
    );
}

#[test]
fn every_kind_of_yaml_value_is_shown_as_the_json_it_stands_for() {
    let text = web(&[(
        ".stylelintrc.yml",
        "rules:\n\
         \x20 alpha-value-notation: 3\n\
         \x20 block-no-empty: 1.5\n\
         \x20 color-named: .inf\n\
         \x20 color-no-hex: ~\n\
         \x20 comment-no-empty: true\n\
         \x20 font-weight-notation: [x, 2]\n\
         \x20 hue-degree-notation: { 1: x }\n",
    )]);

    assert_eq!(
        after(&text, "alpha-value-notation"),
        [format!("3 ({CSS_UNNAMED})")]
    );
    assert_eq!(
        after(&text, "block-no-empty"),
        [format!("1.5 ({CSS_UNNAMED})")]
    );
    assert_eq!(
        after(&text, "color-named"),
        [format!("\".inf\" ({CSS_UNNAMED})")]
    );
    assert_eq!(
        after(&text, "color-no-hex"),
        [format!("null ({CSS_UNNAMED})")]
    );
    assert_eq!(
        after(&text, "comment-no-empty"),
        [format!("true ({CSS_UNNAMED})")]
    );
    assert_eq!(
        after(&text, "font-weight-notation"),
        [format!("[\"x\",2] ({CSS_UNNAMED})")]
    );
    assert_eq!(
        after(&text, "hue-degree-notation"),
        [format!("{{\"1\":\"x\"}} ({CSS_UNNAMED})")]
    );
}

#[test]
fn a_name_that_could_forge_a_line_is_escaped() {
    let text = rules(&[(".markdownlint.json", "{\"MD\\u001b[2J\": \"\\nfake\"}")]);

    assert!(!text.contains('\u{1b}'), "{text}");
    assert_eq!(text.lines().count(), 5, "{text}");
}

#[test]
fn a_default_that_does_not_read_is_named() {
    let broken = Defaults {
        markdownlint: "{",
        stylelint: STYLELINT,
        html_validate: HTML_VALIDATE,
        ruff: RUFF,
    };

    let text = rules_with(broken, &[]);

    let lines: Vec<&str> = text.lines().skip(2).collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert!(
        lines[0].ends_with("Markdownlint: the pipeline's default does not read: Unterminated object on line 1 column 2"),
        "{text}"
    );
}

#[test]
fn a_default_of_markdownlint_that_holds_no_rules_is_named() {
    for (default, why) in [
        ("[1]", "it holds no mapping"),
        ("{ \"extends\": 1 }", "`extends` is not text"),
    ] {
        let broken = Defaults {
            markdownlint: default,
            stylelint: STYLELINT,
            html_validate: HTML_VALIDATE,
            ruff: RUFF,
        };

        let text = rules_with(broken, &[]);

        assert!(
            text.contains(&format!(
                "Markdownlint: the pipeline's default does not read: {why}"
            )),
            "{text}"
        );
    }
}

#[test]
fn a_default_of_markdownlint_that_names_what_it_extends_still_gives_its_rules() {
    let defaults = Defaults {
        markdownlint: "{ \"extends\": \"base.json\", \"MD001\": false }",
        stylelint: STYLELINT,
        html_validate: HTML_VALIDATE,
        ruff: RUFF,
    };

    let text = rules_with(defaults, &[(".markdownlint.jsonc", "{ \"MD001\": true }")]);

    assert_eq!(after(&text, "MD001"), ["true (over the default false)"]);
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
fn a_file_an_exclusion_takes_out_is_not_named_and_the_one_at_the_root_is_read() {
    let declaration = "[repo]\naudience = \"public\"\n\
                       pipeline = \"none\"\n\
                       \n\
                       [[exclude]]\n\
                       path   = \"vendor\"\n\
                       reason = \"a vendored copy, kept as it came\"\n\
                       \n\
                       [[exclude]]\n\
                       path   = \".markdownlint.json\"\n\
                       reason = \"kept out of the json check\"\n";
    let files = [
        (
            "vendor/.markdownlint-cli2.jsonc",
            "{ \"config\": { \"MD001\": false } }",
        ),
        ("vendor/sub/.markdownlint.mjs", "export default {};"),
        (".markdownlint.json", "{ \"MD041\": false }"),
    ];

    let text = report_for(
        declaration,
        &[".nebensound/repo.toml"],
        &files,
        Defaults::BUILT_IN,
    );

    for gone in ["vendor/", "MD001"] {
        assert!(!text.contains(gone), "{gone}: {text}");
    }
    assert_eq!(after(&text, "MD041"), [format!("false ({MD_UNNAMED})")]);
}

#[test]
fn markdownlint_reads_yaml_as_1_2() {
    let text = rules(&[(".markdownlint.yml", "MD013: no\n")]);

    assert_eq!(after(&text, "MD013"), ["\"no\" (over the default false)"]);
}

/// A declaration whose repository runs the checks for stylesheets and pages.
const WEB: &str = "[repo]\naudience = \"public\"\npipeline = { check = [\"css\", \"html\"] }\n";

/// The report for a repository that runs the checks for stylesheets and
/// pages, with a stylesheet and a page at the root, in `web/` and in
/// `web/admin/`, held against these defaults.
fn web_with(defaults: Defaults<'_>, files: &[(&str, &str)]) -> String {
    let paths = [
        ".nebensound/repo.toml",
        "README.md",
        "site.css",
        "index.html",
        "web/theme.scss",
        "web/page.html",
        "web/admin/admin.css",
        "web/admin/index.htm",
    ];
    report_for(WEB, &paths, files, defaults)
}

fn web(files: &[(&str, &str)]) -> String {
    web_with(Defaults::BUILT_IN, files)
}

/// What the report says of a rule the pipeline's file for Stylelint does not
/// name.
const CSS_UNNAMED: &str = "the pipeline's file does not name it; what its `extends: stylelint-config-standard` sets holds";

#[test]
fn stylesheets_and_pages_show_their_defaults_only_where_their_check_covers_a_file() {
    let text = web(&[]);
    assert_eq!(
        text.lines().skip(3).collect::<Vec<_>>(),
        [
            "  css                                                   Stylelint: the default, \
             nothing laid over it",
            "  html                                                  html-validate: the default, \
             nothing laid over it",
        ]
    );
    assert!(
        STYLELINT.contains("stylelint-config-standard-scss"),
        "{STYLELINT}"
    );
    assert!(
        HTML_VALIDATE.contains("html-validate:a11y"),
        "{HTML_VALIDATE}"
    );

    // Without a stylesheet or a page, neither is shown.
    assert_eq!(rules(&[]).lines().count(), 3);
}

#[test]
fn stylelint_reads_the_one_file_at_the_root_and_names_every_other() {
    let text = web(&[
        (
            ".stylelintrc.json",
            "{ \"rules\": { \"color-hex-length\": \"long\" },\n\
             \"overrides\": [{ \"files\": \"web/**\", \"rules\": { \"color-named\": null } },\n\
             { \"files\": [\"*.scss\", \"admin/*\"], \"name\": \"none\" }] }\n",
        ),
        (".config/stylelintrc.yml", "rules:\n  color-named: never\n"),
        (
            "web/.stylelintrc.yaml",
            "extends: stylelint-config-recess-order\n",
        ),
        ("web/stylelint.config.mjs", "export default {};\n"),
    ]);

    assert!(
        text.contains("Stylelint: the default, with these laid over it"),
        "{text}"
    );
    assert_eq!(
        after(&text, ".stylelintrc.json"),
        [
            "laid over the default rule by rule",
            "for `web/**`: laid over the default and the rules above, rule by rule",
            "for `*.scss`, `admin/*`: laid over the default and the rules above, rule by rule; it \
             names no rule",
        ]
    );
    assert_eq!(
        after(&text, "color-hex-length"),
        [format!("\"long\" ({CSS_UNNAMED})")]
    );
    assert_eq!(
        after(&text, "color-named"),
        [format!("null ({CSS_UNNAMED})")]
    );
    // Every other file of Stylelint's configuration is named, and nothing of
    // it is read.
    assert_eq!(after(&text, ".config/stylelintrc.yml").len(), 1, "{text}");
    assert!(
        after(&text, ".config/stylelintrc.yml")[0].ends_with(
            "from one file at the root of the repository, and that is `.stylelintrc.json`"
        ),
        "{text}"
    );
    assert_eq!(after(&text, "web/.stylelintrc.yaml").len(), 1, "{text}");
    assert!(
        after(&text, "web/.stylelintrc.yaml")[0]
            .ends_with("a difference for a folder belongs in that file, as a pattern"),
        "{text}"
    );
    assert_eq!(after(&text, "web/stylelint.config.mjs").len(), 1, "{text}");
    assert!(
        after(&text, "web/stylelint.config.mjs")[0].ends_with("and never runs this one"),
        "{text}"
    );
    assert!(!text.contains("recess-order"), "{text}");
}

#[test]
fn stylelint_names_every_key_it_does_not_hand_over() {
    let text = web(&[(
        ".stylelintrc.yaml",
        "extends: stylelint-config-recess-order\nplugins: [./plugin.mjs]\nrules:\n  my-rule: true\n",
    )]);

    assert_eq!(
        after(&text, ".stylelintrc.yaml"),
        [
            "laid over the default rule by rule; it names no rule",
            "`extends` is not handed to Stylelint: it names further configuration, a module or \
             another file, which would be loaded, and the tool reads one file",
            "`plugins` is not handed to Stylelint: it names modules of rules, which would be \
             loaded and run, and the CSS check runs no code of the repository",
            "`rules.my-rule` is no rule of Stylelint 18.0.0 or of its plugin for SCSS, so it is \
             not handed to Stylelint",
        ]
    );
}

#[test]
fn html_validate_reads_every_configuration_above_a_page() {
    let text = web(&[
        (
            ".htmlvalidate.json",
            "{ \"extends\": [\"html-validate:standard\"], \"rules\": { \"no-inline-style\": \"off\" } }\n",
        ),
        (".htmlvalidate.cjs", "module.exports = {};\n"),
        ("web/admin/.htmlvalidate.json", "{ \"root\": true }\n"),
        (
            "docs/.htmlvalidate.json",
            "{ \"rules\": { \"a\": \"off\" } }\n",
        ),
    ]);

    assert!(
        text.contains("html-validate: the default, with these laid over it"),
        "{text}"
    );
    assert_eq!(
        after(&text, ".htmlvalidate.json"),
        ["laid over the default rule by rule"]
    );
    assert_eq!(
        after(&text, "extends"),
        ["html-validate:standard: what it sets lies beneath the rules here"]
    );
    assert_eq!(
        after(&text, "no-inline-style"),
        [
            "\"off\" (the pipeline's file does not name it; what its `extends: \
             html-validate:recommended, html-validate:a11y, html-validate:prettier` sets holds)"
        ]
    );
    assert_eq!(
        after(&text, ".htmlvalidate.cjs"),
        ["written in JavaScript, which phase 0 does not run"]
    );
    assert_eq!(
        after(&text, "web/admin/.htmlvalidate.json"),
        [
            "laid over the default rule by rule, in place of any configuration above it; \
             it names no rule"
        ]
    );
    assert!(!text.contains("docs/"), "{text}");
}

#[test]
fn html_validate_reads_nothing_above_a_configuration_that_says_root() {
    let paths = [
        ".nebensound/repo.toml",
        "site/index.html",
        "site/admin/index.html",
    ];
    let text = report_for(
        WEB,
        &paths,
        &[
            (".htmlvalidate.json", "{ \"rules\": { \"a\": \"off\" } }\n"),
            ("site/.htmlvalidate.json", "{ \"root\": true }\n"),
            (
                "site/admin/.htmlvalidate.json",
                "{ \"root\": false, \"rules\": { \"b\": \"off\" } }\n",
            ),
        ],
        Defaults::BUILT_IN,
    );

    assert_eq!(
        after(&text, "site/.htmlvalidate.json"),
        [
            "laid over the default rule by rule, in place of any configuration above it; \
             it names no rule"
        ]
    );
    assert_eq!(
        after(&text, "site/admin/.htmlvalidate.json"),
        ["laid over the default and what lies above it, rule by rule"]
    );
    assert!(after(&text, ".htmlvalidate.json").is_empty(), "{text}");
}

#[test]
fn a_rule_the_default_of_stylelint_or_html_validate_names_is_compared() {
    let defaults = Defaults {
        markdownlint: MARKDOWNLINT,
        stylelint: "rules:\n  color-named: never\n",
        html_validate: "{ \"extends\": \"html-validate:standard\", \"rules\": { \"void-style\": \"off\" } }",
        ruff: RUFF,
    };
    let text = web_with(
        defaults,
        &[
            (
                ".stylelintrc.yaml",
                "rules:\n  color-named: never\n  color-no-hex: true\n",
            ),
            (
                ".htmlvalidate.json",
                "{ \"rules\": { \"void-style\": \"error\" } }\n",
            ),
        ],
    );

    assert_eq!(
        after(&text, "color-named"),
        ["\"never\" (the same as the default)"]
    );
    assert_eq!(
        after(&text, "color-no-hex"),
        ["true (the pipeline's file does not name it, so it is off)"]
    );
    assert_eq!(
        after(&text, "void-style"),
        ["\"error\" (over the default \"off\")"]
    );
}

#[test]
fn a_configuration_of_stylelint_or_html_validate_that_does_not_read_says_why() {
    for (file, source, why) in [
        (
            ".stylelintrc.json",
            "[1]",
            "holds no mapping, so nothing of it is handed to Stylelint",
        ),
        (
            ".stylelintrc.json",
            "{ \"rules\": [] }",
            "`rules` is not a mapping of rules, so it is not handed to Stylelint",
        ),
        (
            ".stylelintrc.json",
            "{ // a comment\n}",
            "cannot be read, so nothing of it is handed to Stylelint: ",
        ),
        (
            ".htmlvalidate.json",
            "{ \"extends\": 3 }",
            "cannot be read: `extends` is neither text nor a list of text",
        ),
        (
            ".htmlvalidate.json",
            "{ \"extends\": [3] }",
            "cannot be read: `extends` holds an entry that is not text",
        ),
        (
            ".htmlvalidate.json",
            "{ \"rules\": [] }",
            "cannot be read: `rules` is not a mapping",
        ),
        (
            ".htmlvalidate.json",
            "[1]",
            "cannot be read: it holds no mapping",
        ),
        (".htmlvalidate.json", "{", "cannot be read: "),
    ] {
        let text = web(&[(file, source)]);
        let shown = after(&text, file);
        assert!(
            shown.last().is_some_and(|last| last.starts_with(why)),
            "{source}: {text}"
        );
    }

    let broken = Defaults {
        markdownlint: MARKDOWNLINT,
        stylelint: "a: [\n",
        html_validate: "{",
        ruff: RUFF,
    };
    let text = web_with(broken, &[]);
    assert!(
        text.contains("Stylelint: the pipeline's default does not read: "),
        "{text}"
    );
    assert!(
        text.contains("html-validate: the pipeline's default does not read: "),
        "{text}"
    );
}

#[test]
fn a_configuration_of_stylelint_or_html_validate_the_disk_does_not_hold_says_so() {
    let text = report_for(
        WEB,
        &[
            ".nebensound/repo.toml",
            "site.css",
            "index.html",
            ".stylelintrc.yml",
            ".htmlvalidate.json",
        ],
        &[],
        Defaults::BUILT_IN,
    );

    assert_eq!(
        after(&text, ".stylelintrc.yml"),
        ["cannot be read, so nothing of it is handed to Stylelint: no such file"]
    );
    assert_eq!(
        after(&text, ".htmlvalidate.json"),
        ["cannot be read: no such file"]
    );
}

/// A declaration whose repository runs the check for Python, with a unit at
/// `app` that names it again and one at `tools` that names it alone.
const PYTHON: &str = "[repo]\naudience = \"public\"\npipeline = { check = [\"python\"] }\n\
                      \n\
                      [component.app]\npath     = \"app\"\npipeline = { check = [\"python\"] }\n";

/// The report for a repository that runs the check for Python, with a module
/// at the root and in `app/`, held against these defaults.
fn python_with(defaults: Defaults<'_>, files: &[(&str, &str)]) -> String {
    let paths = [
        ".nebensound/repo.toml",
        "README.md",
        "main.py",
        "app/run.py",
    ];
    report_for(PYTHON, &paths, files, defaults)
}

fn python(files: &[(&str, &str)]) -> String {
    python_with(Defaults::BUILT_IN, files)
}

/// What the report says of an entry the default's `ignore` does not name.
const NOT_IGNORED: &str = "the default's `ignore` does not name it; its `select` is [\"ALL\"]";

#[test]
fn without_a_configuration_of_its_own_ruff_runs_with_the_default_alone() {
    let text = python(&[]);

    assert_eq!(
        text.lines().skip(3).collect::<Vec<_>>(),
        [
            "  python                                                Ruff: the default, \
          nothing laid over it"
        ]
    );
    assert!(RUFF.contains("select = [\"ALL\"]"), "{RUFF}");
    // Without a Python file the check covers, Ruff is not shown.
    assert_eq!(rules(&[]).lines().count(), 3);
}

#[test]
fn a_rule_switched_off_names_the_rule_and_the_file() {
    let text = python(&[(
        "pyproject.toml",
        "[project]\nname = \"thing\"\n\n[tool.ruff]\nline-length = 100\n\n\
         [tool.ruff.lint]\nignore = [\"D100\", \"COM812\"]\n\
         extend-select = [\"CPY001\", \"ALL\", \"E\"]\n",
    )]);

    assert!(
        text.contains("Ruff: the default, with these laid over it"),
        "{text}"
    );
    assert_eq!(
        after(&text, "pyproject.toml"),
        ["laid over the default rule by rule"]
    );
    assert_eq!(
        after(&text, "D100"),
        [format!("off by `lint.ignore` ({NOT_IGNORED})")]
    );
    assert_eq!(
        after(&text, "COM812"),
        ["off by `lint.ignore` (the same as the default: its `ignore` names it)"]
    );
    assert_eq!(
        after(&text, "CPY001"),
        ["on by `lint.extend-select` (over the default, whose `ignore` names it)"]
    );
    // `ALL` and `E` switch back on what the default's `ignore` names under them.
    assert_eq!(
        after(&text, "ALL"),
        [
            "on by `lint.extend-select` (over the default, whose `ignore` names \
             [\"W191\",\"E111\",\"E114\",\"E117\",\"D206\",\"D300\",\"Q000\",\"Q001\",\"Q002\",\
             \"Q003\",\"COM812\",\"COM819\",\"ISC002\",\"D203\",\"D213\",\"CPY001\"] under it)"
        ]
    );
    assert_eq!(
        after(&text, "E"),
        [
            "on by `lint.extend-select` (over the default, whose `ignore` names \
             [\"E111\",\"E114\",\"E117\"] under it)"
        ]
    );
}

#[test]
fn an_extend_select_prefix_covers_only_its_own_linter() {
    let defaults = Defaults {
        ruff: "[lint]\nselect = [\"ALL\"]\n\
               ignore = [\"E501\", \"ERA001\", \"PLR2004\", \"PLW0603\", \"PTH123\"]\n",
        ..Defaults::BUILT_IN
    };
    let text = python_with(
        defaults,
        &[(
            "ruff.toml",
            "[lint]\nextend-select = [\"E\", \"PL\", \"PLR\", \"PT\", \"PLR2\"]\n",
        )],
    );

    // `E` is pycodestyle, not eradicate's `ERA`.
    assert_eq!(
        after(&text, "E"),
        ["on by `lint.extend-select` (over the default, whose `ignore` names [\"E501\"] under it)"]
    );
    // `PL` holds pylint's categories, `PLR` only its own.
    assert_eq!(
        after(&text, "PL"),
        [
            "on by `lint.extend-select` (over the default, whose `ignore` names \
             [\"PLR2004\",\"PLW0603\"] under it)"
        ]
    );
    assert_eq!(
        after(&text, "PLR2"),
        [
            "on by `lint.extend-select` (over the default, whose `ignore` names \
             [\"PLR2004\"] under it)"
        ]
    );
    // `PT` is flake8-pytest-style, not `PTH`.
    assert_eq!(
        after(&text, "PT"),
        [
            "on by `lint.extend-select` (the default names it in neither `select` nor \
             `ignore`; its `select` is [\"ALL\"])"
        ]
    );
}

#[test]
fn a_select_and_per_file_ignores_start_over() {
    let text = python(&[(
        "ruff.toml",
        "[lint]\nselect = [\"E\", \"F\"]\n\n[lint.per-file-ignores]\n\
         \"**/tests/**\" = [\"S101\", \"PLR2004\"]\n\"scripts/*\" = [\"T201\"]\n\n\
         [lint.extend-per-file-ignores]\n\"**/conftest.py\" = [\"ANN\"]\n",
    )]);

    assert_eq!(
        after(&text, "lint.select"),
        [
            "[\"E\",\"F\"] (starts over: the `select` and `ignore` beneath it no longer hold, \
             the default's `select` [\"ALL\"] included)"
        ]
    );
    assert_eq!(
        after(&text, "lint.per-file-ignores"),
        [
            "starts over: the default's for [\"**/*_test.py\",\"**/conftest.py\",\
             \"**/test_*.py\"] no longer hold"
        ]
    );
    assert_eq!(
        after(&text, "S101"),
        ["off in \"**/tests/**\" by `lint.per-file-ignores` (the same as the default)"]
    );
    assert_eq!(
        after(&text, "PLR2004"),
        [
            "off in \"**/tests/**\" by `lint.per-file-ignores` (the default switches off \
             [\"S101\"] there)"
        ]
    );
    assert_eq!(
        after(&text, "T201"),
        ["off in \"scripts/*\" by `lint.per-file-ignores` (the default names no such pattern)"]
    );
    assert_eq!(
        after(&text, "ANN"),
        [
            "off in \"**/conftest.py\" by `lint.extend-per-file-ignores` (the default \
             switches off [\"S101\"] there)"
        ]
    );
}

#[test]
fn per_file_ignores_naming_every_pattern_of_the_default_still_start_over() {
    let text = python(&[(
        ".ruff.toml",
        "[lint.per-file-ignores]\n\"**/tests/**\" = []\n\"**/test_*.py\" = []\n\
         \"**/*_test.py\" = []\n\"**/conftest.py\" = []\n",
    )]);

    assert_eq!(
        after(&text, "lint.per-file-ignores"),
        ["starts over, in place of the default's"]
    );
}

#[test]
fn settings_at_the_top_count_where_the_lint_table_does_not_name_them() {
    let text = python(&[(
        "ruff.toml",
        "select = [\"E\"]\nextend-ignore = [\"E501\"]\nignore = [\"W\"]\n\n\
         [lint]\nignore = [\"D\"]\n",
    )]);

    assert_eq!(after(&text, "select").len(), 1, "{text}");
    assert_eq!(
        after(&text, "D"),
        [format!("off by `lint.ignore` ({NOT_IGNORED})")]
    );
    assert!(after(&text, "W").is_empty(), "{text}");
    assert_eq!(
        after(&text, "E501"),
        [format!("off by `extend-ignore` ({NOT_IGNORED})")]
    );
}

#[test]
fn the_configuration_ruff_finds_for_each_unit_is_read() {
    // `.ruff.toml` comes before `ruff.toml`; a `pyproject.toml` without a
    // `[tool.ruff]` table is passed by, and the one above it found.
    let text = python(&[
        (".ruff.toml", "[lint]\nignore = [\"D100\"]\n"),
        ("ruff.toml", "[lint]\nignore = [\"D101\"]\n"),
        ("app/pyproject.toml", "[project]\nname = \"app\"\n"),
        ("app/sub/ruff.toml", "[lint]\nignore = [\"D102\"]\n"),
    ]);

    assert_eq!(
        after(&text, ".ruff.toml"),
        ["laid over the default rule by rule"]
    );
    assert!(!after(&text, "D100").is_empty(), "{text}");
    for unread in ["D101", "D102", "app/pyproject.toml", "ruff.toml"] {
        assert!(after(&text, unread).is_empty(), "{unread}: {text}");
    }
}

#[test]
fn a_unit_inside_another_that_runs_the_check_reads_no_configuration_of_its_own() {
    let text = python(&[("app/ruff.toml", "[lint]\nignore = [\"D100\"]\n")]);
    assert!(
        text.contains("Ruff: the default, nothing laid over it"),
        "{text}"
    );

    // A unit that runs the check where the repository does not reads its own,
    // and two units that find the same file show it once.
    let declaration = "[repo]\naudience = \"public\"\npipeline = \"none\"\n\
                       \n\
                       [component.app]\npath     = \"app\"\npipeline = { check = [\"python\"] }\n\
                       \n\
                       [component.cli]\npath     = \"app/cli\"\npipeline = { check = [\"python\"] }\n\
                       \n\
                       [component.web]\npath     = \"web\"\npipeline = { check = [\"python\"] }\n";
    let text = report_for(
        declaration,
        &[
            ".nebensound/repo.toml",
            "app/run.py",
            "app/cli/main.py",
            "web/site.py",
        ],
        &[
            ("app/ruff.toml", "[lint]\nignore = [\"D100\"]\n"),
            (
                "pyproject.toml",
                "[tool.ruff]\nextend-select = [\"D213\"]\n",
            ),
        ],
        Defaults::BUILT_IN,
    );
    assert_eq!(after(&text, "app/ruff.toml").len(), 1, "{text}");
    assert_eq!(after(&text, "pyproject.toml").len(), 1, "{text}");
    assert_eq!(after(&text, "D213").len(), 1, "{text}");
}

#[test]
fn a_ruff_configuration_that_names_no_rule_says_so() {
    let text = python(&[("ruff.toml", "line-length = 100\n")]);

    assert_eq!(
        after(&text, "ruff.toml"),
        ["laid over the default rule by rule; it names no rule"]
    );
}

#[test]
fn what_extend_names_lies_beneath_it_over_the_default() {
    let text = python(&[
        (
            "app/ruff.toml",
            "extend = \"../config/./base.toml\"\n[lint]\nignore = [\"D100\"]\n",
        ),
        ("config/base.toml", "extend = \"pyproject.toml\"\n"),
        ("config/pyproject.toml", "[project]\nname = \"config\"\n"),
        (
            "pyproject.toml",
            "[tool.ruff]\nextend = \"app/ruff.toml\"\n",
        ),
    ]);

    let lines: Vec<&str> = text.lines().skip(4).map(str::trim).collect();
    let squeezed: Vec<String> = lines
        .iter()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    assert_eq!(
        squeezed,
        [
            "pyproject.toml laid over the default rule by rule",
            "extend \"app/ruff.toml\": what it names lies beneath the rules here",
            "app/ruff.toml laid over the default beneath pyproject.toml, whose `extend` names it",
            format!("D100 off by `lint.ignore` ({NOT_IGNORED})").as_str(),
            "extend \"../config/./base.toml\": what it names lies beneath the rules here",
            "config/base.toml laid over the default beneath app/ruff.toml, whose `extend` names it",
            "extend \"pyproject.toml\": what it names lies beneath the rules here",
            "config/pyproject.toml laid over the default beneath config/base.toml, whose \
             `extend` names it; it names no rule",
        ],
        "{text}"
    );
}

#[test]
fn an_extend_that_cannot_be_followed_says_why() {
    for (extend, why) in [
        (
            "~/ruff.toml",
            "lies outside the repository: named, not read",
        ),
        (
            "$HOME/ruff.toml",
            "lies outside the repository: named, not read",
        ),
        (
            "/etc/ruff.toml",
            "lies outside the repository: named, not read",
        ),
        (
            "../ruff.toml",
            "lies outside the repository: named, not read",
        ),
        (
            "ruff.toml",
            "comes back round to a file above it, which Ruff refuses",
        ),
    ] {
        let text = python(&[("ruff.toml", &format!("extend = {extend:?}\n"))]);
        assert_eq!(
            after(&text, "extend"),
            [format!("{extend:?}: {why}")],
            "{text}"
        );
    }

    let text = python(&[
        ("ruff.toml", "extend = \"base.toml\"\n"),
        ("base.toml", "extend = \"ruff.toml\"\n"),
    ]);
    assert_eq!(
        after(&text, "extend").last().map(String::as_str),
        Some("\"ruff.toml\": comes back round to a file above it, which Ruff refuses"),
        "{text}"
    );

    let text = python(&[("ruff.toml", "extend = \"missing.toml\"\n")]);
    assert_eq!(
        after(&text, "missing.toml"),
        ["cannot be read: no such file"],
        "{text}"
    );
}

#[test]
fn a_configuration_of_ruff_that_does_not_read_says_why() {
    for (file, source, why) in [
        ("ruff.toml", "a = [", "cannot be read: "),
        (
            "ruff.toml",
            "extend = 3\n",
            "cannot be read: `extend` is not text",
        ),
        (
            "ruff.toml",
            "lint = 3\n",
            "cannot be read: `lint` is not a table",
        ),
        (
            "ruff.toml",
            "[lint]\nselect = \"E\"\n",
            "cannot be read: `lint.select` is not a list of text",
        ),
        (
            "ruff.toml",
            "ignore = [3]\n",
            "cannot be read: `ignore` is not a list of text",
        ),
        (
            "ruff.toml",
            "[lint]\nper-file-ignores = 3\n",
            "cannot be read: `lint.per-file-ignores` is not a table",
        ),
        (
            "ruff.toml",
            "[lint.per-file-ignores]\n\"a.py\" = \"E\"\n",
            "cannot be read: `lint.per-file-ignores.a.py` is not a list of text",
        ),
        (
            "pyproject.toml",
            "[tool]\nruff = 3\n",
            "cannot be read: `tool.ruff` is not a table",
        ),
        ("pyproject.toml", "[tool", "cannot be read: "),
    ] {
        let text = python(&[(file, source)]);
        let shown = after(&text, file);
        assert_eq!(shown.len(), 1, "{source}: {text}");
        assert!(shown[0].starts_with(why), "{source}: {text}");
    }

    // A `pyproject.toml` whose `tool` is no table, or holds no `ruff`, holds
    // no Ruff settings.
    for source in ["tool = 3\n", "[tool.black]\nline-length = 100\n"] {
        let text = python(&[("pyproject.toml", source)]);
        assert!(
            text.contains("Ruff: the default, nothing laid over it"),
            "{text}"
        );
    }

    for ruff in ["a = [", "extend = 3\n"] {
        let broken = Defaults {
            ruff,
            ..Defaults::BUILT_IN
        };
        let text = python_with(broken, &[]);
        assert!(
            text.contains("Ruff: the pipeline's default does not read: "),
            "{text}"
        );
    }
}

#[test]
fn a_default_that_names_rules_in_every_setting_is_read_in_full() {
    let defaults = Defaults {
        ruff: "extend-select = [\"D213\"]\nextend-ignore = [\"E501\"]\n\
               [extend-per-file-ignores]\n\"a.py\" = [\"T201\"]\n",
        ..Defaults::BUILT_IN
    };
    let text = python_with(
        defaults,
        &[(
            "ruff.toml",
            "[lint]\nextend-select = [\"D213\", \"E501\"]\n\
             [lint.extend-per-file-ignores]\n\"a.py\" = [\"T201\"]\n",
        )],
    );

    assert_eq!(
        after(&text, "D213"),
        ["on by `lint.extend-select` (the same as the default: its `select` names it)"]
    );
    assert_eq!(
        after(&text, "E501"),
        ["on by `lint.extend-select` (over the default, whose `ignore` names it)"]
    );
    assert_eq!(
        after(&text, "T201"),
        ["off in \"a.py\" by `lint.extend-per-file-ignores` (the same as the default)"]
    );
}
