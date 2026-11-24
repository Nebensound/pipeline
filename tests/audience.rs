//! For whom a repository is meant: the field `audience` in `[repo]`, demanded
//! at the commit at hand, held against what a run knows of the repository,
//! carried by the plan and shown by `explain`, and of no weight for the
//! versions of a history.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;

use nebensound_repo::audience::{self, Visibility};
use nebensound_repo::finding::Position;
use nebensound_repo::repository::Repository;
use nebensound_repo::{Analysis, Audience, Declaration, Severity, UnitName};
use support::{Git, analysis, call, central, repository};

/// A declaration with this audience, or none when it is [`None`].
fn declared(audience: Option<&str>) -> String {
    let line = audience.map_or_else(String::new, |value| format!("audience = \"{value}\"\n"));
    format!("[repo]\n{line}pipeline = \"none\"\n")
}

/// The codes of everything phase 0 found.
fn codes(analysis: &Analysis) -> Vec<&'static str> {
    analysis
        .findings()
        .iter()
        .map(|finding| finding.rule.code())
        .collect()
}

/// Each of the three values reads, and passes phase 0.
#[test]
fn the_field_takes_exactly_three_values() {
    for (value, audience) in [
        ("public", Audience::Public),
        ("internal", Audience::Internal),
        ("customer", Audience::Customer),
    ] {
        let source = declared(Some(value));
        let declaration = Declaration::parse(&source).expect("a declaration");
        assert_eq!(declaration.repo.audience, Some(audience));
        assert_eq!(audience.to_string(), value);

        let found = analysis(&source, &[".nebensound/repo.toml"]);
        assert!(
            found.findings().is_empty(),
            "{value}: {:?}",
            found.findings()
        );
        assert_eq!(found.audience, Some(audience));
    }

    let refused = Declaration::parse(&declared(Some("everyone"))).expect_err("not a value");
    let message = refused.to_string();
    assert!(message.contains("everyone"), "{message}");
    assert!(message.contains("customer"), "{message}");
}

#[test]
fn a_declaration_without_the_field_is_an_error_of_its_own() {
    let found = analysis(&declared(None), &[".nebensound/repo.toml"]);

    assert_eq!(codes(&found), ["missing-audience"]);
    assert!(found.failed());
    assert_eq!(found.audience, None);
    let finding = found.findings()[0].clone();
    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(finding.unit, Some(UnitName::Repository));
    assert_eq!(
        finding.path.map(|path| path.to_string()).as_deref(),
        Some(".nebensound/repo.toml")
    );
    for value in ["\"public\"", "\"internal\"", "\"customer\""] {
        assert!(finding.message.contains(value), "{}", finding.message);
    }
}

#[test]
fn declare_fails_without_the_field_and_names_the_finding() {
    let dir = repository(&[(".nebensound/repo.toml", &declared(None))]);
    let root = dir.path().to_str().unwrap();

    let (text, code) = call(&["nebensound-repo", "-C", root, "check"]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("error[missing-audience] the repository at .nebensound/repo.toml: "),
        "{text}"
    );

    let (text, code) = call(&["nebensound-repo", "-C", root, "plan"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.starts_with("there is no plan"), "{text}");
}

/// What `check` says with a visibility, and its exit code.
fn checked(audience: &str, visibility: Option<&str>) -> (String, u8) {
    let dir = repository(&[(".nebensound/repo.toml", &declared(Some(audience)))]);
    let root = dir.path().to_str().unwrap();
    let mut args = vec!["nebensound-repo", "-C", root, "check"];
    if let Some(visibility) = visibility {
        args.extend(["--visibility", visibility]);
    }
    call(&args)
}

#[test]
fn public_in_a_private_repository_fails_in_a_run_and_passes_locally() {
    let (text, code) = checked("public", Some("private"));
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("error[mismatched-audience] the repository at .nebensound/repo.toml: "),
        "{text}"
    );
    assert!(text.contains("audience = \"public\""), "{text}");
    assert!(text.contains("as private"), "{text}");
    assert!(text.contains("make the repository public"), "{text}");

    // On a developer's machine nothing is compared.
    let (text, code) = checked("public", None);
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("mismatched-audience"), "{text}");
}

