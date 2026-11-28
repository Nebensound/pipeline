//! `todo!`, `unimplemented!` and `dbg!`: leftovers the Rust check holds as
//! errors, with one item exempted the way Rust offers it.
//!
//! Clippy recognises the three macros exactly, with the lints [`LINTS`], which
//! belong to its group `clippy::restriction` and are off by default. Every
//! Clippy run of the check hands them over as errors on the command line
//! ([`denied`]), for every target, tests included. The command line wins over
//! a `[lints]` or `[workspace.lints]` table of the manifest and over
//! `RUSTFLAGS` or the `rustflags` of Cargo's configuration, but an attribute in
//! the source wins over the command line: `#![allow(clippy::todo)]` at the
//! crate root lets every `todo!` of the crate pass. Forbidding the lints
//! (`-F`) instead would refuse every attribute, the exemption included, and
//! `--force-warn` ignores every attribute but only warns, so no form of the
//! command line gives both. The check therefore reads the sources itself:
//!
//! - **The exemption** is an outer attribute `#[expect(<lint>, reason =
//!   "…")]`, written as it stands, on the item that holds the call. Its reason
//!   has to hold a letter or a digit. Rust reports an expectation that nothing
//!   fulfils, so an exemption cannot outlive its call.
//! - **Every other attribute that lowers one of them** is a finding at its
//!   line ([`attributes`]): `allow`, `warn` or `expect` naming one of
//!   [`LINTS`] or [`GROUP`], written inside a crate or a module (`#![…]`),
//!   inside a `cfg_attr`, inside a macro or handed to one, and an `expect`
//!   without a reason.
//! - **Clippy's configuration** can switch `dbg!` off in every test with
//!   [`DBG_IN_TESTS`]; a file that does is a finding at that line
//!   ([`configuration`]). Clippy takes its configuration from a folder Cargo's
//!   configuration can name, so every `clippy.toml` and `.clippy.toml` the
//!   repository keeps is read ([`is_configuration`]).
//!
//! The search reads tokens, not text, so what stands in a comment or a string
//! is no attribute. It does not expand macros: code of the repository that
//! builds such an attribute by other means, or a macro of its own that
//! expands to one of the three (which Clippy does not report at the call),
//! is for the review of the change to see.

use std::str::FromStr as _;

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use serde::Deserialize;

use crate::finding::Position;
use crate::path::RelativePath;

/// The lints that report `todo!`, `unimplemented!` and `dbg!`.
pub const LINTS: [&str; 3] = ["clippy::todo", "clippy::unimplemented", "clippy::dbg_macro"];

/// The group of Clippy's lints that holds [`LINTS`]: lowering it lowers them.
pub const GROUP: &str = "clippy::restriction";

/// The key of Clippy's configuration that lets `dbg!` pass in every test.
pub const DBG_IN_TESTS: &str = "allow-dbg-in-tests";

/// The names Clippy takes its configuration from, in a folder.
pub const CONFIGURATION: [&str; 2] = ["clippy.toml", ".clippy.toml"];

/// The levels below an error that an attribute can set.
const LOWERING: [&str; 3] = ["allow", "warn", "expect"];

/// What follows `--` in a Clippy run to hand it [`LINTS`] as errors.
#[must_use]
pub fn denied() -> Vec<&'static str> {
    LINTS.iter().flat_map(|lint| ["-D", lint]).collect()
}

/// An attribute that lowers one of [`LINTS`] other than the exemption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lowering {
    /// Where its level stands.
    pub position: Position,
    /// What is wrong with it.
    pub message: String,
}

/// Where a group of tokens stands: in an attribute, outer (`#[…]`) or inner
/// (`#![…]`), or elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Outer,
    Inner,
    Other,
}

/// Every attribute in this Rust source that lowers one of [`LINTS`] and is no
/// exemption, in the order they stand; [`None`] when the source holds no
/// tokens Rust could read, which Clippy then reports itself where it compiles
/// the file.
#[must_use]
pub fn attributes(source: &str) -> Option<Vec<Lowering>> {
    let stream = TokenStream::from_str(source).ok()?;
    let mut found = Vec::new();
    walk(stream, Place::Other, &mut found);
    // The positions are read; the spans of the file are not needed again,
    // and every file read on this thread would otherwise keep its own.
    proc_macro2::extra::invalidate_current_thread_spans();
    Some(found)
}

