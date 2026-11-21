//! The programs the checks and builds call, and the runtimes they run on:
//! pinned, with a download per platform and its digest.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use nebensound_repo::names::{CheckName, ProgramName};
use nebensound_repo::programs::{
    Algorithm, Digest, Installer, Platform, Program, at_least, is_version,
};
use nebensound_repo::register::{Register, RegisterError};

fn builtin() -> Register {
    Register::builtin().expect("the register this crate was built with")
}

fn refuses(source: &str) -> String {
    match Register::parse(source) {
        Ok(_) => panic!("expected this register to be refused, and it was accepted"),
        Err(error) => error.to_string(),
    }
}

fn names(list: &[(&ProgramName, &Program)]) -> Vec<String> {
    list.iter().map(|(name, _)| name.to_string()).collect()
}

fn check(name: &str) -> CheckName {
    CheckName::new(name).expect("a name")
}

#[test]
fn every_check_with_a_building_block_names_the_programs_it_calls() {
    let register = builtin();
    for (name, programs) in [
        ("css", &["prettier", "stylelint"][..]),
        ("dockerfile", &["dockerfmt", "hadolint"]),
        ("html", &["html-validate", "lychee", "prettier"]),
        ("json", &["biome", "jq", "prettier"]),
        ("markdown", &["lychee", "markdownlint-cli2", "prettier"]),
        ("rust", &["cargo-llvm-cov"]),
        ("toml", &["taplo"]),
        (
            "yaml",
            &["actionlint", "check-jsonschema", "prettier", "yamllint"],
        ),
    ] {
        assert_eq!(
            names(&register.programs_of(&[check(name)])),
            programs,
            "{name}"
        );
    }
    // Each program once, in name order, whichever checks call it.
    assert_eq!(
        names(&register.programs_of(&[check("yaml"), check("json"), check("python")])),
        [
            "actionlint",
            "biome",
            "check-jsonschema",
            "jq",
            "prettier",
            "yamllint"
        ]
    );
}

#[test]
fn every_program_is_pinned_to_the_version_its_building_block_installs() {
    let register = builtin();
    for (name, version, runtime) in [
        ("actionlint", "1.7.12", None),
        ("biome", "2.5.15", None),
        ("cargo-llvm-cov", "0.9.0", None),
        ("check-jsonschema", "0.39.0", Some(("python", "3.11.0"))),
        ("dockerfmt", "1.0.0", None),
        ("hadolint", "2.15.1", None),
        ("html-validate", "11.21.1", Some(("node", "22.22.0"))),
        ("jq", "1.8.1", None),
        ("lychee", "0.24.2", None),
        ("markdownlint-cli2", "0.23.2", Some(("node", "22.0.0"))),
        ("prettier", "3.9.6", Some(("node", "14.0.0"))),
        ("stylelint", "18.0.0", Some(("node", "22.22.2"))),
        ("taplo", "0.10.0", None),
        ("trivy", "0.76.2", None),
        ("yamllint", "1.38.0", Some(("python", "3.10.0"))),
    ] {
        let program = &register.program[&ProgramName::new(name).expect("a name")];
        assert_eq!(program.version, version, "{name}");
        assert_eq!(
            program.runtime.as_ref().map(ToString::to_string).as_deref(),
            runtime.map(|(runtime, _)| runtime),
            "{name}"
        );
        assert_eq!(
            program.runtime_minimum.as_deref(),
            runtime.map(|(_, minimum)| minimum),
            "{name}"
        );
        assert_eq!(program.command, name, "{name}");
        let every = program.download.contains_key(&Platform::Any);
        assert_eq!(every, runtime.is_some(), "{name}");
        for (platform, download) in &program.download {
            assert!(download.url.starts_with("https://"), "{name} {platform}");
            assert!(download.url.contains(version), "{name} {platform}");
        }
    }
    assert_eq!(register.program.len(), 15);

    // Stylelint brings the configurations the pipeline's default extends and
    // the syntax it reads SCSS with; no other program brings anything.
    for (name, program) in &register.program {
        let companions: Vec<(&str, &str)> = program
            .companions
            .iter()
            .map(|companion| (companion.name.as_str(), companion.version.as_str()))
            .collect();
        let expected: &[(&str, &str)] = if name.as_str() == "stylelint" {
            &[
                ("stylelint-config-standard", "41.0.0"),
                ("stylelint-config-standard-scss", "18.0.0"),
                ("postcss-scss", "4.0.9"),
            ]
        } else {
            &[]
        };
        assert_eq!(companions, expected, "{name}");
        for companion in &program.companions {
            let download = companion.download();
            assert!(download.url.starts_with("https://registry.npmjs.org/"));
            assert!(
                download
                    .url
                    .ends_with(&format!("-{}.tgz", companion.version))
            );
            assert_eq!(download.executable, None);
            assert_eq!(download.digest.algorithm(), Algorithm::Sha512);
        }
    }

    let runtimes: Vec<(String, String, Installer)> = register
        .runtime
        .iter()
        .map(|(name, runtime)| (name.to_string(), runtime.version.clone(), runtime.installer))
        .collect();
    assert_eq!(
        runtimes,
        [
            ("node".to_owned(), "24.22.3".to_owned(), Installer::Npm),
            ("python".to_owned(), "3.14.10".to_owned(), Installer::Pip),
        ]
    );
}

