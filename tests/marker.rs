//! Markers of open tasks: every `TODO` and `FIXME` names the issue that tracks
//! it, an allowance with a reason lets it stay, and anything else fails
//! `check` by name.
//!
//! The cases are built from [`TODO_WORD`], [`FIXME_WORD`] and [`ALLOW`], so that this
//! file holds no marker and no allowance of its own: the search runs over it
//! as over every file of the repository.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;
use std::path::Path;

use nebensound_repo::init::UNRESOLVED_PIN;
use nebensound_repo::marker::{self, ALLOWANCE as ALLOW, BINARY_PREFIX, is_reference, judge};
use nebensound_repo::path::RelativePath;
use support::{QUIET, call, repository};

const TODO_WORD: &str = "TODO";
const FIXME_WORD: &str = "FIXME";

/// The codes of the search, and no other.
const CODES: [&str; 4] = [
    "unresolved-init-marker",
    "unreferenced-marker",
    "allowance-without-reason",
    "pointless-allowance",
];

/// What the search finds in a text: the code, the line and the column of
/// each finding, in order.
fn found(text: &str) -> Vec<(&'static str, usize, usize)> {
    let path = RelativePath::new("file").unwrap();
    judge(&path, text)
        .into_iter()
        .map(|finding| {
            let position = finding.position.expect("a place in the file");
            (finding.rule.code(), position.line, position.column)
        })
        .collect()
}

/// The findings of the search in what `check` prints, each a line.
fn printed(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|line| {
            CODES
                .iter()
                .any(|code| line.starts_with(&format!("  error[{code}]")))
        })
        .map(|line| {
            let (head, _) = line.split_once(": ").expect("a message");
            head.trim().to_owned()
        })
        .collect()
}

fn check(root: &Path) -> (Vec<String>, String) {
    let (output, _) = call(&["nebensound-repo", "-C", root.to_str().unwrap(), "check"]);
    (printed(&output), output)
}

#[test]
fn the_word_in_capitals_followed_by_a_parenthesis_or_a_colon_is_a_marker() {
    for marked in [
        format!("{TODO_WORD}: read the rest"),
        format!("{FIXME_WORD}: read the rest"),
        format!("// {TODO_WORD}(alice): read the rest"),
        format!("#{TODO_WORD}: no space before"),
        format!("\"{TODO_WORD}: in a string\""),
        format!("`{FIXME_WORD}(init)` in a code span"),
        format!("-{TODO_WORD}: after a hyphen"),
        format!("({TODO_WORD}: in parentheses)"),
    ] {
        assert_eq!(found(&marked).len(), 1, "{marked:?}");
    }
}

#[test]
fn the_word_in_a_sentence_or_inside_another_word_is_no_marker() {
    for prose in [
        format!("every {TODO_WORD} names an issue"),
        format!("{TODO_WORD} - read the rest"),
        format!("{TODO_WORD} (#12) with a space"),
        format!("{TODO_WORD}."),
        format!("{TODO_WORD}..init"),
        format!("{TODO_WORD}S: the plural"),
        format!("MY{TODO_WORD}: inside a word"),
        format!("_{TODO_WORD}: after an underscore"),
        format!("1{FIXME_WORD}: after a digit"),
        format!("\u{e9}{TODO_WORD}: after a letter that is no ASCII"),
        "todo: in small letters".to_owned(),
        "Todo: with one capital".to_owned(),
        "fn main() { todo!() }".to_owned(),
    ] {
        assert_eq!(found(&prose), [], "{prose:?}");
    }
}

#[test]
fn the_prose_of_this_repository_s_documents_holds_no_marker() {
    // Lines as the documents of this repository write them.
    for prose in [
        format!(
            "The file stays a valid workflow, but no branch, tag or commit can be named\n\
             `{TODO_WORD}..init`, so a run of it fails at once."
        ),
        format!(
            "No open task hides in a comment: `check` holds every `{TODO_WORD}` and \
             `{FIXME_WORD}` in the"
        ),
        format!(
            "**A marker** is the word `{TODO_WORD}` or `{FIXME_WORD}` in capitals, directly \
             followed by"
        ),
        format!(
            "Error. A `{TODO_WORD}` or `{FIXME_WORD}` marker names no issue that tracks its \
             task (see"
        ),
        format!("in a test that shows one. It is `{ALLOW}` followed by white space"),
        format!(
            "   `{FIXME_WORD}` in the files against the rule that it names the issue that tracks"
        ),
    ] {
        assert_eq!(found(&prose), [], "{prose:?}");
    }
}

