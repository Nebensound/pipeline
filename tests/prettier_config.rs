//! Prettier's options, read by the tool from the repository's own file and
//! its `.editorconfig`: which keys are carried over, refused or unknown, how
//! the options of each file are worked out, and which files are named as not
//! read. How the checks start Prettier with them is tested in `run.rs`; that
//! the real Prettier formats with them, and loads nothing of the repository,
//! the cases of the blocks show.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use nebensound_repo::Register;
use nebensound_repo::linter_config::editorconfig::{self, Property};
use nebensound_repo::linter_config::prettier::{
    CARRIED, Handed, Kind, NEUTRAL, Neutral, PARSERS, REFUSED, Setting, VERSION, beside, flag,
    from_editorconfig, read, read_in,
};
use nebensound_repo::linter_config::{Problem, findings};
use nebensound_repo::names::ProgramName;
use nebensound_repo::path::RelativePath;
use nebensound_repo::programs::Program;
use support::analysis;

fn prettier() -> (ProgramName, Program) {
    let register = Register::builtin().expect("the register");
    let (name, program) = register
        .program
        .iter()
        .find(|(name, _)| name.as_str() == "prettier")
        .expect("Prettier in the register");
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

/// Prettier's configuration in a repository holding these files, every one
/// of them kept.
fn handed(files: &[(&str, &str)]) -> (Handed, Vec<Problem>) {
    let all = paths(&files.iter().map(|(path, _)| *path).collect::<Vec<_>>());
    let refs: Vec<&RelativePath> = all.iter().collect();
    let (name, program) = prettier();
    read(&refs, &|_| true, &name, &program, &reader(files))
}

fn messages(problems: &[Problem]) -> Vec<String> {
    problems
        .iter()
        .map(|problem| format!("{}: {}", problem.file, problem.message))
        .collect()
}

/// The flags a file is handed.
fn flags(handed: &Handed, file: &str) -> Vec<String> {
    handed
        .options_for(file)
        .iter()
        .map(|(name, setting)| flag(name, setting))
        .collect()
}

/// The flags a file is handed with only this `.editorconfig` beside it.
fn edited(config: &str, file: &str) -> Vec<String> {
    let (handed, problems) = handed(&[(".editorconfig", config)]);
    assert!(problems.is_empty(), "{problems:?}");
    flags(&handed, file)
}

/// Whether the one section of an `.editorconfig` holds for a file.
fn applies(pattern: &str, file: &str) -> bool {
    let read = editorconfig::parse(&format!("[{pattern}]\nindent_size = 3\n")).unwrap();
    assert!(read.unread.is_empty(), "{:?}", read.unread);
    !read.config.properties(file).is_empty()
}

/// Why the tool does not read the one section of an `.editorconfig`.
fn unread(pattern: &str) -> String {
    let read = editorconfig::parse(&format!("[{pattern}]\nindent_size = 3\n")).unwrap();
    let (section, why) = read.unread.first().expect("a section not read").clone();
    assert_eq!(section, pattern);
    assert!(read.config.properties("a").is_empty());
    why
}

#[test]
fn every_option_of_prettier_is_carried_over_accepted_or_refused_once() {
    let carried: Vec<&str> = CARRIED.iter().map(|(name, _)| *name).collect();
    let neutral: Vec<&str> = NEUTRAL.iter().map(|(name, ..)| *name).collect();
    let refused: Vec<&str> = REFUSED.iter().map(|(name, _)| *name).collect();
    for list in [&carried, &neutral, &refused] {
        assert!(list.windows(2).all(|pair| pair[0] < pair[1]), "{list:?}");
    }
    let mut all: Vec<&str> = [carried.as_slice(), neutral.as_slice(), refused.as_slice()].concat();
    all.sort_unstable();
    all.dedup();
    // The 30 options Prettier 3.9.6 gives, and `pluginSearchDirs`, which it
    // gave until 3.0 and which names folders.
    assert_eq!(all.len(), 31, "{all:?}");
    assert!(refused.contains(&"pluginSearchDirs"));
    assert!(CARRIED.contains(&("parser", Kind::Parser)));
    assert!(
        NEUTRAL
            .iter()
            .any(|entry| entry.0 == "rangeEnd" && entry.1 == Neutral::Unbounded)
    );
}

/// The lists were read from the Prettier the register pins: raising the pin
/// fails here until they are read again from the new version.
#[test]
fn the_lists_follow_the_pinned_prettier() {
    let (_, program) = prettier();
    assert_eq!(program.version, VERSION);
    // The 25 `choices` of the option `parser` in Prettier 3.9.6's `index.mjs`.
    assert_eq!(PARSERS.len(), 25);
    assert!(
        PARSERS.windows(2).all(|pair| pair[0] < pair[1]),
        "{PARSERS:?}"
    );
}

#[test]
fn without_a_file_prettier_formats_with_its_own_defaults() {
    let (handed, problems) = handed(&[("README.md", "# x\n")]);

    assert!(problems.is_empty());
    assert!(!handed.configured());
    let groups = handed.groups(&["a.json", "b.json"]);
    assert_eq!(groups, [(Vec::<String>::new(), vec!["a.json", "b.json"])]);
}

#[test]
fn every_option_of_the_file_is_handed_over_as_a_flag() {
    let (handed, problems) = handed(&[(
        ".prettierrc",
        "$schema: https://json.schemastore.org/prettierrc\nprintWidth: 100\nuseTabs: true\n\
         endOfLine: crlf\nproseWrap: always\nsingleQuote: false\ntabWidth: 4.0\n",
    )]);

    assert!(problems.is_empty(), "{problems:?}");
    assert!(handed.configured());
    assert_eq!(
        flags(&handed, "a.json"),
        [
            "--end-of-line=crlf",
            "--print-width=100",
            "--prose-wrap=always",
            "--single-quote=false",
            "--tab-width=4",
            "--use-tabs=true"
        ]
    );
    assert_eq!(handed.file.unwrap().to_string(), ".prettierrc");
    assert_eq!(
        flag(
            "htmlWhitespaceSensitivity",
            &Setting::Choice("ignore".to_owned())
        ),
        "--html-whitespace-sensitivity=ignore"
    );
    assert_eq!(
        flag("printWidth", &Setting::Unbounded),
        "--print-width=Infinity"
    );
}

#[test]
fn the_file_is_read_in_the_format_its_name_gives() {
    for (file, content) in [
        (".prettierrc", "{ \"semi\": false }\n"),
        (".prettierrc", "semi: false\n"),
        (".prettierrc.json", "{ \"semi\": false }\n"),
        (".prettierrc.json", "\u{feff}{ \"semi\": false }\n"),
        (".prettierrc.yml", "semi: false\n"),
        (".prettierrc.yaml", "semi: false\n"),
        (".prettierrc.json5", "// JSON5\n{ semi: false, }\n"),
        (".prettierrc.toml", "semi = false\n"),
    ] {
        let (handed, problems) = handed(&[(file, content)]);
        assert!(problems.is_empty(), "{file}: {problems:?}");
        assert_eq!(flags(&handed, "a.md"), ["--semi=false"], "{file}");
    }
    // An empty file of YAML sets nothing, and is read.
    let (handed, problems) = handed(&[(".prettierrc", "")]);
    assert!(problems.is_empty());
    assert!(handed.configured());
    assert!(flags(&handed, "a.md").is_empty());
}

#[test]
fn the_first_name_at_the_root_is_read_and_every_other_file_is_named() {
    let (handed, problems) = handed(&[
        (".prettierrc.yaml", "semi: false\n"),
        (".prettierrc.toml", "semi = true\n"),
        ("prettier.config.js", "module.exports = { semi: true };\n"),
        ("docs/.prettierrc", "semi: true\n"),
        ("docs/.prettierrc.mjs", "export default {};\n"),
    ]);

    assert_eq!(handed.file.unwrap().to_string(), ".prettierrc.yaml");
    let named: Vec<String> = problems.iter().map(|p| p.file.to_string()).collect();
    assert_eq!(
        named,
        [
            ".prettierrc.toml",
            "prettier.config.js",
            "docs/.prettierrc",
            "docs/.prettierrc.mjs"
        ]
    );
    assert!(problems.iter().all(|p| p.message.starts_with("not read: ")));
    assert!(
        problems[1]
            .message
            .contains("reads the configuration of prettier only as data"),
        "{problems:?}"
    );
}

#[test]
fn every_key_that_is_not_handed_over_is_named_and_the_rest_is_handed_over() {
    let (handed, problems) = handed(&[(
        ".prettierrc.json",
        "{ \"plugins\": [\"./plugin.mjs\"], \"parser\": \"toml\", \"requirePragma\": true, \
         \"checkIgnorePragma\": true, \"rangeStart\": 10, \"rangeEnd\": 100, \
         \"parenSpacing\": true, \"useTabs\": \"yes\", \"printWidth\": -1, \"tabWidth\": 1.5, \
         \"trailingComma\": \"always\", \"cursorOffset\": 0, \"semi\": false }",
    )]);

    assert_eq!(flags(&handed, "a.json"), ["--semi=false"]);
    assert_eq!(
        messages(&problems),
        [
            ".prettierrc.json: `checkIgnorePragma` is not handed to Prettier: any value but \
             `false` would pass every file that holds a `@noformat` comment unchecked",
            ".prettierrc.json: `cursorOffset` is not handed to Prettier: it places an editor's \
             cursor, and the check formats whole files",
            ".prettierrc.json: `parenSpacing` is not an option of Prettier 3.9.6 the check knows, \
             so it is not handed to Prettier",
            ".prettierrc.json: `parser` is not the name of a parser Prettier brings itself; any \
             other would need a plugin, so it is not handed to Prettier",
            ".prettierrc.json: `plugins` is not handed to Prettier: it names modules that would \
             be loaded and run, and the check runs no code of the repository",
            ".prettierrc.json: `printWidth` is not a whole number of 0 or more, so it is not \
             handed to Prettier",
            ".prettierrc.json: `rangeEnd` is not handed to Prettier: any value but `Infinity` \
             would leave the part of every file after it unchecked",
            ".prettierrc.json: `rangeStart` is not handed to Prettier: any value but `0` would \
             leave the part of every file before it unchecked",
            ".prettierrc.json: `requirePragma` is not handed to Prettier: any value but `false` \
             would pass every file that holds no `@format` comment unchecked",
            ".prettierrc.json: `tabWidth` is not a whole number of 0 or more, so it is not \
             handed to Prettier",
            ".prettierrc.json: `trailingComma` is not one of `all`, `es5`, `none`, so it is not \
             handed to Prettier",
            ".prettierrc.json: `useTabs` is not true or false, so it is not handed to Prettier",
        ]
    );
}

/// `requirePragma`, `checkIgnorePragma`, `rangeStart` and `rangeEnd` at the
/// value that changes nothing, as each format the tool reads writes it, are
/// accepted without a finding and not handed over: the command line sets
/// them to that value anyway.
#[test]
fn the_value_that_changes_nothing_is_accepted_in_every_format() {
    for (file, content) in [
        (
            ".prettierrc.json",
            "{ \"requirePragma\": false, \"checkIgnorePragma\": false, \"rangeStart\": 0, \
             \"rangeEnd\": 1e999 }",
        ),
        (
            ".prettierrc.json",
            "{ \"rangeStart\": -0, \"rangeEnd\": 1.8E308 }",
        ),
        (
            ".prettierrc.json5",
            "{ requirePragma: false, checkIgnorePragma: false, rangeStart: 0, rangeEnd: Infinity }",
        ),
        (
            ".prettierrc.json5",
            "{ rangeStart: 0x0, rangeEnd: +Infinity, }",
        ),
        (".prettierrc.json5", "{ rangeStart: 0.0, rangeEnd: 1e999 }"),
        (
            ".prettierrc",
            "requirePragma: false\ncheckIgnorePragma: false\nrangeStart: 0\nrangeEnd: .inf\n",
        ),
        (".prettierrc", "{ \"rangeEnd\": 1e999 }\n"),
        (".prettierrc.yml", "rangeStart: 0.0\nrangeEnd: +.inf\n"),
        (".prettierrc.yaml", "rangeEnd: .Inf\n"),
        (
            ".prettierrc.toml",
            "requirePragma = false\ncheckIgnorePragma = false\nrangeStart = 0\nrangeEnd = inf\n",
        ),
        (".prettierrc.toml", "rangeStart = 0.0\nrangeEnd = +inf\n"),
        (
            ".prettierrc.yaml",
            "overrides:\n  - files: \"*.md\"\n    options:\n      requirePragma: false\n      \
             rangeEnd: .inf\n",
        ),
    ] {
        let (handed, problems) = handed(&[(file, content)]);
        assert!(problems.is_empty(), "{file} {content}: {problems:?}");
        assert!(handed.configured());
        assert!(flags(&handed, "a.md").is_empty(), "{file} {content}");
    }
}

/// A value of the four that takes effect, or one that is not even of their
/// kind, fails by name in every format.
#[test]
fn a_value_that_takes_effect_fails_by_name_in_every_format() {
    for (file, content, keys) in [
        (
            ".prettierrc.json",
            "{ \"requirePragma\": null, \"rangeStart\": 1, \"rangeEnd\": 1e308 }",
            &["rangeEnd", "rangeStart", "requirePragma"][..],
        ),
        (
            ".prettierrc.json5",
            "{ checkIgnorePragma: 'false', rangeStart: NaN, rangeEnd: -Infinity }",
            &["checkIgnorePragma", "rangeEnd", "rangeStart"],
        ),
        (
            ".prettierrc.yaml",
            "requirePragma: true\nrangeStart: .inf\nrangeEnd: ~\n",
            &["rangeEnd", "rangeStart", "requirePragma"],
        ),
        (
            ".prettierrc",
            "rangeEnd: Infinity\nrangeStart: \"0\"\n",
            &["rangeEnd", "rangeStart"],
        ),
        (
            ".prettierrc.toml",
            "checkIgnorePragma = true\nrangeStart = 1979-05-27\nrangeEnd = 0\n",
            &["checkIgnorePragma", "rangeEnd", "rangeStart"],
        ),
    ] {
        let (_, problems) = handed(&[(file, content)]);
        let named: Vec<&str> = problems
            .iter()
            .map(|problem| {
                assert!(problem.message.contains("any value but"), "{problem:?}");
                problem.message.split('`').nth(1).unwrap()
            })
            .collect();
        assert_eq!(named, keys, "{file} {content}");
    }
}

/// Numbers are read as JavaScript reads them, in every form JSON5 writes,
/// and a key YAML writes as a number is a key as well.
#[test]
fn every_value_is_read_as_javascript_holds_it() {
    let (handed, problems) = handed(&[(
        ".prettierrc.json5",
        "{ printWidth: 0X64, tabWidth: +4, quoteProps: 'preserve', overrides: [{ files: ['*.md'], \
         options: { printWidth: 0x50 } }], semi: null }",
    )]);
    assert_eq!(
        flags(&handed, "a.md"),
        [
            "--print-width=80",
            "--quote-props=preserve",
            "--tab-width=4"
        ]
    );
    assert_eq!(
        messages(&problems),
        [".prettierrc.json5: `semi` is not true or false, so it is not handed to Prettier"]
    );

    let (handed, problems) =
        self::handed(&[(".prettierrc.json5", "{ tabWidth: .5, printWidth: 5. }")]);
    assert_eq!(flags(&handed, "a.md"), ["--print-width=5"]);
    assert_eq!(problems.len(), 1, "{problems:?}");

    let (handed, problems) = self::handed(&[(
        ".prettierrc.yaml",
        "1: true\nprintWidth: 1.0e2\nsingleQuote: true\noverrides: [{ files: \"*.md\" }]\n",
    )]);
    assert_eq!(
        flags(&handed, "a.md"),
        ["--print-width=100", "--single-quote=true"]
    );
    assert_eq!(
        messages(&problems),
        [
            ".prettierrc.yaml: `1` is not an option of Prettier 3.9.6 the check knows, so it is not \
          handed to Prettier"
        ]
    );

    let (handed, problems) = self::handed(&[(
        ".prettierrc.toml",
        "printWidth = 100.0\ntrailingComma = \"none\"\n\n[[overrides]]\nfiles = [\"*.md\"]\n\n\
         [overrides.options]\nuseTabs = true\n",
    )]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        flags(&handed, "a.md"),
        [
            "--print-width=100",
            "--trailing-comma=none",
            "--use-tabs=true"
        ]
    );
}

#[test]
fn a_file_that_holds_no_options_hands_nothing_over() {
    for (file, content, message) in [
        (
            ".prettierrc",
            "\"@company/prettier-config\"\n",
            ".prettierrc: names a shared configuration, `@company/prettier-config`, a module \
             that would be loaded and run, so nothing of it is handed to Prettier; write the \
             options themselves into the file",
        ),
        (
            ".prettierrc.json",
            "[1]",
            ".prettierrc.json: holds no mapping, so nothing of it is handed to Prettier",
        ),
    ] {
        let (handed, problems) = handed(&[(file, content)]);
        assert!(!handed.configured());
        assert_eq!(messages(&problems), [message]);
    }
    for (file, content) in [
        (".prettierrc.json", "{"),
        (".prettierrc.json", "// comment\n{ \"semi\": false }\n"),
        (".prettierrc.json", "{ \"semi\": false, }\n"),
        (".prettierrc.json", "{ \"rangeEnd\": Infinity }\n"),
        (".prettierrc.json", ""),
        (".prettierrc.json5", "{ semi: false"),
        // JSON5 needs a comma between two entries, as Prettier does.
        (".prettierrc.json5", "{ semi: false tabWidth: 4 }"),
        (".prettierrc.toml", "semi ="),
        // TOML has no number too large for a double: `inf` is its word for no end.
        (".prettierrc.toml", "rangeEnd = 1e999\n"),
        (".prettierrc", "a: [\n"),
        // Prettier stops at a file of YAML with more than one document.
        (".prettierrc.yaml", "tabWidth: 4\n---\nsemi: false\n"),
    ] {
        let (handed, problems) = handed(&[(file, content)]);
        assert!(!handed.configured());
        assert!(
            problems[0]
                .message
                .starts_with("cannot be read, so nothing of it is handed to Prettier: "),
            "{problems:?}"
        );
    }
    let (_, problems) = handed(&[(".prettierrc.yml", "tabWidth: 4\n---\nsemi: false\n")]);
    assert_eq!(
        messages(&problems),
        [
            ".prettierrc.yml: cannot be read, so nothing of it is handed to Prettier: it holds more \
             than one document"
        ]
    );
    let (handed, problems) = handed(&[(".prettierrc.yml", "---\ntabWidth: 4\n")]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(flags(&handed, "a.json"), ["--tab-width=4"]);
    let (_, problems) = self::handed(&[("README.md", "")]);
    assert!(problems.is_empty());
}

#[test]
fn a_difference_by_pattern_with_a_slash_matches_the_whole_path() {
    let (handed, problems) = handed(&[(
        ".prettierrc.yaml",
        "tabWidth: 2\noverrides:\n  - files: \"crates/*/tests/fixtures/**/*.json\"\n    \
         options:\n      tabWidth: 4\n",
    )]);

    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        flags(&handed, "crates/a/tests/fixtures/deep/x.json"),
        ["--tab-width=4"]
    );
    assert_eq!(
        flags(&handed, "crates/a/tests/fixtures/x.json"),
        ["--tab-width=4"]
    );
    assert_eq!(flags(&handed, "crates/a/x.json"), ["--tab-width=2"]);
    assert_eq!(flags(&handed, "x.json"), ["--tab-width=2"]);
    let groups = handed.groups(&["x.json", "crates/a/tests/fixtures/x.json", "y.json"]);
    assert_eq!(
        groups,
        [
            (vec!["--tab-width=2".to_owned()], vec!["x.json", "y.json"]),
            (
                vec!["--tab-width=4".to_owned()],
                vec!["crates/a/tests/fixtures/x.json"]
            ),
        ]
    );
}

#[test]
fn the_patterns_of_an_entry_are_read_as_prettier_reads_them() {
    let (handed, problems) = handed(&[(
        ".prettierrc.json",
        r#"{ "overrides": [
            { "files": ["*.md"], "excludeFiles": ["docs/**", "notes/b.md"],
              "options": { "proseWrap": "always" } },
            { "files": "./site/*.html", "options": { "printWidth": 120 } },
            { "files": ["site/**"], "excludeFiles": "*.css", "options": { "semi": false } },
            { "files": "!*.yaml", "options": { "bracketSpacing": false } },
            { "files": "*.yaml" },
            { "files": "*.md", "options": { "proseWrap": "never" } },
            { "files": "src/{a,b}.json", "options": { "tabWidth": 4 } }
        ] }"#,
    )]);

    assert!(problems.is_empty(), "{problems:?}");
    // Braces with a list of alternatives are read: either of them matches.
    assert_eq!(
        flags(&handed, "src/b.json"),
        ["--bracket-spacing=false", "--tab-width=4"]
    );
    assert_eq!(flags(&handed, "src/c.json"), ["--bracket-spacing=false"]);
    // Without a `/`, a pattern matches the file's name in any folder; an
    // exclusion with a `/` takes nothing out of it, a path written out does.
    assert_eq!(
        flags(&handed, "docs/a.md"),
        ["--bracket-spacing=false", "--prose-wrap=never"]
    );
    let (only_first, _) = self::handed(&[(
        ".prettierrc.json",
        r#"{ "overrides": [ { "files": ["*.md"], "excludeFiles": ["docs/**", "notes/b.md"],
            "options": { "proseWrap": "always" } } ] }"#,
    )]);
    assert_eq!(flags(&only_first, "docs/a.md"), ["--prose-wrap=always"]);
    assert!(flags(&only_first, "notes/b.md").is_empty());
    assert_eq!(flags(&only_first, "notes/c.md"), ["--prose-wrap=always"]);
    // A leading `./` names the root; a pattern with a `/` matches from there.
    assert_eq!(
        flags(&handed, "site/a.html"),
        [
            "--bracket-spacing=false",
            "--print-width=120",
            "--semi=false"
        ]
    );
    assert_eq!(
        flags(&handed, "site/x/a.html"),
        ["--bracket-spacing=false", "--semi=false"]
    );
    // With a `/`, an exclusion without one matches at the root only.
    assert_eq!(
        flags(&handed, "site/a.css"),
        ["--bracket-spacing=false", "--semi=false"]
    );
    // A leading `!` takes the match back; an entry without options sets
    // nothing.
    assert!(flags(&handed, "a.yaml").is_empty());
}

