//! Phase 0 end to end: what a declaration means, and what it gets wrong.

// A test that cannot read its own fixtures has nothing to say, so it should
// fail loudly instead of carrying a Result through every call. The workspace
// denies these for production code, where an unproven invariant hides in them;
// here the reason travels with the call and the failure is the point.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use nebensound_declaration::finding::{Finding, Rule, Severity, UnitName};
use nebensound_declaration::names::ComponentName;
use nebensound_declaration::path::RelativePath;
use nebensound_declaration::resolve::{Origin, PipelineSource};
use nebensound_declaration::validate::taken_names;
use nebensound_declaration::{Analysis, Central, Declaration, Tree, analyse, resolve};

fn tree(paths: &[&str]) -> Tree {
    Tree::new(
        paths
            .iter()
            .map(|path| RelativePath::new(path).expect("a path in the fixture")),
    )
}

fn central() -> Central {
    Central::builtin().expect("what is compiled into this crate")
}

fn declaration(source: &str) -> Declaration {
    Declaration::parse(source).expect("a declaration in the fixture")
}

fn run(source: &str, paths: &[&str]) -> Analysis {
    analyse(&declaration(source), &central(), &tree(paths))
}

/// The first finding of a rule, whether it hangs at a path or not.
fn found(analysis: &Analysis, rule: Rule) -> Option<&Finding> {
    analysis
        .findings()
        .into_iter()
        .find(|finding| finding.rule == rule)
}

fn codes(analysis: &Analysis) -> Vec<&'static str> {
    let mut codes: Vec<_> = analysis
        .findings()
        .iter()
        .map(|finding| finding.rule.code())
        .collect();
    codes.sort_unstable();
    codes.dedup();
    codes
}

fn unit<'a>(analysis: &'a Analysis, name: &str) -> &'a nebensound_declaration::Unit {
    let wanted = UnitName::Component(ComponentName::new(name).expect("a name"));
    analysis
        .units
        .iter()
        .find(|unit| unit.name == wanted)
        .unwrap_or_else(|| panic!("expected a unit called {name}"))
}

const CRATE_FILES: &[&str] = &[
    "README.md",
    "AGENTS.md",
    "Cargo.toml",
    "Cargo.lock",
    "LICENSE",
    ".github/workflows/ci.yml",
    ".nebensound/repo.toml",
    "crates/thing/Cargo.toml",
    "crates/thing/src/lib.rs",
];

#[test]
fn a_pure_crate_resolves_and_has_nothing_wrong_with_it() {
    let analysis = run("[repo]\npipeline = \"rust-library\"\n", CRATE_FILES);
    assert!(
        analysis.findings().is_empty(),
        "expected nothing to report: {:?}",
        analysis.findings()
    );
    assert!(!analysis.failed());

    let repository = &analysis.units[0];
    assert_eq!(repository.name, UnitName::Repository);
    assert!(repository.path.is_root());
    assert_eq!(repository.origin, Origin::Declared);
    assert!(repository.carries_version);
    assert_eq!(
        repository.version_file().expect("the repository has one"),
        RelativePath::new(".nebensound/version").expect("a path")
    );
    assert_eq!(
        repository.pipeline.source,
        PipelineSource::Preset(
            nebensound_declaration::names::PresetName::new("rust-library").expect("a name")
        )
    );
    assert_eq!(repository.pipeline.targets().len(), 1);
}

#[test]
fn saying_nothing_runs_nothing_and_that_is_a_statement() {
    let analysis = run("[repo]\n", &["README.md"]);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(analysis.units[0].pipeline.source, PipelineSource::Default);
    assert!(analysis.units[0].pipeline.targets().is_empty());
}

#[test]
fn a_preset_the_register_does_not_have_is_named_with_the_ones_it_does() {
    let analysis = run("[repo]\npipeline = \"rust-libary\"\n", &["README.md"]);
    assert_eq!(codes(&analysis), ["unknown-preset"]);
    let finding = &analysis.findings()[0];
    assert_eq!(finding.severity, Severity::Error);
    assert!(finding.message.contains("rust-library"), "{finding}");
    assert!(analysis.failed());
}

