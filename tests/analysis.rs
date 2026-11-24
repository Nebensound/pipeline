//! Phase 0 end to end: what a declaration means, and what it gets wrong.

// A test that cannot read its own fixtures has nothing to say, so it should
// fail loudly instead of carrying a Result through every call. The workspace
// denies these for production code, where an unproven invariant hides in them;
// here the reason travels with the call and the failure is the point.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use nebensound_repo::finding::{Finding, Position, Rule, Severity, UnitName};
use nebensound_repo::names::ComponentName;
use nebensound_repo::path::RelativePath;
use nebensound_repo::resolve::{Origin, PipelineSource};
use nebensound_repo::validate::taken_names;
use nebensound_repo::{Analysis, Central, Declaration, Tree, analyse, resolve};

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

fn unit<'a>(analysis: &'a Analysis, name: &str) -> &'a nebensound_repo::Unit {
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
    let analysis = run(
        "[repo]\naudience = \"public\"\npipeline = \"rust-library\"\n",
        CRATE_FILES,
    );
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
        repository.pipeline.source,
        PipelineSource::Preset(
            nebensound_repo::names::PresetName::new("rust-library").expect("a name")
        )
    );
    assert_eq!(repository.pipeline.targets().len(), 1);
}

#[test]
fn saying_nothing_runs_nothing_and_that_is_a_statement() {
    let analysis = run("[repo]\naudience = \"public\"\n", &["README.md"]);
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
    assert_eq!(analysis.units[0].pipeline.source, PipelineSource::Default);
    assert!(analysis.units[0].pipeline.targets().is_empty());
}

#[test]
fn a_preset_the_register_does_not_have_is_named_with_the_ones_it_does() {
    let analysis = run(
        "[repo]\naudience = \"public\"\npipeline = \"rust-libary\"\n",
        &["README.md"],
    );
    assert_eq!(codes(&analysis), ["unknown-preset"]);
    let finding = &analysis.findings()[0];
    assert_eq!(finding.severity, Severity::Error);
    assert!(finding.message.contains("rust-library"), "{finding}");
    assert!(analysis.failed());
}

#[test]
fn an_explicit_pipeline_is_checked_name_by_name() {
    let analysis = run(
        "[repo]\naudience = \"public\"\npipeline = { version = [\"cargo\", \"crago\"], check = [\"rust\", \"rustc\"], \
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
        "[repo]\naudience = \"public\"\n\n[component.doc]\npath = \"doc\"\npipeline.repo = true\n",
        &["README.md", "doc/manual.typ"],
    );
    assert!(codes(&analysis).contains(&"no-requestable-default"));

    let circular = run(
        "[repo]\naudience = \"public\"\n\n[repo.component]\npipeline.repo = true\n\n\
         [component.doc]\npath = \"doc\"\npipeline.repo = true\n",
        &["README.md", "doc/manual.typ"],
    );
    assert!(codes(&circular).contains(&"no-requestable-default"));
}

