//! The test cases of the building blocks, and running a block against one.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use support::{Scene, write};
use xtask::cases::{self, Expect};

#[test]
fn cases_are_found_passing_first_each_kind_by_name() {
    let scene = Scene::new();
    let block = scene.path().join("check-demo");
    write(
        &block,
        &[
            ("tests/pass/b/file", ""),
            ("tests/pass/a/file", ""),
            ("tests/pass/.DS_Store", ""),
            ("tests/fail/.DS_Store", ""),
            (
                "tests/fail/z/expected-finding",
                "# A comment.\n\n  first *\nsecond\n",
            ),
        ],
    );
    let found = cases::discover(&block).unwrap();
    let names: Vec<(&str, &Expect)> = found
        .iter()
        .map(|case| (case.name.as_str(), &case.expect))
        .collect();
    let fail = Expect::Fail(vec!["first *".to_owned(), "second".to_owned()]);
    assert_eq!(
        names,
        [("a", &Expect::Pass), ("b", &Expect::Pass), ("z", &fail)]
    );
    assert_eq!(found[0].label(), "passing case `a`");
    assert_eq!(found[2].label(), "failing case `z`");
    assert!(
        cases::discover(&scene.path().join("no block"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn what_is_no_case_is_refused() {
    for (files, message) in [
        (
            &[("tests/pass/stray", "")][..],
            "is no case: a case is a directory",
        ),
        (&[("tests/fail/a/file", "")], "names no finding"),
        (
            &[("tests/fail/a/expected-finding", "# only a comment\n")],
            "names no finding",
        ),
        (
            &[("tests/pass/a/expected-finding", "x")],
            "only a failing case names a finding",
        ),
        (&[("tests/pass", "")], "cannot list"),
    ] {
        let scene = Scene::new();
        write(scene.path(), files);
        let error = cases::discover(scene.path()).unwrap_err();
        assert!(error.to_string().contains(message), "{files:?}: {error}");
    }
}

#[test]
fn a_pattern_matches_a_whole_line() {
    for (pattern, line, expected) in [
        ("exact", "exact", true),
        ("exact", "exact and more", false),
        ("exact", "not exact", false),
        ("start*", "start and more", true),
        ("*end", "the end", true),
        ("*end", "the end?", false),
        ("*middle*", "in the middle of it", true),
        ("a*b*c", "a-b-c", true),
        ("a*b*c", "a-c-b", false),
        ("a*b*c", "abc", true),
        ("a*b*b", "ab", false),
        ("a*x*c", "abc", false),
        ("*", "", true),
        ("**", "anything", true),
        ("", "", true),
        ("", "x", false),
    ] {
        assert_eq!(
            cases::matches(pattern, line),
            expected,
            "{pattern:?} on {line:?}"
        );
    }
}

#[test]
fn a_run_is_judged_against_what_the_case_expects() {
    let log = ["one line".to_owned(), "::error file=a::wrong".to_owned()];
    assert_eq!(cases::judge(&Expect::Pass, true, &log), None);
    assert!(
        cases::judge(&Expect::Pass, false, &log)
            .unwrap()
            .contains("meant to pass")
    );
    let fail = Expect::Fail(vec![
        "::error file=a::*".to_owned(),
        "missing*".to_owned(),
        "other".to_owned(),
    ]);
    assert!(
        cases::judge(&fail, true, &log)
            .unwrap()
            .contains("lets the mistake through")
    );
    assert_eq!(
        cases::judge(&fail, false, &log).unwrap(),
        "the block fails, but without the finding the case names: no line matches `missing*`, `other`"
    );
    assert_eq!(
        cases::judge(&Expect::Fail(vec!["one *".to_owned()]), false, &log),
        None
    );
}

#[test]
fn the_plan_names_the_units_of_a_check() {
    let plan = r#"{"units":[
        {"path":".","checks":["json","toml"]},
        {"path":"tools","checks":["rust"]},
        {"path":"other","checks":["json"]},
        {"path":"none"},
        {"checks":["json"]}
    ]}"#;
    assert_eq!(cases::units(plan, "json").unwrap(), [".", "other"]);
    assert!(cases::units(plan, "yaml").unwrap().is_empty());
    assert!(
        cases::units("not json", "json")
            .unwrap_err()
            .to_string()
            .contains("no JSON")
    );
    assert!(
        cases::units("{}", "json")
            .unwrap_err()
            .to_string()
            .contains("names no units")
    );
}

#[test]
fn the_plan_names_the_units_of_a_version_writer() {
    let plan = r#"{"versions":[
        {"path":".","writers":[]},
        {"path":"lib","writers":["cargo"]},
        {"path":"doc","writers":["asciidoc","cargo"]},
        {"path":"none"},
        {"writers":["cargo"]}
    ]}"#;
    assert_eq!(cases::writers(plan, "cargo").unwrap(), ["lib", "doc"]);
    assert!(cases::writers(plan, "other").unwrap().is_empty());
    assert!(
        cases::writers("not json", "cargo")
            .unwrap_err()
            .to_string()
            .contains("no JSON")
    );
    assert!(
        cases::writers("{}", "cargo")
            .unwrap_err()
            .to_string()
            .contains("names no versions")
    );
}

#[test]
fn the_plan_names_the_units_of_a_build_with_their_writers() {
    let plan = r#"{"units":[
        {"kind":"repository","name":null,"path":".","build":"demo"},
        {"kind":"component","name":"lib","path":"lib","build":"demo"},
        {"kind":"component","name":"doc","path":"doc","build":"demo"},
        {"kind":"component","name":"odd","path":"odd","build":"demo"},
        {"kind":"component","name":"other","path":"other","build":"other"},
        {"kind":"component","name":"none","path":"none","build":null},
        {"kind":"component","name":"lost","build":"demo"},
        {"kind":"component","name":"untouched","path":"untouched","build":"demo"},
        {"kind":"component","name":"unversioned","path":"unversioned","build":"demo"}
    ],"versions":[
        {"kind":"repository","name":null,"version":"1.0.0","writers":[]},
        {"kind":"component","name":"lib","version":"0.2.0-pr.7","writers":["cargo"]},
        {"kind":"component","name":"odd","version":"0.1.0","writers":[1, "cargo"]},
        {"kind":"component","name":"other","version":"0.1.0","writers":["cargo"]},
        {"kind":"component","name":"doc","version":"0.1.0"},
        {"kind":"component","name":"lost","version":"0.1.0","writers":[]},
        {"kind":"component","name":"untouched","version":null,"writers":["cargo"]}
    ]}"#;
    let built = |path: &str, writers: &[&str]| cases::Built {
        path: path.to_owned(),
        writers: writers.iter().map(|writer| (*writer).to_owned()).collect(),
    };
    // `untouched` has no version of the run and `unversioned` no entry under
    // `versions` at all: neither builds.
    assert_eq!(
        cases::builds(plan, "demo").unwrap(),
        [
            built(".", &[]),
            built("lib", &["cargo"]),
            built("doc", &[]),
            built("odd", &["cargo"]),
        ]
    );
    assert!(
        cases::builds(r#"{"units":[{"path":"x","build":"demo"}]}"#, "demo")
            .unwrap()
            .is_empty()
    );
    assert!(cases::builds(plan, "absent").unwrap().is_empty());
    assert!(
        cases::builds("not json", "demo")
            .unwrap_err()
            .to_string()
            .contains("no JSON")
    );
    assert!(
        cases::builds("{}", "demo")
            .unwrap_err()
            .to_string()
            .contains("names no units")
    );
}
