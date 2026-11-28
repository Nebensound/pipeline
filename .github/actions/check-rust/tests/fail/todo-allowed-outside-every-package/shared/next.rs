#![allow(clippy::todo)]
//! A module of no package, which the crate pulls in with `#[path]`.

/// The number after `value`, once it is written.
#[must_use]
pub fn next(value: u8) -> u8 {
    todo!("count up from {value}")
}
