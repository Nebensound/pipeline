//! How the tool gets at the tree and the declaration — and what it says when
//! that does not work.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;

use nebensound::repository::{Repository, central, central_from};
use support::{Git, QUIET, directory, repository};

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

    assert!(error.to_string().contains("not a regular file"), "{error}");
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

#[test]
fn without_a_git_program_there_is_no_history_either() {
    let git = Git::new();
    git.write(&[(".nebensound/repo.toml", QUIET)])
        .commit("chore: start");

    let error = Repository::at(git.path())
        .with_git("git-that-is-not-installed-here")
        .history()
        .expect_err("there is no such program");

    assert!(error.to_string().contains("git does not list"), "{error}");
}

/// A git that refuses every call naming `word` and passes every other one on.
///
/// Written by a shell rather than by this process: a file this process held
/// open for writing could be inherited by a process another test starts at
/// that moment, and running it would then fail as busy.
#[cfg(unix)]
fn refusing(dir: &std::path::Path, word: &str) -> std::path::PathBuf {
    let path = dir.join(format!("git-refusing-{}", word.len()));
    let script = format!(
        "#!/bin/sh\nfor argument in \"$@\"; do\n  if [ \"$argument\" = '{word}' ]; then\n    \
         echo 'refused {word}' >&2\n    exit 3\n  fi\ndone\nexec git \"$@\"\n"
    );
    let status = std::process::Command::new("sh")
        .args([
            "-c",
            "printf '%s' \"$2\" > \"$1\" && chmod 755 \"$1\"",
            "sh",
        ])
        .arg(&path)
        .arg(script)
        .status()
        .expect("a shell");
    assert!(status.success());
    path
}

#[cfg(unix)]
#[test]
fn a_git_that_does_not_answer_about_the_history_is_reported() {
    let git = Git::new();
    git.write(&[(".nebensound/repo.toml", QUIET), ("README.md", "# x\n")]);
    let first = git.commit("chore: start");
    git.tag("v0.0.1", &first);
    git.promote();
    let tools = tempfile::tempdir().expect("a directory");

    for word in [
        "--is-shallow-repository",
        "refs/remotes/origin/release^{commit}",
        "refs/remotes/origin/main^{commit}",
        "for-each-ref",
        "rev-list",
        "log",
        "cat-file",
    ] {
        let error = Repository::at(git.path())
            .with_git(refusing(tools.path(), word))
            .history()
            .expect_err(word);
        assert!(
            error.to_string().contains(&format!("refused {word}")),
            "{word}: {error}"
        );
    }

    let error = Repository::at(git.path())
        .with_git(refusing(tools.path(), "log"))
        .analyse(&central().expect("what is compiled into the tool"))
        .expect_err("the history is part of phase 0");
    assert!(error.to_string().contains("refused log"), "{error}");

    // A merge into release reads the merge on its own, then the line of
    // main from the commit it merges.
    git.write(&[("README.md", "# x\n\nMore.\n")]);
    git.commit("docs: more");
    git.merge_into_release();
    for word in ["--max-count=1", "cat-file"] {
        let error = Repository::at(git.path())
            .with_git(refusing(tools.path(), word))
            .history()
            .expect_err(word);
        assert!(
            error.to_string().contains(&format!("refused {word}")),
            "{word}: {error}"
        );
    }
}
