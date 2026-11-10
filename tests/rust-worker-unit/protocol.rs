use std::io::{Cursor, Read, Seek, SeekFrom};

use serde_json::json;

use crate::{
    budget::CHUNK,
    error::Failure,
    support::*,
    wire::{self, Frames, Remote},
};

#[test]
fn guid_reference_mixed_endian_and_item_ordinal() {
    assert_eq!(wire::parse_id(GUID).unwrap(), ID);
    assert_eq!(wire::parse_id(&GUID.to_ascii_uppercase()).unwrap(), ID);
    assert_eq!(
        wire::item_id(0),
        [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(&wire::item_id(1023)[..4], &[0, 4, 0, 0]);
}

#[test]
fn guid_empty_malformed_and_nonascii_rejected() {
    for value in [
        "00000000-0000-0000-0000-000000000000",
        "00112233445566778899aabbccddeeff",
        "00112233_4455-6677-8899-aabbccddeeff",
        "00112233-4455-6677-8899-aabbccddeefg",
        "",
        "00112233-4455-6677-8899-aabbccddee한",
    ] {
        assert_eq!(wire::parse_id(value), Err(Failure::Protocol));
    }
}

#[test]
fn config_reference_exact_fields() {
    let c = wire::read_config(&mut Cursor::new(config_bytes())).unwrap();
    assert_eq!(c.id, ID);
    assert_eq!(c.length, 22);
    assert_eq!(c.limits.entries, 1024);
    assert_eq!(c.limits.processed, 134_217_728);
    assert_eq!(c.limits.output, 16_777_216);
    assert_eq!(c.limits.duration.as_nanos(), 300_000_000_000);
}

#[test]
fn config_every_truncated_prefix_rejected() {
    let bytes = config_bytes();
    for length in 0..64 {
        assert!(matches!(
            wire::read_config(&mut Cursor::new(&bytes[..length])),
            Err(Failure::Truncated)
        ));
    }
}

#[test]
fn config_magic_version_and_zero_job_rejected() {
    for offset in [0, 4, 8] {
        let mut bytes = config_bytes();
        if offset == 8 {
            bytes[8..24].fill(0);
        } else {
            bytes[offset] ^= 1;
        }
        assert!(matches!(
            wire::read_config(&mut Cursor::new(bytes)),
            Err(Failure::Protocol)
        ));
    }
}

#[test]
fn config_unsigned_negative_length_and_ticks_rejected() {
    for (offset, values) in [
        (24, vec![0, 21, 16_777_217, u64::MAX]),
        (56, vec![0, 3_000_000_001, u64::MAX]),
    ] {
        for value in values {
            let mut bytes = config_bytes();
            put64(&mut bytes, offset, value);
            assert!(matches!(
                wire::read_config(&mut Cursor::new(bytes)),
                Err(Failure::Protocol)
            ));
        }
    }
}

#[test]
fn config_streaming_limit_caps_rejected() {
    for (offset, value) in [(32, 134_217_729), (40, 16_777_217), (48, 1025)] {
        let mut bytes = config_bytes();
        put64(&mut bytes, offset, value);
        assert!(matches!(
            wire::read_config(&mut Cursor::new(bytes)),
            Err(Failure::Budget)
        ));
    }
}

#[test]
fn remote_exact_request_sequence_seek_and_eof() {
    let responses = [reply(0, 0, b"ab"), reply(1, 21, b"z")].concat();
    let capture = Capture::default();
    let mut remote = Remote::new(Cursor::new(responses), capture.clone(), &config(22));
    let mut first = [0; 2];
    remote.read_exact(&mut first).unwrap();
    assert_eq!(&first, b"ab");
    assert_eq!(remote.seek(SeekFrom::End(-1)).unwrap(), 21);
    assert_eq!(remote.read(&mut first).unwrap(), 1);
    assert_eq!(first[0], b'z');
    assert_eq!(remote.read(&mut first).unwrap(), 0);
    assert_eq!(
        *capture.0.borrow(),
        [
            reply(0, 0, b"ab")[..48].to_vec(),
            reply(1, 21, b"z")[..48].to_vec()
        ]
        .concat()
    );
}

#[test]
fn remote_every_response_identity_field_rejected() {
    for offset in [0, 4, 8, 24, 32, 40, 44] {
        let mut bytes = reply(0, 0, b"x");
        bytes[offset] ^= 1;
        let mut remote = Remote::new(Cursor::new(bytes), Vec::new(), &config(22));
        assert_eq!(io_failure(remote.read(&mut [0; 1])), Failure::Protocol);
    }
}

#[test]
fn remote_truncated_header_and_body_are_eof() {
    let bytes = reply(0, 0, b"xy");
    for length in 0..50 {
        let mut remote = Remote::new(Cursor::new(&bytes[..length]), Vec::new(), &config(22));
        assert_eq!(io_failure(remote.read(&mut [0; 2])), Failure::Truncated);
    }
}

#[test]
fn remote_empty_read_and_invalid_seeks_do_not_request() {
    let capture = Capture::default();
    let mut remote = Remote::new(Cursor::new(Vec::<u8>::new()), capture.clone(), &config(22));
    assert_eq!(remote.read(&mut []).unwrap(), 0);
    for from in [
        SeekFrom::Start(u64::MAX),
        SeekFrom::Current(i64::MIN),
        SeekFrom::End(i64::MAX),
    ] {
        assert_eq!(io_failure(remote.seek(from)), Failure::Protocol);
    }
    remote.seek(SeekFrom::End(0)).unwrap();
    assert_eq!(remote.read(&mut [0; 1]).unwrap(), 0);
    assert!(capture.0.borrow().is_empty());
}

#[test]
fn remote_chunk_is_capped_at_64k() {
    let bytes = vec![7; CHUNK];
    let capture = Capture::default();
    let mut remote = Remote::new(
        Cursor::new(reply(0, 0, &bytes)),
        capture.clone(),
        &config(100_000),
    );
    let mut buffer = vec![0; CHUNK + 1];
    assert_eq!(remote.read(&mut buffer).unwrap(), CHUNK);
    assert_eq!(buffer[CHUNK], 0);
    assert_eq!(&capture.0.borrow()[40..44], &65_536u32.to_le_bytes());
}

#[test]
fn remote_request_count_exact_cap_then_no_request() {
    let bytes: Vec<u8> = (0..65_536)
        .flat_map(|sequence| reply(sequence, 0, b"x"))
        .collect();
    let capture = Capture::default();
    let mut remote = Remote::new(Cursor::new(bytes), capture.clone(), &config(22));
    for _ in 0..65_536 {
        remote.seek(SeekFrom::Start(0)).unwrap();
        assert_eq!(remote.read(&mut [0; 1]).unwrap(), 1);
    }
    let before = capture.0.borrow().len();
    assert_eq!(io_failure(remote.read(&mut [0; 1])), Failure::Budget);
    assert_eq!(capture.0.borrow().len(), before);
}

#[test]
fn remote_cumulative_bytes_cap_with_repeated_ranges() {
    // 각 응답을 순서대로 생성해 큰 응답 사본을 보관하지 않음.
    struct Replies {
        sequence: u64,
        current: Cursor<Vec<u8>>,
    }
    impl Read for Replies {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.current.position() as usize == self.current.get_ref().len() {
                self.current = Cursor::new(reply(self.sequence, 0, &vec![0; CHUNK]));
                self.sequence += 1;
            }
            self.current.read(out)
        }
    }
    let capture = Capture::default();
    let mut remote = Remote::new(
        Replies {
            sequence: 0,
            current: Cursor::new(Vec::new()),
        },
        capture.clone(),
        &config(CHUNK as u64),
    );
    let mut buffer = [0; CHUNK];
    for _ in 0..768 {
        remote.seek(SeekFrom::Start(0)).unwrap();
        remote.read_exact(&mut buffer).unwrap();
    }
    remote.seek(SeekFrom::Start(0)).unwrap();
    let before = capture.0.borrow().len();
    assert_eq!(io_failure(remote.read(&mut [0; 1])), Failure::Budget);
    assert_eq!(capture.0.borrow().len(), before);
}

#[test]
fn frames_normal_sequence_ids_and_terminal() {
    let capture = Capture::default();
    let mut frames = Frames::new(capture.clone(), ID, budget()).unwrap();
    frames.begin().unwrap();
    frames.begin_item(0, 2).unwrap();
    frames.data(b"ok").unwrap();
    frames.end_item().unwrap();
    frames.complete().unwrap();
    let before = capture.0.borrow().clone();
    assert_eq!(
        frame_shapes(&frame_records(&before)),
        vec![
            (1, vec![]),
            (2, 2u64.to_le_bytes().to_vec()),
            (3, b"ok".to_vec()),
            (4, 2u64.to_le_bytes().to_vec()),
            (5, vec![1]),
        ]
    );
    assert_eq!(&before[32..48], &[0; 16]);
    assert_eq!(
        &before[64 + 32..64 + 48],
        &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(frames.complete(), Err(Failure::Budget));
    assert_eq!(frames.begin_item(1, 0), Err(Failure::Budget));
    assert_eq!(*capture.0.borrow(), before);
}

#[test]
fn frames_transition_and_order_rejections() {
    let capture = Capture::default();
    let mut frames = Frames::new(capture.clone(), ID, budget()).unwrap();
    assert_eq!(frames.complete(), Err(Failure::Protocol));
    assert_eq!(frames.begin_item(0, 0), Err(Failure::Protocol));
    assert_eq!(frames.data(b"x"), Err(Failure::Protocol));
    assert_eq!(frames.end_item(), Err(Failure::Protocol));
    assert!(capture.0.borrow().is_empty());
    frames.begin().unwrap();
    assert_eq!(frames.begin(), Err(Failure::Protocol));
    assert_eq!(frames.begin_item(1, 0), Err(Failure::Protocol));
    frames.begin_item(0, 0).unwrap();
    assert_eq!(frames.begin_item(1, 0), Err(Failure::Protocol));
    assert_eq!(frames.complete(), Err(Failure::Protocol));
    frames.end_item().unwrap();
    frames.complete().unwrap();
}

#[test]
fn frames_data_length_rejected_without_writing_success() {
    for data in [vec![], vec![0; 4], vec![0; CHUNK + 1]] {
        let capture = Capture::default();
        let mut frames = Frames::new(capture.clone(), ID, budget()).unwrap();
        frames.begin().unwrap();
        frames.begin_item(0, 3).unwrap();
        let before = capture.0.borrow().len();
        assert_eq!(frames.data(&data), Err(Failure::Integrity));
        assert_eq!(frames.end_item(), Err(Failure::Integrity));
        assert_eq!(frames.complete(), Err(Failure::Protocol));
        assert_eq!(capture.0.borrow().len(), before);
    }
}

#[test]
fn frames_exact_chunk_and_zero_size_item() {
    let capture = Capture::default();
    let mut frames = Frames::new(capture.clone(), ID, budget()).unwrap();
    frames.begin().unwrap();
    frames.begin_item(0, CHUNK as u64).unwrap();
    frames.data(&vec![0; CHUNK]).unwrap();
    frames.end_item().unwrap();
    frames.begin_item(1, 0).unwrap();
    frames.end_item().unwrap();
    frames.complete().unwrap();
    assert_eq!(
        frame_records(&capture.0.borrow())
            .iter()
            .map(|x| x.kind)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 2, 4, 5]
    );
}

#[test]
fn frames_sequence_cap_prevents_completed_frame() {
    let capture = Capture::default();
    let mut frames = Frames::new(capture.clone(), ID, budget()).unwrap();
    frames.begin().unwrap();
    frames.begin_item(0, 65_534).unwrap();
    for _ in 0..65_534 {
        frames.data(b"x").unwrap();
    }
    let before = capture.0.borrow().len();
    assert_eq!(frames.end_item(), Err(Failure::Budget));
    assert_eq!(frames.complete(), Err(Failure::Protocol));
    assert_eq!(capture.0.borrow().len(), before);
}

#[test]
fn json_frame_invalid_length_reads_header_only() {
    for length in [i32::MIN, -1, 0, 33_554_433, i32::MAX] {
        let (mut input, calls) = Observed::new(length.to_le_bytes().to_vec());
        let result = json_result(&mut input);
        assert_eq!(result["Job"]["State"], 7);
        assert_eq!(result["Job"]["Error"], 7);
        assert_eq!(&*calls.borrow(), &[4]);
    }
}

#[test]
fn json_truncated_frame_and_trailing_input_rejected() {
    let bytes = json_bytes(&request(&sample(&[]).bytes));
    for length in [0, 1, 3, 4, bytes.len() - 1] {
        let value = json_result(&mut Cursor::new(&bytes[..length]));
        assert_eq!(value["Job"]["State"], 7);
        assert_eq!(value["Job"]["Error"], 10);
    }
    let mut trailing = bytes;
    trailing.push(0);
    let value = json_result(&mut Cursor::new(trailing));
    assert_eq!(value["Job"]["State"], 7);
    assert_eq!(value["Job"]["Error"], 12);
}

#[test]
fn json_pascalcase_numeric_enums_and_listing_crc_status() {
    let mut req = request(&sample(&[Spec::stored("a")]).bytes);
    req["JobId"] = json!(GUID.to_ascii_uppercase());
    let value = json_result(&mut Cursor::new(json_bytes(&req)));
    assert_eq!(value["Job"]["JobId"], GUID);
    assert_eq!(value["Job"]["State"], 4);
    assert_eq!(value["Job"]["Error"], 0);
    assert_eq!(value["Job"]["Entries"][0]["Entry"]["Kind"], 0);
    assert_eq!(value["Job"]["Entries"][0]["Verification"], 0);
    assert_eq!(value["Probe"]["ContentCrcVerified"], false);
    assert!(value.get("job").is_none());
}

#[test]
fn json_action_enum_and_field_case_rejected() {
    for (action, code) in [
        (json!(1), 2),
        (json!(-1), 2),
        (json!("List"), 12),
        (json!(0.5), 12),
    ] {
        let mut req = request(&sample(&[]).bytes);
        req["Action"] = action;
        let value = json_result(&mut Cursor::new(json_bytes(&req)));
        assert_eq!(value["Job"]["State"], 7);
        assert_eq!(value["Job"]["Error"], code);
    }
    let mut req = request(&sample(&[]).bytes);
    let action = req.as_object_mut().unwrap().remove("Action").unwrap();
    req["action"] = action;
    assert_eq!(
        json_result(&mut Cursor::new(json_bytes(&req)))["Job"]["Error"],
        12
    );
}

#[test]
fn json_limit_integer_overflow_and_invalid_job_rejected() {
    for value in [json!(0), json!(-1), json!(u64::MAX), json!("1024")] {
        let mut req = request(&sample(&[]).bytes);
        req["Limits"]["MaxEntries"] = value;
        assert_eq!(
            json_result(&mut Cursor::new(json_bytes(&req)))["Job"]["Error"],
            12
        );
    }
    let mut req = request(&sample(&[]).bytes);
    req["JobId"] = json!("00000000-0000-0000-0000-000000000000");
    assert_eq!(
        json_result(&mut Cursor::new(json_bytes(&req)))["Job"]["State"],
        7
    );
}

#[test]
fn failure_codes_and_actual_eof_classification() {
    let cases = [
        (Failure::Unsupported, 10, 2),
        (Failure::UnsupportedEntry, 11, 4),
        (Failure::Integrity, 12, 10),
        (Failure::Budget, 13, 7),
        (Failure::Protocol, 14, 12),
        (Failure::Truncated, 15, 10),
        (Failure::Engine, 16, 12),
        (Failure::DuplicateName, 17, 12),
        (Failure::MetadataMismatch, 18, 10),
    ];
    for (failure, exit, archive) in cases {
        assert_eq!(failure.exit_code(), exit);
        assert_eq!(failure.archive_code(), archive);
    }
    assert_eq!(
        Failure::from(std::io::Error::from(std::io::ErrorKind::UnexpectedEof)),
        Failure::Truncated
    );
    assert_eq!(
        Failure::from(std::io::Error::from(std::io::ErrorKind::InvalidData)),
        Failure::Integrity
    );
}
