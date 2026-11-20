//! The test of the case: it needs a module the package leaves out.

mod support;

#[test]
fn counts_up() {
    assert_eq!(lib::next(1), support::TWO);
}
