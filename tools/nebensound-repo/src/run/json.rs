//! The JSON check: every `.json`, `.jsonc` and `.geojson` file parses and is
//! formatted the way Prettier writes it; every JSON lockfile parses.

use std::ffi::OsString;

use super::{Session, Tool, has_extension, is_lockfile};

/// Biome's configuration for parsing JSONC and nothing else: formatter,
/// assists and every lint rule switched off, and no version control read.
pub const BIOME: &str = include_str!("../../run/biome.json");

/// Runs the check; whether it found nothing.
pub(super) fn run(session: &mut Session<'_, '_>) -> bool {
    let jq = session.tool("jq", false);
    let biome = session.tool("biome", false);
    let prettier = session.tool("prettier", false);
    let (Some(jq), Some(biome), Some(prettier)) = (jq, biome, prettier) else {
        return false;
    };
    let Some(temp) = session.folder(|_| {
        vec![
            ("nothing-ignored".to_owned(), Some(Vec::new())),
            ("biome".to_owned(), None),
            (
                "biome/biome.json".to_owned(),
                Some(BIOME.as_bytes().to_vec()),
            ),
        ]
    }) else {
        return false;
    };
    let files = session.files.clone();
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    // Each part runs even when one before it failed, so that one run reports
    // every broken and every unformatted file at once.
    let parsed = strict(session, &jq, &files);
    let commented = commented(session, &biome, &temp.join("biome"), &files);
    let formatted = formatted(session, &prettier, &temp.join("nothing-ignored"), &files);
    parsed && commented && formatted
}

/// Strict JSON: no comments, no trailing commas, and exactly one document.
/// Prettier would accept the first two and so cannot be the judge of
/// validity. `jq empty` would also pass an empty file and a file holding two
/// values; slurped, the file is the list of its values, and that list has to
/// hold one. Every file but a `.jsonc` one; what jq writes to its standard
/// error is the message.
fn strict(session: &mut Session<'_, '_>, jq: &Tool, files: &[&str]) -> bool {
    let mut passed = true;
    for file in files.iter().filter(|file| !has_extension(file, "jsonc")) {
        let one = match session
            .command(&jq.executable, &jq.search_path)
            .args(["--exit-status", "--slurp", "length == 1", file])
            .output()
        {
            Ok(output) => output,
            Err(error) => {
                session
                    .log
                    .error(None, None, &format!("jq cannot be started: {error}"));
                return false;
            }
        };
        let message = match one.status.code() {
            Some(0) => continue,
            Some(1) => {
                let count = session
                    .command(&jq.executable, &jq.search_path)
                    .args(["--slurp", "length", file])
                    .output()
                    .map(|output| output.stdout)
                    .unwrap_or_default();
                format!(
                    "not valid JSON: the file holds {} values, a JSON document is exactly one",
                    String::from_utf8_lossy(&count).trim_end()
                )
            }
            _ => format!(
                "not valid JSON: {}",
                String::from_utf8_lossy(&one.stderr).trim_end_matches('\n')
            ),
        };
        session.log.error(Some(file), None, &message);
        passed = false;
    }
    passed
}

/// Strict JSONC as jsonc.org defines it: JSON plus `//` and `/* */`
/// comments, and trailing commas, which the specification lets a parser
/// accept and which Prettier writes into every multi-line `.jsonc` file.
/// Prettier's own parser would also take unquoted keys, single quotes,
/// hexadecimal numbers and NaN. Biome is used as a parser only, with the
/// configuration in `config`; a repository's own Biome configuration is not
/// read. It names files by their absolute path, and a finding needs them from
/// the repository root.
fn commented(
    session: &mut Session<'_, '_>,
    biome: &Tool,
    config: &std::path::Path,
    files: &[&str],
) -> bool {
    let commented: Vec<&str> = files
        .iter()
        .copied()
        .filter(|file| has_extension(file, "jsonc"))
        .collect();
    if commented.is_empty() {
        session.log.line("No .jsonc file to parse.");
        return true;
    }
    let mut flag = OsString::from("--config-path=");
    flag.push(config);
    let args = ["lint".into(), flag, "--reporter=github".into()];
    let ran = session.over(biome, &args, &commented, "Biome");
    let root = format!("{}/", session.context.root.display());
    for line in ran.text.lines() {
        session.log.line(&line.replace(&root, ""));
    }
    session
        .log
        .line(&format!("{} .jsonc file(s) parsed.", commented.len()));
    ran.success
}

/// Formatted the way Prettier writes it, every file but a lockfile.
fn formatted(
    session: &mut Session<'_, '_>,
    prettier: &Tool,
    ignore: &std::path::Path,
    files: &[&str],
) -> bool {
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
    session.prettier(prettier, ignore, &formatted)
}
