//! The test of the case.

#[test]
fn counts_up() {
    assert_eq!(fixture::next(1), 2);
}

#[test]
fn knows_even() {
    assert!(fixture::even(2));
}
