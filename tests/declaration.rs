//! `docs/declaration.md` describes every field the schema of the declaration
//! knows, and every finding code phase 0 can report.
//!
//! The fields are read off the schema `nebensound-repo schema` prints, which is
//! derived from the types phase 0 reads the file with. A field added to those
//! types fails here until the page describes it, and a row the page keeps for a
//! field that no longer exists fails as well.
//!
//! It reads a file outside the package: in the crate `cargo package` builds,
//! the page is not there to be read, so the package leaves this file out (see
//! its `Cargo.toml`).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde_json::Value;
use support::call;

const PAGE: &str = "docs/declaration.md";
const FINDINGS: &str = "src/finding.rs";

/// The heading of the section that describes the keys at the top of the file.
const TOP_LEVEL: &str = "The top level";

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{relative} can be read: {error}"))
}

fn schema() -> Value {
    let (text, code) = call(&["nebensound-repo", "schema"]);
    assert_eq!(code, 0, "{text}");
    serde_json::from_str(&text).expect("the tool prints JSON")
}

/// Where each field is written: the table it stands in, and its name.
///
/// The table is named as a reader finds it in the file: `repo`,
/// `repo.component`, `component.<name>`, `include`. A table that can stand in
/// several places, as the pipeline can, is described once under the name of
/// its field.
fn fields(schema: &Value) -> BTreeSet<(String, String)> {
    let mut references = BTreeMap::new();
    count_references(schema, &mut references);
    let mut found = BTreeSet::new();
    walk(schema, schema, &references, String::new(), &mut found);
    found
}

fn count_references(node: &Value, counts: &mut BTreeMap<String, usize>) {
    match node {
        Value::Object(map) => {
            if let Some(Value::String(target)) = map.get("$ref") {
                *counts.entry(target.clone()).or_default() += 1;
            }
            for value in map.values() {
                count_references(value, counts);
            }
        }
        Value::Array(items) => {
            for value in items {
                count_references(value, counts);
            }
        }
        _ => {}
    }
}

/// The definition a field's schema refers to, directly or through the
/// `allOf` / `anyOf` schemars wraps an optional or described field in.
fn referenced(node: &Value) -> Option<&str> {
    node.get("$ref").and_then(Value::as_str).or_else(|| {
        ["allOf", "anyOf"]
            .iter()
            .filter_map(|key| node.get(*key).and_then(Value::as_array))
            .flatten()
            .find_map(|choice| choice.get("$ref").and_then(Value::as_str))
    })
}

fn walk(
    node: &Value,
    schema: &Value,
    references: &BTreeMap<String, usize>,
    table: String,
    found: &mut BTreeSet<(String, String)>,
) {
    if let Some(target) = node.get("$ref").and_then(Value::as_str) {
        let name = target
            .strip_prefix("#/definitions/")
            .expect("a reference into the definitions");
        walk(
            &schema["definitions"][name],
            schema,
            references,
            table,
            found,
        );
        return;
    }
    if let Some(properties) = node.get("properties").and_then(Value::as_object) {
        for (key, value) in properties {
            found.insert((table.clone(), key.clone()));
            let shared = referenced(value).is_some_and(|target| references[target] > 1);
            let below = if shared || table.is_empty() {
                key.clone()
            } else {
                format!("{table}.{key}")
            };
            walk(value, schema, references, below, found);
        }
    }
    if let Some(keys) = node.get("patternProperties").and_then(Value::as_object) {
        for value in keys.values() {
            walk(value, schema, references, format!("{table}.<name>"), found);
        }
    }
    if let Some(items) = node.get("items") {
        walk(items, schema, references, table.clone(), found);
    }
    for key in ["allOf", "anyOf", "oneOf"] {
        for choice in node
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            walk(choice, schema, references, table.clone(), found);
        }
    }
}

/// A section of the page: its heading and the rows of its tables.
struct Section {
    heading: String,
    level: usize,
    rows: Vec<Vec<String>>,
}

/// The page split at its headings. Code blocks are not read for headings.
fn sections(page: &str) -> Vec<Section> {
    let mut all: Vec<Section> = Vec::new();
    let mut fenced = false;
    for line in page.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            let level = 1 + rest.chars().take_while(|c| *c == '#').count();
            all.push(Section {
                heading: rest.trim_start_matches('#').trim().to_owned(),
                level,
                rows: Vec::new(),
            });
        } else if line.starts_with('|')
            && let Some(section) = all.last_mut()
        {
            let cells = line
                .trim()
                .trim_matches('|')
                .split(" | ")
                .map(|cell| cell.trim().to_owned())
                .collect();
            section.rows.push(cells);
        }
    }
    all
}

