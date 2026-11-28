//! `.editorconfig` at the root of the repository, read the way Prettier reads
//! it (see [`super::prettier`]).
//!
//! An editor and several programs read the same file, so the tool reads it
//! as Prettier 3.9.6 does, the version the register pins, and judges none of
//! its properties: a property the tool does not use is passed over, not
//! named. Prettier reads it through the package `editorconfig-without-wasm`,
//! whose ways the reading below follows:
//!
//! - A line is a section (`[pattern]`), a property (`name = value`, both
//!   trimmed, split at the first `=`), a comment (it starts with `#` or `;`)
//!   or blank. Any other line makes the whole file read as one that says
//!   nothing; the tool names it instead ([`parse`]). A section written twice
//!   is one section, in the place of its first.
//! - A property's name is read in lower case, and so is the value of the
//!   properties the specification defines with fixed values (`charset`,
//!   `end_of_line`, `indent_size`, `indent_style`, `insert_final_newline`,
//!   `trim_trailing_whitespace`). A value that reads as JSON is that value:
//!   `4` is a number, `"4"` is text.
//! - A section's pattern without a `/` matches a file of that name in any
//!   folder; one that starts with `/`, or holds a `/`, matches from the root.
//!   `*` stops at a `/`, `**` does not, `?` is one character, `[...]` one of
//!   them (`[!...]` none of them), `{a,b}` either, and a sequence such as
//!   `{1..3}`, `{01..10..3}` or `{a..c}` each number or letter it stands for,
//!   as the package `brace-expansion` expands it.
//!   A pattern with a form of `(...)`, which the package's matcher reads as a
//!   group, is not read; the tool names its section ([`Read::unread`]).
//! - Each section whose pattern matches a file lays its properties over those
//!   of the sections before it ([`EditorConfig::properties`]).

use regex::Regex;
use serde_json::Value;

/// The name of the file.
pub const NAME: &str = ".editorconfig";

/// The properties whose value the specification fixes, which are read in
/// lower case.
const KNOWN: &[&str] = &[
    "charset",
    "end_of_line",
    "indent_size",
    "indent_style",
    "insert_final_newline",
    "trim_trailing_whitespace",
];

/// The most patterns one section may stand for once its braces are
/// expanded, so that a range like `{1..100000000}` stays a finding and does
/// not fill the memory.
const EXPANDED: u64 = 10_000;

/// The value of a property, as the package reads it.
#[derive(Debug, Clone, PartialEq)]
pub enum Property {
    /// Text.
    Text(String),
    /// A number.
    Number(f64),
    /// Anything else JSON reads: true, false, a list or an object.
    Other,
}

impl Property {
    /// The value written after the `=` of `name`, read as the package reads
    /// it.
    fn read(name: &str, written: &str) -> Self {
        match serde_json::from_str::<Value>(written) {
            Ok(Value::String(text)) => Self::Text(text),
            Ok(Value::Number(number)) => number.as_f64().map_or(Self::Other, Self::Number),
            Ok(Value::Null) => Self::Text(written.to_owned()),
            Ok(_) => Self::Other,
            Err(_) if KNOWN.contains(&name) => Self::Text(written.to_lowercase()),
            Err(_) => Self::Text(written.to_owned()),
        }
    }

    /// Whether it is this text.
    fn is(&self, text: &str) -> bool {
        matches!(self, Self::Text(value) if value == text)
    }

    /// The whole number above 0 it is, as JavaScript's
    /// `Number.isSafeInteger` with `> 0` takes it.
    #[must_use]
    pub fn positive(&self) -> Option<u64> {
        match self {
            Self::Number(number) => whole(*number).filter(|number| *number > 0),
            _ => None,
        }
    }
}

/// The whole number of 0 or more a number of JavaScript is, where it is one
/// that JavaScript holds exactly (no larger than 2^53 - 1).
#[must_use]
pub fn whole(number: f64) -> Option<u64> {
    if number.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(&number) {
        // A whole number from 0 to 2^53 - 1 fits, and nothing is cut off.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(number as u64)
    } else {
        None
    }
}

/// A section of the file: its pattern as written, the pattern built, and
/// its properties, each name once, in the order written.
#[derive(Debug, Clone)]
pub struct Section {
    /// The pattern, as written between the brackets.
    pub name: String,
    /// The pattern built, or [`None`] where the tool does not read it.
    pattern: Option<Regex>,
    /// The properties, their names in lower case.
    pub properties: Vec<(String, Property)>,
}

