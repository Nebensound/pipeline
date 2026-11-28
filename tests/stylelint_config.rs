//! Stylelint's configuration, read by the tool from the repository's own
//! file: which keys are carried over, set aside, refused or unknown, how the
//! configuration of each file is worked out over the pipeline's default, and
//! which files are named as not read. How the CSS check hands it to Stylelint
//! is tested in `run.rs`, how the rules report shows it in `rules.rs`; that
//! the real Stylelint lints with it, and loads nothing of the repository, the
//! cases of the block show.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use nebensound_repo::Register;
use nebensound_repo::linter_config::stylelint::{
    CARRIED, Handed, Layer, NEUTRAL, Pinned, REFUSED, RULES, SCSS_PLUGIN_BROUGHT_BY, SCSS_RULES,
    SET_ASIDE, WARNING, beside, is_rule, pinned, read, read_in,
};
use nebensound_repo::linter_config::{Problem, findings, locate};
use nebensound_repo::names::ProgramName;
use nebensound_repo::path::RelativePath;
use nebensound_repo::programs::Program;
use nebensound_repo::rules::STYLELINT;
use serde_json::{Value, json};
use support::analysis;

fn register() -> Register {
    Register::builtin().expect("the register")
}

fn stylelint() -> (ProgramName, Program) {
    let register = register();
    let (name, program) = register
        .program
        .iter()
        .find(|(name, _)| name.as_str() == "stylelint")
        .expect("Stylelint in the register");
    (name.clone(), program.clone())
}

fn paths(files: &[&str]) -> Vec<RelativePath> {
    files
        .iter()
        .map(|file| RelativePath::new(file).unwrap())
        .collect()
}

/// A reader of these files, from memory.
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

/// Stylelint's configuration in a repository holding these files, every one
/// of them kept.
fn configured(files: &[(&str, &str)]) -> (Handed, Vec<Problem>) {
    let all = paths(&files.iter().map(|(path, _)| *path).collect::<Vec<_>>());
    let refs: Vec<&RelativePath> = all.iter().collect();
    let (name, program) = stylelint();
    read(&refs, &|_| true, &name, &program, &reader(files))
}

fn messages(problems: &[Problem]) -> Vec<String> {
    problems
        .iter()
        .map(|problem| format!("{}: {}", problem.file, problem.message))
        .collect()
}

/// The pipeline's default compiled into the tool.
fn default() -> Pinned {
    pinned(STYLELINT).expect("the default reads")
}

/// The rules a file is handed, as one JSON object in their order.
fn rules(layer: &Layer) -> String {
    let entries: Vec<String> = layer
        .rules
        .iter()
        .map(|(key, value)| format!("{}:{value}", Value::from(key.as_str())))
        .collect();
    format!("{{{}}}", entries.join(","))
}

#[test]
fn every_key_of_a_configuration_is_known_once() {
    let set_aside: Vec<&str> = SET_ASIDE.iter().map(|(key, _)| *key).collect();
    let neutral: Vec<&str> = NEUTRAL.iter().map(|(key, _)| *key).collect();
    let refused: Vec<&str> = REFUSED.iter().map(|(key, _)| *key).collect();
    let lists = [
        CARRIED,
        set_aside.as_slice(),
        neutral.as_slice(),
        refused.as_slice(),
    ];
    for list in lists {
        assert!(list.windows(2).all(|pair| pair[0] < pair[1]), "{list:?}");
    }
    let mut all: Vec<&str> = lists.concat();
    all.sort_unstable();
    all.dedup();
    // The keys of a configuration of Stylelint 18.0.0 that a file of data can
    // write, and the schema's `$schema`.
    assert_eq!(all.len(), 24, "{all:?}");
}

#[test]
fn the_rules_are_those_of_the_pinned_stylelint_and_its_plugin_for_scss() {
    assert_eq!(RULES.len(), 153);
    assert_eq!(SCSS_RULES.len(), 71);
    assert!(is_rule("color-hex-length"));
    assert!(is_rule("value-type-custom-property-required-list"));
    assert!(is_rule("scss/dollar-variable-pattern"));
    // A rule of the plugin is written with its prefix, and a rule Stylelint
    // dropped is none.
    assert!(!is_rule("dollar-variable-pattern"));
    assert!(!is_rule("scss/color-hex-length"));
    assert!(!is_rule("indentation"));
}

