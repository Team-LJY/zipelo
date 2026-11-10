use crate::{error::Failure, support::*};

fn successful_output(bytes: Vec<u8>, expected: &[&[u8]]) {
    let (result, output) = extract(bytes, limits());
    assert_eq!(result, Ok(()));
    assert_eq!(validate_success(&frame_records(&output), expected), Ok(()));
}

fn failed_without_complete(bytes: Vec<u8>) -> Failure {
    let (result, output) = extract(bytes, limits());
    let failure = result.expect_err("실패가 필요함");
    assert!(!frame_records(&output).iter().any(|r| r.kind == 5));
    failure
}

fn failed_single_item_without_end(bytes: Vec<u8>) -> Failure {
    let (result, output) = extract(bytes, limits());
    let failure = result.expect_err("실패가 필요함");
    assert!(
        !frame_records(&output)
            .iter()
            .any(|r| matches!(r.kind, 4 | 5))
    );
    failure
}

#[test]
fn zip_stored_listing_and_extraction_reference() {
    let s = sample(&[Spec::stored("a")]);
    let entries = list(s.bytes.clone()).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["Entry"]["Name"], "a");
    assert_eq!(entries[0]["Entry"]["DeclaredSize"], 9);
    assert_eq!(entries[0]["EntryId"], "entry-0");
    successful_output(s.bytes, &[DATA]);
}

#[test]
fn zip_deflate_fixed_block_reference() {
    successful_output(sample(&[Spec::deflate("a")]).bytes, &[DATA]);
}

#[test]
fn zip_empty_archive_file_and_directory_complete() {
    for entries in [vec![], vec![Spec::empty("a")], vec![Spec::empty("dir/")]] {
        let s = sample(&entries);
        assert_eq!(list(s.bytes.clone()).unwrap().len(), entries.len());
        successful_output(s.bytes, &vec![&[][..]; entries.len()]);
    }
}

#[test]
fn zip_central_order_and_utf8_names_preserved() {
    let s = sample(&[Spec::stored("한글/a"), Spec::empty("z"), Spec::empty("A")]);
    let listed = list(s.bytes.clone()).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|v| v["Entry"]["Name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["한글/a", "z", "A"]
    );
    assert_eq!(listed[2]["EntryId"], "entry-2");
    successful_output(s.bytes, &[DATA, &[], &[]]);
}

#[test]
fn zip_exact_duplicate_names_fail_before_collapsing() {
    assert_eq!(
        list(sample(&[Spec::stored("a"), Spec::stored("a")]).bytes),
        Err(Failure::DuplicateName)
    );
}

#[test]
fn zip_local_name_flags_and_method_mismatch() {
    for offset in [6, 8, 30] {
        let mut s = sample(&[Spec::stored("a")]);
        s.bytes[offset] ^= 1;
        assert_eq!(list(s.bytes), Err(Failure::MetadataMismatch));
    }
}

#[test]
fn zip_local_crc_size_and_packed_mismatch() {
    for offset in [14, 18, 22] {
        let mut s = sample(&[Spec::stored("a")]);
        s.bytes[offset] ^= 1;
        assert_eq!(list(s.bytes), Err(Failure::MetadataMismatch));
    }
}

#[test]
fn zip_payload_corruption_list_is_not_crc_validation() {
    let mut s = sample(&[Spec::stored("a")]);
    s.bytes[s.data[0]] ^= 1;
    assert_eq!(list(s.bytes.clone()).unwrap()[0]["Verification"], 0);
    assert_eq!(failed_single_item_without_end(s.bytes), Failure::Integrity);
}

#[test]
fn zip_header_crc_matches_but_content_crc_fails() {
    let mut e = Spec::stored("a");
    e.crc ^= 1;
    let s = sample(&[e]);
    assert!(list(s.bytes.clone()).is_ok());
    assert_eq!(failed_single_item_without_end(s.bytes), Failure::Integrity);
}

#[test]
fn zip_signed_and_unsigned_descriptors() {
    for descriptor in [1, 2] {
        for mut e in [Spec::stored("a"), Spec::deflate("a")] {
            e.descriptor = descriptor;
            successful_output(sample(&[e]).bytes, &[DATA]);
        }
    }
}

#[test]
fn zip_descriptor_crc_packed_and_size_mismatch() {
    for descriptor in [1, 2] {
        for field in [0, 4, 8] {
            let mut e = Spec::stored("a");
            e.descriptor = descriptor;
            let mut s = sample(&[e]);
            let at = s.descriptor[0] + if descriptor == 2 { 4 } else { 0 } + field;
            s.bytes[at] ^= 1;
            assert_eq!(list(s.bytes), Err(Failure::MetadataMismatch));
        }
    }
}