/// What the tool read of the file.
#[derive(Debug, Clone, Default)]
pub struct EditorConfig {
    /// Its sections, in the order written.
    pub sections: Vec<Section>,
}

/// The file, read, and the patterns the tool does not read in it.
#[derive(Debug, Clone, Default)]
pub struct Read {
    /// The file.
    pub config: EditorConfig,
    /// Each section whose pattern the tool does not read, with why; it
    /// matches no file.
    pub unread: Vec<(String, String)>,
}

impl EditorConfig {
    /// The properties that hold for a file, a path from the repository root:
    /// each section that matches it laid over the ones before, then the
    /// values the specification derives from others, as the package derives
    /// them.
    #[must_use]
    pub fn properties(&self, file: &str) -> Vec<(String, Property)> {
        let mut laid: Vec<(String, Property)> = Vec::new();
        for section in &self.sections {
            if section
                .pattern
                .as_ref()
                .is_some_and(|pattern| pattern.is_match(file))
            {
                for (name, value) in &section.properties {
                    put(&mut laid, name, value.clone());
                }
            }
        }
        let has = |laid: &[(String, Property)], name: &str| laid.iter().any(|(key, _)| key == name);
        let get = |laid: &[(String, Property)], name: &str| {
            laid.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        if get(&laid, "indent_style").is_some_and(|style| style.is("tab"))
            && !has(&laid, "indent_size")
        {
            put(&mut laid, "indent_size", Property::Text("tab".to_owned()));
        }
        if let Some(size) = get(&laid, "indent_size")
            && !has(&laid, "tab_width")
            && !size.is("tab")
        {
            put(&mut laid, "tab_width", size);
        }
        if let (Some(size), Some(width)) = (get(&laid, "indent_size"), get(&laid, "tab_width"))
            && size.is("tab")
        {
            put(&mut laid, "indent_size", width);
        }
        laid
    }
}

/// Sets a property: in the place of the same name, else last.
fn put(laid: &mut Vec<(String, Property)>, name: &str, value: Property) {
    match laid.iter_mut().find(|(key, _)| key == name) {
        Some(slot) => slot.1 = value,
        None => laid.push((name.to_owned(), value)),
    }
}

/// Reads the text of the file.
///
/// # Errors
///
/// Where a line is neither a section, a property, a comment nor blank: the
/// package then reads the whole file as one that says nothing, so the tool
/// names it, with the line.
pub fn parse(source: &str) -> Result<Read, String> {
    let mut sections: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut current: Option<usize> = None;
    let source = source.replace("\r\n", "\n").replace('\r', "\n");
    for (index, line) in source.split('\n').enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[') {
            let Some(name) = inner.strip_suffix(']') else {
                return Err(format!(
                    "line {} opens a section and does not close it",
                    index + 1
                ));
            };
            let place = sections.iter().position(|(written, _)| written == name);
            current = Some(place.unwrap_or_else(|| {
                sections.push((name.to_owned(), Vec::new()));
                sections.len() - 1
            }));
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            return Err(format!(
                "line {} is neither a section, a property nor a comment",
                index + 1
            ));
        };
        // A property before the first section, such as `root`, holds for
        // no file.
        if let Some(place) = current {
            let properties = &mut sections[place].1;
            let (name, value) = (name.trim().to_owned(), value.trim().to_owned());
            match properties.iter_mut().find(|(written, _)| *written == name) {
                Some(slot) => slot.1 = value,
                None => properties.push((name, value)),
            }
        }
    }
    let mut read = Read::default();
    for (name, written) in sections {
        let pattern = match glob(&name) {
            Ok(pattern) => Some(pattern),
            Err(why) => {
                read.unread.push((name.clone(), why));
                None
            }
        };
        let mut properties = Vec::new();
        for (key, value) in written {
            let key = key.to_lowercase();
            let value = Property::read(&key, &value);
            put(&mut properties, &key, value);
        }
        read.config.sections.push(Section {
            name,
            pattern,
            properties,
        });
    }
    Ok(read)
}

