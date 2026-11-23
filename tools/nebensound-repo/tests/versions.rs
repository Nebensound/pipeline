//! Which version every unit carries, read from real Git repositories: the line
//! of `main` and the declaration each of its commits holds. Tags are records,
//! held against the history and never read for a version.
//!
//! Each fixture is built commit by commit in a temporary directory and tagged
//! the way the release stage tags it, so that what is checked is the history a
//! repository really has, and that a faithful record changes nothing.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use nebensound_repo::repository::Repository;
use nebensound_repo::{Analysis, Rule, Severity};
use support::{Git, QUIET, call, central, repository};

/// Phase 0 on the fixture as it stands, history included.
fn analysed(git: &Git) -> Analysis {
    Repository::at(git.path())
        .analyse(&central())
        .expect("the fixture reads")
}

/// The version of the commit at hand, by the prefix of each unit.
fn versions(analysis: &Analysis) -> BTreeMap<String, Option<String>> {
    analysis
        .versions
        .iter()
        .map(|unit| {
            (
                unit.prefix.clone(),
                unit.version.as_ref().map(ToString::to_string),
            )
        })
        .collect()
}

/// The version one unit has at the commit at hand.
fn version(git: &Git, prefix: &str) -> Option<String> {
    let analysis = analysed(git);
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    versions(&analysis)
        .remove(prefix)
        .unwrap_or_else(|| panic!("no unit with the prefix {prefix}"))
}

/// The current version of one unit at the commit at hand: the highest one
/// the history gives a commit of the line up to it.
fn current(analysis: &Analysis, prefix: &str) -> Option<String> {
    analysis
        .versions
        .iter()
        .find(|unit| unit.prefix == prefix)
        .map_or_else(
            || panic!("no unit with the prefix {prefix}"),
            |unit| unit.current.as_ref().map(ToString::to_string),
        )
}

/// Deletes every tag of the fixture.
fn delete_tags(git: &Git) {
    for tag in git.run(&["tag", "--list"]).lines() {
        git.run(&["tag", "--delete", tag]);
    }
}

/// The findings of one rule.
fn found(analysis: &Analysis, rule: Rule) -> Vec<String> {
    analysis
        .findings()
        .into_iter()
        .filter(|finding| finding.rule == rule)
        .map(ToString::to_string)
        .collect()
}

/// Tags the commit at hand with the version a unit has there, as the release
/// stage would, and returns that version.
fn release(git: &Git, prefix: &str) -> String {
    let version = version(git, prefix).expect("the commit touches the unit");
    git.tag(&format!("{prefix}{version}"), &git.head());
    version
}

/// A repository and two components, `app` and `lib`.
const UNITS: &str = r#"[repo]
audience = "public"
pipeline = "none"

[component.app]
path     = "app"
pipeline = "none"

[component.lib]
path     = "lib"
pipeline = "none"
"#;

/// The declaration of [`units`] with a floor written on `lib`.
fn lib_floored(floor: &str) -> String {
    format!("{UNITS}minimum-version = \"{floor}\"\n")
}

fn units() -> Git {
    units_declared(UNITS)
}

/// The units of [`units`], declared as given.
fn units_declared(declaration: &str) -> Git {
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", declaration),
        ("README.md", "# Units\n"),
        ("app/a.md", "# App\n"),
        ("lib/b.md", "# Lib\n"),
    ]);
    git.commit("chore: start");
    git
}

#[test]
fn one_unit_goes_through_several_releases() {
    let git = Git::new();
    git.write(&[(".nebensound/repo.toml", QUIET), ("README.md", "# One\n")]);

    git.commit("chore: start");
    assert_eq!(release(&git, "v"), "0.0.1");

    git.write(&[("README.md", "# One\n\nMore.\n")]);
    git.commit("feat: below 0.1.0 a feature is a patch");
    assert_eq!(release(&git, "v"), "0.0.2");

    // The step to 0.1.0 is a person's: minimum-version lifts the raised
    // version to it.
    git.write(&[(
        ".nebensound/repo.toml",
        "[repo]\naudience = \"public\"\npipeline = \"none\"\nminimum-version = \"0.1.0\"\n",
    )]);
    git.commit("feat: take the step to 0.1.0");
    assert_eq!(release(&git, "v"), "0.1.0");

    // From 0.1.0 to below 1.0.0 a feature is a patch, and a breaking change
    // raises the minor version.
    git.write(&[("README.md", "# One\n\nMore and more.\n")]);
    git.commit("feat: something new");
    let feature = git.head();
    assert_eq!(release(&git, "v"), "0.1.1");

    // The run of this commit is late with its tag, or sets none: the next
    // one comes to the number it would have had all the same. A tag is a
    // record and moves nothing.
    git.write(&[("README.md", "# One\n\nFixed.\n")]);
    git.commit("fix: a correction");
    let late = git.head();
    assert_eq!(version(&git, "v").as_deref(), Some("0.1.2"));

    git.write(&[("README.md", "# One\n\nBroken.\n")]);
    git.commit("refactor!: a breaking change");
    assert_eq!(version(&git, "v").as_deref(), Some("0.2.0"));

    git.tag("v0.1.2", &late);
    assert_eq!(version(&git, "v").as_deref(), Some("0.2.0"));
    git.tag("v0.2.0", &git.head());

    // No change leads to 1.0.0; minimum-version takes the step, and from
    // there each change raises its own field.
    git.write(&[(
        ".nebensound/repo.toml",
        "[repo]\naudience = \"public\"\npipeline = \"none\"\nminimum-version = \"1.0.0\"\n",
    )]);
    git.commit("feat!: take the step to 1.0.0");
    assert_eq!(release(&git, "v"), "1.0.0");
    git.write(&[("README.md", "# One\n\nNew.\n")]);
    git.commit("feat: something new after 1.0.0");
    assert_eq!(release(&git, "v"), "1.1.0");
    git.write(&[("README.md", "# One\n\nBroken again.\n")]);
    git.commit("fix!: a breaking correction");
    assert_eq!(release(&git, "v"), "2.0.0");

    // A commit comes to the same version on a second run, and with all of
    // the tags deleted.
    git.run(&["checkout", "--quiet", "--detach", &feature]);
    assert_eq!(version(&git, "v").as_deref(), Some("0.1.1"));
    delete_tags(&git);
    assert_eq!(version(&git, "v").as_deref(), Some("0.1.1"));
    git.run(&["checkout", "--quiet", "main"]);
    assert_eq!(version(&git, "v").as_deref(), Some("2.0.0"));
}

