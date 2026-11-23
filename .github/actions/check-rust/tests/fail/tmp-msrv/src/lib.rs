//! A crate for a test case of `check-rust`.

/// The number after `value`, or `value` itself at the top.
#[must_use]
pub fn next(value: u8) -> u8 {
    value.saturating_add(1)
}

/// Whether `value` is even.
#[must_use]
#[allow(clippy::incompatible_msrv)]
pub fn even(value: u8) -> bool {
    value.is_multiple_of(2)
}
