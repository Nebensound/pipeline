//! The test of the case.

#[test]
fn counts_up() {
    assert_eq!(fixture::next(1), 2);
}

#[test]
#[should_panic(expected = "count down from 2")]
fn counting_down_is_not_written_yet() {
    let _ = fixture::previous(2);
}
