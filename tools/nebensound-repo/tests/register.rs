//! The central register, and the promise that its entries point at something.

// A test that cannot read its own fixtures has nothing to say, so it should
// fail loudly instead of carrying a Result through every call. The workspace
// denies these for production code, where an unproven invariant hides in them;
// here the reason travels with the call and the failure is the point.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use nebensound_repo::Declaration;
use nebensound_repo::model::Publish;
use nebensound_repo::names::{
    BuildName, CheckName, EcosystemName, PresetName, TargetName, VersionWriterName,
};
use nebensound_repo::path::RelativePath;
use nebensound_repo::register::{BUILTIN_REGISTER, Register, RegisterError, Stands};
use nebensound_repo::{Central, Ruling, Tree};

fn refuses(source: &str) -> String {
    match Register::parse(source) {
        Ok(_) => panic!("expected this register to be refused, and it was accepted"),
        Err(error) => error.to_string(),
    }
}

fn builtin() -> Register {
    Register::builtin().expect("the register this crate was built with")
}

#[test]
fn the_register_this_crate_ships_is_sound() {
    let register = builtin();
    let central = Central::builtin().expect("what is compiled in");
    assert_eq!(
        central.base_checks(),
        ["markdown", "yaml", "json", "toml"].map(|name| CheckName::new(name).expect("a name"))
    );
    for name in [
        "none",
        "rust-workspace",
        "rust-library-package",
        "rust-library-public-package",
        "rust-library",
        "rust-library-public",
        "rust-binary",
        "rust-service",
        "python-service",
        "platformio-firmware",
        "esphome-firmware",
        "asciidoc-site",
        "typst-document",
    ] {
        let preset = PresetName::new(name).expect("a name");
        assert!(
            register.preset.contains_key(&preset),
            "the preset table lists the {name} preset"
        );
    }
}

/// One row of the preset table: version, check, build, publish,
/// dependabot. `publish` is empty for `false`.
type Row = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    Option<&'static str>,
    &'static [&'static str],
    &'static [&'static str],
);

/// The preset table, row for row: what each preset promises.
const PRESET_TABLE: &[Row] = &[
    ("none", &[], &[], None, &[], &[]),
    ("rust-workspace", &[], &["rust"], None, &[], &["cargo"]),
    (
        "rust-library-package",
        &["cargo"],
        &[],
        Some("cargo-package"),
        &["private-registry"],
        &[],
    ),
    (
        "rust-library-public-package",
        &["cargo"],
        &[],
        Some("cargo-package"),
        &["crates.io"],
        &[],
    ),
    (
        "rust-library",
        &["cargo"],
        &["rust"],
        Some("cargo-package"),
        &["private-registry"],
        &["cargo"],
    ),
    (
        "rust-library-public",
        &["cargo"],
        &["rust"],
        Some("cargo-package"),
        &["crates.io"],
        &["cargo"],
    ),
    (
        "rust-binary",
        &["cargo"],
        &["rust"],
        Some("cargo-release"),
        &["release-assets"],
        &["cargo"],
    ),
    (
        "rust-service",
        &["cargo"],
        &["rust", "dockerfile"],
        Some("docker"),
        &["ghcr"],
        &["cargo", "docker"],
    ),
    (
        "python-service",
        &[],
        &["python", "dockerfile"],
        Some("docker"),
        &["ghcr"],
        &["uv", "docker"],
    ),
    (
        "platformio-firmware",
        &[],
        &["cpp"],
        Some("platformio"),
        &["release-assets"],
        &[],
    ),
    (
        "esphome-firmware",
        &[],
        &["yaml"],
        Some("esphome"),
        &["release-assets"],
        &[],
    ),
    (
        "asciidoc-site",
        &["asciidoc"],
        &["asciidoc"],
        Some("asciidoctor"),
        &["pages"],
        &[],
    ),
    (
        "typst-document",
        &[],
        &["typst"],
        Some("typst"),
        &["release-assets"],
        &[],
    ),
];

