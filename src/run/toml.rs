//! The TOML check: every `.toml` file and `Pipfile` and every TOML lockfile
//! passes `taplo lint`, against a schema where one is assigned, and every
//! `.toml` file and `Pipfile` is formatted the way Taplo writes it.
//!
//! Every file is checked on its own, so that one run reports every
//! unformatted and every invalid file, and a finding carries Taplo's own
//! diagnostic. Taplo reads every file from standard input: given a path,
//! Taplo 0.10.0 silently skips a file outside its default `include` of
//! `**/*.toml` (every lockfile and every `Pipfile`) and one the repository's
//! `taplo.toml` leaves out, and reports success for both. So a repository's
//! `include` and `exclude` take nothing out of this check.
//!
//! Formatting: `.toml` files and every `Pipfile` are formatted the way Taplo
//! writes them, with the repository's `taplo.toml`, which Taplo finds at the
//! repository root; `--stdin-filepath` keeps its per-path rules working.
//! Since `taplo format` skips a `Pipfile` given as a path, the finding on one
//! names the standard-input form. A lockfile is not format-checked.
//!
//! Lint: every file parses and holds no conflicting keys. It runs without the
//! repository's configuration, so that no `[schema] enabled = false` there can
//! switch off the validation. Schemas are assigned here by file name, not
//! through Taplo's default catalogs, which Taplo 0.10.0 cannot read. A file
//! that names its own schema with a `#:schema` directive keeps it, as the
//! assignment would outrank it. Taplo honours the directive only in the
//! comments before the first key or table, so only those are searched. As
//! the file comes from standard input, only an absolute URL works there.
//!
//! One file is the exception: `.nebensound/repo.toml` is always checked
//! against the schema of the declaration compiled into this tool, directive
//! or not. That is the schema of the very version of the pipeline whose
//! phase 0 reads the file; a directive points an editor at the copy on
//! `main`, which can be ahead of the version a repository pins.
//!
//! The schemas are compiled in: copies of those `SchemaStore` publishes, from
//! one commit of it (see `schemas/README.md`), and the one of the
//! declaration. Validating against them makes no request to a server of
//! anyone else. Their references to each
//! other are relative; each copy gets its own `file://` location as `$id`, so
//! that they resolve to the files beside it.

use std::ffi::OsString;
use std::path::Path;

use super::{Entry, Session, Tool, is_lockfile, name_of};

/// The schemas the check assigns, each with its file name: `SchemaStore`'s for
/// `Cargo.toml` and the ones it references, and the declaration's.
pub const SCHEMAS: &[(&str, &str)] = &[
    (
        "cargo-lints-cargo.json",
        include_str!("../../schemas/cargo-lints-cargo.json"),
    ),
    (
        "cargo-lints-clippy.json",
        include_str!("../../schemas/cargo-lints-clippy.json"),
    ),
    (
        "cargo-lints-rust.json",
        include_str!("../../schemas/cargo-lints-rust.json"),
    ),
    (
        "cargo-lints-rustdoc.json",
        include_str!("../../schemas/cargo-lints-rustdoc.json"),
    ),
    ("cargo.json", include_str!("../../schemas/cargo.json")),
    (
        "nebensound-declaration.json",
        include_str!("../../schemas/nebensound-declaration.json"),
    ),
    ("quikrun.json", include_str!("../../schemas/quikrun.json")),
];

/// The file the declaration of a repository lives in.
const DECLARATION: &str = ".nebensound/repo.toml";

/// Runs the check; whether it found nothing.
pub(super) fn run(session: &mut Session<'_, '_, '_>) -> bool {
    let Some(taplo) = session.tool("taplo", false) else {
        return false;
    };
    // The folder's own path goes into every `$id`.
    let Some(temp) = session.folder(|temp| {
        let schemas = temp.join("schemas");
        let mut entries: Vec<Entry> =
            vec![("schemas".to_owned(), None), ("cache".to_owned(), None)];
        entries.extend(SCHEMAS.iter().map(|(name, schema)| {
            let location = url(&schemas.join(name));
            (
                format!("schemas/{name}"),
                Some(with_id(schema, &location).into_bytes()),
            )
        }));
        entries
    }) else {
        return false;
    };
    let schemas = temp.join("schemas");
    let cache = temp.join("cache");
    let files = session.files.clone();
    let mut passed = true;
    for file in &files {
        if !is_lockfile(file) {
            let args = [
                "format".into(),
                "--check".into(),
                "--colors".into(),
                "never".into(),
                "--stdin-filepath".into(),
                file.into(),
                "-".into(),
            ];
            let ran = session.execute(&taplo, &args, Some(file), "Taplo");
            if !ran.success {
                let message = format!(
                    "does not parse, or is not formatted the way Taplo writes it; {}",
                    format_hint(file)
                );
                report(session, file, &ran.text, &message);
                passed = false;
            }
        }
        passed &= lint(session, &taplo, file, &schemas, &cache);
    }
    passed
}