/// The smallest register with one program on a runtime and one with builds.
const SOUND: &str = r#"
[version]
[build]
[target]
[preset]
[ecosystem]
[check.json]
extensions = [".json"]
programs = ["tool", "package"]

[program.tool]
version = "1.2.3"
command = "tool"
[program.tool.download.linux-x86_64]
url = "https://example.org/tool.tar.gz"
digest = "sha256:0000000000000000000000000000000000000000000000000000000000000000"
executable = "tool"

[program.package]
version = "4.5.6"
command = "package"
runtime = "rt"
runtime-minimum = "7.0.0"
[program.package.download.any]
url = "https://example.org/package.tgz"
digest = "sha256:0000000000000000000000000000000000000000000000000000000000000000"

[runtime.rt]
title = "A runtime"
version = "7.8.9"
commands = ["rt"]
installer = "npm"
[runtime.rt.download.macos-aarch64]
url = "https://example.org/rt.zip"
digest = "sha256:0000000000000000000000000000000000000000000000000000000000000000"
executable = "rt/bin/rt"
"#;

#[test]
fn a_sound_register_reads_with_its_defaults() {
    let register = Register::parse(SOUND).expect("a sound register");
    let tool = &register.program[&ProgramName::new("tool").expect("a name")];
    assert_eq!(tool.version_args, ["--version"]);
    assert_eq!(tool.runtime, None);
    let rt = register.runtime.values().next().expect("one runtime");
    assert_eq!(rt.version_args, ["--version"]);
    assert_eq!(rt.title, "A runtime");
}