fn shown<T: ToString>(list: &[T]) -> Vec<String> {
    list.iter().map(ToString::to_string).collect()
}

#[test]
fn every_preset_reads_as_its_row_in_the_preset_table() {
    let register = builtin();

    for &(name, version, check, build, publish, dependabot) in PRESET_TABLE {
        let preset = register
            .preset
            .get(&PresetName::new(name).expect("a name"))
            .unwrap_or_else(|| panic!("the preset table lists the {name} preset"));

        assert_eq!(shown(&preset.version), shown(version), "{name}: version");
        assert_eq!(shown(&preset.check), shown(check), "{name}: check");
        assert_eq!(
            preset.build.as_ref().map(ToString::to_string).as_deref(),
            build,
            "{name}: build"
        );
        let targets = match &preset.publish {
            Publish::Nothing => Vec::new(),
            Publish::To(targets) => shown(targets),
        };
        assert_eq!(targets, shown(publish), "{name}: publish");
        assert_eq!(
            shown(&preset.dependabot),
            shown(dependabot),
            "{name}: dependabot"
        );
    }

    // And nothing beyond the table: a preset the table does not name is one
    // nobody can look up there.
    let listed = PRESET_TABLE.iter().map(|row| row.0).collect::<Vec<_>>();
    for name in register.preset.keys() {
        assert!(
            listed.contains(&name.as_str()),
            "{name} is not in the preset table"
        );
    }
}

#[test]
fn every_version_writer_on_offer_is_here_and_no_other() {
    let register = builtin();
    let offered = register
        .version
        .keys()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(offered, ["asciidoc", "cargo"]);
}

#[test]
fn every_check_on_offer_is_here() {
    let register = builtin();
    for name in [
        "markdown",
        "yaml",
        "json",
        "toml",
        "rust",
        "python",
        "cpp",
        "javascript",
        "php",
        "r",
        "shell",
        "sql",
        "qml",
        "html",
        "css",
        "asciidoc",
        "typst",
        "dockerfile",
    ] {
        let check = CheckName::new(name).expect("a name");
        assert!(
            register.check.contains_key(&check),
            "the register offers the {name} check"
        );
    }
}

/// One row of the coverage table: check, extensions, whole file names.
type Coverage = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
);

/// The coverage table, check for check. A file type that drops out of a check
/// drops out silently: nothing else fails, the file is simply nobody's any
/// more. So every extension and every file name stands here a second time, and
/// the register has to agree with it entry for entry.
const COVERAGE_TABLE: &[Coverage] = &[
    ("asciidoc", &[".adoc", ".asciidoc"], &[]),
    (
        "cpp",
        &[".c", ".h", ".cpp", ".hpp", ".cc", ".hh", ".cxx", ".ino"],
        &[],
    ),
    ("css", &[".css", ".scss"], &[]),
    (
        "dockerfile",
        &[".dockerfile"],
        &["Dockerfile", "Containerfile"],
    ),
    ("html", &[".html", ".htm"], &[]),
    (
        "javascript",
        &[".js", ".mjs", ".cjs", ".jsx", ".ts", ".tsx"],
        &[],
    ),
    (
        "json",
        &[".json", ".jsonc", ".geojson"],
        &["composer.lock", "flake.lock", "Pipfile.lock"],
    ),
    ("markdown", &[".md", ".markdown"], &[]),
    ("php", &[".php"], &[]),
    ("python", &[".py", ".pyi"], &[]),
    ("qml", &[".qml"], &[]),
    ("r", &[".r"], &[]),
    ("rust", &[".rs"], &[]),
    ("shell", &[".sh", ".bash"], &[]),
    ("sql", &[".sql"], &[]),
    (
        "toml",
        &[".toml"],
        &[
            "Cargo.lock",
            "uv.lock",
            "poetry.lock",
            "pdm.lock",
            "Pipfile",
        ],
    ),
    ("typst", &[".typ"], &[]),
    ("yaml", &[".yml", ".yaml"], &[]),
];