#[test]
fn a_requested_default_is_taken_and_says_where_it_came_from() {
    let analysis = run(
        "[repo]\naudience = \"public\"\n\n[repo.component]\npipeline = \"typst-document\"\n\n\
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
            &format!("[repo]\naudience = \"public\"\nversioning = \"{value}\"\n"),
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
        "[repo]\naudience = \"public\"\n\n[repo.component]\npipeline = \"typst-document\"\n\n\
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

    // The picture below a folder is not a direct child, so it makes no unit.
    assert_eq!(analysis.units.len(), 4);
}

#[test]
fn child_folders_make_one_unit_per_folder() {
    let analysis = run(
        "[repo]\naudience = \"public\"\n\n\
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
            "[repo]\naudience = \"public\"\n\n[component.crates]\npath = \"crates\"\n\
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
        "[repo]\naudience = \"public\"\n\n[component.doc]\npath = \"doc\"\nversioning = \"child-files\"\n",
        &["README.md", "doc/___.md"],
    );
    assert!(codes(&analysis).contains(&"ungeneratable-name"));
}

#[test]
fn two_generated_units_cannot_share_a_tag_prefix() {
    let analysis = run(
        "[repo]\naudience = \"public\"\n\n[component.doc]\npath = \"doc\"\nversioning = \"child-files\"\n",
        &["README.md", "doc/plan.md", "doc/plan.typ"],
    );
    let finding = found(&analysis, Rule::UngeneratableName).expect("the collision is reported");
    assert!(finding.message.contains("already taken"), "{finding}");
}

#[test]
fn declaring_children_that_do_not_exist_is_a_warning() {
    let analysis = run(
        "[repo]\naudience = \"public\"\n\n[component.doc]\npath = \"doc\"\nversioning = \"child-folders\"\n",
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
        "[repo]\naudience = \"public\"\n\n[component.ghost]\npath = \"nowhere\"\n",
        &["README.md"],
    );
    assert!(codes(&analysis).contains(&"missing-path"));
}

#[test]
fn every_file_needs_a_check_that_covers_it() {
    let uncovered = run("[repo]\naudience = \"public\"\n", CRATE_FILES);
    let finding = found(&uncovered, Rule::UncoveredFile).expect("the Rust file is uncovered");
    assert_eq!(
        finding.path,
        Some(RelativePath::new("crates/thing/src/lib.rs").expect("a path"))
    );

    // Declaring the check anywhere that contains the file is enough.
    let covered = run(
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\npipeline = { check = [\"rust\"] }\n",
        CRATE_FILES,
    );
    assert!(covered.findings().is_empty(), "{:?}", covered.findings());
}

#[test]
fn a_language_the_register_knows_nothing_about_reports_rather_than_hides() {
    let analysis = run(
        "[repo]\naudience = \"public\"\n",
        &["README.md", "tool.zig"],
    );
    assert_eq!(codes(&analysis), ["language-without-check"]);
}

#[test]
fn a_pipfile_is_toml_and_needs_no_exclusion() {
    let analysis = run(
        "[repo]\naudience = \"public\"\n",
        &["README.md", "Pipfile", "api/Pipfile"],
    );
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
}

/// The Python project files that stay a gap on purpose: dependency lists that a
/// uv project keeps in `pyproject.toml` and `uv.lock`, and settings files whose
/// content belongs in `pyproject.toml`.
const PYTHON_GAPS: &[&str] = &[
    "requirements.txt",
    "requirements-dev.txt",
    "requirements/base.txt",
    "constraints.txt",
    ".coveragerc",
    "pytest.ini",
    "tox.ini",
    "setup.cfg",
];

/// The codes phase 0 reports for one of [`PYTHON_GAPS`], with the gap itself
/// or without it. The manifest of the ecosystem `pip` also warns while nothing
/// watches it, excluded or not; that belongs to the manifests of `pip`, not to
/// the gap.
fn expected(name: &str, gap: bool) -> Vec<&'static str> {
    let mut codes = Vec::new();
    if gap {
        codes.push("language-without-check");
    }
    if name == "requirements.txt" {
        codes.push("unwatched-manifest");
    }
    codes
}

#[test]
fn each_python_file_that_stays_a_gap_is_reported_where_it_lies() {
    for name in PYTHON_GAPS {
        let analysis = run("[repo]\naudience = \"public\"\n", &["README.md", name]);
        assert_eq!(codes(&analysis), expected(name, true), "{name}");
        let finding = found(&analysis, Rule::LanguageWithoutCheck).expect("the gap");
        assert_eq!(
            finding.path,
            Some(RelativePath::new(name).expect("a path")),
            "{name}"
        );
        assert!(analysis.failed(), "{name}");
    }
}

#[test]
fn a_python_file_that_stays_a_gap_is_excluded_by_its_repository() {
    for name in PYTHON_GAPS {
        let analysis = run(
            &format!(
                "[repo]\naudience = \"public\"\n\n[[exclude]]\npath = \"{name}\"\n\
                 reason = \"kept by this repository for a reason of its own\"\n"
            ),
            &["README.md", name],
        );
        assert_eq!(codes(&analysis), expected(name, false), "{name}");
        assert!(!analysis.failed(), "{name}");
    }
}

#[test]
fn an_empty_marker_keeps_a_folder_and_one_with_content_is_a_gap() {
    let paths = ["README.md", "assets/.gitkeep"];
    let empty = tree(&paths).with_empty([RelativePath::new("assets/.gitkeep").expect("a path")]);

    let kept = analyse(
        &declaration("[repo]\naudience = \"public\"\n"),
        &central(),
        &empty,
    );
    assert!(kept.findings().is_empty(), "{:?}", kept.findings());

    let filled = run("[repo]\naudience = \"public\"\n", &paths);
    assert_eq!(codes(&filled), ["language-without-check"]);
}

#[test]
fn an_exclusion_excuses_a_path_and_has_to_be_used() {
    let excused = run(
        "[repo]\naudience = \"public\"\n\n[[exclude]]\npath = \"vendor\"\nreason = \"third-party code, taken as it is\"\n",
        &["README.md", "vendor/thing.rs"],
    );
    assert!(excused.findings().is_empty(), "{:?}", excused.findings());

    let pointless = run(
        "[repo]\naudience = \"public\"\n\n[[exclude]]\npath = \"vendor\"\nreason = \"third-party code\"\n",
        &["README.md"],
    );
    assert_eq!(codes(&pointless), ["pointless-exclusion"]);
    assert!(!pointless.failed());
}

#[test]
fn include_takes_an_extension_back_into_the_check() {
    let taken_back = run(
        "[[include]]\npath = \"*.png\"\n\n[repo]\naudience = \"public\"\n",
        &["README.md", "design/logo.png"],
    );
    assert_eq!(codes(&taken_back), ["language-without-check"]);

    let pointless = run(
        "[[include]]\npath = \"*.rs\"\n\n[repo]\naudience = \"public\"\n",
        &["README.md"],
    );
    assert_eq!(codes(&pointless), ["pointless-inclusion"]);

    // Saying the same thing twice says the same thing. Judged one entry at a
    // time, each copy would look pointless because the other one still pulls
    // the file back — so every copy has to go before the question is asked.
    let twice = run(
        "[[include]]\npath = \"*.png\"\n\n[[include]]\npath = \"*.png\"\n\n[repo]\naudience = \"public\"\n",
        &["README.md", "design/logo.png"],
    );
    assert_eq!(codes(&twice), ["language-without-check"]);

    // And a pattern that pulls nothing back is reported once, not once per copy.
    let pointless_twice = run(
        "[[include]]\npath = \"*.rs\"\n\n[[include]]\npath = \"*.rs\"\n\n[repo]\naudience = \"public\"\n",
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
        "[repo]\naudience = \"public\"\npipeline = \"rust-library\"\n",
        &["README.md", "Cargo.toml"],
    );
    assert!(codes(&unused).contains(&"unused-check"));

    let silenced = run(
        "[repo]\naudience = \"public\"\npipeline = \"rust-library\"\n\
         allow-unused = { rust = \"the crate arrives with the next pull request\" }\n",
        &["README.md", "Cargo.toml"],
    );
    assert!(!codes(&silenced).contains(&"unused-check"));

    let pointless = run(
        "[repo]\naudience = \"public\"\nallow-unused = { rust = \"nothing runs it here\" }\n",
        &["README.md"],
    );
    assert_eq!(codes(&pointless), ["pointless-suppression"]);
}

#[test]
fn publishing_needs_a_build_that_reaches_the_target() {
    let nothing_built = run(
        "[repo]\naudience = \"public\"\npipeline = { check = [\"rust\"], publish = [\"private-registry\"] }\n",
        CRATE_FILES,
    );
    assert!(codes(&nothing_built).contains(&"unreachable-target"));

    let wrong_kind = run(
        "[repo]\naudience = \"public\"\npipeline = { check = [\"rust\"], build = \"cargo-package\", publish = [\"ghcr\"] }\n",
        CRATE_FILES,
    );
    let finding = found(&wrong_kind, Rule::UnreachableTarget).expect("ghcr does not take a crate");
    assert!(finding.message.contains("crate"), "{finding}");
}

#[test]
fn the_one_target_that_cannot_be_taken_back_also_demands_the_language() {
    let analysis = run(
        "[repo]\naudience = \"public\"\npipeline = { check = [\"python\"], build = \"cargo-package\", \
         publish = [\"crates.io\"] }\n",
        &["README.md", "tool.py"],
    );
    assert!(codes(&analysis).contains(&"target-demands-check"));

    let allowed = run(
        "[repo]\naudience = \"public\"\npipeline = { version = [\"cargo\"], check = [\"rust\"], \
         build = \"cargo-package\", publish = [\"crates.io\"], dependabot = [\"cargo\"] }\n",
        CRATE_FILES,
    );
    assert!(allowed.findings().is_empty(), "{:?}", allowed.findings());
}

#[test]
fn a_unit_without_a_version_has_no_identity_to_publish_under() {
    let analysis = run(
        "[repo]\naudience = \"public\"\n\n[component.thing]\npath = \"crates/thing\"\nversioning = \"none\"\n\
         pipeline = \"rust-library\"\n",
        CRATE_FILES,
    );
    assert!(codes(&analysis).contains(&"publish-without-version"));
    assert!(!unit(&analysis, "thing").carries_version);
}

#[test]
fn a_version_writer_needs_a_version_to_copy() {
    let without = run(
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n\n\
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
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\n\
         pipeline = { version = [\"cargo\"], check = [\"rust\"] }\n",
        CRATE_FILES,
    );
    assert!(with.findings().is_empty(), "{:?}", with.findings());

    // A unit that generates children hands the writer on to each of them, as
    // it does with publishing.
    let generating = run(
        "[repo]\naudience = \"public\"\n\n[component.sites]\npath = \"sites\"\nversioning = \"child-files\"\n\
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
fn a_build_needs_the_version_writers_the_register_names_for_it() {
    // cargo-package needs the writer cargo: without it, the crate would carry
    // the version its Cargo.toml holds, not the one the run gives the unit.
    let without = run(
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\n\
         pipeline = { check = [\"rust\"], build = \"cargo-package\" }\n",
        CRATE_FILES,
    );
    assert_eq!(codes(&without), ["build-without-version-writer"]);
    let finding = found(&without, Rule::BuildWithoutVersionWriter).expect("the finding");
    assert_eq!(finding.severity, Severity::Error);
    assert!(without.failed());
    assert_eq!(
        finding.unit,
        Some(UnitName::Component(
            ComponentName::new("thing").expect("a name")
        ))
    );
    assert!(finding.message.contains("cargo-package"), "{finding}");
    assert!(
        finding.message.contains("version writer cargo"),
        "{finding}"
    );

    // Another writer does not stand in for the one the build needs.
    let other = run(
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\n\
         pipeline = { version = [\"asciidoc\"], check = [\"rust\"], \
         build = \"cargo-package\" }\n",
        CRATE_FILES,
    );
    assert_eq!(codes(&other), ["build-without-version-writer"]);

    // With the writer, nothing is wrong.
    let with = run(
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\n\
         pipeline = { version = [\"cargo\"], check = [\"rust\"], \
         build = \"cargo-package\" }\n",
        CRATE_FILES,
    );
    assert!(with.findings().is_empty(), "{:?}", with.findings());

    // A build that needs no writer needs none.
    let docker = run(
        "[repo]\naudience = \"public\"\npipeline = { check = [\"rust\"], build = \"docker\", \
         dependabot = [\"cargo\"] }\n",
        CRATE_FILES,
    );
    assert!(
        !codes(&docker).contains(&"build-without-version-writer"),
        "{:?}",
        docker.findings()
    );

    // A build the register does not know is reported as that, and only that.
    let unknown = run(
        "[repo]\naudience = \"public\"\npipeline = { check = [\"rust\"], build = \"nothing-like-it\", \
         dependabot = [\"cargo\"] }\n",
        CRATE_FILES,
    );
    assert!(
        !codes(&unknown).contains(&"build-without-version-writer"),
        "{:?}",
        unknown.findings()
    );
}

#[test]
fn a_generator_without_the_writer_its_build_needs_is_reported_once() {
    // The children inherit the pipeline, and with it the mistake; it is the
    // generator's declaration that is wrong, so it is reported there alone.
    let analysis = run(
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.crates]\npath = \"crates\"\nversioning = \"child-folders\"\n\
         pipeline = { check = [\"rust\"], build = \"cargo-package\" }\n",
        CRATE_FILES,
    );
    let reported: Vec<_> = analysis
        .findings()
        .into_iter()
        .filter(|finding| finding.rule == Rule::BuildWithoutVersionWriter)
        .map(|finding| finding.unit.clone())
        .collect();
    assert_eq!(
        reported,
        [Some(UnitName::Component(
            ComponentName::new("crates").expect("a name")
        ))]
    );
    assert!(
        analysis
            .units
            .iter()
            .any(|unit| unit.name.to_string() == "component thing"),
        "the child exists and inherits the build"
    );
}

#[test]
fn a_unit_that_is_never_built_needs_no_writer_for_its_build() {
    // Without a version of its own and without children, the unit gets no
    // build job, so nothing it builds could carry a wrong version; and the
    // writer would be refused there as having nothing to copy.
    let analysis = run(
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\nversioning = \"none\"\n\
         pipeline = { check = [\"rust\"], build = \"cargo-package\" }\n",
        CRATE_FILES,
    );
    assert!(!unit(&analysis, "thing").carries_version);
    assert!(
        !codes(&analysis).contains(&"build-without-version-writer"),
        "{:?}",
        analysis.findings()
    );
}

#[test]
fn every_preset_that_builds_passes_with_the_writers_it_selects() {
    let central = central();
    let mut seen = 0;
    for (name, preset) in &central.register().preset {
        if preset.build.is_none() {
            continue;
        }
        let analysis = run(
            &format!("[repo]\naudience = \"public\"\npipeline = \"{name}\"\n"),
            CRATE_FILES,
        );
        assert!(
            !codes(&analysis).contains(&"build-without-version-writer"),
            "{name}: {:?}",
            analysis.findings()
        );
        seen += 1;
    }
    assert!(seen > 0, "some preset builds");
}

/// The units a finding of `rule` is about, each with the path it names.
fn reported(analysis: &Analysis, rule: Rule) -> Vec<(String, String)> {
    analysis
        .findings()
        .into_iter()
        .filter(|finding| finding.rule == rule)
        .map(|finding| {
            (
                finding
                    .unit
                    .as_ref()
                    .map_or_else(String::new, ToString::to_string),
                finding
                    .path
                    .as_ref()
                    .map_or_else(String::new, ToString::to_string),
            )
        })
        .collect()
}

const DOCKER: &str = "[repo]\naudience = \"public\"\npipeline = { check = [\"dockerfile\"], build = \"docker\", \
                      dependabot = [\"docker\"] }\n";

#[test]
fn a_container_build_needs_its_dockerfile() {
    // The build docker reads deploy/Dockerfile below the unit and nothing
    // else; without it the build job would fail before it builds.
    let without = run(DOCKER, &["README.md", "Dockerfile"]);
    assert_eq!(codes(&without), ["build-without-file"]);
    assert!(without.failed());
    let finding = found(&without, Rule::BuildWithoutFile).expect("the finding");
    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(finding.unit, Some(UnitName::Repository));
    assert_eq!(
        finding.path,
        Some(RelativePath::new("deploy/Dockerfile").expect("a path"))
    );
    assert!(finding.message.contains("builds docker"), "{finding}");
    assert!(finding.message.contains("deploy/Dockerfile"), "{finding}");

    // A folder of that name is no Dockerfile.
    let folder = run(DOCKER, &["README.md", "deploy/Dockerfile/Dockerfile"]);
    assert_eq!(
        reported(&folder, Rule::BuildWithoutFile),
        [("the repository".to_owned(), "deploy/Dockerfile".to_owned())]
    );

    // With the file in place, nothing is wrong.
    let with = run(DOCKER, &["README.md", "deploy/Dockerfile"]);
    assert!(with.findings().is_empty(), "{:?}", with.findings());

    // A build that reads no file needs none.
    let crate_only = run(
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\n\
         pipeline = { version = [\"cargo\"], check = [\"rust\"], \
         build = \"cargo-package\" }\n",
        CRATE_FILES,
    );
    assert!(
        !codes(&crate_only).contains(&"build-without-file"),
        "{:?}",
        crate_only.findings()
    );

    // A build the register does not know is reported as that, and only that.
    let unknown = run(
        "[repo]\naudience = \"public\"\npipeline = { check = [\"dockerfile\"], build = \"nothing-like-it\", \
         dependabot = [\"docker\"] }\n",
        &["README.md", "Dockerfile"],
    );
    assert!(
        codes(&unknown).contains(&"unknown-register-entry"),
        "{:?}",
        unknown.findings()
    );
    assert!(
        !codes(&unknown).contains(&"build-without-file"),
        "{:?}",
        unknown.findings()
    );
}

#[test]
fn each_service_needs_the_dockerfile_below_its_own_path() {
    let analysis = run(
        "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n\
         [component.api]\npath = \"api\"\n\
         pipeline = { check = [\"dockerfile\"], build = \"docker\", dependabot = [\"docker\"] }\n\n\
         [component.web]\npath = \"web\"\n\
         pipeline = { check = [\"dockerfile\"], build = \"docker\", dependabot = [\"docker\"] }\n",
        &["README.md", "api/deploy/Dockerfile", "web/Dockerfile"],
    );
    // The one of api does not stand in for web's.
    assert_eq!(
        reported(&analysis, Rule::BuildWithoutFile),
        [(
            "component web".to_owned(),
            "web/deploy/Dockerfile".to_owned()
        )]
    );
}

#[test]
fn every_generated_unit_needs_its_own_dockerfile() {
    // The children are what the plan builds, each from the files below its own
    // path; the generator leaves the building to them and needs none.
    let analysis = run(
        "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n\
         [component.services]\npath = \"services\"\nversioning = \"child-folders\"\n\
         pipeline = { check = [\"dockerfile\"], build = \"docker\", dependabot = [\"docker\"] }\n",
        &[
            "README.md",
            "services/api/deploy/Dockerfile",
            "services/web/Dockerfile",
        ],
    );
    assert_eq!(
        reported(&analysis, Rule::BuildWithoutFile),
        [(
            "component web".to_owned(),
            "services/web/deploy/Dockerfile".to_owned()
        )]
    );
}

#[test]
fn a_unit_that_is_never_built_needs_no_file_for_its_build() {
    let analysis = run(
        "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n\
         [component.api]\npath = \"api\"\nversioning = \"none\"\n\
         pipeline = { check = [\"dockerfile\"], build = \"docker\", dependabot = [\"docker\"] }\n",
        &["README.md", "api/Dockerfile"],
    );
    assert!(!unit(&analysis, "api").carries_version);
    assert!(
        !codes(&analysis).contains(&"build-without-file"),
        "{:?}",
        analysis.findings()
    );
}

#[test]
fn every_preset_that_builds_a_container_asks_for_its_dockerfile() {
    let central = central();
    let docker = nebensound_repo::names::BuildName::new("docker").expect("a name");
    let mut seen = 0;
    for (name, preset) in &central.register().preset {
        if preset.build.as_ref() != Some(&docker) {
            continue;
        }
        let declaration = format!("[repo]\naudience = \"public\"\npipeline = \"{name}\"\n");
        let without = run(&declaration, &["README.md"]);
        assert_eq!(
            reported(&without, Rule::BuildWithoutFile),
            [("the repository".to_owned(), "deploy/Dockerfile".to_owned())],
            "{name}"
        );
        let with = run(&declaration, &["README.md", "deploy/Dockerfile"]);
        assert!(
            !codes(&with).contains(&"build-without-file"),
            "{name}: {:?}",
            with.findings()
        );
        seen += 1;
    }
    assert!(seen > 0, "some preset builds a container");
}

#[test]
fn a_finding_says_what_it_is_about() {
    let analysis = run(
        "[repo]\naudience = \"public\"\n\n[component.ghost]\npath = \"nowhere\"\n",
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
        "[repo]\naudience = \"public\"\npipeline = { version = [\"cargo\"], check = [\"rust\"], \
         build = \"cargo-package\", dependabot = [\"cargo\"] }\n",
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
        Rule::ControlCharacter,
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
        Rule::ReleaseFieldWithoutVersion,
        Rule::BuildWithoutVersionWriter,
        Rule::BuildWithoutFile,
        Rule::UnwatchedManifest,
        Rule::PointlessWatch,
        Rule::MissingLockfile,
        Rule::UnconventionalCommit,
        Rule::ShallowClone,
        Rule::ExhaustedVersion,
        Rule::ConflictingTag,
        Rule::UnexcusedIgnore,
        Rule::UnreadableIgnoreFile,
        Rule::UnreadConfiguration,
        Rule::PartialPackageExclusion,
    ];
    let mut codes: Vec<_> = rules.iter().map(|rule| rule.code()).collect();
    codes.sort_unstable();
    let unique = codes.len();
    codes.dedup();
    assert_eq!(codes.len(), unique, "two rules share a code");
}

#[test]
fn a_finding_that_points_at_a_place_names_its_line_and_column() {
    let finding = Finding::error(Rule::PartialPackageExclusion, "only part of a package")
        .at(RelativePath::new(".nebensound/repo.toml").unwrap())
        .on(Position {
            line: 12,
            column: 10,
        });
    assert_eq!(
        finding.to_string(),
        "error[partial-package-exclusion] at .nebensound/repo.toml:12:10: only part of a package"
    );

    // A place without a file to be in names nothing.
    let nowhere = Finding::warning(Rule::PointlessWatch, "x").on(Position { line: 1, column: 1 });
    assert_eq!(nowhere.to_string(), "warning[pointless-watch]: x");
}

#[test]
fn a_finding_about_nothing_in_particular_still_reads() {
    let analysis = run(
        "[[include]]\npath = \"*.rs\"\n\n[repo]\naudience = \"public\"\n",
        &["README.md"],
    );
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
        "[repo]\naudience = \"public\"\nallow-unused = { rust = \"nothing runs it here\" }\n",
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
    let source = "[repo]\naudience = \"public\"\npipeline = { check = [\"rust\"], build = \"cargo-package\", \
                  publish = [\"private-registry\"] }\n";
    let files = tree(CRATE_FILES);
    let resolution = resolve(&declaration(source), central().register(), &files);

    let narrower = Central::parse(
        "[version]\n[check.rust]\nextensions = [\".rs\"]\n[build.cargo-package]\nproduces = \"crate\"\n\
         [target]\n[preset]\n[ecosystem]\n",
        "[repo]\naudience = \"public\"\npipeline = { check = [] }\n\n\
         [dependabot]\ninterval = \"weekly\"\nopen-pull-requests-limit = 5\n\
         commit-prefix = \"deps\"\n[dependabot.group]\nname = \"all\"\n\
         update-types = [\"minor\"]\n",
    )
    .expect("central data without that target");
    let findings = nebensound_repo::validate(&declaration(source), &resolution, &narrower, &files);
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == Rule::UnreachableTarget),
        "an unknown target is resolve's business, not validate's"
    );
}

#[test]
fn a_public_library_is_one_word_apart_from_a_private_one() {
    let private = run(
        "[repo]\naudience = \"public\"\npipeline = \"rust-library\"\n",
        CRATE_FILES,
    );
    let public = run(
        "[repo]\naudience = \"public\"\npipeline = \"rust-library-public\"\n",
        CRATE_FILES,
    );

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
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n\n\
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
    let unwatched = run(
        "[repo]\naudience = \"public\"\n",
        &["README.md", "Cargo.lock"],
    );
    let finding = found(&unwatched, Rule::UnwatchedManifest).expect("the manifest is reported");
    assert_eq!(finding.severity, Severity::Warning);
    assert!(!unwatched.failed(), "a warning stops nothing");

    let watched = run(
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n",
        &["README.md", "Cargo.lock"],
    );
    assert!(watched.findings().is_empty(), "{:?}", watched.findings());

    // And the other way round: watched, but no manifest there.
    let pointless = run(
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n",
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
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"github-actions\"] }\n",
        &["README.md", ".github/workflows/ci.yml"],
    );
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());
}

#[test]
fn an_exclusion_that_matches_nothing_is_a_warning() {
    let analysis = run(
        "[repo]\naudience = \"public\"\n\n[[exclude]]\npath = \"vendor\"\nreason = \"third-party code\"\n",
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
        "[repo]\naudience = \"public\"\npipeline = { check = [\"rust\"], dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\n\
         pipeline = { version = [\"cargo\"], build = \"cargo-package\", \
         publish = [\"crates.io\"] }\n\n\
         [component.workflows]\npath = \".github\"\npipeline = { check = [\"yaml\"] }\n",
        CRATE_FILES,
    );
    assert!(analysis.findings().is_empty(), "{:?}", analysis.findings());

    // Without the check at the workspace, the condition bites.
    let bare = run(
        "[repo]\naudience = \"public\"\npipeline = { dependabot = [\"cargo\"] }\n\n\
         [component.thing]\npath = \"crates/thing\"\n\
         pipeline = { build = \"cargo-package\", publish = [\"crates.io\"] }\n",
        CRATE_FILES,
    );
    assert!(codes(&bare).contains(&"target-demands-check"));
}

#[test]
fn every_kind_of_unit_has_a_tag_prefix_of_its_own() {
    let analysis = run(
        "[repo]\naudience = \"public\"\nversioning = \"self-and-child-folders\"\n\n[component.docs]\npath = \
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
        "[repo]\naudience = \"public\"\n\n[component.docs]\npath = \"docs\"\nversioning = \"child-files\"\n\n\
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