/// A parser Prettier brings itself is handed over, at the top and in an
/// entry of `overrides`: a repository may say that its files of one name are
/// read as JSON5.
#[test]
fn a_parser_prettier_brings_itself_is_handed_over() {
    let (handed, problems) = handed(&[(
        ".prettierrc.json",
        "{ \"overrides\": [{ \"files\": \"*.json\", \"options\": { \"parser\": \"json5\" } }] }",
    )]);

    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(flags(&handed, "a.json"), ["--parser=json5"]);
    assert!(flags(&handed, "a.md").is_empty());

    for parser in PARSERS {
        let content = format!("parser: {parser}\n");
        let (handed, problems) = self::handed(&[(".prettierrc", content.as_str())]);
        assert!(problems.is_empty(), "{parser}: {problems:?}");
        assert_eq!(flags(&handed, "a.md"), [format!("--parser={parser}")]);
    }
}

/// A parser only a plugin would bring, one Prettier keeps for itself, or a
/// name in another case is no parser Prettier brings, and fails by name.
#[test]
fn an_entry_that_names_a_plugin_parser_or_a_plugin_does_not_reach_prettier() {
    let (handed, problems) = handed(&[(
        ".prettierrc.yaml",
        "overrides:\n  - files: \"*.json\"\n    options:\n      parser: toml\n      \
         plugins: [prettier-plugin-toml]\n      tabWidth: 4\n",
    )]);

    assert_eq!(flags(&handed, "a.json"), ["--tab-width=4"]);
    assert_eq!(
        messages(&problems),
        [
            ".prettierrc.yaml: `overrides[0].options.parser` is not the name of a parser \
             Prettier brings itself; any other would need a plugin, so it is not handed to \
             Prettier",
            ".prettierrc.yaml: `overrides[0].options.plugins` is not handed to Prettier: it \
             names modules that would be loaded and run, and the check runs no code of the \
             repository",
        ]
    );
    for parser in ["__js_expression", "JSON", "babel-estree", "1"] {
        let content = format!("{{ \"parser\": \"{parser}\" }}");
        let (handed, problems) = self::handed(&[(".prettierrc.json", content.as_str())]);
        assert!(flags(&handed, "a.md").is_empty());
        assert_eq!(problems.len(), 1, "{parser}");
    }
}