#[test]
fn public_is_declared_exactly_when_the_repository_is_public() {
    for (audience, visibility, fits) in [
        ("public", "public", true),
        ("public", "private", false),
        ("public", "internal", false),
        ("internal", "public", false),
        ("internal", "private", true),
        ("internal", "internal", true),
        ("customer", "public", false),
        ("customer", "private", true),
        ("customer", "internal", true),
    ] {
        let (text, code) = checked(audience, Some(visibility));
        assert_eq!(code, u8::from(!fits), "{audience} in {visibility}: {text}");
        assert_eq!(
            text.contains("mismatched-audience"),
            !fits,
            "{audience} in {visibility}: {text}"
        );
    }

    let (text, _) = checked("customer", Some("public"));
    assert!(text.contains("as public"), "{text}");
    assert!(
        text.contains("declare \"public\", or make the repository private"),
        "{text}"
    );
    let (text, _) = checked("public", Some("internal"));
    assert!(text.contains("as internal"), "{text}");
}

#[test]
fn a_visibility_is_one_github_knows() {
    let dir = repository(&[(".nebensound/repo.toml", &declared(Some("public")))]);
    let root = dir.path().to_str().unwrap();

    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root,
        "check",
        "--visibility",
        "secret",
    ]);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("secret"), "{text}");
}

#[test]
fn without_an_audience_there_is_nothing_to_compare() {
    // The missing field is reported once, by its own finding.
    assert_eq!(audience::against(None, Visibility::Public), None);

    let dir = repository(&[(".nebensound/repo.toml", &declared(None))]);
    let root = dir.path().to_str().unwrap();
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root,
        "check",
        "--visibility",
        "public",
    ]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("missing-audience"), "{text}");
    assert!(!text.contains("mismatched-audience"), "{text}");
}

#[test]
fn a_finding_about_the_audience_hangs_at_the_declaration_in_the_tree() {
    let found = analysis(&declared(Some("internal")), &[".nebensound/repo.toml"])
        .with_visibility(Visibility::Public);

    assert_eq!(codes(&found), ["mismatched-audience"]);
    assert!(found.loose.is_empty(), "{:?}", found.loose);
    let finding = found.findings()[0];
    assert_eq!(finding.position, None::<Position>);

    // Without the file in the tree it hangs at no path.
    let found =
        analysis(&declared(Some("internal")), &["README.md"]).with_visibility(Visibility::Public);
    assert_eq!(codes(&found), ["mismatched-audience"]);
    assert_eq!(found.loose.len(), 1, "{:?}", found.loose);
}

#[test]
fn explain_shows_the_audience_for_every_path() {
    let dir = repository(&[
        (".nebensound/repo.toml", &declared(Some("customer"))),
        ("README.md", "# x\n"),
    ]);
    let root = dir.path().to_str().unwrap();
    let (text, code) = call(&["nebensound-repo", "-C", root, "explain", "README.md"]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.lines()
            .any(|line| line.starts_with("  Audience") && line.ends_with("customer")),
        "{text}"
    );

    let dir = repository(&[
        (".nebensound/repo.toml", &declared(None)),
        ("README.md", "# x\n"),
    ]);
    let root = dir.path().to_str().unwrap();
    let (text, _) = call(&["nebensound-repo", "-C", root, "explain", "README.md"]);
    assert!(
        text.lines()
            .any(|line| line.starts_with("  Audience") && line.ends_with("none declared")),
        "{text}"
    );
}

/// The plan of the fixture as JSON, for a pull request when given one.
fn plan(git: &Git, pull_request: Option<&str>) -> serde_json::Value {
    let mut args = vec!["nebensound-repo", "-C", git.root(), "plan"];
    if let Some(number) = pull_request {
        args.extend(["--pull-request", number, "--message", "chore: change it"]);
    }
    let (text, code) = call(&args);
    assert_eq!(code, 0, "{text}");
    serde_json::from_str(&text).expect("the plan is JSON")
}