/// The manifest table, ecosystem for ecosystem: the file names each one is
/// recognized by in the tree.
const MANIFEST_TABLE: &[(&str, &[&str])] = &[
    ("cargo", &["Cargo.lock"]),
    ("docker", &["Dockerfile", "Containerfile"]),
    ("github-actions", &[]),
    ("npm", &["package-lock.json"]),
    ("pip", &["requirements.txt"]),
    ("uv", &["uv.lock"]),
];

/// A list compared as a set that still counts: sorted, duplicates kept. The
/// order in which the register writes a list says nothing; a second copy of
/// an entry does.
fn counted<T: ToString>(list: &[T]) -> Vec<String> {
    let mut shown = shown(list);
    shown.sort();
    shown
}

#[test]
fn every_check_covers_exactly_its_row_in_the_coverage_table() {
    let register = builtin();

    // Every check has a row and every row a check. A check without a row
    // covers what nobody pinned; a row without a check pins nothing.
    assert_eq!(
        counted(&register.check.keys().collect::<Vec<_>>()),
        counted(&COVERAGE_TABLE.iter().map(|row| row.0).collect::<Vec<_>>()),
        "the checks on offer"
    );

    for &(name, extensions, filenames) in COVERAGE_TABLE {
        let check = &register.check[&CheckName::new(name).expect("a name")];
        assert_eq!(
            counted(&check.extensions),
            counted(extensions),
            "{name}: extensions"
        );
        assert_eq!(
            counted(&check.filenames),
            counted(filenames),
            "{name}: file names"
        );
    }
}

#[test]
fn every_ecosystem_is_recognized_by_exactly_its_row_in_the_manifest_table() {
    let register = builtin();

    assert_eq!(
        counted(&register.ecosystem.keys().collect::<Vec<_>>()),
        counted(&MANIFEST_TABLE.iter().map(|row| row.0).collect::<Vec<_>>()),
        "the ecosystems on offer"
    );

    for &(name, manifests) in MANIFEST_TABLE {
        let ecosystem = &register.ecosystem[&EcosystemName::new(name).expect("a name")];
        assert_eq!(
            counted(&ecosystem.manifests),
            counted(manifests),
            "{name}: manifests"
        );
    }
}

#[test]
fn a_check_is_found_by_extension_or_by_whole_name() {
    let register = builtin();
    let named = |name: &str| CheckName::new(name).expect("a name");

    assert_eq!(
        register.check_for("crates/x/src/lib.rs"),
        Some(&named("rust"))
    );
    assert_eq!(register.check_for("README.md"), Some(&named("markdown")));
    assert_eq!(register.check_for("Cargo.lock"), Some(&named("toml")));
    assert_eq!(register.check_for("api/uv.lock"), Some(&named("toml")));
    assert_eq!(register.check_for("composer.lock"), Some(&named("json")));
    assert_eq!(register.check_for("sub/flake.lock"), Some(&named("json")));
    assert_eq!(register.check_for("poetry.lock"), Some(&named("toml")));
    assert_eq!(register.check_for("api/pdm.lock"), Some(&named("toml")));
    assert_eq!(register.check_for("Pipfile.lock"), Some(&named("json")));
    // TOML without an extension, named whole like a lockfile.
    assert_eq!(register.check_for("Pipfile"), Some(&named("toml")));
    assert_eq!(register.check_for("api/Pipfile"), Some(&named("toml")));
    // A lockfile is assigned by its name, never by the extension `.lock`.
    assert_eq!(register.check_for("yarn.lock"), None);
    assert_eq!(register.check_for("Gemfile.lock"), None);
    assert_eq!(register.check_for("other.lock"), None);
    assert_eq!(
        register.check_for("deploy/Dockerfile"),
        Some(&named("dockerfile"))
    );
    assert_eq!(register.check_for("Scripts/Start.R"), Some(&named("r")));
    assert_eq!(register.check_for("notes.unknown"), None);
    assert_eq!(register.check_for("LICENSE"), None);
}