#[test]
fn every_entry_that_cannot_be_used_is_named() {
    let (handed, problems) = handed(&[(
        ".prettierrc.json",
        r#"{ "overrides": [
            1,
            { "options": {} },
            { "files": 1 },
            { "files": ["*.md", 2] },
            { "files": "*.md", "excludeFiles": {} },
            { "files": "+(a|b).md" },
            { "files": "[a" },
            { "files": "*.md", "options": [], "unknown": 1 },
            { "files": "*.md", "options": { "overrides": [] } },
            { "files": "(foo|bar).json" },
            { "files": "src/{1..3}.json" },
            { "files": ["*.md", "g/[[:digit:]].json"] }
        ] }"#,
    )]);

    assert_eq!(handed.overrides.len(), 2);
    let all = messages(&problems);
    let heads: Vec<&str> = all
        .iter()
        .map(|message| message.split(", so ").next().unwrap())
        .collect();
    assert_eq!(
        heads,
        [
            ".prettierrc.json: `overrides[0]` is not a mapping",
            ".prettierrc.json: `overrides[1]` has no `files`",
            ".prettierrc.json: `overrides[2].files` is not a pattern or a list of patterns",
            ".prettierrc.json: `overrides[3].files` is not a pattern or a list of patterns",
            ".prettierrc.json: `overrides[4].excludeFiles` is not a pattern or a list of patterns",
            ".prettierrc.json: `overrides[5].files` holds `+(a|b).md`, a group, `(...)`, a form \
             of pattern the tool does not read",
            heads[6],
            ".prettierrc.json: `overrides[7].unknown` is not a setting of an entry of \
             `overrides` the check knows",
            ".prettierrc.json: `overrides[7].options` is not a mapping",
            ".prettierrc.json: `overrides[8].options.overrides` is not an option of Prettier \
             3.9.6 the check knows",
            ".prettierrc.json: `overrides[9].files` holds `(foo|bar).json`, a group, `(...)`, a \
             form of pattern the tool does not read",
            ".prettierrc.json: `overrides[10].files` holds `src/{1..3}.json`, a sequence, \
             `{a..b}`, a form of pattern the tool does not read",
            ".prettierrc.json: `overrides[11].files` holds `g/[[:digit:]].json`, a class of \
             characters, `[[:...:]]`, a form of pattern the tool does not read",
        ]
    );
    assert!(
        heads[6].starts_with(
            ".prettierrc.json: `overrides[6].files` holds a pattern that does not read: "
        ),
        "{all:?}"
    );
    let (_, problems) = self::handed(&[(".prettierrc.json", "{ \"overrides\": {} }")]);
    assert_eq!(
        messages(&problems),
        [".prettierrc.json: `overrides` is not a list, so it is not handed to Prettier"]
    );
}

