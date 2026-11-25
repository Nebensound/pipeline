//! Markers of open tasks: every `TODO` and `FIXME` in a repository names the
//! issue that tracks it, or `check` fails on it.
//!
//! Phase 0 reads every file Git tracks that is text and that no `[[exclude]]`
//! entry takes out, each once, and judges every marker in it ([`findings`]).
//! It starts no program and uses no network, so whether the issue a marker
//! names exists, or is still open, is not asked here.
//!
//! - **A marker** is the word `TODO` or `FIXME`, in capitals, directly followed
//!   by `(` or `:`, where the start of the line or a character that is no
//!   letter, digit or `_` stands before the word. The word in a sentence, the
//!   word with a space or a dot after it, `todo` in small letters (Rust's
//!   `todo!` among them) and the word inside a longer one are no marker.
//! - **A reference** stands alone in the parentheses directly after the word:
//!   `#` and the number of an issue, with its repository as `owner/repo` in
//!   front when the issue is in another one ([`is_reference`]). A marker with
//!   one stays. A link to an issue is none: one form is read the same way in
//!   every file and in every repository, and a link could name any host.
//! - **The marker `init` writes**, the word `TODO` with `init` in the
//!   parentheses, is always the error `unresolved-init-marker`. Every other
//!   marker without a reference is the error `unreferenced-marker`.
//! - **An allowance** is [`ALLOWANCE`] followed by white space and its reason,
//!   inside whatever comment the file's language has. It lets every marker on
//!   its own line and on the line directly below stay; a blank line or another
//!   line between them breaks the tie. It counts where it stands as a word,
//!   like a marker, and is followed by white space or the end of the line.
//!   Its reason is the rest of the line, and it has to hold a letter or a
//!   digit, so that the mark closing a comment, such as `-->` or `*/`, is no
//!   reason. An allowance without a reason is the error
//!   `allowance-without-reason`, and one with no marker on its line or the
//!   line below the error `pointless-allowance`, so that dead allowances do
//!   not pile up.
//! - **A file is text** when no NUL byte stands among its first
//!   [`BINARY_PREFIX`] bytes, as Git decides it, and it is valid UTF-8
//!   ([`text`]). Any other file is passed over, as is one that cannot be read:
//!   the findings about that file say why.

use crate::annotate::Coverage;
use crate::finding::{Finding, Position, Rule};
use crate::path::RelativePath;
use crate::report::shown;

/// The words that mark an open task.
pub const WORDS: [&str; 2] = ["TODO", "FIXME"];

/// The word of the marker `init` writes.
const INIT_WORD: &str = "TODO";

/// What stands in the parentheses of the marker `init` writes.
const INIT_NAME: &str = "init";

/// The directive that lets a marker stay without an issue, written on the
/// marker's line or on the line directly above it and followed by its reason.
pub const ALLOWANCE: &str = "nebensound:allow(todo)";

/// How many bytes at the start of a file decide whether it is binary: a NUL
/// among them makes it so. Git decides it the same way.
pub const BINARY_PREFIX: usize = 8000;

/// Reads a file of the repository as it stands there, as bytes.
pub type ReadBytes<'a> = dyn Fn(&RelativePath) -> Result<Vec<u8>, String> + 'a;

/// What a marker says about the issue that tracks its task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    /// It names the issue.
    Referenced,
    /// It is the marker `init` writes.
    Init,
    /// It names no issue: its word.
    Unreferenced(&'static str),
}

/// What stands at a place in a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A marker.
    Marker(Marker),
    /// An allowance, and whether it gives a reason.
    Allowance {
        /// Whether the rest of its line holds a letter or a digit.
        reasoned: bool,
    },
}

/// A marker or an allowance, where it starts in its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mark<'a> {
    /// The byte offset in the line.
    offset: usize,
    /// What it is.
    kind: Kind,
    /// The line from the start of the mark on.
    from: &'a str,
}

impl Mark<'_> {
    fn is_marker(&self) -> bool {
        matches!(self.kind, Kind::Marker(_))
    }

    fn is_allowance(&self) -> bool {
        matches!(self.kind, Kind::Allowance { .. })
    }
}

