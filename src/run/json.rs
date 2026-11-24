//! The JSON check: every `.json`, `.jsonc` and `.geojson` file parses and is
//! formatted the way Prettier writes it; every JSON lockfile parses.
//!
//! Whether a file parses is the tool's own judgement: `serde_json` reads
//! strict JSON and `jsonc-parser` JSON with comments, both linked into the
//! tool. Only the format check starts a program, Prettier.

use serde::de::IgnoredAny;

use super::{BOM, Session, Tool, has_extension, is_lockfile};

/// What the JSONC parser accepts beyond strict JSON, as jsonc.org defines
/// JSONC: `//` and `/* */` comments, and trailing commas, which the
/// specification lets a parser accept and which Prettier writes into every
/// multi-line `.jsonc` file. Everything else the parser can be asked to take
/// is refused: unquoted keys, single quotes, missing commas, hexadecimal
/// numbers, a leading `+`, a bare decimal point, `NaN` and `Infinity`, and the
/// escapes of JSON5. Prettier's own parser would take all of them, so it
/// cannot be the judge.
pub const JSONC: jsonc_parser::ParseOptions = jsonc_parser::ParseOptions {
    allow_comments: true,
    allow_trailing_commas: true,
    allow_loose_object_property_names: false,
    allow_missing_commas: false,
    allow_single_quoted_strings: false,
    allow_hexadecimal_numbers: false,
    allow_unary_plus_numbers: false,
    allow_bare_decimal_point_numbers: false,
    allow_non_finite_numbers: false,
    allow_extended_string_escapes: false,
};

/// Runs the check; whether it found nothing.
pub(super) fn run(session: &mut Session<'_, '_, '_>) -> bool {
    // A missing program is named and fails the check, and the parsing, which
    // needs none, still runs, so that one run reports every finding.
    let prettier = session.tool("prettier", false);
    let files = session.files.clone();
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    // Each part runs even when one before it failed, so that one run reports
    // every broken and every unformatted file at once.
    let parsed = parsed(session, &files);
    let formatted = prettier.is_some_and(|prettier| formatted(session, &prettier, &files));
    parsed && formatted
}

/// Every file parses: strict JSON, or JSONC for a `.jsonc` file. A finding
/// names the file, and the line and column where the parser stopped.
fn parsed(session: &mut Session<'_, '_, '_>, files: &[&str]) -> bool {
    let mut passed = true;
    let mut commented = 0;
    for file in files {
        let jsonc = has_extension(file, "jsonc");
        commented += usize::from(jsonc);
        let Some(bytes) = session.contents(file) else {
            passed = false;
            continue;
        };
        let finding = if jsonc {
            commented_finding(&bytes)
        } else {
            strict_finding(&bytes)
        };
        if let Some(finding) = finding {
            session.finding(file, &finding);
            passed = false;
        }
    }
    session.log.line(&format!(
        "{} file(s) parsed, {commented} of them as JSONC.",
        files.len()
    ));
    passed
}

/// Strict JSON: no comments, no trailing commas, and exactly one value. An
/// empty file holds none, and two values one after the other are no JSON
/// document either. [`None`] when the file is that.
#[must_use]
pub fn strict_finding(bytes: &[u8]) -> Option<super::Finding> {
    let bytes = bytes.strip_prefix(BOM.as_bytes()).unwrap_or(bytes);
    let mut values = 0;
    for value in serde_json::Deserializer::from_slice(bytes).into_iter::<IgnoredAny>() {
        if let Err(error) = value {
            // The error says where on its own; the finding says it apart.
            let message = error.to_string();
            let place = format!(" at line {} column {}", error.line(), error.column());
            let message = message.strip_suffix(&place).unwrap_or(&message);
            return Some(super::Finding {
                position: Some((error.line(), error.column())),
                message: format!("not valid JSON: {message}"),
            });
        }
        values += 1;
    }
    (values != 1).then(|| super::Finding {
        position: None,
        message: format!(
            "not valid JSON: the file holds {values} values, a JSON document is exactly one"
        ),
    })
}

/// JSONC, as [`JSONC`] reads it, holding exactly one value. [`None`] when the
/// file is that.
#[must_use]
pub fn commented_finding(bytes: &[u8]) -> Option<super::Finding> {
    let text = match super::utf8(bytes, "JSONC") {
        Ok(text) => text,
        Err(finding) => return Some(finding),
    };
    match jsonc_parser::parse_to_value(text, &JSONC) {
        Ok(Some(_)) => None,
        Ok(None) => Some(super::Finding {
            position: None,
            message: "not valid JSONC: the file holds no value, a JSONC document is exactly one"
                .to_owned(),
        }),
        Err(error) => Some(super::Finding {
            position: Some((error.line_display(), error.column_display())),
            message: format!("not valid JSONC: {}", error.kind()),
        }),
    }
}

/// Formatted the way Prettier writes it, every file but a lockfile.
fn formatted(session: &mut Session<'_, '_, '_>, prettier: &Tool, files: &[&str]) -> bool {
    let formatted: Vec<&str> = files
        .iter()
        .copied()
        .filter(|file| !is_lockfile(file))
        .collect();
    if formatted.is_empty() {
        session
            .log
            .line("No file to format-check: lockfiles are parsed, not formatted.");
        return true;
    }
    let Some(temp) = session.folder(|_| vec![("nothing-ignored".to_owned(), Some(Vec::new()))])
    else {
        return false;
    };
    session.prettier(prettier, &temp.join("nothing-ignored"), &formatted)
}