#[test]
fn the_file_lies_over_editorconfig() {
    let (handed, problems) = handed(&[
        (
            ".editorconfig",
            "root = true\n\n[*]\nindent_style = space\nindent_size = 4\n",
        ),
        (".prettierrc", "tabWidth: 2\n"),
    ]);

    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        flags(&handed, "a.json"),
        ["--tab-width=2", "--use-tabs=false"]
    );
}

#[test]
fn editorconfig_is_mapped_as_prettier_maps_it() {
    let cases: &[(&str, &[&str])] = &[
        ("[*]\nindent_style = tab\n", &["--use-tabs=true"]),
        (
            "[*]\nindent_style = tab\ntab_width = 8\n",
            &["--tab-width=8", "--use-tabs=true"],
        ),
        (
            "[*]\nindent_style = Space\nindent_size = 4\n",
            &["--tab-width=4", "--use-tabs=false"],
        ),
        (
            "[*]\nindent_style = space\nindent_size = 4\ntab_width = 8\n",
            &["--tab-width=4", "--use-tabs=false"],
        ),
        ("[*]\nindent_size = 4\n", &["--tab-width=4"]),
        ("[*]\nindent_size = tab\n", &["--use-tabs=true"]),
        ("[*]\nindent_size = \"4\"\n", &[]),
        ("[*]\nindent_size = 0\n", &[]),
        ("[*]\nindent_size = 4.5\n", &[]),
        ("[*]\ntab_width = 1e20\n", &[]),
        ("[*]\nindent_size = true\n", &[]),
        ("[*]\nindent_size = null\n", &[]),
        ("[*]\nindent_style = unset\n", &[]),
        ("[*]\nmax_line_length = off\n", &["--print-width=Infinity"]),
        ("[*]\nmax_line_length = 100\n", &["--print-width=100"]),
        ("[*]\nmax_line_length = OFF\n", &[]),
        ("[*]\nquote_type = single\n", &["--single-quote=true"]),
        ("[*]\nquote_type = \"double\"\n", &["--single-quote=false"]),
        ("[*]\nquote_type = backtick\n", &[]),
        ("[*]\nend_of_line = LF\n", &["--end-of-line=lf"]),
        ("[*]\nend_of_line = cr\n", &["--end-of-line=cr"]),
        ("[*]\nend_of_line = auto\n", &[]),
        (
            "[*]\nindent_style = tab\n\n[*.md]\nindent_style = space\n",
            &["--use-tabs=false"],
        ),
    ];
    for (config, expected) in cases {
        assert_eq!(edited(config, "docs/a.md"), *expected, "{config}");
    }
    // A property before the first section holds for no file; a section
    // written twice is one, its later value counting.
    assert_eq!(
        edited(
            "indent_style = tab\n[*]\nmax_line_length = 80\n[*.md]\nindent_size = 3\n[*]\n\
             max_line_length = 90\n",
            "a.md"
        ),
        ["--print-width=90", "--tab-width=3"]
    );
}

