//! `init`: the first draft of a repository's switch to the pipeline, written
//! where it is missing, and the error `check` gives for a decision it left
//! open.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use nebensound_repo::audience::Visibility;
use nebensound_repo::history::Tag;
use nebensound_repo::init::{
    self, CALLER, DECLARATION, DEPENDABOT, MARKER, UNRESOLVED_PIN, caller, fill, highest, passing,
    release,
};
use nebensound_repo::register::BUILTIN_DEFAULTS;
use nebensound_repo::repository::Repository;
use nebensound_repo::{TagVersion, Version};
use support::{Git, call, central, directory, repository};

/// The files every repository holds, so that the report names none missing.
const MINIMUM: &[(&str, &str)] = &[
    ("README.md", "# tool\n\nA tool.\n"),
    ("LICENSE", "All rights reserved.\n"),
    ("AGENTS.md", "# Rules\n\nEverything is English.\n"),
    (".github/CODEOWNERS", "* @example\n"),
    (".gitignore", "target/\n"),
    (
        ".editorconfig",
        "root = true\n\n[*]\ninsert_final_newline = true\n",
    ),
];

/// A single crate with a binary, its lockfile and the minimum files.
const SINGLE_CRATE: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        "[package]\nname = \"tool\"\nedition = \"2024\"\n",
    ),
    ("Cargo.lock", "version = 4\n"),
    ("src/main.rs", "fn main() {}\n"),
];

/// What `init` writes as the declaration of [`SINGLE_CRATE`] with old tags up
/// to `v0.3.1` and `--visibility public`.
const SINGLE_CRATE_DECLARATION: &str = "\
#:schema https://raw.githubusercontent.com/Nebensound/pipeline/main/schemas/nebensound-declaration.json

# The declaration of this repository: what it consists of and what the
# pipeline does with it. Every field is described in
# https://github.com/Nebensound/pipeline/blob/main/docs/declaration.md

[repo]
audience = \"public\"
# Above v0.3.1, the highest release the old tags record.
minimum-version = \"0.3.2\"