#[test]
fn a_lockfile_is_a_whole_name_a_check_covers_that_ends_in_lock() {
    let register = builtin();

    assert!(register.is_lockfile("Cargo.lock"));
    assert!(register.is_lockfile("api/uv.lock"));
    assert!(register.is_lockfile("sub/flake.lock"));
    assert!(register.is_lockfile("poetry.lock"));
    assert!(register.is_lockfile("api/pdm.lock"));
    assert!(register.is_lockfile("Pipfile.lock"));
    // Named by a check, but not a lockfile.
    assert!(!register.is_lockfile("deploy/Dockerfile"));
    assert!(!register.is_lockfile("api/Pipfile"));
    // A lockfile, but no check covers it.
    assert!(!register.is_lockfile("yarn.lock"));
    assert!(!register.is_lockfile("other.lock"));
    assert!(!register.is_lockfile("Cargo.toml"));
}

#[test]
fn the_lockfile_of_a_manifest_is_one_a_check_names() {
    let register = builtin();
    let cargo = &register.ecosystem[&EcosystemName::new("cargo").expect("a name")];
    let lockfile = cargo.lockfile.as_ref().expect("Cargo knows a lockfile");
    assert_eq!(lockfile.manifest, "Cargo.toml");
    assert!(register.is_lockfile(&lockfile.name));
    assert_eq!(lockfile.stands, Stands::CargoWorkspaceRoot);

    // A name no check covers would be a second list of lockfiles.
    let written = "name = \"Cargo.lock\"";
    assert!(BUILTIN_REGISTER.contains(written));
    let message = refuses(&BUILTIN_REGISTER.replace(written, "name = \"yarn.lock\""));
    assert_eq!(
        message,
        "cargo refers to the lockfile \"yarn.lock\", which the register does not define"
    );
}

#[test]
fn some_files_are_left_to_nobody_on_purpose() {
    let central = Central::builtin().expect("what is compiled in");
    let nothing = Declaration::parse("[repo]\n").expect("a declaration");
    let excluded = |path: &str| {
        matches!(
            central.ruling(
                &nothing,
                &RelativePath::new(path).expect("a path"),
                &Tree::default()
            ),
            Ruling::Excluded { .. }
        )
    };

    assert!(excluded("design/logo.png"));
    assert!(excluded("main.pdf"));
    assert!(!excluded("src/lib.rs"));

    // The required files that carry no extension have to be named, or
    // every repository would trip over its own LICENSE.
    assert!(excluded("LICENSE"));
    assert!(excluded(".gitignore"));
    assert!(excluded("justfile"));
    assert!(
        excluded("crates/thing/.gitignore"),
        "**/ matches everywhere"
    );
    assert!(!excluded("mystery"));

    // Lockfiles in a format no check covers.
    assert!(excluded("yarn.lock"));
    assert!(excluded("backend/Gemfile.lock"));
    assert!(!excluded("Cargo.lock"));
    assert!(!excluded("composer.lock"));
}

