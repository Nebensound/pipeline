//! What holds `todo!`, `unimplemented!` and `dbg!` beside Clippy: the
//! attributes that lower Clippy's lints for them, and Clippy's configuration.
//!
//! Each source here is Rust as a repository writes it. What Clippy itself does
//! with the lints and these attributes is shown by the cases of the block
//! `check-rust`, which run the real toolchain.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use nebensound_repo::finding::Position;
use nebensound_repo::path::RelativePath;
use nebensound_repo::run::rust::leftovers::{
    self, CONFIGURATION, DBG_IN_TESTS, GROUP, LINTS, Lowering,
};

/// The lines of every attribute `attributes` finds in `source`.
fn lines(source: &str) -> Vec<usize> {
    leftovers::attributes(source)
        .unwrap()
        .iter()
        .map(|lowering| lowering.position.line)
        .collect()
}

/// The one finding in `source`.
fn only(source: &str) -> Lowering {
    let found = leftovers::attributes(source).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    found.into_iter().next().unwrap()
}

#[test]
fn clippy_is_handed_the_three_lints_as_errors() {
    assert_eq!(
        LINTS,
        ["clippy::todo", "clippy::unimplemented", "clippy::dbg_macro"]
    );
    assert_eq!(
        leftovers::denied(),
        [
            "-D",
            "clippy::todo",
            "-D",
            "clippy::unimplemented",
            "-D",
            "clippy::dbg_macro"
        ]
    );
}

#[test]
fn the_exemption_on_one_item_with_a_reason_passes() {
    for source in [
        "#[expect(clippy::todo, reason = \"the parser comes with the next issue\")]\n\
         pub fn parse() { todo!() }\n",
        "#[expect(clippy::unimplemented, reason = r\"never called on this platform\")]\n\
         fn f() { unimplemented!() }\n",
        "#[expect(clippy::dbg_macro, reason = r#\"a \"trace\" for the test\"#)]\n\
         fn f() { dbg!(1); }\n",
        "#[expect(clippy::todo, clippy::dbg_macro, reason = \"two at once\")]\nfn f() {}\n",
        "#[expect(clippy::restriction, reason = \"generated code\")]\nfn f() {}\n",
        // Inside a macro of the repository it stands on the item it expands to.
        "macro_rules! m {\n    () => {\n        #[expect(clippy::todo, reason = \"r\")]\n        \
         fn f() { todo!() }\n    };\n}\n",
        "#[r#expect(clippy::r#todo, reason = \"raw identifiers name the same\")]\nfn f() {}\n",
    ] {
        assert_eq!(lines(source), Vec::<usize>::new(), "{source}");
    }
}

#[test]
fn an_attribute_that_lowers_one_of_them_is_a_finding_on_its_line() {
    // Each level below an error, each lint and the group, whatever stands
    // around them.
    for (source, line) in [
        ("#![allow(clippy::todo)]\npub fn f() { todo!() }\n", 1),
        ("//! A crate.\n\n#![allow(clippy::unimplemented)]\n", 3),
        ("#![warn(clippy::dbg_macro)]\n#![allow(warnings)]\n", 1),
        ("#![allow(clippy::restriction)]\n", 1),
        (
            "#![expect(clippy::todo, reason = \"the whole crate\")]\n",
            1,
        ),
        ("#![allow(clippy :: todo)]\n", 1),
        ("#![r#allow(clippy::r#todo)]\n", 1),
        ("mod inner {\n    #![allow(clippy::todo)]\n}\n", 2),
        ("#[allow(clippy::todo)]\nmod everything;\n", 1),
        ("#[allow(clippy::todo)]\nfn f() { todo!() }\n", 1),
        (
            "#[allow(clippy::todo, reason = \"allow is no exemption\")]\nfn f() {}\n",
            1,
        ),
        (
            "fn f() {\n    #[allow(clippy::dbg_macro)]\n    dbg!(1);\n}\n",
            2,
        ),
        ("#![cfg_attr(test, allow(clippy::dbg_macro))]\n", 1),
        (
            "#[cfg_attr(test, expect(clippy::dbg_macro, reason = \"only in a cfg_attr\"))]\n\
             fn f() {}\n",
            1,
        ),
        (
            "#[cfg_attr(all(), cfg_attr(all(), allow(clippy::todo)))]\nfn f() {}\n",
            1,
        ),
        (
            "macro_rules! m {\n    () => {\n        #[allow(clippy::todo)]\n        \
             fn f() { todo!() }\n    };\n}\n",
            3,
        ),
        (
            "macro_rules! m {\n    ($a:meta) => { #[$a] fn f() { todo!() } };\n}\n\
             m!(allow(clippy::todo));\n",
            4,
        ),
        ("#[allow(dead_code, clippy::todo)]\nfn f() {}\n", 1),
        // The attribute and something more in one pair of brackets is no
        // attribute Rust reads as the exemption.
        (
            "#[expect(clippy::todo, reason = \"r\"), allow(dead_code)]\nfn f() {}\n",
            1,
        ),
    ] {
        assert_eq!(lines(source), [line], "{source}");
    }
}