/// The text of a file, or [`None`] when it is binary or not valid UTF-8.
#[must_use]
pub fn text(bytes: &[u8]) -> Option<&str> {
    let prefix = bytes.get(..BINARY_PREFIX).unwrap_or(bytes);
    if prefix.contains(&0) {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

/// Whether this, from the parentheses of a marker, names an issue: `#` and
/// its number, with `owner/repo` in front for an issue in another repository.
/// The number has no leading zero; the owner holds ASCII letters, digits and
/// `-`, the repository ASCII letters, digits, `-`, `_` and `.`.
#[must_use]
pub fn is_reference(inside: &str) -> bool {
    let Some((repository, number)) = inside.split_once('#') else {
        return false;
    };
    let number_reads = number.bytes().all(|byte| byte.is_ascii_digit())
        && number.bytes().next().is_some_and(|first| first != b'0');
    let repository_reads = repository.is_empty()
        || repository.split_once('/').is_some_and(|(owner, name)| {
            !owner.is_empty()
                && !name.is_empty()
                && owner
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        });
    number_reads && repository_reads
}

/// Whether a mark that starts at `offset` in `line` stands as a word: the start
/// of the line or a character that is no letter, digit or `_` before it.
fn starts_word(line: &str, offset: usize) -> bool {
    line.get(..offset)
        .unwrap_or_default()
        .chars()
        .next_back()
        .is_none_or(|before| !(before.is_alphanumeric() || before == '_'))
}

/// What a marker of this word says, given what follows its `(`.
fn judged(word: &'static str, after: &str) -> Marker {
    match after.split_once(')') {
        Some((inside, _)) if word == INIT_WORD && inside == INIT_NAME => Marker::Init,
        Some((inside, _)) if is_reference(inside) => Marker::Referenced,
        _ => Marker::Unreferenced(word),
    }
}

/// Every marker and allowance in one line, in the order they stand.
fn marks(line: &str) -> Vec<Mark<'_>> {
    let mut found = Vec::new();
    for word in WORDS {
        for (offset, _) in line.match_indices(word) {
            if !starts_word(line, offset) {
                continue;
            }
            let after = line.get(offset + word.len()..).unwrap_or_default();
            let marker = if let Some(inside) = after.strip_prefix('(') {
                judged(word, inside)
            } else if after.starts_with(':') {
                Marker::Unreferenced(word)
            } else {
                continue;
            };
            found.push(Mark {
                offset,
                kind: Kind::Marker(marker),
                from: line.get(offset..).unwrap_or_default(),
            });
        }
    }
    for (offset, _) in line.match_indices(ALLOWANCE) {
        let after = line.get(offset + ALLOWANCE.len()..).unwrap_or_default();
        if !starts_word(line, offset) || after.chars().next().is_some_and(|c| !c.is_whitespace()) {
            continue;
        }
        found.push(Mark {
            offset,
            kind: Kind::Allowance {
                reasoned: after.chars().any(char::is_alphanumeric),
            },
            from: line.get(offset..).unwrap_or_default(),
        });
    }
    found.sort_by_key(|mark| mark.offset);
    found
}

/// Every line that holds a mark, by its number counted from 1, in order. A
/// text that holds none of the words searched for is not split into lines.
fn lines(text: &str) -> Vec<(usize, &str, Vec<Mark<'_>>)> {
    if !WORDS.iter().any(|word| text.contains(word)) && !text.contains(ALLOWANCE) {
        return Vec::new();
    }
    text.split('\n')
        .enumerate()
        .filter_map(|(index, line)| {
            let found = marks(line);
            (!found.is_empty()).then_some((index + 1, line, found))
        })
        .collect()
}

/// Where a mark stands: its line, and its column counted in characters.
fn position(number: usize, line: &str, mark: &Mark<'_>) -> Position {
    Position {
        line: number,
        column: line.get(..mark.offset).unwrap_or_default().chars().count() + 1,
    }
}

/// The rest of the line behind a marker `init` wrote, as its report and its
/// finding quote it.
fn after_init(mark: &Mark<'_>) -> String {
    let marker = format!("{INIT_WORD}({INIT_NAME})");
    mark.from
        .get(marker.len()..)
        .unwrap_or_default()
        .trim_start_matches(':')
        .trim()
        .to_owned()
}

/// Every marker `init` writes in a text, allowed or not: where it stands, and
/// what the rest of its line says.
#[must_use]
pub fn init_markers(text: &str) -> Vec<(Position, String)> {
    let mut found = Vec::new();
    for (number, line, marks) in lines(text) {
        for mark in &marks {
            if mark.kind == Kind::Marker(Marker::Init) {
                found.push((position(number, line, mark), after_init(mark)));
            }
        }
    }
    found
}

/// The findings about the markers and allowances in the text of the file at
/// `path`, in the order they stand.
#[must_use]
pub fn judge(path: &RelativePath, text: &str) -> Vec<Finding> {
    let lines = lines(text);
    let mut found = Vec::new();
    let allows = |marks: &[Mark<'_>]| marks.iter().any(Mark::is_allowance);
    let marked = |marks: &[Mark<'_>]| marks.iter().any(Mark::is_marker);
    for (index, (number, line, marks)) in lines.iter().enumerate() {
        let above = index
            .checked_sub(1)
            .and_then(|before| lines.get(before))
            .is_some_and(|(other, _, before)| other + 1 == *number && allows(before));
        let below = lines
            .get(index + 1)
            .is_some_and(|(other, _, after)| *other == number + 1 && marked(after));
        let allowed = above || allows(marks);
        let allows_something = below || marked(marks);
        for mark in marks {
            let at = |finding: Finding| finding.at(path.clone()).on(position(*number, line, mark));
            match mark.kind {
                Kind::Allowance { reasoned } => {
                    if !reasoned {
                        found.push(at(Finding::error(
                            Rule::AllowanceWithoutReason,
                            format!(
                                "{ALLOWANCE} gives no reason why the marker may stay without an \
                                 issue. Write the reason behind it, on the same line"
                            ),
                        )));
                    }
                    if !allows_something {
                        found.push(at(Finding::error(
                            Rule::PointlessAllowance,
                            format!(
                                "{ALLOWANCE} stands on no line with a marker of an open task and \
                                 above none, so it lets nothing stay. Remove it, or move it onto \
                                 the marker's line or the line directly above it"
                            ),
                        )));
                    }
                }
                Kind::Marker(Marker::Referenced) => {}
                Kind::Marker(_) if allowed => {}
                Kind::Marker(Marker::Init) => {
                    let rest = after_init(mark);
                    let open = if rest.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", shown(rest.trim_end_matches('.')))
                    };
                    found.push(at(Finding::error(
                        Rule::UnresolvedInitMarker,
                        format!(
                            "init left a decision open here, and nobody has taken it yet{open}. \
                             Take it, write it in place of the marker and remove the marker"
                        ),
                    )));
                }
                Kind::Marker(Marker::Unreferenced(word)) => {
                    found.push(at(Finding::error(
                        Rule::UnreferencedMarker,
                        format!(
                            "'{}' names no issue that tracks the task. Open an issue and write its \
                             number into the marker, as {word}(#123), or {word}(owner/repo#123) \
                             for an issue in another repository; or, where the marker has to \
                             stay without one, write {ALLOWANCE} and the reason on its line or \
                             the line above",
                            shown(mark.from.trim_end())
                        ),
                    )));
                }
            }
        }
    }
    found
}

/// The findings about the markers and allowances in every file that is text
/// and that no `[[exclude]]` entry takes out, each file read once. A file that
/// cannot be read is passed over: the findings about that file say why.
#[must_use]
pub fn findings<'a>(
    files: impl IntoIterator<Item = (&'a RelativePath, &'a Coverage)>,
    read: &ReadBytes<'_>,
) -> Vec<Finding> {
    let mut found = Vec::new();
    for (path, coverage) in files {
        if matches!(coverage, Coverage::Excluded { .. }) {
            continue;
        }
        let Ok(bytes) = read(path) else {
            continue;
        };
        if let Some(text) = text(&bytes) {
            found.extend(judge(path, text));
        }
    }
    found
}
