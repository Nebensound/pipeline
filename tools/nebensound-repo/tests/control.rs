//! Control characters in tracked file names and declared paths: an error of
//! phase 0 of their own, `control-character`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound_repo::control::holds_control;
use nebensound_repo::finding::{Rule, Severity, UnitName};
use nebensound_repo::names::ComponentName;
use nebensound_repo::path::RelativePath;
use nebensound_repo::report;
use support::{QUIET, analysis, call, repository};

/// The findings of the rule, as the report writes them.
fn written(analysis: &nebensound_repo::Analysis) -> Vec<String> {
    analysis
        .findings()
        .into_iter()
        .filter(|finding| finding.rule == Rule::ControlCharacter)
        .map(|finding| report::shown(&finding.to_string()))
        .collect()
}

#[test]
fn a_control_character_is_what_the_report_writes_as_an_escape() {
    // Every character, one at a time: the rule refuses exactly those the
    // report would not show as they stand.
    for character in (0..=0x0010_FFFF).filter_map(char::from_u32) {
        let text = character.to_string();
        assert_eq!(
            holds_control(&text),
            report::shown(&text) != text,
            "the rule and the report disagree on {character:?}"
        );
    }
    assert!(holds_control("a\nb"));
    assert!(holds_control("\u{9f}"));
    assert!(!holds_control("a b\u{a0}c"));
}

#[test]
fn a_tracked_file_whose_name_holds_a_control_character_is_an_error_at_it() {
    let analysis = analysis(QUIET, &["README.md", "docs/a\nb.md", "x\u{1b}[31m/c.md"]);

    assert!(analysis.failed());
    let found: Vec<_> = analysis
        .findings()
        .into_iter()
        .filter(|finding| finding.rule == Rule::ControlCharacter)
        .collect();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found
            .iter()
            .all(|finding| finding.severity == Severity::Error)
    );
    assert_eq!(
        found[0].path,
        Some(RelativePath::new("docs/a\nb.md").unwrap())
    );
    assert_eq!(found[0].unit, None);
    assert_eq!(
        written(&analysis),
        [
            "error[control-character] at docs/a\\nb.md: the name of this file holds a control \
             character, shown here as an escape. A tool that takes names one per line, or as the \
             value of a step output, would misread it; rename the file",
            "error[control-character] at x\\u{1b}[31m/c.md: the name of this file holds a \
             control character, shown here as an escape. A tool that takes names one per line, \
             or as the value of a step output, would misread it; rename the file",
        ]
    );
}

#[test]
fn a_declared_path_that_holds_a_control_character_is_an_error_at_the_declaration() {
    let source = "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n\
                  [component.app]\npath = \"app\\n::warning::forged\"\n\n\
                  [component.lib]\npath = \"lib\"\n\n\
                  [[include]]\npath = \"*.md\"\n\n\
                  [[include]]\npath = \"**/a\\tb\"\n\n\
                  [[exclude]]\npath = \"lib\"\nreason = \"x\"\n\n\
                  [[exclude]]\npath = \"lib/\\u001b\"\nreason = \"y\"\n";
    let analysis = analysis(source, &["README.md", "lib/a.md"]);

    assert!(analysis.failed());
    let found: Vec<_> = analysis
        .findings()
        .into_iter()
        .filter(|finding| finding.rule == Rule::ControlCharacter)
        .cloned()
        .collect();
    assert_eq!(found.len(), 3, "{found:?}");
    for finding in &found {
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(
            finding.path,
            Some(RelativePath::new(".nebensound/repo.toml").unwrap())
        );
    }
    assert_eq!(
        found
            .iter()
            .map(|finding| finding.unit.clone())
            .collect::<Vec<_>>(),
        [
            Some(UnitName::Component(ComponentName::new("app").unwrap())),
            None,
            None
        ]
    );
    let tail = "which holds a control character, shown here as an escape. No tracked file may \
                carry it either; write the path without it";
    assert_eq!(
        written(&analysis),
        [
            format!(
                "error[control-character] component app at .nebensound/repo.toml: the path of \
                 [component.app] is \"app\\n::warning::forged\", {tail}"
            ),
            format!(
                "error[control-character] at .nebensound/repo.toml: the path of [[include]] \
                 entry 2 is \"**/a\\tb\", {tail}"
            ),
            format!(
                "error[control-character] at .nebensound/repo.toml: the path of [[exclude]] \
                 entry 2 is \"lib/\\u{{1b}}\", {tail}"
            ),
        ]
    );
}