#[test]
fn an_explicit_pipeline_is_checked_name_by_name() {
    let analysis = run(
        "[repo]\npipeline = { version = [\"cargo\", \"crago\"], check = [\"rust\", \"rustc\"], \
         build = \"cargo-pack\", publish = [\"crates.moon\"], \
         dependabot = [\"cargo\", \"crago\"] }\n",
        CRATE_FILES,
    );
    assert_eq!(codes(&analysis), ["unknown-register-entry"]);
    assert_eq!(
        analysis.findings().len(),
        5,
        "one finding each for version writer, check, build, target and ecosystem"
    );
    assert!(
        analysis
            .findings()
            .iter()
            .any(|finding| finding.message.contains("version writer crago")),
        "{:?}",
        analysis.findings()
    );
    assert_eq!(
        analysis.units[0]
            .pipeline
            .version
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["cargo"],
        "the known writer stays"
    );
    assert_eq!(
        analysis.units[0].pipeline.source,
        PipelineSource::WrittenOut
    );
}

#[test]
fn asking_for_a_default_that_is_not_offered_is_an_error() {
    let analysis = run(
        "[repo]\n\n[component.doc]\npath = \"doc\"\npipeline.repo = true\n",
        &["README.md", "doc/manual.typ"],
    );
    assert!(codes(&analysis).contains(&"no-requestable-default"));

    let circular = run(
        "[repo]\n\n[repo.component]\npipeline.repo = true\n\n\
         [component.doc]\npath = \"doc\"\npipeline.repo = true\n",
        &["README.md", "doc/manual.typ"],
    );
    assert!(codes(&circular).contains(&"no-requestable-default"));
}

#[test]
fn a_requested_default_is_taken_and_says_where_it_came_from() {
    let analysis = run(
        "[repo]\n\n[repo.component]\npipeline = \"typst-document\"\n\n\
         [component.doc]\npath = \"doc\"\npipeline.repo = true\n",
        &["README.md", "doc/manual.typ"],
    );
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    let doc = unit(&analysis, "doc");
    assert_eq!(doc.pipeline.source, PipelineSource::RequestedDefault);
    assert_eq!(doc.pipeline.targets().len(), 1);
}

#[test]
fn the_repository_cannot_declare_itself_out_of_a_version() {
    for value in ["none", "child-files", "child-folders"] {
        let analysis = run(
            &format!("[repo]\nversioning = \"{value}\"\n"),
            &["README.md", "doc.md"],
        );
        assert!(
            codes(&analysis).contains(&"repository-without-version"),
            "{value} should have been refused"
        );
        assert!(analysis.units[0].carries_version);
    }
}

#[test]
fn child_files_make_one_unit_per_document() {
    let analysis = run(
        "[repo]\n\n[repo.component]\npipeline = \"typst-document\"\n\n\
         [component.manuals]\npath = \"manuals\"\nversioning = \"child-files\"\n\
         pipeline.repo = true\n",
        &[
            "README.md",
            "manuals/setup.typ",
            "manuals/operation.typ",
            "manuals/images/plan.png",
        ],
    );
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());

    let folder = unit(&analysis, "manuals");
    assert!(!folder.carries_version, "the folder itself carries none");
    assert_eq!(folder.version_file(), None);

    let setup = unit(&analysis, "setup");
    assert!(setup.carries_version);
    assert_eq!(
        setup.origin,
        Origin::Generated {
            by: UnitName::Component(ComponentName::new("manuals").expect("a name"))
        }
    );
    // A generated unit has nowhere to say anything, so it inherits everything.
    assert_eq!(setup.pipeline.source, PipelineSource::Inherited);
    assert_eq!(setup.pipeline.targets(), folder.pipeline.targets());
    assert_eq!(
        setup.version_file().expect("a generated unit carries one"),
        RelativePath::new(".nebensound/versions/setup").expect("a path")
    );

    // The picture below a folder is not a direct child, so it makes no unit.
    assert_eq!(analysis.units.len(), 4);
}

