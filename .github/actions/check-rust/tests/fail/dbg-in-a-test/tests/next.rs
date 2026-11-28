//! The test of the case.

#[test]
fn counts_up() {
    assert_eq!(dbg!(fixture::next(1)), 2);
}