#[test]
fn names_without_a_control_character_find_nothing() {
    let analysis = analysis(
        "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[[exclude]]\npath = \
         \"a b.md\"\nreason = \"x\"\n",
        &["README.md", "a b.md", "c\u{a0}d.md"],
    );
    assert!(written(&analysis).is_empty(), "{:?}", analysis.findings());
    assert_eq!(Rule::ControlCharacter.code(), "control-character");
}

#[test]
fn the_report_writes_a_unit_path_with_a_control_character_as_an_escape() {
    let analysis = analysis(
        "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.app]\npath = \
         \"a\\n::error::forged\"\n",
        &["README.md", "a/b.md"],
    );
    let configuration = report::configuration(&analysis, &support::central());
    assert!(
        configuration.contains("a\\n::error::forged"),
        "{configuration}"
    );
    assert!(
        !configuration
            .lines()
            .any(|line| line.starts_with("::error::forged")),
        "{configuration}"
    );
}

#[test]
fn check_on_a_repository_with_such_a_name_fails_by_name_and_hands_out_no_list() {
    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("README.md", "# x\n"),
        ("docs/a\nb.md", "# a\n"),
    ]);
    let root = dir.path().to_str().unwrap();

    let (text, code) = call(&["nebensound-repo", "-C", root, "check"]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("error[control-character] at docs/a\\nb.md: the name of this file"),
        "{text}"
    );
    assert!(!text.lines().any(|line| line.starts_with("b.md")), "{text}");

    for (call_args, refusal) in [
        (
            vec!["plan"],
            "there is no plan: phase 0 found errors, and `check` says which\n",
        ),
        (
            vec!["files", "-z", "markdown", "."],
            "there is no file list: phase 0 found errors, and `check` says which\n",
        ),
    ] {
        let mut args = vec!["nebensound-repo", "-C", root];
        args.extend(call_args);
        let (text, code) = call(&args);
        assert_eq!(code, 1, "{text}");
        assert!(text.starts_with(refusal), "{text}");
    }
}

#[test]
fn a_declaration_that_does_not_read_writes_no_control_character_of_it() {
    // Before phase 0 can name a path, the declaration has to read. Where it
    // does not, the message quotes the path or the line, and writes every
    // control character in it as an escape.
    let forged = "a\\n::warning::forged";
    let cases = [
        (
            "[repo]\naudience = \"public\"\n\n[component.a]\npath = \"a\r::warning::forged\"\n"
                .to_owned(),
            "\\r::warning::forged",
        ),
        (
            format!(
                "[repo]\naudience = \"public\"\n\n[[exclude]]\npath = \"{forged}\"\nreason = \
                 \"x\"\n\n[[exclude]]\npath = \"{forged}\"\nreason = \"y\"\n"
            ),
            "a\\n::warning::forged stands twice under [[exclude]]",
        ),
        (
            format!(
                "[repo]\naudience = \"public\"\n\n[[exclude]]\npath = \"**/{forged}\"\nreason = \
                 \"x\"\n"
            ),
            "**/a\\n::warning::forged takes a whole type or name out of the check",
        ),
        (
            format!(
                "[repo]\naudience = \"public\"\n\n[[exclude]]\npath = \"{forged}\"\nreason = \
                 \"x\"\nonly-if-empty = true\n"
            ),
            "a\\n::warning::forged sets only-if-empty",
        ),
    ];
    for (declaration, said) in cases {
        let dir = repository(&[
            (".nebensound/repo.toml", &declaration),
            ("README.md", "# x\n"),
        ]);
        let (text, code) = call(&["nebensound-repo", "-C", dir.path().to_str().unwrap()]);
        assert_eq!(code, 1, "{text}");
        assert!(text.contains(said), "{said}\n{text}");
        assert!(!text.contains('\r'), "{text:?}");
        assert!(
            !text.lines().any(|line| line.starts_with("::warning")),
            "{text}"
        );
    }
}
