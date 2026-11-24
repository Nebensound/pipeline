//! The Dependabot configuration a declaration and the policy give, and the
//! warning when the file in the tree departs from it.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound_repo::Central;
use nebensound_repo::central::CentralError;
use nebensound_repo::dependabot::{Entry, differences, entries, findings, render};
use nebensound_repo::finding::{Rule, Severity};
use nebensound_repo::path::RelativePath;
use nebensound_repo::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER};
use support::{DEPENDABOT, QUIET, analysis, call, central, repository};
use yaml_rust2::{Yaml, YamlLoader};

/// A service in Rust at the root: its workspace, its Dockerfile, its workflow.
const RUST_SERVICE: &str = "[repo]\naudience = \"internal\"\npipeline = \"rust-service\"\n";

const RUST_SERVICE_FILES: &[&str] = &[
    ".nebensound/repo.toml",
    "Cargo.toml",
    "Cargo.lock",
    "src/main.rs",
    "deploy/Dockerfile",
    ".github/workflows/ci.yml",
];

fn yaml(source: &str) -> Yaml {
    YamlLoader::load_from_str(source)
        .expect("YAML in the fixture")
        .into_iter()
        .next()
        .expect("a document in the fixture")
}

fn root(dir: &tempfile::TempDir) -> &str {
    dir.path().to_str().expect("a path in UTF-8")
}

/// The one entry for this ecosystem.
fn entry<'a>(entries: &'a [Entry], ecosystem: &str) -> &'a Entry {
    entries
        .iter()
        .find(|entry| entry.package_ecosystem == ecosystem)
        .unwrap_or_else(|| panic!("an entry for {ecosystem} in {entries:?}"))
}

#[test]
fn the_rust_service_preset_gives_cargo_docker_and_github_actions_with_the_policy() {
    let found = entries(&analysis(RUST_SERVICE, RUST_SERVICE_FILES), &central());

    let ecosystems: Vec<&str> = found
        .iter()
        .map(|entry| entry.package_ecosystem.as_str())
        .collect();
    assert_eq!(ecosystems, ["cargo", "docker", "github-actions"]);

    assert_eq!(entry(&found, "cargo").directories, ["/"]);
    assert_eq!(entry(&found, "docker").directories, ["/deploy"]);
    assert_eq!(entry(&found, "github-actions").directories, ["/"]);
    for each in &found {
        assert_eq!(each.interval, "weekly");
        assert_eq!(each.group, "minor-and-patch");
        assert_eq!(each.update_types, ["minor", "patch"]);
        assert_eq!(each.open_pull_requests_limit, 5);
    }
    assert_eq!(entry(&found, "cargo").prefix, "deps");
    assert_eq!(entry(&found, "docker").prefix, "docker(deps)");
    assert_eq!(entry(&found, "github-actions").prefix, "ci(deps)");
    assert!(entry(&found, "cargo").exclude_patterns.is_empty());
    assert!(entry(&found, "docker").exclude_patterns.is_empty());
    assert_eq!(
        entry(&found, "github-actions").exclude_patterns,
        ["Nebensound/pipeline/.github/workflows/nebensound.yml"],
        "the pin of the pipeline is a pull request of its own"
    );
}

#[test]
fn the_python_service_preset_gives_uv_docker_and_github_actions() {
    let found = entries(
        &analysis(
            "[repo]\naudience = \"internal\"\npipeline = \"python-service\"\n",
            &["pyproject.toml", "uv.lock", "app/main.py", "Dockerfile"],
        ),
        &central(),
    );
    let ecosystems: Vec<(&str, &str)> = found
        .iter()
        .map(|entry| (entry.package_ecosystem.as_str(), entry.prefix.as_str()))
        .collect();
    assert_eq!(
        ecosystems,
        [
            ("docker", "docker(deps)"),
            ("github-actions", "ci(deps)"),
            ("uv", "deps")
        ]
    );
}