#[test]
fn a_register_whose_entries_point_at_nothing_is_refused() {
    let sound = r#"
[version.v]
copy = "v"
[check.markdown]
extensions = [".md"]
[build.b]
produces = "thing"
[target.t]
accepts = ["thing"]
release = "single-stage"
[ecosystem.e]
package-ecosystem = "e"
manifests = ["E.toml"]
[preset.p]
version = ["v"]
check = ["markdown"]
build = "b"
publish = ["t"]
dependabot = ["e"]
"#;
    assert!(Register::parse(sound).is_ok());

    assert!(
        refuses(&sound.replace("dependabot = [\"e\"]", "dependabot = [\"missing\"]"))
            .contains("ecosystem")
    );
    assert!(
        refuses(&sound.replace("version = [\"v\"]", "version = [\"missing\"]"))
            .contains("version writer")
    );
    assert!(
        refuses(&sound.replace("check = [\"markdown\"]", "check = [\"missing\"]"))
            .contains("check")
    );
    assert!(refuses(&sound.replace("build = \"b\"\n", "build = \"missing\"\n")).contains("build"));
    assert!(
        refuses(&sound.replace("publish = [\"t\"]", "publish = [\"missing\"]")).contains("target")
    );
    assert!(
        refuses(&sound.replace(
            "[target.t]\naccepts = [\"thing\"]",
            "[target.t]\naccepts = [\"thing\"]\nrequires-check = \"missing\""
        ))
        .contains("check")
    );

    let message = refuses(&sound.replace(
        "produces = \"thing\"",
        "produces = \"thing\"\nrequires-version = [\"missing\"]",
    ));
    assert!(message.contains("version writer"), "{message}");
    assert!(message.contains("\"missing\""), "{message}");
    assert!(
        Register::parse(&sound.replace(
            "produces = \"thing\"",
            "produces = \"thing\"\nrequires-version = [\"v\"]",
        ))
        .is_ok()
    );

    // A build names the programs it calls, and each has to be pinned.
    let message = refuses(&sound.replace(
        "produces = \"thing\"",
        "produces = \"thing\"\nprograms = [\"missing\"]",
    ));
    assert!(message.contains("program"), "{message}");
    assert!(message.contains("\"missing\""), "{message}");

    let message = refuses("this is not toml");
    assert!(message.contains("not readable"), "{message}");
    assert!(matches!(
        Register::parse("this is not toml"),
        Err(RegisterError::Unreadable(_))
    ));
}

#[test]
fn an_artifact_kind_has_to_be_a_name() {
    let message = refuses(
        "[version]\n[check.a]\nextensions = [\".a\"]\n[build.b]\nproduces = 4\n[target]\n\
         [preset]\n[ecosystem]\n",
    );
    assert!(message.contains("string"), "{message}");

    // The same one level down: an extension is a string.
    let message = refuses(
        "[version]\n[check.a]\nextensions = [4]\n[build]\n[target]\n[preset]\n[ecosystem]\n",
    );
    assert!(message.contains("string"), "{message}");
}

#[test]
fn the_two_library_presets_differ_only_in_where_they_publish() {
    let register = builtin();
    let preset = |name: &str| {
        register
            .preset
            .get(&PresetName::new(name).expect("a name"))
            .unwrap_or_else(|| panic!("the {name} preset"))
    };

    let private = preset("rust-library");
    let public = preset("rust-library-public");

    assert_eq!(private.check, public.check);
    assert_eq!(private.build, public.build);

    let target = |publish: &Publish| match publish {
        Publish::Nothing => panic!("both of these publish"),
        Publish::To(targets) => targets.iter().map(ToString::to_string).collect::<Vec<_>>(),
    };
    assert_eq!(target(&private.publish), ["private-registry"]);

    // Only crates.io, and deliberately not both: a crate that is on crates.io
    // is resolvable from there for everyone, and a second copy would raise the
    // question which of the two is the real one.
    assert_eq!(target(&public.publish), ["crates.io"]);
}

#[test]
fn the_target_that_cannot_be_taken_back_is_the_one_with_a_condition() {
    let register = builtin();
    let target = |name: &str| {
        register
            .target
            .get(&TargetName::new(name).expect("a name"))
            .unwrap_or_else(|| panic!("the {name} target"))
    };

    assert_eq!(
        target("crates.io").requires_check,
        CheckName::new("rust").ok()
    );
    assert_eq!(target("private-registry").requires_check, None);
}

