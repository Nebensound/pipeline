//! Where a repository departs from the organization's conventions: its name,
//! its default branch and the files every repository holds.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound_repo::conformance::{
    Identity, Told, branch_from_head, conforming_name, findings, name_from_address, notes,
    suggested_name,
};
use nebensound_repo::finding::{Finding, Position, Rule, Severity};
use nebensound_repo::output::Output;
use nebensound_repo::path::RelativePath;
use nebensound_repo::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER};
use nebensound_repo::report;
use support::{DEPENDABOT, QUIET, analysis, call, git, repository};

/// Every minimum file, at the place it is written anew.
const MINIMUM: &[(&str, &str)] = &[
    (".nebensound/repo.toml", QUIET),
    ("README.md", "# x\n"),
    ("LICENSE", "All rights reserved.\n"),
    ("AGENTS.md", "# Rules\n"),
    (".github/CODEOWNERS", "* @example\n"),
    (".github/dependabot.yml", DEPENDABOT),
    (".gitignore", "/target\n"),
    (".editorconfig", "root = true\n"),
];

fn paths(values: &[&str]) -> Vec<RelativePath> {
    values
        .iter()
        .map(|value| RelativePath::new(value).expect("a path in the fixture"))
        .collect()
}

fn known(name: &str, branch: &str) -> Identity {
    Identity {
        name: Told::Known(name.to_owned()),
        default_branch: Told::Known(branch.to_owned()),
    }
}

fn root(dir: &tempfile::TempDir) -> &str {
    dir.path().to_str().expect("a path in UTF-8")
}

#[test]
fn a_name_conforms_in_lowercase_words_joined_by_single_hyphens() {
    for name in ["pipeline", "a1-b2-c3", "x", "2026", ".github"] {
        assert!(conforming_name(name), "{name}");
    }
    for name in [
        "My_Tool", "my_tool", "MyTool", "my.tool", "a--b", "-a", "a-", "", ".GitHub", "über",
    ] {
        assert!(!conforming_name(name), "{name}");
    }
}

#[test]
fn the_suggested_name_lowercases_and_joins_with_hyphens_without_guessing_words() {
    for (name, suggested) in [
        ("My_Tool", "my-tool"),
        ("MyTool", "mytool"),
        ("my.tool", "my-tool"),
        ("my__tool", "my-tool"),
        ("a-_.b", "a-b"),
        ("_private_", "private"),
        (".GitHub", "github"),
        ("Pipeline", "pipeline"),
    ] {
        assert_eq!(suggested_name(name), suggested, "{name}");
        assert!(conforming_name(suggested), "{suggested}");
    }
}

#[test]
fn the_name_is_the_end_of_the_address_of_origin() {
    for (address, name) in [
        (
            "https://github.com/Nebensound/pipeline.git",
            Some("pipeline"),
        ),
        ("https://github.com/Nebensound/My_Tool", Some("My_Tool")),
        (
            "https://github.com/Nebensound/pipeline/\n",
            Some("pipeline"),
        ),
        ("git@github.com:Nebensound/pipeline.git", Some("pipeline")),
        ("git@host:pipeline.git", Some("pipeline")),
        ("ssh://git@github.com/Nebensound/pipeline", Some("pipeline")),
        ("/home/me/repositories/pipeline", Some("pipeline")),
        ("C:\\repositories\\pipeline\\", Some("pipeline")),
        ("../pipeline", Some("pipeline")),
        ("https://github.com/Nebensound/.git", None),
        ("", None),
    ] {
        assert_eq!(name_from_address(address).as_deref(), name, "{address:?}");
    }
}

#[test]
fn the_default_branch_is_what_origin_head_names() {
    assert_eq!(
        branch_from_head("refs/remotes/origin/main\n").as_deref(),
        Some("main")
    );
    assert_eq!(
        branch_from_head("refs/remotes/origin/develop/2026").as_deref(),
        Some("develop/2026")
    );
    assert_eq!(branch_from_head("refs/remotes/upstream/main"), None);
    assert_eq!(branch_from_head("refs/remotes/origin/"), None);
}

#[test]
fn a_name_with_capitals_or_underscores_gets_the_warning_with_the_suggested_name() {
    let files = paths(&["README.md"]);
    for (name, suggested) in [("My_Tool", "my-tool"), ("Pipeline", "pipeline")] {
        let found = findings(&known(name, "main"), &files);
        let warning = found
            .iter()
            .find(|finding| finding.rule == Rule::NonconformingName)
            .expect("a warning about the name");
        assert_eq!(warning.severity, Severity::Warning);
        assert_eq!(warning.rule.code(), "nonconforming-name");
        assert!(warning.message.contains(name), "{warning}");
        assert!(
            warning
                .message
                .contains(&format!("{suggested} would conform")),
            "{warning}"
        );
        assert_eq!(warning.path, None);
    }
}