/// A section's pattern, built over the path from the repository root.
fn glob(name: &str) -> Result<Regex, String> {
    let pattern = match name.find('/') {
        None => format!("**/{name}"),
        Some(0) => name[1..].to_owned(),
        Some(_) => name.to_owned(),
    };
    let chars: Vec<char> = pattern.chars().collect();
    let mut count = 1;
    let body = translate(&chars, true, &mut count)?;
    Regex::new(&format!("^{body}$")).map_err(|error| format!("it does not read: {error}"))
}

/// The pattern `chars` as a regular expression; `start` says whether it
/// begins a part of the path. `count` is how many patterns the braces have
/// expanded to so far.
fn translate(chars: &[char], start: bool, count: &mut u64) -> Result<String, String> {
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        let at_start = if index == 0 {
            start
        } else {
            chars[index - 1] == '/'
        };
        let next = chars.get(index + 1).copied();
        match chars[index] {
            '\\' if next.is_some() => {
                out.push_str(&regex::escape(&chars[index + 1].to_string()));
                index += 2;
            }
            '*' | '?' | '+' | '@' | '!' if next == Some('(') => {
                return Err(
                    "it holds a group, `(...)`, a form of pattern the tool does not read"
                        .to_owned(),
                );
            }
            '*' if next == Some('*') => {
                if at_start && chars.get(index + 2) == Some(&'/') {
                    out.push_str("(?:.*/)?");
                    index += 3;
                } else {
                    out.push_str(".*");
                    index += 2;
                }
            }
            '*' => {
                out.push_str("[^/]*");
                index += 1;
            }
            '?' => {
                out.push_str("[^/]");
                index += 1;
            }
            '[' if let Some((built, used)) = class(&chars[index + 1..]) => {
                out.push_str(&built);
                index += used + 1;
            }
            '{' if let Some(end) = closing(&chars[index + 1..]) => {
                let inner = &chars[index + 1..index + 1 + end];
                out.push_str(&braces(inner, at_start, count)?);
                index += end + 2;
            }
            other => {
                out.push_str(&regex::escape(&other.to_string()));
                index += 1;
            }
        }
    }
    Ok(out)
}

/// A class of characters after its `[`, and how many characters it used up
/// with its `]`; [`None`] where it is not closed, and the `[` is then itself.
/// A `]` right after the `[` or `[!` is one of the characters. A class never
/// matches a `/`, as each part of the path is matched on its own.
fn class(chars: &[char]) -> Option<(String, usize)> {
    let negated = matches!(chars.first(), Some('!' | '^'));
    let from = usize::from(negated);
    let end = chars
        .iter()
        .enumerate()
        .skip(from + 1)
        .find(|(_, char)| **char == ']')
        .map(|(index, _)| index)?;
    let inner: String = chars[from..end]
        .iter()
        .map(|char| match char {
            '\\' | '[' | ']' | '^' | '&' | '~' => format!("\\{char}"),
            other => other.to_string(),
        })
        .collect();
    let built = if negated {
        format!("[^/{inner}]")
    } else {
        format!("[{inner}&&[^/]]")
    };
    Some((built, end + 1))
}

/// Counts the patterns one more pair of braces multiplies the section's
/// patterns by.
///
/// # Errors
///
/// Where they come to more than [`EXPANDED`].
fn expand(count: &mut u64, by: usize) -> Result<(), String> {
    *count = count.saturating_mul(u64::try_from(by).unwrap_or(u64::MAX));
    if *count > EXPANDED {
        return Err(too_many());
    }
    Ok(())
}

/// Where the `}` stands that closes a `{`, in the characters after it.
fn closing(chars: &[char]) -> Option<usize> {
    let mut depth = 0;
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 1,
            '{' => depth += 1,
            '}' if depth == 0 => return Some(index),
            '}' => depth -= 1,
            _ => {}
        }
        index += 1;
    }
    None
}

