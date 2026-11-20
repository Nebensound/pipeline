//! Which version every unit carries, read from real Git repositories: the line
//! of `main`, the declaration each of its commits holds and the branch
//! `origin/release`. Tags are records, held against the history and never
//! read for a version.
//!
//! Each fixture is built commit by commit in a temporary directory and tagged
//! the way the release stages tag it, so that what is checked is the history a
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

/// The declaration of [`units`] with a floor written on `lib`.
fn lib_floored(floor: &str) -> String {
    UNITS.replace(
        "release  = \"two-stage\"\n",
        &format!("release  = \"two-stage\"\nminimum-version = \"{floor}\"\n"),
    )
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
    // The branch release exists from the first commit on, before anything is
    // promoted into it.
    git.run(&["branch", "release"]);
    git.run(&["update-ref", "refs/remotes/origin/release", "release"]);
    git
}

#[test]
fn one_unit_goes_through_several_single_stage_releases() {
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
        "[repo]\npipeline = \"none\"\nminimum-version = \"0.1.0\"\n",
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
        "[repo]\npipeline = \"none\"\nminimum-version = \"1.0.0\"\n",
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
fn a_two_stage_unit_goes_through_pre_releases_a_promotion_and_the_next_pre_release() {
    let git = units();
    // The first commit adds the declaration, which touches every unit.
    assert_eq!(version(&git, "lib-v").as_deref(), Some("0.0.1-pre.1"));
    git.write(&[(
        ".nebensound/repo.toml",
        &UNITS.replace(
            "release  = \"two-stage\"\n",
            "release  = \"two-stage\"\nminimum-version = \"1.0.0\"\n",
        ),
    )]);
    git.commit("feat: aim the library at 1.0.0");
    assert_eq!(release(&git, "lib-v"), "1.0.0-pre.2");

    git.write(&[("lib/b.md", "# Lib\n\nFixed.\n")]);
    git.commit("fix: a correction in the library");
    assert_eq!(release(&git, "lib-v"), "1.0.0-pre.3");

    // A commit beside the unit gives it no version.
    git.write(&[("app/a.md", "# App\n\nMore.\n")]);
    git.commit("feat: only the app");
    assert_eq!(version(&git, "lib-v"), None);
    // The first commit and the change of the declaration touched it before.
    assert_eq!(version(&git, "app-v").as_deref(), Some("0.0.3"));
    // The current version is the highest one the history gives a commit up
    // to the commit at hand, a pre-release included, and the version of the
    // commit at hand where it touches the unit.
    let analysis = analysed(&git);
    assert_eq!(current(&analysis, "lib-v").as_deref(), Some("1.0.0-pre.3"));
    assert_eq!(current(&analysis, "app-v").as_deref(), Some("0.0.3"));

    git.write(&[("lib/b.md", "# Lib\n\nNew.\n")]);
    git.commit("feat: something new in the library");
    assert_eq!(release(&git, "lib-v"), "1.0.0-pre.4");

    // The promoted commit is the full release of the target, as soon as the
    // merge into release exists: no tag is needed for it.
    delete_tags(&git);
    git.promote();
    let analysis = analysed(&git);
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("1.0.0"));
    assert_eq!(current(&analysis, "lib-v").as_deref(), Some("1.0.0"));

    // The next merge starts the next pre-release from it, with no tag in the
    // repository.
    git.write(&[("lib/b.md", "# Lib\n\nFixed again.\n")]);
    git.commit("fix: the next correction");
    assert_eq!(version(&git, "lib-v").as_deref(), Some("1.0.1-pre.1"));
    assert!(git.run(&["tag", "--list"]).is_empty());

    git.write(&[("lib/b.md", "# Lib\n\nAnother feature.\n")]);
    git.commit("feat: the next feature");
    assert_eq!(release(&git, "lib-v"), "1.1.0-pre.2");
}

#[test]
fn a_promoted_commit_releases_what_came_before_it_and_nothing_twice() {
    let git = units();
    git.write(&[("lib/b.md", "# Lib\n\nFixed.\n")]);
    git.commit("fix: in the library");
    git.write(&[("README.md", "# Units\n\nMore.\n")]);
    git.commit("docs: beside the library");

    // It touches nothing of the library itself, and it is what is released.
    let promoted = git.promote();
    assert_eq!(version(&git, "lib-v").as_deref(), Some("0.0.1"));
    git.tag("lib-v0.0.1", &promoted);
    assert_eq!(version(&git, "lib-v").as_deref(), Some("0.0.1"));

    // Promoted again with nothing new for the library: nothing to release.
    git.write(&[("README.md", "# Units\n\nEven more.\n")]);
    git.commit("docs: still beside the library");
    git.promote();
    assert_eq!(version(&git, "lib-v"), None);
    // Its current version is the full release.
    let analysis = analysed(&git);
    assert_eq!(current(&analysis, "lib-v").as_deref(), Some("0.0.1"));
    assert_eq!(analysis.versions[2].waiting, Some(0));
}