#[test]
fn a_reference_is_an_issue_number_with_its_repository_in_front_or_alone() {
    for reference in [
        "#1",
        "#123",
        "owner/repo#5",
        "my-org/my.repo_x#5",
        "O1/R-2#90",
    ] {
        assert!(is_reference(reference), "{reference:?}");
    }
    for other in [
        "",
        "#",
        "#0",
        "#012",
        "#12a",
        "12",
        "init",
        "owner#5",
        "/repo#5",
        "owner/#5",
        "own er/repo#5",
        "owner_x/repo#5",
        "owner/re po#5",
        "owner/repo/x#5",
        "https://github.com/owner/repo/issues/5",
        "#1, #2",
    ] {
        assert!(!is_reference(other), "{other:?}");
    }
}

#[test]
fn a_marker_with_a_reference_stays_and_one_without_fails() {
    for stays in [
        format!("// {TODO_WORD}(#12): read the rest"),
        format!("// {FIXME_WORD}(#12): read the rest"),
        format!("// {TODO_WORD}(owner/repo#12): read the rest"),
        format!("// {FIXME_WORD}(owner/repo#12)"),
        format!("// {TODO_WORD}(#12) without a colon"),
    ] {
        assert_eq!(found(&stays), [], "{stays:?}");
    }
    for fails in [
        format!("// {TODO_WORD}: read the rest"),
        format!("// {TODO_WORD}(https://github.com/owner/repo/issues/12): a link"),
        format!("// {TODO_WORD}(#12: not closed"),
        format!("// {TODO_WORD}(): empty"),
        format!("// {FIXME_WORD}(init): only {TODO_WORD} has the marker of init"),
        format!("// {TODO_WORD}(INIT): in capitals"),
    ] {
        assert_eq!(
            found(&fails).first(),
            Some(&("unreferenced-marker", 1, 4)),
            "{fails:?}"
        );
    }
    assert_eq!(
        found(&format!("// {TODO_WORD}(init): open")),
        [("unresolved-init-marker", 1, 4)]
    );
}

#[test]
fn the_pin_init_writes_for_no_release_holds_one_marker() {
    let line =
        format!("    uses: Nebensound/pipeline/.github/workflows/nebensound.yml@{UNRESOLVED_PIN}");

    assert_eq!(found(&line), [("unresolved-init-marker", 1, 77)]);
}

#[test]
fn every_marker_of_a_line_is_judged_where_it_stands() {
    assert_eq!(
        found(&format!(
            "\u{e4}\u{f6} {TODO_WORD}: one, {FIXME_WORD}(#3), {TODO_WORD}(init)\nplain\n{FIXME_WORD}: two"
        )),
        [
            ("unreferenced-marker", 1, 4),
            ("unresolved-init-marker", 1, 26),
            ("unreferenced-marker", 3, 1),
        ]
    );
}

#[test]
fn the_finding_quotes_the_marker_and_names_the_way_out() {
    let path = RelativePath::new("src/lib.rs").unwrap();

    let findings = judge(
        &path,
        &format!("let x = 1; // {TODO_WORD}: count from one\r\n"),
    );

    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].message,
        format!(
            "'{TODO_WORD}: count from one' names no issue that tracks the task. Open an issue and write \
             its number into the marker, as {TODO_WORD}(#123), or {TODO_WORD}(owner/repo#123) for an \
             issue in another repository; or, where the marker has to stay without one, write \
             {ALLOW} and the reason on its line or the line above"
        )
    );
}

#[test]
fn an_allowance_lets_the_markers_of_its_line_and_of_the_line_directly_below_stay() {
    for allowed in [
        format!("// {TODO_WORD}: read the rest // {ALLOW} the format is not settled"),
        format!("// {ALLOW} the format is not settled\n// {TODO_WORD}: read the rest"),
        format!("// {ALLOW} the format is not settled\n// {TODO_WORD}: one, {FIXME_WORD}: two"),
        format!("# {ALLOW} a draft for the owner\n# {TODO_WORD}(init): open"),
        format!("<!-- {ALLOW} a draft -->\n{TODO_WORD}: write it"),
        format!("/* {ALLOW} a draft */ {FIXME_WORD}: write it"),
        format!("// {ALLOW}\tafter a tab\n// {TODO_WORD}: x"),
    ] {
        assert_eq!(found(&allowed), [], "{allowed:?}");
    }
}

