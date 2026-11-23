//! A crate for a test case of `check-rust`.

/// The number after `value`, or `value` itself at the top.
#[must_use]
pub fn next(value: u8) -> u8 {
    value.saturating_add(1)
}

/// The value itself, without the feature `extra`.
#[cfg(not(feature = "extra"))]
#[must_use]
pub fn same(value: u8) -> u8 {
    return value;
}