#[test]
fn one_commit_raises_every_unit_it_touches() {
    let git = units();
    git.write(&[
        ("app/a.md", "# App\n\nBoth.\n"),
        ("lib/b.md", "# Lib\n\nBoth.\n"),
    ]);
    git.commit("Change the app and the library");

    let analysis = analysed(&git);
    let found_versions = versions(&analysis);
    assert_eq!(found_versions["v"].as_deref(), Some("0.0.2"));
    assert_eq!(found_versions["app-v"].as_deref(), Some("0.0.2"));
    assert_eq!(found_versions["lib-v"].as_deref(), Some("0.0.2"));
    // It counts as a patch everywhere, and is named once.
    let unconventional = found(&analysis, Rule::UnconventionalCommit);
    assert_eq!(unconventional.len(), 1, "{unconventional:?}");
    assert!(
        unconventional[0].contains("\"Change the app and the library\""),
        "{unconventional:?}"
    );
    assert!(
        analysis
            .findings()
            .iter()
            .all(|finding| finding.severity == Severity::Warning),
        "{:?}",
        analysis.findings()
    );
}

#[test]
fn the_declaration_touches_every_unit_and_a_merge_what_it_brings_in() {
    let git = units();
    git.write(&[(
        ".nebensound/repo.toml",
        &format!("{UNITS}\n# A comment changes the declaration.\n"),
    )]);
    git.commit("chore: change the declaration");
    let analysis = analysed(&git);
    assert!(
        versions(&analysis).values().all(Option::is_some),
        "{:?}",
        versions(&analysis)
    );

    git.run(&["checkout", "--quiet", "-b", "side"]);
    git.write(&[("app/a.md", "# App\n\nOn the side.\n")]);
    git.commit("feat: on a side branch");
    git.run(&["checkout", "--quiet", "main"]);
    git.run(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "Merge side",
        "side",
    ]);
    // The merge touches what it changes against its first parent: the app
    // and the repository, not the library. Its own message decides, and it
    // is no Conventional Commit; the commit of the side branch is off the
    // line and is not read.
    let analysis = analysed(&git);
    let found_versions = versions(&analysis);
    assert_eq!(found_versions["v"].as_deref(), Some("0.0.3"));
    assert_eq!(found_versions["app-v"].as_deref(), Some("0.0.3"));
    assert_eq!(found_versions["lib-v"], None);
    let unconventional = found(&analysis, Rule::UnconventionalCommit);
    assert_eq!(unconventional.len(), 1, "{unconventional:?}");
    assert!(
        unconventional[0].contains(&git.head()[..12])
            && unconventional[0].contains("\"Merge side\""),
        "{unconventional:?}"
    );
}

/// What a run at the commit at hand shows of every unit: the version of the
/// commit and the current version, by prefix, and the code of every finding.
type Shown = (
    BTreeMap<String, (Option<String>, Option<String>)>,
    Vec<&'static str>,
);

/// [`Shown`] for the commit at hand.
fn shown(git: &Git) -> Shown {
    let analysis = analysed(git);
    let units = analysis
        .versions
        .iter()
        .map(|unit| {
            (
                unit.prefix.clone(),
                (
                    unit.version.as_ref().map(ToString::to_string),
                    unit.current.as_ref().map(ToString::to_string),
                ),
            )
        })
        .collect();
    let mut codes: Vec<&'static str> = analysis
        .findings()
        .iter()
        .map(|finding| finding.rule.code())
        .collect();
    codes.sort_unstable();
    (units, codes)
}

#[test]
fn a_merge_commit_counts_exactly_as_a_squash_commit_of_the_same_change() {
    let git = units_declared(&lib_floored("1.0.0"));
    // The branch: a message that is no Conventional Commit and a breaking
    // change, neither of which the commit that lands it says.
    git.run(&["checkout", "--quiet", "-b", "topic"]);
    git.write(&[("lib/b.md", "# Lib\n\nStarted.\n")]);
    git.commit("wip");
    git.write(&[("lib/b.md", "# Lib\n\nBroken.\n")]);
    git.commit("feat(lib)!: break on the branch");
    // Meanwhile `main` moves on.
    git.run(&["checkout", "--quiet", "main"]);
    git.write(&[("app/a.md", "# App\n\nMeanwhile.\n")]);
    git.commit("fix: app on main");

    for (message, lib, codes) in [
        ("feat(lib): land the branch", "1.1.0", vec![]),
        (
            "Land the branch",
            "1.0.1",
            vec![Rule::UnconventionalCommit.code()],
        ),
        (
            "fix: land the branch\n\nBREAKING CHANGE: the format changes",
            "2.0.0",
            vec![],
        ),
    ] {
        // The same change lands once by merge commit, once by squash commit,
        // each on a branch of its own from the same point of `main`, and a
        // commit follows each.
        let land = |squash: bool| {
            git.run(&["checkout", "--quiet", "-B", "landed", "main"]);
            if squash {
                git.run(&["merge", "--quiet", "--squash", "topic"]);
                git.commit(message);
            } else {
                git.run(&["merge", "--quiet", "--no-ff", "--message", message, "topic"]);
            }
            let landed = shown(&git);
            git.write(&[("lib/b.md", "# Lib\n\nAfter.\n")]);
            git.commit("fix(lib): after the landing");
            let after = shown(&git);
            (landed, after)
        };
        let (merged, after_merge) = land(false);
        let (squashed, after_squash) = land(true);

        assert_eq!(merged, squashed, "{message}");
        assert_eq!(after_merge, after_squash, "{message}");
        let (units, found_codes) = merged;
        assert_eq!(found_codes, codes, "{message}");
        assert_eq!(units["lib-v"].0.as_deref(), Some(lib), "{message}");
        assert_eq!(units["v"].0.as_deref(), Some("0.0.3"), "{message}");
        assert_eq!(
            units["app-v"],
            (None, Some("0.0.2".to_owned())),
            "{message}"
        );
    }
}

/// The findings `conflicting-tag`, each with the tag it names.
fn conflicting(analysis: &Analysis) -> Vec<String> {
    found(analysis, Rule::ConflictingTag)
}

#[test]
fn a_release_tag_on_a_merged_side_branch_moves_nothing_and_conflicts() {
    let git = units();
    git.tag("app-v0.0.1", &git.head());
    git.run(&["checkout", "--quiet", "-b", "side"]);
    git.write(&[("app/a.md", "# App\n\nOn the side.\n")]);
    let side = git.commit("feat: on a side branch");
    // Only a commit of the first-parent line of main is released, so nothing
    // in the history gives these versions to it.
    git.tag("app-v3.0.0", &side);
    git.tag("app-v0.0.9-pre.1", &side);
    git.tag("lib-v0.0.9-pre.1", &side);
    git.run(&["checkout", "--quiet", "main"]);
    git.write(&[("README.md", "# Units\n\nOn main.\n")]);
    git.commit("docs: on main meanwhile");
    git.run(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "Merge side",
        "side",
    ]);

    git.write(&[("app/a.md", "# App\n\nAfter the merge.\n")]);
    git.commit("fix: after the merge");
    let analysis = analysed(&git);
    let conflicting = conflicting(&analysis);
    assert_eq!(conflicting.len(), 3, "{conflicting:#?}");
    for (tag, unit, floor) in [
        (
            "app-v3.0.0",
            "component app",
            "raise minimum-version above 3.0.0",
        ),
        (
            "app-v0.0.9-pre.1",
            "component app",
            "raise minimum-version to 0.0.9 or higher",
        ),
        (
            "lib-v0.0.9-pre.1",
            "component lib",
            "raise minimum-version to 0.0.9 or higher",
        ),
    ] {
        assert!(
            conflicting.iter().any(|finding| finding
                .starts_with(&format!("error[conflicting-tag] {unit}: the tag \"{tag}\""))
                && finding.contains(&side[..12])
                && finding.contains("to no commit")
                && finding.contains("Delete the tag if nobody published it")
                && finding.contains(floor)),
            "{tag}: {conflicting:#?}"
        );
    }
    assert!(analysis.failed());
    // The merge touched the app once, its side commit not at all, and the
    // commit after it once more; the tags move nothing.
    assert_eq!(versions(&analysis)["app-v"].as_deref(), Some("0.0.3"));
    delete_tags(&git);
    assert_eq!(version(&git, "app-v").as_deref(), Some("0.0.3"));
}