#[test]
fn one_commit_raises_every_unit_it_touches() {
    let git = units();
    git.promote();
    git.write(&[
        ("app/a.md", "# App\n\nBoth.\n"),
        ("lib/b.md", "# Lib\n\nBoth.\n"),
    ]);
    git.commit("Change the app and the library");

    let analysis = analysed(&git);
    let found_versions = versions(&analysis);
    assert_eq!(found_versions["v"].as_deref(), Some("0.0.2"));
    assert_eq!(found_versions["app-v"].as_deref(), Some("0.0.2"));
    assert_eq!(found_versions["lib-v"].as_deref(), Some("0.0.1-pre.2"));
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
fn the_declaration_touches_every_unit_and_a_merge_touches_none() {
    let git = units();
    git.promote();
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
    let analysis = analysed(&git);
    assert!(
        versions(&analysis).values().all(Option::is_none),
        "{:?}",
        versions(&analysis)
    );
    assert!(found(&analysis, Rule::UnconventionalCommit).is_empty());
}

/// The findings `conflicting-tag`, each with the tag it names.
fn conflicting(analysis: &Analysis) -> Vec<String> {
    found(analysis, Rule::ConflictingTag)
}

#[test]
fn a_release_tag_on_a_merged_side_branch_moves_nothing_and_conflicts() {
    let git = units();
    git.promote();
    git.tag("app-v0.0.1", &git.head());
    git.run(&["checkout", "--quiet", "-b", "side"]);
    git.write(&[("app/a.md", "# App\n\nOn the side.\n")]);
    let side = git.commit("feat: on a side branch");
    // Only a commit of the first-parent line of main is released or given a
    // pre-release, so nothing in the history gives these versions to it.
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
    for (tag, unit) in [
        ("app-v3.0.0", "component app"),
        ("app-v0.0.9-pre.1", "component app"),
        ("lib-v0.0.9-pre.1", "component lib"),
    ] {
        assert!(
            conflicting.iter().any(|finding| finding
                .starts_with(&format!("error[conflicting-tag] {unit}: the tag \"{tag}\""))
                && finding.contains(&side[..12])
                && finding.contains("to no commit")
                && finding.contains("Delete the tag if nobody published it")
                && finding.contains("raise minimum-version above")),
            "{tag}: {conflicting:#?}"
        );
    }
    assert!(analysis.failed());
    // The commit after the merge is the one since the release that touched
    // the app, and the tags move nothing.
    assert_eq!(versions(&analysis)["app-v"].as_deref(), Some("0.0.2"));
    delete_tags(&git);
    assert_eq!(version(&git, "app-v").as_deref(), Some("0.0.2"));
}

/// The declaration of [`units`] with `lib` released in one stage.
fn single_stage() -> String {
    UNITS.replace("release  = \"two-stage\"\n", "")
}

#[test]
fn a_switch_to_two_stages_goes_on_from_the_releases_of_one_stage() {
    let git = units_declared(&single_stage());
    assert_eq!(release(&git, "lib-v"), "0.0.1");
    git.write(&[("lib/b.md", "# Lib\n\nBefore the switch.\n")]);
    git.commit("fix: from when the library was released in one stage");
    assert_eq!(release(&git, "lib-v"), "0.0.2");

    git.write(&[(".nebensound/repo.toml", UNITS)]);
    git.commit("feat: release the library in two stages");
    assert_eq!(release(&git, "lib-v"), "0.0.3-pre.1");
    // A full release pushed by hand on a commit where the library was
    // already released in two stages, before anything was promoted: the
    // history gives it 0.0.3-pre.2, so it is a conflicting record.
    git.write(&[("lib/b.md", "# Lib\n\nThe first cycle.\n")]);
    let early = git.commit("fix: the first cycle");
    git.tag("lib-v5.0.0", &early);
    let analysis = analysed(&git);
    assert_eq!(conflicting(&analysis).len(), 1, "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.3-pre.2"));
    // The way out: nobody published it, so it is deleted.
    git.run(&["tag", "--delete", "lib-v5.0.0"]);
    assert_eq!(release(&git, "lib-v"), "0.0.3-pre.2");
    git.write(&[("lib/b.md", "# Lib\n\nStill the first cycle.\n")]);
    git.commit("fix: still the first cycle");
    assert_eq!(release(&git, "lib-v"), "0.0.3-pre.3");
    git.promote();
    assert_eq!(release(&git, "lib-v"), "0.0.3");

    git.write(&[("lib/b.md", "# Lib\n\nThe second cycle.\n")]);
    git.commit("fix: the second cycle");
    assert_eq!(release(&git, "lib-v"), "0.0.4-pre.1");
    git.write(&[("lib/b.md", "# Lib\n\nStill the second cycle.\n")]);
    git.commit("fix: still the second cycle");
    assert_eq!(release(&git, "lib-v"), "0.0.4-pre.2");
    git.promote();
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.4"));
    // The same with every record deleted.
    delete_tags(&git);
    assert_eq!(version(&git, "lib-v").as_deref(), Some("0.0.4"));
}

