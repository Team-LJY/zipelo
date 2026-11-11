use super::*;

#[test]
fn ratio_boundaries_do_not_overflow() {
    assert!(ratio(0, 0).is_ok());
    assert_eq!(ratio(1, 0), Err(ErrorCode::Limit));
    for compressed in [1, 17, 65536, 16 * 1024 * 1024] {
        assert!(ratio(compressed * 1000, compressed).is_ok());
        assert_eq!(
            ratio(compressed * 1000 + 1, compressed),
            Err(ErrorCode::Limit)
        );
    }
    assert!(ratio(u64::MAX, u64::MAX).is_ok());
    assert!(ratio(u64::MAX, u64::MAX / 1000 + 1).is_ok());
    assert_eq!(ratio(u64::MAX, u64::MAX / 1000), Err(ErrorCode::Limit));
    assert_eq!(add(u64::MAX, 1), Err(ErrorCode::Integrity));
}