#[test]
fn the_properties_are_read_as_the_package_reads_them() {
    let read = editorconfig::parse(
        "# a comment\n; another\r\n\r\n[*]\r\nIndent_Size = 4\nindent_size = 2\nquote_type = \
         Single\nend_of_line = \"LF\"\n",
    )
    .unwrap();
    let properties = read.config.properties("a");
    assert_eq!(
        properties,
        [
            ("indent_size".to_owned(), Property::Number(2.0)),
            ("quote_type".to_owned(), Property::Text("Single".to_owned())),
            ("end_of_line".to_owned(), Property::Text("LF".to_owned())),
            ("tab_width".to_owned(), Property::Number(2.0)),
        ]
    );
    assert_eq!(Property::Other.positive(), None);
    assert_eq!(editorconfig::whole(-1.0), None);
    assert_eq!(editorconfig::whole(3.0), Some(3));
}

#[test]
fn a_file_that_does_not_read_is_named() {
    for (config, line) in [
        ("[*]\n[x\n", "line 2 opens a section and does not close it"),
        (
            "[*]\r\nindent_size\n",
            "line 2 is neither a section, a property nor a comment",
        ),
    ] {
        assert_eq!(editorconfig::parse(config).unwrap_err(), line);
        let (handed, problems) = handed(&[(".editorconfig", config)]);
        assert!(!handed.configured());
        assert_eq!(
            messages(&problems),
            [format!(
                ".editorconfig: cannot be read, so nothing of it is handed to Prettier: {line}"
            )]
        );
    }
    let (handed, problems) = handed(&[(".editorconfig", "[+(a|b)]\nindent_size = 4\n")]);
    assert!(handed.configured());
    assert_eq!(
        messages(&problems),
        [
            ".editorconfig: the section `[+(a|b)]` is not handed to Prettier: it holds a group, \
          `(...)`, a form of pattern the tool does not read"
        ]
    );
}

