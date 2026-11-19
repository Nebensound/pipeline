//! The JSON Schema of `.nebensound/repo.toml` that `check-toml` assigns is the
//! one the types of the declaration describe.
//!
//! The file lies beside `check-toml`, which reads it from its own commit. It is
//! compared as JSON, not as text: it is committed the way Prettier writes it,
//! so that `check-json` passes it, and the tool prints it the way `serde_json`
//! does. The README beside the file says how to write it again.
//!
//! This lives in the tool and not in the library: the library is published,
//! and in its package the action is not there to be read.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;
use std::path::Path;

use support::call;

const COMMITTED: &str = "../../.github/actions/check-toml/schemas/nebensound-declaration.json";

fn committed() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(COMMITTED);
    let text = fs::read_to_string(&path).expect("the committed schema can be read");
    serde_json::from_str(&text).expect("the committed schema is JSON")
}

#[test]
fn the_committed_schema_is_the_generated_one() {
    let (text, code) = call(&["nebensound", "schema"]);
    assert_eq!(code, 0, "{text}");
    let generated: serde_json::Value = serde_json::from_str(&text).expect("the tool prints JSON");

    assert!(
        generated == committed(),
        "{COMMITTED} is not the schema the declaration types describe; the README beside it says \
         how to write it again"
    );
}

#[test]
fn the_schema_reads_no_repository() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let nowhere = dir.path().join("nothing here");
    let nowhere = nowhere.to_str().expect("a path in UTF-8");

    let (text, code) = call(&["nebensound", "-C", nowhere, "schema"]);

    assert_eq!(code, 0, "{text}");
    assert!(text.starts_with('{'), "{text}");
}

#[test]
fn the_schema_holds_what_phase_0_holds() {
    let schema = committed();
    let declaration = &schema["properties"];
    let definitions = &schema["definitions"];

    // The shape is closed, as `deny_unknown_fields` closes it: a misspelt key
    // is marked where it is written instead of meaning nothing.
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["required"], serde_json::json!(["repo"]));
    for table in ["Repo", "Component", "ComponentDefaults", "ExplicitPipeline"] {
        assert_eq!(
            definitions[table]["additionalProperties"], false,
            "{table} is closed"
        );
    }
    // TOML has no null, so no field offers one.
    assert!(!schema.to_string().contains("null"), "no null anywhere");
    // A repository excludes paths only, never a pattern.
    assert_eq!(
        definitions["Exclusion"]["properties"]["path"]["allOf"][0]["$ref"],
        "#/definitions/RelativePath"
    );
    assert!(declaration["component"]["patternProperties"].is_object());
}

#[test]
fn the_call_says_what_it_prints() {
    let (text, code) = call(&["nebensound", "schema", "--help"]);

    assert_eq!(code, 0, "{text}");
    assert!(text.contains("JSON Schema"), "{text}");
}

/// Every string of up to `len` characters drawn from `alphabet`.
fn strings(alphabet: &[char], len: usize) -> Vec<String> {
    let mut all = vec![String::new()];
    let mut last = vec![String::new()];
    for _ in 0..len {
        last = last
            .iter()
            .flat_map(|prefix| {
                alphabet.iter().map(move |c| {
                    let mut next = prefix.clone();
                    next.push(*c);
                    next
                })
            })
            .collect();
        all.extend(last.iter().cloned());
    }
    all
}

/// A `pattern` of the committed schema, compiled. JSON Schema matches a
/// pattern anywhere in the string, as `is_match` does.
fn pattern(schema: &serde_json::Value) -> regex::Regex {
    regex::Regex::new(schema["pattern"].as_str().expect("a pattern")).expect("a valid pattern")
}

// The patterns of the types that read themselves by hand are written in
// schema.rs, beside the rule they mirror, not derived from it. These hold them
// to the parsers over every short string of an alphabet that reaches every
// rule, so that a change to a parser cannot leave its pattern behind unseen.