/// The content of a pair of braces: the alternatives it holds, split at its
/// commas outside inner braces, a sequence of numbers or letters
/// ([`sequence`]), or else itself with its braces, as the package leaves
/// `{a}`.
fn braces(inner: &[char], start: bool, count: &mut u64) -> Result<String, String> {
    let mut parts: Vec<&[char]> = Vec::new();
    let mut depth = 0;
    let mut from = 0;
    let mut index = 0;
    while index < inner.len() {
        match inner[index] {
            '\\' => index += 1,
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&inner[from..index]);
                from = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    parts.push(&inner[from..]);
    if parts.len() == 1 {
        let text: String = inner.iter().collect();
        return match sequence(&text) {
            Some(items) => {
                let items = items?;
                expand(count, items.len())?;
                Ok(format!("(?:{})", items.join("|")))
            }
            None => Ok(format!(r"\{{{}\}}", translate(inner, false, count)?)),
        };
    }
    expand(count, parts.len())?;
    let built: Vec<String> = parts
        .into_iter()
        .map(|part| translate(part, start, count))
        .collect::<Result<_, _>>()?;
    Ok(format!("(?:{})", built.join("|")))
}

/// What a sequence stands for, as the package `brace-expansion` expands it;
/// [`None`] where the text is no sequence. A sequence is `a..b` or
/// `a..b..step`, the step a whole number whose sign is passed over and which
/// is at least 1. Its ends are both whole numbers, or both single ASCII
/// letters, which stand for every character from the one to the other, a
/// `\` among them standing for nothing. Where an end or the step is written
/// with a leading zero, every number is padded with zeros to the width of
/// the longer end, its `-` counted.
///
/// # Errors
///
/// Where it stands for more than [`EXPANDED`] patterns, or an end does not
/// fit a 64-bit number.
fn sequence(text: &str) -> Option<Result<Vec<String>, String>> {
    let parts: Vec<&str> = text.split("..").collect();
    let (first, last, step) = match parts[..] {
        [first, last] => (first, last, None),
        [first, last, step] if number(step) => (first, last, Some(step)),
        _ => return None,
    };
    let letters = letter(first) && letter(last);
    let (from, to) = if letters {
        (code(first), code(last))
    } else if number(first) && number(last) {
        let (Ok(from), Ok(to)) = (first.parse::<i64>(), last.parse::<i64>()) else {
            return Some(Err(
                "its braces hold a sequence whose ends are larger than the tool reads".to_owned(),
            ));
        };
        (from, to)
    } else {
        return None;
    };
    let by = step.map_or(1, |step| {
        step.trim_start_matches('-')
            .parse::<u64>()
            .unwrap_or(u64::MAX)
            .max(1)
    });
    let count = from.abs_diff(to) / by;
    if count >= EXPANDED {
        return Some(Err(too_many()));
    }
    let padded = !letters && parts.iter().any(|part| padded(part));
    let width = first.len().max(last.len());
    let direction: i128 = if to < from { -1 } else { 1 };
    Some(Ok((0..=count)
        .map(|index| {
            let at = i128::from(from) + direction * i128::from(index) * i128::from(by);
            let text = if letters {
                letter_at(at)
            } else if padded {
                pad(at, width)
            } else {
                at.to_string()
            };
            regex::escape(&text)
        })
        .collect()))
}

/// Whether `text` is a whole number as a sequence writes it: digits, after
/// a `-` or not.
fn number(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether `text` is one ASCII letter.
fn letter(text: &str) -> bool {
    text.len() == 1 && text.bytes().all(|byte| byte.is_ascii_alphabetic())
}

/// The code of the one ASCII letter `text` holds.
fn code(text: &str) -> i64 {
    text.bytes().next().map_or(0, i64::from)
}

/// The character with the code `at`, which lies between two ASCII letters,
/// as text; a `\` is no text, as the package leaves it out.
fn letter_at(at: i128) -> String {
    let character = u8::try_from(at).map_or('\\', char::from);
    if character == '\\' {
        String::new()
    } else {
        character.to_string()
    }
}

/// Whether an end or the step of a sequence is written with a leading zero.
fn padded(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    digits.len() > 1 && digits.starts_with('0')
}

/// The number `at` padded with zeros to `width` characters, a `-` counted,
/// the zeros after it.
fn pad(at: i128, width: usize) -> String {
    let digits = at.unsigned_abs().to_string();
    let sign = if at < 0 { "-" } else { "" };
    let zeros = width.saturating_sub(digits.len() + sign.len());
    format!("{sign}{}{digits}", "0".repeat(zeros))
}

/// Why a section whose braces stand for too many patterns is not read.
fn too_many() -> String {
    format!("its braces stand for more than {EXPANDED} patterns, more than the tool reads")
}