#[test]
fn a_folder_counts_only_inside_a_unit_that_watches_it_and_outside_every_exclusion() {
    let source = "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n\
                  [component.tool]\npath = \"tools/tool\"\npipeline = \"rust-workspace\"\n\n\
                  [component.xtask]\npath = \"xtask\"\npipeline = \"rust-workspace\"\n\n\
                  [[exclude]]\npath = \"xtask/cases/broken\"\nreason = \"wrong on purpose\"\n";
    let found = entries(
        &analysis(
            source,
            &[
                "tools/tool/Cargo.lock",
                "xtask/Cargo.lock",
                "xtask/cases/broken/Cargo.lock",
                "examples/Cargo.lock",
                "tools/tool/Dockerfile",
            ],
        ),
        &central(),
    );
    assert_eq!(
        entry(&found, "cargo").directories,
        ["/tools/tool", "/xtask"],
        "sorted, the excluded case and the unit that does not watch cargo left out"
    );
    assert!(
        found
            .iter()
            .all(|entry| entry.package_ecosystem != "docker"),
        "no unit watches docker: {found:?}"
    );

    // A watched ecosystem whose every manifest is excluded gets no entry.
    let found = entries(
        &analysis(
            "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n\n\
             [[exclude]]\npath = \"cases\"\nreason = \"wrong on purpose\"\n",
            &["cases/Cargo.lock"],
        ),
        &central(),
    );
    assert!(
        found.iter().all(|entry| entry.package_ecosystem != "cargo"),
        "{found:?}"
    );
}

#[test]
fn composite_actions_put_their_glob_beside_the_workflows() {
    for definition in ["action.yml", "action.yaml"] {
        let found = entries(
            &analysis(
                QUIET,
                &[
                    ".github/workflows/ci.yml",
                    &format!(".github/actions/setup/{definition}"),
                ],
            ),
            &central(),
        );
        assert_eq!(
            entry(&found, "github-actions").directories,
            ["/", "/.github/actions/*"]
        );
    }
    // Neither deeper nor elsewhere.
    let found = entries(
        &analysis(
            QUIET,
            &[
                ".github/actions/setup/tests/case/action.yml",
                "actions/setup/action.yml",
                ".github/actions/action.yml",
                ".github/other/setup/action.yml",
                ".github/actions/setup/README.md",
            ],
        ),
        &central(),
    );
    assert_eq!(entry(&found, "github-actions").directories, ["/"]);
}

#[test]
fn one_folder_is_written_as_directory_and_several_as_directories() {
    let found = entries(&analysis(QUIET, &[]), &central());
    assert_eq!(render(&found), DEPENDABOT);

    let found = entries(
        &analysis(
            QUIET,
            &[".github/actions/a/action.yml", ".github/workflows/ci.yml"],
        ),
        &central(),
    );
    let text = render(&found);
    assert!(
        text.contains("    directories:\n      - /\n      - /.github/actions/*\n"),
        "{text}"
    );
    assert!(!text.contains("directory:"), "{text}");

    let found = entries(&analysis(RUST_SERVICE, RUST_SERVICE_FILES), &central());
    let text = render(&found);
    // Only github-actions sets patterns to keep out of the group.
    assert_eq!(text.matches("exclude-patterns:").count(), 1, "{text}");
    assert!(text.contains("      prefix: docker(deps)\n"), "{text}");
    assert!(
        text.starts_with("# Written by `nebensound-repo dependabot`"),
        "{text}"
    );
    // Dependabot applies and creates its own labels only without the key.
    assert!(!text.contains("labels"), "{text}");
}

#[test]
fn the_printed_file_differs_from_itself_in_nothing() {
    for (source, files) in [
        (QUIET, &[][..]),
        (RUST_SERVICE, RUST_SERVICE_FILES),
        (QUIET, &[".github/actions/a/action.yml"][..]),
    ] {
        let found = entries(&analysis(source, files), &central());
        assert!(differences(&found, &yaml(&render(&found))).is_empty());
    }
}