#[test]
fn a_name_pattern_accepts_exactly_what_its_parser_accepts() {
    use nebensound_declaration::names::{
        BuildName, CheckName, ComponentName, EcosystemName, PresetName, TargetName,
        VersionWriterName,
    };

    type Parses = fn(&str) -> bool;

    let schema = committed();
    // A component name is a key, so its pattern stands as the one key of
    // `patternProperties` rather than as a definition.
    let component = schema["properties"]["component"]["patternProperties"]
        .as_object()
        .and_then(|keys| keys.keys().next())
        .expect("the pattern of a component name");
    let component = regex::Regex::new(component).expect("a valid pattern");
    let candidates = strings(&['a', '1', '-', '.', 'A', '_'], 5);
    for candidate in &candidates {
        assert_eq!(
            component.is_match(candidate),
            ComponentName::new(candidate).is_ok(),
            "ComponentName: the schema and the parser disagree on {candidate:?}"
        );
    }

    let parsers: [(&str, Parses); 6] = [
        ("PresetName", |s| PresetName::new(s).is_ok()),
        ("CheckName", |s| CheckName::new(s).is_ok()),
        ("BuildName", |s| BuildName::new(s).is_ok()),
        ("TargetName", |s| TargetName::new(s).is_ok()),
        ("VersionWriterName", |s| VersionWriterName::new(s).is_ok()),
        ("EcosystemName", |s| EcosystemName::new(s).is_ok()),
    ];
    for (name, parses) in parsers {
        let regex = pattern(&schema["definitions"][name]);
        for candidate in &candidates {
            assert_eq!(
                regex.is_match(candidate),
                parses(candidate),
                "{name}: the schema and the parser disagree on {candidate:?}"
            );
        }
    }
}

#[test]
fn the_path_pattern_accepts_exactly_what_the_parser_accepts() {
    use nebensound_declaration::path::RelativePath;

    let regex = pattern(&committed()["definitions"]["RelativePath"]);
    for candidate in strings(&['a', '.', '/', '\\'], 6) {
        assert_eq!(
            regex.is_match(&candidate),
            RelativePath::new(&candidate).is_ok(),
            "the schema and the parser disagree on {candidate:?}"
        );
    }
}

#[test]
fn the_reason_pattern_accepts_exactly_what_the_parser_accepts() {
    use nebensound_declaration::names::Reason;

    let regex = pattern(&committed()["definitions"]["Reason"]);
    for candidate in strings(&['a', ' ', '\t', '\n'], 4) {
        assert_eq!(
            regex.is_match(&candidate),
            Reason::new(&candidate).is_ok(),
            "the schema and the parser disagree on {candidate:?}"
        );
    }
}

#[test]
fn the_pattern_schema_lets_through_everything_the_parser_accepts() {
    use nebensound_declaration::pattern::Pattern;

    // Looser on purpose (see schema.rs): every pattern the parser reads has to
    // pass, not the other way round.
    let schema = committed();
    let choices: Vec<regex::Regex> = schema["definitions"]["Pattern"]["anyOf"]
        .as_array()
        .expect("the forms of a pattern")
        .iter()
        .map(pattern)
        .collect();
    for candidate in strings(&['a', '.', '/', '*', '-'], 6) {
        if Pattern::new(&candidate).is_ok() {
            assert!(
                choices.iter().any(|regex| regex.is_match(&candidate)),
                "the parser reads {candidate:?}, the schema refuses it"
            );
        }
    }
}

#[test]
fn the_version_pattern_accepts_exactly_what_the_parser_accepts() {
    use nebensound_declaration::TagVersion;

    // Short strings hold no number beyond `u64`, the one thing the parser
    // refuses that a pattern cannot say.
    let regex = pattern(&committed()["definitions"]["Version"]);
    let agree = |candidate: &str| {
        assert_eq!(
            regex.is_match(candidate),
            TagVersion::read(candidate).is_ok(),
            "the schema and the parser disagree on {candidate:?}"
        );
    };
    for candidate in strings(&['0', '1', '.', 'v', '-'], 6) {
        agree(&candidate);
    }
    // The pre-release part, behind three numbers that read.
    for suffix in strings(&['p', 'r', 'e', '.', '0', '1', '-'], 6) {
        agree(&format!("1.0.0-{suffix}"));
    }
}

#[test]
fn both_fields_stand_on_the_repository_and_on_a_component() {
    let schema = committed();
    let definitions = &schema["definitions"];

    for table in ["Repo", "Component"] {
        let properties = &definitions[table]["properties"];
        assert_eq!(
            properties["release"]["anyOf"][0]["$ref"], "#/definitions/Release",
            "{table}"
        );
        assert_eq!(
            properties["minimum-version"]["anyOf"][0]["$ref"], "#/definitions/Version",
            "{table}"
        );
    }
    let values: Vec<&serde_json::Value> = definitions["Release"]["oneOf"]
        .as_array()
        .expect("the values of a release mode")
        .iter()
        .map(|value| &value["const"])
        .collect();
    assert_eq!(values, ["single-stage", "two-stage"]);
}