#[test]
fn a_section_matches_as_the_package_matches_it() {
    let cases: &[(&str, &str, bool)] = &[
        ("*.md", "a.md", true),
        ("*.md", "docs/a.md", true),
        ("*.md", "a.mdx", false),
        ("/README.md", "README.md", true),
        ("/README.md", "docs/README.md", false),
        ("docs/*.md", "docs/a.md", true),
        ("docs/*.md", "docs/x/a.md", false),
        ("docs/*.md", "a.md", false),
        ("docs/**/*.md", "docs/a.md", true),
        ("docs/**/*.md", "docs/x/y/a.md", true),
        ("docs/**.md", "docs/x/a.md", true),
        ("docs/**", "docs/x/a.md", true),
        ("**", "x/a.md", true),
        ("?.md", "a.md", true),
        ("?.md", "ab.md", false),
        ("[ab].md", "a.md", true),
        ("[ab].md", "c.md", false),
        ("[!ab].md", "c.md", true),
        ("[!ab].md", "a.md", false),
        ("[^ab].md", "a.md", false),
        ("[]a].md", "].md", true),
        ("[a&&b].md", "&.md", true),
        ("[ab.md", "[ab.md", true),
        ("{a,b}.md", "b.md", true),
        ("{a,{b,c}}.md", "c.md", true),
        ("{a,b}.md", "c.md", false),
        ("{1..3}.md", "2.md", true),
        ("{3..1}.md", "1.md", true),
        ("{1..3}.md", "4.md", false),
        ("{a}.md", "{a}.md", true),
        ("{a..c}.md", "b.md", true),
        ("{a..c}.md", "{a..c}.md", false),
        ("{c..a}.md", "a.md", true),
        ("{Z..a}.md", "_.md", true),
        ("{Z..a}.md", "[.md", true),
        ("{Z..a}.md", ".md", true),
        ("{01..03}.json", "02.json", true),
        ("{01..03}.json", "2.json", false),
        ("{-01..1}.md", "-01.md", true),
        ("{-01..1}.md", "000.md", true),
        ("{1..10..3}.md", "7.md", true),
        ("{1..10..3}.md", "8.md", false),
        ("{10..1..-3}.md", "4.md", true),
        ("{1..10..03}.md", "04.md", true),
        ("{1..3..0}.md", "2.md", true),
        ("{1..3..99999999999999999999}.md", "1.md", true),
        ("{1..3..99999999999999999999}.md", "2.md", false),
        ("{a..1}.md", "{a..1}.md", true),
        ("{ab..c}.md", "{ab..c}.md", true),
        ("{+1..3}.md", "{+1..3}.md", true),
        ("{-..3}.md", "{-..3}.md", true),
        ("{1..3..x}.md", "{1..3..x}.md", true),
        ("{1..2..3..4}.md", "{1..2..3..4}.md", true),
        ("{a,b.md", "{a,b.md", true),
        ("\\*.md", "*.md", true),
        ("\\*.md", "a.md", false),
        ("{a\\,b,c}.md", "a,b.md", true),
        ("{a\\}b,c}.md", "a}b.md", true),
        ("{,x}a.md", "a.md", true),
    ];
    for (pattern, file, expected) in cases {
        assert_eq!(applies(pattern, file), *expected, "[{pattern}] {file}");
    }
}