#[test]
fn the_first_name_at_the_root_is_read_and_every_other_file_is_named() {
    let (name, program) = stylelint();
    let all = paths(&[
        ".config/stylelintrc.json",
        ".stylelintrc.cjs",
        ".stylelintrc.yaml",
        "docs/.stylelintrc.json",
        "site.css",
        "vendor/.stylelintrc",
        "web/stylelint.config.mjs",
    ]);
    let refs: Vec<&RelativePath> = all.iter().collect();
    let kept = |file: &RelativePath| !file.to_string().starts_with("vendor/");

    let located = locate(&refs, &kept, &name, &program);

    assert_eq!(
        located.file.map(ToString::to_string).as_deref(),
        Some(".stylelintrc.yaml")
    );
    let unread: Vec<String> = located
        .unread
        .iter()
        .map(|unread| format!("{}: {}", unread.file, unread.why))
        .collect();
    let names = "(`.stylelintrc`, `.stylelintrc.json`, `.stylelintrc.yaml`, `.stylelintrc.yml`)";
    assert_eq!(
        unread,
        [
            format!(
                ".config/stylelintrc.json: not read: the tool reads the configuration of \
                 stylelint only as data, from one file at the root of the repository {names}, \
                 and never runs this one"
            ),
            format!(
                ".stylelintrc.cjs: not read: the tool reads the configuration of stylelint only \
                 as data, from one file at the root of the repository {names}, and never runs \
                 this one"
            ),
            format!(
                "docs/.stylelintrc.json: not read: the tool reads the configuration of stylelint \
                 from one file at the root of the repository {names}, and from no folder below \
                 it; a difference for a folder belongs in that file, as a pattern"
            ),
            format!(
                "web/stylelint.config.mjs: not read: the tool reads the configuration of \
                 stylelint only as data, from one file at the root of the repository {names}, \
                 and never runs this one"
            ),
        ]
    );
}

#[test]
fn without_a_file_the_default_holds() {
    let (handed, problems) = configured(&[("site.css", "a {}\n")]);

    assert!(handed.file.is_none());
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        handed.job(
            &default(),
            &["site.css".to_owned(), "web/theme.scss".to_owned()]
        ),
        "{\"groups\":[\
         {\"extends\":[\"stylelint-config-standard\"],\"customSyntax\":null,\
         \"config\":{\"rules\":{}},\"files\":[\"site.css\"]},\
         {\"extends\":[\"stylelint-config-standard\",\"stylelint-config-standard-scss\"],\
         \"customSyntax\":\"postcss-scss\",\"config\":{\"rules\":{}},\
         \"files\":[\"web/theme.scss\"]}]}"
    );
}

#[test]
fn the_default_compiled_in_reads_as_the_check_hands_it_over() {
    let default = default();

    assert_eq!(default.base.extends, ["stylelint-config-standard"]);
    assert!(default.base.custom_syntax.is_none());
    assert!(default.base.rules.is_empty());
    assert_eq!(default.overrides.len(), 1);
    let (patterns, layer) = &default.overrides[0];
    assert_eq!(patterns.written, ["**/*.scss"]);
    assert!(patterns.matches("theme.scss") && patterns.matches("web/admin/theme.scss"));
    assert!(!patterns.matches("site.css"));
    assert_eq!(layer.extends, ["stylelint-config-standard-scss"]);
    assert_eq!(layer.extends, [SCSS_PLUGIN_BROUGHT_BY]);
    assert_eq!(layer.custom_syntax.as_deref(), Some("postcss-scss"));
}

