//! The register as a page to read: `docs/register.md`.
//!
//! The page is printed from the register and the defaults compiled into the
//! tool, so it cannot say something they do not. A test compares it with the
//! committed copy and fails on a difference: a change to the register that
//! leaves the page behind does not pass.
//!
//! It is printed the way Prettier writes Markdown, since `check-markdown`
//! judges the committed copy with Prettier: tables are aligned, every column
//! as wide as its widest cell and at least three characters.

use std::fmt::Display;

use nebensound_declaration::programs::{Download, Platform};
use nebensound_declaration::{Central, Publish, Register};

/// What stands above the tables: what the page is and where it comes from.
const INTRODUCTION: &str = "\
# Register

The vocabulary of the declaration: every name a `pipeline` may use, and what the
pipeline adds to every repository without being asked. The fields that use these
names are described in [the reference of the declaration](declaration.md).

This page is generated. `nebensound register` prints it from the register and
the defaults compiled into the tool, and a test of the tool fails when the
committed page differs. To change it, change `register.toml` or `defaults.toml`
beside the library in `tools/nebensound/crates/nebensound-declaration/`, then
write the page again from `tools/nebensound/`:

```sh
cargo run --quiet -- register > ../../docs/register.md
```

A name stands here once the format knows it. Whether a step behind it is built
yet is a separate question: [the phases](phases.md) say which phases run today,
and a check whose building block is not written yet fails by name in its unit.";

/// The page, as `docs/register.md` holds it.
#[must_use]
pub fn register(central: &Central) -> String {
    let register = central.register();
    let mut text = [
        INTRODUCTION.to_owned(),
        base_set(central),
        presets(register),
        checks(register),
        programs(register),
        runtimes(register),
        builds(register),
        targets(register),
        version_writers(register),
        ecosystems(register),
        excluded(central),
    ]
    .join("\n\n");
    text.push('\n');
    text
}

fn base_set(central: &Central) -> String {
    section(
        "Base set",
        "What holds in every repository without being declared: these checks are in\n\
         force for every file, and these ecosystems are watched. A pipeline adds to\n\
         them and cannot take anything away.",
        &["What", "Names"],
        &[
            vec!["Checks".to_owned(), codes(central.base_checks())],
            vec!["Dependabot".to_owned(), codes(central.base_dependabot())],
        ],
    )
}

fn presets(register: &Register) -> String {
    let rows: Vec<Vec<String>> = register
        .preset
        .iter()
        .map(|(name, preset)| {
            vec![
                code(name),
                codes(&preset.version),
                codes(&preset.check),
                preset.build.as_ref().map_or_else(none, code),
                publish(&preset.publish),
                codes(&preset.dependabot),
            ]
        })
        .collect();
    section(
        "Presets",
        "A preset is a pipeline with a name: `pipeline = \"<preset>\"` takes all of it.\n\
         Its checks come on top of the base set.",
        &[
            "Preset",
            "Version",
            "Check",
            "Build",
            "Publish",
            "Dependabot",
        ],
        &rows,
    )
}

fn checks(register: &Register) -> String {
    let rows: Vec<Vec<String>> = register
        .check
        .iter()
        .map(|(name, check)| {
            vec![
                code(name),
                codes(&check.extensions),
                codes(&check.filenames),
                codes(&check.programs),
            ]
        })
        .collect();
    section(
        "Checks",
        "A check covers every file with one of its extensions or file names. A file\n\
         that no check covers has to be excluded. The programs are what the check calls\n\
         (see [Programs](#programs)).",
        &["Check", "Extensions", "File names", "Programs"],
        &rows,
    )
}

fn programs(register: &Register) -> String {
    let rows: Vec<Vec<String>> = register
        .program
        .iter()
        .map(|(name, program)| {
            vec![
                code(name),
                code(&program.version),
                program.runtime.as_ref().map_or_else(none, code),
                platforms(&program.download),
            ]
        })
        .collect();
    section(
        "Programs",
        "Every program a check calls, pinned to one version. `nebensound setup` finds or\n\
         installs them (see [the tool](../tools/nebensound/README.md#setting-up-the-programs)).\n\
         A program on a runtime is one package, which the runtime's installer puts in\n\
         place; every other one is a build per platform. Each download is refused unless\n\
         it has the digest the register pins; the addresses and digests stand in\n\
         `register.toml`, each with the release data it was taken from. On a platform\n\
         without a build the program has to be on the computer already.",
        &["Program", "Version", "Runtime", "Builds"],
        &rows,
    )
}

fn runtimes(register: &Register) -> String {
    let rows: Vec<Vec<String>> = register
        .runtime
        .iter()
        .map(|(name, runtime)| {
            vec![
                code(name),
                cell(&runtime.title),
                code(&runtime.version),
                code(&runtime.installer),
                platforms(&runtime.download),
            ]
        })
        .collect();
    section(
        "Runtimes",
        "What a program on a runtime runs on. A runtime on the computer is used as it is,\n\
         whatever its version, as long as it prints one; one that does not is passed\n\
         over. The pinned one is installed only when the runtime is missing and someone\n\
         agrees.",
        &["Runtime", "Name", "Version", "Installs with", "Builds"],
        &rows,
    )
}