/// The rust service's file with one line replaced.
fn edited(from: &str, to: &str) -> Vec<String> {
    let found = entries(&analysis(RUST_SERVICE, RUST_SERVICE_FILES), &central());
    let text = render(&found);
    assert!(text.contains(from), "{from:?} stands in {text}");
    differences(&found, &yaml(&text.replacen(from, to, 1)))
}

#[test]
fn editing_any_value_of_the_policy_by_hand_is_named_by_its_key_path() {
    assert_eq!(
        edited("version: 2", "version: 1"),
        ["version is 1, and the policy sets 2"]
    );
    assert_eq!(
        edited("interval: weekly", "interval: daily"),
        ["updates[0].schedule.interval is daily, and the policy sets weekly"]
    );
    assert_eq!(
        edited(
            "open-pull-requests-limit: 5",
            "open-pull-requests-limit: 10"
        ),
        ["updates[0].open-pull-requests-limit is 10, and the policy sets 5"]
    );
    assert_eq!(
        edited(
            "open-pull-requests-limit: 5",
            "open-pull-requests-limit: true"
        ),
        ["updates[0].open-pull-requests-limit is true, and the policy sets 5"]
    );
    assert_eq!(
        edited("prefix: deps", "prefix: chore"),
        ["updates[0].commit-message.prefix is chore, and the policy sets deps"]
    );
    assert_eq!(
        edited("          - patch\n", ""),
        [
            "updates[0].groups.minor-and-patch.update-types is [minor], and the policy sets \
             [minor, patch]"
        ]
    );
    assert_eq!(
        edited(
            "          - Nebensound/pipeline/.github/workflows/nebensound.yml\n",
            "          - Nebensound/pipeline\n"
        ),
        [
            "updates[2].groups.minor-and-patch.exclude-patterns is [Nebensound/pipeline], and \
             the policy sets [Nebensound/pipeline/.github/workflows/nebensound.yml]"
        ]
    );
    assert_eq!(
        edited("    directory: /deploy\n", "    directory: /docker\n"),
        ["updates[1].directories lacks /deploy"]
    );
    assert_eq!(
        edited("package-ecosystem: docker", "package-ecosystem: bundler"),
        ["updates has no entry for docker"]
    );
}

#[test]
fn what_the_generator_does_not_set_is_tolerated() {
    // `labels` among it: the policy sets none, so labels added by hand stand.
    let found = entries(&analysis(RUST_SERVICE, RUST_SERVICE_FILES), &central());
    let file = yaml(
        "# By hand, and all the same.\n\
         version: 2\n\
         registries:\n  own:\n    type: cargo-registry\n\
         updates:\n\
         \x20 - package-ecosystem: bundler\n    directory: /\n\
         \x20 - package-ecosystem: github-actions\n    directories: ['/', '/.github/workflows']\n\
         \x20   schedule: {interval: weekly, day: monday}\n\
         \x20   groups:\n      minor-and-patch:\n        update-types: [patch, minor]\n\
         \x20       exclude-patterns: [Nebensound/pipeline/.github/workflows/nebensound.yml]\n\
         \x20   open-pull-requests-limit: 5\n    commit-message: {prefix: ci(deps), include: scope}\n\
         \x20   labels: [dependencies]\n    reviewers: [someone]\n\
         \x20 - package-ecosystem: cargo\n    directory: ./\n\
         \x20   schedule: {interval: weekly}\n\
         \x20   groups: {minor-and-patch: {update-types: [minor, patch]}, other: {patterns: ['*']}}\n\
         \x20   open-pull-requests-limit: 5\n    commit-message: {prefix: deps}\n\
         \x20   labels: [dependencies]\n\
         \x20 - package-ecosystem: docker\n    directories: [deploy/]\n\
         \x20   schedule: {interval: weekly}\n\
         \x20   groups: {minor-and-patch: {update-types: [minor, patch]}}\n\
         \x20   open-pull-requests-limit: 5\n    commit-message: {prefix: docker(deps)}\n\
         \x20   labels: [dependencies]\n",
    );
    assert_eq!(differences(&found, &file), Vec::<String>::new());
}

