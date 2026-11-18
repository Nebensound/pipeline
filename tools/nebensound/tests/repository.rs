//! How the tool gets at the tree and the declaration — and what it says when
//! that does not work.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;

use nebensound::repository::{Repository, central, central_from};
use support::{QUIET, directory, repository};

#[test]
fn the_built_in_central_data_reads() {
    assert!(central().is_ok());
}

#[test]
fn broken_central_data_says_so() {
    let error = central_from("this is not a register", "[repo]\n").expect_err("not readable");

    assert!(error.to_string().contains("built-in register"), "{error}");
}

#[test]
fn without_a_git_program_it_says_so() {
    let dir = repository(&[(".nebensound/repo.toml", QUIET)]);
    let error = Repository::at(dir.path())
        .with_git("git-that-is-not-installed-here")
        .tree()
        .expect_err("there is no such program");

    assert!(error.to_string().contains("git does not list"), "{error}");
}

#[test]
fn an_entry_that_is_not_a_path_is_reported() {
    let error = Repository::at(".")
        .tree_of(b"good.md\0/absolute\0")
        .expect_err("an absolute path is not one inside the repository");

    assert!(error.to_string().contains("/absolute"), "{error}");
}

#[test]
fn a_listing_becomes_a_tree() {
    let tree = Repository::at(".")
        .tree_of(b"README.md\0doc/a.md\0")
        .expect("both are paths");

    assert_eq!(tree.files().count(), 2);
}

#[test]
fn an_entry_that_is_not_text_is_reported() {
    // git hands out the path as it stands on disk. Replacing what is not UTF-8
    // would rename the file quietly, and two different names could collapse
    // into one.
    let error = Repository::at(".")
        .tree_of(b"good.md\0bad\xff.md\0")
        .expect_err("that is not text");

    assert!(error.to_string().contains("not UTF-8"), "{error}");
}

#[test]
fn a_declaration_that_is_not_a_regular_file() {
    let dir = directory(&[("README.md", "# x\n")]);
    fs::create_dir_all(dir.path().join(".nebensound/repo.toml")).expect("a folder");

    let error = Repository::at(dir.path())
        .declaration()
        .expect_err("a folder is not a declaration");

    assert!(error.to_string().contains("not a regular file"), "{error}");
}

#[cfg(unix)]
#[test]
fn a_declaration_that_points_somewhere_else_is_refused() {
    // A pull request could aim the declaration out of the checkout, or at a
    // device that never ends. What is read is what stands in the repository.
    let dir = directory(&[("elsewhere.toml", "[repo]\n")]);
    fs::create_dir_all(dir.path().join(".nebensound")).expect("a folder");
    std::os::unix::fs::symlink(
        "../elsewhere.toml",
        dir.path().join(".nebensound/repo.toml"),
    )
    .expect("a link");

    let error = Repository::at(dir.path())
        .declaration()
        .expect_err("a link is not a declaration");

    assert!(
        error.to_string().contains("behind a link, or is one"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn a_declaration_in_a_linked_folder_is_refused() {
    // A link before the file aims the read just as far out of the checkout
    // as one at the file itself.
    let outside = directory(&[("repo.toml", QUIET)]);
    let dir = directory(&[("README.md", "# x\n")]);
    std::os::unix::fs::symlink(outside.path(), dir.path().join(".nebensound")).expect("a link");

    let error = Repository::at(dir.path())
        .declaration()
        .expect_err("a linked folder holds no declaration of this repository");

    assert!(
        error.to_string().contains("behind a link, or is one"),
        "{error}"
    );
}

#[test]
fn a_declaration_below_something_that_is_not_a_folder() {
    let dir = directory(&[(".nebensound", "this is a file\n")]);

    let error = Repository::at(dir.path())
        .declaration()
        .expect_err(".nebensound is not a folder here");

    assert!(error.to_string().contains("not readable"), "{error}");
}

#[test]
fn a_declaration_that_does_not_match_the_shape() {
    let dir = directory(&[(".nebensound/repo.toml", "[repo]\nversioning = \"yearly\"\n")]);

    let error = Repository::at(dir.path())
        .declaration()
        .expect_err("that is not a value for versioning");

    assert!(
        error.to_string().contains(".nebensound/repo.toml"),
        "{error}"
    );
}

#[test]
fn a_repository_knows_where_it_lies() {
    let dir = directory(&[]);

    assert_eq!(Repository::at(dir.path()).root(), dir.path());
}