fn builds(register: &Register) -> String {
    let rows: Vec<Vec<String>> = register
        .build
        .iter()
        .map(|(name, build)| vec![code(name), code(&build.produces)])
        .collect();
    section(
        "Builds",
        "A build produces one kind of artifact. The kind decides which targets can take\n\
         it.",
        &["Build", "Produces"],
        &rows,
    )
}

fn targets(register: &Register) -> String {
    let rows: Vec<Vec<String>> = register
        .target
        .iter()
        .map(|(name, target)| {
            vec![
                code(name),
                codes(&target.accepts),
                target.requires_check.as_ref().map_or_else(none, code),
                code(&target.release),
            ]
        })
        .collect();
    section(
        "Publish targets",
        "A target accepts some kinds of artifact, and it can demand that a check is in\n\
         force for the unit that publishes there. It also says how a unit that publishes\n\
         there is released when the unit does not say: a unit with several targets is\n\
         released in two stages if any of them is, and a unit without a target in one\n\
         (see [Releases](declaration.md#releases)).",
        &["Target", "Accepts", "Requires check", "Release"],
        &rows,
    )
}

fn version_writers(register: &Register) -> String {
    let rows: Vec<Vec<String>> = register
        .version
        .iter()
        .map(|(name, writer)| vec![code(name), cell(&writer.copy)])
        .collect();
    section(
        "Version writers",
        "A version writer receives a unit's version as a derived copy.",
        &["Writer", "Writes"],
        &rows,
    )
}

fn ecosystems(register: &Register) -> String {
    let rows: Vec<Vec<String>> = register
        .ecosystem
        .iter()
        .map(|(name, ecosystem)| {
            vec![
                code(name),
                code(&ecosystem.package_ecosystem),
                codes(&ecosystem.manifests),
            ]
        })
        .collect();
    section(
        "Ecosystems",
        "The package ecosystems Dependabot can watch, and the file names phase 0\n\
         recognizes a manifest of each by.",
        &["Ecosystem", "Dependabot name", "Manifests"],
        &rows,
    )
}

fn excluded(central: &Central) -> String {
    let rows: Vec<Vec<String>> = central
        .defaults()
        .exclude
        .iter()
        .map(|entry| vec![code(&entry.path), cell(entry.reason.as_str())])
        .collect();
    section(
        "Excluded everywhere",
        "What nobody checks, in any repository. A declaration can pull an entry back in\n\
         with `[[include]]`; it cannot add a pattern of its own.",
        &["Pattern", "Reason"],
        &rows,
    )
}

/// A heading, a sentence and a table.
fn section(heading: &str, sentence: &str, header: &[&str], rows: &[Vec<String>]) -> String {
    format!(
        "## {heading}\n\n{sentence}\n\n{}",
        table(header, rows).join("\n")
    )
}

/// A table the way Prettier aligns it.
fn table(header: &[&str], rows: &[Vec<String>]) -> Vec<String> {
    let header: Vec<String> = header.iter().map(|title| (*title).to_owned()).collect();
    let widths: Vec<usize> = (0..header.len())
        .map(|column| {
            std::iter::once(&header)
                .chain(rows)
                .map(|row| row[column].chars().count())
                .fold(3, usize::max)
        })
        .collect();
    let line = |cells: &[String]| {
        let padded: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(text, width)| format!("{text:width$}"))
            .collect();
        format!("| {} |", padded.join(" | "))
    };
    let rule: Vec<String> = widths.iter().map(|width| "-".repeat(*width)).collect();
    let mut lines = vec![line(&header), line(&rule)];
    lines.extend(rows.iter().map(|row| line(row)));
    lines
}

/// Text as a table cell holds it: a `|` would end the cell.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

/// A name as code.
fn code<T: Display + ?Sized>(name: &T) -> String {
    cell(&format!("`{name}`"))
}

/// A list of names as code, or that there is none.
fn codes<T: Display>(names: &[T]) -> String {
    if names.is_empty() {
        return none();
    }
    names.iter().map(code).collect::<Vec<_>>().join(", ")
}

/// An empty cell, said out loud.
fn none() -> String {
    "none".to_owned()
}

/// The platforms there is a download for, or that one package serves all.
fn platforms(downloads: &std::collections::BTreeMap<Platform, Download>) -> String {
    if downloads.contains_key(&Platform::Any) {
        return "one package for every platform".to_owned();
    }
    codes(&downloads.keys().collect::<Vec<_>>())
}

/// Where a preset publishes.
fn publish(publish: &Publish) -> String {
    match publish {
        Publish::Nothing => "nothing".to_owned(),
        Publish::To(targets) => codes(targets),
    }
}
