//! The file format: what a declaration may say, and what it may not.

// A test that cannot read its own fixtures has nothing to say, so it should
// fail loudly instead of carrying a Result through every call. The workspace
// denies these for production code, where an unproven invariant hides in them;
// here the reason travels with the call and the failure is the point.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use nebensound_repo::model::{ExplicitPipeline, Pipeline, Publish, Versioning};
use nebensound_repo::names::{
    ArtifactKind, BuildName, CheckName, ComponentName, Extension, NameError, PresetName, Reason,
    ReasonError, TargetName, VersionWriterName,
};
use nebensound_repo::path::{PathError, RelativePath};
use nebensound_repo::{Declaration, EDITION, Edition};

fn parse(source: &str) -> Declaration {
    match Declaration::parse(source) {
        Ok(declaration) => declaration,
        Err(error) => panic!("expected this to parse, and it did not: {error}"),
    }
}

fn refuses(source: &str) -> String {
    match Declaration::parse(source) {
        Ok(_) => panic!("expected this to be refused, and it was accepted"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn the_minimal_declaration_is_a_statement() {
    let declaration = parse("[repo]\npipeline = \"none\"\n");
    assert_eq!(
        declaration.repo.pipeline,
        Some(Pipeline::Preset(
            PresetName::new("none").expect("a valid preset name")
        ))
    );
    assert!(declaration.component.is_empty());
    assert!(declaration.exclude.is_empty());
    assert!(declaration.include.is_empty());
}

#[test]
fn an_empty_file_is_not_one() {
    assert!(refuses("").contains("repo"));
}

#[test]
fn a_misspelled_field_is_an_error_rather_than_a_shrug() {
    let message = refuses("[repo]\npublsh = false\n");
    assert!(message.contains("publsh"), "{message}");
}

#[test]
fn a_worked_example_parses() {
    let declaration = parse(
        r#"
[[include]]
path = "*.json"


[repo]
pipeline = "none"

[repo.component]
pipeline = "typst-document"

[component.inventory-crate]
path = "crates/inventory"
pipeline = "rust-library"

[component.manuals]
path = "manuals"
versioning = "child-files"
pipeline.repo = true

[[exclude]]
path = "crates/inventory/vendor"
reason = "third-party code, taken as it is"
"#,
    );

    assert_eq!(declaration.include.len(), 1);
    assert_eq!(declaration.component.len(), 2);
    assert_eq!(declaration.exclude.len(), 1);

    let manuals = &declaration.component[&ComponentName::new("manuals").expect("a name")];
    assert_eq!(manuals.versioning, Some(Versioning::ChildFiles));
    assert_eq!(manuals.pipeline, Some(Pipeline::FromRepo));
}

#[test]
fn a_pipeline_is_a_preset_or_a_table_and_nothing_in_between() {
    let written = parse(
        "[repo]\npipeline = { version = [\"cargo\"], check = [\"rust\"], \
         build = \"cargo-package\", publish = [\"crates.io\"] }\n",
    );
    assert_eq!(
        written.repo.pipeline,
        Some(Pipeline::Explicit(ExplicitPipeline {
            version: vec![VersionWriterName::new("cargo").expect("a name")],
            check: vec![CheckName::new("rust").expect("a name")],
            build: Some(BuildName::new("cargo-package").expect("a name")),
            publish: Some(Publish::To(vec![
                TargetName::new("crates.io").expect("a name")
            ])),
            dependabot: Vec::new(),
        }))
    );

    let message = refuses("[repo]\npipeline = { repo = true, build = \"docker\" }\n");
    assert!(message.contains("not a form this format has"), "{message}");

    // A version writer alone is a table written out as well.
    let message = refuses("[repo]\npipeline = { repo = true, version = [\"cargo\"] }\n");
    assert!(message.contains("not a form this format has"), "{message}");

    let message = refuses("[repo]\npipeline = { repo = false }\n");
    assert!(message.contains("says nothing"), "{message}");

    let message = refuses("[repo]\npipeline = { presset = \"rust-library\" }\n");
    assert!(message.contains("presset"), "{message}");

    let message = refuses("[repo]\npipeline = 4\n");
    assert!(message.contains("preset name"), "{message}");
}

#[test]
fn an_empty_pipeline_table_says_nothing_runs() {
    let declaration = parse("[repo]\npipeline = {}\n");
    assert_eq!(
        declaration.repo.pipeline,
        Some(Pipeline::Explicit(ExplicitPipeline::default()))
    );
}

#[test]
fn publish_is_false_or_a_list() {
    let nothing = parse("[repo]\npipeline = { publish = false }\n");
    assert_eq!(
        nothing.repo.pipeline,
        Some(Pipeline::Explicit(ExplicitPipeline {
            publish: Some(Publish::Nothing),
            ..ExplicitPipeline::default()
        }))
    );

    let message = refuses("[repo]\npipeline = { publish = true }\n");
    assert!(message.contains("names no target"), "{message}");
}

#[test]
fn every_versioning_value_is_spelled_the_way_the_format_spells_it() {
    for (written, expected, covers_self, generates) in [
        ("self", Versioning::Own, true, false),
        (
            "self-and-child-folders",
            Versioning::OwnAndChildFolders,
            true,
            true,
        ),
        (
            "self-and-child-files",
            Versioning::OwnAndChildFiles,
            true,
            true,
        ),
        ("child-folders", Versioning::ChildFolders, false, true),
        ("child-files", Versioning::ChildFiles, false, true),
        ("none", Versioning::Nothing, false, false),
    ] {
        let declaration = parse(&format!("[repo]\nversioning = \"{written}\"\n"));
        assert_eq!(declaration.repo.versioning, Some(expected));
        assert_eq!(expected.covers_self(), covers_self);
        assert_eq!(expected.generates_children(), generates);
        assert_eq!(expected.to_string(), written);
    }
    assert_eq!(Versioning::default(), Versioning::Own);
    assert!(Versioning::ChildFolders.covers_child_folders());
    assert!(Versioning::ChildFiles.covers_child_files());
}

#[test]
fn a_reason_has_to_say_something() {
    assert_eq!(Reason::new("   "), Err(ReasonError::Blank));
    assert_eq!(
        Reason::new("   ").unwrap_err().to_string(),
        "a reason must say something, and this one is blank"
    );
    let reason = Reason::new("vendored").expect("a reason");
    assert_eq!(reason.as_str(), "vendored");
    assert_eq!(reason.to_string(), "vendored");
    assert!(refuses("[[exclude]]\npath = \"vendor\"\nreason = \"\"\n").contains("blank"));
}

#[test]
fn a_name_has_to_survive_a_git_tag() {
    assert_eq!(
        ComponentName::new(""),
        Err(NameError::Empty {
            kind: "component name"
        })
    );
    assert!(matches!(
        ComponentName::new("Manual"),
        Err(NameError::Character { found: 'M', .. })
    ));
    assert!(matches!(
        ComponentName::new("-lead"),
        Err(NameError::Boundary { .. })
    ));
    assert!(matches!(
        ComponentName::new("trail-"),
        Err(NameError::Boundary { .. })
    ));
    assert!(matches!(
        ComponentName::new("a..b"),
        Err(NameError::DoubleDot { .. })
    ));

    let name = ComponentName::new("inventory-crate").expect("a name");
    assert_eq!(name.as_str(), "inventory-crate");
    assert_eq!(name.to_string(), "inventory-crate");

    for message in [
        ComponentName::new("").unwrap_err().to_string(),
        ComponentName::new("A").unwrap_err().to_string(),
        ComponentName::new("-a").unwrap_err().to_string(),
        ComponentName::new("a..b").unwrap_err().to_string(),
    ] {
        assert!(message.contains("component name"), "{message}");
    }

    assert!(ArtifactKind::new("crate").is_ok());
    assert!(refuses("[component.A]\npath = \".\"\n").contains("component name"));
}

#[test]
fn an_extension_carries_its_dot() {
    assert_eq!(Extension::new(".rs").expect("an extension").as_str(), ".rs");
    assert_eq!(
        Extension::new(".rs").expect("an extension").to_string(),
        ".rs"
    );
    assert!(Extension::new("rs").is_err());
    assert!(Extension::new(".").is_err());

    assert_eq!(Extension::of("src/lib.rs"), Extension::new(".rs").ok());
    assert_eq!(Extension::of("README.MD"), Extension::new(".md").ok());
    assert_eq!(Extension::of("Dockerfile"), None);
    assert_eq!(Extension::of(".gitignore"), None);
    assert_eq!(Extension::of("dir/.env"), None);
    assert_eq!(Extension::of("weird.na me"), None);
    assert!(refuses("[[include]]\npath = \"*.json/x\"\n\n[repo]\n").contains("names no type"));
}

#[test]
fn a_path_names_something_inside_the_repository() {
    assert_eq!(RelativePath::new(""), Err(PathError::Empty));
    assert!(matches!(
        RelativePath::new("/etc/passwd"),
        Err(PathError::Absolute { .. })
    ));
    assert!(matches!(
        RelativePath::new("crates\\foo"),
        Err(PathError::Backslash { .. })
    ));
    assert!(matches!(
        RelativePath::new("../outside"),
        Err(PathError::Segment { .. })
    ));
    assert!(matches!(
        RelativePath::new("crates//foo"),
        Err(PathError::Segment { .. })
    ));
    assert_eq!(
        RelativePath::new("/"),
        Err(PathError::Absolute {
            value: "/".to_owned()
        })
    );

    for message in [
        RelativePath::new("").unwrap_err().to_string(),
        RelativePath::new("/x").unwrap_err().to_string(),
        RelativePath::new("a\\b").unwrap_err().to_string(),
        RelativePath::new("..").unwrap_err().to_string(),
    ] {
        assert!(!message.is_empty(), "every rejection explains itself");
    }

    let root = RelativePath::new(".").expect("the root");
    assert!(root.is_root());
    assert_eq!(root, RelativePath::root());
    assert_eq!(root.to_string(), ".");
    assert_eq!(root.file_name(), None);

    let nested = RelativePath::new("crates/foo/").expect("a trailing slash is a directory");
    assert_eq!(nested.to_string(), "crates/foo");
    assert_eq!(nested.segments(), ["crates", "foo"]);
    assert_eq!(nested.file_name(), Some("foo"));
}

#[test]
fn containment_is_by_segment_rather_than_by_text() {
    let foo = RelativePath::new("crates/foo").expect("a path");
    let foobar = RelativePath::new("crates/foobar").expect("a path");
    let inside = RelativePath::new("crates/foo/src/lib.rs").expect("a path");

    assert!(foo.contains(&inside));
    assert!(!foo.contains(&foobar), "a text prefix is not containment");
    assert!(RelativePath::root().contains(&foobar));

    assert!(foo.is_parent_of(&foo.join("src")));
    assert!(!foo.is_parent_of(&inside));
    assert!(foo.contains(&foo));
}

#[test]
fn a_publish_that_is_neither_false_nor_a_list_is_told_what_it_may_be() {
    let message = refuses("[repo]\npipeline = { publish = 4 }\n");
    assert!(
        message.contains("`false`, or a list of target names"),
        "{message}"
    );
}

#[test]
fn a_pipeline_that_is_neither_a_name_nor_a_table_is_told_what_it_may_be() {
    let message = refuses("[repo]\npipeline = [1]\n");
    assert!(
        message.contains("a preset name, `{ repo = true }`, or a table"),
        "{message}"
    );
}

#[test]
fn a_value_of_the_wrong_type_is_refused_where_a_name_belongs() {
    for (source, expected) in [
        ("[repo]\n\n[component.thing]\npath = 4\n", "string"),
        ("[[include]]\npath = 4\n\n[repo]\n", "string"),
        (
            "[repo]\n\n[[exclude]]\npath = \"vendor\"\nreason = 4\n",
            "string",
        ),
        ("[repo]\npipeline = { check = [4] }\n", "string"),
        ("[repo]\npipeline = { publish = [4] }\n", "string"),
        ("[repo]\npipeline = { build = 4 }\n", "string"),
        ("[repo]\npipeline = { repo = \"yes\" }\n", "boolean"),
        ("[repo]\nversioning = \"sideways\"\n", "sideways"),
        ("[repo]\nallow-unused = { rust = 4 }\n", "string"),
    ] {
        let message = refuses(source);
        assert!(
            message.contains(expected),
            "expected {expected:?} in the refusal of {source:?}, and got {message}"
        );
    }
}

#[test]
fn the_same_pattern_must_not_stand_twice() {
    // As a table this came for free: TOML forbids the same key twice. A list
    // does not, so the rule stands in the parser.
    let message = refuses(
        "[repo]\n\n[[exclude]]\npath = \"vendor\"\nreason = \"third-party code\"\n\n\
         [[exclude]]\npath = \"vendor\"\nreason = \"once more\"\n",
    );
    assert!(message.contains("twice"), "{message}");

    // Centrally the same rule holds.
    let central = Declaration::parse_central(
        "[repo]\npipeline = { check = [] }\n\n[[exclude]]\npath = \"*.png\"\n\
         reason = \"image\"\n\n[[exclude]]\npath = \"*.png\"\nreason = \"once more\"\n",
    )
    .expect_err("centrally too, one of the two is then untrue");
    assert!(central.to_string().contains("twice"), "{central}");
}

#[test]
fn edition_1_reads_like_a_declaration_without_one() {
    let without = parse("[repo]\npipeline = \"none\"\n");
    let with = parse("edition = 1\n\n[repo]\npipeline = \"none\"\n");
    assert_eq!(without.edition, None);
    assert_eq!(with.edition, Some(Edition(1)));
    assert_eq!(
        Declaration {
            edition: None,
            ..with
        },
        without
    );
    for source in ["[repo]\n", "edition = 1\n[repo]\n"] {
        assert_eq!(Declaration::read_edition(source).ok(), Some(EDITION));
    }
}

#[test]
fn an_edition_this_tool_does_not_know_asks_for_a_newer_pipeline() {
    // Read before anything else of the file: a field the newer edition
    // brings does not hide that a newer pipeline is needed.
    for source in [
        "edition = 2\n\n[repo]\npipeline = \"none\"\n",
        "edition = 2\nsomething-new = true\n\n[repo]\nsomewhere = \"else\"\n",
    ] {
        let message = refuses(source);
        assert_eq!(
            message,
            "it is written for edition 2, and this tool knows edition 1 only: a newer \
             pipeline is needed to read it",
            "{source:?}"
        );
    }
}

#[test]
fn an_edition_is_a_whole_number_from_1() {
    for (source, expected) in [
        (
            "edition = 0\n[repo]\n",
            "edition 0 does not exist: an edition is a whole number from 1",
        ),
        (
            "edition = -3\n[repo]\n",
            "edition -3 does not exist: an edition is a whole number from 1",
        ),
    ] {
        assert_eq!(refuses(source), expected, "{source:?}");
    }
    // Anything but a whole number is a value of the wrong type.
    for source in ["edition = \"1\"\n[repo]\n", "edition = 1.0\n[repo]\n"] {
        let message = refuses(source);
        assert!(message.contains("edition"), "{source:?}: {message}");
        assert!(message.contains("u64"), "{source:?}: {message}");
    }
    // A file that does not read as TOML says so, before any edition.
    assert!(Declaration::read_edition("edition = \n").is_err());
}
