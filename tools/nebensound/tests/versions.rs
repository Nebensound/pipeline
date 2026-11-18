//! Which version every unit carries, read from real Git repositories: tags,
//! the first-parent line of `HEAD` and the branch `origin/release`.
//!
//! Each fixture is built commit by commit in a temporary directory and tagged
//! the way the release stages tag it, so that what is checked is the history a
//! repository really has.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use nebensound::repository::Repository;
use nebensound_declaration::{Analysis, Rule, Severity};
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

/// The last full release of one unit at the commit at hand.
fn base(analysis: &Analysis, prefix: &str) -> String {
    analysis
        .versions
        .iter()
        .find(|unit| unit.prefix == prefix)
        .map_or_else(
            || panic!("no unit with the prefix {prefix}"),
            |unit| unit.base.to_string(),
        )
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

    // The run of this commit is late with its tag: the next one comes to the
    // number it would have had all the same, raised by each commit since the
    // last tag on its own.
    git.write(&[("README.md", "# One\n\nFixed.\n")]);
    git.commit("fix: a correction");
    let late = git.head();
    assert_eq!(version(&git, "v").as_deref(), Some("0.1.2"));

    git.write(&[("README.md", "# One\n\nBroken.\n")]);
    git.commit("refactor!: a breaking change");
    let analysis = analysed(&git);
    assert_eq!(versions(&analysis)["v"].as_deref(), Some("0.2.0"));
    assert_eq!(base(&analysis, "v"), "0.1.1");

    git.tag("v0.1.2", &late);
    let analysis = analysed(&git);
    assert_eq!(versions(&analysis)["v"].as_deref(), Some("0.2.0"));
    assert_eq!(base(&analysis, "v"), "0.1.2");
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

    // A commit that carries its release is that release, also on a second
    // run.
    git.run(&["checkout", "--quiet", "--detach", &feature]);
    assert_eq!(version(&git, "v").as_deref(), Some("0.1.1"));
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

    git.write(&[("lib/b.md", "# Lib\n\nNew.\n")]);
    git.commit("feat: something new in the library");
    assert_eq!(release(&git, "lib-v"), "1.0.0-pre.4");

    // The promoted commit is the full release of the target.
    let promoted = git.promote();
    let analysis = analysed(&git);
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("1.0.0"));
    assert_eq!(base(&analysis, "lib-v"), "0.0.0");
    git.tag("lib-v1.0.0", &promoted);
    let analysis = analysed(&git);
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("1.0.0"));
    assert_eq!(base(&analysis, "lib-v"), "1.0.0");

    // The next merge starts the next pre-release from it.
    git.write(&[("lib/b.md", "# Lib\n\nFixed again.\n")]);
    git.commit("fix: the next correction");
    assert_eq!(release(&git, "lib-v"), "1.0.1-pre.1");

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

#[test]
fn a_release_on_a_merged_side_branch_is_ignored() {
    let git = units();
    git.promote();
    git.tag("app-v0.0.1", &git.head());
    git.run(&["checkout", "--quiet", "-b", "side"]);
    git.write(&[("app/a.md", "# App\n\nOn the side.\n")]);
    let side = git.commit("feat: on a side branch");
    // In one stage only a commit of the first-parent line is released.
    git.tag("app-v3.0.0", &side);
    // In either mode only a commit of that line is given a pre-release; in
    // one stage it would otherwise stop the unit as one never promoted.
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
    let unexpected = found(&analysis, Rule::UnexpectedTag);
    assert_eq!(unexpected.len(), 3, "{unexpected:#?}");
    for (tag, unit, why) in [
        (
            "app-v3.0.0",
            "component app",
            "full release on a commit off",
        ),
        (
            "app-v0.0.9-pre.1",
            "component app",
            "pre-release on a commit off",
        ),
        (
            "lib-v0.0.9-pre.1",
            "component lib",
            "pre-release on a commit off",
        ),
    ] {
        assert!(
            unexpected.iter().any(|finding| finding
                .starts_with(&format!("warning[unexpected-tag] {unit}"))
                && finding.contains(&format!("\"{tag}\""))
                && finding.contains(why)),
            "{tag}: {unexpected:#?}"
        );
    }
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    assert_eq!(base(&analysis, "app-v"), "0.0.1");
    // The commit after the merge is the one since the base that touched it.
    assert_eq!(versions(&analysis)["app-v"].as_deref(), Some("0.0.2"));
}

