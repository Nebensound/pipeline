//! A repository's configuration of a linter, read in one place: which file
//! is read, which are named as not read, and for markdownlint which keys are
//! carried over and how the rules of each file are worked out. How the
//! Markdown check uses it is tested in `run.rs`, how the rules report shows
//! it in `rules.rs`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use nebensound_repo::Register;
use nebensound_repo::linter_config::markdownlint::{
    CARRIED, Combine, Handed, Problem, REFUSED, SET_ASIDE, default_rules, read,
};
use nebensound_repo::linter_config::{findings, locate};
use nebensound_repo::names::ProgramName;
use nebensound_repo::path::RelativePath;
use nebensound_repo::programs::Program;
use nebensound_repo::rules::MARKDOWNLINT;
use support::{QUIET, analysis, call, repository};

fn register() -> Register {
    Register::builtin().expect("the register")
}

fn markdownlint() -> (ProgramName, Program) {
    let register = register();
    let (name, program) = register
        .program
        .iter()
        .find(|(name, _)| name.as_str() == "markdownlint")
        .expect("markdownlint in the register");
    (name.clone(), program.clone())
}

fn paths(files: &[&str]) -> Vec<RelativePath> {
    files
        .iter()
        .map(|file| RelativePath::new(file).unwrap())
        .collect()
}

/// markdownlint's configuration in a repository holding these files, read
/// from memory, every one of them kept.
fn handed(files: &[(&str, &str)]) -> (Handed, Vec<Problem>) {
    let contents: BTreeMap<String, String> = files
        .iter()
        .map(|(path, content)| ((*path).to_owned(), (*content).to_owned()))
        .collect();
    let all = paths(&files.iter().map(|(path, _)| *path).collect::<Vec<_>>());
    let refs: Vec<&RelativePath> = all.iter().collect();
    let source = |path: &RelativePath| {
        contents
            .get(&path.to_string())
            .cloned()
            .ok_or_else(|| "no such file".to_owned())
    };
    let (name, program) = markdownlint();
    read(&refs, &|_| true, &name, &program, &source)
}

fn messages(problems: &[Problem]) -> Vec<String> {
    problems
        .iter()
        .map(|problem| format!("{}: {}", problem.file, problem.message))
        .collect()
}

#[test]
fn every_key_of_an_options_file_is_known_once() {
    let set_aside: Vec<&str> = SET_ASIDE.iter().map(|(key, _)| *key).collect();
    let refused: Vec<&str> = REFUSED.iter().map(|(key, _)| *key).collect();
    for list in [CARRIED, set_aside.as_slice(), refused.as_slice()] {
        assert!(list.windows(2).all(|pair| pair[0] < pair[1]), "{list:?}");
    }
    let mut all: Vec<&str> = [CARRIED, set_aside.as_slice(), refused.as_slice()].concat();
    all.sort_unstable();
    all.dedup();
    // The keys markdownlint-cli2 0.23.2 reads from an options file, and the
    // schema's `$schema`.
    assert_eq!(all.len(), 16, "{all:?}");
}

#[test]
fn the_first_name_at_the_root_is_read_and_every_other_file_is_named() {
    let (name, program) = markdownlint();
    let all = paths(&[
        ".markdownlint-cli2.yaml",
        ".markdownlint.json",
        "README.md",
        "docs/.markdownlint-cli2.jsonc",
        "docs/.markdownlint.mjs",
        "vendor/.markdownlint.yml",
        "x.markdownlint.json",
    ]);
    let refs: Vec<&RelativePath> = all.iter().collect();
    let kept = |file: &RelativePath| !file.to_string().starts_with("vendor/");

    let located = locate(&refs, &kept, &name, &program);

    assert_eq!(
        located.file.map(ToString::to_string).as_deref(),
        Some(".markdownlint-cli2.yaml")
    );
    let unread: Vec<String> = located
        .unread
        .iter()
        .map(|unread| unread.file.to_string())
        .collect();
    assert_eq!(
        unread,
        [
            ".markdownlint.json",
            "docs/.markdownlint-cli2.jsonc",
            "docs/.markdownlint.mjs"
        ]
    );
    assert!(
        located
            .unread
            .iter()
            .all(|unread| unread.why.starts_with("not read: "))
    );
}