#[test]
fn a_default_that_holds_anything_else_does_not_read() {
    for (text, why) in [
        ("a: [\n", ""),
        ("[1]\n", "it holds no mapping"),
        ("overrides: 3\n", "`overrides` is not a list"),
        ("overrides: [3]\n", "`overrides[0]` is not a mapping"),
        (
            "overrides:\n  - rules: {}\n",
            "`overrides[0]` has no `files`",
        ),
        (
            "overrides:\n  - files: 3\n",
            "`overrides[0].files` is neither a pattern nor a list of patterns",
        ),
        (
            "overrides:\n  - files: x\n    extends: 3\n",
            "`overrides[0].extends` does not hold what it takes",
        ),
        (
            "extends: [3]\n",
            "`extends` holds an entry that is not text",
        ),
        (
            "customSyntax: 3\n",
            "`customSyntax` does not hold what it takes",
        ),
        ("rules: 3\n", "`rules` does not hold what it takes"),
        (
            "plugins: [x]\n",
            "`plugins` is no key the tool reads in its default",
        ),
    ] {
        let error = pinned(text).expect_err(text);
        assert!(error.starts_with(why), "{text}: {error}");
    }

    let read = pinned("extends: x\ncustomSyntax: y\nrules:\n  color-named: never\n").unwrap();
    assert_eq!(
        read.base,
        Layer {
            extends: vec!["x".to_owned()],
            custom_syntax: Some("y".to_owned()),
            rules: vec![("color-named".to_owned(), json!("never"))],
        }
    );
}

#[test]
fn the_rules_and_the_options_of_the_file_are_carried_over_in_the_order_written() {
    let (handed, problems) = configured(&[(
        ".stylelintrc.json",
        "{\n\
           \"$schema\": \"https://json.schemastore.org/stylelintrc.json\",\n\
           \"reportNeedlessDisables\": true,\n\
           \"rules\": { \"selector-class-pattern\": null, \"color-hex-length\": [\"long\", { \"severity\": \"warning\" }] },\n\
           \"defaultSeverity\": \"error\",\n\
           \"ignoreDisables\": true,\n\
           \"configurationComment\": \"lint\",\n\
           \"languageOptions\": { \"syntax\": { \"properties\": { \"x\": \"<length>\" } } },\n\
           \"reportInvalidScopeDisables\": true,\n\
           \"reportDescriptionlessDisables\": [true, { \"except\": [\"a\"] }],\n\
           \"reportUnscopedDisables\": false,\n\
           \"allowEmptyInput\": false,\n\
           \"cache\": true,\n\
           \"ignoreFiles\": [\"vendor/**\"],\n\
           \"maxWarnings\": 0,\n\
           \"validate\": false\n\
         }",
    )]);

    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        handed.file.as_ref().map(ToString::to_string).as_deref(),
        Some(".stylelintrc.json")
    );
    let options: Vec<&str> = handed.options.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        options,
        [
            "reportNeedlessDisables",
            "defaultSeverity",
            "ignoreDisables",
            "configurationComment",
            "languageOptions",
            "reportInvalidScopeDisables",
            "reportDescriptionlessDisables",
            "reportUnscopedDisables",
        ]
    );
    assert_eq!(
        handed.job(&default(), &["a.css".to_owned()]),
        "{\"groups\":[{\"extends\":[\"stylelint-config-standard\"],\"customSyntax\":null,\
         \"config\":{\"reportNeedlessDisables\":true,\"defaultSeverity\":\"error\",\
         \"ignoreDisables\":true,\"configurationComment\":\"lint\",\
         \"languageOptions\":{\"syntax\":{\"properties\":{\"x\":\"<length>\"}}},\
         \"reportInvalidScopeDisables\":true,\
         \"reportDescriptionlessDisables\":[true,{\"except\":[\"a\"]}],\
         \"reportUnscopedDisables\":false,\
         \"rules\":{\"selector-class-pattern\":null,\
         \"color-hex-length\":[\"long\",{\"severity\":\"warning\"}]}},\
         \"files\":[\"a.css\"]}]}"
    );
}