#[test]
fn a_full_release_counts_in_two_stages_only_where_it_was_released() {
    let git = units();
    git.write(&[("lib/b.md", "# Lib\n\nBefore the switch.\n")]);
    let before = git.commit("fix: from when the library was released in one stage");
    git.tag("lib-v2.0.0", &before);

    // Nothing is promoted yet, so every commit of main is older than the
    // first promotion, and its full release counts.
    git.write(&[("lib/b.md", "# Lib\n\nThe first cycle.\n")]);
    git.commit("fix: the first cycle");
    let analysis = analysed(&git);
    assert_eq!(base(&analysis, "lib-v"), "2.0.0");
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("2.0.1-pre.1"));
    git.promote();
    assert_eq!(release(&git, "lib-v"), "2.0.1");

    // A full release pushed by hand on a commit of main after the first
    // promotion, and never promoted itself.
    git.write(&[("lib/b.md", "# Lib\n\nThe second cycle.\n")]);
    let stray = git.commit("fix: the second cycle");
    git.tag("lib-v5.0.0", &stray);
    git.write(&[("lib/b.md", "# Lib\n\nStill the second cycle.\n")]);
    git.commit("fix: still the second cycle");
    // The promotion makes origin/release contain it; that does not make it
    // a release.
    git.promote();

    let analysis = analysed(&git);
    let unexpected = found(&analysis, Rule::UnexpectedTag);
    assert_eq!(unexpected.len(), 1, "{unexpected:?}");
    assert!(
        unexpected[0].starts_with("warning[unexpected-tag] component lib")
            && unexpected[0].contains("\"lib-v5.0.0\"")
            && unexpected[0].contains("never promoted"),
        "{unexpected:?}"
    );
    // The release from before the switch and the promoted one still count.
    assert_eq!(base(&analysis, "lib-v"), "2.0.1");
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("2.0.2"));
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
    // A full release pushed by hand on the tip of the side branch.
    git.tag("lib-v7.0.0", &side);

    let analysis = analysed(&git);
    let unexpected = found(&analysis, Rule::UnexpectedTag);
    assert_eq!(unexpected.len(), 1, "{unexpected:?}");
    assert!(
        unexpected[0].starts_with("warning[unexpected-tag] component lib")
            && unexpected[0].contains("\"lib-v7.0.0\"")
            && unexpected[0].contains("never promoted"),
        "{unexpected:?}"
    );
    // The releases from before the switch count, and the promoted commit
    // goes on from the newest of them.
    assert_eq!(base(&analysis, "lib-v"), "0.0.3");
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.4"));
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
    assert_eq!(base(&analysis, "docs-guide-v"), "0.0.1");
    assert_eq!(
        versions(&analysis)["docs-guide-v"].as_deref(),
        Some("0.0.2")
    );
    assert_eq!(versions(&analysis)["docs-handbook-v"], None);

    let (text, code) = call(&["nebensound", "-C", git.root(), "plan"]);
    assert_eq!(code, 0, "{text}");
    let plan: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(
        plan["versions"],
        serde_json::json!([
            {"kind": "repository", "name": null, "path": ".", "prefix": "v",
             "release": "single-stage", "base": "0.0.0", "version": "0.0.2"},
            {"kind": "component", "name": "guide", "path": "docs/guide.md",
             "prefix": "docs-guide-v", "release": "single-stage", "base": "0.0.1",
             "version": "0.0.2"},
            {"kind": "component", "name": "handbook", "path": "docs/handbook.md",
             "prefix": "docs-handbook-v", "release": "single-stage", "base": "0.0.0",
             "version": null}
        ])
    );
}