#[test]
fn a_suggestion_that_would_not_conform_itself_is_left_out() {
    let files = paths(&["README.md"]);
    for name in ["über", "My Repo", "_.-"] {
        let found = findings(&known(name, "main"), &files);
        let warning = found
            .iter()
            .find(|finding| finding.rule == Rule::NonconformingName)
            .expect("a warning about the name");
        assert!(warning.message.contains(name), "{warning}");
        assert!(
            warning
                .message
                .ends_with("lowercase letters and digits joined by single hyphens"),
            "{warning}"
        );
    }
}

#[test]
fn a_conforming_name_and_main_get_no_warning() {
    let found = findings(&known("pipeline", "main"), &paths(&[]));
    assert!(
        found
            .iter()
            .all(|finding| finding.rule == Rule::MissingMinimumFile),
        "{found:?}"
    );
    let found = findings(&known(".github", "main"), &paths(&[]));
    assert!(
        found
            .iter()
            .all(|finding| finding.rule == Rule::MissingMinimumFile),
        "{found:?}"
    );
}

#[test]
fn a_default_branch_other_than_main_gets_a_warning() {
    let found = findings(&known("pipeline", "master"), &paths(&[]));
    let warning = found
        .iter()
        .find(|finding| finding.rule == Rule::NonconformingDefaultBranch)
        .expect("a warning about the branch");
    assert_eq!(warning.severity, Severity::Warning);
    assert_eq!(warning.rule.code(), "nonconforming-default-branch");
    assert!(warning.message.contains("master"), "{warning}");
}

#[test]
fn every_missing_minimum_file_is_named_at_the_path_it_is_written_at() {
    let found = findings(&known("pipeline", "main"), &paths(&[]));
    let missing: Vec<String> = found
        .iter()
        .map(|finding| {
            assert_eq!(finding.rule, Rule::MissingMinimumFile);
            assert_eq!(finding.rule.code(), "missing-minimum-file");
            assert_eq!(finding.severity, Severity::Warning);
            let path = finding.path.as_ref().expect("the missing file").to_string();
            assert!(
                finding.message.contains(&format!("write it at {path}")),
                "{finding}"
            );
            path
        })
        .collect();
    assert_eq!(
        missing,
        [
            "README.md",
            "LICENSE",
            "AGENTS.md",
            ".github/CODEOWNERS",
            ".gitignore",
            ".editorconfig",
        ]
    );
}

#[test]
fn every_place_and_name_github_honours_counts() {
    for files in [
        &[
            "README.md",
            "LICENSE",
            "AGENTS.md",
            "CODEOWNERS",
            ".gitignore",
            ".editorconfig",
        ][..],
        &[
            ".github/readme.rst",
            "LICENSE-MIT",
            "AGENTS.md",
            "docs/CODEOWNERS",
            ".gitignore",
            ".editorconfig",
        ],
        &[
            "docs/README",
            "COPYING.txt",
            "AGENTS.md",
            ".github/CODEOWNERS",
            ".gitignore",
            ".editorconfig",
        ],
        &[
            "Readme.txt",
            "licence.md",
            "AGENTS.md",
            "CODEOWNERS",
            ".gitignore",
            ".editorconfig",
        ],
        &[
            "README.md",
            "UNLICENSE",
            "AGENTS.md",
            "CODEOWNERS",
            ".gitignore",
            ".editorconfig",
        ],
    ] {
        let found = findings(&known("pipeline", "main"), &paths(files));
        assert!(found.is_empty(), "{files:?}: {found:?}");
    }
}

#[test]
fn a_file_elsewhere_or_under_another_name_does_not_count() {
    let found = findings(
        &known("pipeline", "main"),
        &paths(&[
            "src/README.md",
            "READMEFIRST.md",
            "docs/LICENSE",
            "agents.md",
            "docs/AGENTS.md",
            "codeowners",
            "src/CODEOWNERS",
            "src/.gitignore",
            ".github/.editorconfig",
        ]),
    );
    assert_eq!(found.len(), 6, "{found:?}");
}