#[test]
fn zip_packed_range_must_end_before_central() {
    let mut s = sample(&[Spec::deflate("a")]);
    let packed = (s.bytes.len() - s.data[0]) as u32;
    put32(&mut s.bytes, 18, packed);
    put32(&mut s.bytes, s.central[0] + 20, packed);
    assert_eq!(list(s.bytes), Err(Failure::Integrity));
}

#[test]
fn zip_local_records_cannot_overlap() {
    let mut s = sample(&[Spec::stored("a"), Spec::stored("b")]);
    let offset = s.central[1] + 42;
    put32(&mut s.bytes, offset, 0);
    // 다른 이름의 로컬 레코드 참조를 먼저 거부함.
    assert_eq!(list(s.bytes), Err(Failure::MetadataMismatch));
}

#[test]
fn zip_unaccounted_gap_before_central_rejected() {
    let mut s = sample(&[Spec::stored("a")]);
    let central = s.central[0];
    s.bytes.insert(central, 0);
    put32(&mut s.bytes, s.end + 1 + 16, (central + 1) as u32);
    assert_eq!(list(s.bytes), Err(Failure::Integrity));
}

#[test]
fn zip_eocd_and_central_boundaries_rejected() {
    for offset in [12, 16, 20] {
        let mut s = sample(&[Spec::stored("a")]);
        s.bytes[s.end + offset] ^= 1;
        assert_eq!(list(s.bytes), Err(Failure::Integrity));
    }
    let mut s = sample(&[Spec::stored("a")]);
    s.bytes.pop();
    assert_eq!(list(s.bytes), Err(Failure::Integrity));
}

#[test]
fn zip_multidisk_zip64_encryption_and_method_are_unsupported() {
    for mutation in 0..6 {
        let mut s = sample(&[Spec::stored("a")]);
        let c = s.central[0];
        match mutation {
            0 => put16(&mut s.bytes, s.end + 4, 1),
            1 => {
                put16(&mut s.bytes, s.end + 8, u16::MAX);
                put16(&mut s.bytes, s.end + 10, u16::MAX);
            }
            2 => put32(&mut s.bytes, c + 24, u32::MAX),
            3 => put16(&mut s.bytes, c + 8, 0x0801),
            4 => put16(&mut s.bytes, c + 10, 99),
            _ => put16(&mut s.bytes, c + 6, 45),
        }
        assert_eq!(list(s.bytes), Err(Failure::Unsupported));
    }
}

#[test]
fn zip_count_cap_is_checked_before_central_records() {
    let mut s = sample(&[]);
    put16(&mut s.bytes, 8, 1025);
    put16(&mut s.bytes, 10, 1025);
    assert_eq!(list(s.bytes), Err(Failure::Budget));
}

#[test]
fn zip_declared_output_cap_and_stored_size_mismatch() {
    let mut s = sample(&[Spec::stored("a")]);
    put32(&mut s.bytes, s.central[0] + 24, 16_777_217);
    assert_eq!(list(s.bytes), Err(Failure::Budget));
    let mut s = sample(&[Spec::stored("a")]);
    put32(&mut s.bytes, s.central[0] + 24, 10);
    assert_eq!(list(s.bytes), Err(Failure::Integrity));
}

#[test]
fn zip_link_and_special_entry_listing_does_not_authorize_extraction() {
    for (attributes, kind) in [(0xa000_0000, 2), (0x6000_0000, 4)] {
        let mut e = Spec::stored("a");
        e.attributes = attributes;
        let s = sample(&[e]);
        assert_eq!(list(s.bytes.clone()).unwrap()[0]["Entry"]["Kind"], kind);
        assert_eq!(failed_without_complete(s.bytes), Failure::UnsupportedEntry);
    }
}

#[test]
fn zip_reparse_directory_shape_and_nonempty_directory_rejected() {
    let mut reparse = Spec::stored("a");
    reparse.attributes = 0x400;
    let mut malformed_dir = Spec::empty("a");
    malformed_dir.attributes = 0x4000_0010;
    for e in [reparse, malformed_dir, Spec::stored("dir/")] {
        assert_eq!(
            failed_without_complete(sample(&[e]).bytes),
            Failure::UnsupportedEntry
        );
    }
}

#[test]
fn zip_output_budget_is_cumulative_across_entries() {
    let s = sample(&[Spec::stored("a"), Spec::stored("b")]);
    let mut l = limits();
    l.output = 17;
    let (result, output) = extract(s.bytes, l);
    assert_eq!(result, Err(Failure::Budget));
    let records = frame_records(&output);
    assert!(!records.iter().any(|r| r.kind == 5));
    assert!(
        records
            .iter()
            .filter(|r| r.kind == 3)
            .map(|r| r.payload.len())
            .sum::<usize>()
            <= 17
    );
}