#[test]
fn a_tag_is_held_against_the_version_at_the_commit_at_hand() {
    let git = units();
    // A tag on exactly the commit the history gives its version to is a
    // faithful record.
    assert_eq!(release(&git, "lib-v"), "0.0.1");
    let first = git.head();
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    let one = git.commit("fix: lib one");
    assert_eq!(version(&git, "lib-v").as_deref(), Some("0.0.2"));
    git.write(&[("app/a.md", "# App\n\nOne.\n")]);
    let untouched = git.commit("fix: app only");

    // One below the unit's current version on another commit stays
    // unnoticed: the tags of a repository from before it called the pipeline
    // do no harm below its minimum-version.
    git.tag("lib-v0.0.1-pre.1", &first);
    git.tag("lib-v0.0.2-pre.1", &untouched);
    let analysis = analysed(&git);
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    assert_eq!(current(&analysis, "lib-v").as_deref(), Some("0.0.2"));

    // A tag with a higher number on a commit of the line, and one with the
    // current version on another commit than its own, move no version: they
    // are the error, and phase 0 stops.
    git.tag("lib-v4.0.0", &untouched);
    git.tag("lib-v0.0.2", &first);
    let analysis = analysed(&git);
    let conflicting = conflicting(&analysis);
    assert_eq!(conflicting.len(), 2, "{conflicting:#?}");
    assert!(
        conflicting
            .iter()
            .any(|finding| finding.contains("\"lib-v4.0.0\"")
                && finding.contains(&untouched[..12])
                && finding.contains("to no commit")
                && finding.contains("current version (0.0.2)")
                && finding.contains("raise minimum-version above 4.0.0")),
        "{conflicting:#?}"
    );
    assert!(
        conflicting
            .iter()
            .any(|finding| finding.contains("\"lib-v0.0.2\"")
                && finding.contains(&first[..12])
                && finding.contains(&format!("to the commit {}", &one[..12]))),
        "{conflicting:#?}"
    );
    assert!(analysis.failed());
    assert_eq!(versions(&analysis)["lib-v"], None);
    assert_eq!(current(&analysis, "lib-v").as_deref(), Some("0.0.2"));
    let (text, code) = call(&["nebensound-repo", "-C", git.root(), "plan"]);
    assert_eq!(code, 1, "{text}");

    // Measured against the version of the commit at hand where it touches
    // the unit: the next commit of the library is 0.0.3, past the tag of the
    // current version, and still below the higher one.
    git.run(&["tag", "--delete", "lib-v4.0.0"]);
    git.write(&[("lib/b.md", "# Lib\n\nTwo.\n")]);
    git.commit("fix: lib two");
    let analysis = analysed(&git);
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.3"));
    git.tag("lib-v0.0.3", &one);
    assert_eq!(found(&analysed(&git), Rule::ConflictingTag).len(), 1);

    // The other way out: a minimum-version above the tag.
    git.tag("lib-v4.0.0", &untouched);
    git.write(&[(
        ".nebensound/repo.toml",
        &UNITS.replace(
            "path     = \"lib\"\n",
            "path     = \"lib\"\nminimum-version = \"5.0.0\"\n",
        ),
    )]);
    git.commit("chore: lift the library above its old tags");
    let analysis = analysed(&git);
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("5.0.0"));
}

#[test]
fn the_floor_a_conflicting_tag_names_lifts_past_it() {
    // A minimum version names three numbers: past a pre-release tag the
    // finding names its three numbers.
    let git = units();
    let first = git.head();
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    git.commit("fix: lib one");
    git.tag("lib-v0.0.5-pre.1", &first);
    let analysis = analysed(&git);
    let named = conflicting(&analysis);
    assert!(
        named.len() == 1 && named[0].ends_with("raise minimum-version to 0.0.5 or higher"),
        "{named:#?}"
    );
    let floored = |floor: &str| {
        UNITS.replace(
            "path     = \"lib\"\n",
            &format!("path     = \"lib\"\nminimum-version = \"{floor}\"\n"),
        )
    };
    git.write(&[(".nebensound/repo.toml", &floored("0.0.5"))]);
    git.commit("chore: lift the library past its old pre-release");
    assert_eq!(version(&git, "lib-v").as_deref(), Some("0.0.5"));

    // Past a full tag it takes three numbers above it.
    git.tag("lib-v0.0.7", &first);
    let analysis = analysed(&git);
    let named = conflicting(&analysis);
    assert!(
        named.len() == 1 && named[0].ends_with("raise minimum-version above 0.0.7"),
        "{named:#?}"
    );
    git.write(&[(".nebensound/repo.toml", &floored("0.0.8"))]);
    git.commit("chore: lift the library past its old release");
    assert_eq!(version(&git, "lib-v").as_deref(), Some("0.0.8"));
}

#[test]
fn a_commit_whose_declaration_does_not_read_takes_what_the_next_readable_one_says() {
    // The first commits hold a declaration this reader cannot read, as one
    // written for another reader would be: a field it does not know.
    let git = units_declared(&UNITS.replace(
        "[repo]\naudience = \"public\"\n",
        "[repo]\naudience = \"public\"\nlegacy-field = true\n",
    ));
    git.write(&[("app/a.md", "# App\n\nOne.\n")]);
    git.commit("fix: app only");
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    git.commit("fix: lib one");

    // No readable declaration follows them yet, so the one at hand stands in:
    // here the checkout's, not yet committed. Measured against its path,
    // `fix: lib one` touches the library and `fix: app only` does not.
    git.write(&[(".nebensound/repo.toml", UNITS)]);
    let analysis = analysed(&git);
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.2"));
    assert_eq!(current(&analysis, "app-v").as_deref(), Some("0.0.2"));

    git.commit("chore: drop the field");
    let analysis = analysed(&git);
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.3"));
    assert_eq!(versions(&analysis)["app-v"].as_deref(), Some("0.0.3"));

    // From now on those commits count with the path the readable declaration
    // after them gives, `lib`: the move does not count them again at `libs`,
    // where `fix: lib one` would not touch the library, and the library goes
    // on from 0.0.3 instead of giving it out again.
    git.run(&["mv", "lib", "libs"]);
    git.write(&[(
        ".nebensound/repo.toml",
        &UNITS.replace("path     = \"lib\"\n", "path     = \"libs\"\n"),
    )]);
    git.commit("refactor: move the library");
    assert_eq!(version(&git, "lib-v").as_deref(), Some("0.0.4"));
}