#[test]
fn every_key_that_is_not_handed_over_is_named_and_the_rest_is_handed_over() {
    let refused: Vec<String> = REFUSED
        .iter()
        .map(|(key, _)| format!("  \"{key}\": \"x\",\n"))
        .collect();
    let source = format!(
        "{{\n{}  \"ignore\": [\"vendor/**\"],\n  \"rules\": {{ \"color-hex-length\": \"long\", \
         \"my-rule\": true, \"scss/at-rule-no-unknown\": true }}\n}}",
        refused.concat()
    );
    let (handed, problems) = configured(&[(".stylelintrc.json", &source)]);

    let mut expected: Vec<String> = REFUSED
        .iter()
        .map(|(key, why)| {
            format!(".stylelintrc.json: `{key}` is not handed to Stylelint: it {why}")
        })
        .collect();
    expected.extend([
        ".stylelintrc.json: `ignore` is not a setting the CSS check knows, so it is not handed to \
         Stylelint"
            .to_owned(),
        ".stylelintrc.json: `rules.my-rule` is no rule of Stylelint 18.0.0 or of its plugin for \
         SCSS, so it is not handed to Stylelint"
            .to_owned(),
    ]);
    assert_eq!(messages(&problems), expected);
    assert_eq!(
        handed.rules,
        [
            ("color-hex-length".to_owned(), json!("long")),
            ("scss/at-rule-no-unknown".to_owned(), json!(true)),
        ]
    );
    assert!(handed.options.is_empty());
}

#[test]
fn a_default_severity_of_warning_is_refused_since_it_would_fail_nothing() {
    let (handed, problems) = configured(&[(".stylelintrc.yaml", "defaultSeverity: warning\n")]);
    assert_eq!(
        messages(&problems),
        [format!(".stylelintrc.yaml: {WARNING}")]
    );
    assert!(handed.options.is_empty());

    let (handed, problems) = configured(&[(".stylelintrc.yaml", "defaultSeverity: 3\n")]);
    assert_eq!(
        messages(&problems),
        [
            ".stylelintrc.yaml: `defaultSeverity` is neither `error` nor `warning`, so it is not \
          handed to Stylelint"
        ]
    );
    assert!(handed.options.is_empty());
}

#[test]
fn an_entry_of_overrides_carries_rules_for_patterns_and_nothing_else() {
    let (handed, problems) = configured(&[(
        ".stylelintrc.yaml",
        "overrides:\n\
         \x20 - files: legacy/**\n\
         \x20   name: legacy\n\
         \x20   customSyntax: postcss-scss\n\
         \x20   ignoreFiles: [x]\n\
         \x20   rules:\n\
         \x20     color-named: null\n\
         \x20     my-rule: true\n\
         \x20 - 3\n\
         \x20 - rules: {}\n\
         \x20 - files: []\n\
         \x20 - files: [3]\n\
         \x20 - files: 3\n\
         \x20 - files: ['[a']\n\
         \x20 - files: '*.scss'\n\
         \x20 - files: [x]\n\
         \x20   rules: 3\n",
    )]);

    let at = ".stylelintrc.yaml:";
    assert_eq!(
        messages(&problems),
        [
            format!(
                "{at} `overrides[0].customSyntax` is not handed to Stylelint: it names a module \
                 that would read the stylesheets, which would be loaded and run, and the CSS \
                 check runs no code of the repository"
            ),
            format!(
                "{at} `overrides[0].ignoreFiles` is not handed to Stylelint: an entry of \
                 `overrides` carries rules for patterns, and nothing else"
            ),
            format!(
                "{at} `overrides[0].rules.my-rule` is no rule of Stylelint 18.0.0 or of its \
                 plugin for SCSS, so it is not handed to Stylelint"
            ),
            format!("{at} `overrides[1]` is not a mapping, so it is not handed to Stylelint"),
            format!("{at} `overrides[2]` has no `files`, so it is not handed to Stylelint"),
            format!(
                "{at} `overrides[3].files` holds no pattern, so the entry is not handed to \
                 Stylelint"
            ),
            format!(
                "{at} `overrides[4].files` is neither a pattern nor a list of patterns, so the \
                 entry is not handed to Stylelint"
            ),
            format!(
                "{at} `overrides[5].files` is neither a pattern nor a list of patterns, so the \
                 entry is not handed to Stylelint"
            ),
            format!(
                "{at} `overrides[6].files` holds a pattern that does not read: error parsing \
                 glob '[a': unclosed character class; missing ']', so the entry is not handed to \
                 Stylelint"
            ),
            format!(
                "{at} `overrides[8].rules` is not a mapping of rules, so it is not handed to \
                 Stylelint"
            ),
        ]
    );
    // The entries that read are handed over, each with the rules it could
    // hand over.
    let kept: Vec<(Vec<String>, usize)> = handed
        .overrides
        .iter()
        .map(|entry| (entry.patterns.written.clone(), entry.rules.len()))
        .collect();
    assert_eq!(
        kept,
        [
            (vec!["legacy/**".to_owned()], 1),
            (vec!["*.scss".to_owned()], 0),
            (vec!["x".to_owned()], 0),
        ]
    );

    let (handed, problems) = configured(&[(".stylelintrc.yaml", "overrides: {}\n")]);
    assert_eq!(
        messages(&problems),
        [format!(
            "{at} `overrides` is not a list, so it is not handed to Stylelint"
        )]
    );
    assert!(handed.overrides.is_empty());
}

