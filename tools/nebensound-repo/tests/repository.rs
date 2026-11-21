//! How the tool gets at the tree and the declaration — and what it says when
//! that does not work.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;

use nebensound_repo::path::RelativePath;
use nebensound_repo::repository::{Repository, central, central_from};
use support::{Git, QUIET, directory, repository};

#[test]
fn the_built_in_central_data_reads() {
    assert!(central().is_ok());
}

#[test]
fn broken_central_data_says_so() {
    let error = central_from("this is not a register", "[repo]\naudience = \"public\"\n")
        .expect_err("not readable");

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
        .tree_of(b"100644 4b82 0\tgood.md\x00100644 4b82 0\t/absolute\0", b"")
        .expect_err("an absolute path is not one inside the repository");

    assert!(error.to_string().contains("/absolute"), "{error}");
}

#[test]
fn a_listing_becomes_a_tree() {
    let tree = Repository::at(".")
        .tree_of(
            b"100644 4b82 0\tREADME.md\x00100644 4b82 0\tdoc/a.md\0",
            b"",
        )
        .expect("both are paths");

    assert_eq!(tree.files().count(), 2);
}

/// A path as the tree holds it.
fn path(value: &str) -> RelativePath {
    RelativePath::new(value).expect("a path")
}

#[test]
fn a_file_that_holds_the_empty_blob_is_empty() {
    // The empty blob under SHA-1 and under SHA-256; anything else holds
    // content. A file in conflict is listed once for each side, and is empty
    // only when every side is. An entry added with `git add -N` holds the
    // empty blob as a placeholder only, and is not empty.
    let tree = Repository::at(".")
        .tree_of(
            b"100644 e69de29bb2d1d6434b8b29ae775ad8c2e48c5391 0\ta/.gitkeep\0\
              100644 473a0f4c3be8a93681a267e3b1e9a7dcda1185436fe141f7749120a303721813 0\tb/.gitkeep\0\
              100644 4b825dc642cb6eb9a060e54bf8d69288fbee4904 0\tc/.gitkeep\0\
              100644 e69de29bb2d1d6434b8b29ae775ad8c2e48c5391 2\td/.gitkeep\0\
              100644 4b825dc642cb6eb9a060e54bf8d69288fbee4904 3\td/.gitkeep\0\
              100644 e69de29bb2d1d6434b8b29ae775ad8c2e48c5391 0\te/.gitkeep\0",
            b"e/.gitkeep\0",
        )
        .expect("all of them are paths");

    assert_eq!(tree.files().count(), 5);
    assert!(tree.is_empty_file(&path("a/.gitkeep")));
    assert!(tree.is_empty_file(&path("b/.gitkeep")));
    assert!(!tree.is_empty_file(&path("c/.gitkeep")));
    assert!(!tree.is_empty_file(&path("d/.gitkeep")));
    assert!(!tree.is_empty_file(&path("e/.gitkeep")));
}

#[test]
fn a_file_added_with_intent_does_not_count_as_empty() {
    // `git add -N` lists the file with the empty blob, whatever it holds.
    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("kept/.gitkeep", ""),
        ("sub/README.md", "# x\n"),
    ]);
    fs::write(dir.path().join("sub/.gitkeep"), "content\n").expect("a file in the fixture");
    support::git(dir.path(), &["add", "--intent-to-add", "sub/.gitkeep"]);

    let tree = Repository::at(dir.path()).tree().expect("a repository");
    assert!(tree.is_empty_file(&path("kept/.gitkeep")));
    assert!(!tree.is_empty_file(&path("sub/.gitkeep")));

    // Below the root of a larger repository, the paths stay relative to it.
    let tree = Repository::at(dir.path().join("sub"))
        .tree()
        .expect("a folder of a repository");
    assert!(!tree.is_empty_file(&path(".gitkeep")));
}

#[test]
fn an_entry_without_its_object_is_reported() {
    for listing in [&b"README.md\0"[..], &b"100644\tREADME.md\0"[..]] {
        let error = Repository::at(".")
            .tree_of(listing, b"")
            .expect_err("the entry names no object");

        assert!(
            error
                .to_string()
                .contains("is not an entry of `git ls-files --stage`"),
            "{error}"
        );
    }
}

#[test]
fn an_entry_that_is_not_text_is_reported() {
    // git hands out the path as it stands on disk. Replacing what is not UTF-8
    // would rename the file quietly, and two different names could collapse
    // into one.
    let error = Repository::at(".")
        .tree_of(
            b"100644 4b82 0\tgood.md\x00100644 4b82 0\tbad\xff.md\0",
            b"",
        )
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
    let dir = directory(&[("elsewhere.toml", "[repo]\naudience = \"public\"\n")]);
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
    let dir = directory(&[(
        ".nebensound/repo.toml",
        "[repo]\naudience = \"public\"\nversioning = \"yearly\"\n",
    )]);

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
        "ls-tree",
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

#[cfg(unix)]
#[test]
fn a_git_that_does_not_name_the_files_added_with_intent_is_reported() {
    let dir = repository(&[(".nebensound/repo.toml", QUIET)]);
    let tools = tempfile::tempdir().expect("a directory");

    let error = Repository::at(dir.path())
        .with_git(refusing(tools.path(), "--diff-filter=A"))
        .tree()
        .expect_err("the files added with intent are part of the tree");

    assert!(
        error.to_string().contains("refused --diff-filter=A"),
        "{error}"
    );
}