/// The first code span of a text, if it has one.
fn code_span(text: &str) -> Option<&str> {
    let (_, rest) = text.split_once('`')?;
    let (span, _) = rest.split_once('`')?;
    Some(span)
}

/// The table a heading describes: its code span without the brackets of a TOML
/// header, or the empty name for the top level.
fn table_of(heading: &str) -> Option<String> {
    if heading == TOP_LEVEL {
        return Some(String::new());
    }
    code_span(heading).map(|span| span.trim_matches(['[', ']']).to_owned())
}

fn shown(table: &str) -> &str {
    if table.is_empty() { TOP_LEVEL } else { table }
}

#[test]
fn every_field_the_schema_knows_is_described() {
    let known = fields(&schema());
    let tables: BTreeSet<&String> = known.iter().map(|(table, _)| table).collect();
    let page = read(PAGE);

    // The rows of every section that describes a table, by table.
    let mut described: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
    for section in sections(&page) {
        if let Some(table) = table_of(&section.heading)
            && tables.contains(&table)
        {
            described.entry(table).or_default().extend(section.rows);
        }
    }

    for (table, field) in &known {
        let rows = described.get(table).unwrap_or_else(|| {
            panic!(
                "{PAGE} has no section for `{}`, which holds `{field}`",
                shown(table)
            )
        });
        let row = rows
            .iter()
            .find(|row| row[0] == format!("`{field}`"))
            .unwrap_or_else(|| {
                panic!(
                    "{PAGE} does not describe `{field}` in the section for `{}`",
                    shown(table)
                )
            });
        assert!(
            row.len() == 4 && row.iter().all(|cell| !cell.is_empty()),
            "the row of `{field}` in `{}` gives its type, default and effect: {row:?}",
            shown(table)
        );
    }

    for (table, rows) in &described {
        for row in rows.iter().skip(2) {
            let field = row[0].trim_matches('`');
            assert!(
                known.contains(&(table.clone(), field.to_owned())),
                "{PAGE} describes `{field}` in `{}`, and the schema has no such field there",
                shown(table)
            );
        }
    }
}

#[test]
fn the_schema_is_read_where_a_reader_finds_it() {
    let known = fields(&schema());

    for expected in [
        ("", "repo"),
        ("repo", "allow-unused"),
        ("repo.component", "pipeline"),
        ("component.<name>", "path"),
        ("pipeline", "repo"),
        ("pipeline", "check"),
        ("include", "reason"),
        ("exclude", "path"),
    ] {
        let expected = (expected.0.to_owned(), expected.1.to_owned());
        assert!(known.contains(&expected), "{expected:?} in {known:?}");
    }
}

/// The codes `Rule::code` gives, read off its source.
///
/// Each arm is read from its `Self::` to the string after it, so an arm that
/// rustfmt wraps onto two lines is read like one on a single line.
fn codes() -> BTreeSet<String> {
    let source = read(FINDINGS);
    let (_, body) = source
        .split_once("pub fn code(self)")
        .expect("Rule::code in finding.rs");
    let (body, _) = body.split_once("\n    }\n").expect("the end of Rule::code");
    body.split("Self::")
        .skip(1)
        .map(|arm| {
            let (_, rest) = arm.split_once('"').expect("a code in each arm");
            let (code, _) = rest.split_once('"').expect("the end of the code");
            code.to_owned()
        })
        .collect()
}

/// How many variants `enum Rule` declares, read off its source.
fn variants() -> usize {
    let source = read(FINDINGS);
    let (_, body) = source
        .split_once("pub enum Rule {")
        .expect("enum Rule in finding.rs");
    let (body, _) = body.split_once("\n}\n").expect("the end of enum Rule");
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//") && !line.starts_with("#["))
        .count()
}

#[test]
fn every_variant_of_rule_has_a_code_read() {
    assert_eq!(
        codes().len(),
        variants(),
        "the codes read off Rule::code are as many as the variants of Rule, each once"
    );
}

#[test]
fn every_finding_code_is_described() {
    let codes = codes();
    assert!(codes.contains("uncovered-file"), "{codes:?}");

    let page = read(PAGE);
    let all = sections(&page);
    let start = all
        .iter()
        .position(|section| section.heading == "Findings")
        .expect("a section Findings");
    let described: BTreeSet<String> = all[start + 1..]
        .iter()
        .take_while(|section| section.level > all[start].level)
        .filter_map(|section| code_span(&section.heading).map(str::to_owned))
        .collect();

    assert_eq!(
        described, codes,
        "{PAGE} describes each finding code under Findings, once"
    );
}
