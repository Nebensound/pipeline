//! Changing a declaration without losing what someone wrote in it.

// A test that cannot read its own fixtures has nothing to say, so it should
// fail loudly instead of carrying a Result through every call. The workspace
// denies these for production code, where an unproven invariant hides in them;
// here the reason travels with the call and the failure is the point.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use nebensound_repo::Declaration;
use nebensound_repo::edit::{DeclarationFile, EditError};
use nebensound_repo::names::{ComponentName, Extension, Reason};
use nebensound_repo::path::RelativePath;
use nebensound_repo::pattern::Pattern;

fn name(value: &str) -> ComponentName {
    ComponentName::new(value).expect("a name")
}

fn path(value: &str) -> RelativePath {
    RelativePath::new(value).expect("a path")
}

fn reason(value: &str) -> Reason {
    Reason::new(value).expect("a reason")
}

fn extension(value: &str) -> Pattern {
    Pattern::Type(Extension::new(value).expect("an extension"))
}

fn reparse(file: &DeclarationFile) -> Declaration {
    let rendered = file.to_string();
    Declaration::parse(&rendered)
        .unwrap_or_else(|error| panic!("what we wrote does not read back: {error}\n{rendered}"))
}

#[test]
fn the_file_every_repository_starts_with_is_a_statement() {
    let file = DeclarationFile::minimal();
    assert_eq!(file.to_string(), "[repo]\npipeline = \"none\"\n");
    let declaration = reparse(&file);
    assert!(declaration.component.is_empty());
}

#[test]
fn a_comment_survives_a_change() {
    let source = "\
# Why this repository publishes nothing yet.
[repo]
pipeline = \"none\" # decided in #42
";
    let mut file = DeclarationFile::parse(source).expect("this parses");
    file.add_component(&name("thing"), &path("crates/thing"))
        .expect("the component is new");

    let rendered = file.to_string();
    assert!(rendered.contains("# Why this repository publishes nothing yet."));
    assert!(rendered.contains("# decided in #42"));
    assert_eq!(reparse(&file).component.len(), 1);
}

#[test]
fn a_component_is_added_once() {
    let mut file = DeclarationFile::minimal();
    file.add_component(&name("thing"), &path("crates/thing"))
        .expect("the component is new");

    let again = file.add_component(&name("thing"), &path("elsewhere"));
    assert!(matches!(
        again,
        Err(EditError::AlreadyPresent {
            what: "component",
            ..
        })
    ));
    assert!(
        again
            .expect_err("this was refused")
            .to_string()
            .contains("thing"),
        "the message names the component"
    );

    let declaration = reparse(&file);
    assert_eq!(
        declaration.component[&name("thing")].path,
        path("crates/thing")
    );
}

#[test]
fn an_exclusion_carries_its_reason_and_is_written_once() {
    let mut file = DeclarationFile::minimal();
    file.exclude(
        &Pattern::Path(path("vendor")),
        &reason("third-party code, taken as it is"),
    )
    .expect("the path is new");
    assert!(matches!(
        file.exclude(&Pattern::Path(path("vendor")), &reason("again")),
        Err(EditError::AlreadyPresent { .. })
    ));

    let declaration = reparse(&file);
    let entry = &declaration.exclude[0];
    assert_eq!(entry.path, Pattern::Path(path("vendor")));
    assert_eq!(entry.reason.as_str(), "third-party code, taken as it is");
}

#[test]
fn an_inclusion_is_a_table_and_its_reason_is_optional() {
    let mut file = DeclarationFile::minimal();
    file.exclude(&Pattern::Path(path("vendor")), &reason("third-party code"))
        .expect("the path is new");
    file.include(&extension(".json"), None)
        .expect("include is a list of tables");
    file.include(&extension(".json"), Some(&reason("once more")))
        .expect("saying it twice says the same thing");
    file.include(&extension(".css"), Some(&reason("style is checked")))
        .expect("the pattern is new");

    let rendered = file.to_string();
    assert!(rendered.contains("[[include]]"), "{rendered}");
    assert!(rendered.contains("[[exclude]]"), "{rendered}");

    let declaration = reparse(&file);
    assert_eq!(declaration.include.len(), 2, "{rendered}");
    assert_eq!(declaration.include[0].path, extension(".json"));
    assert_eq!(
        declaration.include[0].reason, None,
        "the first version wins; saying it twice says the same thing"
    );
    assert_eq!(
        declaration.include[1].reason.as_ref().map(Reason::as_str),
        Some("style is checked")
    );
    assert_eq!(declaration.exclude.len(), 1);
}

#[test]
fn a_file_whose_shape_is_wrong_says_so_rather_than_overwriting_it() {
    let mut file = DeclarationFile::parse("include = 4\n[repo]\n").expect("this is TOML");
    let error = file
        .include(&extension(".json"), None)
        .expect_err("include is not a list of tables here");
    assert!(error.to_string().contains("include"), "{error}");
    assert!(
        error.to_string().contains("list of tables"),
        "the advice has to name the shape that is wanted: {error}"
    );

    let mut file = DeclarationFile::parse("component = 4\n[repo]\n").expect("this is TOML");
    let error = file
        .add_component(&name("thing"), &path("crates/thing"))
        .expect_err("component is not a table here");
    assert!(error.to_string().contains("component"), "{error}");

    let mut file = DeclarationFile::parse("exclude = 4\n[repo]\n").expect("this is TOML");
    let error = file
        .exclude(&Pattern::Path(path("vendor")), &reason("because"))
        .expect_err("exclude is not a list of tables here");
    assert!(
        error.to_string().contains("list of tables"),
        "a table is exactly what is refused here, so the advice must not ask for one: {error}"
    );

    assert!(matches!(
        DeclarationFile::parse("this is not toml"),
        Err(EditError::Unparsable(_))
    ));
}