#[test]
fn edition_1_resolves_like_a_declaration_without_one() {
    // The same history twice, once with `edition = 1` in every declaration.
    let resolved = |declaration: &str| {
        let git = units_declared(declaration);
        git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
        git.commit("feat: lib one");
        git.write(&[("app/a.md", "# App\n\nOne.\n")]);
        git.commit("fix!: app one");
        let analysis = analysed(&git);
        assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
        analysis.versions
    };
    let without = resolved(UNITS);
    assert_eq!(resolved(&format!("edition = 1\n\n{UNITS}")), without);
    assert_eq!(
        without
            .iter()
            .map(|unit| unit.version.map(|version| version.to_string()))
            .collect::<Vec<_>>(),
        [Some("0.0.3".to_owned()), Some("0.0.2".to_owned()), None]
    );
}

#[test]
fn an_edition_this_tool_does_not_know_stops_wherever_it_stands_on_the_line() {
    let refused = |git: &Git| {
        Repository::at(git.path())
            .analyse(&central())
            .expect_err("a newer pipeline is needed")
            .to_string()
    };
    let newer = "it is written for edition 2, and this tool knows edition 1 only: a newer \
                 pipeline is needed to read it";
    let git = units();
    git.write(&[(".nebensound/repo.toml", &format!("edition = 2\n\n{UNITS}"))]);
    let switched = git.commit("feat: switch to edition 2");
    assert_eq!(refused(&git), format!(".nebensound/repo.toml: {newer}"));

    // The edition says how its commit is read, so it stops phase 0 also
    // behind the commit at hand, named at the commit that switched it on.
    git.write(&[(".nebensound/repo.toml", UNITS)]);
    git.commit("revert: back to edition 1");
    let message = format!(
        ".nebensound/repo.toml at commit {}: {newer}",
        &switched[..12]
    );
    assert_eq!(refused(&git), message);
    let (text, code) = call(&["nebensound-repo", "-C", git.root(), "plan"]);
    assert_ne!(code, 0, "{text}");
    assert!(text.contains(&message), "{text}");

    // Any other mistake in an earlier declaration makes it one that does not
    // read, which counts with what the next readable declaration says.
    let git = units();
    git.write(&[(".nebensound/repo.toml", &format!("edition = 0\n\n{UNITS}"))]);
    git.commit("fix: an edition that does not exist");
    git.write(&[(".nebensound/repo.toml", UNITS)]);
    git.commit("fix: no edition");
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["v"].as_deref(), Some("0.0.3"));
}

#[test]
fn a_moved_or_removed_file_touches_the_unit_it_left() {
    let git = units();
    git.write(&[("app/c.md", "# C\n")]);
    git.commit("feat: add a file to the app");

    git.run(&["mv", "app/c.md", "lib/c.md"]);
    git.commit("refactor: move it into the library");
    let found_versions = versions(&analysed(&git));
    assert!(found_versions["app-v"].is_some(), "{found_versions:?}");
    assert!(found_versions["lib-v"].is_some(), "{found_versions:?}");

    git.remove("lib/c.md");
    git.commit("refactor: remove it again");
    let found_versions = versions(&analysed(&git));
    assert_eq!(found_versions["app-v"], None, "{found_versions:?}");
    assert!(found_versions["lib-v"].is_some(), "{found_versions:?}");
}

/// Moves a component's folder and its path in the declaration in one commit.
fn move_unit(git: &Git, declaration: &str, from: &str, to: &str) -> String {
    git.run(&["mv", from, to]);
    git.write(&[(
        ".nebensound/repo.toml",
        &declaration.replace(
            &format!("path     = \"{from}\""),
            &format!("path     = \"{to}\""),
        ),
    )]);
    git.commit(&format!("refactor: move {from} to {to}"))
}

#[test]
fn a_moved_unit_keeps_counting_the_commits_under_its_old_path() {
    let git = units();
    assert_eq!(release(&git, "app-v"), "0.0.1");
    assert_eq!(release(&git, "lib-v"), "0.0.1");

    // Every tag set on time.
    for number in [2, 3] {
        git.write(&[("lib/b.md", &format!("# Lib\n\nFix {number}.\n"))]);
        git.commit("fix: in the library");
        assert_eq!(release(&git, "lib-v"), format!("0.0.{number}"));
    }
    let third = git.head();
    let moved = UNITS.replace("path     = \"lib\"", "path     = \"crates/lib\"");
    std::fs::create_dir(git.path().join("crates")).expect("a folder");
    move_unit(&git, UNITS, "lib", "crates/lib");
    assert_eq!(release(&git, "lib-v"), "0.0.4");
    // The declaration changed, which touches the app as well.
    assert_eq!(release(&git, "app-v"), "0.0.2");
    git.write(&[("crates/lib/b.md", "# Lib\n\nAfter the move.\n")]);
    git.commit("fix: after the move");
    assert_eq!(release(&git, "lib-v"), "0.0.5");

    // With the tag of the commit before the move set late.
    git.write(&[("app/a.md", "# App\n\nFix.\n")]);
    let late = git.commit("fix: in the app");
    assert_eq!(version(&git, "app-v").as_deref(), Some("0.0.3"));
    std::fs::create_dir(git.path().join("apps")).expect("a folder");
    move_unit(&git, &moved, "app", "apps/app");
    assert_eq!(version(&git, "app-v").as_deref(), Some("0.0.4"));
    git.tag("app-v0.0.3", &late);
    assert_eq!(release(&git, "app-v"), "0.0.4");
    git.write(&[("apps/app/a.md", "# App\n\nAfter the move.\n")]);
    git.commit("fix: after the move of the app");
    assert_eq!(version(&git, "app-v").as_deref(), Some("0.0.5"));

    // An earlier commit, read again, comes to what it got.
    git.run(&["checkout", "--quiet", "--detach", &third]);
    assert_eq!(version(&git, "lib-v").as_deref(), Some("0.0.3"));
}

