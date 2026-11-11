use super::*;

#[test]
fn zip64_sizes_and_offsets_preserve_u64_values() {
    let boundary = u32::MAX as u64;
    for size in [boundary, boundary + 1, 16 * 1024 * 1024 * 1024] {
        let extra = zip64_extra(size, 17, Some(boundary + 7));
        assert_eq!(&extra[..4], &[1, 0, 24, 0]);
        assert_eq!(u64::from_le_bytes(extra[4..12].try_into().unwrap()), size);
        assert_eq!(u64::from_le_bytes(extra[12..20].try_into().unwrap()), 17);
        assert_eq!(
            u64::from_le_bytes(extra[20..28].try_into().unwrap()),
            boundary + 7
        );
    }
    let extra = zip64_extra(17, 23, Some(boundary));
    assert_eq!(&extra[..4], &[1, 0, 8, 0]);
    assert_eq!(
        u64::from_le_bytes(extra[4..12].try_into().unwrap()),
        boundary
    );
    assert!(zip64_extra(boundary - 1, 23, Some(boundary - 1)).is_empty());
}