# The preset rust-binary, written out with publish = false: nothing is published
# until a person decides it. To publish, name the targets under publish, or
# write pipeline = \"rust-binary\" in [repo] in place of this table.
[repo.pipeline]
version = [\"cargo\"]
check = [\"rust\"]
build = \"cargo-release\"
publish = false
dependabot = [\"cargo\"]
";

fn root(path: &Path) -> &str {
    path.to_str().expect("a path in UTF-8")
}

fn text(path: &Path, file: &str) -> String {
    fs::read_to_string(path.join(file)).unwrap_or_else(|error| panic!("{file}: {error}"))
}

/// `init` on the repository at `path`, with these arguments behind it.
fn init(path: &Path, args: &[&str]) -> (String, u8) {
    let mut all = vec!["nebensound-repo", "-C", root(path), "init"];
    all.extend(args);
    call(&all)
}

/// A repository with a history: these files in one commit, tagged with each
/// of `tags`.
fn history(files: &[&[(&str, &str)]], tags: &[&str]) -> Git {
    let git = Git::new();
    for each in files {
        git.write(each);
    }
    let first = git.commit("chore: start");
    for tag in tags {
        git.tag(tag, &first);
    }
    git
}

fn tag(name: &str) -> Tag {
    Tag {
        name: name.to_owned(),
        commit: Some("0".repeat(40)),
    }
}

#[test]
fn a_single_crate_gets_the_three_files_and_passes_phase_0_once_its_markers_are_resolved() {
    let git = history(
        &[MINIMUM, SINGLE_CRATE],
        &["v0.1.0", "v0.3.1", "other-v9.0.0"],
    );

    let (report, code) = init(git.path(), &["--visibility", "public"]);

    assert_eq!(code, 0, "{report}");
    assert_eq!(text(git.path(), DECLARATION), SINGLE_CRATE_DECLARATION);
    assert_eq!(text(git.path(), CALLER), caller(None));
    let dependabot = text(git.path(), DEPENDABOT);
    assert!(
        dependabot.contains("package-ecosystem: cargo"),
        "{dependabot}"
    );
    assert!(
        dependabot.contains("package-ecosystem: github-actions"),
        "{dependabot}"
    );
    for line in [
        "Written:\n  .nebensound/repo.toml\n  .github/workflows/nebensound.yml\n  \
         .github/dependabot.yml\n",
        "audience public: the repository is public on GitHub",
        "minimum-version 0.3.2: above v0.3.1",
        "preset rust-binary: Cargo.toml at the root holds a package with a binary",
        "publication: none. The preset rust-binary publishes to release-assets",
        "pin left open: this tool prints the version 0.0.0",
        ".github/workflows/nebensound.yml:21:77: pin the tag of a release of the pipeline",
        "git add .nebensound/repo.toml .github/workflows/nebensound.yml .github/dependabot.yml",
        "Run nebensound-repo check --visibility public and fix what it finds.",
        "docs/organization/README.md#the-day-a-repository-switches",
    ] {
        assert!(report.contains(line), "{line:?} in:\n{report}");
    }
    for absent in [
        "Left alone",
        "Minimum files",
        "Files no check covers",
        "Workflows",
    ] {
        assert!(!report.contains(absent), "{absent:?} in:\n{report}");
    }

    git.commit("ci: call the pipeline");
    let checked = [
        "nebensound-repo",
        "-C",
        git.root(),
        "check",
        "--name",
        "tool",
        "--default-branch",
        "main",
        "--visibility",
        "public",
    ];
    let (output, code) = call(&checked);
    assert_eq!(code, 1, "{output}");
    assert!(
        output.contains(
            "error[unresolved-init-marker] at .github/workflows/nebensound.yml:21:77: init left \
             a decision open here, and nobody has taken it yet: pin the tag of a release of the \
             pipeline, vX.Y.Z; this nebensound-repo belongs to no release."
        ),
        "{output}"
    );
    assert!(!output.contains("conflicting-tag"), "{output}");

    git.write(&[(CALLER, &caller(Some(Version::new(0, 0, 84))))]);
    git.commit("ci: pin the pipeline");
    let (output, code) = call(&checked);
    assert_eq!(code, 0, "{output}");
    assert!(output.contains("0 errors, 0 warnings"), "{output}");
}

#[test]
fn a_second_run_changes_nothing() {
    let git = history(&[MINIMUM, SINGLE_CRATE], &[]);
    let (_, code) = init(git.path(), &[]);
    assert_eq!(code, 0);
    let written: Vec<String> = [DECLARATION, CALLER, DEPENDABOT]
        .iter()
        .map(|file| text(git.path(), file))
        .collect();

    let (report, code) = init(git.path(), &["--visibility", "public"]);

    assert_eq!(code, 0, "{report}");
    assert!(!report.contains("Written"), "{report}");
    for file in [DECLARATION, CALLER, DEPENDABOT] {
        assert!(
            report.contains(&format!(
                "{file}: it exists, and init never overwrites a file"
            )),
            "{file} in:\n{report}"
        );
    }
    let again: Vec<String> = [DECLARATION, CALLER, DEPENDABOT]
        .iter()
        .map(|file| text(git.path(), file))
        .collect();
    assert_eq!(again, written);
    assert!(!report.contains("Recognised"), "{report}");
    assert!(report.contains("Run nebensound-repo check --visibility public"));
}

#[test]
fn a_workspace_with_a_separately_published_package_leaves_its_components_open() {
    let dir = repository(&[
        ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
        ("Cargo.lock", "version = 4\n"),
        ("crates/core/Cargo.toml", "[package]\nname = \"core\"\n"),
        ("crates/core/src/lib.rs", "\n"),
        ("crates/cli/Cargo.toml", "[package]\nname = \"cli\"\n"),
        ("crates/cli/src/main.rs", "fn main() {}\n"),
    ]);

    let (report, code) = init(dir.path(), &[]);

    assert_eq!(code, 0, "{report}");
    let declaration = text(dir.path(), DECLARATION);
    for line in [
        "# TODO(init): write audience = \"public\", \"internal\" or \"customer\".\n",
        "# The preset rust-workspace: Cargo.toml at the root holds no package of its own.\n\
         pipeline = \"rust-workspace\"\n",
        "\n# TODO(init): declare the Cargo packages below that are components.\n",
        "# - crates/cli\n# - crates/core\n",
    ] {
        assert!(declaration.contains(line), "{line:?} in:\n{declaration}");
    }
    assert!(!declaration.contains("minimum-version"), "{declaration}");
    assert!(!declaration.contains("[repo.pipeline]"), "{declaration}");
    for line in [
        "audience left open: the call names no --visibility, and init does not guess it",
        "preset rust-workspace: Cargo.toml at the root holds no package of its own",
        "components left open: the Cargo packages crates/cli, crates/core",
        ".nebensound/repo.toml:8:3: write audience = \"public\", \"internal\" or \"customer\".",
        ".nebensound/repo.toml:15:3: declare the Cargo packages below that are components.",
        "Take the decision each marker names",
        "Run nebensound-repo check and fix what it finds.",
        "Minimum files the repository does not hold:\n  README.md\n  LICENSE\n",
    ] {
        assert!(report.contains(line), "{line:?} in:\n{report}");
    }
    let dependabot = text(dir.path(), DEPENDABOT);
    assert!(
        dependabot.contains("package-ecosystem: cargo"),
        "{dependabot}"
    );
}

#[test]
fn a_service_gets_its_preset_written_out_and_its_dockerfile_watched() {
    let dir = repository(&[
        ("Cargo.toml", "[package]\nname = \"service\"\n"),
        ("Cargo.lock", "version = 4\n"),
        ("src/main.rs", "fn main() {}\n"),
        ("deploy/Dockerfile", "FROM scratch\n"),
    ]);

    let (report, code) = init(dir.path(), &["--visibility", "private"]);

    assert_eq!(code, 0, "{report}");
    let declaration = text(dir.path(), DECLARATION);
    for line in [
        "# TODO(init): write audience = \"internal\" or \"customer\".\n",
        "[repo.pipeline]\nversion = [\"cargo\"]\ncheck = [\"rust\", \"dockerfile\"]\nbuild = \
         \"docker\"\npublish = false\ndependabot = [\"cargo\", \"docker\"]\n",
    ] {
        assert!(declaration.contains(line), "{line:?} in:\n{declaration}");
    }
    assert!(
        report.contains(
            "audience left open: the repository is private on GitHub, which fits internal and \
             customer alike"
        ),
        "{report}"
    );
    assert!(
        report.contains("preset rust-service: Cargo.toml at the root holds a package, and"),
        "{report}"
    );
    assert!(report.contains("publishes to ghcr"), "{report}");
    assert!(report.contains("check --visibility private"), "{report}");
    let dependabot = text(dir.path(), DEPENDABOT);
    assert!(
        dependabot.contains("package-ecosystem: docker\n    directory: /deploy\n"),
        "{dependabot}"
    );
}

#[test]
fn a_python_service_gets_its_preset_without_a_version_writer() {
    let dir = repository(&[
        ("pyproject.toml", "[project]\nname = \"service\"\n"),
        ("uv.lock", "version = 1\n"),
        ("deploy/Dockerfile", "FROM scratch\n"),
    ]);

    let (report, code) = init(dir.path(), &["--visibility", "internal"]);

    assert_eq!(code, 0, "{report}");
    let declaration = text(dir.path(), DECLARATION);
    assert!(
        declaration.contains(
            "[repo.pipeline]\ncheck = [\"python\", \"dockerfile\"]\nbuild = \"docker\"\npublish = \
             false\ndependabot = [\"uv\", \"docker\"]\n"
        ),
        "{declaration}"
    );
    assert!(
        report.contains("preset python-service: uv.lock and deploy/Dockerfile stand at the root"),
        "{report}"
    );
    assert!(
        report.contains("the repository is internal on GitHub"),
        "{report}"
    );
}

#[test]
fn where_two_presets_fit_the_first_is_written_and_the_other_left_open() {
    let dir = repository(&[
        (
            "Cargo.toml",
            "[package]\nname = \"service\"\n\n[[bin]]\nname = \"run\"\npath = \"run.rs\"\n",
        ),
        ("run.rs", "fn main() {}\n"),
        ("uv.lock", "version = 1\n"),
        ("deploy/Dockerfile", "FROM scratch\n"),
    ]);

    let (report, code) = init(dir.path(), &["--visibility", "public"]);

    assert_eq!(code, 0, "{report}");
    let declaration = text(dir.path(), DECLARATION);
    assert!(
        declaration.contains(
            "# TODO(init): keep the pipeline below, or write the preset python-service \
             instead.\n# It fits as well: uv.lock and deploy/Dockerfile stand at the root.\n"
        ),
        "{declaration}"
    );
    assert!(declaration.contains("write pipeline = \"rust-service\""));
    assert!(
        report.contains("preset python-service left open: it fits as well, as uv.lock"),
        "{report}"
    );
}

#[test]
fn a_binary_is_told_by_its_main_file_its_bin_folder_or_its_bin_table() {
    for (files, preset) in [
        (
            &[
                ("Cargo.toml", "[package]\nname = \"x\"\n"),
                ("src/bin/x.rs", "fn main() {}\n"),
            ][..],
            "rust-binary",
        ),
        (
            &[(
                "Cargo.toml",
                "[package]\nname = \"x\"\n\n[[bin]]\nname = \"x\"\npath = \"x.rs\"\n",
            )][..],
            "rust-binary",
        ),
        (
            &[
                ("Cargo.toml", "[package]\nname = \"x\"\n"),
                ("src/lib.rs", "\n"),
            ][..],
            "rust-library",
        ),
    ] {
        let dir = repository(files);
        let (report, code) = init(dir.path(), &["--visibility", "public"]);
        assert_eq!(code, 0, "{report}");
        assert!(
            text(dir.path(), DECLARATION).contains(&format!("write pipeline = \"{preset}\"")),
            "{preset}: {report}"
        );
    }
}

#[test]
fn what_fits_no_preset_gets_the_base_set_and_its_uncovered_files_named() {
    let dir = repository(&[
        ("README.md", "# x\n"),
        ("logo.png", "\u{89}PNG"),
        ("a.py", "\n"),
        ("b.py", "\n"),
        ("c.py", "\n"),
        ("d/e.py", "\n"),
        ("notes.weird", "\n"),
        (".github/workflows/ci.yml", "on: push\n"),
        (".github/workflows/docs.md", "# x\n"),
        (".github/workflows/nested/x.yml", "on: push\n"),
    ]);

    let (report, code) = init(dir.path(), &["--visibility", "public"]);

    assert_eq!(code, 0, "{report}");
    assert!(
        text(dir.path(), DECLARATION).contains(
            "# No preset fits what init recognised, so only the base set runs: the checks\n\
             # markdown, yaml, json, toml.\npipeline = \"none\"\n"
        ),
        "{report}"
    );
    for line in [
        "preset none: no preset fits, so only the base set runs",
        "Files no check covers yet:\n  no check of the register, 1 file(s): notes.weird\n  the \
         check python, 4 file(s): a.py, b.py, c.py, …\n",
        "Workflows init did not touch:\n  .github/workflows/ci.yml\n\n",
        "Decide for each workflow init did not touch whether the pipeline replaces it",
    ] {
        assert!(report.contains(line), "{line:?} in:\n{report}");
    }
}

#[test]
fn a_manifest_that_does_not_read_gives_no_rust_preset() {
    let dir = repository(&[("Cargo.toml", "[package\n")]);

    let (report, code) = init(dir.path(), &["--visibility", "public"]);

    assert_eq!(code, 0, "{report}");
    assert!(
        report.contains("no Rust preset: Cargo.toml at the root does not read ("),
        "{report}"
    );
    assert!(report.contains("preset none"), "{report}");
}

#[test]
fn an_existing_declaration_is_left_alone_and_gives_the_dependabot_file() {
    let dir = repository(&[
        (
            DECLARATION,
            "[repo]\naudience = \"public\"\npipeline = \"rust-workspace\"\n",
        ),
        ("Cargo.toml", "[workspace]\n"),
        ("Cargo.lock", "version = 4\n"),
        (CALLER, "on: push\n"),
    ]);

    let (report, code) = init(dir.path(), &[]);

    assert_eq!(code, 0, "{report}");
    assert!(
        report.contains("Written:\n  .github/dependabot.yml\n"),
        "{report}"
    );
    assert!(
        report.contains(".nebensound/repo.toml: it exists, and init never overwrites a file"),
        "{report}"
    );
    assert!(
        report.contains(".github/workflows/nebensound.yml: it exists"),
        "{report}"
    );
    assert!(!report.contains("Workflows init did not touch"), "{report}");
    assert!(!report.contains("Recognised"), "{report}");
    assert!(
        text(dir.path(), DEPENDABOT).contains("package-ecosystem: cargo"),
        "{report}"
    );
    assert_eq!(text(dir.path(), CALLER), "on: push\n");
}

#[test]
fn a_declaration_that_does_not_read_gives_no_dependabot_file() {
    let dir = repository(&[(DECLARATION, "[repo\n")]);

    let (report, code) = init(dir.path(), &["--visibility", "public"]);

    assert_eq!(code, 0, "{report}");
    assert!(
        report.contains(
            ".github/dependabot.yml: no declaration that reads is in place (the declaration is \
             not readable:"
        ),
        "{report}"
    );
    assert!(!dir.path().join(DEPENDABOT).exists());
    assert!(dir.path().join(CALLER).exists());
}

#[test]
fn a_dependabot_file_under_its_other_name_is_left_alone() {
    let dir = repository(&[(".github/dependabot.yaml", "version: 2\n")]);

    let (report, code) = init(dir.path(), &["--visibility", "public"]);

    assert_eq!(code, 0, "{report}");
    assert!(
        report.contains(
            ".github/dependabot.yml: .github/dependabot.yaml exists, which Dependabot reads in its \
             place"
        ),
        "{report}"
    );
    assert!(!dir.path().join(DEPENDABOT).exists());
}

#[test]
fn a_place_that_is_no_folder_is_not_written_below() {
    let dir = repository(&[(".nebensound", "a file\n")]);
    std::os::unix::fs::symlink(dir.path().join("elsewhere"), dir.path().join(".github"))
        .expect("a link");

    let (report, code) = init(dir.path(), &["--visibility", "public"]);

    assert_eq!(code, 1, "{report}");
    assert!(
        report.contains("Not written:\n  .nebensound/repo.toml: "),
        "{report}"
    );
    assert!(
        report.contains("/.nebensound is no folder but a file or a link"),
        "{report}"
    );
    assert!(
        report.contains(".github/workflows/nebensound.yml: ") && report.contains("/.github is no"),
        "{report}"
    );
    assert!(!dir.path().join("elsewhere").exists());
}

#[test]
fn a_folder_that_takes_no_file_is_reported() {
    let dir = repository(&[("README.md", "# x\n")]);
    let nebensound = dir.path().join(".nebensound");
    let github = dir.path().join(".github");
    fs::create_dir(&nebensound).unwrap();
    fs::create_dir(&github).unwrap();
    for folder in [&nebensound, &github] {
        fs::set_permissions(folder, fs::Permissions::from_mode(0o555)).expect("read-only");
    }

    let (report, code) = init(dir.path(), &["--visibility", "public"]);

    for folder in [&nebensound, &github] {
        fs::set_permissions(folder, fs::Permissions::from_mode(0o755)).expect("writable again");
    }
    assert_eq!(code, 1, "{report}");
    assert!(report.contains("  .nebensound/repo.toml: "), "{report}");
    assert!(
        report.contains("  .github/workflows/nebensound.yml: "),
        "{report}"
    );
    assert!(report.contains("  .github/dependabot.yml: "), "{report}");
    assert!(!report.contains("Written"), "{report}");
}

#[test]
fn a_declaration_that_could_not_be_written_gives_no_dependabot_file() {
    let dir = repository(&[(".nebensound", "a file\n")]);

    let (report, code) = init(dir.path(), &["--visibility", "public"]);

    assert_eq!(code, 1, "{report}");
    assert!(
        report.contains("Not written:\n  .nebensound/repo.toml: "),
        "{report}"
    );
    assert!(
        report.contains(
            ".github/dependabot.yml: no declaration that reads is in place (init could not write \
             its draft)"
        ),
        "{report}"
    );
    assert!(!dir.path().join(DEPENDABOT).exists());
    assert!(dir.path().join(CALLER).exists());
}

/// A file that takes no byte and fails, as one on a full disk does.
struct Full;

impl std::io::Write for Full {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("no space left"))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_file_whose_write_fails_is_removed_again() {
    let dir = directory(&[("repo.toml", "[re")]);
    let target = dir.path().join("repo.toml");

    let why = fill(&target, &mut Full, "[repo]\n").unwrap_err();

    assert_eq!(why, "no space left; init removed what it had written of it");
    assert!(!target.exists());
}

#[test]
fn a_file_that_cannot_be_removed_after_a_failed_write_is_named() {
    let dir = directory(&[(DECLARATION, "[re")]);
    let folder = dir.path().join(".nebensound");
    let target = dir.path().join(DECLARATION);
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).expect("read-only");

    let why = fill(&target, &mut Full, "[repo]\n").unwrap_err();

    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).expect("writable again");
    assert!(
        why.starts_with("no space left; what was written of it stays at "),
        "{why}"
    );
    assert!(why.contains("since removing it failed: "), "{why}");
    assert!(target.exists());
}

#[test]
fn a_folder_that_is_no_repository_is_reported() {
    let dir = directory(&[("README.md", "# x\n")]);

    let (report, code) = init(dir.path(), &[]);

    assert_eq!(code, 1, "{report}");
    assert!(report.contains("git does not list"), "{report}");
    assert!(!dir.path().join(DECLARATION).exists());
}

#[test]
fn broken_central_data_is_reported() {
    let dir = repository(&[("README.md", "# x\n")]);
    let mut out = Vec::new();

    let code = nebensound_repo::run_against(
        ["nebensound-repo", "-C", root(dir.path()), "init"],
        "not a register",
        BUILTIN_DEFAULTS,
        nebensound_repo::output::Output::Elsewhere,
        &mut out,
    );

    assert_eq!(code, 1);
    let report = String::from_utf8(out).unwrap();
    assert!(report.contains("the built-in register"), "{report}");
}

/// A git that refuses every call naming `word` and passes every other one on.
fn refusing(dir: &Path, word: &str) -> std::path::PathBuf {
    let path = dir.join(format!("git-refusing-{}", word.len()));
    support::program::executable(
        &path,
        format!(
            "#!/bin/sh\nfor argument in \"$@\"; do\n  if [ \"$argument\" = '{word}' ]; then\n    \
             echo 'refused {word}' >&2\n    exit 3\n  fi\ndone\nexec git \"$@\"\n"
        ),
    );
    path
}

#[test]
fn a_git_that_does_not_name_the_tags_is_reported() {
    let git = history(&[MINIMUM], &["v1.0.0"]);
    let tools = tempfile::tempdir().expect("a directory");

    for word in ["rev-parse", "--merged"] {
        let error = init::run(
            &Repository::at(git.path()).with_git(refusing(tools.path(), word)),
            &central(),
            None,
            "0.0.0",
        )
        .expect_err(word);
        assert!(
            error.to_string().contains(&format!("refused {word}")),
            "{error}"
        );
    }
    assert!(!git.path().join(DECLARATION).exists());
}

#[test]
fn a_release_binary_pins_its_own_tag() {
    let dir = repository(&[("README.md", "# x\n")]);

    let outcome = init::run(
        &Repository::at(dir.path()),
        &central(),
        Some(Visibility::Public),
        "0.0.84",
    )
    .expect("the repository reads");

    assert!(!outcome.failed);
    assert!(
        outcome
            .report
            .contains("pin v0.0.84: the release of the pipeline this tool belongs to"),
        "{}",
        outcome.report
    );
    let written = text(dir.path(), CALLER);
    assert!(
        written
            .contains("    uses: Nebensound/pipeline/.github/workflows/nebensound.yml@v0.0.84\n"),
        "{written}"
    );
    assert!(!written.contains(MARKER), "{written}");
    assert!(
        !outcome.report.contains("Decisions left open"),
        "{}",
        outcome.report
    );
}

#[test]
fn only_a_plain_version_other_than_0_0_0_is_a_release() {
    assert_eq!(release("0.0.84"), Some(Version::new(0, 0, 84)));
    assert_eq!(release("1.2.3"), Some(Version::new(1, 2, 3)));
    for none in ["0.0.0", "0.0.84-pr.12", "0.0.84-pre.1", "v0.0.84", ""] {
        assert_eq!(release(none), None, "{none}");
    }
}

#[test]
fn the_caller_carries_the_tag_or_the_marker_on_the_line_of_its_reference() {
    let pinned = caller(Some(Version::new(0, 0, 84)));
    let unresolved = caller(None);

    assert!(pinned.contains("nebensound.yml@v0.0.84\n"), "{pinned}");
    assert!(
        unresolved.contains(&format!("nebensound.yml@{UNRESOLVED_PIN}\n")),
        "{unresolved}"
    );
    assert!(UNRESOLVED_PIN.starts_with("TODO..init # TODO(init): "));
    for written in [&pinned, &unresolved] {
        assert!(written.starts_with("name: nebensound\n"), "{written}");
        assert!(!written.contains("{pin}"), "{written}");
    }
}

#[test]
fn the_floor_passes_the_highest_release_of_the_prefix() {
    let tags = [
        tag("v0.9.0"),
        tag("v1.4.3"),
        tag("v1.5.0-pre.2"),
        tag("v2.0"),
        tag("release-3.0.0"),
        tag("tool-v9.0.0"),
    ];

    let (found, name) = highest("v", &tags).expect("a release");
    assert_eq!(found, TagVersion::pre(Version::new(1, 5, 0), 2));
    assert_eq!(name, "v1.5.0-pre.2");
    assert_eq!(passing(found), Some(Version::new(1, 5, 0)));

    let (found, name) = highest("v", &tags[..2]).expect("a release");
    assert_eq!(name, "v1.4.3");
    assert_eq!(passing(found), Some(Version::new(1, 4, 4)));

    assert_eq!(highest("v", &tags[3..5]), None);
    assert_eq!(highest("v", &[]), None);
    assert_eq!(
        passing(TagVersion::stable(Version::new(1, 2, u64::MAX))),
        None
    );
}

#[test]
fn a_tag_no_version_follows_is_left_open() {
    let git = history(&[MINIMUM], &["v1.2.18446744073709551615"]);

    let (report, code) = init(git.path(), &["--visibility", "public"]);

    assert_eq!(code, 0, "{report}");
    assert!(
        text(git.path(), DECLARATION).contains(
            "# TODO(init): delete the tag v1.2.18446744073709551615 if nobody published it.\n"
        ),
        "{report}"
    );
    assert!(
        report.contains("minimum-version left open: no version passes the tag"),
        "{report}"
    );
}

#[test]
fn check_reports_every_marker_left_in_the_files_init_writes_and_nowhere_else() {
    let dir = repository(&[
        (
            DECLARATION,
            "[repo]\n# TODO(init): write audience = \"public\".\npipeline = \"none\"\n",
        ),
        (CALLER, "on: push\n# TODO(init)\n"),
        (
            DEPENDABOT,
            "version: 2 # TODO(init): one\n# TODO(init): two\n",
        ),
        ("README.md", "# x\n\nTODO(init) stays here.\n"),
        (".github/workflows/gate.yml", "# TODO(init): elsewhere\n"),
    ]);

    let (output, code) = call(&["nebensound-repo", "-C", root(dir.path()), "check"]);

    assert_eq!(code, 1, "{output}");
    let findings: Vec<&str> = output
        .lines()
        .filter(|line| line.starts_with("  error[unresolved-init-marker]"))
        .collect();
    assert_eq!(
        findings,
        [
            "  error[unresolved-init-marker] at .github/dependabot.yml:1:14: init left a \
             decision open here, and nobody has taken it yet: one. Take it, write it in place of \
             the marker and remove the marker",
            "  error[unresolved-init-marker] at .github/dependabot.yml:2:3: init left a decision \
             open here, and nobody has taken it yet: two. Take it, write it in place of the \
             marker and remove the marker",
            "  error[unresolved-init-marker] at .github/workflows/nebensound.yml:2:3: init left \
             a decision open here, and nobody has taken it yet. Take it, write it in place of \
             the marker and remove the marker",
            "  error[unresolved-init-marker] at .nebensound/repo.toml:2:3: init left a decision \
             open here, and nobody has taken it yet: write audience = \"public\". Take it, write \
             it in place of the marker and remove the marker",
        ],
        "{output}"
    );
}

#[test]
fn a_file_init_writes_that_cannot_be_read_gives_no_marker_finding() {
    let dir = repository(&[
        (DECLARATION, support::QUIET),
        ("target.yml", "# TODO(init)\n"),
    ]);
    fs::create_dir_all(dir.path().join(".github/workflows")).unwrap();
    std::os::unix::fs::symlink("../../target.yml", dir.path().join(CALLER)).expect("a link");
    support::git(dir.path(), &["add", "--all"]);

    let (output, _) = call(&["nebensound-repo", "-C", root(dir.path()), "check"]);

    assert!(!output.contains("unresolved-init-marker"), "{output}");
}