#[test]
fn a_program_or_runtime_that_does_not_fit_is_refused() {
    assert!(Register::parse(SOUND).is_ok());

    let message = refuses(&SOUND.replace(
        "programs = [\"tool\", \"package\"]",
        "programs = [\"missing\"]",
    ));
    assert!(
        message.contains("refers to the program \"missing\""),
        "{message}"
    );

    let message = refuses(&SOUND.replace("runtime = \"rt\"", "runtime = \"missing\""));
    assert!(
        message.contains("refers to the runtime \"missing\""),
        "{message}"
    );

    let message = refuses(&SOUND.replace("version = \"1.2.3\"", "version = \"1.2\""));
    assert_eq!(
        message,
        "tool pins \"1.2\", and a version here is three numbers, such as 1.2.3"
    );
    assert!(matches!(
        Register::parse(&SOUND.replace("version = \"7.8.9\"", "version = \"latest\"")),
        Err(RegisterError::Version { .. })
    ));

    for (from, to, problem) in [
        (
            "[program.package.download.any]",
            "[program.package.download.linux-x86_64]",
            "package runs on a runtime, so it has one download, for `any`, and names no \
             executable",
        ),
        (
            "url = \"https://example.org/package.tgz\"",
            "url = \"https://example.org/package.tgz\"\nexecutable = \"package\"",
            "package runs on a runtime",
        ),
        (
            "[program.tool.download.linux-x86_64]",
            "[program.tool.download.any]",
            "tool has a download for `any`, which only a program on a runtime has",
        ),
        (
            "executable = \"tool\"\n",
            "",
            "tool has a build that names no executable",
        ),
        (
            "[runtime.rt.download.macos-aarch64]",
            "[runtime.rt.download.any]",
            "rt has a download for `any`, which only a program on a runtime has",
        ),
    ] {
        let message = refuses(&SOUND.replace(from, to));
        assert!(message.starts_with(problem), "{message}");
    }

    let message = refuses(
        "[version]\n[build]\n[target]\n[preset]\n[ecosystem]\n[check.json]\nextensions = \
         [\".json\"]\n[program.bare]\nversion = \"1.0.0\"\ncommand = \"bare\"\ndownload = {}\n",
    );
    assert_eq!(message, "bare has no download");

    // A companion is pinned like its program, and only a program on a runtime
    // has one: its installer puts it beside the program.
    let companion = "[[program.package.companion]]\nname = \"extra\"\nversion = \"1.0.0\"\n\
                     url = \"https://example.org/extra.tgz\"\ndigest = \"sha256:\
                     0000000000000000000000000000000000000000000000000000000000000000\"\n";
    assert!(Register::parse(&format!("{SOUND}{companion}")).is_ok());
    let message = refuses(&format!("{SOUND}{}", companion.replace("1.0.0", "1.0")));
    assert_eq!(
        message,
        "the companion extra of package pins \"1.0\", and a version here is three numbers, \
         such as 1.2.3"
    );
    let message = refuses(&format!(
        "{SOUND}{}",
        companion.replace("program.package.", "program.tool.")
    ));
    assert_eq!(
        message,
        "tool names a companion, and only a program on a runtime has one"
    );

    let message = refuses(&SOUND.replace("installer = \"npm\"", "installer = \"cargo\""));
    assert!(message.contains("unknown variant"), "{message}");
    let message = refuses(&SOUND.replace(
        "[runtime.rt.download.macos-aarch64]",
        "[runtime.rt.download.amiga-m68k]",
    ));
    assert!(message.contains("unknown variant"), "{message}");
}

#[test]
fn a_digest_names_its_algorithm_and_has_its_length() {
    let hex256 = "ab".repeat(32);
    let digest = Digest::new(&format!("sha256:{hex256}")).expect("a digest");
    assert_eq!(digest.algorithm(), Algorithm::Sha256);
    assert_eq!(digest.hex(), hex256);
    assert_eq!(digest.to_string(), format!("sha256:{hex256}"));
    let digest = Digest::new(&format!("sha512:{}", "0".repeat(128))).expect("a digest");
    assert_eq!(digest.algorithm().as_str(), "sha512");

    for (value, problem) in [
        ("abc", "it reads <algorithm>:<digits>"),
        ("md5:abc", "the algorithm is sha256 or sha512"),
        ("sha256:abc", "sha256 has 64 lowercase hexadecimal digits"),
        (
            &format!("sha256:{}", "AB".repeat(32)),
            "sha256 has 64 lowercase hexadecimal digits",
        ),
        (
            &format!("sha512:{hex256}"),
            "sha512 has 128 lowercase hexadecimal digits",
        ),
    ] {
        let message = Digest::new(value).expect_err("refused");
        assert!(message.ends_with(problem), "{message}");
    }

    let message = refuses(&SOUND.replacen(
        "digest = \"sha256:0000000000000000000000000000000000000000000000000000000000000000\"",
        "digest = \"sha256:0\"",
        1,
    ));
    assert!(message.contains("is no digest"), "{message}");

    // A digest is a string in the first place.
    let message = refuses(&SOUND.replacen(
        "digest = \"sha256:0000000000000000000000000000000000000000000000000000000000000000\"",
        "digest = 4",
        1,
    ));
    assert!(message.contains("invalid type"), "{message}");
}