#[test]
fn child_folders_make_one_unit_per_folder() {
    let analysis = run(
        "[repo]\n\n\
         [component.crates]\npath = \"crates\"\nversioning = \"self-and-child-folders\"\n\
         pipeline = { check = [\"rust\"] }\n",
        &[
            "README.md",
            "crates/one/Cargo.toml",
            "crates/one/src/lib.rs",
            "crates/two/Cargo.toml",
            "crates/two/src/lib.rs",
            "crates/loose.rs",
        ],
    );
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert!(unit(&analysis, "crates").carries_version);
    assert!(unit(&analysis, "one").carries_version);
    assert!(unit(&analysis, "two").carries_version);
    assert_eq!(analysis.units.len(), 4, "the loose file is not a folder");

    let resolution = resolve(
        &declaration(
            "[repo]\n\n[component.crates]\npath = \"crates\"\n\
             versioning = \"self-and-child-folders\"\npipeline = { check = [\"rust\"] }\n",
        ),
        central().register(),
        &tree(&[
            "README.md",
            "crates/one/Cargo.toml",
            "crates/one/src/lib.rs",
        ]),
    );
    assert_eq!(taken_names(&resolution).len(), 2);
}

#[test]
fn a_generated_name_that_cannot_be_a_tag_is_reported_rather_than_invented() {
    let analysis = run(
        "[repo]\n\n[component.doc]\npath = \"doc\"\nversioning = \"child-files\"\n",
        &["README.md", "doc/___.md"],
    );
    assert!(codes(&analysis).contains(&"ungeneratable-name"));
}

#[test]
fn two_generated_units_cannot_share_a_tag_prefix() {
    let analysis = run(
        "[repo]\n\n[component.doc]\npath = \"doc\"\nversioning = \"child-files\"\n",
        &["README.md", "doc/plan.md", "doc/plan.typ"],
    );
    let finding = found(&analysis, Rule::UngeneratableName).expect("the collision is reported");
    assert!(finding.message.contains("already taken"), "{finding}");
}

#[test]
fn declaring_children_that_do_not_exist_is_a_warning() {
    let analysis = run(
        "[repo]\n\n[component.doc]\npath = \"doc\"\nversioning = \"child-folders\"\n",
        &["README.md", "doc/plan.md"],
    );
    let finding =
        found(&analysis, Rule::ChildlessGenerator).expect("the empty promise is reported");
    assert_eq!(finding.severity, Severity::Warning);
    assert!(!analysis.failed(), "a warning does not stop the run");
}

#[test]
fn a_component_that_names_a_path_the_repository_lacks_is_an_error() {
    let analysis = run(
        "[repo]\n\n[component.ghost]\npath = \"nowhere\"\n",
        &["README.md"],
    );
    assert!(codes(&analysis).contains(&"missing-path"));
}

#[test]
fn every_file_needs_a_check_that_covers_it() {
    let uncovered = run("[repo]\n", CRATE_FILES);
    let finding = found(&uncovered, Rule::UncoveredFile).expect("the Rust file is uncovered");
    assert_eq!(
        finding.path,
        Some(RelativePath::new("crates/thing/src/lib.rs").expect("a path"))
    );

    // Declaring the check anywhere that contains the file is enough.
    let covered = run(
        "[repo]\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\npipeline = { check = [\"rust\"] }\n",
        CRATE_FILES,
    );
    assert!(covered.findings().is_empty(), "{:?}", covered.findings());
}

#[test]
fn a_language_the_register_knows_nothing_about_reports_rather_than_hides() {
    let analysis = run("[repo]\n", &["README.md", "tool.zig"]);
    assert_eq!(codes(&analysis), ["language-without-check"]);
}

#[test]
fn an_exclusion_excuses_a_path_and_has_to_be_used() {
    let excused = run(
        "[repo]\n\n[[exclude]]\npath = \"vendor\"\nreason = \"third-party code, taken as it is\"\n",
        &["README.md", "vendor/thing.rs"],
    );
    assert!(excused.findings().is_empty(), "{:?}", excused.findings());

    let pointless = run(
        "[repo]\n\n[[exclude]]\npath = \"vendor\"\nreason = \"third-party code\"\n",
        &["README.md"],
    );
    assert_eq!(codes(&pointless), ["pointless-exclusion"]);
    assert!(!pointless.failed());
}