#[test]
fn an_allowance_reaches_no_further_than_the_line_directly_below() {
    for between in [
        format!("// {ALLOW} a draft\n\n// {TODO_WORD}: read the rest"),
        format!("// {ALLOW} a draft\n// another comment\n// {TODO_WORD}: read the rest"),
    ] {
        assert_eq!(
            found(&between),
            [("pointless-allowance", 1, 4), ("unreferenced-marker", 3, 4)],
            "{between:?}"
        );
    }
    assert_eq!(
        found(&format!(
            "// {ALLOW} one line\n// {TODO_WORD}: a\n// {TODO_WORD}: b"
        )),
        [("unreferenced-marker", 3, 4)]
    );
    assert_eq!(
        found(&format!(
            "// {ALLOW} stacked\n// {ALLOW} directly above\n// {TODO_WORD}: a"
        )),
        [("pointless-allowance", 1, 4)]
    );
}

#[test]
fn an_allowance_needs_a_reason_with_a_letter_or_a_digit() {
    for (unreasoned, column) in [
        (format!("// {ALLOW}\n// {TODO_WORD}: x"), 4),
        (format!("// {ALLOW}   \n// {TODO_WORD}: x"), 4),
        (format!("// {ALLOW}\r\n// {TODO_WORD}: x\r\n"), 4),
        (format!("<!-- {ALLOW} -->\n{TODO_WORD}: x"), 6),
        (format!("/* {ALLOW} */\n{TODO_WORD}: x"), 4),
        (format!("# {ALLOW} ...\n# {TODO_WORD}: x"), 3),
    ] {
        // The marker stays: the one finding names what is missing.
        assert_eq!(
            found(&unreasoned),
            [("allowance-without-reason", 1, column)],
            "{unreasoned:?}"
        );
    }
    assert_eq!(found(&format!("# {ALLOW} 1\n# {TODO_WORD}: x")), []);
}

#[test]
fn an_allowance_with_no_marker_on_its_line_or_below_fails() {
    assert_eq!(
        found(&format!("// {ALLOW} nothing below\nlet x = 1;\n")),
        [("pointless-allowance", 1, 4)]
    );
    assert_eq!(
        found(&format!("let x = 1;\n// {ALLOW} at the end")),
        [("pointless-allowance", 2, 4)]
    );
    assert_eq!(
        found(&format!("// {ALLOW}\n")),
        [
            ("allowance-without-reason", 1, 4),
            ("pointless-allowance", 1, 4)
        ]
    );
    assert_eq!(
        found(&format!(
            "// {ALLOW} above a referenced one\n// {TODO_WORD}(#4): x"
        )),
        []
    );
}

#[test]
fn the_directive_counts_only_as_a_word_followed_by_white_space_or_the_end() {
    for other in [
        format!("const A: &str = \"{ALLOW}\";"),
        format!("x{ALLOW} a reason"),
        format!("{ALLOW}: a reason"),
        format!("`{ALLOW}` in prose"),
    ] {
        assert_eq!(found(&other), [], "{other:?}");
    }
    assert_eq!(
        found(&format!("x{ALLOW} a reason\n// {TODO_WORD}: x")),
        [("unreferenced-marker", 2, 4)]
    );
}

#[test]
fn a_file_is_text_when_its_start_holds_no_nul_and_it_is_utf_8() {
    let at = |offset: usize| {
        let mut bytes = vec![b'a'; BINARY_PREFIX + 10];
        bytes[offset] = 0;
        bytes
    };

    assert_eq!(marker::text(b""), Some(""));
    assert_eq!(marker::text("caf\u{e9}".as_bytes()), Some("caf\u{e9}"));
    assert_eq!(marker::text(b"\0"), None);
    assert_eq!(marker::text(&at(BINARY_PREFIX - 1)), None);
    assert!(marker::text(&at(BINARY_PREFIX)).is_some());
    assert_eq!(marker::text(b"caf\xe9"), None);
}

