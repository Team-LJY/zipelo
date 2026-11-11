use super::*;

#[test]
fn offsets_do_not_overflow() {
    assert_eq!(add(u64::MAX, 1), Err(ErrorCode::Integrity));
    assert_eq!(add(u64::MAX - 1, 1), Ok(u64::MAX));
}

#[test]
fn eocd_fields_expand_independently_and_match_unextended_values() {
    for maximum in [u16::MAX as u64, u32::MAX as u64] {
        assert_eq!(expanded(maximum, maximum, 0), Ok(0));
        assert_eq!(expanded(maximum, maximum, u64::MAX), Ok(u64::MAX));
        assert_eq!(expanded(7, maximum, 7), Ok(7));
        assert_eq!(expanded(7, maximum, 8), Err(ErrorCode::Integrity));
    }
}

#[test]
fn central_extensions_consume_only_sentinel_fields_in_order() {
    for mask in 0..16 {
        let mut extra = Vec::new();
        let values = [11u64, 22, 33];
        for (i, value) in values.iter().enumerate() {
            if mask & (1 << i) != 0 {
                extra.extend(value.to_le_bytes());
            }
        }
        if mask & 8 != 0 {
            extra.extend(44u32.to_le_bytes());
        }
        let mut rest = extra.as_slice();
        for (i, value) in values.iter().enumerate() {
            let ordinary = if mask & (1 << i) != 0 {
                u32::MAX
            } else {
                *value as u32
            };
            assert_eq!(large_value(ordinary, &mut rest), Ok(*value));
        }
        assert_eq!(
            disk_start(if mask & 8 != 0 { u16::MAX } else { 44 }, &mut rest),
            Ok(44)
        );
        assert!(rest.is_empty());
    }
    for length in 0..4 {
        assert_eq!(
            disk_start(u16::MAX, &mut vec![0; length].as_slice()),
            Err(ErrorCode::Integrity)
        );
    }
    for length in 0..8 {
        assert_eq!(
            large_value(u32::MAX, &mut vec![0; length].as_slice()),
            Err(ErrorCode::Integrity)
        );
    }
}

#[test]
fn local_zip64_always_reads_both_sizes() {
    let extra = [11u64.to_le_bytes(), 22u64.to_le_bytes()].concat();
    for size in [11, u32::MAX] {
        for compressed in [22, u32::MAX] {
            assert_eq!(local_sizes(size, compressed, Some(&extra)), Ok((11, 22)));
        }
    }
    assert_eq!(local_sizes(11, 22, None), Ok((11, 22)));
    assert_eq!(local_sizes(u32::MAX, 22, None), Err(ErrorCode::Integrity));
    assert_eq!(local_sizes(11, u32::MAX, None), Err(ErrorCode::Integrity));
    assert_eq!(
        local_sizes(12, u32::MAX, Some(&extra)),
        Err(ErrorCode::Integrity)
    );
    for length in 0..16 {
        assert_eq!(
            local_sizes(u32::MAX, u32::MAX, Some(&extra[..length])),
            Err(ErrorCode::Integrity)
        );
    }
}

#[test]
fn descriptor_width_uses_sizes_and_crc() {
    for (compressed, size) in [(21u64, 21u64), (0, 0)] {
        let crc = 123u32;
        let wide = [
            crc.to_le_bytes().as_slice(),
            &compressed.to_le_bytes(),
            &size.to_le_bytes(),
        ]
        .concat();
        let narrow = [
            crc.to_le_bytes(),
            (compressed as u32).to_le_bytes(),
            (size as u32).to_le_bytes(),
        ]
        .concat();
        assert_eq!(descriptor_size(&wide, crc, compressed, size, false), Ok(20));
        assert_eq!(descriptor_size(&wide, crc, compressed, size, true), Ok(20));
        assert_eq!(
            descriptor_size(&narrow, crc, compressed, size, false),
            Ok(12)
        );
        assert_eq!(
            descriptor_size(&narrow, crc, compressed, size, true),
            Err(ErrorCode::Integrity)
        );
        let followed = [narrow.as_slice(), b"PK\x03\x04\x14\0\0\0"].concat();
        assert_eq!(
            descriptor_size(&followed, crc, compressed, size, false),
            Ok(12)
        );
        assert_eq!(
            descriptor_size(&wide, crc + 1, compressed, size, false),
            Err(ErrorCode::Integrity)
        );
        assert_eq!(
            descriptor_size(&wide[..11], crc, compressed, size, false),
            Err(ErrorCode::Integrity)
        );
    }
}