#[test]
fn a_hand_pushed_tag_is_ignored_with_a_warning() {
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

    // An annotated tag counts as well as a lightweight one.
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
    // Not a version the pipeline makes.
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
    let unexpected = found(&analysis, Rule::UnexpectedTag);
    assert_eq!(unexpected.len(), 4, "{unexpected:#?}");
    for (tag, unit) in [
        ("lib-v5.0.0", "component lib"),
        ("v1.0", "the repository"),
        ("app-v1.0.0-rc.1", "component app"),
        ("v2.0.0", "the repository"),
    ] {
        assert!(
            unexpected
                .iter()
                .any(|finding| finding.contains(&format!("\"{tag}\""))
                    && finding.contains(unit)
                    && finding.starts_with("warning[unexpected-tag]")),
            "{tag}: {unexpected:#?}"
        );
    }
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    assert_eq!(base(&analysis, "v"), "0.0.1");
    assert_eq!(base(&analysis, "lib-v"), "0.0.0");
    assert_eq!(versions(&analysis)["v"].as_deref(), Some("0.0.3"));
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
    let (text, code) = call(&["nebensound", "-C", root, "plan"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("there is no plan"), "{text}");
    let (text, code) = call(&["nebensound", "-C", root, "check"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("error[shallow-clone]"), "{text}");
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
    assert_eq!(versions(&analysis)["app-v"].as_deref(), Some("0.0.1"));
    assert!(found(&analysis, Rule::UnexpectedTag).is_empty());
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
    assert!(switch[0].contains("lib-v0.0.1-pre.1"), "{switch:?}");
    assert_eq!(versions(&analysis)["lib-v"], None);
    // The branch release is left, and nothing is released in two stages.
    assert_eq!(found(&analysis, Rule::PointlessReleaseBranch).len(), 1);

    // Promoted first, the switch goes on from the full release.
    git.run(&["reset", "--quiet", "--hard", "HEAD~1"]);
    let promoted = git.promote();
    git.tag("lib-v0.0.1", &promoted);
    git.write(&[(".nebensound/repo.toml", &single)]);
    git.commit("chore: release the library in one stage");
    let analysis = analysed(&git);
    assert!(found(&analysis, Rule::UnreleasedBeforeSwitch).is_empty());
    assert_eq!(versions(&analysis)["lib-v"].as_deref(), Some("0.0.2"));
}

#[test]
fn a_version_field_at_its_largest_number_is_an_error() {
    let max = u64::MAX;
    let git = units();
    let promoted = git.promote();
    git.tag(&format!("v{max}.0.0"), &promoted);
    git.tag(&format!("lib-v{max}.0.0"), &promoted);
    git.write(&[("lib/b.md", "# Lib\n\nBroken.\n")]);
    git.commit("feat!: one major version too many");

    let analysis = analysed(&git);
    let exhausted = found(&analysis, Rule::ExhaustedVersion);
    assert_eq!(exhausted.len(), 2, "{:?}", analysis.findings());
    assert!(
        exhausted
            .iter()
            .all(|finding| finding.contains(&format!("{max}.0.0 cannot be raised"))),
        "{exhausted:?}"
    );
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
fn a_commit_without_a_declaration_still_counts() {
    // The repository adopted the declaration later; what came before is part
    // of its history all the same, and names no floor.
    let git = Git::new();
    git.write(&[("README.md", "# Late\n")]);
    git.commit("chore: before the declaration");
    git.write(&[(
        ".nebensound/repo.toml",
        "[repo]\npipeline = \"none\"\nminimum-version = \"0.1.0\"\n",
    )]);
    git.commit("feat: declare the repository");
    assert_eq!(version(&git, "v").as_deref(), Some("0.1.0"));

    // Without the floor in its own declaration, the first commit is 0.0.1.
    git.run(&["checkout", "--quiet", "--detach", "HEAD~1"]);
    git.write(&[(".nebensound/repo.toml", QUIET)]);
    assert_eq!(version(&git, "v").as_deref(), Some("0.0.1"));
}

#[test]
fn a_repository_without_a_commit_has_no_version_yet() {
    let dir = repository(&[(".nebensound/repo.toml", QUIET), ("README.md", "# x\n")]);

    let analysis = Repository::at(dir.path())
        .analyse(&central())
        .expect("the fixture reads");
    assert_eq!(analysis.versions.len(), 1);
    assert_eq!(analysis.versions[0].version, None);
    assert_eq!(analysis.versions[0].base.to_string(), "0.0.0");
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

/// The versions of the commit at hand, of the units it touches.
fn touched(git: &Git) -> BTreeMap<String, String> {
    let analysis = analysed(git);
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    versions(&analysis)
        .into_iter()
        .filter_map(|(prefix, version)| Some((prefix, version?)))
        .collect()
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

/// One walk through a history: commits that touch the units at random, with
/// every kind of message, a floor of either stage raised, lowered or taken
/// away now and then, pre-release floors on the unit released in two stages,
/// promotions, and tags that some runs set late. No version may be given to
/// two commits, and read again at the end every commit still comes to the
/// version it got.
///
/// Returns how often it promoted, moved a floor, set a tag late and wrote a
/// pre-release floor, so that the test can show that every one of them
/// happened.
fn walk(seed: u64, steps: usize) -> [usize; 4] {
    let mut random = Walk(seed);
    let mut happened = [0; 4];
    let git = units();
    // What each commit got, by unit, and which commit holds each version.
    let mut got: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut holder: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut late = Vec::new();
    // The floors of `app` and `lib` as the declaration names them now.
    let mut floors = [String::new(), String::new()];

    let mut record = |git: &Git,
                      versions: BTreeMap<String, String>,
                      late: &mut Vec<(String, String)>,
                      delay: bool| {
        let head = git.head();
        for (prefix, version) in versions {
            let owner = holder
                .entry((prefix.clone(), version.clone()))
                .or_insert_with(|| head.clone());
            assert_eq!(
                *owner, head,
                "seed {seed}: {prefix}{version} for two commits"
            );
            got.insert((prefix.clone(), head.clone()), version.clone());
            let tag = format!("{prefix}{version}");
            let pending = late.iter().any(|(waiting, _)| *waiting == tag);
            if !pending && git.run(&["tag", "--list", &tag]).is_empty() {
                // A run of a single-stage unit may set its tag late; a
                // promotion's tag is set before anything else merges.
                if delay && !version.contains("-pre.") && prefix != "lib-v" {
                    late.push((tag, head.clone()));
                } else {
                    git.tag(&tag, &head);
                }
            }
        }
    };

    record(&git, touched(&git), &mut late, false);
    for step in 0..steps {
        let choice = random.below(10);
        if choice == 0 {
            happened[0] += 1;
            git.promote();
            record(&git, touched(&git), &mut late, false);
            continue;
        }
        if choice <= 2 {
            // Raised, lowered or taken away again, on either stage, and on
            // `lib` also as a pre-release, which sets the number of the next
            // pre-release when it names the target.
            happened[1] += 1;
            let unit = usize::try_from(random.below(2)).expect("small");
            let pre = unit == 1 && random.below(3) != 0;
            floors[unit] = match random.below(4) {
                0 => String::new(),
                major if pre => format!(
                    "minimum-version = \"{major}.0.0-pre.{}\"\n",
                    random.below(12) + 1
                ),
                major => format!("minimum-version = \"{major}.0.0\"\n"),
            };
            happened[3] += usize::from(pre);
            git.write(&[(".nebensound/repo.toml", &floored(&floors))]);
        } else {
            for (bit, path) in [(1, "app/a.md"), (2, "lib/b.md"), (4, "README.md")] {
                if random.below(8) & bit != 0 {
                    let previous = std::fs::read_to_string(git.path().join(path)).expect("a file");
                    git.write(&[(path, &format!("{previous}\nStep {step}.\n"))]);
                }
            }
        }
        let message = [
            "fix: a correction",
            "feat: something new",
            "feat!: a breaking change",
            "docs: words",
            "Update things",
            "fix: a correction\n\nBREAKING CHANGE: it breaks",
        ][usize::try_from(random.below(6)).expect("small")];
        git.commit(message);
        let delay = random.below(3) == 0;
        happened[2] += usize::from(delay);
        record(&git, touched(&git), &mut late, delay);
        // The late tags arrive once the next run has read the history.
        if !delay {
            for (tag, commit) in late.drain(..) {
                git.tag(&tag, &commit);
            }
        }
    }
    for (tag, commit) in late.drain(..) {
        git.tag(&tag, &commit);
    }

    // Read again at every commit of main, each commit still comes to what it
    // got: the version is a function of the history, not of the moment.
    let line = git.run(&["rev-list", "--first-parent", "main"]);
    for commit in line.lines() {
        git.run(&["checkout", "--quiet", "--detach", commit]);
        for (prefix, version) in touched(&git) {
            assert_eq!(
                got.get(&(prefix.clone(), commit.to_owned())),
                Some(&version),
                "seed {seed}: {prefix} at {commit}"
            );
        }
    }
    happened
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
    git.tag("v1.0.0", &git.head());

    git.write(&[(
        ".nebensound/repo.toml",
        "[repo]\npipeline = \"none\"\nminimum-version = \"3.0.0\"\n",
    )]);
    let lifted = git.commit("feat: aim at 3.0.0");
    assert_eq!(version(&git, "v").as_deref(), Some("3.0.0"));

    // Its tag is late, and the floor is taken away meanwhile. Lowering it has
    // no effect: since the base, 3.0.0 is still the highest floor named.
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
    let git = units();
    git.write(&[("lib/b.md", "# Lib\n\nFirst.\n")]);
    git.commit("feat: the first library");
    // The first full release, set by a person on a promoted commit.
    let first = git.promote();
    git.tag("lib-v1.0.0", &first);

    let mut given = vec!["lib-v1.0.0".to_owned()];
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
fn a_pre_release_floor_in_one_stage_is_an_error_until_the_base_reaches_it() {
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
    let (text, code) = call(&["nebensound", "-C", git.root(), "check"]);
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

    // Once the base has reached its three numbers, a pre-release floor is
    // ignored.
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