#[test]
fn the_patterns_of_an_entry_are_read_as_stylelint_reads_them() {
    let (handed, problems) = configured(&[(
        ".stylelintrc.json",
        "{ \"overrides\": [\n\
           { \"files\": \"*.scss\", \"rules\": { \"color-named\": \"never\" } },\n\
           { \"files\": [\"./legacy/*.css\"], \"rules\": { \"color-no-hex\": true } },\n\
           { \"files\": [\"!web/**\"], \"rules\": { \"block-no-empty\": null } },\n\
           { \"files\": [\"**/*.css\", \"!legacy/**\"], \"rules\": { \"comment-no-empty\": null } }\n\
         ] }",
    )]);
    assert!(problems.is_empty(), "{problems:?}");
    let matched = |file: &str| -> Vec<bool> {
        handed
            .overrides
            .iter()
            .map(|entry| entry.patterns.matches(file))
            .collect()
    };

    // A pattern without a `/` matches the file's name in any folder, and a
    // pattern with `!` matches every path its pattern does not.
    assert_eq!(matched("web/theme.scss"), [true, false, false, true]);
    // `./` names the root, and `*` stops at a `/`.
    assert_eq!(matched("legacy/old.css"), [false, true, true, true]);
    assert_eq!(matched("legacy/x/old.css"), [false, false, true, true]);
    // Beside another pattern, one with `!` takes nothing back.
    assert_eq!(matched("web/site.css"), [false, false, false, true]);
}

/// Whether each pattern, alone in an entry of `overrides` in the root's
/// `.stylelintrc.json`, matches each of `stylesheets`.
fn each_pattern_matches(patterns: &[&str], stylesheets: &[&str]) -> Vec<Vec<bool>> {
    let entries: Vec<String> = patterns
        .iter()
        .map(|pattern| format!("{{ \"files\": {}, \"rules\": {{}} }}", json!(pattern)))
        .collect();
    let (handed, problems) = configured(&[(
        ".stylelintrc.json",
        &format!("{{ \"overrides\": [{}] }}", entries.join(", ")),
    )]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(handed.overrides.len(), patterns.len());
    handed
        .overrides
        .iter()
        .map(|entry| {
            stylesheets
                .iter()
                .map(|stylesheet| entry.patterns.matches(stylesheet))
                .collect()
        })
        .collect()
}

#[test]
fn a_pattern_that_does_not_read_fails_even_where_it_climbs_above_the_root() {
    // Joined onto the root it names no file there, and it still matches a
    // file's name, so it has to read.
    let (handed, problems) = configured(&[(
        ".stylelintrc.json",
        "{ \"overrides\": [{ \"files\": \"../[\", \"rules\": {} }] }",
    )]);
    assert!(handed.overrides.is_empty());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0]
            .message
            .starts_with("`overrides[0].files` holds a pattern that does not read: "),
        "{problems:?}"
    );
}