#[test]
fn include_takes_an_extension_back_into_the_check() {
    let taken_back = run(
        "[[include]]\npath = \"*.png\"\n\n[repo]\n",
        &["README.md", "design/logo.png"],
    );
    assert_eq!(codes(&taken_back), ["language-without-check"]);

    let pointless = run("[[include]]\npath = \"*.rs\"\n\n[repo]\n", &["README.md"]);
    assert_eq!(codes(&pointless), ["pointless-inclusion"]);

    // Saying the same thing twice says the same thing. Judged one entry at a
    // time, each copy would look pointless because the other one still pulls
    // the file back — so every copy has to go before the question is asked.
    let twice = run(
        "[[include]]\npath = \"*.png\"\n\n[[include]]\npath = \"*.png\"\n\n[repo]\n",
        &["README.md", "design/logo.png"],
    );
    assert_eq!(codes(&twice), ["language-without-check"]);

    // And a pattern that pulls nothing back is reported once, not once per copy.
    let pointless_twice = run(
        "[[include]]\npath = \"*.rs\"\n\n[[include]]\npath = \"*.rs\"\n\n[repo]\n",
        &["README.md"],
    );
    assert_eq!(
        pointless_twice.findings().len(),
        1,
        "{:?}",
        pointless_twice.findings()
    );
}

#[test]
fn a_check_that_finds_nothing_is_an_error_someone_may_silence() {
    let unused = run(
        "[repo]\npipeline = \"rust-library\"\n",
        &["README.md", "Cargo.toml"],
    );
    assert!(codes(&unused).contains(&"unused-check"));

    let silenced = run(
        "[repo]\npipeline = \"rust-library\"\n\
         allow-unused = { rust = \"the crate arrives with the next pull request\" }\n",
        &["README.md", "Cargo.toml"],
    );
    assert!(!codes(&silenced).contains(&"unused-check"));

    let pointless = run(
        "[repo]\nallow-unused = { rust = \"nothing runs it here\" }\n",
        &["README.md"],
    );
    assert_eq!(codes(&pointless), ["pointless-suppression"]);
}

#[test]
fn publishing_needs_a_build_that_reaches_the_target() {
    let nothing_built = run(
        "[repo]\npipeline = { check = [\"rust\"], publish = [\"private-registry\"] }\n",
        CRATE_FILES,
    );
    assert!(codes(&nothing_built).contains(&"unreachable-target"));

    let wrong_kind = run(
        "[repo]\npipeline = { check = [\"rust\"], build = \"cargo-package\", publish = [\"ghcr\"] }\n",
        CRATE_FILES,
    );
    let finding = found(&wrong_kind, Rule::UnreachableTarget).expect("ghcr does not take a crate");
    assert!(finding.message.contains("crate"), "{finding}");
}

#[test]
fn the_one_target_that_cannot_be_taken_back_also_demands_the_language() {
    let analysis = run(
        "[repo]\npipeline = { check = [\"python\"], build = \"cargo-package\", \
         publish = [\"crates.io\"] }\n",
        &["README.md", "tool.py"],
    );
    assert!(codes(&analysis).contains(&"target-demands-check"));

    let allowed = run(
        "[repo]\npipeline = { check = [\"rust\"], build = \"cargo-package\", \
         publish = [\"crates.io\"], dependabot = [\"cargo\"] }\n",
        CRATE_FILES,
    );
    assert!(allowed.findings().is_empty(), "{:?}", allowed.findings());
}

#[test]
fn a_unit_without_a_version_has_no_identity_to_publish_under() {
    let analysis = run(
        "[repo]\n\n[component.thing]\npath = \"crates/thing\"\nversioning = \"none\"\n\
         pipeline = \"rust-library\"\n",
        CRATE_FILES,
    );
    assert!(codes(&analysis).contains(&"publish-without-version"));
    assert_eq!(unit(&analysis, "thing").version_file(), None);
}

