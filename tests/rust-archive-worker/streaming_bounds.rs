use super::*;

#[test]
fn offsets_do_not_overflow() {
    assert_eq!(add(u64::MAX, 1), Err(ErrorCode::Integrity));
    assert_eq!(add(u64::MAX - 1, 1), Ok(u64::MAX));
}
