//! Configuration files of programs that no check starts any more are named.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use nebensound_repo::finding::{Rule, Severity};
use nebensound_repo::unread;
use support::{QUIET, analysis, call, repository};

#[test]
fn every_configuration_of_yamllint_at_the_root_is_named_as_unread() {
    let analysis = analysis(
        QUIET,
        &[
            ".nebensound/repo.toml",
            ".yamllint",
            ".yamllint.yaml",
            ".yamllint.yml",
            "sub/.yamllint",
            "yamllint.yaml",
            "config.yaml",
        ],
    );

    let found = unread::findings(&analysis);

    let named: Vec<String> = found
        .iter()
        .map(|finding| finding.path.as_ref().unwrap().to_string())
        .collect();
    assert_eq!(named, [".yamllint", ".yamllint.yaml", ".yamllint.yml"]);
    for finding in &found {
        assert_eq!(finding.rule, Rule::UnreadConfiguration);
        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(
            finding.message,
            "the YAML check no longer starts yamllint, so nothing the pipeline runs reads this \
             file: its rules and what it leaves out change nothing. Remove it, or keep it for a \
             yamllint run of your own"
        );
    }
}

#[test]
fn every_configuration_of_html_validate_is_named_as_unread_where_the_check_found_it() {
    let analysis = analysis(
        QUIET,
        &[
            ".nebensound/repo.toml",
            ".htmlvalidate.json",
            ".htmlvalidateignore",
            "html-validate.config.mjs",
            "html-validate.config.cjs",
            "html-validate.config.js",
            "html-validate.config.mts",
            "html-validate.config.cts",
            "html-validate.config.ts",
            "index.html",
            "site/.htmlvalidate.mjs",
            "site/.htmlvalidate.cjs",
            "site/.htmlvalidate.js",
            "site/.htmlvalidateignore",
            "site/html-validate.config.mjs",
            "site/htmlvalidate.json",
        ],
    );

    let found = unread::findings(&analysis);

    // The ignore files and the configuration in every folder, as
    // html-validate read them for each page; the flat configuration at the
    // root alone, where the check found it.
    let named: Vec<String> = found
        .iter()
        .map(|finding| finding.path.as_ref().unwrap().to_string())
        .collect();
    assert_eq!(
        named,
        [
            ".htmlvalidate.json",
            ".htmlvalidateignore",
            "html-validate.config.cjs",
            "html-validate.config.cts",
            "html-validate.config.js",
            "html-validate.config.mjs",
            "html-validate.config.mts",
            "html-validate.config.ts",
            "site/.htmlvalidate.cjs",
            "site/.htmlvalidate.js",
            "site/.htmlvalidate.mjs",
            "site/.htmlvalidateignore",
        ]
    );
    for finding in &found {
        assert_eq!(finding.rule, Rule::UnreadConfiguration);
        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(
            finding.message,
            "the HTML check no longer starts html-validate, so nothing the pipeline runs reads \
             this file: its rules and what it leaves out change nothing. Remove it, or keep it \
             for a html-validate run of your own"
        );
    }
}

#[test]
fn phase_0_warns_and_passes() {
    let dir = repository(&[
        (".nebensound/repo.toml", QUIET),
        (".yamllint.yaml", "ignore: config.yaml\nrules: {}\n"),
    ]);
    let root = dir.path().to_str().unwrap();

    let (text, code) = call(&["nebensound-repo", "-C", root, "check"]);

    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("warning[unread-configuration] at .yamllint.yaml: the YAML check"),
        "{text}"
    );
    // What it would leave out is no longer held against the declaration.
    assert!(!text.contains("unexcused-ignore"), "{text}");
}

#[test]
fn a_repository_without_one_hears_nothing() {
    let analysis = analysis(QUIET, &[".nebensound/repo.toml", "a.yml"]);

    assert!(unread::findings(&analysis).is_empty());
    assert_eq!(Rule::UnreadConfiguration.code(), "unread-configuration");
}
