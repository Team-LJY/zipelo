use super::*;

fn field(tag: u16, value: &[u8]) -> Vec<u8> {
    [
        tag.to_le_bytes().as_slice(),
        &(value.len() as u16).to_le_bytes(),
        value,
    ]
    .concat()
}

#[test]
fn truncated_headers_and_values_are_rejected_by_every_policy() {
    for policy in [
        Policy::Streaming,
        Policy::Classic,
        Policy::Recovery,
        Policy::Names,
    ] {
        for bytes in [vec![0], vec![0, 0], vec![0, 0, 0], vec![2, 0, 1, 0]] {
            assert!(matches!(parse(&bytes, policy), Err(ErrorCode::Integrity)));
        }
        assert!(parse(&field(2, &[]), policy).is_ok());
    }
}

#[test]
fn zip64_is_borrowed_only_for_normal_streaming() {
    let bytes = field(1, &[0; 16]);
    assert_eq!(
        parse(&bytes, Policy::Streaming).unwrap().zip64,
        Some(&bytes[4..])
    );
    for policy in [Policy::Classic, Policy::Recovery] {
        assert!(matches!(
            parse(&bytes, policy),
            Err(ErrorCode::UnsupportedFeature)
        ));
    }
    assert!(matches!(
        parse(
            &[bytes.as_slice(), bytes.as_slice()].concat(),
            Policy::Streaming
        ),
        Err(ErrorCode::Integrity)
    ));
    for tag in [1, 0x0017, 0x0018] {
        assert!(matches!(
            parse(&field(tag, &[]), Policy::Recovery),
            Err(ErrorCode::UnsupportedFeature)
        ));
    }
}

#[test]
fn aes_policy_keeps_support_and_duplicate_checks() {
    let value = [2, 0, b'A', b'E', 3, 8, 0];
    let bytes = field(0x9901, &value);
    for policy in [Policy::Streaming, Policy::Classic] {
        assert_eq!(
            parse(&bytes, policy).unwrap().aes,
            Some(AesExtra {
                version: 2,
                method: 8
            })
        );
        assert!(matches!(
            parse(&[bytes.as_slice(), bytes.as_slice()].concat(), policy),
            Err(ErrorCode::Integrity)
        ));
        assert!(matches!(
            parse(&field(0x9901, &value[..6]), policy),
            Err(ErrorCode::Integrity)
        ));
        let mut unsupported = value;
        unsupported[4] = 2;
        assert!(matches!(
            parse(&field(0x9901, &unsupported), policy),
            Err(ErrorCode::UnsupportedFeature)
        ));
    }
    assert!(parse(&bytes, Policy::Recovery).unwrap().aes.is_none());
}

#[test]
fn unicode_and_aes_fields_can_share_the_same_extra() {
    let bytes = [
        field(0x7075, &[1, 0, 0, 0, 0]),
        field(0x9901, &[2, 0, b'A', b'E', 3, 0, 0]),
    ]
    .concat();
    assert_eq!(
        parse(&bytes, Policy::Names).unwrap().unicode,
        Some(&bytes[4..9])
    );
    assert!(parse(&bytes, Policy::Streaming).unwrap().aes.is_some());
    assert!(matches!(
        parse(&field(0x7075, &[0; 4]), Policy::Names),
        Err(ErrorCode::Integrity)
    ));
}

#[test]
fn mixed_invalid_fields_keep_error_precedence() {
    let unsupported_aes = field(0x9901, &[3, 0, b'A', b'E', 3, 0, 0]);
    let duplicate_zip64 = [field(1, &[]), field(1, &[])].concat();
    let extra = [unsupported_aes.as_slice(), duplicate_zip64.as_slice()].concat();
    assert!(matches!(
        parse(&extra, Policy::Streaming),
        Err(ErrorCode::Integrity)
    ));
    assert!(matches!(
        parse(&extra, Policy::Classic),
        Err(ErrorCode::UnsupportedFeature)
    ));
    let truncated = [unsupported_aes.as_slice(), &[2, 0, 1, 0]].concat();
    assert!(matches!(
        parse(&truncated, Policy::Streaming),
        Err(ErrorCode::Integrity)
    ));
    assert!(matches!(
        parse(&truncated, Policy::Classic),
        Err(ErrorCode::UnsupportedFeature)
    ));
    assert!(matches!(
        parse(&[1, 0, 8, 0], Policy::Recovery),
        Err(ErrorCode::UnsupportedFeature)
    ));
}