#[test]
fn the_package_side_is_rust_library_without_check_and_dependabot() {
    let register = builtin();
    let preset = |name: &str| {
        register
            .preset
            .get(&PresetName::new(name).expect("a name"))
            .unwrap_or_else(|| panic!("the {name} preset"))
    };

    // The member builds and publishes but does not check: the workspace runs
    // the check, and it is enough that *one* containing unit runs it. And it
    // watches nothing: it has nothing of its own to resolve.
    for name in ["rust-library-package", "rust-library-public-package"] {
        let member = preset(name);
        assert!(member.check.is_empty(), "{name} does not check");
        assert!(member.dependabot.is_empty(), "{name} watches nothing");
        assert!(member.build.is_some(), "{name} builds");
        assert_ne!(member.publish, Publish::Nothing, "{name} publishes");
    }

    // The package side is rust-library without check and without dependabot:
    // the workspace carries those — rust-workspace, or the product preset of
    // a workspace that is a product itself.
    let whole = preset("rust-library");
    let member = preset("rust-library-package");
    assert_eq!(whole.version, member.version);
    assert_eq!(whole.build, member.build);
    assert_eq!(whole.publish, member.publish);
    assert!(!whole.check.is_empty() && member.check.is_empty());
    assert!(!whole.dependabot.is_empty() && member.dependabot.is_empty());
}

#[test]
fn a_build_names_the_version_writers_it_needs() {
    let register = builtin();
    let needs = |name: &str| {
        register
            .build
            .get(&BuildName::new(name).expect("a name"))
            .unwrap_or_else(|| panic!("the {name} build"))
            .requires_version
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };

    // A crate carries the version of its Cargo.toml, which only the writer
    // `cargo` sets to the unit's.
    assert_eq!(needs("cargo-package"), ["cargo"]);
    assert!(needs("docker").is_empty());
}

#[test]
fn a_build_names_the_programs_it_calls() {
    let register = builtin();
    let calls = |name: &str| {
        register
            .build
            .get(&BuildName::new(name).expect("a name"))
            .unwrap_or_else(|| panic!("the {name} build"))
            .programs
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };

    // The image is scanned before it is handed on.
    assert_eq!(calls("docker"), ["trivy"]);
    assert!(calls("cargo-package").is_empty());
}

#[test]
fn a_build_names_the_files_it_reads_in_the_unit() {
    let register = builtin();
    let reads = |name: &str| {
        register
            .build
            .get(&BuildName::new(name).expect("a name"))
            .unwrap_or_else(|| panic!("the {name} build"))
            .requires_file
            .clone()
    };

    // The image is built from deploy/Dockerfile below the unit, and nothing
    // else.
    assert_eq!(
        reads("docker"),
        [RelativePath::new("deploy/Dockerfile").expect("a path")]
    );
    assert!(reads("cargo-package").is_empty());
}

#[test]
fn a_file_a_build_needs_is_a_path_inside_the_unit() {
    let builtin = BUILTIN_REGISTER;
    let docker = "requires-file = [\"deploy/Dockerfile\"]";
    assert!(
        builtin.contains(docker),
        "the docker build reads its Dockerfile"
    );
    for outside in ["/deploy/Dockerfile", "../Dockerfile"] {
        let message =
            refuses(&builtin.replace(docker, &format!("requires-file = [\"{outside}\"]")));
        assert!(message.contains("not readable"), "{outside}: {message}");
    }
}

#[test]
fn every_preset_selects_the_version_writers_its_build_needs() {
    let register = builtin();
    let mut checked = 0;
    for (name, preset) in &register.preset {
        let Some(build) = &preset.build else {
            continue;
        };
        let needs: &[VersionWriterName] = &register.build[build].requires_version;
        for writer in needs {
            assert!(
                preset.version.contains(writer),
                "the {name} preset builds {build}, which needs the version writer {writer}"
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "some preset builds what needs a writer");
}
