//! A version as a declaration writes it, as a tag carries it, and how a commit
//! message raises one.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use nebensound_repo::Version;
use nebensound_repo::version::{Change, TagVersion, Unconventional, VersionError, change_of};

#[test]
fn three_numbers_read_as_a_version() {
    for (text, expected) in [
        ("0.0.0", Version::new(0, 0, 0)),
        ("0.1.0", Version::new(0, 1, 0)),
        ("1.0.0", Version::new(1, 0, 0)),
        ("10.20.30", Version::new(10, 20, 30)),
        ("18446744073709551615.0.0", Version::new(u64::MAX, 0, 0)),
    ] {
        let version = Version::parse(text).unwrap_or_else(|error| panic!("{text}: {error}"));
        assert_eq!(version, expected);
        assert_eq!(version.to_string(), text, "it reads back as written");
    }
}

#[test]
fn anything_but_three_numbers_is_refused() {
    for text in [
        "",
        "1",
        "1.0",
        "1.0.0.0",
        "v1.0.0",
        "1.0.0-pre.1",
        "1.0.0+build",
        "1..0",
        ".1.0",
        "1.0.",
        "01.0.0",
        "1.00.0",
        "1.0.01",
        "-1.0.0",
        "+1.0.0",
        "1.a.0",
        " 1.0.0",
        "1.0.0 ",
        "١.0.0",
    ] {
        assert_eq!(
            Version::parse(text),
            Err(VersionError::Shape {
                value: text.to_owned()
            }),
            "{text:?}"
        );
    }
    let message = Version::parse("v1.0.0").unwrap_err().to_string();
    assert!(message.contains("X.Y.Z"), "{message}");
    assert!(message.contains("\"v1.0.0\""), "{message}");
}

#[test]
fn a_number_beyond_counting_is_refused() {
    let text = "1.18446744073709551616.0";
    let error = Version::parse(text).unwrap_err();

    assert_eq!(
        error,
        VersionError::TooLarge {
            value: text.to_owned()
        }
    );
    assert!(error.to_string().contains("too large"), "{error}");
}

#[test]
fn versions_are_ordered_number_by_number() {
    let parse = |text: &str| Version::parse(text).expect("a version");

    assert!(parse("0.9.0") < parse("0.10.0"), "not by text");
    assert!(parse("0.10.9") < parse("1.0.0"));
    assert!(parse("1.2.3") < parse("1.2.4"));
}

fn change(message: &str) -> Change {
    change_of(message).unwrap_or_else(|error| panic!("{message:?}: {error}"))
}

#[test]
fn the_type_decides_between_patch_and_minor() {
    assert_eq!(change("fix: a correction"), Change::Patch);
    assert_eq!(change("feat: something new"), Change::Minor);
    assert_eq!(change("FEAT: the type ignores case"), Change::Minor);
    assert_eq!(change("docs: every other type"), Change::Patch);
    assert_eq!(change("chore(deps): with a scope"), Change::Patch);
    assert_eq!(change("feat(plan): with a scope"), Change::Minor);
}

#[test]
fn a_breaking_change_is_major() {
    assert_eq!(change("fix!: marked in the header"), Change::Major);
    assert_eq!(change("refactor(tool)!: with a scope"), Change::Major);
    assert_eq!(
        change("fix: a correction\n\nBody.\n\nBREAKING CHANGE: the plan reads differently"),
        Change::Major
    );
    assert_eq!(
        change("feat: something\n\nBREAKING-CHANGE: with a hyphen"),
        Change::Major
    );
}

#[test]
fn a_breaking_footer_counts_only_as_written() {
    // Lower case, or on the header line, is not the footer token.
    assert_eq!(
        change("fix: a correction\n\nbreaking change: not the token"),
        Change::Patch
    );
    assert_eq!(
        change("feat: mentions BREAKING CHANGE: in the header"),
        Change::Minor
    );
}

#[test]
fn a_message_that_is_not_conventional_is_refused_with_its_header() {
    for message in [
        "",
        "Initial commit",
        "Merge pull request #1 from somewhere",
        "fix:no space after the colon",
        "fix: ",
        ": no type",
        "fix up: two words",
        "fix(): an empty scope",
        "fix(plan: an open scope",
        "fix(a(b)): a nested scope",
        "fix(plan)x: something behind the scope",
        "Revert \"feat: something\"",
    ] {
        let refused = change_of(message).expect_err(message);
        let header = message.lines().next().unwrap_or_default();
        assert_eq!(
            refused,
            Unconventional {
                header: header.to_owned()
            }
        );
        assert!(refused.to_string().contains("not a Conventional Commit"));
    }
}

#[test]
fn changes_are_ordered_by_the_field_they_raise() {
    assert!(Change::Patch < Change::Minor && Change::Minor < Change::Major);
}

#[test]
fn below_0_1_0_every_change_is_a_patch() {
    for change in [Change::Patch, Change::Minor, Change::Major] {
        assert_eq!(Version::ZERO.raised(change), Some(Version::new(0, 0, 1)));
        assert_eq!(
            Version::new(0, 0, 7).raised(change),
            Some(Version::new(0, 0, 8))
        );
    }
}