#[test]
fn a_pattern_is_joined_onto_the_root_as_node_joins_paths() {
    let matched = each_pattern_matches(
        &[
            "web/../site.css",
            "web//./site.css",
            "web/",
            // A pattern matches the file's name as well, so this one matches
            // in any folder.
            "**/site.css",
            // Above the root, a pattern matches no file there, and with `!`
            // every file.
            "../site.css",
            "!../site.css",
            // A pattern that starts with `/` is a path of its own.
            "/site.css",
        ],
        &["site.css", "web/site.css", "web"],
    );
    assert_eq!(
        matched,
        [
            [true, false, false],
            [false, true, false],
            [false, false, false],
            [true, true, false],
            [false, false, false],
            [true, true, true],
            [false, false, false],
        ]
    );
}

#[test]
fn a_file_in_the_folder_config_is_not_read_and_its_patterns_count_for_nothing() {
    // Only the root is read: the forms Stylelint would also find in `.config`
    // are named, and nothing of them is handed over.
    for name in [
        ".config/stylelintrc",
        ".config/stylelintrc.json",
        ".config/stylelintrc.yaml",
        ".config/stylelintrc.yml",
    ] {
        let (handed, problems) = configured(&[
            (
                name,
                "{ \"overrides\": [{ \"files\": [\"../web/*.css\"], \"rules\": { \"color-named\": \"never\" } }] }",
            ),
            ("web/site.css", "a {}\n"),
        ]);
        assert!(handed.file.is_none(), "{name}");
        assert!(handed.overrides.is_empty(), "{name}");
        assert_eq!(problems.len(), 1, "{name}: {problems:?}");
        assert_eq!(problems[0].file.to_string(), name);
        assert!(
            problems[0].message.ends_with(
                "from one file at the root of the repository (`.stylelintrc`, \
                           `.stylelintrc.json`, `.stylelintrc.yaml`, `.stylelintrc.yml`), and \
                           never runs this one"
            ),
            "{name}: {problems:?}"
        );
    }

    // Beside a file at the root, the root's patterns count from the root.
    let (handed, problems) = configured(&[
        (".config/stylelintrc.json", "{}"),
        (
            ".stylelintrc.json",
            "{ \"overrides\": [{ \"files\": [\"web/*.css\"], \"rules\": {} }] }",
        ),
    ]);
    assert_eq!(
        handed.file.as_ref().map(ToString::to_string).as_deref(),
        Some(".stylelintrc.json")
    );
    assert_eq!(messages(&problems).len(), 1, "{problems:?}");
    assert!(handed.overrides[0].patterns.matches("web/site.css"));
    assert!(!handed.overrides[0].patterns.matches(".config/web/site.css"));
}

#[test]
fn fix_and_quiet_are_accepted_at_false_and_fail_by_name_at_any_other_value() {
    // In every format the reader takes: plain JSON, and YAML under three
    // names.
    let write = |file: &str, key: &str, value: &str| {
        if file == ".stylelintrc.json" {
            format!("{{ \"{key}\": {value} }}")
        } else {
            format!("{key}: {value}\n")
        }
    };
    for file in [
        ".stylelintrc",
        ".stylelintrc.json",
        ".stylelintrc.yaml",
        ".stylelintrc.yml",
    ] {
        for (key, why) in NEUTRAL {
            let (handed, problems) = configured(&[(file, &write(file, key, "false"))]);
            assert!(problems.is_empty(), "{file} {key}: {problems:?}");
            assert_eq!(
                handed.file.as_ref().map(ToString::to_string).as_deref(),
                Some(file)
            );
            // Not handed over: Stylelint takes `false` without it.
            assert!(handed.options.is_empty(), "{file} {key}");
            assert_eq!(
                handed.job(&default(), &["a.css".to_owned()]),
                "{\"groups\":[{\"extends\":[\"stylelint-config-standard\"],\
                 \"customSyntax\":null,\"config\":{\"rules\":{}},\"files\":[\"a.css\"]}]}"
            );

            for value in ["true", "null", "\"strict\"", "0"] {
                let (handed, problems) = configured(&[(file, &write(file, key, value))]);
                assert_eq!(
                    messages(&problems),
                    [format!(
                        "{file}: `{key}` is not handed to Stylelint: any value but `false` {why}"
                    )],
                    "{file} {key}: {value}"
                );
                assert!(handed.options.is_empty(), "{file} {key}: {value}");
            }
        }
    }
    assert_eq!(
        NEUTRAL.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
        ["fix", "quiet"]
    );
}