#[test]
fn the_folders_of_several_entries_count_together_and_each_entry_follows_the_policy() {
    let found = entries(
        &analysis(
            "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n",
            &["a/Cargo.lock", "b/Cargo.lock", "c/Cargo.lock"],
        ),
        &central(),
    );
    let policy = "schedule: {interval: weekly}\n    \
                  groups: {minor-and-patch: {update-types: [minor, patch]}}\n    \
                  open-pull-requests-limit: 5\n    commit-message: {prefix: deps}";
    let file = yaml(&format!(
        "version: 2\nupdates:\n\
         \x20 - package-ecosystem: cargo\n    directory: /a\n    {policy}\n\
         \x20 - package-ecosystem: cargo\n    directory: /b\n    {}\n\
         \x20 - package-ecosystem: github-actions\n    directory: /\n",
        policy.replace("weekly", "monthly")
    ));
    let named = differences(&found, &file);
    assert!(
        named.contains(&"updates[0,1].directories lacks /c".to_owned()),
        "{named:?}"
    );
    assert!(
        named.contains(
            &"updates[1].schedule.interval is monthly, and the policy sets weekly".to_owned()
        ),
        "{named:?}"
    );
}

#[test]
fn a_file_without_entries_misses_every_one() {
    let found = entries(&analysis(RUST_SERVICE, RUST_SERVICE_FILES), &central());
    for file in ["version: 2\n", "version: 2\nupdates: none\n", "just text\n"] {
        let named = differences(&found, &yaml(file));
        for ecosystem in ["cargo", "docker", "github-actions"] {
            assert!(
                named.contains(&format!("updates has no entry for {ecosystem}")),
                "{file}: {named:?}"
            );
        }
    }
    let named = differences(&found, &yaml("updates: []\n"));
    assert_eq!(named[0], "version is missing, and the policy sets 2");
}

/// The findings for a tree whose files read as given.
fn findings_for(files: &[(&str, Result<&str, &str>)]) -> Vec<nebensound_repo::Finding> {
    let mut paths = vec![".nebensound/repo.toml"];
    paths.extend(files.iter().map(|(path, _)| *path));
    let analysis = analysis(QUIET, &paths);
    let read = |path: &RelativePath| {
        files
            .iter()
            .find(|(name, _)| path.to_string() == *name)
            .map(|(_, content)| content.map(ToOwned::to_owned).map_err(ToOwned::to_owned))
            .expect("only a file of the tree is read")
    };
    findings(&analysis, &central(), &read)
}

#[test]
fn the_file_in_the_tree_is_compared_under_either_name() {
    assert!(findings_for(&[(".github/dependabot.yml", Ok(DEPENDABOT))]).is_empty());
    assert!(findings_for(&[(".github/dependabot.yaml", Ok(DEPENDABOT))]).is_empty());
    // With both, the one ending in .yml is the one compared.
    assert!(
        findings_for(&[
            (".github/dependabot.yaml", Ok("version: 1\n")),
            (".github/dependabot.yml", Ok(DEPENDABOT)),
        ])
        .is_empty()
    );
}

