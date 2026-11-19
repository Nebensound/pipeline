//! `docs/register.md` is the page `nebensound register` prints.
//!
//! The page is compared as text: it is printed the way Prettier writes it, so
//! the committed copy and the printed one are the same bytes. The page says how
//! to write it again.
//!
//! This lives in the tool and not in the library: the library is published,
//! and in its package the page is not there to be read.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;
use std::path::Path;

use nebensound::output::Output;
use nebensound_declaration::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER};
use support::call;

const COMMITTED: &str = "../../docs/register.md";

/// One call against a given register and given defaults.
fn call_against(register: &str, defaults: &str) -> (String, u8) {
    let mut out = Vec::new();
    let code = nebensound::run_against(
        ["nebensound", "register"],
        register,
        defaults,
        Output::Elsewhere,
        &mut out,
    );
    (String::from_utf8(out).expect("the output is UTF-8"), code)
}

#[test]
fn the_committed_page_is_the_generated_one() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(COMMITTED);
    let committed = fs::read_to_string(&path).expect("the committed page can be read");

    let (text, code) = call(&["nebensound", "register"]);

    assert_eq!(code, 0, "{text}");
    assert!(
        text == committed,
        "{COMMITTED} is not the page the register prints; write it again from tools/nebensound/ \
         with `cargo run --quiet -- register > ../../docs/register.md`"
    );
}

#[test]
fn the_page_reads_no_repository() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let nowhere = dir.path().join("nothing here");
    let nowhere = nowhere.to_str().expect("a path in UTF-8");

    let (text, code) = call(&["nebensound", "-C", nowhere, "register"]);

    assert_eq!(code, 0, "{text}");
    assert!(text.starts_with("# Register\n"), "{text}");
}

/// Every table row of the page, its cells without the padding.
fn rows(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .filter(|line| line.starts_with('|'))
        .map(|line| {
            line.trim_matches('|')
                .split(" | ")
                .map(|cell| cell.trim().to_owned())
                .collect()
        })
        .collect()
}

#[test]
fn the_page_names_what_the_register_says() {
    let (text, _) = call(&["nebensound", "register"]);
    let rows = rows(&text);

    for expected in [
        &["Checks", "`markdown`, `yaml`, `json`, `toml`"][..],
        &[
            "`rust-library-public`",
            "`cargo`",
            "`rust`",
            "`cargo-package`",
            "`crates.io`",
            "`cargo`",
        ],
        &["`none`", "none", "none", "none", "nothing", "none"],
        &[
            "`dockerfile`",
            "`.dockerfile`",
            "`Dockerfile`, `Containerfile`",
            "none",
        ],
        &[
            "`json`",
            "`.json`, `.jsonc`, `.geojson`",
            "`composer.lock`, `flake.lock`",
            "`biome`, `jq`, `prettier`",
        ],
        &[
            "`prettier`",
            "`3.9.6`",
            "`node`",
            "one package for every platform",
        ],
        &[
            "`jq`",
            "`1.7.1`",
            "none",
            "`linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`",
        ],
        &[
            "`python`",
            "Python",
            "`3.12.16`",
            "`pip`",
            "`linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, \
             `windows-aarch64`",
        ],
        &["`docker`", "`container`"],
        &["`crates.io`", "`crate`", "`rust`", "`two-stage`"],
        &["`ghcr`", "`container`", "none", "`single-stage`"],
        &[
            "`cargo`",
            "the package version in the unit's Cargo.toml, written through Cargo",
        ],
        &["`github-actions`", "`github-actions`", "none"],
        &["`**/LICENSE`", "legal text, taken over unchanged"],
    ] {
        assert!(
            rows.iter().any(|row| row == expected),
            "no row {expected:?}:\n{text}"
        );
    }
}

#[test]
fn every_table_is_aligned_the_way_prettier_aligns_it() {
    let (text, _) = call(&["nebensound", "register"]);

    let mut table: Vec<&str> = Vec::new();
    for line in text.lines().chain([""]) {
        if line.starts_with('|') {
            table.push(line);
            continue;
        }
        if let Some(first) = table.first() {
            let width = first.chars().count();
            for row in &table {
                assert_eq!(row.chars().count(), width, "{row:?} is out of line");
            }
            assert!(
                table[1].chars().all(|c| matches!(c, '|' | '-' | ' ')),
                "{:?} is no rule",
                table[1]
            );
        }
        table.clear();
    }
}

#[test]
fn a_bar_in_a_cell_does_not_end_it() {
    let register = BUILTIN_REGISTER.replace(
        "written through Cargo",
        "written through Cargo | not by hand",
    );

    let (text, code) = call_against(&register, BUILTIN_DEFAULTS);

    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("written through Cargo \\| not by hand |"),
        "{text}"
    );
}

#[test]
fn broken_built_in_data_says_so() {
    let (text, code) = call_against("this is not a register", "[repo]\n");

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("built-in register"), "{text}");
    assert!(!text.contains("# Register"), "{text}");
}

#[test]
fn the_call_says_what_it_prints() {
    let (text, code) = call(&["nebensound", "register", "--help"]);

    assert_eq!(code, 0, "{text}");
    assert!(text.contains("docs/register.md"), "{text}");
}
