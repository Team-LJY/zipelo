use super::*;

fn sample_start() -> Start {
    Start {
        action: Action::Extract {
            archive: NativePath::Unix(b"/archive".to_vec()),
            destination: NativePath::Unix(b"/output".to_vec()),
            selection: Selection::All,
            list_id: None,
            gzip_output_name: None,
        },
        limits: Limits {
            input: 7,
            output: 8,
            entries: 9,
        },
    }
}

fn encoded(message: &Message) -> Vec<u8> {
    let mut bytes = Vec::new();
    Writer::new(&mut bytes, 17).send(message).unwrap();
    bytes
}

fn decoded(bytes: &[u8]) -> Message {
    let mut reader = Reader::new(bytes);
    let (job, sequence, message) = reader.read().unwrap().unwrap();
    assert_eq!((job, sequence), (17, 0));
    assert!(reader.read().unwrap().is_none());
    message
}

#[test]
fn interactive_start_preserves_legacy_body_and_frame_version() {
    let start = sample_start();
    let legacy = encoded(&Message::Start(start.clone()));
    let interactive = encoded(&Message::StartInteractive(start.clone()));
    let mut expected = legacy;
    expected[6..8].copy_from_slice(&4u16.to_le_bytes());
    assert_eq!(interactive, expected);
    assert_eq!(&interactive[..6], b"ZAW1\x01\x00");
    match decoded(&interactive) {
        Message::StartInteractive(actual) => assert_eq!(actual, start),
        other => panic!("unexpected frame: {other:?}"),
    }
}

#[test]
fn interactive_start_rejects_list_and_create_on_both_codec_directions() {
    for action in [
        Action::List {
            archive: NativePath::Unix(b"/archive".to_vec()),
        },
        Action::CreateZip {
            sources: Vec::new(),
            output_file: NativePath::Unix(b"/output.zip".to_vec()),
            method: Method::Store,
            encryption: Encryption::None,
        },
    ] {
        let start = Start {
            action,
            limits: Limits::default(),
        };
        let mut bytes = Vec::new();
        assert!(
            Writer::new(&mut bytes, 17)
                .send(&Message::StartInteractive(start.clone()))
                .is_err()
        );
        assert!(bytes.is_empty());
        let mut frame = encoded(&Message::Start(start));
        frame[6..8].copy_from_slice(&4u16.to_le_bytes());
        assert_eq!(
            Reader::new(frame.as_slice()).read().unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn conflict_reply_choices_and_apply_flag_have_stable_bytes() {
    for (choice, tag) in [
        (ConflictChoice::Overwrite, 0),
        (ConflictChoice::Skip, 1),
        (ConflictChoice::Rename, 2),
    ] {
        for apply_to_remaining in [false, true] {
            let frame = encoded(&Message::ConflictReply {
                challenge_id: 0x0807060504030201,
                choice,
                apply_to_remaining,
            });
            assert_eq!(&frame[6..12], &[5, 0, 10, 0, 0, 0]);
            assert_eq!(
                &frame[32..],
                &[1, 2, 3, 4, 5, 6, 7, 8, tag, u8::from(apply_to_remaining)]
            );
            match decoded(&frame) {
                Message::ConflictReply {
                    challenge_id,
                    choice: actual_choice,
                    apply_to_remaining: actual_apply,
                } => {
                    assert_eq!(challenge_id, 0x0807060504030201);
                    assert_eq!(actual_choice, choice);
                    assert_eq!(actual_apply, apply_to_remaining);
                }
                other => panic!("unexpected frame: {other:?}"),
            }
        }
    }
}

#[test]
fn conflict_metadata_round_trips_unicode_names_and_full_width_sizes() {
    let name = "자료/보고서.txt";
    let required = encoded(&Message::ConflictRequired {
        challenge_id: u64::MAX,
        entry_id: u32::MAX,
        relative_name: name.to_owned(),
        incoming_size: u64::MAX - 1,
        existing_size: u64::MAX - 2,
    });
    assert_eq!(&required[6..8], &108u16.to_le_bytes());
    match decoded(&required) {
        Message::ConflictRequired {
            challenge_id,
            entry_id,
            relative_name,
            incoming_size,
            existing_size,
        } => {
            assert_eq!(challenge_id, u64::MAX);
            assert_eq!(entry_id, u32::MAX);
            assert_eq!(relative_name, name);
            assert_eq!(incoming_size, u64::MAX - 1);
            assert_eq!(existing_size, u64::MAX - 2);
        }
        other => panic!("unexpected frame: {other:?}"),
    }
    for choice in [
        ConflictChoice::Overwrite,
        ConflictChoice::Skip,
        ConflictChoice::Rename,
    ] {
        let resolved = encoded(&Message::ConflictResolved {
            entry_id: 23,
            relative_name: "자료/보고서 (1).txt".to_owned(),
            choice,
        });
        assert_eq!(&resolved[6..8], &109u16.to_le_bytes());
        match decoded(&resolved) {
            Message::ConflictResolved {
                entry_id,
                relative_name,
                choice: actual,
            } => {
                assert_eq!(entry_id, 23);
                assert_eq!(relative_name, "자료/보고서 (1).txt");
                assert_eq!(actual, choice);
            }
            other => panic!("unexpected frame: {other:?}"),
        }
    }
}

#[test]
fn conflict_frames_reject_unknown_choices_flags_utf8_and_kinds() {
    let reply = encoded(&Message::ConflictReply {
        challenge_id: 1,
        choice: ConflictChoice::Skip,
        apply_to_remaining: false,
    });
    for (offset, value) in [(40, 3), (41, 2)] {
        let mut invalid = reply.clone();
        invalid[offset] = value;
        assert!(Reader::new(invalid.as_slice()).read().is_err());
    }
    let mut required = encoded(&Message::ConflictRequired {
        challenge_id: 1,
        entry_id: 2,
        relative_name: "a".to_owned(),
        incoming_size: 3,
        existing_size: 4,
    });
    required[48] = 0xff;
    assert!(Reader::new(required.as_slice()).read().is_err());
    let mut resolved = encoded(&Message::ConflictResolved {
        entry_id: 1,
        relative_name: "a".to_owned(),
        choice: ConflictChoice::Rename,
    });
    *resolved.last_mut().unwrap() = 3;
    assert!(Reader::new(resolved.as_slice()).read().is_err());
    let mut unknown = reply;
    unknown[6..8].copy_from_slice(&110u16.to_le_bytes());
    assert!(Reader::new(unknown.as_slice()).read().is_err());
}

#[test]
fn conflict_frames_reject_truncation_and_trailing_fields() {
    for message in [
        Message::StartInteractive(sample_start()),
        Message::ConflictReply {
            challenge_id: 1,
            choice: ConflictChoice::Overwrite,
            apply_to_remaining: true,
        },
        Message::ConflictRequired {
            challenge_id: 1,
            entry_id: 2,
            relative_name: "a".to_owned(),
            incoming_size: 3,
            existing_size: 4,
        },
        Message::ConflictResolved {
            entry_id: 2,
            relative_name: "a".to_owned(),
            choice: ConflictChoice::Skip,
        },
    ] {
        let frame = encoded(&message);
        for length in 1..frame.len() {
            assert!(Reader::new(&frame[..length]).read().is_err());
        }
        let mut trailing = frame;
        trailing.push(0);
        let length = (trailing.len() - 32) as u32;
        trailing[8..12].copy_from_slice(&length.to_le_bytes());
        assert!(Reader::new(trailing.as_slice()).read().is_err());
    }
}