#[test]
fn a_generated_unit_whose_folder_moves_keeps_counting() {
    let git = Git::new();
    git.write(&[
        (
            ".nebensound/repo.toml",
            "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.docs]\npath = \"docs\"\npipeline = \
             \"none\"\nversioning = \"child-folders\"\n",
        ),
        ("README.md", "# Docs\n"),
        ("docs/guide/a.md", "# Guide\n"),
        ("docs/handbook/b.md", "# Handbook\n"),
    ]);
    git.commit("chore: start");
    assert_eq!(release(&git, "docs-guide-v"), "0.0.1");
    git.write(&[("docs/guide/a.md", "# Guide\n\nMore.\n")]);
    let late = git.commit("fix: the guide");
    assert_eq!(version(&git, "docs-guide-v").as_deref(), Some("0.0.2"));

    // The folder is renamed and keeps its name as a unit: the declaration
    // does not change, the tree of each commit tells where the unit was.
    git.run(&["mv", "docs/guide", "docs/guide.v2"]);
    git.commit("refactor: rename the guide's folder");
    let analysis = analysed(&git);
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    assert_eq!(
        versions(&analysis)["docs-guide-v"].as_deref(),
        Some("0.0.3")
    );
    assert_eq!(versions(&analysis)["docs-handbook-v"], None);
    git.tag("docs-guide-v0.0.2", &late);
    assert_eq!(release(&git, "docs-guide-v"), "0.0.3");
}

#[test]
fn generated_units_follow_the_children_each_commit_held() {
    // Each commit of the line generates the units of its own tree, whether
    // it adds a file to a child, adds a child, removes a file from one or
    // removes a child, and whatever declaration it holds.
    let declared = "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.docs]\npath = \"docs\"\npipeline = \
                    \"none\"\nversioning = \"child-folders\"\n";
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", declared),
        ("README.md", "# Docs\n"),
        ("docs/guide/a.md", "# Guide\n"),
    ]);
    git.commit("chore: start");
    assert_eq!(release(&git, "docs-guide-v"), "0.0.1");

    git.write(&[("docs/guide/b.md", "# More of the guide\n")]);
    git.commit("fix: a second page for the guide");
    assert_eq!(release(&git, "docs-guide-v"), "0.0.2");

    git.write(&[("docs/handbook/c.md", "# Handbook\n")]);
    git.commit("feat: add a handbook");
    assert_eq!(version(&git, "docs-guide-v"), None);
    // Its count starts here: before this commit no declaration named it,
    // although the first one wrote the declaration, which touches every
    // unit.
    assert_eq!(release(&git, "docs-handbook-v"), "0.0.1");

    git.remove("docs/guide/b.md");
    git.commit("fix: drop the guide's second page");
    assert_eq!(release(&git, "docs-guide-v"), "0.0.3");
    assert_eq!(version(&git, "docs-handbook-v"), None);

    git.remove("docs/guide/a.md");
    git.commit("fix: drop the guide");
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(
        versions(&analysis),
        BTreeMap::from([
            ("v".to_owned(), Some("0.0.5".to_owned())),
            ("docs-handbook-v".to_owned(), None),
        ])
    );

    // A new declaration touches every unit; the commits before it are still
    // read with the declaration and the children they held.
    git.write(&[(
        ".nebensound/repo.toml",
        &format!("{declared}minimum-version = \"0.2.0\"\n"),
    )]);
    git.commit("feat: lift the docs");
    assert_eq!(
        current(&analysed(&git), "docs-handbook-v").as_deref(),
        Some("0.2.0")
    );
    assert_eq!(release(&git, "docs-handbook-v"), "0.2.0");
}

#[test]
fn the_developers_configuration_does_not_change_the_answer() {
    let git = units();
    git.write(&[("app/c.md", "# C\n")]);
    git.commit("feat: add a file to the app");
    git.run(&["mv", "app/c.md", "lib/c.md"]);
    git.commit("refactor: move it into the library");
    let plain = versions(&analysed(&git));

    for (key, value) in [
        ("diff.renames", "true"),
        ("diff.relative", "true"),
        ("diff.ignoreSubmodules", "all"),
        ("log.showRoot", "false"),
        ("log.showSignature", "true"),
        ("color.ui", "always"),
        ("core.quotePath", "true"),
        ("i18n.logOutputEncoding", "UTF-16"),
    ] {
        git.run(&["config", "--local", key, value]);
    }
    assert_eq!(versions(&analysed(&git)), plain);

    // The first commit is a creation even where the configuration says
    // otherwise.
    let first = git.run(&["rev-list", "--max-parents=0", "HEAD"]);
    git.run(&["checkout", "--quiet", "--detach", first.trim()]);
    assert_eq!(version(&git, "app-v").as_deref(), Some("0.0.1"));
}

#[test]
fn a_generated_unit_carries_its_parents_name_in_its_prefix() {
    let git = Git::new();
    git.write(&[
        (
            ".nebensound/repo.toml",
            "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.docs]\npath = \"docs\"\npipeline = \
             \"none\"\nversioning = \"child-files\"\n",
        ),
        ("README.md", "# Docs\n"),
        ("docs/guide.md", "# Guide\n"),
        ("docs/handbook.md", "# Handbook\n"),
    ]);
    git.commit("chore: start");
    assert_eq!(release(&git, "docs-guide-v"), "0.0.1");

    git.write(&[("docs/guide.md", "# Guide\n\nMore.\n")]);
    git.commit("fix: only the guide");
    let analysis = analysed(&git);
    assert_eq!(
        versions(&analysis)["docs-guide-v"].as_deref(),
        Some("0.0.2")
    );
    assert_eq!(versions(&analysis)["docs-handbook-v"], None);

    let (text, code) = call(&["nebensound-repo", "-C", git.root(), "plan"]);
    assert_eq!(code, 0, "{text}");
    let plan: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(
        plan["versions"],
        serde_json::json!([
            {"kind": "repository", "name": null, "path": ".", "prefix": "v",
             "current": "0.0.2", "version": "0.0.2", "writers": []},
            {"kind": "component", "name": "guide", "path": "docs/guide.md",
             "prefix": "docs-guide-v", "current": "0.0.2", "version": "0.0.2",
             "writers": []},
            // Not touched: its current version is what the first commit gave it.
            {"kind": "component", "name": "handbook", "path": "docs/handbook.md",
             "prefix": "docs-handbook-v", "current": "0.0.1", "version": null,
             "writers": []}
        ])
    );
}