#[test]
fn a_version_writer_needs_a_version_to_copy() {
    let without = run(
        "[repo]\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\nversioning = \"none\"\n\
         pipeline = { version = [\"cargo\"], check = [\"rust\"] }\n",
        CRATE_FILES,
    );
    assert_eq!(codes(&without), ["version-writer-without-version"]);
    assert!(
        without.findings()[0].message.contains("cargo"),
        "{}",
        without.findings()[0]
    );

    // The same unit with a version of its own is fine.
    let with = run(
        "[repo]\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\n\
         pipeline = { version = [\"cargo\"], check = [\"rust\"] }\n",
        CRATE_FILES,
    );
    assert!(with.findings().is_empty(), "{:?}", with.findings());

    // A unit that generates children hands the writer on to each of them, as
    // it does with publishing.
    let generating = run(
        "[repo]\n\n[component.sites]\npath = \"sites\"\nversioning = \"child-files\"\n\
         pipeline = \"asciidoc-site\"\n",
        &["README.md", "sites/setup.adoc", "sites/operation.adoc"],
    );
    assert!(
        !codes(&generating).contains(&"version-writer-without-version"),
        "{:?}",
        generating.findings()
    );
    assert!(!unit(&generating, "sites").carries_version);
    for child in ["setup", "operation"] {
        let child = unit(&generating, child);
        assert!(child.carries_version, "{}", child.name);
        assert_eq!(
            child
                .pipeline
                .version
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["asciidoc"],
            "{} inherits the writer",
            child.name
        );
    }
}

#[test]
fn a_finding_says_what_it_is_about() {
    let analysis = run(
        "[repo]\n\n[component.ghost]\npath = \"nowhere\"\n",
        &["README.md"],
    );
    let rendered = analysis.findings()[0].to_string();
    assert!(
        rendered.starts_with("error[missing-path] component ghost at nowhere:"),
        "{rendered}"
    );
    assert_eq!(Severity::Error.to_string(), "error");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert_eq!(UnitName::Repository.to_string(), "the repository");
    assert_eq!(Rule::MissingPath.to_string(), "missing-path");
}

#[test]
fn an_explicit_pipeline_may_leave_publish_out_altogether() {
    let analysis = run(
        "[repo]\npipeline = { check = [\"rust\"], build = \"cargo-package\", \
         dependabot = [\"cargo\"] }\n",
        CRATE_FILES,
    );
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert!(analysis.units[0].pipeline.targets().is_empty());
}

#[test]
fn every_rule_has_a_code_of_its_own() {
    let rules = [
        Rule::UnknownPreset,
        Rule::UnknownRegisterEntry,
        Rule::NoRequestableDefault,
        Rule::RepositoryWithoutVersion,
        Rule::MissingPath,
        Rule::LanguageWithoutCheck,
        Rule::ChildlessGenerator,
        Rule::UngeneratableName,
        Rule::UncoveredFile,
        Rule::UnusedCheck,
        Rule::PointlessSuppression,
        Rule::PointlessExclusion,
        Rule::PointlessInclusion,
        Rule::UnreachableTarget,
        Rule::TargetDemandsCheck,
        Rule::PublishWithoutVersion,
        Rule::VersionWriterWithoutVersion,
        Rule::UnwatchedManifest,
        Rule::PointlessWatch,
        Rule::UnexpectedTag,
        Rule::UnconventionalCommit,
        Rule::ShallowClone,
        Rule::MissingReleaseBranch,
        Rule::UnreleasedBeforeSwitch,
        Rule::ExhaustedVersion,
        Rule::PointlessReleaseBranch,
    ];
    let mut codes: Vec<_> = rules.iter().map(|rule| rule.code()).collect();
    codes.sort_unstable();
    let unique = codes.len();
    codes.dedup();
    assert_eq!(codes.len(), unique, "two rules share a code");
}

