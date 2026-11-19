//! A crate for a test case of `check-rust` that depends on one outside its
//! workspace.

/// The number after `value`, as the library counts.
#[must_use]
pub fn next(value: u8) -> u8 {
    lib::next(value)
}
