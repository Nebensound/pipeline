//! A version as a declaration writes it: three numbers, nothing else.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use nebensound_declaration::Version;
use nebensound_declaration::version::VersionError;

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