#[test]
fn a_finding_about_nothing_in_particular_still_reads() {
    let analysis = run("[[include]]\npath = \"*.rs\"\n\n[repo]\n", &["README.md"]);
    let finding = &analysis.findings()[0];
    assert_eq!(finding.unit, None);
    assert_eq!(finding.path, None);
    assert!(
        finding
            .to_string()
            .starts_with("warning[pointless-inclusion]: "),
        "{finding}"
    );

    let about_a_unit_only = run(
        "[repo]\nallow-unused = { rust = \"nothing runs it here\" }\n",
        &["README.md"],
    );
    let finding = &about_a_unit_only.findings()[0];
    assert!(finding.unit.is_some());
    assert_eq!(finding.path, None);
    assert!(finding.to_string().contains("the repository:"), "{finding}");
}

#[test]
fn validating_against_a_register_that_lost_a_target_does_not_panic() {
    // resolve() drops targets the register does not know, so validate() only
    // ever sees known ones when the two are handed the same register. It is
    // public and takes both, so it has to survive being handed a pair that
    // disagrees rather than assuming they match.
    let source = "[repo]\npipeline = { check = [\"rust\"], build = \"cargo-package\", \
                  publish = [\"private-registry\"] }\n";
    let files = tree(CRATE_FILES);
    let resolution = resolve(&declaration(source), central().register(), &files);

    let narrower = Central::parse(
        "[version]\n[check.rust]\nextensions = [\".rs\"]\n[build.cargo-package]\nproduces = \"crate\"\n\
         [target]\n[preset]\n[ecosystem]\n",
        "[repo]\npipeline = { check = [] }\n",
    )
    .expect("central data without that target");
    let findings =
        nebensound_declaration::validate(&declaration(source), &resolution, &narrower, &files);
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == Rule::UnreachableTarget),
        "an unknown target is resolve's business, not validate's"
    );
}

#[test]
fn a_public_library_is_one_word_apart_from_a_private_one() {
    let private = run("[repo]\npipeline = \"rust-library\"\n", CRATE_FILES);
    let public = run("[repo]\npipeline = \"rust-library-public\"\n", CRATE_FILES);

    assert!(private.findings().is_empty(), "{:?}", private.findings());
    assert!(public.findings().is_empty(), "{:?}", public.findings());

    let target = |analysis: &Analysis| analysis.units[0].pipeline.targets()[0].to_string();
    assert_eq!(target(&private), "private-registry");
    assert_eq!(target(&public), "crates.io");

    // The preset carries the rust check, which is what crates.io demands. A
    // pipeline written out by hand can get that wrong; the preset cannot.
    assert!(!public.failed());
}

#[test]
fn a_workspace_member_has_no_ecosystem_of_its_own() {
    // rust-library carries `dependabot = ["cargo"]`. That is right for a
    // repository that *is* a crate — not for a package inside a workspace:
    // there the lockfile lies at the root, and the member has nothing of its
    // own to resolve.
    let analysis = run(
        "[repo]\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\npipeline = \"rust-library\"\n",
        CRATE_FILES,
    );
    assert!(codes(&analysis).contains(&"pointless-watch"));
    assert!(!analysis.failed(), "a warning, not an error");
}

#[test]
fn a_manifest_is_watched_and_a_watched_ecosystem_has_a_manifest() {
    // The root manifest lies in no component: only the repository itself can
    // have it watched.
    let unwatched = run("[repo]\n", &["README.md", "Cargo.lock"]);
    let finding = found(&unwatched, Rule::UnwatchedManifest).expect("the manifest is reported");
    assert_eq!(finding.severity, Severity::Warning);
    assert!(!unwatched.failed(), "a warning stops nothing");

    let watched = run(
        "[repo]\npipeline = { dependabot = [\"cargo\"] }\n",
        &["README.md", "Cargo.lock"],
    );
    assert!(watched.findings().is_empty(), "{:?}", watched.findings());

    // And the other way round: watched, but no manifest there.
    let pointless = run(
        "[repo]\npipeline = { dependabot = [\"cargo\"] }\n",
        &["README.md"],
    );
    assert_eq!(codes(&pointless), ["pointless-watch"]);
    assert!(!pointless.failed());
}