#[test]
fn only_a_tag_that_reads_as_a_release_on_a_commit_of_this_history_is_held() {
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", &format!("{UNITS}\n[component.tool]\npath = \"tool\"\npipeline = \"none\"\nversioning = \"none\"\n")),
        ("README.md", "# Units\n"),
        ("app/a.md", "# App\n"),
        ("lib/b.md", "# Lib\n"),
        ("tool/c.md", "# Tool\n"),
    ]);
    let first = git.commit("chore: start");
    git.run(&["checkout", "--quiet", "-b", "elsewhere"]);
    let elsewhere = git.commit("chore: on a branch nobody released");
    git.run(&["checkout", "--quiet", "main"]);
    git.write(&[("README.md", "# Units\n\nMore.\n")]);
    git.commit("docs: more");
    git.write(&[("README.md", "# Units\n\nEven more.\n")]);
    let more = git.commit("docs: even more");

    // An annotated tag is a faithful record as well as a lightweight one.
    git.run(&[
        "tag",
        "--annotate",
        "--message",
        "Released",
        "v0.0.1",
        &first,
    ]);
    // A release of a unit on a commit that does not touch it.
    git.tag("lib-v5.0.0", &more);
    // Not a version the pipeline makes, so the release of no unit.
    git.tag("v1.0", &first);
    git.tag("app-v1.0.0-rc.1", &first);
    // Not on a commit at all.
    let blob = git.run(&["rev-parse", "HEAD:README.md"]);
    git.tag("v2.0.0", blob.trim());
    // In history this commit does not have, and of a unit without a version,
    // and of nobody: none of them is this commit's to judge.
    git.tag("v9.0.0", &elsewhere);
    git.tag("tool-v1.0.0", &first);
    git.tag("nightly", &first);

    let analysis = analysed(&git);
    let conflicting = conflicting(&analysis);
    assert_eq!(conflicting.len(), 1, "{:?}", analysis.findings());
    assert!(
        conflicting[0].starts_with("error[conflicting-tag] component lib: the tag \"lib-v5.0.0\"")
            && conflicting[0].contains(&more[..12])
            && conflicting[0].contains("current version (0.0.1)"),
        "{conflicting:?}"
    );
    assert_eq!(analysis.findings().len(), 1, "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["v"].as_deref(), Some("0.0.3"));
    assert_eq!(current(&analysis, "lib-v").as_deref(), Some("0.0.1"));
}

#[test]
fn a_unit_no_commit_has_given_a_version_yet_has_none_and_every_tag_conflicts() {
    // The declaration of `tool` and its file are not committed yet, so no
    // commit of the line declares it.
    let git = units();
    let first = git.head();
    git.write(&[
        (
            ".nebensound/repo.toml",
            &format!("{UNITS}\n[component.tool]\npath     = \"tool\"\npipeline = \"none\"\n"),
        ),
        ("tool/t.md", "# Tool\n"),
    ]);
    git.run(&["add", "--all"]);
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["tool-v"], None);
    assert_eq!(current(&analysis, "tool-v"), None);

    // So any tag that reads as one of its releases could meet a release.
    git.tag("tool-v0.0.1", &first);
    let analysis = analysed(&git);
    let conflicting = conflicting(&analysis);
    assert_eq!(conflicting.len(), 1, "{:?}", analysis.findings());
    assert!(
        conflicting[0]
            .starts_with("error[conflicting-tag] component tool: the tag \"tool-v0.0.1\"")
            && conflicting[0].contains("to no commit")
            && conflicting[0].contains("current version (none yet)"),
        "{conflicting:?}"
    );
}

#[test]
fn the_tags_of_a_removed_unit_are_passed_over() {
    let git = Git::new();
    git.write(&[
        (
            ".nebensound/repo.toml",
            &format!("{QUIET}\n[component.viewer]\npath = \"viewer\"\npipeline = \"none\"\n"),
        ),
        ("README.md", "# Viewer\n"),
        ("viewer/a.md", "# Viewer\n"),
    ]);
    let first = git.commit("chore: start");
    assert_eq!(release(&git, "viewer-v"), "0.0.1");
    assert_eq!(release(&git, "v"), "0.0.1");

    git.write(&[(".nebensound/repo.toml", QUIET)]);
    git.remove("viewer/a.md");
    git.commit("refactor: drop the viewer");
    // Its tags start with the repository's prefix `v` and still name the
    // releases of the unit it was, in full and before: they are no release
    // of the repository, and neither is a tag that reads as no version.
    git.tag("viewer-v9.0.0", &first);
    git.tag("viewer-v0.0.2-pre.1", &first);
    git.tag("v1.0", &first);
    git.tag("viewer-v1", &first);

    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["v"].as_deref(), Some("0.0.2"));
}

#[test]
fn a_shallow_clone_is_an_error_and_gets_no_plan() {
    let git = units();
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

    let analysis = Repository::at(&target)
        .analyse(&central())
        .expect("the clone reads");
    let shallow = found(&analysis, Rule::ShallowClone);
    assert_eq!(shallow.len(), 1, "{:?}", analysis.findings());
    assert!(
        shallow[0].starts_with("error[shallow-clone]"),
        "{shallow:?}"
    );
    assert!(analysis.versions.is_empty());

    let root = target.to_str().expect("a path in UTF-8");
    let (text, code) = call(&["nebensound-repo", "-C", root, "plan"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("there is no plan"), "{text}");
    let (text, code) = call(&["nebensound-repo", "-C", root, "check"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("error[shallow-clone]"), "{text}");

    // A building block's checkout in a unit job holds only the commit at
    // hand, and its file lists do not depend on any version.
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root,
        "files",
        "--",
        "markdown",
        ".",
    ]);
    assert_eq!(code, 0, "{text}");
    assert!(text.lines().any(|line| line == "README.md"), "{text}");
    let (text, code) = call(&["nebensound-repo", "-C", root, "excluded-packages"]);
    assert_eq!((text.as_str(), code), ("[]\n", 0));
}

#[test]
fn a_version_field_at_its_largest_number_is_an_error() {
    let max = u64::MAX;
    let git = units_declared(&lib_floored(&format!("{max}.0.0")).replace(
        "[repo]\naudience = \"public\"\n",
        &format!("[repo]\naudience = \"public\"\nminimum-version = \"{max}.0.0\"\n"),
    ));
    assert_eq!(version(&git, "lib-v"), Some(format!("{max}.0.0")));
    git.write(&[("lib/b.md", "# Lib\n\nBroken.\n")]);
    git.commit("feat!: one major version too many");

    let analysis = analysed(&git);
    let exhausted = found(&analysis, Rule::ExhaustedVersion);
    assert_eq!(exhausted.len(), 2, "{:?}", analysis.findings());
    for (unit, version) in [
        ("the repository", format!("{max}.0.0")),
        ("component lib", format!("{max}.0.0")),
    ] {
        assert!(
            exhausted.iter().any(|finding| finding.starts_with(&format!(
                "error[exhausted-version] {unit}: {version} cannot be raised"
            ))),
            "{unit}: {exhausted:?}"
        );
    }
    assert_eq!(versions(&analysis)["v"], None);
    assert_eq!(versions(&analysis)["lib-v"], None);
    assert!(analysis.failed());

    // That commit is no release, so the base stays where it was, and the
    // commit after it cannot be raised either.
    git.write(&[("lib/b.md", "# Lib\n\nFixed.\n")]);
    git.commit("fix: after the one too many");
    let analysis = analysed(&git);
    let exhausted = found(&analysis, Rule::ExhaustedVersion);
    assert_eq!(exhausted.len(), 2, "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["lib-v"], None);
}

#[test]
fn a_commit_without_a_declaration_does_not_count() {
    // The repository adopted the declaration later; what came before names
    // no unit, and the count starts at the declaration.
    let git = Git::new();
    git.write(&[("README.md", "# Late\n")]);
    git.commit("chore: before the declaration");
    git.write(&[(".nebensound/repo.toml", QUIET)]);
    git.commit("feat: declare the repository");
    assert_eq!(version(&git, "v").as_deref(), Some("0.0.1"));

    // The commit before it, with the declaration in the working tree only,
    // has no version: its own history declares nothing.
    git.run(&["checkout", "--quiet", "--detach", "HEAD~1"]);
    git.write(&[(".nebensound/repo.toml", QUIET)]);
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["v"], None);
    assert_eq!(current(&analysis, "v"), None);
}