#[test]
fn what_is_not_known_is_not_checked_and_said_in_one_line() {
    let identity = Identity {
        name: Told::Unknown("Git knows no address of origin".to_owned()),
        default_branch: Told::Unknown("Git does not know".to_owned()),
    };
    let found = findings(&identity, &paths(&[]));
    assert!(
        found
            .iter()
            .all(|finding| finding.rule == Rule::MissingMinimumFile),
        "{found:?}"
    );
    assert_eq!(
        notes(&identity),
        [
            "The repository's name is not checked: Git knows no address of origin.",
            "The default branch is not checked: Git does not know.",
        ]
    );
    assert!(notes(&known("pipeline", "main")).is_empty());
}

#[test]
fn the_warnings_never_fail_phase_0() {
    // A non-conforming fixture: capitals and an underscore, master, and no
    // minimum file but the README.
    let dir = repository(&[(".nebensound/repo.toml", QUIET), ("README.md", "# x\n")]);
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root(&dir),
        "check",
        "--name",
        "My_Tool",
        "--default-branch",
        "master",
    ]);

    assert_eq!(code, 0, "warnings stop nothing: {text}");
    assert!(
        text.contains(
            "warning[nonconforming-name]: the repository is named My_Tool, and a name is \
             lowercase letters and digits joined by single hyphens; my-tool would conform"
        ),
        "{text}"
    );
    assert!(
        text.contains("warning[nonconforming-default-branch]: the default branch is master"),
        "{text}"
    );
    assert!(
        text.contains("warning[missing-minimum-file] at LICENSE: the repository holds no licence"),
        "{text}"
    );
    assert!(text.contains("0 errors, 8 warnings"), "{text}");
    assert!(!text.contains("is not checked"), "{text}");

    // The plan is made all the same: what Check starts does not depend on
    // the warnings.
    let (plan, code) = call(&["nebensound-repo", "-C", root(&dir), "plan"]);
    assert_eq!(code, 0, "{plan}");
    assert!(plan.starts_with('{'), "{plan}");
}

#[test]
fn a_conforming_repository_reports_nothing() {
    let dir = repository(MINIMUM);
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root(&dir),
        "check",
        "--name=pipeline",
        "--default-branch=main",
    ]);

    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Findings\n\n  none"), "{text}");
    assert!(
        text.contains("8 files, 0 gaps, 0 errors, 0 warnings"),
        "{text}"
    );
}

#[test]
fn without_the_options_git_tells_the_name_and_the_default_branch() {
    let dir = repository(MINIMUM);
    git(
        dir.path(),
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:example/Some_Tool.git",
        ],
    );
    git(
        dir.path(),
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/trunk",
        ],
    );
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir)]);

    assert_eq!(code, 0, "{text}");
    assert!(text.contains("some-tool would conform"), "{text}");
    assert!(text.contains("the default branch is trunk"), "{text}");
    assert!(!text.contains("is not checked"), "{text}");

    // What the call names wins over what Git knows.
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root(&dir),
        "check",
        "--name",
        "some-tool",
        "--default-branch",
        "main",
    ]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Findings\n\n  none"), "{text}");
}

#[test]
fn where_git_does_not_know_it_says_so_in_one_line_and_does_not_guess() {
    let dir = repository(MINIMUM);
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "check"]);

    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Findings\n\n  none"), "{text}");
    assert!(
        text.contains(
            "\n\nThe repository's name is not checked: Git knows no address of origin, and the \
             call names none with --name.\n\n"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "\n\nThe default branch is not checked: Git does not know which branch origin/HEAD \
             names (`git remote set-head origin --auto` tells it), and the call names none with \
             --default-branch.\n\n"
        ),
        "{text}"
    );

    // An address without a name, and an origin/HEAD that names no branch of
    // origin.
    git(
        dir.path(),
        &["remote", "add", "origin", "https://github.com/example/.git"],
    );
    git(
        dir.path(),
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/upstream/main",
        ],
    );
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "check"]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains(
            "The repository's name is not checked: the address of origin, \
             \"https://github.com/example/.git\", ends without a name."
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "The default branch is not checked: origin/HEAD names \
             \"refs/remotes/upstream/main\", which is no branch of origin."
        ),
        "{text}"
    );
}

#[test]
fn a_git_that_cannot_read_origin_head_stops_the_check() {
    let dir = repository(MINIMUM);
    let head = dir.path().join(".git/refs/remotes/origin/HEAD");
    std::fs::create_dir_all(head.parent().expect("a folder")).expect("the folder of the ref");
    std::fs::write(&head, "garbage\n").expect("a broken ref");
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root(&dir),
        "check",
        "--name=pipeline",
    ]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("refs/remotes/origin/HEAD"), "{text}");
    assert!(!text.contains("Findings"), "{text}");
}