#[test]
fn an_expectation_without_a_reason_is_no_exemption() {
    for source in [
        "#[expect(clippy::todo)]\nfn f() { todo!() }\n",
        "#[expect(clippy::todo, reason = \"\")]\nfn f() {}\n",
        "#[expect(clippy::todo, reason = \" -- \")]\nfn f() {}\n",
        "#[expect(clippy::todo, reason = 1)]\nfn f() {}\n",
        "#[expect(clippy::todo, reason)]\nfn f() {}\n",
        "#[expect(clippy::todo, because = \"a reason by another name\")]\nfn f() {}\n",
        "#[expect(clippy::todo, reason == \"r\")]\nfn f() {}\n",
    ] {
        let lowering = only(source);
        assert_eq!(lowering.position, Position { line: 1, column: 3 });
        assert_eq!(
            lowering.message,
            "#[expect(clippy::todo)] gives no reason: the exemption of one item needs \
             reason = \"…\"",
            "{source}"
        );
    }
}

#[test]
fn the_message_says_what_the_attribute_does_and_the_way_out() {
    let inner = only("pub mod a {\n  #![expect(clippy::todo, clippy::dbg_macro)]\n}\n");
    assert_eq!(inner.position, Position { line: 2, column: 6 });
    assert_eq!(
        inner.message,
        "#![expect(clippy::todo, clippy::dbg_macro)] covers the whole crate or module, and the \
         Rust check holds todo!, unimplemented! and dbg! as errors there; exempt the one item \
         with #[expect(…, reason = \"…\")] on it"
    );
    let elsewhere = only("#[cfg_attr(test, allow(clippy::restriction))]\nfn f() {}\n");
    assert_eq!(
        elsewhere.position,
        Position {
            line: 1,
            column: 18
        }
    );
    assert_eq!(
        elsewhere.message,
        "allow(clippy::restriction) lowers what the Rust check holds as errors; exempt the one \
         item with #[expect(…, reason = \"…\")] on it, written as it stands"
    );
    assert_eq!(GROUP, "clippy::restriction");
}

#[test]
fn what_lowers_nothing_of_the_three_is_no_finding() {
    for source in [
        // Other lints, and other levels.
        "#![allow(clippy::all, clippy::pedantic, warnings)]\n",
        "#![deny(clippy::todo)]\n#![forbid(clippy::dbg_macro)]\n",
        "#[allow(clippy::todo_like, clippy::todo::more, rustc::todo, todo)]\nfn f() {}\n",
        "#[allow(clippy::panic, clippy::unimplemented_thing)]\nfn f() {}\n",
        // Not an attribute's list.
        "#[allow = \"clippy::todo\"]\nfn f() {}\n",
        "#[allow[clippy::todo]]\nfn f() {}\n",
        "fn f() { allow }\n",
        // In a comment or a string, and in a doc comment.
        "// #![allow(clippy::todo)]\n/* #[allow(clippy::dbg_macro)] */\nfn f() {}\n",
        "const S: &str = \"#![allow(clippy::todo)]\";\n",
        "/// ```\n/// #![allow(clippy::todo)]\n/// ```\nfn f() {}\n",
        // The calls themselves are Clippy's to report.
        "fn f() { todo!(); unimplemented!(); dbg!(1); }\n",
        "",
    ] {
        assert_eq!(lines(source), Vec::<usize>::new(), "{source}");
    }
}

#[test]
fn a_source_without_tokens_rust_could_read_is_left_to_clippy() {
    assert_eq!(leftovers::attributes("fn f() { (\n"), None);
    assert_eq!(leftovers::attributes("\"never closed\n"), None);
}

#[test]
fn clippys_configuration_that_lets_dbg_pass_in_tests_is_found() {
    assert_eq!(DBG_IN_TESTS, "allow-dbg-in-tests");
    assert_eq!(CONFIGURATION, ["clippy.toml", ".clippy.toml"]);
    assert_eq!(
        leftovers::configuration("msrv = \"1.85\"\nallow-dbg-in-tests = true\n"),
        Some(Position {
            line: 2,
            column: 22
        })
    );
    for source in [
        "",
        "allow-dbg-in-tests = false\n",
        "allow-dbg-in-tests = \"true\"\n",
        "allow-unwrap-in-tests = true\nallow_dbg_in_tests = true\n",
        "allow-dbg-in-tests = \n",
        "[section]\nallow-dbg-in-tests = true\n",
    ] {
        assert_eq!(leftovers::configuration(source), None, "{source}");
    }
}

#[test]
fn clippy_reads_its_configuration_from_two_names() {
    for (path, read) in [
        ("clippy.toml", true),
        (".clippy.toml", true),
        ("crates/a/clippy.toml", true),
        ("config/.clippy.toml", true),
        ("Clippy.toml", false),
        ("clippy.toml.bak", false),
        ("clippy/rules.toml", false),
    ] {
        assert_eq!(
            leftovers::is_configuration(&RelativePath::new(path).unwrap()),
            read,
            "{path}"
        );
    }
}
