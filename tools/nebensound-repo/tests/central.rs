//! Patterns, defaults, and how the two are resolved against each other.

// A test that cannot read its own defaults has nothing to say.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use nebensound_repo::names::{Extension, Reason};
use nebensound_repo::path::RelativePath;
use nebensound_repo::pattern::{Pattern, PatternError};
use nebensound_repo::{Central, Declaration, Ruling, Tree};

fn path(value: &str) -> RelativePath {
    RelativePath::new(value).expect("a path")
}

const REGISTER: &str = "[version]\n[check.rust]\nextensions = [\".rs\"]\n[build]\n[target]\n[preset]\n\
                        [ecosystem.cargo]\npackage-ecosystem = \"cargo\"\n\
                        manifests = [\"Cargo.toml\"]\n";

#[test]
fn a_pattern_is_a_type_a_name_or_a_path() {
    assert_eq!(
        Pattern::new("*.png").expect("a type"),
        Pattern::Type(Extension::new(".png").expect("an extension"))
    );
    assert_eq!(
        Pattern::new("**/LICENSE").expect("a name"),
        Pattern::Name("LICENSE".to_owned())
    );
    assert_eq!(
        Pattern::new("crates/foo").expect("a path"),
        Pattern::Path(path("crates/foo"))
    );

    assert!(matches!(
        Pattern::new("*.pn/g"),
        Err(PatternError::Type { .. })
    ));
    assert!(matches!(
        Pattern::new("**/a/b"),
        Err(PatternError::Name { .. })
    ));
    assert!(matches!(
        Pattern::new("**/"),
        Err(PatternError::Name { .. })
    ));
    assert!(matches!(Pattern::new(""), Err(PatternError::Path { .. })));

    for message in [
        Pattern::new("*.pn/g").unwrap_err().to_string(),
        Pattern::new("**/a/b").unwrap_err().to_string(),
        Pattern::new("").unwrap_err().to_string(),
    ] {
        assert!(!message.is_empty(), "every refusal explains itself");
    }
}

#[test]
fn a_pattern_matches_and_writes_itself_back() {
    let png = Pattern::new("*.png").expect("a type");
    let license = Pattern::new("**/LICENSE").expect("a name");
    let vendor = Pattern::new("vendor").expect("a path");

    assert!(png.matches(&path("design/logo.png")));
    assert!(!png.matches(&path("design/logo.svg")));
    assert!(license.matches(&path("crates/a/LICENSE")));
    assert!(!license.matches(&path("LICENSE.md")));

    // A name matches every segment, not only the last one: otherwise the form
    // would be useless for the case it is written for.
    let modules = Pattern::new("**/node_modules").expect("a name");
    assert!(modules.matches(&path("assets/node_modules/ms/index.js")));
    assert!(modules.matches(&path("node_modules/x.js")));
    assert!(!modules.matches(&path("assets/node_modules_alt/x.js")));

    // A folder needs no pattern for that: a path contains by segment.
    assert!(vendor.matches(&path("vendor/thing.rs")));
    assert!(!vendor.matches(&path("vendored/thing.rs")));

    assert_eq!(png.to_string(), "*.png");
    assert_eq!(license.to_string(), "**/LICENSE");
    assert_eq!(vendor.to_string(), "vendor");

    assert!(png.is_wildcard());
    assert!(license.is_wildcard());
    assert!(!vendor.is_wildcard(), "a path is not a pattern");

    // A concrete path beats every pattern, a file name beats a type.
    assert!(vendor.specificity() > license.specificity());
    assert!(license.specificity() > png.specificity());
    assert!(
        path("a/b/c").segments().len() > path("a").segments().len(),
        "and a longer path a shorter one"
    );
}