#[test]
fn without_a_file_nothing_is_laid_over_the_default() {
    let (handed, problems) = handed(&[("README.md", "# x\n")]);

    assert!(handed.file.is_none());
    assert!(problems.is_empty());
    assert_eq!(
        handed.job(&default_rules(MARKDOWNLINT), &["README.md".to_owned()]),
        "{\"frontMatter\":null,\"noInlineConfig\":false,\"groups\":[{\"config\":\
         {\"MD013\":false},\"files\":[\"README.md\"]}]}"
    );
}

#[test]
fn a_file_of_rules_only_is_laid_over_the_default() {
    let (handed, problems) = handed(&[(
        ".markdownlint.jsonc",
        "{ \"$schema\": \"x\", \"MD013\": true, \"headings\": false, \"Default\": true }",
    )]);

    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        handed.job(&default_rules(MARKDOWNLINT), &["a.md".to_owned()]),
        "{\"frontMatter\":null,\"noInlineConfig\":false,\"groups\":[{\"config\":\
         {\"MD013\":true,\"Default\":true,\"headings\":false},\"files\":[\"a.md\"]}]}"
    );
}

#[test]
fn the_rules_of_a_file_come_from_the_first_entry_of_overrides_that_matches_it() {
    let (handed, problems) = handed(&[(
        ".markdownlint-cli2.jsonc",
        "{ \"config\": { \"MD033\": false },
           \"overrides\": [
             { \"filter\": [\".github/*.md\", \"!.github/keep.md\"], \"combine\": \"merge\",
               \"config\": { \"MD041\": false } },
             { \"filter\": [\".github/**\"], \"combine\": \"replace\",
               \"config\": { \"MD001\": false } },
             { \"filter\": [\"!**/*.md\"], \"combine\": \"merge\",
               \"config\": { \"MD003\": false } }
           ] }",
    )]);
    assert!(problems.is_empty(), "{problems:?}");
    let default = default_rules(MARKDOWNLINT);
    let keys = |file: &str| -> Vec<String> {
        handed
            .rules_for(&default, file)
            .into_iter()
            .map(|(key, _)| key)
            .collect()
    };

    assert_eq!(keys(".github/a.md"), ["MD013", "MD033", "MD041"]);
    // Taken back by `!`, the next entry counts; `replace` leaves the file's
    // rules out and the default in.
    assert_eq!(keys(".github/keep.md"), ["MD013", "MD001"]);
    // `*` stops at a `/`.
    assert_eq!(keys(".github/deep/a.md"), ["MD013", "MD001"]);
    // An entry of patterns with `!` alone matches what none of them takes
    // back.
    assert_eq!(keys("README.md"), ["MD013", "MD033"]);
    assert_eq!(keys("notes.txt"), ["MD013", "MD033", "MD003"]);
    assert_eq!(handed.overrides[1].combine, Combine::Replace);

    let job = handed.job(
        &default,
        &[
            ".github/a.md".to_owned(),
            "README.md".to_owned(),
            "docs/b.md".to_owned(),
        ],
    );
    assert_eq!(
        job,
        "{\"frontMatter\":null,\"noInlineConfig\":false,\"groups\":[\
         {\"config\":{\"MD013\":false,\"MD033\":false,\"MD041\":false},\"files\":[\".github/a.md\"]},\
         {\"config\":{\"MD013\":false,\"MD033\":false},\"files\":[\"README.md\",\"docs/b.md\"]}]}"
    );
}

#[test]
fn a_key_of_the_default_takes_the_value_of_the_file_in_its_place() {
    let (handed, _) = handed(&[(
        ".markdownlint.json",
        "{ \"MD013\": { \"line_length\": 100 } }",
    )]);
    let mut default = default_rules("{ \"MD001\": true, \"MD013\": false }");
    default.insert("MD003".to_owned(), serde_json::Value::Bool(true));

    let laid = handed.rules_for(&default, "a.md");

    assert_eq!(
        laid.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>(),
        ["MD001", "MD003", "MD013"]
    );
    assert_eq!(laid[2].1, serde_json::json!({ "line_length": 100 }));
}

#[test]
fn the_options_that_are_data_are_handed_over() {
    let (handed, problems) = handed(&[(
        ".markdownlint-cli2.yaml",
        "$schema: x\nfrontMatter: \"^---$\"\nnoInlineConfig: true\nignores: [a]\nglobs: [b]\n\
         gitignore: true\nnoBanner: true\nnoProgress: true\nshowFound: true\n",
    )]);

    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(handed.front_matter.as_deref(), Some("^---$"));
    assert!(handed.no_inline_config);
    assert_eq!(
        handed.job(&serde_json::Map::new(), &[]),
        "{\"frontMatter\":\"^---$\",\"noInlineConfig\":true,\"groups\":[]}"
    );
}

#[test]
fn every_key_that_is_not_handed_over_is_named_and_the_rest_is_handed_over() {
    let (handed, problems) = handed(&[(
        ".markdownlint-cli2.jsonc",
        "{
           \"config\": { \"extends\": \"base.json\", \"MD041\": false, \"MD999\": true },
           \"customRules\": [\"./rule.mjs\"],
           \"fix\": true,
           \"frontMatter\": 1,
           \"markdownItPlugins\": [[\"plugin\"]],
           \"modulePaths\": [\"x\"],
           \"noInlineConfig\": \"yes\",
           \"outputFormatters\": [[\"formatter\"]],
           \"overrides\": [
             1,
             { \"filter\": [\"a/**\"] },
             { \"filter\": [\"a/**\"], \"config\": { \"extends\": \"x\", \"MD001\": false },
               \"combine\": \"both\", \"label\": \"x\" },
             { \"filter\": \"a/**\", \"config\": {}, \"combine\": \"merge\" },
             { \"filter\": [1], \"config\": {}, \"combine\": \"merge\" },
             { \"filter\": [], \"config\": {}, \"combine\": \"merge\" },
             { \"filter\": [\"a[\"], \"config\": {}, \"combine\": \"merge\" },
             { \"filter\": [\"b/**\"], \"config\": { \"MD001\": false }, \"combine\": \"replace\" }
           ],
           \"whatever\": 1
         }",
    )]);

    let file = ".markdownlint-cli2.jsonc";
    let code = "which would be loaded and run, and the Markdown check runs no code of the \
                repository";
    let expected: Vec<String> = [
        "`config.MD999` is no rule, alias or tag of markdownlint 0.41.1, so it is not handed to \
         markdownlint"
            .to_owned(),
        "`config.extends` is not handed to markdownlint: it names another file of configuration, \
         and the tool reads one file"
            .to_owned(),
        format!("`customRules` is not handed to markdownlint: it names modules of rules, {code}"),
        "`fix` is not handed to markdownlint: it makes markdownlint write the files it lints, and \
         the Markdown check only reads them"
            .to_owned(),
        "`frontMatter` is not text, so it is not handed to markdownlint".to_owned(),
        format!(
            "`markdownItPlugins` is not handed to markdownlint: it names modules of the parser, \
             {code}"
        ),
        "`modulePaths` is not handed to markdownlint: it names folders modules would be loaded \
         from, and the Markdown check runs no code of the repository"
            .to_owned(),
        "`noInlineConfig` is neither true nor false, so it is not handed to markdownlint"
            .to_owned(),
        format!(
            "`outputFormatters` is not handed to markdownlint: it names modules that would print \
             the findings, {code}"
        ),
        "`overrides[0]` is not a mapping, so it is not handed to markdownlint".to_owned(),
        "`overrides[1]` has no `config`, `combine`, so it is not handed to markdownlint".to_owned(),
        "`overrides[2].label` is not a setting the Markdown check knows, so it is not handed to \
         markdownlint"
            .to_owned(),
        "`overrides[2].config.extends` is not handed to markdownlint: it names another file of \
         configuration, and the tool reads one file"
            .to_owned(),
        "`overrides[2].combine` is neither `merge` nor `replace`, so the entry is not handed to \
         markdownlint"
            .to_owned(),
        "`overrides[3].filter` is not a list of patterns, so the entry is not handed to \
         markdownlint"
            .to_owned(),
        "`overrides[4].filter` is not a list of patterns, so the entry is not handed to \
         markdownlint"
            .to_owned(),
        "`overrides[5].filter` holds no pattern, so the entry is not handed to markdownlint"
            .to_owned(),
        "`whatever` is not a setting the Markdown check knows, so it is not handed to markdownlint"
            .to_owned(),
    ]
    .into_iter()
    .map(|message| format!("{file}: {message}"))
    .collect();
    let found = messages(&problems);
    let unreadable = found
        .iter()
        .position(|message| {
            message.contains("`overrides[6].filter` holds a pattern that does not read: ")
        })
        .expect("the pattern that does not read");
    let mut rest = found.clone();
    rest.remove(unreadable);
    assert_eq!(rest, expected);
    // What is not named is handed over.
    assert_eq!(handed.rules.keys().collect::<Vec<_>>(), ["MD041"]);
    assert_eq!(handed.overrides.len(), 1);
    assert_eq!(handed.overrides[0].filter, ["b/**"]);
}

#[test]
fn a_file_that_holds_no_configuration_hands_nothing_over() {
    for (file, source, message) in [
        (
            ".markdownlint.json",
            "[1]",
            "holds no mapping, so nothing of it is handed to markdownlint",
        ),
        (
            ".markdownlint.json",
            "{",
            "cannot be read, so nothing of it is handed to markdownlint: ",
        ),
        (
            ".markdownlint-cli2.jsonc",
            "{ \"config\": 1, \"overrides\": {} }",
            "`config` is not a mapping of rules, so it is not handed to markdownlint",
        ),
    ] {
        let (handed, problems) = handed(&[(file, source)]);

        assert!(handed.rules.is_empty());
        assert!(
            messages(&problems)[0].starts_with(&format!("{file}: {message}")),
            "{problems:?}"
        );
    }
    let (_, problems) = handed(&[(".markdownlint-cli2.jsonc", "{ \"overrides\": {} }")]);
    assert_eq!(
        messages(&problems),
        [
            ".markdownlint-cli2.jsonc: `overrides` is not a list, so it is not handed to markdownlint"
        ]
    );
}

#[test]
fn an_empty_file_of_yaml_is_read_as_one_that_sets_nothing() {
    let (handed, problems) = handed(&[(".markdownlint.yaml", "")]);

    assert!(problems.is_empty());
    assert_eq!(
        handed.file.map(|file| file.to_string()).as_deref(),
        Some(".markdownlint.yaml")
    );
}

#[test]
fn a_default_that_does_not_read_lays_nothing_beneath() {
    assert!(default_rules("{").is_empty());
    assert!(default_rules("[1]").is_empty());
}

#[test]
fn phase_0_names_every_file_that_is_not_read() {
    let declaration = "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n\
                       [[exclude]]\npath   = \"vendor\"\nreason = \"kept as it came\"\n";
    let analysis = analysis(
        declaration,
        &[
            ".nebensound/repo.toml",
            ".markdownlint-cli2.jsonc",
            ".markdownlint.mjs",
            "docs/.markdownlint.json",
            "vendor/.markdownlint.cjs",
        ],
    );

    let found = findings(&analysis, &register());

    let summary: Vec<String> = found
        .iter()
        .map(|finding| format!("{} {}", finding.rule, finding.path.as_ref().unwrap()))
        .collect();
    assert_eq!(
        summary,
        [
            "unread-configuration .markdownlint.mjs",
            "unread-configuration docs/.markdownlint.json",
        ]
    );
}

#[test]
fn check_fails_on_a_file_that_is_not_read() {
    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("README.md", "# x\n"),
        ("docs/.markdownlint.json", "{ \"MD041\": false }\n"),
    ]);
    let root = dir.path().to_str().unwrap();

    let (text, code) = call(&["nebensound-repo", "-C", root, "check"]);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("error[unread-configuration] at docs/.markdownlint.json: not read: "),
        "{text}"
    );
}