#[test]
fn zip_deflate_malformed_payload_never_completes() {
    let mut e = Spec::deflate("a");
    e.packed = vec![7, 0];
    let failure = failed_single_item_without_end(sample(&[e]).bytes);
    // crate가 노출한 형식 오류만 확인함. 내부 파서 단계는 추정하지 않음.
    assert_eq!(failure, Failure::Integrity);
}

#[test]
fn zip_deflate_packed_tail_has_explicit_observation() {
    for tail in [vec![0xde, 0xad], vec![3, 0]] {
        let mut e = Spec::deflate("a");
        e.packed.extend(tail);
        let (result, output) = extract(sample(&[e]).bytes, limits());
        let records = frame_records(&output);
        match result {
            Ok(()) => {
                assert_eq!(validate_success(&records, &[DATA]), Ok(()));
                println!(
                    "OBSERVATION deflate-packed-tail: accepted; frozen C# comparison still required"
                );
            }
            Err(Failure::Integrity | Failure::MetadataMismatch | Failure::Truncated) => {
                assert!(!records.iter().any(|r| matches!(r.kind, 4 | 5)));
                println!(
                    "OBSERVATION deflate-packed-tail: rejected {result:?}; frozen C# comparison still required"
                );
            }
            Err(error) => panic!("정책 관측을 예산 또는 엔진 오류로 대체할 수 없음: {error:?}"),
        }
    }
}

#[test]
fn zip_matching_headers_with_overlapping_packed_ranges_rejected() {
    let mut s = sample(&[Spec::stored("first"), Spec::stored("second")]);
    assert!(list(s.bytes.clone()).is_ok());
    let packed_end = s.data[1] + DATA.len();
    let packed = (packed_end - s.data[0]) as u32;
    // 첫 데이터 범위가 두 번째 로컬 헤더와 데이터를 포함하게 함.
    for offset in [18, 22] {
        put32(&mut s.bytes, s.local[0] + offset, packed);
    }
    for offset in [20, 24] {
        put32(&mut s.bytes, s.central[0] + offset, packed);
    }
    for (index, name) in ["first", "second"].iter().enumerate() {
        let local = s.local[index];
        let central = s.central[index];
        assert_eq!(
            &s.bytes[local + 30..local + 30 + name.len()],
            name.as_bytes()
        );
        assert_eq!(
            &s.bytes[central + 46..central + 46 + name.len()],
            name.as_bytes()
        );
        for (l, c, width) in [
            (4, 6, 2),
            (6, 8, 2),
            (8, 10, 2),
            (14, 16, 4),
            (18, 20, 4),
            (22, 24, 4),
            (26, 28, 2),
        ] {
            assert_eq!(
                &s.bytes[local + l..local + l + width],
                &s.bytes[central + c..central + c + width]
            );
        }
        assert_eq!(
            u32::from_le_bytes(s.bytes[central + 42..central + 46].try_into().unwrap()) as usize,
            local
        );
    }
    assert!(s.data[0] < s.local[1] && s.local[1] < s.data[1]);
    assert!(s.data[1] < packed_end && packed_end == s.central[0]);
    assert_eq!(list(s.bytes.clone()), Err(Failure::Integrity));
    assert_eq!(failed_single_item_without_end(s.bytes), Failure::Integrity);
}

#[test]
fn zip_success_preserves_empty_items_between_payloads() {
    let entries = [
        Spec::stored("a"),
        Spec::empty("empty"),
        Spec::empty("dir/"),
        Spec::deflate("b"),
    ];
    successful_output(sample(&entries).bytes, &[DATA, &[], &[], DATA]);
}

#[test]
fn success_oracle_rejects_item_omission_ownership_length_and_order() {
    let (result, output) = extract(
        sample(&[Spec::stored("a"), Spec::empty("empty"), Spec::stored("b")]).bytes,
        limits(),
    );
    assert_eq!(result, Ok(()));
    let records = frame_records(&output);
    let expected: &[&[u8]] = &[DATA, &[], DATA];
    assert_eq!(validate_success(&records, expected), Ok(()));
    for mutation in 0..10 {
        let mut changed = records.clone();
        match mutation {
            0 => {
                changed.drain(4..6);
            }
            1 => changed[2].item_id[0] = 3,
            2 => changed[1].item_length = Some(8),
            3 => changed[3].item_length = Some(8),
            4 => changed.swap(2, 3),
            5 => changed[2].payload[0] ^= 1,
            6 => changed[2].payload_length -= 1,
            7 => {
                changed.pop();
            }
            8 => changed.push(records.last().unwrap().clone()),
            _ => changed[4].item_id[0] = 3,
        }
        assert!(
            validate_success(&changed, expected).is_err(),
            "mutation {mutation}"
        );
    }
}