#[test]
fn the_more_specific_entry_wins_and_on_a_tie_the_inclusion() {
    let central = Central::parse(
        REGISTER,
        "[repo]\npipeline = { check = [] }\n\n[[exclude]]\npath = \"*.png\"\nreason = \"image\"\n",
    )
    .expect("the defaults");

    // Excused centrally, nothing said locally.
    let quiet = Declaration::parse("[repo]\n").expect("a declaration");
    assert!(matches!(
        central.ruling(&quiet, &path("design/logo.png"), &Tree::default()),
        Ruling::Excluded { .. }
    ));

    // Equal specificity: the inclusion wins.
    let taken_back =
        Declaration::parse("[[include]]\npath = \"*.png\"\n\n[repo]\n").expect("a declaration");
    assert_eq!(
        central.ruling(&taken_back, &path("design/logo.png"), &Tree::default()),
        Ruling::Checked
    );

    // The concrete path beats the pattern — in the other direction as well.
    let deeper = Declaration::parse(
        "[[include]]\npath = \"*.png\"\n\n[repo]\n\n[[exclude]]\npath = \"design\"\nreason = \"design material\"\n",
    )
    .expect("a declaration");
    assert!(matches!(
        central.ruling(&deeper, &path("design/logo.png"), &Tree::default()),
        Ruling::Excluded { pattern, reason }
            if pattern.to_string() == "design" && reason.as_str() == "design material"
    ));

    // And what nobody names is checked.
    assert_eq!(
        central.ruling(&quiet, &path("src/lib.rs"), &Tree::default()),
        Ruling::Checked
    );
}

#[test]
fn a_repository_may_add_a_pattern_but_not_excuse_one() {
    let refused =
        Declaration::parse("[repo]\n\n[[exclude]]\npath = \"*.rs\"\nreason = \"too much work\"\n")
            .expect_err("a pattern in an exclusion is forbidden");
    let message = refused.to_string();
    assert!(message.contains("only add"), "{message}");

    assert!(
        Declaration::parse("[[include]]\npath = \"*.rs\"\n\n[repo]\n").is_ok(),
        "the other way round it is allowed"
    );
    assert!(
        Declaration::parse_central(
            "[repo]\npipeline = { check = [] }\n\n[[exclude]]\n\
                                    path = \"*.rs\"\nreason = \"allowed centrally\"\n"
        )
        .is_ok(),
        "centrally, work may be excused"
    );
}

#[test]
fn only_the_central_list_may_excuse_a_file_while_it_is_empty() {
    let refused = Declaration::parse(
        "[repo]\n\n[[exclude]]\npath = \"keep/.gitkeep\"\nreason = \"a marker\"\n\
         only-if-empty = true\n",
    )
    .expect_err("a repository may not set only-if-empty");
    let message = refused.to_string();
    assert!(
        message.contains("keep/.gitkeep sets only-if-empty"),
        "{message}"
    );

    let central = Declaration::parse_central(
        "[repo]\npipeline = { check = [] }\n\n[[exclude]]\npath = \"**/.gitkeep\"\n\
         reason = \"a marker\"\nonly-if-empty = true\n",
    )
    .expect("centrally, an exclusion may depend on the file being empty");
    assert!(central.exclude[0].only_if_empty);
}

#[test]
fn the_defaults_have_to_write_their_base_set_out() {
    assert!(Central::parse(REGISTER, "[repo]\npipeline = \"none\"\n").is_err());
    assert!(Central::parse(REGISTER, "[repo]\n").is_err());
    assert!(
        Central::parse(REGISTER, "[repo]\npipeline = { check = [\"rustc\"] }\n").is_err(),
        "a check the register does not know"
    );
    assert!(
        Central::parse(
            REGISTER,
            "[repo]\npipeline = { check = [], dependabot = [\"crago\"] }\n"
        )
        .is_err(),
        "an ecosystem the register does not know"
    );
    assert!(Central::parse("not toml", "[repo]\n").is_err());
    assert!(Central::parse(REGISTER, "not toml").is_err());

    for message in [
        Central::parse(REGISTER, "[repo]\n")
            .unwrap_err()
            .to_string(),
        Central::parse("not toml", "[repo]\n")
            .unwrap_err()
            .to_string(),
        Central::parse(REGISTER, "not toml")
            .unwrap_err()
            .to_string(),
    ] {
        assert!(!message.is_empty());
    }
}