#[test]
fn the_rules_of_a_file_are_laid_as_stylelint_lays_them() {
    let default = pinned(
        "extends: [base]\n\
         rules:\n\
         \x20 color-named: never\n\
         \x20 block-no-empty: true\n\
         overrides:\n\
         \x20 - files: ['**/*.scss']\n\
         \x20   extends: [scss]\n\
         \x20   customSyntax: syntax\n\
         \x20   rules:\n\
         \x20     block-no-empty: null\n\
         \x20 - files: ['web/**']\n\
         \x20   rules:\n\
         \x20     comment-no-empty: true\n",
    )
    .unwrap();
    let (handed, problems) = configured(&[(
        ".stylelintrc.yaml",
        "rules:\n\
         \x20 color-hex-length: long\n\
         \x20 color-named: null\n\
         overrides:\n\
         \x20 - files: ['web/**']\n\
         \x20   rules:\n\
         \x20     color-hex-length: short\n\
         \x20 - files: ['*.scss']\n\
         \x20   rules:\n\
         \x20     color-hex-length: null\n",
    )]);
    assert!(problems.is_empty(), "{problems:?}");

    let css = handed.layer_for(&default, "site.css");
    assert_eq!(css.extends, ["base"]);
    assert!(css.custom_syntax.is_none());
    assert_eq!(
        rules(&css),
        "{\"color-named\":null,\"block-no-empty\":true,\"color-hex-length\":\"long\"}"
    );

    // Each entry that matches is laid over the one before it, in its order.
    let scss = handed.layer_for(&default, "web/theme.scss");
    assert_eq!(scss.extends, ["base", "scss"]);
    assert_eq!(scss.custom_syntax.as_deref(), Some("syntax"));
    assert_eq!(
        rules(&scss),
        "{\"color-named\":null,\"block-no-empty\":null,\"comment-no-empty\":true,\
         \"color-hex-length\":null}"
    );

    // Files with the same configuration go in one group.
    let job = handed.job(
        &default,
        &[
            "a.css".to_owned(),
            "web/a.scss".to_owned(),
            "b.css".to_owned(),
        ],
    );
    assert_eq!(job.matches("\"files\":").count(), 2, "{job}");
    assert!(job.contains("\"files\":[\"a.css\",\"b.css\"]"), "{job}");
}

#[test]
fn a_rule_of_the_plugin_for_scss_counts_only_for_the_files_read_as_scss() {
    // Stylelint knows no rule of the plugin where the plugin is not loaded,
    // and reports one handed over as unknown, even one switched off.
    let default = default();
    let (handed, problems) = configured(&[(
        ".stylelintrc.json",
        "{\n\
           \"rules\": { \"scss/dollar-variable-pattern\": null, \"color-named\": null },\n\
           \"overrides\": [\n\
             { \"files\": [\"**\"], \"rules\": { \"scss/comment-no-empty\": true } }\n\
           ]\n\
         }",
    )]);
    assert!(problems.is_empty(), "{problems:?}");

    let css = handed.layer_for(&default, "site.css");
    assert_eq!(rules(&css), "{\"color-named\":null}");

    let scss = handed.layer_for(&default, "web/theme.scss");
    assert_eq!(
        rules(&scss),
        "{\"scss/dollar-variable-pattern\":null,\"color-named\":null,\
         \"scss/comment-no-empty\":true}"
    );

    let job = handed.job(
        &default,
        &["site.css".to_owned(), "web/theme.scss".to_owned()],
    );
    assert_eq!(job.matches("scss/").count(), 2, "{job}");
}

#[test]
fn a_file_that_holds_no_configuration_hands_nothing_over() {
    let (handed, problems) = configured(&[(".stylelintrc", "- a\n")]);
    assert!(handed.file.is_none());
    assert_eq!(
        messages(&problems),
        [".stylelintrc: holds no mapping, so nothing of it is handed to Stylelint"]
    );

    // JSON takes no comment, as Stylelint read it.
    let (handed, problems) = configured(&[(".stylelintrc.json", "{ // no\n}")]);
    assert!(handed.file.is_none());
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0]
            .message
            .starts_with("cannot be read, so nothing of it is handed to Stylelint: "),
        "{problems:?}"
    );

    // An empty file of YAML lays nothing over the default.
    let (handed, problems) = configured(&[(".stylelintrc.yml", "")]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        handed.file.as_ref().map(ToString::to_string).as_deref(),
        Some(".stylelintrc.yml")
    );
    assert!(handed.rules.is_empty());
}