#[test]
fn a_platform_is_an_operating_system_and_an_architecture() {
    for (os, arch, key, windows) in [
        ("linux", "x86_64", "linux-x86_64", false),
        ("linux", "aarch64", "linux-aarch64", false),
        ("macos", "x86_64", "macos-x86_64", false),
        ("macos", "aarch64", "macos-aarch64", false),
        ("windows", "x86_64", "windows-x86_64", true),
        ("windows", "aarch64", "windows-aarch64", true),
    ] {
        let platform = Platform::of(os, arch).expect("a platform the register knows");
        assert_eq!(platform.as_str(), key);
        assert_eq!(platform.to_string(), key);
        assert_eq!(platform.is_windows(), windows);
    }
    assert_eq!(Platform::of("freebsd", "x86_64"), None);
    assert_eq!(Platform::of("linux", "riscv64"), None);
    assert_eq!(Platform::Any.as_str(), "any");
    assert!(!Platform::Any.is_windows());
}

#[test]
fn a_version_is_three_numbers() {
    assert!(is_version("1.2.3"));
    assert!(is_version("24.22.3"));
    assert!(!is_version("1.2"));
    assert!(!is_version("1.2.3.4"));
    assert!(!is_version("1..3"));
    assert!(!is_version("v1.2.3"));
    assert!(!is_version("+1.2.3"));
    assert_eq!(Installer::Npm.to_string(), "npm");
    assert_eq!(Installer::Pip.to_string(), "pip");
}

#[test]
fn a_version_is_compared_number_by_number() {
    for (version, floor, later) in [
        ("3.11.0", "3.11.0", true),
        ("3.14.10", "3.11.0", true),
        ("3.9.19", "3.11.0", false),
        ("3.10.0", "3.9.99", true),
        ("24.22.3", "22.0.0", true),
        ("4.1.0", "4.2.0", false),
        ("4.2.1", "4.2.0", true),
        ("4.2.0", "4.2.1", false),
        // Not three numbers that fit in 64 bits: nothing to compare.
        ("3.11", "3.11.0", false),
        ("3.11.0", "3.11", false),
        ("99999999999999999999.0.0", "1.0.0", false),
        ("1.0.0", "99999999999999999999.0.0", false),
    ] {
        assert_eq!(at_least(version, floor), later, "{version} >= {floor}");
    }
}

#[test]
fn a_program_on_a_runtime_names_the_lowest_version_it_runs_on() {
    for (from, to, problem) in [
        (
            "runtime-minimum = \"7.0.0\"\n",
            "",
            "package runs on a runtime, so it names the lowest version of it that it runs on \
             (`runtime-minimum`)"
                .to_owned(),
        ),
        (
            "command = \"tool\"\n",
            "command = \"tool\"\nruntime-minimum = \"1.0.0\"\n",
            "tool names a `runtime-minimum`, and runs on no runtime".to_owned(),
        ),
        (
            "runtime-minimum = \"7.0.0\"",
            "runtime-minimum = \"8.0.0\"",
            "package needs rt 8.0.0 or later, and the register pins rt 7.8.9".to_owned(),
        ),
        (
            "runtime-minimum = \"7.0.0\"",
            "runtime-minimum = \"7.8.10\"",
            "package needs rt 7.8.10 or later, and the register pins rt 7.8.9".to_owned(),
        ),
        (
            "runtime-minimum = \"7.0.0\"",
            "runtime-minimum = \"7\"",
            "the `runtime-minimum` of package pins \"7\", and a version here is three \
             numbers, such as 1.2.3"
                .to_owned(),
        ),
    ] {
        assert_eq!(refuses(&SOUND.replace(from, to)), problem);
    }
    assert!(matches!(
        Register::parse(
            &SOUND.replace("runtime-minimum = \"7.0.0\"", "runtime-minimum = \"9.0.0\"")
        ),
        Err(RegisterError::Minimum { .. })
    ));
    // The pinned runtime itself is the highest a program may need.
    assert!(
        Register::parse(
            &SOUND.replace("runtime-minimum = \"7.0.0\"", "runtime-minimum = \"7.8.9\"")
        )
        .is_ok()
    );
}