#[test]
fn what_is_compiled_in_carries_itself() {
    let central = Central::builtin().expect("what is compiled in");
    assert_eq!(central.base_checks().len(), 4);
    assert_eq!(
        central.base_dependabot().len(),
        1,
        "github-actions everywhere"
    );
    assert!(!central.defaults().exclude.is_empty());
    assert!(
        central
            .register()
            .check
            .contains_key(&nebensound_repo::names::CheckName::new("rust").expect("a name"))
    );

    // The defaults are themselves a declaration and carry reasons.
    for entry in &central.defaults().exclude {
        assert!(!entry.reason.as_str().is_empty());
    }
    let _ = Reason::new("x").expect("a reason");
}

const IMAGE: &str = "image, binary; no tool has an opinion about it";
const FONT: &str = "font, binary; no tool has an opinion about it";
const ARCHIVE: &str = "archive; its content is reachable by no check";
const FIRMWARE: &str = "firmware image; a generated result, not the source";
const CAD: &str = "CAD geometry; partly readable as text, but without a tool in the organization";
const LEGAL: &str = "legal text, taken over unchanged";
const UNFORMATTED: &str = "tool configuration without a formatter";
const LOCKFILE: &str = "lockfile in a format of its own; no tool in the organization checks it";
const COMMANDS: &str = "command list in a syntax of its own; no tool in the organization checks it";

/// What nobody checks, entry for entry: the pattern and its reason. A line
/// here takes a whole type out of every repository's check, so a typo in one
/// of them excuses the wrong files everywhere at once. The defaults have to
/// agree with this table in both directions.
const UNCHECKED_TABLE: &[(&str, &str)] = &[
    ("*.png", IMAGE),
    ("*.jpg", IMAGE),
    ("*.jpeg", IMAGE),
    ("*.gif", IMAGE),
    ("*.webp", IMAGE),
    (
        "*.svg",
        "image as source text; checkable, but without a formatter in the organization",
    ),
    ("*.ico", IMAGE),
    ("*.pdf", "rendered result, not the source"),
    (
        "*.xlsx",
        "spreadsheet document; packed XML that no tool here opens",
    ),
    (
        "*.docx",
        "text document; packed XML that no tool here opens",
    ),
    ("*.odt", "text document; packed XML that no tool here opens"),
    ("*.woff", FONT),
    ("*.woff2", FONT),
    ("*.ttf", FONT),
    ("*.otf", FONT),
    ("*.zip", ARCHIVE),
    ("*.gz", ARCHIVE),
    ("*.tar", ARCHIVE),
    (
        "*.bin",
        "binary artifact; not a source, and no tool reads it",
    ),
    ("*.hex", FIRMWARE),
    ("*.elf", FIRMWARE),
    ("*.stl", CAD),
    ("*.step", CAD),
    (
        "*.gcode",
        "machine code; generated by the slicer, not written",
    ),
    ("*.db", "database file, binary; no tool reads it"),
    ("**/LICENSE", LEGAL),
    ("**/NOTICE", LEGAL),
    (
        "**/CODEOWNERS",
        "a syntax of its own, read by GitHub; no formatter for it",
    ),
    ("**/.gitignore", UNFORMATTED),
    ("**/.gitattributes", UNFORMATTED),
    ("**/.dockerignore", UNFORMATTED),
    ("**/.editorconfig", UNFORMATTED),
    ("**/yarn.lock", LOCKFILE),
    ("**/Gemfile.lock", LOCKFILE),
    ("**/justfile", COMMANDS),
    ("**/Makefile", COMMANDS),
    ("**/.gitkeep", MARKER),
    ("**/.keep", MARKER),
    (
        "**/py.typed",
        "an empty marker of a typed package (PEP 561)",
    ),
    (
        "**/.python-version",
        "a version number read by uv; no tool has an opinion about its format",
    ),
];