#[test]
fn init_s_report_names_every_marker_of_init_allowed_or_not() {
    let text = format!(
        "# {ALLOW} kept\n# {TODO_WORD}(init): one\n# {TODO_WORD}: other\n\n# {TODO_WORD}(init)\n{FIXME_WORD}(init)"
    );

    let markers: Vec<(usize, usize, String)> = marker::init_markers(&text)
        .into_iter()
        .map(|(position, rest)| (position.line, position.column, rest))
        .collect();

    assert_eq!(markers, [(2, 3, "one".to_owned()), (5, 3, String::new())]);
    assert_eq!(marker::init_markers("nothing here"), []);
}

/// The rows of the table in a file whose comments start with `open` and end
/// with `close`: a marker with a reference, here and in another repository,
/// the marker of init, a marker without a reference, a marker with an
/// allowance on its line, one below an allowance, an allowance without a
/// reason and one with nothing below.
fn rows(open: &str, close: &str) -> String {
    [
        format!("{open}{TODO_WORD}(#12): referenced{close}"),
        format!("{open}{FIXME_WORD}(owner/repo#12): referenced in another repository{close}"),
        format!("{open}{TODO_WORD}(init){close}"),
        format!("{open}{TODO_WORD}: no reference{close}"),
        format!("{open}{FIXME_WORD}: allowed on its line {ALLOW} a draft{close}"),
        format!("{open}{ALLOW} the line below is a draft{close}"),
        format!("{open}{TODO_WORD}(init): allowed from above{close}"),
        format!("{open}{ALLOW}{close}"),
        format!("{open}{FIXME_WORD}: under an allowance without a reason{close}"),
        format!("{open}{ALLOW} nothing below{close}"),
        String::new(),
    ]
    .join("\n")
}

#[test]
fn check_holds_every_row_in_rust_markdown_and_yaml() {
    let rust = rows("// ", "");
    let markdown = rows("<!-- ", " -->");
    let yaml = rows("# ", "");
    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("src/lib.rs", &rust),
        ("README.md", &markdown),
        (".github/workflows/ci.yml", &yaml),
    ]);

    let (findings, output) = check(dir.path());

    let mut expected = Vec::new();
    for (path, column) in [
        (".github/workflows/ci.yml", 3),
        ("README.md", 6),
        ("src/lib.rs", 4),
    ] {
        expected.extend([
            format!("error[unresolved-init-marker] at {path}:3:{column}"),
            format!("error[unreferenced-marker] at {path}:4:{column}"),
            format!("error[allowance-without-reason] at {path}:8:{column}"),
            format!("error[pointless-allowance] at {path}:10:{column}"),
        ]);
    }
    assert_eq!(findings, expected, "{output}");
}

#[test]
fn a_file_an_exclusion_takes_out_yields_no_finding() {
    let declaration = format!(
        "{QUIET}\n[[exclude]]\npath = \"vendor\"\nreason = \"code of others, taken over \
         unchanged\"\n"
    );
    let notes = format!("# Notes\n\n{TODO_WORD}: read the rest\n");
    let dir = repository(&[
        (".nebensound/repo.toml", &declaration),
        ("vendor/notes.md", &notes),
        ("notes.md", &notes),
    ]);

    let (findings, output) = check(dir.path());

    assert_eq!(
        findings,
        ["error[unreferenced-marker] at notes.md:3:1"],
        "{output}"
    );
}

#[test]
fn a_file_that_is_binary_or_no_utf_8_is_passed_over() {
    let line = format!("{TODO_WORD}: read the rest\n");
    let dir = repository(&[(".nebensound/repo.toml", QUIET)]);
    let write = |name: &str, bytes: &[u8]| fs::write(dir.path().join(name), bytes).unwrap();
    write("binary.txt", &[b"\0".as_slice(), line.as_bytes()].concat());
    write(
        "latin.txt",
        &[b"caf\xe9\n".as_slice(), line.as_bytes()].concat(),
    );
    let mut late = vec![b'a'; BINARY_PREFIX];
    late.extend(b"\0\n");
    late.extend(line.as_bytes());
    write("late.txt", &late);
    support::git(dir.path(), &["add", "--all"]);

    let (findings, output) = check(dir.path());

    assert_eq!(
        findings,
        ["error[unreferenced-marker] at late.txt:2:1"],
        "{output}"
    );
}
