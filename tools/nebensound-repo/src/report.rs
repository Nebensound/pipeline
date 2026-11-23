//! The result as text.
//!
//! The same analysis the interface shows, only flat — so that a log says what
//! the screen says. Built line by line: a line is a value, and nobody has to
//! handle a write error a `String` cannot have.

use crate::annotate::{Coverage, Node};
use crate::finding::{Finding, Position, UnitName};
use crate::names::CheckName;
use crate::path::RelativePath;
use crate::resolve::{MinimumVersionSource, Origin, PipelineSource, Unit};
use crate::{Analysis, Central, Publish, Severity};

/// The column the annotations start in.
const COLUMN: usize = 56;

/// A piece of text as it can be written into a log.
///
/// Git allows a newline or an escape sequence in a tracked file name, and a
/// reason is whatever someone wrote into the declaration. Written out as they
/// stand, either could forge a line of this report or set the terminal to
/// something the reader never asked for. The path keeps its bytes for the
/// analysis; only what is shown is escaped.
///
/// The runner of GitHub Actions also reads an older form of a workflow
/// command, `##[name]data`, and it looks for that form anywhere in a line, not
/// only at its start. A file named `a##[warning]x.json` would start a warning
/// wherever its name is printed, with no line break in it.
///
/// Text that holds a control character (Unicode category `Cc`) or `##[` has
/// every control character written as its escape: a tab as `\t`, a line break
/// as `\n`, a carriage return as `\r`, any other as `\u{<hex>}`; a backslash
/// and a quote get a backslash in front; and a `[` right behind `##` is
/// written as `\u{5b}`, its escape, so that `##[warning]` reads
/// `##\u{5b}warning]`. Every other character stays as it is, so a decomposed
/// accent reads as the accent. Text with neither stays as it is. The shell
/// function `shown` of the action `tool` and the copies in the scripts the
/// checks run write the same.
#[must_use]
pub fn shown(text: &str) -> String {
    if !text.chars().any(char::is_control) && !text.contains(OLDER_FORM) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\\' | '\'' | '"' => {
                out.push('\\');
                out.push(character);
            }
            _ if character.is_control() => out.extend(character.escape_unicode()),
            _ => out.push(character),
        }
    }
    // No escape above writes a `#` or a `[` or replaces one, so `##[` stands
    // in `out` exactly where it stands in the text.
    out.replace(OLDER_FORM, "##\\u{5b}")
}

/// How the older form of a workflow command starts: see [`shown`].
const OLDER_FORM: &str = "##[";

/// What a file can expect: checked, excused, or open.
fn mark(coverage: &Coverage) -> String {
    match coverage {
        Coverage::Covered(check) => format!("\u{2713} {check}"),
        Coverage::Uncovered(check) => format!("\u{d7} {check} \u{2014} nobody runs it here"),
        Coverage::Unknown => "? no check in the register covers this kind of file".to_owned(),
        Coverage::Excluded { pattern, reason } => format!("\u{2013} {pattern}: {reason}"),
    }
}