#[test]
fn a_unit_counts_only_the_commits_that_give_it_a_version() {
    let none = UNITS.replace(
        "path     = \"app\"\npipeline = \"none\"\n",
        "path     = \"app\"\npipeline = \"none\"\nversioning = \"none\"\n",
    );
    let without = UNITS.replace(
        "[component.app]\npath     = \"app\"\npipeline = \"none\"\n",
        "",
    );
    // Declared without a version for a time: its count starts when it gets
    // one.
    let git = units_declared(&none);
    git.write(&[("app/a.md", "# App\n\nOne.\n")]);
    git.commit("fix: app without a version");
    git.write(&[(".nebensound/repo.toml", UNITS)]);
    git.commit("feat: give the app a version");
    assert_eq!(release(&git, "app-v"), "0.0.1");
    git.write(&[("app/a.md", "# App\n\nTwo.\n")]);
    git.commit("fix: app two");
    assert_eq!(release(&git, "app-v"), "0.0.2");

    // Removed, and back: it goes on from where it was, and the commits in
    // between do not count, so the count never restarts.
    git.write(&[(".nebensound/repo.toml", &without)]);
    git.commit("refactor: the app is no unit any more");
    git.write(&[("app/a.md", "# App\n\nThree.\n")]);
    git.commit("fix: app as plain files");
    git.write(&[(".nebensound/repo.toml", UNITS)]);
    git.commit("feat: the app is a unit again");
    assert_eq!(release(&git, "app-v"), "0.0.3");
    assert_eq!(release(&git, "v"), "0.0.7");
}

#[test]
fn a_repository_without_a_commit_has_no_version_yet() {
    let dir = repository(&[(".nebensound/repo.toml", QUIET), ("README.md", "# x\n")]);

    let analysis = Repository::at(dir.path())
        .analyse(&central())
        .expect("the fixture reads");
    assert_eq!(analysis.versions.len(), 1);
    assert_eq!(analysis.versions[0].version, None);
    assert_eq!(analysis.versions[0].current, None);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
}

#[test]
fn a_directory_that_is_not_a_repository_has_no_history() {
    let dir = tempfile::tempdir().expect("a directory");

    let error = Repository::at(dir.path())
        .history()
        .expect_err("there is no repository");
    assert!(error.to_string().contains("git does not list"), "{error}");
}

/// A small generator of numbers, so that the walk below is different for
/// every seed and the same in every run.
struct Walk(u64);

impl Walk {
    fn below(&mut self, bound: u64) -> u64 {
        // xorshift64
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % bound
    }
}

/// The versions of the commit at hand, of the units it touches. Phase 0 finds
/// no error in the walk below: every stray tag is deleted before a run reads
/// the history.
fn touched(git: &Git) -> BTreeMap<String, String> {
    let analysis = analysed(git);
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    versions(&analysis)
        .into_iter()
        .filter_map(|(prefix, version)| Some((prefix, version?)))
        .collect()
}

/// What the walk below declares about `app` or `lib`.
struct Walked {
    /// Its name.
    name: &'static str,
    /// Where it lives now.
    path: String,
    /// Its `minimum-version` line, or nothing.
    floor: String,
}

/// The declaration of the walk's units as they stand.
fn declared(units: &[Walked; 2]) -> String {
    let mut text = String::from("[repo]\naudience = \"public\"\npipeline = \"none\"\n");
    for unit in units {
        text.push_str(&["\n[component.", unit.name, "]\npath     = \""].concat());
        text.push_str(&unit.path);
        text.push_str(&["\"\npipeline = \"none\"\n", &unit.floor].concat());
    }
    text
}

/// One walk through a history, as it goes.
struct Walker {
    seed: u64,
    random: Walk,
    git: Git,
    /// `app` and `lib` as the declaration names them now.
    units: [Walked; 2],
    /// What each commit got, by unit.
    got: BTreeMap<(String, String), String>,
    /// Which commit holds each version, by unit.
    holder: BTreeMap<(String, String), String>,
    /// The tags runs have not set yet, with their commits.
    late: Vec<(String, String)>,
    /// How often it moved a floor, set a tag late, moved a unit, and pushed a
    /// pre-release onto a commit.
    happened: [usize; 4],
}

impl Walker {
    fn new(seed: u64) -> Self {
        Self {
            seed,
            random: Walk(seed),
            git: units(),
            // As `units` starts.
            units: [
                Walked {
                    name: "app",
                    path: "app".to_owned(),
                    floor: String::new(),
                },
                Walked {
                    name: "lib",
                    path: "lib".to_owned(),
                    floor: String::new(),
                },
            ],
            got: BTreeMap::new(),
            holder: BTreeMap::new(),
            late: Vec::new(),
            happened: [0; 4],
        }
    }

    /// Notes what the commit at hand got, and tags it as the release stage
    /// would: at once, or late when `delay` says so.
    fn record(&mut self, delay: bool) {
        let seed = self.seed;
        let head = self.git.head();
        for (prefix, version) in touched(&self.git) {
            let owner = self
                .holder
                .entry((prefix.clone(), version.clone()))
                .or_insert_with(|| head.clone());
            assert_eq!(
                *owner, head,
                "seed {seed}: {prefix}{version} for two commits"
            );
            self.got
                .insert((prefix.clone(), head.clone()), version.clone());
            let tag = format!("{prefix}{version}");
            let pending = self.late.iter().any(|(waiting, _)| *waiting == tag);
            if pending || !self.git.run(&["tag", "--list", &tag]).is_empty() {
                continue;
            }
            if delay {
                self.late.push((tag, head.clone()));
            } else {
                self.git.tag(&tag, &head);
            }
        }
    }

    /// The late tags arrive.
    fn flush(&mut self) {
        for (tag, commit) in self.late.drain(..) {
            self.git.tag(&tag, &commit);
        }
    }

    fn declare(&self) {
        self.git
            .write(&[(".nebensound/repo.toml", &declared(&self.units))]);
    }

    /// A floor raised, lowered or taken away again.
    fn move_floor(&mut self, unit: usize) {
        self.happened[0] += 1;
        let major = self.random.below(4);
        self.units[unit].floor = match major {
            0 => String::new(),
            _ => format!("minimum-version = \"{major}.0.0\"\n"),
        };
        self.declare();
    }

