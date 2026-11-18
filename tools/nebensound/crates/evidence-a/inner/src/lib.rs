//! Evidence for #10: checked although it lies inside an excluded package.

/// Never called by a test, so coverage has to name it.
#[must_use]
pub fn untested() -> u32 {
    2
}