#[test]
fn a_tag_is_held_against_the_version_at_the_commit_at_hand() {
    let git = units_declared(&single_stage());
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
        &single_stage().replace(
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
fn a_commit_whose_declaration_does_not_read_takes_what_the_commit_at_hand_says() {
    // The first commits hold a declaration this reader cannot read, as one
    // written for another reader would be: a field it does not know.
    let git = units_declared(&single_stage().replace("[repo]\n", "[repo]\nlegacy-field = true\n"));
    git.tag("lib-v0.0.1", &git.head());
    git.write(&[("app/a.md", "# App\n\nOne.\n")]);
    git.commit("fix: app only");
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    git.commit("fix: lib one");
    git.write(&[(".nebensound/repo.toml", &single_stage())]);
    git.commit("chore: drop the field");

    // Measured against the path of the commit at hand, `fix: lib one`
    // touches the library and `fix: app only` does not; in the mode of the
    // commit at hand, one stage, the first commit is 0.0.1, as its tag
    // records.
    let analysis = analysed(&git);
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.3"));
    assert_eq!(versions(&analysis)["app-v"].as_deref(), Some("0.0.3"));
}

#[test]
fn edition_1_resolves_like_a_declaration_without_one() {
    // The same history twice, once with `edition = 1` in every declaration.
    let resolved = |declaration: &str| {
        let git = units_declared(declaration);
        git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
        git.commit("feat: lib one");
        git.promote();
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
    // read, which counts with what the commit at hand says.
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
fn a_switch_to_one_stage_and_back_keeps_the_releases_of_each_period() {
    let git = units();
    assert_eq!(release(&git, "lib-v"), "0.0.1-pre.1");
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    git.commit("fix: lib one");
    assert_eq!(release(&git, "lib-v"), "0.0.1-pre.2");
    git.promote();
    assert_eq!(release(&git, "lib-v"), "0.0.1");

    git.write(&[(".nebensound/repo.toml", &single_stage())]);
    git.commit("feat: release lib in one stage");
    assert_eq!(release(&git, "lib-v"), "0.0.2");
    git.write(&[("lib/b.md", "# Lib\n\nTwo.\n")]);
    git.commit("fix: lib two");
    assert_eq!(release(&git, "lib-v"), "0.0.3");
    // The branch release holds the releases of the first period.
    let analysis = analysed(&git);
    assert!(found(&analysis, Rule::PointlessReleaseBranch).is_empty());

    // Back in two stages, the full releases of the single-stage period count:
    // the library was released in one stage at their commits.
    git.write(&[(".nebensound/repo.toml", UNITS)]);
    git.commit("feat: release lib in two stages again");
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(release(&git, "lib-v"), "0.0.4-pre.1");
    git.promote();
    assert_eq!(release(&git, "lib-v"), "0.0.4");

    // And once more in one stage, from the promoted release.
    git.write(&[(".nebensound/repo.toml", &single_stage())]);
    git.commit("feat: release lib in one stage again");
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.5"));
    delete_tags(&git);
    assert_eq!(version(&git, "lib-v").as_deref(), Some("0.0.5"));
}

#[test]
fn the_merges_of_main_below_the_start_of_release_promoted_nothing() {
    let git = Git::new();
    git.write(&[
        (
            ".nebensound/repo.toml",
            &UNITS.replace("release  = \"two-stage\"\n", ""),
        ),
        ("README.md", "# Units\n"),
        ("app/a.md", "# App\n"),
        ("lib/b.md", "# Lib\n"),
    ]);
    git.commit("chore: start");
    assert_eq!(release(&git, "lib-v"), "0.0.1");

    // A side branch merged into main with a merge commit, long before the
    // branch release exists.
    git.run(&["checkout", "--quiet", "-b", "side"]);
    git.write(&[("lib/b.md", "# Lib\n\nOn the side.\n")]);
    let side = git.commit("fix: side work");
    git.run(&["checkout", "--quiet", "main"]);
    git.run(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "Merge side",
        "side",
    ]);
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    git.commit("fix: one");
    assert_eq!(release(&git, "lib-v"), "0.0.2");
    git.write(&[("lib/b.md", "# Lib\n\nTwo.\n")]);
    git.commit("fix: two");
    assert_eq!(release(&git, "lib-v"), "0.0.3");

    // The switch to two stages, and the branch release started on main as it
    // stands: its first-parent line goes on below that commit through the
    // merge above.
    git.write(&[(".nebensound/repo.toml", UNITS)]);
    git.commit("feat: release lib in two stages");
    git.run(&["branch", "release"]);
    git.write(&[("lib/b.md", "# Lib\n\nThree.\n")]);
    git.commit("fix: three");
    git.promote();
    // A full release pushed by hand on the tip of the side branch: the
    // history gives that commit nothing, so it is no release.
    git.tag("lib-v7.0.0", &side);

    let analysis = analysed(&git);
    let conflicting = conflicting(&analysis);
    assert_eq!(conflicting.len(), 1, "{conflicting:?}");
    assert!(
        conflicting[0].starts_with("error[conflicting-tag] component lib")
            && conflicting[0].contains("\"lib-v7.0.0\"")
            && conflicting[0].contains("to no commit"),
        "{conflicting:?}"
    );
    // The releases from before the switch count, and the promoted commit
    // goes on from the newest of them, not from the side branch.
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.4"));
}

#[test]
fn the_pull_request_into_release_reads_the_line_of_main() {
    let git = units();
    for prefix in ["v", "app-v", "lib-v"] {
        release(&git, prefix);
    }
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    git.commit("fix: lib one");
    release(&git, "v");
    assert_eq!(release(&git, "lib-v"), "0.0.1-pre.2");
    git.promote();
    assert_eq!(release(&git, "lib-v"), "0.0.1");
    git.write(&[("app/a.md", "# App\n\nOne.\n")]);
    git.commit("fix: app one");
    release(&git, "v");
    assert_eq!(release(&git, "app-v"), "0.0.2");
    git.write(&[("lib/b.md", "# Lib\n\nTwo.\n")]);
    git.commit("fix: lib two");
    release(&git, "v");
    assert_eq!(release(&git, "lib-v"), "0.0.2-pre.1");

    // Its first parent is release, whose first-parent line holds the merges
    // of release; the commits of main are those the merge brings in.
    git.merge_into_release();
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(current(&analysis, "v").as_deref(), Some("0.0.4"));
    assert_eq!(current(&analysis, "app-v").as_deref(), Some("0.0.2"));
    assert_eq!(current(&analysis, "lib-v").as_deref(), Some("0.0.2-pre.1"));
    // A merge touches nothing and is no promoted commit of main.
    assert!(versions(&analysis).values().all(Option::is_none));

    // Once it is merged and origin/release points at it, the commit it
    // promotes is the full release, with no tag set yet.
    git.run(&["update-ref", "refs/remotes/origin/release", "HEAD"]);
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(current(&analysis, "lib-v").as_deref(), Some("0.0.2"));
    assert!(versions(&analysis).values().all(Option::is_none));
}

#[test]
fn the_pull_request_into_release_keeps_a_switch_to_one_stage() {
    let both = UNITS.replace(
        "path     = \"app\"\npipeline = \"none\"\n",
        "path     = \"app\"\npipeline = \"none\"\nrelease  = \"two-stage\"\n",
    );
    let git = units_declared(&both);
    assert_eq!(release(&git, "app-v"), "0.0.1-pre.1");
    git.write(&[("app/a.md", "# App\n\nOne.\n")]);
    git.commit("fix: app one");
    assert_eq!(release(&git, "app-v"), "0.0.1-pre.2");
    git.promote();
    assert_eq!(release(&git, "app-v"), "0.0.1");
    git.write(&[(".nebensound/repo.toml", UNITS)]);
    git.commit("feat: release app in one stage");
    assert_eq!(release(&git, "app-v"), "0.0.2");

    // On release's own line app was released in two stages last; on the
    // line of main it was promoted and released in one stage since.
    git.merge_into_release();
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(current(&analysis, "app-v").as_deref(), Some("0.0.2"));
}

#[test]
fn a_pull_request_into_main_onto_the_start_of_release_reads_its_own_line() {
    // release is started on the tip of main, so the merge commit of a pull
    // request into main has it as its first parent, like a merge into
    // release. What it merges is no commit of main.
    let git = units();
    git.run(&["update-ref", "refs/remotes/origin/main", "main"]);
    git.run(&["checkout", "--quiet", "-b", "feature"]);
    git.write(&[("app/a.md", "# App\n\nA feature.\n")]);
    git.commit("feat: a feature");
    git.run(&["checkout", "--quiet", "--detach", "main"]);
    git.run(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "Merge feature",
        "feature",
    ]);

    // Read as a merge into release, the feature would be a commit of the
    // line and give the app a version.
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(current(&analysis, "app-v").as_deref(), Some("0.0.1"));
    assert_eq!(versions(&analysis)["app-v"], None);
}

#[test]
fn a_merge_into_release_of_an_older_commit_of_main_is_refused() {
    let git = units();
    assert_eq!(release(&git, "lib-v"), "0.0.1-pre.1");
    git.write(&[("lib/b.md", "# Lib\n\nZero.\n")]);
    git.commit("fix: lib zero");
    assert_eq!(release(&git, "lib-v"), "0.0.1-pre.2");
    git.promote();
    assert_eq!(release(&git, "lib-v"), "0.0.1");
    let mut given = vec![
        "0.0.1-pre.1".to_owned(),
        "0.0.1-pre.2".to_owned(),
        "0.0.1".to_owned(),
    ];
    let mut step = |git: &Git, expected: &str| {
        let version = release(git, "lib-v");
        assert_eq!(version, expected);
        assert!(!given.contains(&version), "lib-v{version} for two commits");
        given.push(version);
    };
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    let older = git.commit("fix: lib one");
    step(&git, "0.0.2-pre.1");
    git.write(&[("lib/b.md", "# Lib\n\nTwo.\n")]);
    git.commit("feat: lib two");
    step(&git, "0.0.2-pre.2");

    // The pull request into release that merges the older commit, and then
    // the merge itself, pushed past the check: both are refused, and neither
    // gives a version, so the release stage tags nothing.
    let merge = git.merge_into_release_from(&older);
    for _ in ["pull request", "merge"] {
        let analysis = analysed(&git);
        let refused = found(&analysis, Rule::StalePromotion);
        assert_eq!(refused.len(), 1, "{:?}", analysis.findings());
        assert!(
            refused[0].starts_with("error[stale-promotion]")
                && refused[0].contains(&older[..12])
                && refused[0].contains("only the tip of main is promoted"),
            "{refused:?}"
        );
        assert_eq!(analysis.findings().len(), 1, "{:?}", analysis.findings());
        assert!(analysis.versions.is_empty());
        git.run(&["update-ref", "refs/remotes/origin/release", &merge]);
    }

    // What the check cannot do: the older commit read on its own is no merge,
    // and Git keeps no record of where main pointed when it was merged, so it
    // resolves to the full version with no finding. Only phase 0 at the merge
    // tells, and the release stage resolves here only once that has passed.
    git.run(&["checkout", "--quiet", "--detach", &older]);
    assert_eq!(version(&git, "lib-v").as_deref(), Some("0.0.2"));

    // Read later, for the same reason, the merge is taken for a promotion
    // like any other, and with no tag read the base moves with it: the
    // commits of main go on from the older commit's full release. Here that
    // gives no number out again; that it cannot in any history is the case
    // left open in #81.
    git.run(&["branch", "--force", "release", &merge]);
    git.run(&["checkout", "--quiet", "main"]);
    git.write(&[("lib/b.md", "# Lib\n\nThree.\n")]);
    git.commit("fix: lib three");
    step(&git, "0.0.3-pre.2");
    // The tip is promoted as ever.
    git.merge_into_release();
    assert!(analysed(&git).findings().is_empty());
    git.run(&["checkout", "--quiet", "main"]);
    git.promote();
    step(&git, "0.0.3");
    git.write(&[("lib/b.md", "# Lib\n\nFour.\n")]);
    git.commit("fix: lib four");
    step(&git, "0.0.4-pre.1");
}

#[test]
fn a_merge_into_release_of_another_branch_is_refused() {
    // Once something is promoted, the first parent of a merge into release is
    // a merge of release, which no pull request into main has.
    let git = units();
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    git.commit("fix: lib one");
    git.promote();
    git.run(&["checkout", "--quiet", "-b", "side"]);
    git.write(&[("lib/b.md", "# Lib\n\nOn the side.\n")]);
    let side = git.commit("fix: side work");
    git.run(&["checkout", "--quiet", "main"]);

    git.merge_into_release_from(&side);
    let analysis = analysed(&git);
    let refused = found(&analysis, Rule::StalePromotion);
    assert_eq!(refused.len(), 1, "{:?}", analysis.findings());
    assert!(refused[0].contains(&side[..12]), "{refused:?}");
    assert!(analysis.failed());
}

#[test]
fn a_moved_or_removed_file_touches_the_unit_it_left() {
    let git = units();
    git.promote();
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
    assert_eq!(release(&git, "lib-v"), "0.0.1-pre.1");

    // Two stages, every tag set on time.
    for number in [2, 3] {
        git.write(&[("lib/b.md", &format!("# Lib\n\nFix {number}.\n"))]);
        git.commit("fix: in the library");
        assert_eq!(release(&git, "lib-v"), format!("0.0.1-pre.{number}"));
    }
    let third = git.head();
    let moved = UNITS.replace("path     = \"lib\"", "path     = \"crates/lib\"");
    std::fs::create_dir(git.path().join("crates")).expect("a folder");
    move_unit(&git, UNITS, "lib", "crates/lib");
    assert_eq!(release(&git, "lib-v"), "0.0.1-pre.4");
    // The declaration changed, which touches the app as well.
    assert_eq!(release(&git, "app-v"), "0.0.2");
    git.write(&[("crates/lib/b.md", "# Lib\n\nAfter the move.\n")]);
    git.commit("fix: after the move");
    assert_eq!(release(&git, "lib-v"), "0.0.1-pre.5");
    git.promote();
    assert_eq!(release(&git, "lib-v"), "0.0.1");

    // One stage, with the tag of the commit before the move set late.
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
    assert_eq!(version(&git, "lib-v").as_deref(), Some("0.0.1-pre.3"));
}

#[test]
fn a_generated_unit_whose_folder_moves_keeps_counting() {
    let git = Git::new();
    git.write(&[
        (
            ".nebensound/repo.toml",
            "[repo]\npipeline = \"none\"\n\n[component.docs]\npath = \"docs\"\npipeline = \
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
    let declared = "[repo]\npipeline = \"none\"\n\n[component.docs]\npath = \"docs\"\npipeline = \
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
fn a_generated_unit_takes_the_pre_release_floor_of_its_parent() {
    let git = Git::new();
    git.write(&[
        (
            ".nebensound/repo.toml",
            "[repo]\npipeline = \"none\"\n\n[component.docs]\npath = \"docs\"\npipeline = \
             \"none\"\nversioning = \"child-files\"\nminimum-version = \"1.0.0-pre.1\"\n",
        ),
        ("README.md", "# Docs\n"),
        ("docs/guide.md", "# Guide\n"),
    ]);
    git.commit("chore: start");

    let analysis = analysed(&git);
    let refused = found(&analysis, Rule::PreReleaseMinimum);
    assert_eq!(refused.len(), 1, "{refused:#?}");
    // Where the parent writes it, since the unit has nowhere to write one.
    assert!(
        refused[0].starts_with(
            "error[pre-release-minimum] component guide: .nebensound/repo.toml, line 8, column 19: "
        ),
        "{refused:#?}"
    );
    assert_eq!(versions(&analysis)["docs-guide-v"], None);
}

#[test]
fn the_developers_configuration_does_not_change_the_answer() {
    let git = units();
    git.promote();
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
            "[repo]\npipeline = \"none\"\n\n[component.docs]\npath = \"docs\"\npipeline = \
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
             "release": "single-stage", "current": "0.0.2", "version": "0.0.2",
             "writers": []},
            {"kind": "component", "name": "guide", "path": "docs/guide.md",
             "prefix": "docs-guide-v", "release": "single-stage", "current": "0.0.2",
             "version": "0.0.2", "writers": []},
            // Not touched: its current version is what the first commit gave it.
            {"kind": "component", "name": "handbook", "path": "docs/handbook.md",
             "prefix": "docs-handbook-v", "release": "single-stage", "current": "0.0.1",
             "version": null, "writers": []}
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
    git.run(&["branch", "release"]);
    git.run(&["update-ref", "refs/remotes/origin/release", "release"]);
    git.run(&["checkout", "--quiet", "-b", "elsewhere"]);
    let elsewhere = git.commit("chore: on a branch nobody released");
    git.run(&["checkout", "--quiet", "main"]);
    git.write(&[("README.md", "# Units\n\nMore.\n")]);
    git.commit("docs: more");
    git.promote();
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
    // A full release of a two-stage unit on a commit that was never promoted,
    // after the first promotion.
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
    // The declaration of `tool` comes in through a merge commit, which
    // touches nothing, and its own commit lies off the line of main.
    let git = units();
    git.run(&["checkout", "--quiet", "-b", "side"]);
    git.write(&[
        (
            ".nebensound/repo.toml",
            &format!("{UNITS}\n[component.tool]\npath     = \"tool\"\npipeline = \"none\"\n"),
        ),
        ("tool/t.md", "# Tool\n"),
    ]);
    let side = git.commit("feat: add the tool");
    git.run(&["checkout", "--quiet", "main"]);
    git.run(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "Merge the tool",
        "side",
    ]);
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["tool-v"], None);
    assert_eq!(current(&analysis, "tool-v"), None);

    // So any tag that reads as one of its releases could meet a release.
    git.tag("tool-v0.0.1", &side);
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
fn a_two_stage_unit_without_the_branch_release_is_an_error() {
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", UNITS),
        ("README.md", "# Units\n"),
        ("app/a.md", "# App\n"),
        ("lib/b.md", "# Lib\n"),
    ]);
    let first = git.commit("chore: start");
    git.tag("lib-v1.0.0", &first);

    let analysis = analysed(&git);
    let missing = found(&analysis, Rule::MissingReleaseBranch);
    assert_eq!(missing.len(), 1, "{:?}", analysis.findings());
    assert!(
        missing[0].starts_with("error[missing-release-branch] component lib"),
        "{missing:?}"
    );
    assert_eq!(versions(&analysis)["lib-v"], None);
    assert_eq!(current(&analysis, "lib-v"), None);
    assert_eq!(versions(&analysis)["app-v"].as_deref(), Some("0.0.1"));
    // Nothing is told of the unit, so nothing is held against its tags.
    assert!(conflicting(&analysis).is_empty());

    // Released in one stage from now on, it still was in two stages at a
    // commit that counts for it: a deleted branch must not move a version.
    git.write(&[(".nebensound/repo.toml", &single_stage())]);
    git.commit("chore: release the library in one stage");
    let analysis = analysed(&git);
    let missing = found(&analysis, Rule::MissingReleaseBranch);
    assert_eq!(missing.len(), 1, "{:?}", analysis.findings());
    assert!(
        missing[0].starts_with("error[missing-release-branch] component lib")
            && missing[0].contains("is or was released in two stages"),
        "{missing:?}"
    );
    assert_eq!(versions(&analysis)["lib-v"], None);
    assert_eq!(versions(&analysis)["app-v"].as_deref(), Some("0.0.2"));
}

#[test]
fn switching_to_one_stage_before_promoting_is_an_error() {
    let git = units();
    git.tag("lib-v0.0.1-pre.1", &git.head());
    let single = UNITS.replace("release  = \"two-stage\"\n", "");
    git.write(&[(".nebensound/repo.toml", &single)]);
    git.commit("chore: release the library in one stage");

    let analysis = analysed(&git);
    let switch = found(&analysis, Rule::UnreleasedBeforeSwitch);
    assert_eq!(switch.len(), 1, "{:?}", analysis.findings());
    assert!(
        switch[0].starts_with("error[unreleased-before-switch] component lib")
            && switch[0].contains("up to lib-v0.0.1-pre.1,")
            && switch[0].contains("revert the switch"),
        "{switch:?}"
    );
    assert_eq!(versions(&analysis)["lib-v"], None);
    // Nothing is released in two stages any more, and the branch release
    // holds what was: it is not pointless.
    assert!(found(&analysis, Rule::PointlessReleaseBranch).is_empty());
    // The same without the tag: the history alone tells it.
    delete_tags(&git);
    assert_eq!(
        found(&analysed(&git), Rule::UnreleasedBeforeSwitch).len(),
        1
    );

    // As the message says: the switch reverted, the tip promoted, and the
    // switch again goes on from the full release.
    git.write(&[(".nebensound/repo.toml", UNITS)]);
    git.commit("revert: release the library in two stages again");
    assert_eq!(release(&git, "lib-v"), "0.0.1-pre.3");
    git.promote();
    assert_eq!(release(&git, "lib-v"), "0.0.1");
    git.write(&[(".nebensound/repo.toml", &single)]);
    git.commit("chore: release the library in one stage");
    let analysis = analysed(&git);
    assert!(found(&analysis, Rule::UnreleasedBeforeSwitch).is_empty());
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.2"));
}

#[test]
fn the_pre_releases_a_promotion_released_do_not_stop_a_switch_to_one_stage() {
    let git = units();
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    git.commit("fix: lib one");
    assert_eq!(release(&git, "lib-v"), "0.0.1-pre.2");
    git.promote();
    assert_eq!(release(&git, "lib-v"), "0.0.1");
    let single = UNITS.replace("release  = \"two-stage\"\n", "");
    git.write(&[(".nebensound/repo.toml", &single)]);
    git.commit("chore: release the library in one stage");

    // The promotion released that commit with every pre-release before it.
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.2"));
    assert_eq!(analysis.versions[2].waiting, None);
}

#[test]
fn a_pre_release_pushed_by_hand_is_a_conflicting_record_and_no_pre_release() {
    let git = units();
    git.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    git.commit("fix: lib one");
    assert_eq!(release(&git, "lib-v"), "0.0.1-pre.2");
    git.promote();
    assert_eq!(release(&git, "lib-v"), "0.0.1");
    // Pushed by hand onto a commit of main after the promotion, which the
    // history gives no pre-release: it does not stop a switch to one stage,
    // as a pre-release given to a commit would.
    git.write(&[("app/a.md", "# App\n\nOne.\n")]);
    let stray = git.commit("fix: app one");
    git.tag("lib-v3.0.0-pre.1", &stray);
    let single = UNITS.replace("release  = \"two-stage\"\n", "");
    git.write(&[(".nebensound/repo.toml", &single)]);
    git.commit("chore: release the library in one stage");
    let analysis = analysed(&git);
    assert!(found(&analysis, Rule::UnreleasedBeforeSwitch).is_empty());
    let conflicting = conflicting(&analysis);
    assert_eq!(conflicting.len(), 1, "{:?}", analysis.findings());
    assert!(
        conflicting[0].contains("\"lib-v3.0.0-pre.1\"") && conflicting[0].contains(&stray[..12]),
        "{conflicting:?}"
    );
    assert_eq!(analysis.findings().len(), 1, "{:?}", analysis.findings());

    // The way out the message names: nobody published it, so it goes.
    git.run(&["tag", "--delete", "lib-v3.0.0-pre.1"]);
    let analysis = analysed(&git);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.2"));
}

#[test]
fn a_version_field_at_its_largest_number_is_an_error() {
    let max = u64::MAX;
    let git = units_declared(&lib_floored(&format!("{max}.0.0")).replace(
        "[repo]\n",
        &format!("[repo]\nminimum-version = \"{max}.0.0\"\n"),
    ));
    git.write(&[("lib/b.md", "# Lib\n\nReleased.\n")]);
    git.commit("fix: something to promote");
    git.promote();
    assert_eq!(version(&git, "lib-v"), Some(format!("{max}.0.0")));
    git.write(&[("lib/b.md", "# Lib\n\nBroken.\n")]);
    git.commit("feat!: one major version too many");

    let analysis = analysed(&git);
    let exhausted = found(&analysis, Rule::ExhaustedVersion);
    assert_eq!(exhausted.len(), 2, "{:?}", analysis.findings());
    for (unit, version) in [
        ("the repository", format!("{max}.0.1")),
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
}

#[test]
fn a_branch_release_without_a_two_stage_unit_is_a_warning() {
    let git = Git::new();
    git.write(&[(".nebensound/repo.toml", QUIET), ("README.md", "# One\n")]);
    git.commit("chore: start");
    git.promote();

    let analysis = analysed(&git);
    let pointless = found(&analysis, Rule::PointlessReleaseBranch);
    assert_eq!(pointless.len(), 1, "{:?}", analysis.findings());
    assert!(
        pointless[0].starts_with("warning[pointless-release-branch]"),
        "{pointless:?}"
    );
    assert!(!analysis.failed());
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

/// The versions of the commit at hand, of the units it touches; [`None`] when
/// phase 0 refuses the commit, which only a pre-release floor in one stage
/// may do in the walk below. Then no release stage runs and nothing is tagged.
fn touched(git: &Git) -> Option<BTreeMap<String, String>> {
    let analysis = analysed(git);
    if analysis.failed() {
        let findings = analysis.findings();
        assert!(
            findings
                .iter()
                .all(|finding| finding.severity != Severity::Error
                    || finding.rule == Rule::PreReleaseMinimum),
            "{findings:?}"
        );
        return None;
    }
    Some(
        versions(&analysis)
            .into_iter()
            .filter_map(|(prefix, version)| Some((prefix, version?)))
            .collect(),
    )
}

/// The declaration of [`units`] with the floors of `app` and `lib` written in.
fn floored([app, lib]: &[String; 2]) -> String {
    UNITS
        .replace(
            "path     = \"app\"\n",
            &format!("path     = \"app\"\n{app}"),
        )
        .replace(
            "path     = \"lib\"\n",
            &format!("path     = \"lib\"\n{lib}"),
        )
}

/// What the walk below declares about `app` or `lib`.
struct Walked {
    /// Its name.
    name: &'static str,
    /// Where it lives now.
    path: String,
    /// Whether it is released in two stages now.
    two_stage: bool,
    /// Its `minimum-version` line, or nothing.
    floor: String,
}

/// The declaration of the walk's units as they stand.
fn declared(units: &[Walked; 2]) -> String {
    let mut text = String::from("[repo]\npipeline = \"none\"\n");
    for unit in units {
        let release = if unit.two_stage {
            "release  = \"two-stage\"\n"
        } else {
            ""
        };
        text.push_str(&["\n[component.", unit.name, "]\npath     = \""].concat());
        text.push_str(&unit.path);
        text.push_str(&["\"\npipeline = \"none\"\n", release, &unit.floor].concat());
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
    /// How often it promoted, moved a floor, set a tag late, wrote a
    /// pre-release floor in two stages, moved a unit, switched a unit's mode,
    /// wrote a pre-release floor in one stage, and pushed a pre-release onto
    /// a commit released in one stage.
    happened: [usize; 8],
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
                    two_stage: false,
                    floor: String::new(),
                },
                Walked {
                    name: "lib",
                    path: "lib".to_owned(),
                    two_stage: true,
                    floor: String::new(),
                },
            ],
            got: BTreeMap::new(),
            holder: BTreeMap::new(),
            late: Vec::new(),
            happened: [0; 8],
        }
    }

    /// Notes what the commit at hand got, and tags it as the release stages
    /// would: at once, or late when `delay` says so and the unit is released
    /// in one stage. A promotion's tag is set before anything else merges.
    fn record(&mut self, delay: bool) {
        let seed = self.seed;
        let head = self.git.head();
        let Some(touched) = touched(&self.git) else {
            return;
        };
        for (prefix, version) in touched {
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
            let two_stage = self
                .units
                .iter()
                .any(|unit| unit.two_stage && format!("{}-v", unit.name) == prefix);
            if delay && !version.contains("-pre.") && !two_stage {
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

    /// A floor raised, lowered or taken away again, or written as a
    /// pre-release. In two stages that sets the number of the next pre-release
    /// when it names the target. In one stage, which gives out none, phase 0
    /// refuses it until the versions have reached its `X.Y.Z`, and the person
    /// then writes `X.Y.Z` instead.
    fn move_floor(&mut self, unit: usize) {
        self.happened[1] += 1;
        let pre = self.random.below(3) != 0;
        let major = self.random.below(4);
        let full = format!("minimum-version = \"{major}.0.0\"\n");
        self.units[unit].floor = match major {
            0 => String::new(),
            _ if pre => format!(
                "minimum-version = \"{major}.0.0-pre.{}\"\n",
                self.random.below(12) + 1
            ),
            _ => full.clone(),
        };
        if major != 0 && pre {
            if self.units[unit].two_stage {
                self.happened[3] += 1;
            } else {
                self.happened[6] += 1;
                self.declare();
                self.git.commit("chore: aim at a pre-release");
                self.record(false);
                self.flush();
                self.units[unit].floor = full;
            }
        }
        self.declare();
    }

    /// A pre-release pushed by hand onto the commit at hand, for a unit that
    /// is released in one stage there. It moves no version: it is the error
    /// `conflicting-tag`, and the person deletes it again.
    fn push_stray(&mut self, step: usize) {
        let Some(unit) = self.units.iter().find(|unit| !unit.two_stage) else {
            return;
        };
        self.happened[7] += 1;
        let tag = format!("{}-v99.0.0-pre.{}", unit.name, step + 1);
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
        self.happened[4] += 1;
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

    /// The unit switches its mode. Before it leaves two stages its last
    /// pre-release is promoted, and a pre-release floor goes, since one stage
    /// gives out none. Before it enters them the release stages have set
    /// every tag, as they take their turns.
    fn switch(&mut self, unit: usize) {
        self.happened[5] += 1;
        if self.units[unit].two_stage {
            self.git.promote();
            self.record(false);
            if self.units[unit].floor.contains("-pre.") {
                self.units[unit].floor = String::new();
            }
        } else {
            self.flush();
        }
        self.units[unit].two_stage = !self.units[unit].two_stage;
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

    /// One step: a promotion, or a commit of some kind.
    ///
    /// No older commit of main is merged into release past the check that
    /// refuses it: read later, such a merge is taken for a promotion and
    /// moves the base, which can give a version out twice. That case is left
    /// open in #81; the refusal itself is checked on its own.
    fn step(&mut self, step: usize) {
        let choice = self.random.below(12);
        let unit = usize::try_from(self.random.below(2)).expect("small");
        match choice {
            0 => {
                self.happened[0] += 1;
                self.git.promote();
                self.record(false);
                return;
            }
            1 | 2 => self.move_floor(unit),
            3 => self.move_unit(unit),
            4 => self.switch(unit),
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
        self.happened[2] += usize::from(delay);
        self.record(delay);
        // The late tags arrive once the next run has read the history.
        if !delay {
            self.flush();
        }
    }

    /// Read again at every commit of main, each commit still comes to what it
    /// got: the version is a function of the history, not of the moment, and
    /// of no tag. A commit phase 0 refused is refused again and got nothing.
    fn read_again(&self) {
        let seed = self.seed;
        let line = self.git.run(&["rev-list", "--first-parent", "main"]);
        for commit in line.lines() {
            self.git.run(&["checkout", "--quiet", "--detach", commit]);
            let Some(touched) = touched(&self.git) else {
                assert!(
                    !self.got.keys().any(|(_, got)| got == commit),
                    "seed {seed}: {commit} refused when read again"
                );
                continue;
            };
            for (prefix, version) in touched {
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
/// away now and then, pre-release floors in either mode, promotions, units
/// that move to another folder, switches from one stage to two and back,
/// pre-releases pushed by hand onto commits released in one stage, and tags
/// that some runs set late. No version may be given to two commits, and read
/// again at the end every commit still comes to the version it got, with
/// every tag and with none.
///
/// Returns how often each kind of step happened ([`Walker::happened`]), so
/// that the test can show that every one of them did.
fn walk(seed: u64, steps: usize) -> [usize; 8] {
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
    let mut seen = [0; 8];
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
        "[repo]\npipeline = \"none\"\nminimum-version = \"3.0.0\"\n",
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
fn a_lowered_floor_in_two_stages_gives_no_version_twice() {
    // The first full release, a step a person takes with minimum-version.
    let git = units_declared(&lib_floored("1.0.0"));
    git.write(&[("lib/b.md", "# Lib\n\nFirst.\n")]);
    git.commit("feat: the first library");
    git.promote();
    assert_eq!(release(&git, "lib-v"), "1.0.0");

    let mut given = vec![
        "lib-v1.0.0-pre.1".to_owned(),
        "lib-v1.0.0-pre.2".to_owned(),
        "lib-v1.0.0".to_owned(),
    ];
    let mut step = |git: &Git, expected: &str| {
        let tag = format!("lib-v{}", release(git, "lib-v"));
        assert_eq!(tag, format!("lib-v{expected}"));
        assert!(!given.contains(&tag), "{tag} for two commits");
        given.push(tag);
    };

    git.write(&[("lib/b.md", "# Lib\n\nFixed.\n")]);
    git.commit("fix: in the library");
    step(&git, "1.0.1-pre.1");
    git.write(&[(
        ".nebensound/repo.toml",
        &UNITS.replace(
            "release  = \"two-stage\"\n",
            "release  = \"two-stage\"\nminimum-version = \"3.0.0\"\n",
        ),
    )]);
    git.commit("chore: aim the library at 3.0.0");
    step(&git, "3.0.0-pre.2");
    // The floor goes again before the promotion: the target stays where the
    // cycle took it, and the promotion releases it.
    git.write(&[(".nebensound/repo.toml", UNITS)]);
    git.commit("revert: drop the library's floor again");
    step(&git, "3.0.0-pre.3");
    git.promote();
    step(&git, "3.0.0");

    // The cycles after it go on from there.
    git.write(&[("lib/b.md", "# Lib\n\nBroken.\n")]);
    git.commit("feat!: break the library");
    step(&git, "4.0.0-pre.1");
    git.promote();
    step(&git, "4.0.0");
    git.write(&[("lib/b.md", "# Lib\n\nBroken again.\n")]);
    git.commit("feat!: break the library again");
    step(&git, "5.0.0-pre.1");
    git.write(&[("lib/b.md", "# Lib\n\nFixed again.\n")]);
    git.commit("fix: in the library again");
    step(&git, "5.0.0-pre.2");
}

#[test]
fn a_pre_release_floor_sets_the_number_of_the_pre_release() {
    let git = units_declared(&lib_floored("1.0.0-pre.1"));
    assert_eq!(release(&git, "lib-v"), "1.0.0-pre.1");
    for step in [2, 3] {
        git.write(&[("lib/b.md", &format!("# Lib\n\nStep {step}.\n"))]);
        git.commit("fix: in the library");
        assert_eq!(release(&git, "lib-v"), format!("1.0.0-pre.{step}"));
    }

    // The commit that sets 1.0.0-pre.10 is that pre-release, and the next
    // one goes on from it.
    git.write(&[(".nebensound/repo.toml", &lib_floored("1.0.0-pre.10"))]);
    git.commit("chore: go on from the tenth pre-release");
    assert_eq!(release(&git, "lib-v"), "1.0.0-pre.10");
    git.write(&[("lib/b.md", "# Lib\n\nAfter the floor.\n")]);
    git.commit("fix: after the floor");
    assert_eq!(release(&git, "lib-v"), "1.0.0-pre.11");

    // A lowered floor has no effect.
    git.write(&[(".nebensound/repo.toml", &lib_floored("1.0.0-pre.5"))]);
    git.commit("chore: lower the floor");
    assert_eq!(release(&git, "lib-v"), "1.0.0-pre.12");
    git.promote();
    assert_eq!(release(&git, "lib-v"), "1.0.0");

    // Once a full release has reached it, the floor does nothing.
    git.write(&[("lib/b.md", "# Lib\n\nThe next cycle.\n")]);
    git.commit("fix: the next cycle");
    assert_eq!(release(&git, "lib-v"), "1.0.1-pre.1");

    // A floor of the target the commits raise to sets the number. Once a
    // floor lifts the target past it, the position among the commits counts
    // again.
    git.write(&[(".nebensound/repo.toml", &lib_floored("2.0.0-pre.3"))]);
    git.commit("feat!: aim at the third pre-release of 2.0.0");
    assert_eq!(release(&git, "lib-v"), "2.0.0-pre.3");
    git.write(&[("lib/b.md", "# Lib\n\nOn the way to 2.0.0.\n")]);
    git.commit("fix: on the way");
    assert_eq!(release(&git, "lib-v"), "2.0.0-pre.4");
    git.write(&[(".nebensound/repo.toml", &lib_floored("3.0.0-pre.2"))]);
    git.commit("chore: aim at 3.0.0");
    assert_eq!(release(&git, "lib-v"), "3.0.0-pre.4");
    git.promote();
    assert_eq!(release(&git, "lib-v"), "3.0.0");
}

#[test]
fn a_pre_release_number_at_its_largest_is_an_error() {
    let max = u64::MAX;
    let git = units_declared(&lib_floored(&format!("1.0.0-pre.{max}")));
    assert_eq!(version(&git, "lib-v"), Some(format!("1.0.0-pre.{max}")));

    git.write(&[("lib/b.md", "# Lib\n\nOne too many.\n")]);
    git.commit("fix: one pre-release too many");
    let analysis = analysed(&git);
    let exhausted = found(&analysis, Rule::ExhaustedVersion);
    assert_eq!(exhausted.len(), 1, "{:?}", analysis.findings());
    assert!(
        exhausted[0].starts_with("error[exhausted-version] component lib")
            && exhausted[0].contains(&format!("1.0.0-pre.{max} cannot be raised")),
        "{exhausted:?}"
    );
    assert_eq!(versions(&analysis)["lib-v"], None);
}

#[test]
fn a_pre_release_floor_in_one_stage_is_an_error_until_the_versions_reach_it() {
    let declaration = |repo: &str, app: &str| {
        floored(&[app.to_owned(), String::new()]).replace(
            "[repo]\npipeline = \"none\"\n",
            &format!("[repo]\npipeline = \"none\"\n{repo}"),
        )
    };
    let git = units_declared(&declaration(
        "minimum-version = \"1.0.0-pre.3\"\n",
        "minimum-version = \"2.0.0-pre.1\"\n",
    ));

    let analysis = analysed(&git);
    let refused = found(&analysis, Rule::PreReleaseMinimum);
    assert_eq!(refused.len(), 2, "{refused:#?}");
    for (unit, place, written) in [
        ("the repository", "line 3, column 19", "1.0.0"),
        ("component app", "line 7, column 19", "2.0.0"),
    ] {
        assert!(
            refused.iter().any(|finding| finding.starts_with(&format!(
                "error[pre-release-minimum] {unit}: .nebensound/repo.toml, {place}: "
            )) && finding
                .contains(&format!("minimum-version = \"{written}\""))
                && finding.contains("two stages")),
            "{unit}: {refused:#?}"
        );
    }
    assert!(analysis.failed());
    assert_eq!(versions(&analysis)["v"], None);
    assert_eq!(versions(&analysis)["app-v"], None);
    // The unit released in two stages beside them is not held up.
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.1-pre.1"));
    let (text, code) = call(&["nebensound-repo", "-C", git.root(), "check"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("line 7, column 19"), "{text}");

    // Taking the floor away is lowering it, which has no effect; the error
    // then points at the unit's table.
    git.write(&[(
        ".nebensound/repo.toml",
        &declaration("minimum-version = \"1.0.0\"\n", ""),
    )]);
    git.commit("chore: write the repository's floor as a full release");
    let analysis = analysed(&git);
    let refused = found(&analysis, Rule::PreReleaseMinimum);
    assert_eq!(refused.len(), 1, "{refused:#?}");
    assert!(
        refused[0].starts_with(
            "error[pre-release-minimum] component app: .nebensound/repo.toml, line 5, column 1: "
        ),
        "{refused:#?}"
    );
    assert_eq!(versions(&analysis)["v"].as_deref(), Some("1.0.0"));
    git.write(&[(
        ".nebensound/repo.toml",
        &declaration(
            "minimum-version = \"1.0.0\"\n",
            "minimum-version = \"2.0.0\"\n",
        ),
    )]);
    git.commit("chore: write the app's floor as a full release");
    assert_eq!(release(&git, "app-v"), "2.0.0");

    // Once the versions have reached its three numbers, a pre-release floor
    // is ignored.
    git.write(&[(
        ".nebensound/repo.toml",
        &declaration(
            "minimum-version = \"1.0.0\"\n",
            "minimum-version = \"2.0.0-pre.7\"\n",
        ),
    )]);
    git.commit("chore: name a pre-release the app has passed");
    let analysis = analysed(&git);
    assert!(
        found(&analysis, Rule::PreReleaseMinimum).is_empty(),
        "{:?}",
        analysis.findings()
    );
    assert_eq!(versions(&analysis)["app-v"].as_deref(), Some("2.0.1"));
}

#[test]
fn a_pre_release_floor_in_one_stage_is_measured_against_the_versions_not_the_tags() {
    let floor = |value: &str| floored(&[format!("minimum-version = \"{value}\"\n"), String::new()]);
    let git = units_declared(&floor("0.5.0"));
    assert_eq!(release(&git, "app-v"), "0.5.0");
    let mut late = Vec::new();
    for (number, expected) in [(1, "0.5.1"), (2, "0.5.2")] {
        git.write(&[("app/a.md", &format!("# App\n\nFix {number}.\n"))]);
        git.commit(&format!("fix: app {number}"));
        assert_eq!(version(&git, "app-v").as_deref(), Some(expected));
        late.push((git.head(), expected));
    }

    // A pre-release floor below what the commits before have come to is
    // reached, whether their tags are set yet or not.
    git.write(&[(".nebensound/repo.toml", &floor("0.5.1-pre.1"))]);
    git.commit("chore: name a pre-release floor the app has passed");
    let analysis = analysed(&git);
    assert!(
        found(&analysis, Rule::PreReleaseMinimum).is_empty(),
        "{:?}",
        analysis.findings()
    );
    assert_eq!(versions(&analysis)["app-v"].as_deref(), Some("0.5.3"));

    for (commit, version) in &late {
        git.tag(&format!("app-v{version}"), commit);
    }
    let analysis = analysed(&git);
    assert!(
        found(&analysis, Rule::PreReleaseMinimum).is_empty(),
        "{:?}",
        analysis.findings()
    );
    assert_eq!(versions(&analysis)["app-v"].as_deref(), Some("0.5.3"));
    git.tag("app-v0.5.3", &git.head());

    // One that no patch reaches is not reached, with every tag set or with
    // the last one late, and the next commit beside the app still sees it.
    git.write(&[("app/a.md", "# App\n\nFix 4.\n")]);
    git.commit("fix: app 4");
    let fourth = git.head();
    assert_eq!(version(&git, "app-v").as_deref(), Some("0.5.4"));
    git.write(&[(".nebensound/repo.toml", &floor("0.6.0-pre.1"))]);
    git.commit("chore: name a pre-release floor the app has not reached");
    let refused = |git: &Git| {
        let analysis = analysed(git);
        assert_eq!(versions(&analysis)["app-v"], None);
        found(&analysis, Rule::PreReleaseMinimum).len()
    };
    assert_eq!(refused(&git), 1);
    git.tag("app-v0.5.4", &fourth);
    assert_eq!(refused(&git), 1);
    git.write(&[("README.md", "# Units\n\nBeside the app.\n")]);
    git.commit("docs: beside the app");
    assert_eq!(refused(&git), 1);
}