/// Looks through the tokens of one group, and every group in it, for a level
/// of [`LOWERING`] followed by its parentheses.
fn walk(stream: TokenStream, place: Place, found: &mut Vec<Lowering>) {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    for (index, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Group(group) => {
                let before = |back: usize| index.checked_sub(back).and_then(|at| tokens.get(at));
                let inner = matches!(group.delimiter(), Delimiter::Bracket)
                    && is_punct(before(1), '!')
                    && is_punct(before(2), '#');
                let outer =
                    matches!(group.delimiter(), Delimiter::Bracket) && is_punct(before(1), '#');
                let within = if inner {
                    Place::Inner
                } else if outer {
                    Place::Outer
                } else {
                    Place::Other
                };
                walk(group.stream(), within, found);
            }
            TokenTree::Ident(ident) => {
                let level = unraw(&ident.to_string());
                let Some(TokenTree::Group(list)) = tokens.get(index + 1) else {
                    continue;
                };
                if !LOWERING.contains(&level.as_str()) || list.delimiter() != Delimiter::Parenthesis
                {
                    continue;
                }
                let items = items(list.stream());
                let lints = items
                    .iter()
                    .filter_map(|item| {
                        let path = path(item);
                        LINTS.into_iter().chain([GROUP]).find(|lint| *lint == path)
                    })
                    .collect::<Vec<&str>>()
                    .join(", ");
                if lints.is_empty() {
                    continue;
                }
                // The attribute is the level and its list, and nothing else.
                let alone = index == 0 && tokens.len() == 2;
                let start = ident.span().start();
                let position = Position {
                    line: start.line,
                    column: start.column + 1,
                };
                let message = match (alone, place, level.as_str()) {
                    (true, Place::Outer, "expect") if items.iter().any(|item| reason(item)) => {
                        continue;
                    }
                    (true, Place::Outer, "expect") => format!(
                        "#[expect({lints})] gives no reason: the exemption of one item needs \
                         reason = \"…\""
                    ),
                    (true, Place::Inner, _) => format!(
                        "#![{level}({lints})] covers the whole crate or module, and the Rust \
                         check holds todo!, unimplemented! and dbg! as errors there; exempt the \
                         one item with #[expect(…, reason = \"…\")] on it"
                    ),
                    _ => format!(
                        "{level}({lints}) lowers what the Rust check holds as errors; exempt the \
                         one item with #[expect(…, reason = \"…\")] on it, written as it stands"
                    ),
                };
                found.push(Lowering { position, message });
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

/// Whether this token is the punctuation `mark`.
fn is_punct(token: Option<&TokenTree>, mark: char) -> bool {
    matches!(token, Some(TokenTree::Punct(punct)) if punct.as_char() == mark)
}

/// An identifier without the `r#` of a raw one, which names the same thing.
fn unraw(ident: &str) -> String {
    ident.strip_prefix("r#").unwrap_or(ident).to_owned()
}

/// The items of a list, split at its commas.
fn items(stream: TokenStream) -> Vec<Vec<TokenTree>> {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    tokens
        .split(|token| is_punct(Some(token), ','))
        .map(<[TokenTree]>::to_vec)
        .collect()
}

/// The path an item of a list names, such as `clippy::todo`, written without
/// spaces and raw identifiers; anything else in it makes no path of [`LINTS`].
fn path(item: &[TokenTree]) -> String {
    item.iter()
        .map(|token| match token {
            TokenTree::Ident(ident) => unraw(&ident.to_string()),
            TokenTree::Punct(punct) if punct.as_char() == ':' => ":".to_owned(),
            other => format!(" {other} "),
        })
        .collect()
}

/// Whether an item of a list is `reason = "…"`, also written as a raw
/// string, with a reason that holds a letter or a digit.
fn reason(item: &[TokenTree]) -> bool {
    let [
        TokenTree::Ident(key),
        TokenTree::Punct(equals),
        TokenTree::Literal(text),
    ] = item
    else {
        return false;
    };
    let text = text.to_string();
    let inside = text
        .trim_start_matches('r')
        .trim_matches('#')
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_default();
    key == "reason" && equals.as_char() == '=' && inside.chars().any(char::is_alphanumeric)
}

/// Whether Clippy takes its configuration from a file of this name.
#[must_use]
pub fn is_configuration(path: &RelativePath) -> bool {
    path.file_name()
        .is_some_and(|name| CONFIGURATION.contains(&name))
}

/// What the check reads of Clippy's configuration.
#[derive(Deserialize)]
struct Configuration {
    #[serde(rename = "allow-dbg-in-tests")]
    dbg_in_tests: Option<toml::Spanned<toml::Value>>,
}

/// Where this configuration of Clippy sets [`DBG_IN_TESTS`] to `true`, if it
/// does; [`None`] as well when it is no TOML, which Clippy then reports
/// itself where it reads it.
#[must_use]
pub fn configuration(source: &str) -> Option<Position> {
    let read: Configuration = toml::from_str(source).ok()?;
    read.dbg_in_tests
        .filter(|value| value.get_ref() == &toml::Value::Boolean(true))
        .map(|value| Position::at(source, value.span().start))
}