#[test]
fn a_section_the_tool_does_not_read_is_named() {
    assert_eq!(
        unread("*(a).md"),
        "it holds a group, `(...)`, a form of pattern the tool does not read"
    );
    assert_eq!(
        unread("{1..99999999999999999999}"),
        "its braces hold a sequence whose ends are larger than the tool reads"
    );
    for pattern in [
        "{1..20000}",
        "{1..20001..2}",
        "{0..999}{0..99}",
        "{0..99}{0..99}{a,b}",
    ] {
        assert_eq!(
            unread(pattern),
            "its braces stand for more than 10000 patterns, more than the tool reads",
            "{pattern}"
        );
    }
    assert!(unread("[z-a]").starts_with("it does not read: "));
    for pattern in ["{*(a)}", "{a,*(b)}"] {
        assert_eq!(
            unread(pattern),
            "it holds a group, `(...)`, a form of pattern the tool does not read",
            "{pattern}"
        );
    }
}

#[test]
fn a_prettier_key_of_a_package_file_is_named() {
    let files = [
        (".editorconfig", "[*]\nindent_size = 2\n"),
        ("docs/.editorconfig", "[*]\nindent_size = 4\n"),
        ("vendor/package.json", "{ \"prettier\": {} }"),
        ("package.json", "{ \"prettier\": { \"semi\": false } }"),
        ("a/package.json", "{ \"prettier\": \"@company/config\" }"),
        ("b/package.json", "{ \"prettier\": false }"),
        ("c/package.json", "{ \"prettier\": \"\" }"),
        ("d/package.json", "{ \"prettier\": 0 }"),
        ("e/package.json", "{ \"prettier\": 1 }"),
        ("f/package.json", "{ \"prettier\": null }"),
        ("g/package.json", "{ \"name\": \"x\" }"),
        ("h/package.json", "{"),
        ("i/package.json", "{ \"prettier\": true }"),
        ("package.yaml", "prettier:\n  semi: false\n"),
    ];
    let all = paths(&files.iter().map(|(path, _)| *path).collect::<Vec<_>>());
    let refs: Vec<&RelativePath> = all.iter().collect();
    let kept = |file: &RelativePath| !file.to_string().starts_with("vendor/");

    let named = beside(&refs, &kept, &reader(&files));

    let summary: Vec<String> = named.iter().map(|unread| unread.file.to_string()).collect();
    assert_eq!(
        summary,
        [
            "package.json",
            "a/package.json",
            "e/package.json",
            "i/package.json",
            "package.yaml"
        ]
    );
    assert_eq!(
        named[0].why,
        "not read: the tool reads the configuration of prettier only as data, from one file at \
         the root of the repository, and not from the `prettier` key of `package.json`"
    );
}