#[test]
fn a_register_without_stylelint_reads_nothing_for_it() {
    let mut register = register();
    register
        .program
        .retain(|name, _| name.as_str() != "stylelint");
    let all = paths(&[".stylelintrc.json"]);
    let refs: Vec<&RelativePath> = all.iter().collect();

    let (handed, problems) = read_in(
        &register,
        &refs,
        &|_| true,
        &reader(&[(".stylelintrc.json", "{ \"plugins\": [] }")]),
    );

    assert!(handed.file.is_none());
    assert!(problems.is_empty());

    let (handed, problems) = read_in(
        &self::register(),
        &refs,
        &|_| true,
        &reader(&[(".stylelintrc.json", "{ \"plugins\": [] }")]),
    );
    assert!(handed.file.is_some());
    assert_eq!(problems.len(), 1);
}

#[test]
fn a_package_json_with_a_configuration_of_stylelint_is_named() {
    let all = paths(&[
        "package.json",
        "web/package.json",
        "admin/package.json",
        "broken/package.json",
        "vendor/package.json",
        "stylelint.json",
    ]);
    let refs: Vec<&RelativePath> = all.iter().collect();
    let read = reader(&[
        ("package.json", "{ \"name\": \"site\" }"),
        ("web/package.json", "{ \"stylelint\": { \"rules\": {} } }"),
        ("admin/package.json", "{ \"stylelint\": null }"),
        ("broken/package.json", "{ \"stylelint\": "),
        ("vendor/package.json", "{ \"stylelint\": {} }"),
        ("stylelint.json", "{ \"stylelint\": {} }"),
    ]);
    let kept = |file: &RelativePath| !file.to_string().starts_with("vendor/");

    let unread = beside(&refs, &kept, &read);

    let named: Vec<String> = unread
        .iter()
        .map(|unread| format!("{}: {}", unread.file, unread.why))
        .collect();
    assert_eq!(
        named,
        [
            "web/package.json: not read: the tool reads the configuration of stylelint only as \
          data, from one file at the root of the repository, and not from the `stylelint` key of \
          `package.json`"
        ]
    );

    // The check names it as well, as it reads the configuration.
    let (handed, problems) = configured(&[
        (".stylelintrc.json", "{}"),
        ("web/package.json", "{ \"stylelint\": { \"rules\": {} } }"),
    ]);
    assert!(handed.file.is_some());
    assert_eq!(
        messages(&problems),
        [named[0].clone()],
        "the reader names what phase 0 names"
    );
}

#[test]
fn phase_0_names_every_file_of_stylelint_that_is_not_read() {
    let declaration = "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n\
                       [[exclude]]\npath   = \"vendor\"\nreason = \"kept as it came\"\n";
    let analysis = analysis(
        declaration,
        &[
            ".config/stylelintrc.json",
            ".nebensound/repo.toml",
            ".stylelintrc.json",
            "package.json",
            "stylelint.config.mjs",
            "vendor/.stylelintrc.cjs",
            "vendor/package.json",
        ],
    );
    let read = reader(&[
        ("package.json", "{ \"stylelint\": { \"rules\": {} } }"),
        (
            "vendor/package.json",
            "{ \"stylelint\": { \"rules\": {} } }",
        ),
    ]);

    let found = findings(&analysis, &register(), &read);

    let summary: Vec<String> = found
        .iter()
        .map(|finding| format!("{} {}", finding.rule, finding.path.as_ref().unwrap()))
        .collect();
    assert_eq!(
        summary,
        [
            "unread-configuration .config/stylelintrc.json",
            "unread-configuration stylelint.config.mjs",
            "unread-configuration package.json",
        ]
    );
}