#[test]
fn the_plan_carries_the_value_and_the_one_before() {
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", &declared(None)),
        ("README.md", "# x\n"),
    ]);
    git.commit("chore: start without an audience");
    git.write(&[(".nebensound/repo.toml", &declared(Some("internal")))]);
    git.commit("chore: say for whom");

    let json = plan(&git, None);
    assert_eq!(json["audience"], "internal");
    assert_eq!(json["previous_audience"], serde_json::Value::Null);

    // A pull request that changes the value: the one before is that of its
    // base branch.
    git.write(&[(".nebensound/repo.toml", &declared(Some("public")))]);
    git.commit("chore: open it up");
    let json = plan(&git, Some("7"));
    assert_eq!(json["audience"], "public");
    assert_eq!(json["previous_audience"], "internal");

    // One that leaves it alone carries it twice.
    git.write(&[("README.md", "# y\n")]);
    git.commit("docs: reword");
    let json = plan(&git, Some("8"));
    assert_eq!(json["audience"], "public");
    assert_eq!(json["previous_audience"], "public");
}

#[test]
fn the_value_before_is_read_also_from_a_declaration_phase_0_cannot_read() {
    let git = Git::new();
    git.write(&[
        (
            ".nebensound/repo.toml",
            "[repo]\naudience = \"customer\"\npublsh = false\n",
        ),
        ("README.md", "# x\n"),
    ]);
    git.commit("chore: start with a misspelt field");
    git.write(&[(".nebensound/repo.toml", &declared(Some("internal")))]);
    git.commit("chore: correct it");

    assert_eq!(plan(&git, None)["previous_audience"], "customer");

    // A value that is none of the three is no value before.
    git.write(&[(".nebensound/repo.toml", "[repo]\naudience = \"everyone\"\n")]);
    git.commit("chore: name no valid audience");
    git.write(&[(".nebensound/repo.toml", &declared(Some("internal")))]);
    git.commit("chore: correct it again");
    assert_eq!(
        plan(&git, None)["previous_audience"],
        serde_json::Value::Null
    );
}

#[test]
fn the_first_commit_has_no_value_before() {
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", &declared(Some("public"))),
        ("README.md", "# x\n"),
    ]);
    git.commit("chore: start");

    let analysis = Repository::at(git.path())
        .analyse(&central())
        .expect("the fixture reads");
    assert_eq!(analysis.audience, Some(Audience::Public));
    assert_eq!(analysis.previous_audience, None);
}

/// Every version and current version of every unit at the commit at hand,
/// by prefix.
fn resolved(git: &Git) -> BTreeMap<String, (Option<String>, Option<String>)> {
    let analysis = Repository::at(git.path())
        .analyse(&central())
        .expect("the fixture reads");
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    analysis
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
        .collect()
}

/// A history of a repository with a component, every earlier
/// declaration with the given audience line, and the commit at hand with the
/// field whatever came before. Returns the versions at the commit at hand,
/// which the declarations of every commit of the line count into.
fn history(earlier: &str) -> BTreeMap<String, (Option<String>, Option<String>)> {
    let declaration = |audience: &str, floor: &str| {
        format!("[repo]\n{audience}pipeline = \"none\"\n\n[component.lib]\npath = \"lib\"\n{floor}")
    };
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", &declaration(earlier, "")),
        ("README.md", "# x\n"),
        ("lib/a.md", "# A\n"),
    ]);
    git.commit("feat: start");
    git.write(&[("lib/b.md", "# B\n")]);
    git.commit("feat!: change the library");
    git.write(&[("README.md", "# y\n")]);
    git.commit("fix: reword");
    git.write(&[(
        ".nebensound/repo.toml",
        &declaration(earlier, "minimum-version = \"0.1.0\"\n"),
    )]);
    git.commit("chore: raise the floor");
    git.write(&[("lib/c.md", "# C\n")]);
    git.commit("feat: add to the library");
    // The commit at hand declares the field in both histories, and changes
    // the declaration in both.
    git.write(&[(
        ".nebensound/repo.toml",
        &declaration(
            "audience = \"public\"\n",
            "minimum-version = \"0.1.0\"\n# Said for whom.\n",
        ),
    )]);
    git.commit("chore: say for whom");
    resolved(&git)
}

#[test]
fn the_versions_of_a_history_are_the_same_with_and_without_the_field_before() {
    let without = history("");
    let with = history("audience = \"public\"\n");

    assert_eq!(without, with);
    // The history gives something to compare: the component's versions move.
    assert_eq!(
        without["lib-v"],
        (Some("0.1.2".to_owned()), Some("0.1.2".to_owned()))
    );
}
