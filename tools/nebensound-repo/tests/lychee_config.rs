//! lychee's configuration as the link check hands it over: which keys it
//! carries over, which it refuses, and how it reads `.lycheeignore`. How the
//! checks use it is tested in `run.rs`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use nebensound_repo::lychee_config::{
    CARRIED, EXCLUDE_PATH, REFUSED, exclude_path, hand_over, ignore_lines,
};

#[test]
fn every_key_is_carried_over_refused_or_applied_by_the_tool_once() {
    let refused: Vec<&str> = REFUSED.iter().map(|(key, _)| *key).collect();
    for list in [CARRIED, refused.as_slice()] {
        assert!(list.windows(2).all(|pair| pair[0] < pair[1]), "{list:?}");
    }
    for key in CARRIED {
        assert!(!refused.contains(key), "{key}");
    }
    assert!(!CARRIED.contains(&EXCLUDE_PATH));
    assert!(!refused.contains(&EXCLUDE_PATH));
    // The keys lychee 0.24.2 reads from its configuration file, the version
    // the register pins: 46 carried over, 14 refused, and `exclude_path`.
    assert_eq!(CARRIED.len() + refused.len() + 1, 61);
}

#[test]
fn an_empty_configuration_hands_over_nothing() {
    let handed = hand_over("", "").unwrap();
    assert!(handed.settings.is_empty());
    assert!(handed.exclude_path.is_none());
}

#[test]
fn the_lycheeignore_comes_behind_the_own_exclude() {
    let handed = hand_over(
        "exclude = ['a']\nexclude_path = ['^docs/']\ntimeout = 5\n",
        "b\n#c\n  # d\n\n e \n",
    )
    .unwrap();
    assert_eq!(
        handed.settings.to_string(),
        "exclude = [\"a\", \"b\", \" e \"]\ntimeout = 5\n"
    );
    let set = handed.exclude_path.unwrap();
    assert!(set.is_match("docs/a.md"));
    assert!(!set.is_match("a.md"));
}

#[test]
fn every_reason_is_named() {
    let reasons = hand_over(
        "cache = true\nexclude = 1\nexclude_path = 1\nwhatever = 1\n",
        "",
    )
    .unwrap_err();
    assert_eq!(
        reasons,
        [
            "`cache` is not handed to lychee: it makes lychee read and write a cache file",
            "`whatever` is not a setting the link check knows, so it is not handed to lychee",
            "`exclude_path` is not a list of text",
            "`exclude` is not a list of text",
        ]
    );
}

#[test]
fn exclude_path_is_read_as_lychee_builds_it() {
    let table = |source: &str| -> toml::Table { toml::from_str(source).unwrap() };
    assert!(exclude_path(&table("")).unwrap().is_none());
    assert!(exclude_path(&table("exclude_path = ['(']")).is_err());
    assert_eq!(
        exclude_path(&table("exclude_path = ['a', 2]")).unwrap_err(),
        "`exclude_path` is not a list of text"
    );
}

#[test]
fn a_lycheeignore_is_read_line_by_line_as_lychee_reads_it() {
    assert_eq!(
        ignore_lines("a\r\n\n# b\n\t# c\n d\n"),
        ["a".to_owned(), " d".to_owned()]
    );
}