    /// A pre-release pushed by hand onto the commit at hand. It moves no
    /// version: it is the error `conflicting-tag`, and the person deletes it
    /// again.
    fn push_stray(&mut self, step: usize) {
        self.happened[3] += 1;
        let tag = format!("{}-v99.0.0-pre.{}", self.units[step % 2].name, step + 1);
        let before = versions(&analysed(&self.git));
        self.git.tag(&tag, &self.git.head());
        let analysis = analysed(&self.git);
        let conflicting = found(&analysis, Rule::ConflictingTag);
        assert!(
            conflicting.len() == 1 && conflicting[0].contains(&format!("\"{tag}\"")),
            "seed {}: {:?}",
            self.seed,
            analysis.findings()
        );
        assert_eq!(versions(&analysis), before, "seed {}", self.seed);
        self.git.run(&["tag", "--delete", &tag]);
    }

    /// The unit's folder moves, and the declaration with it.
    fn move_unit(&mut self, unit: usize) {
        self.happened[2] += 1;
        let to = if self.units[unit].path.contains('/') {
            self.units[unit].name.to_owned()
        } else {
            std::fs::create_dir_all(self.git.path().join("moved")).expect("a folder");
            format!("moved/{}", self.units[unit].name)
        };
        self.git.run(&["mv", &self.units[unit].path, &to]);
        self.units[unit].path = to;
        self.declare();
    }

    /// Changes to the files of the units and beside them, at random.
    fn change_files(&mut self, step: usize) {
        for (bit, path) in [
            (1, format!("{}/a.md", self.units[0].path)),
            (2, format!("{}/b.md", self.units[1].path)),
            (4, "README.md".to_owned()),
        ] {
            if self.random.below(8) & bit != 0 {
                let previous =
                    std::fs::read_to_string(self.git.path().join(&path)).expect("a file");
                self.git
                    .write(&[(&path, &format!("{previous}\nStep {step}.\n"))]);
            }
        }
    }

    /// One step: a commit of some kind.
    fn step(&mut self, step: usize) {
        let choice = self.random.below(12);
        let unit = usize::try_from(self.random.below(2)).expect("small");
        match choice {
            1 | 2 => self.move_floor(unit),
            3 => self.move_unit(unit),
            _ => self.change_files(step),
        }
        let message = [
            "fix: a correction",
            "feat: something new",
            "feat!: a breaking change",
            "docs: words",
            "Update things",
            "fix: a correction\n\nBREAKING CHANGE: it breaks",
        ][usize::try_from(self.random.below(6)).expect("small")];
        self.git.commit(message);
        if self.random.below(6) == 0 {
            self.push_stray(step);
        }
        let delay = self.random.below(3) == 0;
        self.happened[1] += usize::from(delay);
        self.record(delay);
        // The late tags arrive once the next run has read the history.
        if !delay {
            self.flush();
        }
    }

    /// Read again at every commit of main, each commit still comes to what it
    /// got: the version is a function of the history, not of the moment, and
    /// of no tag.
    fn read_again(&self) {
        let seed = self.seed;
        let line = self.git.run(&["rev-list", "--first-parent", "main"]);
        for commit in line.lines() {
            self.git.run(&["checkout", "--quiet", "--detach", commit]);
            for (prefix, version) in touched(&self.git) {
                assert_eq!(
                    self.got.get(&(prefix.clone(), commit.to_owned())),
                    Some(&version),
                    "seed {seed}: {prefix} at {commit}"
                );
            }
        }
    }
}

/// One walk through a history: commits that touch the units at random, with
/// every kind of message, a floor of either unit raised, lowered or taken
/// away now and then, units that move to another folder, pre-releases pushed
/// by hand onto commits, and tags that some runs set late. No version may be
/// given to two commits, and read again at the end every commit still comes
/// to the version it got, with every tag and with none.
///
/// Returns how often each kind of step happened ([`Walker::happened`]), so
/// that the test can show that every one of them did.
fn walk(seed: u64, steps: usize) -> [usize; 4] {
    let mut walker = Walker::new(seed);
    walker.record(false);
    for step in 0..steps {
        walker.step(step);
    }
    walker.flush();
    walker.read_again();
    delete_tags(&walker.git);
    walker.read_again();
    walker.happened
}

#[test]
fn two_different_commits_never_resolve_to_the_same_version_of_a_unit() {
    let mut seen = [0; 4];
    for seed in [0x9e37_79b9, 0x85eb_ca6b, 0xc2b2_ae35, 0x27d4_eb2f] {
        for (total, count) in seen.iter_mut().zip(walk(seed, 24)) {
            *total += count;
        }
    }
    assert!(seen.iter().all(|count| *count > 0), "{seen:?}");
}

#[test]
fn a_lowered_floor_and_a_late_tag_give_no_version_twice() {
    let git = Git::new();
    git.write(&[(".nebensound/repo.toml", QUIET), ("README.md", "# One\n")]);
    git.commit("chore: start");
    // A tag from before the floor, of a release made elsewhere: below the
    // versions from the floor on, it stays unnoticed.
    git.tag("v1.0.0", &git.head());

    git.write(&[(
        ".nebensound/repo.toml",
        "[repo]\naudience = \"public\"\npipeline = \"none\"\nminimum-version = \"3.0.0\"\n",
    )]);
    let lifted = git.commit("feat: aim at 3.0.0");
    assert_eq!(version(&git, "v").as_deref(), Some("3.0.0"));

    // Its tag is late, and the floor is taken away meanwhile. Lowering it has
    // no effect: the versions have passed it.
    git.write(&[(".nebensound/repo.toml", QUIET)]);
    git.commit("fix: drop the floor");
    assert_eq!(release(&git, "v"), "3.0.1");
    git.write(&[("README.md", "# One\n\nBroken.\n")]);
    git.commit("feat!: break something");
    assert_eq!(version(&git, "v").as_deref(), Some("4.0.0"));

    git.tag("v3.0.0", &lifted);
    assert_eq!(version(&git, "v").as_deref(), Some("4.0.0"));
}

#[test]
fn a_commit_whose_declaration_does_not_read_takes_the_types_at_hand_while_none_follows() {
    let rules = "[repo]\naudience = \"public\"\npipeline = \"none\"\nminimum-version = \"1.0.0\"\nfeat-types = [\"rule\"]\n";
    let git = Git::new();
    git.write(&[(".nebensound/repo.toml", rules), ("README.md", "# Rules\n")]);
    git.commit("chore: start");
    git.write(&[(".nebensound/repo.toml", "[repo\n")]);
    git.commit("rule: a declaration that does not read");

    // No readable declaration follows the commit at hand, so the one in the
    // working tree stands in for it, with its list.
    git.write(&[(".nebensound/repo.toml", rules)]);
    assert_eq!(version(&git, "v").as_deref(), Some("1.1.0"));
    git.write(&[(
        ".nebensound/repo.toml",
        &rules.replace("feat-types = [\"rule\"]\n", ""),
    )]);
    assert_eq!(version(&git, "v").as_deref(), Some("1.0.1"));
}