#[test]
fn a_missing_file_an_unreadable_one_and_a_differing_one_get_one_warning_each() {
    let missing = findings_for(&[]);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].rule, Rule::StaleDependabot);
    assert_eq!(missing[0].rule.code(), "stale-dependabot");
    assert_eq!(missing[0].severity, Severity::Warning);
    assert_eq!(
        missing[0].path.as_ref().map(ToString::to_string).as_deref(),
        Some(".github/dependabot.yml")
    );
    assert!(
        missing[0]
            .message
            .contains("holds no Dependabot configuration"),
        "{}",
        missing[0]
    );

    for content in [Err("it lies behind a link"), Ok("updates: [\n")] {
        let unreadable = findings_for(&[(".github/dependabot.yaml", content)]);
        assert_eq!(unreadable.len(), 1, "{unreadable:?}");
        assert_eq!(unreadable[0].rule, Rule::StaleDependabot);
        assert_eq!(
            unreadable[0]
                .path
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some(".github/dependabot.yaml")
        );
        assert!(
            unreadable[0].message.contains("cannot be read ("),
            "{}",
            unreadable[0]
        );
    }

    let differing = findings_for(&[(
        ".github/dependabot.yml",
        Ok(&DEPENDABOT
            .replace("weekly", "daily")
            .replace("prefix: ci(deps)", "prefix: ci")),
    )]);
    assert_eq!(
        differing.len(),
        1,
        "one finding for the file: {differing:?}"
    );
    assert!(
        differing[0].message.contains(
            "updates[0].schedule.interval is daily, and the policy sets weekly; \
             updates[0].commit-message.prefix is ci, and the policy sets ci(deps)"
        ),
        "{}",
        differing[0]
    );

    // An empty file holds nothing of what the policy sets.
    let empty = findings_for(&[(".github/dependabot.yml", Ok(""))]);
    assert!(
        empty[0].message.contains("version is missing"),
        "{}",
        empty[0]
    );
}

#[test]
fn the_command_prints_the_file_and_check_holds_the_tree_against_it() {
    let dir = repository(&[(".nebensound/repo.toml", QUIET), ("README.md", "# x\n")]);
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "dependabot"]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(text, DEPENDABOT);

    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "check"]);
    assert_eq!(code, 0, "a warning stops nothing: {text}");
    assert!(
        text.contains(
            "warning[stale-dependabot] at .github/dependabot.yml: the repository holds no \
             Dependabot configuration"
        ),
        "{text}"
    );

    let edited = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("README.md", "# x\n"),
        (
            ".github/dependabot.yml",
            &DEPENDABOT.replace("open-pull-requests-limit: 5", "open-pull-requests-limit: 9"),
        ),
    ]);
    let (text, code) = call(&["nebensound-repo", "-C", root(&edited), "check"]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains(
            "warning[stale-dependabot] at .github/dependabot.yml: the Dependabot configuration \
             departs from what the declaration and the policy give: \
             updates[0].open-pull-requests-limit is 9, and the policy sets 5."
        ),
        "{text}"
    );
    // Only check looks for it.
    let (text, _) = call(&["nebensound-repo", "-C", root(&edited), "tree"]);
    assert!(!text.contains("stale-dependabot"), "{text}");
}

#[test]
fn a_declaration_with_errors_gets_no_file() {
    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        ("src/main.rs", "fn main() {}\n"),
    ]);
    let (text, code) = call(&["nebensound-repo", "-C", root(&dir), "dependabot"]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(
        text,
        "there is no Dependabot configuration: phase 0 found errors, and `check` says which\n"
    );
}

#[test]
fn a_repository_cannot_set_the_policy() {
    let refused = nebensound_repo::Declaration::parse(
        "[repo]\naudience = \"public\"\n\n[dependabot]\ninterval = \"daily\"\n",
    )
    .expect_err("the policy is central");
    assert!(
        refused.to_string().contains("unknown field `dependabot`"),
        "{refused}"
    );
}

#[test]
fn the_compiled_in_policy_is_the_one_the_defaults_hold() {
    let central = Central::parse(BUILTIN_REGISTER, BUILTIN_DEFAULTS).expect("the built-in data");
    let policy = central.dependabot();
    assert_eq!(policy.interval, "weekly");
    assert_eq!(policy.open_pull_requests_limit, 5);
    assert_eq!(policy.group.update_types, ["minor", "patch"]);
}

/// The built-in defaults with one line of the policy replaced.
fn defaults_with(from: &str, to: &str) -> String {
    assert!(
        BUILTIN_DEFAULTS.contains(from),
        "{from:?} stands in the defaults"
    );
    BUILTIN_DEFAULTS.replacen(from, to, 1)
}

