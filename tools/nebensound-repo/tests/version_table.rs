//! `nebensound-repo version`: the versions as a table, in the terminal and in
//! the summary of a run, and read for a pull request as its squash merge would
//! put it on `main`.
//!
//! The fixtures are real Git repositories, built commit by commit, so that
//! what is checked is the history a repository really has.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound_repo::repository::Repository;
use nebensound_repo::versions;
use nebensound_repo::{Analysis, TagVersion};
use support::{Git, QUIET, call, central, repository};

/// A repository and two components, `lib` released in two stages.
const UNITS: &str = r#"[repo]
pipeline = "none"

[component.app]
path     = "app"
pipeline = "none"

[component.lib]
path     = "lib"
pipeline = "none"
release  = "two-stage"
"#;

/// [`UNITS`] with every unit released, tagged as the release stages tag:
/// the first commit releases the repository and `app` in full and `lib` as its
/// first pre-release; the second one touches `lib` and the repository, and is
/// promoted into `release`, which makes it `lib`'s first full release.
fn released() -> Git {
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", UNITS),
        ("README.md", "# Units\n"),
        ("app/a.md", "# App\n"),
        ("lib/b.md", "# Lib\n"),
    ]);
    let first = git.commit("chore: start");
    git.tag("v0.0.1", &first);
    git.tag("app-v0.0.1", &first);
    git.tag("lib-v0.0.1-pre.1", &first);
    git.write(&[("lib/b.md", "# Lib\n\nStarted.\n")]);
    let second = git.commit("fix: lib started");
    git.tag("v0.0.2", &second);
    git.tag("lib-v0.0.1-pre.2", &second);
    git.promote();
    git.tag("lib-v0.0.1", &second);
    git
}

/// Phase 0 on the fixture as it stands.
fn analysed(repository: &Repository) -> Analysis {
    let analysis = repository.analyse(&central()).expect("the fixture reads");
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    analysis
}

/// The version and the waiting commits of one unit, by its prefix.
fn of(analysis: &Analysis, prefix: &str) -> (Option<String>, Option<usize>) {
    let unit = analysis
        .versions
        .iter()
        .find(|unit| unit.prefix == prefix)
        .unwrap_or_else(|| panic!("no unit with the prefix {prefix}"));
    (unit.version.as_ref().map(ToString::to_string), unit.waiting)
}

/// One call of `version` on the fixture, with these arguments behind it.
fn version(git: &Git, args: &[&str]) -> (String, u8) {
    let mut all = vec!["nebensound-repo", "-C", git.root(), "version"];
    all.extend(args);
    call(&all)
}