#[test]
fn from_0_1_0_to_below_1_0_0_a_breaking_change_raises_the_minor_version() {
    for (base, raised) in [
        (Version::new(0, 1, 0), Version::new(0, 2, 0)),
        (Version::new(0, 3, 2), Version::new(0, 4, 0)),
        (Version::new(0, 99, 7), Version::new(0, 100, 0)),
    ] {
        assert_eq!(base.raised(Change::Major), Some(raised), "{base}");
    }
    // Everything else is a patch, a feature included.
    for change in [Change::Patch, Change::Minor] {
        assert_eq!(
            Version::new(0, 3, 2).raised(change),
            Some(Version::new(0, 3, 3))
        );
    }
    // No change leads to 1.0.0: that step is taken with minimum-version.
    assert_eq!(
        Version::new(0, u64::MAX, 0).raised(Change::Major),
        None,
        "a full minor field does not spill into 1.0.0"
    );
}

#[test]
fn from_1_0_0_on_each_change_raises_its_field() {
    let base = Version::new(1, 2, 3);
    assert_eq!(base.raised(Change::Patch), Some(Version::new(1, 2, 4)));
    assert_eq!(base.raised(Change::Minor), Some(Version::new(1, 3, 0)));
    assert_eq!(base.raised(Change::Major), Some(Version::new(2, 0, 0)));

    let first = Version::new(1, 0, 0);
    assert_eq!(first.raised(Change::Patch), Some(Version::new(1, 0, 1)));
    assert_eq!(first.raised(Change::Minor), Some(Version::new(1, 1, 0)));
    assert_eq!(first.raised(Change::Major), Some(Version::new(2, 0, 0)));
}

#[test]
fn a_field_at_its_largest_number_cannot_be_raised() {
    let max = u64::MAX;
    assert_eq!(Version::new(0, 0, max).raised(Change::Major), None);
    assert_eq!(Version::new(1, 2, max).raised(Change::Patch), None);
    assert_eq!(Version::new(1, max, 3).raised(Change::Minor), None);
    assert_eq!(Version::new(max, 2, 3).raised(Change::Major), None);
    // The fields a change resets do not stand in its way.
    assert_eq!(
        Version::new(1, max, max).raised(Change::Major),
        Some(Version::new(2, 0, 0))
    );
}

#[test]
fn a_tag_version_reads_and_writes_strictly() {
    for (text, expected) in [
        ("0.0.1", TagVersion::stable(Version::new(0, 0, 1))),
        ("1.2.3", TagVersion::stable(Version::new(1, 2, 3))),
        ("1.2.3-pre.1", TagVersion::pre(Version::new(1, 2, 3), 1)),
        ("1.2.3-pre.10", TagVersion::pre(Version::new(1, 2, 3), 10)),
    ] {
        assert_eq!(TagVersion::parse(text), Some(expected), "{text}");
        assert_eq!(expected.to_string(), text);
    }
    assert!(TagVersion::pre(Version::ZERO, 1).is_pre());
    assert!(!TagVersion::stable(Version::ZERO).is_pre());

    for text in [
        "",
        "1.2",
        "v1.2.3",
        "1.2.3-pre",
        "1.2.3-pre.",
        "1.2.3-pre.0",
        "1.2.3-pre.01",
        "1.2.3-pre.1a",
        "1.2.3-pre.+1",
        "1.2.3-pre.1.1",
        "1.2.3-rc.1",
        "1.2.3-pre1",
        "1.2.3+build",
        "1.2-pre.1",
        "1.2.3-pre.99999999999999999999",
    ] {
        assert_eq!(TagVersion::parse(text), None, "{text}");
    }
}

#[test]
fn a_tag_version_says_why_it_does_not_read() {
    let refused = |text: &str| TagVersion::read(text).expect_err(text);

    for text in [
        "",
        "1.2",
        "v1.2.3",
        "1.2.3-",
        "1.2.3-pre.",
        "1.2.3-pre.0",
        "1.2-pre.1",
    ] {
        let error = refused(text);
        assert!(
            matches!(error, VersionError::TagShape { .. }),
            "{text}: {error:?}"
        );
        assert!(error.to_string().contains("X.Y.Z-pre.N"), "{error}");
    }
    for (text, identifier) in [
        ("1.2.3-rc.1", "rc.1"),
        ("1.2.3-pre1", "pre1"),
        ("1.2.3-pre", "pre"),
        ("1.0.0-alpha", "alpha"),
    ] {
        let error = refused(text);
        assert_eq!(
            error,
            VersionError::PreRelease {
                value: text.to_owned(),
                identifier: identifier.to_owned(),
            }
        );
        assert!(error.to_string().contains("pre.N"), "{error}");
    }
    for text in ["99999999999999999999.0.0", "1.0.0-pre.99999999999999999999"] {
        assert_eq!(
            refused(text),
            VersionError::TooLarge {
                value: text.to_owned()
            }
        );
    }
    assert_eq!(
        TagVersion::read("1.0.0-pre.3"),
        Ok(TagVersion::pre(Version::new(1, 0, 0), 3))
    );
}

#[test]
fn a_pre_release_comes_before_its_full_release() {
    let parse = |text: &str| TagVersion::parse(text).expect("a tag version");

    assert!(parse("1.0.0-pre.1") < parse("1.0.0-pre.2"));
    assert!(parse("1.0.0-pre.2") < parse("1.0.0-pre.10"), "not by text");
    assert!(parse("1.0.0-pre.10") < parse("1.0.0"));
    assert!(parse("1.0.0") < parse("1.0.1-pre.1"));
    assert!(parse("0.9.9") < parse("1.0.0-pre.1"));
    assert_eq!(
        parse("1.0.0").partial_cmp(&parse("1.0.0")),
        Some(std::cmp::Ordering::Equal)
    );
}
