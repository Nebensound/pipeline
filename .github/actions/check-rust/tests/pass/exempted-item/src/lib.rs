//! A crate for a test case of `check-rust`.

/// The number after `value`, or `value` itself at the top.
#[must_use]
pub fn next(value: u8) -> u8 {
    value.saturating_add(1)
}

/// The number before `value`, once it is written.
#[expect(clippy::todo, reason = "counting down comes with a later change")]
#[must_use]
pub fn previous(value: u8) -> u8 {
    todo!("count down from {value}")
}