const MARKER: &str = "an empty marker that keeps a folder";

/// The entries of the table that hold only while the file is empty.
const ONLY_IF_EMPTY: &[&str] = &["**/.gitkeep", "**/.keep", "**/py.typed"];

#[test]
fn what_nobody_checks_is_exactly_the_unchecked_table() {
    let central = Central::builtin().expect("what is compiled in");

    // Compared as sets that still count: the order of the lines says nothing,
    // a second line for the same pattern would.
    let mut excused = central
        .defaults()
        .exclude
        .iter()
        .map(|entry| (entry.path.to_string(), entry.reason.as_str().to_owned()))
        .collect::<Vec<_>>();
    excused.sort();
    let mut pinned = UNCHECKED_TABLE
        .iter()
        .map(|&(pattern, reason)| (pattern.to_owned(), reason.to_owned()))
        .collect::<Vec<_>>();
    pinned.sort();

    assert_eq!(excused, pinned);

    // An entry that holds only for an empty file excuses less than one that
    // holds always; the wrong flag on either side changes what is excused.
    let conditional = central
        .defaults()
        .exclude
        .iter()
        .filter(|entry| entry.only_if_empty)
        .map(|entry| entry.path.to_string())
        .collect::<Vec<_>>();
    assert_eq!(conditional, ONLY_IF_EMPTY);
}

#[test]
fn a_marker_that_keeps_a_folder_is_excused_only_while_empty() {
    let central = Central::builtin().expect("what is compiled in");
    let plain = Declaration::parse("[repo]\n").expect("a declaration");
    let empty = Tree::new([path("assets/.gitkeep"), path("logs/.keep")])
        .with_empty([path("assets/.gitkeep"), path("logs/.keep")]);
    let filled = Tree::new([path("assets/.gitkeep"), path("logs/.keep")]);

    for marker in ["assets/.gitkeep", "logs/.keep"] {
        assert!(
            matches!(
                central.ruling(&plain, &path(marker), &empty),
                Ruling::Excluded { reason, .. } if reason.as_str() == MARKER
            ),
            "{marker} is empty"
        );
        // With content, or with nothing known about its content, it is a
        // file like any other.
        assert_eq!(
            central.ruling(&plain, &path(marker), &filled),
            Ruling::Checked,
            "{marker} holds content"
        );
    }
}

#[test]
fn a_tree_knows_only_its_own_files_as_empty() {
    let tree = Tree::new([path("assets/.gitkeep")])
        .with_empty([path("assets/.gitkeep"), path("elsewhere/.gitkeep")]);

    assert!(tree.is_empty_file(&path("assets/.gitkeep")));
    assert!(!tree.is_empty_file(&path("elsewhere/.gitkeep")));
    assert!(!Tree::default().is_empty_file(&path("assets/.gitkeep")));
}

#[test]
fn an_ada_specification_is_not_excused_as_a_drawing() {
    // `*.ads` stood among the defaults as "drawing". The extension is also the
    // one Ada specification files carry, so the line excused source code. It
    // is gone: such a file belongs in the check, and until the register knows
    // a check for it, phase 0 reports the gap instead of hiding it.
    let central = Central::builtin().expect("what is compiled in");
    let plain = Declaration::parse("[repo]\n").expect("a declaration");

    assert!(matches!(
        central.ruling(&plain, &path("src/buffers.ads"), &Tree::default()),
        Ruling::Checked
    ));
    // The neighbours stay excused, so the entry was removed and not the list.
    assert!(matches!(
        central.ruling(&plain, &path("data/inventory.db"), &Tree::default()),
        Ruling::Excluded { .. }
    ));
}
