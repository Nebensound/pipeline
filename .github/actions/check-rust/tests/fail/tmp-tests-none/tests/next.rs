//! The test of the case.

#[test]
fn counts_up() {
    assert_eq!(fixture::next(1), 2);
}

#[cfg(not(feature = "extra"))]
#[test]
fn fails_without_extra() {
    assert_eq!(fixture::next(1), 3);
}