#[test]
fn github_actions_is_watched_everywhere_without_writing_it_down() {
    // It stands in the base set, the way markdown does among the checks. A
    // repository that writes it down as well therefore gets no finding about a
    // missing manifest — there is none for github-actions.
    let analysis = run(
        "[repo]\npipeline = { dependabot = [\"github-actions\"] }\n",
        &["README.md", ".github/workflows/ci.yml"],
    );
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
}

#[test]
fn an_exclusion_that_matches_nothing_is_a_warning() {
    let analysis = run(
        "[repo]\n\n[[exclude]]\npath = \"vendor\"\nreason = \"third-party code\"\n",
        &["README.md"],
    );
    assert_eq!(codes(&analysis), ["pointless-exclusion"]);
    assert!(
        analysis.findings()[0].message.contains("third-party code"),
        "the reason stands in the message"
    );
}

#[test]
fn a_package_in_a_workspace_is_checked_by_the_workspace() {
    // The check sits with the workspace, build and publish with the package.
    // The crates.io target demands the rust check — and it is in force, even
    // though the component does not run it itself.
    // The sibling beside it does not count: it does not contain the package, so
    // its checks do not hold for it.
    let analysis = run(
        "[repo]\npipeline = { check = [\"rust\"], dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\n\
         pipeline = { build = \"cargo-package\", publish = [\"crates.io\"] }\n\n\
         [component.workflows]\npath = \".github\"\npipeline = { check = [\"yaml\"] }\n",
        CRATE_FILES,
    );
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());

    // Without the check at the workspace, the condition bites.
    let bare = run(
        "[repo]\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\n\
         pipeline = { build = \"cargo-package\", publish = [\"crates.io\"] }\n",
        CRATE_FILES,
    );
    assert!(codes(&bare).contains(&"target-demands-check"));
}

#[test]
fn every_kind_of_unit_has_a_tag_prefix_of_its_own() {
    let analysis = run(
        "[repo]\nversioning = \"self-and-child-folders\"\n\n[component.docs]\npath = \
         \"guides/docs\"\nversioning = \"child-files\"\n",
        &["guides/docs/intro.md", "guides/docs/setup.md", "tools/a.md"],
    );
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());

    let prefixes: Vec<(String, String)> = analysis
        .units
        .iter()
        .map(|unit| (unit.path.to_string(), unit.tag_prefix()))
        .collect();
    let expected = [
        (".", "v"),
        ("guides/docs", "docs-v"),
        ("guides", "guides-v"),
        ("tools", "tools-v"),
        ("guides/docs/intro.md", "docs-intro-v"),
        ("guides/docs/setup.md", "docs-setup-v"),
    ];
    for (path, prefix) in expected {
        assert!(
            prefixes.contains(&(path.to_owned(), prefix.to_owned())),
            "{path}: {prefixes:?}"
        );
    }
}

#[test]
fn a_generated_unit_cannot_take_another_units_tag_prefix() {
    // `docs-guide-v` would be the tags of both.
    let analysis = run(
        "[repo]\n\n[component.docs]\npath = \"docs\"\nversioning = \"child-files\"\n\n\
         [component.docs-guide]\npath = \"guide.md\"\n",
        &["docs/guide.md", "docs/intro.md", "guide.md"],
    );

    let refused = found(&analysis, Rule::UngeneratableName).expect("the prefix is taken");
    assert_eq!(refused.severity, Severity::Error);
    assert!(refused.message.contains("docs-guide-v"), "{refused}");
    assert_eq!(
        refused.path.as_ref().map(ToString::to_string).as_deref(),
        Some("docs/guide.md")
    );
    let names: Vec<String> = analysis
        .units
        .iter()
        .map(|unit| unit.name.to_string())
        .collect();
    assert!(names.contains(&"component intro".to_owned()), "{names:?}");
    assert!(!names.contains(&"component guide".to_owned()), "{names:?}");
}