/// How the formatting of a file is fixed. `taplo format` given a `Pipfile` as
/// a path skips it, as it lies outside Taplo's default `include`, so the hint
/// on one names the standard-input form, which prints the fixed file.
fn format_hint(file: &str) -> String {
    if name_of(file) == "Pipfile" {
        format!("'taplo format --stdin-filepath {file} - < {file}' prints the fixed file")
    } else {
        "'taplo format' fixes the latter".to_owned()
    }
}

/// `taplo lint` over one file, against the schema assigned to it.
fn lint(
    session: &mut Session<'_, '_, '_>,
    taplo: &Tool,
    file: &str,
    schemas: &Path,
    cache: &Path,
) -> bool {
    let mut args: Vec<OsString> = vec!["lint".into(), "--no-auto-config".into()];
    let schema = if file == DECLARATION {
        Some("nebensound-declaration.json")
    } else if name_of(file) == "Cargo.toml" && !directive(&read(session, file)) {
        Some("cargo.json")
    } else {
        None
    };
    if let Some(schema) = schema {
        args.push("--schema".into());
        args.push(url(&schemas.join(schema)).into());
    }
    args.extend([
        "--cache-path".into(),
        cache.into(),
        "--colors".into(),
        "never".into(),
        "-".into(),
    ]);
    let ran = session.execute(taplo, &args, Some(file), "Taplo");
    if !ran.success {
        report(session, file, &ran.text, "fails taplo lint");
    }
    ran.success
}

/// A file of the repository as text, or nothing when it cannot be read: then
/// Taplo, which reads it next, says why.
fn read(session: &Session<'_, '_, '_>, file: &str) -> String {
    std::fs::read(session.context.root.join(file))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// Whether the comments before the first key or table hold a `#:schema`
/// directive, the only place Taplo honours one.
#[must_use]
pub fn directive(text: &str) -> bool {
    for line in text.split('\n') {
        let line = line.trim_start_matches(is_space);
        if line
            .strip_prefix("#:schema")
            .and_then(|rest| rest.chars().next())
            .is_some_and(is_space)
        {
            return true;
        }
        if !(line.is_empty() || line.starts_with('#')) {
            return false;
        }
    }
    false
}

/// White space as the C locale has it.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

/// A schema with its own location as `$id` in front of its keys.
#[must_use]
pub fn with_id(schema: &str, location: &str) -> String {
    let id = serde_json::Value::String(location.to_owned());
    match schema.split_once('{') {
        Some((before, after)) => format!("{before}{{\"$id\": {id},{after}"),
        None => schema.to_owned(),
    }
}

/// The `file://` address of a file.
fn url(path: &Path) -> String {
    format!("file://{}", path.display())
}

/// Writes what Taplo printed and the finding on the file: the message, with
/// the first line of every `error:` block and every `ERROR` or `WARN` log
/// line (twelve at most), at the first place Taplo points to. Backtraces and
/// the like stay in the output.
fn report(session: &mut Session<'_, '_, '_>, file: &str, text: &str, message: &str) {
    session.log.lines(text);
    let position = text.lines().find_map(place);
    let detail: Vec<&str> = text
        .lines()
        .filter(|line| diagnostic(line))
        .map(|line| line.trim_start_matches(' '))
        .take(12)
        .collect();
    let message = if detail.is_empty() {
        message.to_owned()
    } else {
        format!("{message}\n{}", detail.join("\n"))
    };
    session.log.error(
        Some(file),
        position.map(|(line, column)| (line, Some(column))),
        &message,
    );
}

/// The line and column of a line such as `┌─ stdin:3:11`, Taplo's pointer to
/// a place.
fn place(line: &str) -> Option<(&str, &str)> {
    let (_, pointer) = line.split_once("┌─ ")?;
    let (rest, column) = pointer.rsplit_once(':')?;
    let (_, row) = rest.rsplit_once(':')?;
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    (digits(row) && digits(column)).then_some((row, column))
}

/// Whether a line opens one of Taplo's diagnostics (`error:`, `warning:`,
/// each with a code in brackets or none) or is one of its `ERROR` or `WARN`
/// log lines.
fn diagnostic(line: &str) -> bool {
    let opens = ["error", "warning"].iter().any(|kind| {
        line.strip_prefix(kind).is_some_and(|rest| {
            rest.starts_with(':')
                || rest
                    .strip_prefix('[')
                    .and_then(|code| code.split_once(']'))
                    .is_some_and(|(_, after)| after.starts_with(':'))
        })
    });
    let logged = ["ERROR ", "WARN "]
        .iter()
        .any(|level| line.trim_start_matches(' ').starts_with(level));
    opens || logged
}