#[test]
fn an_empty_name_or_branch_is_a_wrong_call() {
    let dir = repository(MINIMUM);
    for option in ["--name=", "--default-branch="] {
        let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "check", option]);
        assert_eq!(code, 2, "{option}: {text}");
    }
}

#[test]
fn in_a_run_every_finding_is_an_annotation() {
    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("README.md", "# x\n"),
        ("src/main.rs", "fn main() {}\n"),
    ]);
    let mut out = Vec::new();
    let code = nebensound_repo::run_against(
        [
            "nebensound-repo",
            "-C",
            root(&dir),
            "check",
            "--name",
            "My_Tool",
        ],
        BUILTIN_REGISTER,
        BUILTIN_DEFAULTS,
        Output::Workflow,
        &mut out,
    );
    let text = String::from_utf8(out).expect("the output is UTF-8");

    assert_eq!(code, 1, "{text}");
    // Every finding stands outside the stopped stretches, where the runner
    // takes it as an annotation.
    let outside = support::outside(&text);
    let stands = |start: &str| outside.iter().any(|line| line.starts_with(start));
    // A finding at a file of the tree names that file.
    assert!(
        stands("::error file=src/main.rs,title=uncovered-file::[uncovered-file] at src/main.rs: "),
        "{text}"
    );
    // One about the repository names none.
    assert!(
        stands(
            "::warning title=nonconforming-name::[nonconforming-name] the repository is named \
             My_Tool, and a name"
        ),
        "{text}"
    );
    // Nor does one at a file the repository does not hold: it would point
    // nowhere.
    assert!(
        stands(
            "::warning title=missing-minimum-file::[missing-minimum-file] at LICENSE: the \
             repository holds no licence"
        ),
        "{text}"
    );
    // The one-line notes and the summary stay as they are.
    assert!(
        text.contains("The default branch is not checked: "),
        "{text}"
    );
    // The Dependabot configuration is not compared beside an error: what a
    // declaration with errors gives could watch the wrong folders.
    assert!(text.contains("1 error, 6 warnings"), "{text}");
    assert!(!text.contains("stale-dependabot"), "{text}");

    // Without a finding there is nothing to annotate.
    let clean = repository(MINIMUM);
    let mut out = Vec::new();
    let code = nebensound_repo::run_against(
        [
            "nebensound-repo",
            "-C",
            root(&clean),
            "check",
            "--name=pipeline",
            "--default-branch=main",
        ],
        BUILTIN_REGISTER,
        BUILTIN_DEFAULTS,
        Output::Workflow,
        &mut out,
    );
    let text = support::printed(&String::from_utf8(out).expect("the output is UTF-8"));
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Findings\n\n  none"), "{text}");
    assert!(!text.contains("::"), "{text}");
}

#[test]
fn an_annotation_names_the_place_in_its_file_and_escapes_what_would_end_it() {
    let mut analysis = analysis(QUIET, &["docs/a.md", "docs/b,c:d.md"]);
    let at = |value: &str| RelativePath::new(value).expect("a path in the fixture");
    let loose = analysis.tree.attach(vec![
        Finding::error(Rule::PartialPackageExclusion, "100% wrong\nhere")
            .at(at("docs/a.md"))
            .on(Position { line: 3, column: 7 }),
        Finding::warning(Rule::PointlessExclusion, "a folder").at(at("docs")),
        Finding::warning(Rule::PointlessExclusion, "odd name").at(at("docs/b,c:d.md")),
    ]);
    assert!(loose.is_empty(), "{loose:?}");
    let text = report::commands(&analysis).join("\n");

    assert_eq!(
        text,
        "::warning title=pointless-exclusion::[pointless-exclusion] at docs: a folder\n\
         ::error file=docs/a.md,line=3,col=7,title=partial-package-exclusion::\
         [partial-package-exclusion] at docs/a.md:3:7: 100%25 wrong\\nhere\n\
         ::warning file=docs/b%2Cc%3Ad.md,title=pointless-exclusion::[pointless-exclusion] \
         at docs/b,c:d.md: odd name"
    );
}

#[test]
fn a_terminal_is_told_before_the_run_it_is_attached_in() {
    assert_eq!(Output::detect(true, false), Output::Workflow);
    assert_eq!(Output::detect(true, true), Output::Terminal);
    assert_eq!(Output::detect(false, true), Output::Terminal);
    assert_eq!(Output::detect(false, false), Output::Elsewhere);
}
