//! A crate for a test case of `build-cargo-package`, depending on `lib`.

/// Two steps after `value`.
#[must_use]
pub fn twice(value: u8) -> u8 {
    lib::next(lib::next(value))
}