#[test]
fn phase_0_names_every_file_prettier_would_read_and_the_tool_does_not() {
    // An `.editorconfig` in a folder is not read, and not named: the
    // pipeline's defaults take every `.editorconfig` out.
    let declaration = "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n\
                       [[exclude]]\npath   = \"vendor\"\nreason = \"kept as it came\"\n";
    let files = [
        (".nebensound/repo.toml", declaration),
        (".editorconfig", "[*]\n"),
        (".prettierrc.yaml", "semi: false\n"),
        ("prettier.config.mjs", "export default {};\n"),
        ("docs/.editorconfig", "[*]\n"),
        ("package.json", "{ \"prettier\": {} }"),
        ("vendor/.prettierrc.cjs", "module.exports = {};\n"),
    ];
    let analysis = analysis(
        declaration,
        &files.iter().map(|(path, _)| *path).collect::<Vec<_>>(),
    );
    let register = Register::builtin().unwrap();

    let found = findings(&analysis, &register, &reader(&files));

    let summary: Vec<String> = found
        .iter()
        .map(|finding| format!("{} {}", finding.rule, finding.path.as_ref().unwrap()))
        .collect();
    assert_eq!(
        summary,
        [
            "unread-configuration prettier.config.mjs",
            "unread-configuration package.json",
        ]
    );
}

#[test]
fn a_register_without_prettier_reads_nothing() {
    let register =
        Register::parse("[version]\n[build]\n[target]\n[preset]\n[ecosystem]\n[check]\n")
            .expect("a register without programs");
    let all = paths(&[".prettierrc"]);
    let refs: Vec<&RelativePath> = all.iter().collect();

    let (handed, problems) = read_in(
        &register,
        &refs,
        &|_| true,
        &reader(&[(".prettierrc", "x")]),
    );

    assert!(!handed.configured());
    assert!(problems.is_empty());
    let builtin = Register::builtin().unwrap();
    let (handed, _) = read_in(
        &builtin,
        &refs,
        &|_| true,
        &reader(&[(".prettierrc", "semi: false\n")]),
    );
    assert_eq!(flags(&handed, "a.md"), ["--semi=false"]);
}

#[test]
fn the_kinds_say_what_they_take() {
    assert!(CARRIED.contains(&("useTabs", Kind::Flag)));
    assert!(CARRIED.contains(&("printWidth", Kind::Count)));
    assert_eq!(
        from_editorconfig(&editorconfig::EditorConfig::default(), "a"),
        BTreeMap::new()
    );
}