#[test]
fn a_policy_dependabot_would_not_take_does_not_read() {
    for (from, to, problem) in [
        (
            "interval                 = \"weekly\"",
            "interval = \"hourly\"",
            "the interval",
        ),
        (
            "update-types = [\"minor\", \"patch\"]",
            "update-types = []",
            "groups []",
        ),
        (
            "update-types = [\"minor\", \"patch\"]",
            "update-types = [\"minor\", \"tiny\"]",
            "a group takes",
        ),
        (
            "commit-prefix = \"deps\"",
            "commit-prefix = \"\"",
            "commit prefix \"\"",
        ),
        (
            "commit-prefix      = \"ci(deps)\"",
            &format!("commit-prefix = \"{}\"", "x".repeat(51)),
            "1 to 50 characters",
        ),
        (
            "[dependabot.ecosystem.docker]",
            "[dependabot.ecosystem.bundler]",
            "names the ecosystem bundler",
        ),
    ] {
        let error = Central::parse(BUILTIN_REGISTER, &defaults_with(from, to))
            .expect_err("a policy Dependabot would not take");
        assert!(matches!(error, CentralError::Policy { .. }), "{error}");
        assert!(error.to_string().contains(problem), "{problem}: {error}");
        assert!(
            error
                .to_string()
                .starts_with("the Dependabot policy in the defaults ")
        );
    }

    // Without one, or with a key it does not know: `labels` too, which the
    // policy leaves to Dependabot.
    let without = BUILTIN_DEFAULTS
        .split_once("[dependabot]")
        .map(|(before, _)| before.to_owned())
        .expect("the policy stands in the defaults");
    let error = Central::parse(BUILTIN_REGISTER, &without).expect_err("no policy");
    assert!(error.to_string().contains("is missing"), "{error}");
    let error = Central::parse(
        BUILTIN_REGISTER,
        &defaults_with(
            "open-pull-requests-limit = 5\n",
            "open-pull-requests-limit = 5\nlabels = [\"dependencies\"]\n",
        ),
    )
    .expect_err("a key the policy does not know");
    assert!(matches!(error, CentralError::Policy { .. }), "{error}");
    // It names the table, and where in the text the key stands.
    let message = error.to_string();
    assert!(
        message.starts_with("the Dependabot policy in the defaults is not readable: "),
        "{message}"
    );
    assert!(message.contains("unknown field `labels`"), "{message}");
    assert!(message.contains("at line "), "{message}");
    // The rest of the defaults is read as before.
    let error = Central::parse(
        BUILTIN_REGISTER,
        &format!("{BUILTIN_DEFAULTS}\n[[exclude]]\npath = \"*.png\"\nreason = \"twice\"\n"),
    )
    .expect_err("a pattern twice");
    assert!(error.to_string().contains("stands twice"), "{error}");
    let error = Central::parse(
        BUILTIN_REGISTER,
        &format!("{BUILTIN_DEFAULTS}\n[[exclude]]\npath = \"*.tiff\"\nmeaning = \"none\"\n"),
    )
    .expect_err("a key the declaration does not know");
    assert!(matches!(error, CentralError::Defaults(_)), "{error}");
    // Read without the policy, the rest still names its line and column.
    assert!(
        error.to_string().contains("unknown field `meaning`"),
        "{error}"
    );
    assert!(error.to_string().contains("at line "), "{error}");
    let error = Central::parse(
        BUILTIN_REGISTER,
        &format!("edition = 2\n{BUILTIN_DEFAULTS}"),
    )
    .expect_err("a newer edition");
    assert!(error.to_string().contains("edition 2"), "{error}");
    let error = Central::parse(BUILTIN_REGISTER, "[repo\n").expect_err("no TOML");
    assert!(matches!(error, CentralError::Defaults(_)), "{error}");
}
