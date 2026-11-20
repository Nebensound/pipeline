//! A crate for a test case of `build-cargo-package`.

/// The number after `value`, or `value` itself at the top.
#[must_use]
pub fn next(value: u8) -> u8 {
    value.saturating_add(1)
}