/// The rows of the text table, each split into its cells.
fn text_rows(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .skip(2)
        .map(|line| {
            line.split("  ")
                .map(str::trim)
                .filter(|cell| !cell.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .collect()
}

/// The rows of the Markdown table, each split into its cells, the escapes
/// taken back. An empty cell is left out, as the text table shows none.
fn markdown_rows(markdown: &str) -> Vec<Vec<String>> {
    markdown
        .lines()
        .filter(|line| line.starts_with("| ") && !line.starts_with("| ---"))
        .map(|line| {
            line.trim_matches('|')
                .split(" | ")
                .map(|cell| cell.trim().replace('\\', ""))
                .filter(|cell| !cell.is_empty())
                .collect()
        })
        .collect()
}

#[test]
fn the_table_says_what_the_plan_says_in_both_forms() {
    let git = released();
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    git.commit("feat: lib one");
    git.write(&[("lib/b.md", "# Lib\n\nTwo.\n")]);
    git.commit("fix: lib two");

    let (text, code) = version(&git, &[]);
    assert_eq!(code, 0, "{text}");
    // Waiting is filled in two stages only: in one stage every commit that
    // touches the unit is a release of its own.
    assert_eq!(
        text,
        "Versions\n\n  Unit            Path  Release       Current      This commit  \
         Waiting\n  the repository  .     single-stage  0.0.4        0.0.4\n  \
         component app   app   single-stage  0.0.1        none\n  component lib   \
         lib   two-stage     0.0.2-pre.2  0.0.2-pre.2  2\n"
    );

    let (markdown, code) = version(&git, &["--markdown"]);
    assert_eq!(code, 0, "{markdown}");
    assert!(markdown.starts_with("## Versions\n\n"), "{markdown}");
    assert!(
        markdown.contains(
            "| Unit | Path | Release | Current | This commit | Waiting |\n| --- | --- | --- | \
             --- | --- | --- |\n"
        ),
        "{markdown}"
    );
    assert!(
        markdown.contains(
            "| component lib | lib | two\\-stage | 0\\.0\\.2\\-pre\\.2 | 0\\.0\\.2\\-pre\\.2 | 2 |"
        ),
        "{markdown}"
    );
    assert!(!markdown.contains("pull request"), "{markdown}");
    assert_eq!(text_rows(&text), markdown_rows(&markdown));

    // The values are those of the plan, which hands them on to the run.
    let (plan, code) = call(&["nebensound-repo", "-C", git.root(), "plan"]);
    assert_eq!(code, 0, "{plan}");
    let plan: serde_json::Value = serde_json::from_str(&plan).expect("valid JSON");
    let planned: Vec<String> = plan["versions"]
        .as_array()
        .expect("a list of versions")
        .iter()
        .map(|unit| unit["version"].as_str().unwrap_or("none").to_owned())
        .collect();
    let shown: Vec<String> = text_rows(&text)[1..]
        .iter()
        .map(|row| row[4].clone())
        .collect();
    assert_eq!(planned, shown);
}

#[test]
fn waiting_counts_the_touching_commits_since_the_last_promotion() {
    let git = released();
    let analysis = analysed(&Repository::at(git.path()));
    // The commit at hand is promoted, so nothing waits for lib. In one stage
    // every commit that touches the unit is a release, and nothing waits.
    assert_eq!(of(&analysis, "v"), (Some("0.0.2".to_owned()), None));
    assert_eq!(of(&analysis, "app-v"), (None, None));
    assert_eq!(of(&analysis, "lib-v"), (Some("0.0.1".to_owned()), Some(0)));

    git.write(&[("app/a.md", "# App\n\nOne.\n")]);
    git.commit("fix: app one");
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    git.commit("fix: lib one");
    let analysis = analysed(&Repository::at(git.path()));
    // No tag of these two commits is set, and none is needed.
    assert_eq!(of(&analysis, "v"), (Some("0.0.4".to_owned()), None));
    assert_eq!(of(&analysis, "app-v"), (None, None));
    assert_eq!(
        of(&analysis, "lib-v"),
        (Some("0.0.2-pre.1".to_owned()), Some(1))
    );
}

#[test]
fn a_pull_request_shows_the_version_its_squash_merge_would_get() {
    let git = released();
    git.write(&[("app/a.md", "# App\n\nOn main.\n")]);
    git.commit("fix: app on main");
    git.run(&["update-ref", "refs/remotes/origin/main", "main"]);

    // The branch of the pull request, with a commit whose message is not the
    // one the merge takes, and a release tag set on it by hand.
    git.run(&["checkout", "--quiet", "-b", "topic"]);
    git.write(&[("lib/b.md", "# Lib\n\nOn the branch.\n")]);
    let topic = git.commit("wip");
    git.tag("lib-v9.9.9", &topic);

    // The merge commit GitHub builds for the pull request, detached.
    git.run(&["checkout", "--quiet", "--detach", "main"]);
    git.run(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "Merge topic",
        "topic",
    ]);
    let message = "feat(lib)!: a breaking change\n\nThe description.";

    // Read as it is, the merge touches nothing, and the tag on the branch is
    // a record of a version the history gives no commit.
    let as_it_is = Repository::at(git.path())
        .analyse(&central())
        .expect("the fixture reads");
    assert_eq!(of(&as_it_is, "lib-v"), (None, Some(0)));
    assert!(
        as_it_is.findings().iter().any(|finding| finding
            .to_string()
            .starts_with("error[conflicting-tag] component lib: the tag \"lib-v9.9.9\"")),
        "{:?}",
        as_it_is.findings()
    );

    // Read as the pull request, it is the squash merge: the tag lies on no
    // commit the merge contains, and the message is the one given.
    let merged = analysed(&Repository::at(git.path()).as_pull_request(message));
    assert!(merged.findings().is_empty(), "{:?}", merged.findings());
    let pull_request = |prefix| of(&merged, prefix);
    assert_eq!(pull_request("v"), (Some("0.0.4".to_owned()), None));
    assert_eq!(pull_request("app-v"), (None, None));
    assert_eq!(
        pull_request("lib-v"),
        (Some("0.0.2-pre.1".to_owned()), Some(1))
    );

    // The current version carries the number of the pull request only where
    // the pull request touches the unit: it is that version then.
    let (text, code) = version(&git, &["--pull-request", "17", "--message", message]);
    assert_eq!(code, 0, "{text}");
    let rows = text_rows(&text);
    assert_eq!(rows[0][4], "This pull request");
    assert_eq!(rows[1][3..5], ["0.0.4-pr.17", "0.0.4-pr.17"]);
    assert_eq!(rows[2][3..5], ["0.0.2", "none"]);
    assert_eq!(
        rows[3][3..],
        ["0.0.2-pre.1-pr.17", "0.0.2-pre.1-pr.17", "1"]
    );
    let (markdown, code) = version(
        &git,
        &["--markdown", "--pull-request", "17", "--message", message],
    );
    assert_eq!(code, 0, "{markdown}");
    assert_eq!(rows, markdown_rows(&markdown));
    assert!(
        markdown.contains("This is pull request 17. Its version is the one its squash merge"),
        "{markdown}"
    );

    // What the squash merge then puts on main comes to exactly that version.
    git.run(&["checkout", "--quiet", "main"]);
    git.run(&["merge", "--quiet", "--squash", "topic"]);
    git.commit(message);
    let squashed = analysed(&Repository::at(git.path()));
    for prefix in ["v", "app-v", "lib-v"] {
        assert_eq!(of(&squashed, prefix), of(&merged, prefix), "{prefix}");
    }
}

#[test]
fn a_breaking_change_in_the_description_counts_for_the_pull_request() {
    let git = released();
    git.write(&[(
        ".nebensound/repo.toml",
        &format!("{UNITS}minimum-version = \"1.0.0\"\n"),
    )]);
    git.commit("feat!: lib reaches 1.0.0");
    git.promote();
    git.tag("lib-v1.0.0", &git.head());
    git.run(&["update-ref", "refs/remotes/origin/main", "main"]);
    git.run(&["checkout", "--quiet", "-b", "topic"]);
    git.write(&[("lib/b.md", "# Lib\n\nBroken.\n")]);
    git.commit("fix: looks like a patch");
    git.run(&["checkout", "--quiet", "--detach", "main"]);
    git.run(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "Merge topic",
        "topic",
    ]);

    let read = |message: &str| {
        of(
            &analysed(&Repository::at(git.path()).as_pull_request(message)),
            "lib-v",
        )
        .0
    };
    assert_eq!(read("fix: lib").as_deref(), Some("1.0.1-pre.1"));
    assert_eq!(read("feat: lib").as_deref(), Some("1.1.0-pre.1"));
    // A description from GitHub ends its lines with a carriage return.
    assert_eq!(
        read("fix: lib\r\n\r\nBREAKING CHANGE: the format changes").as_deref(),
        Some("2.0.0-pre.1")
    );
}

#[test]
fn a_pull_request_into_release_is_read_as_it_is() {
    let git = released();
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    git.commit("fix: lib one");
    git.merge_into_release();

    let as_it_is = analysed(&Repository::at(git.path()));
    let as_pull_request = analysed(&Repository::at(git.path()).as_pull_request("feat!: anything"));
    assert_eq!(as_it_is.versions, as_pull_request.versions);
    assert!(as_it_is.versions.iter().all(|unit| unit.version.is_none()));
}

#[test]
fn the_repository_always_has_a_row_also_before_its_first_commit() {
    // The repository always carries a version, so the table is never empty.
    // Before the first commit there is nothing to count.
    let dir = repository(&[(".nebensound/repo.toml", QUIET), ("README.md", "# New\n")]);
    let root = dir.path().to_str().expect("a path in UTF-8");

    let (text, code) = call(&["nebensound-repo", "-C", root, "version"]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        text_rows(&text)[1],
        ["the repository", ".", "single-stage", "none", "none"]
    );
}

#[test]
fn an_error_in_the_history_fails_with_the_finding_and_no_table() {
    let git = released();
    git.write(&[("README.md", "# Units\n\nMore.\n")]);
    git.commit("docs: more");
    let clone = tempfile::tempdir().expect("a directory");
    let target = clone.path().join("shallow");
    git.run(&[
        "clone",
        "--quiet",
        "--depth",
        "1",
        &format!("file://{}", git.root()),
        target.to_str().expect("a path in UTF-8"),
    ]);
    let root = target.to_str().expect("a path in UTF-8");

    let (text, code) = call(&["nebensound-repo", "-C", root, "version"]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.starts_with("there are no versions: phase 0 found errors\n\nFindings\n\n"),
        "{text}"
    );
    assert!(text.contains("  error[shallow-clone]"), "{text}");
    assert!(!text.contains("Waiting"), "{text}");

    let (markdown, code) = call(&["nebensound-repo", "-C", root, "version", "--markdown"]);
    assert_eq!(code, 1, "{markdown}");
    assert!(
        markdown.starts_with(
            "## Versions\n\nThere are no versions: phase 0 found errors.\n\n- error\\[shallow\\-clone\\]"
        ),
        "{markdown}"
    );
    assert!(!markdown.contains("| Unit |"), "{markdown}");
}

#[test]
fn the_pull_request_and_its_message_go_together() {
    let git = Git::new();
    git.write(&[(".nebensound/repo.toml", QUIET)]);
    git.commit("chore: start");

    for args in [
        &["--pull-request", "3"][..],
        &["--message", "fix: x"][..],
        &["--pull-request", "0", "--message", "fix: x"][..],
        &["--pull-request", "x", "--message", "fix: x"][..],
    ] {
        let (text, code) = version(&git, args);
        assert_eq!(code, 2, "{args:?}: {text}");
    }
}

#[test]
fn a_message_that_starts_with_a_hyphen_stays_the_message() {
    // The job passes the message as `--message=<message>`, so a title such as
    // `- fix: x` or `--help` reaches the tool as the message, not as an option.
    let git = Git::new();
    git.write(&[(".nebensound/repo.toml", QUIET)]);
    git.commit("chore: start");

    for message in ["- fix: x", "--help"] {
        let argument = format!("--message={message}");
        let (text, code) = version(&git, &["--pull-request", "3", &argument]);
        assert_eq!(code, 0, "{message}: {text}");
        assert!(text.contains("This pull request"), "{message}: {text}");
    }
}

#[test]
fn a_pull_request_version_is_the_merge_version_with_its_number() {
    let version = TagVersion::read("1.2.0-pre.3").expect("a version");
    assert_eq!(versions::of_pull_request(&version, 42), "1.2.0-pre.3-pr.42");
}

#[test]
fn the_current_version_is_the_highest_the_history_gives_a_pre_release_included() {
    let git = released();
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    git.commit("feat: lib one");
    git.write(&[("app/a.md", "# App\n\nOne.\n")]);
    git.commit("fix: app one");
    // Without a single tag in the repository.
    for tag in git.run(&["tag", "--list"]).lines() {
        git.run(&["tag", "--delete", tag]);
    }

    let analysis = analysed(&Repository::at(git.path()));
    let current = |prefix: &str| {
        analysis
            .versions
            .iter()
            .find(|unit| unit.prefix == prefix)
            .and_then(|unit| unit.current)
            .map(|current| current.to_string())
    };

    // `lib` is not touched: its current version is the pre-release, above
    // its last full release.
    assert_eq!(of(&analysis, "lib-v"), (None, Some(1)));
    assert_eq!(current("lib-v").as_deref(), Some("0.0.2-pre.1"));
    assert_eq!(current("app-v").as_deref(), Some("0.0.2"));
    assert_eq!(current("v").as_deref(), Some("0.0.4"));
}