fn names(checks: &[CheckName]) -> String {
    if checks.is_empty() {
        return "\u{2014}".to_owned();
    }
    checks
        .iter()
        .map(CheckName::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn column(indent: usize, text: &str) -> String {
    let padded = format!("{:indent$}{text}", "", indent = indent);
    format!("{padded:COLUMN$}")
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("{count} {one}")
    } else {
        format!("{count} {many}")
    }
}

/// Where a pipeline came from, in a word.
fn source(source: &PipelineSource) -> String {
    match source {
        PipelineSource::Default => "nothing declared".to_owned(),
        PipelineSource::Preset(name) => format!("preset {name}"),
        PipelineSource::RequestedDefault => "the default in [repo.component]".to_owned(),
        PipelineSource::WrittenOut => "written out".to_owned(),
        PipelineSource::Inherited => "inherited".to_owned(),
    }
}

/// A list of names, separated by commas.
fn listed<T: ToString>(names: &[T]) -> String {
    names
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The version below which the unit's version never falls, and where it came
/// from.
fn minimum_version(unit: &Unit) -> String {
    let Some(floor) = &unit.minimum_version else {
        return "none".to_owned();
    };
    match &floor.source {
        MinimumVersionSource::Written => format!("{} (written)", floor.version),
        MinimumVersionSource::Inherited { by } => {
            format!("{} (inherited from {by})", floor.version)
        }
    }
}

/// The commit types that count like `feat` beside `feat` itself, as the
/// declaration at hand lists them.
fn feat_types(analysis: &Analysis) -> String {
    let types = analysis.feat_types.types();
    if types.is_empty() {
        return "none".to_owned();
    }
    listed(types)
}

fn unit_lines(unit: &Unit) -> Vec<String> {
    let mut lines = vec![format!(
        "{}{}",
        column(2, &format!("{}", unit.name)),
        shown(&unit.path.to_string())
    )];
    if let Origin::Generated { by } = &unit.origin {
        lines.push(format!("{}generated by {by}", column(6, "Origin")));
    }
    lines.push(format!(
        "{}{}",
        column(6, "Pipeline"),
        source(&unit.pipeline.source)
    ));
    lines.push(format!(
        "{}{}",
        column(6, "Checks"),
        names(&unit.pipeline.checks)
    ));
    if let Some(build) = &unit.pipeline.build {
        lines.push(format!("{}{build}", column(6, "Build")));
    }
    if let Publish::To(targets) = &unit.pipeline.publish
        && !targets.is_empty()
    {
        lines.push(format!("{}{}", column(6, "Publish"), listed(targets)));
    }
    if !unit.pipeline.dependabot.is_empty() {
        lines.push(format!(
            "{}{}",
            column(6, "Dependabot"),
            listed(&unit.pipeline.dependabot)
        ));
    }
    // A version comes from the history, never from a tag.
    let version = if unit.carries_version {
        "history"
    } else {
        "none"
    };
    lines.push(format!(
        "{}{version} (versioning = \"{}\")",
        column(6, "Version"),
        unit.versioning
    ));
    lines.push(format!(
        "{}{}",
        column(6, "Minimum version"),
        minimum_version(unit)
    ));
    lines
}

/// The resolved configuration: the base set, and every unit with what holds
/// for it and where that came from.
///
/// A value arrives from three directions, and without this output nobody sees
/// which one it took.
#[must_use]
pub fn configuration(analysis: &Analysis, central: &Central) -> String {
    let mut lines = vec!["Resolved configuration".to_owned(), String::new()];
    lines.push(format!(
        "{}{}",
        column(2, "Base checks"),
        names(central.base_checks())
    ));
    let watched = central
        .base_dependabot()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    lines.push(format!("{}{watched}", column(2, "Base watch")));
    // The list holds for the whole repository: every unit's version is raised
    // by the same reading of a commit.
    lines.push(format!(
        "{}{}",
        column(2, "Feat types"),
        feat_types(analysis)
    ));
    for unit in &analysis.units {
        lines.push(String::new());
        lines.extend(unit_lines(unit));
    }
    lines.join("\n")
}

fn node_lines(node: &Node, depth: usize, inherited: &[UnitName], lines: &mut Vec<String>) {
    let indent = 2 + depth * 2;
    // A node always has a last segment: the root is not one.
    let name = shown(node.path.file_name().unwrap_or_default());

    // A unit that begins here is named here, and not again at every child
    // below it.
    let opened: Vec<String> = node
        .units
        .iter()
        .filter(|unit| !inherited.contains(unit))
        .map(ToString::to_string)
        .collect();

    let opened = if opened.is_empty() {
        String::new()
    } else {
        format!("[{}]", opened.join(", "))
    };
    let annotation = match node.coverage() {
        Some(coverage) => format!("{} {opened}", shown(&mark(coverage))),
        None => opened,
    };

    let label = if node.coverage().is_some() {
        name
    } else {
        format!("{name}/")
    };
    let line = format!("{}{annotation}", column(indent, &label));
    lines.push(line.trim_end().to_owned());

    for finding in &node.findings {
        lines.push(format!(
            "{:indent$}{}",
            "",
            shown(&finding.to_string()),
            indent = indent + 4
        ));
    }
    for child in node.children() {
        node_lines(child, depth + 1, &node.units, lines);
    }
}

/// The whole annotated tree.
///
/// The repository stands on a line of its own above it. In the result it is not
/// a node — a request with one file answers with one node, not two — but the
/// units that hold everywhere are worth writing down once instead of at every
/// line below.
#[must_use]
pub fn tree(analysis: &Analysis) -> String {
    let everywhere: Vec<UnitName> = analysis
        .units
        .iter()
        .filter(|unit| unit.path.is_root())
        .map(|unit| unit.name.clone())
        .collect();
    let names = everywhere
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    let mut lines = vec![
        "Tree".to_owned(),
        String::new(),
        format!("{}[{names}]", column(2, ".")),
    ];
    for node in analysis.tree.nodes() {
        node_lines(node, 1, &everywhere, &mut lines);
    }
    lines.join("\n")
}

/// Every finding, the loose ones first.
#[must_use]
pub fn findings(analysis: &Analysis) -> String {
    let found = analysis.findings();
    if found.is_empty() {
        return "Findings\n\n  none".to_owned();
    }
    let mut lines = vec!["Findings".to_owned(), String::new()];
    for finding in found {
        lines.push(format!("  {}", shown(&finding.to_string())));
    }
    lines.join("\n")
}

/// Every finding, the loose ones first, each as a workflow command of GitHub
/// Actions: an annotation of the run, `::error` or `::warning`, titled with its
/// code. It names the file where there is one: the file in the tree the
/// finding hangs at, with its line and column when the finding points at a
/// place in it. A path the repository does not hold, such as a missing file,
/// stays in the message, since an annotation on it would point nowhere.
#[must_use]
pub fn annotations(analysis: &Analysis) -> String {
    let found = analysis.findings();
    if found.is_empty() {
        return "Findings\n\n  none".to_owned();
    }
    let mut lines = vec!["Findings".to_owned(), String::new()];
    for finding in found {
        let file = finding
            .path
            .as_ref()
            .filter(|path| {
                analysis
                    .tree
                    .at(path)
                    .is_some_and(|node| node.coverage().is_some())
            })
            .map(|path| (path, finding.position));
        lines.push(annotation(finding, file));
    }
    lines.join("\n")
}

/// One finding as a workflow command, on the file and the place given.
#[must_use]
pub fn annotation(finding: &Finding, file: Option<(&RelativePath, Option<Position>)>) -> String {
    let mut properties = Vec::new();
    if let Some((path, position)) = file {
        properties.push(format!("file={}", property(&path.to_string())));
        if let Some(Position { line, column }) = position {
            properties.push(format!("line={line},col={column}"));
        }
    }
    let code = finding.rule.code();
    properties.push(format!("title={}", property(code)));
    // The severity is the command and the code the title. The log shows only
    // the message, though, so the message carries the code as well: the key a
    // reader looks the finding up by.
    let place = finding.place();
    let message = if place.is_empty() {
        format!("[{code}] {}", finding.message)
    } else {
        format!("[{code}] {}: {}", place.trim_start(), finding.message)
    };
    format!(
        "::{} {}::{}",
        finding.severity,
        properties.join(","),
        data(&message)
    )
}

/// The message of a workflow command. `shown` already writes every control
/// character, line breaks included, as an escape, so no line break can end the
/// command; only `%`, which starts an escape, is left to escape the way the
/// runner reads it.
fn data(text: &str) -> String {
    shown(text).replace('%', "%25")
}

/// A property of a workflow command, where `:` and `,` end the value as well.
fn property(text: &str) -> String {
    data(text).replace(':', "%3A").replace(',', "%2C")
}

/// The summary line with the counts.
#[must_use]
pub fn summary(analysis: &Analysis) -> String {
    let files = analysis.tree.files().len();
    let gaps = analysis.tree.gaps().len();
    let found = analysis.findings();
    let errors = found
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .count();
    let warnings = found.len() - errors;
    format!(
        "{}, {}, {}, {}",
        plural(files, "file", "files"),
        plural(gaps, "gap", "gaps"),
        plural(errors, "error", "errors"),
        plural(warnings, "warning", "warnings")
    )
}

/// What holds for one path, and why. [`None`] when the tree does not hold it.
#[must_use]
pub fn explain(analysis: &Analysis, path: &RelativePath) -> Option<String> {
    let node = analysis.tree.at(path)?;
    let units = node
        .units
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let mut lines = vec![shown(&node.path.to_string()), String::new()];
    // The audience is the repository's, so it holds for every path.
    lines.push(format!(
        "{}{}",
        column(2, "Audience"),
        analysis
            .audience
            .map_or("none declared", crate::Audience::as_str)
    ));
    lines.push(format!("{}{units}", column(2, "Units")));
    // Every unit containing the path has a floor of its own, so each says
    // which, and where it came from.
    let containing: Vec<&Unit> = analysis
        .units
        .iter()
        .filter(|unit| node.units.contains(&unit.name))
        .collect();
    for unit in &containing {
        lines.push(format!(
            "{}{}: {}",
            column(2, "Minimum version"),
            unit.name,
            minimum_version(unit)
        ));
    }
    lines.push(format!(
        "{}{}",
        column(2, "Feat types"),
        feat_types(analysis)
    ));
    lines.push(format!("{}{}", column(2, "Checks"), names(&node.checks)));
    match node.coverage() {
        Some(coverage) => lines.push(format!(
            "{}{}",
            column(2, "Coverage"),
            shown(&mark(coverage))
        )),
        None => lines.push(format!(
            "{}{}",
            column(2, "Holds"),
            plural(node.children().len(), "entry", "entries")
        )),
    }
    if node.findings.is_empty() {
        lines.push(format!("{}none", column(2, "Findings")));
    } else {
        for finding in &node.findings {
            lines.push(format!(
                "{}{}",
                column(2, "Finding"),
                shown(&finding.to_string())
            ));
        }
    }
    Some(lines.join("\n"))
}
