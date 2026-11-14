use super::*;
use sha2::{Digest, Sha256};

fn list_id(windows: bool) -> ListId {
    ListId {
        catalog_version: 1,
        format: Format::Zip,
        source_object: if windows {
            SourceObject::Windows {
                volume_serial: 19,
                file_id: [23; 16],
            }
        } else {
            SourceObject::Unix {
                device: 19,
                inode: 23,
            }
        },
        archive_bytes: 37,
        archive_sha256: [41; 32],
    }
}

fn guarded(windows: bool) -> GuardedStart {
    GuardedStart {
        start: Start {
            action: Action::Preview {
                archive: if windows {
                    NativePath::Windows("C:\\archive.zip".encode_utf16().collect())
                } else {
                    NativePath::Unix(b"/archive.zip".to_vec())
                },
                entry_id: 13,
                list_id: list_id(windows),
            },
            limits: Limits::default(),
        },
        ceiling: Limits::default(),
        interactive: true,
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

fn raw(kind: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = b"ZAW1".to_vec();
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(kind.to_le_bytes());
    bytes.extend((body.len() as u32).to_le_bytes());
    bytes.extend([0; 4]);
    bytes.extend(17u64.to_le_bytes());
    bytes.extend(0u64.to_le_bytes());
    bytes.extend(body);
    bytes
}

fn metadata(info: PreviewInfo) -> Vec<u8> {
    let mut body = info.entry_id.to_le_bytes().to_vec();
    body.push(match info.format {
        PreviewFormat::Utf8Text => 0,
        PreviewFormat::Rgba8 => 1,
    });
    body.extend(info.total_bytes.to_le_bytes());
    body.extend(info.width.to_le_bytes());
    body.extend(info.height.to_le_bytes());
    raw(111, &body)
}

fn reject_encode(message: Message) {
    let mut bytes = Vec::new();
    assert_eq!(
        Writer::new(&mut bytes, 17)
            .send(&message)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidData
    );
    assert!(bytes.is_empty());
}

fn reject_decode(bytes: &[u8]) {
    assert!(Reader::new(bytes).read().is_err());
}

#[test]
fn preview_guarded_request_preserves_identity_and_field_order() {
    for windows in [false, true] {
        let start = guarded(windows);
        assert!(start.valid());
        assert_eq!(start.start.action.kind(), ActionKind::Preview);
        let bytes = encoded(&Message::StartGuarded(start.clone()));
        assert_eq!(&bytes[..8], b"ZAW1\x01\x00\x06\x00");
        assert_eq!(bytes[32], 3);
        let (path_bytes, object_bytes) = if windows {
            ("C:\\archive.zip".encode_utf16().count() * 2, 1 + 8 + 16)
        } else {
            (b"/archive.zip".len(), 1 + 8 + 8)
        };
        let id_at = 32 + 1 + 1 + 4 + path_bytes;
        assert_eq!(&bytes[id_at..id_at + 4], &13u32.to_le_bytes());
        assert_eq!(&bytes[id_at + 4..id_at + 7], &[1, 0, 0]);
        let hash_at = id_at + 4 + 2 + 1 + object_bytes + 8;
        assert_eq!(&bytes[hash_at..hash_at + 32], &[41; 32]);
        match decoded(&bytes) {
            Message::StartGuarded(actual) => assert_eq!(actual, start),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn preview_requires_guarded_interactive_requests_in_both_directions() {
    let start = guarded(false);
    reject_encode(Message::Start(start.start.clone()));
    reject_encode(Message::StartInteractive(start.start.clone()));
    let bytes = encoded(&Message::StartGuarded(start.clone()));
    let tail = 8 + 8 + 4 + 1;
    for kind in [1, 4] {
        reject_decode(&raw(kind, &bytes[32..bytes.len() - tail]));
    }
    let mut noninteractive = start.clone();
    noninteractive.interactive = false;
    assert!(!noninteractive.valid());
    reject_encode(Message::StartGuarded(noninteractive));
    let mut bytes = bytes;
    *bytes.last_mut().unwrap() = 0;
    reject_decode(&bytes);
}

#[test]
fn preview_request_rejects_missing_or_malformed_list_identity() {
    for windows in [false, true] {
        let bytes = encoded(&Message::StartGuarded(guarded(windows)));
        for length in 33..bytes.len() {
            reject_decode(&raw(6, &bytes[32..length]));
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        let length = (trailing.len() - 32) as u32;
        trailing[8..12].copy_from_slice(&length.to_le_bytes());
        reject_decode(&trailing);
    }
}

#[test]
fn preview_metadata_enforces_text_dimension_and_pixel_boundaries() {
    for (format, width, height, total_bytes, valid) in [
        (PreviewFormat::Utf8Text, 0, 0, 0, true),
        (PreviewFormat::Utf8Text, 0, 0, MAX_PREVIEW_TEXT_BYTES, true),
        (
            PreviewFormat::Utf8Text,
            0,
            0,
            MAX_PREVIEW_TEXT_BYTES + 1,
            false,
        ),
        (PreviewFormat::Utf8Text, 1, 0, 0, false),
        (PreviewFormat::Utf8Text, 0, 1, 0, false),
        (PreviewFormat::Rgba8, 1, 1, 4, true),
        (PreviewFormat::Rgba8, 8192, 1, 8192 * 4, true),
        (PreviewFormat::Rgba8, 1, 8192, 8192 * 4, true),
        (PreviewFormat::Rgba8, 8193, 1, 8193 * 4, false),
        (PreviewFormat::Rgba8, 1, 8193, 8193 * 4, false),
        (PreviewFormat::Rgba8, 4000, 4000, 64_000_000, true),
        (PreviewFormat::Rgba8, 4001, 4000, 64_016_000, false),
        (PreviewFormat::Rgba8, 8192, 8192, 268_435_456, false),
        (PreviewFormat::Rgba8, 0, 1, 0, false),
        (PreviewFormat::Rgba8, 1, 0, 0, false),
        (PreviewFormat::Rgba8, 1, 1, 3, false),
        (PreviewFormat::Rgba8, 1, 1, 5, false),
        (PreviewFormat::Rgba8, u32::MAX, u32::MAX, u64::MAX, false),
    ] {
        let info = PreviewInfo {
            entry_id: 13,
            format,
            total_bytes,
            width,
            height,
        };
        assert_eq!(info.valid(), valid, "{info:?}");
        let raw = metadata(info);
        if valid {
            assert_eq!(encoded(&Message::PreviewBegin(info)), raw);
            match decoded(&raw) {
                Message::PreviewBegin(actual) => assert_eq!(actual, info),
                other => panic!("{other:?}"),
            }
        } else {
            reject_encode(Message::PreviewBegin(info));
            reject_decode(&raw);
        }
    }
}

#[test]
fn preview_metadata_rejects_unknown_format_and_truncated_frames() {
    let bytes = metadata(PreviewInfo {
        entry_id: 13,
        format: PreviewFormat::Rgba8,
        total_bytes: 4,
        width: 1,
        height: 1,
    });
    let mut unknown = bytes.clone();
    unknown[36] = 2;
    reject_decode(&unknown);
    for length in 1..bytes.len() {
        reject_decode(&bytes[..length]);
    }
    let mut oversized = bytes;
    oversized[8..12]
        .copy_from_slice(&((zipelo_archive_worker::wire::MAX_BODY + 1) as u32).to_le_bytes());
    reject_decode(&oversized);
}

#[test]
fn preview_chunks_have_bounded_raw_bytes_and_exact_offsets() {
    for length in [1, MAX_PREVIEW_CHUNK_BYTES] {
        for offset in [0, MAX_PREVIEW_RGBA_BYTES - length as u64] {
            let data = (0..length).map(|i| (i % 256) as u8).collect::<Vec<_>>();
            let bytes = encoded(&Message::PreviewChunk {
                entry_id: 13,
                offset,
                data: data.clone(),
            });
            let mut body = 13u32.to_le_bytes().to_vec();
            body.extend(offset.to_le_bytes());
            body.extend((length as u32).to_le_bytes());
            body.extend(&data);
            assert_eq!(bytes, raw(112, &body));
            assert!(bytes.len() - 32 < zipelo_archive_worker::wire::MAX_BODY);
            match decoded(&bytes) {
                Message::PreviewChunk {
                    entry_id,
                    offset: actual,
                    data: result,
                } => {
                    assert_eq!(entry_id, 13);
                    assert_eq!(actual, offset);
                    assert_eq!(result, data);
                }
                other => panic!("{other:?}"),
            }
        }
    }
}

#[test]
fn preview_chunks_reject_empty_oversized_overflow_and_short_payloads() {
    for (offset, length) in [
        (0, 0),
        (0, MAX_PREVIEW_CHUNK_BYTES + 1),
        (MAX_PREVIEW_RGBA_BYTES, 1),
        (u64::MAX, 1),
    ] {
        reject_encode(Message::PreviewChunk {
            entry_id: 13,
            offset,
            data: vec![0; length],
        });
        let mut body = 13u32.to_le_bytes().to_vec();
        body.extend(offset.to_le_bytes());
        body.extend((length as u32).to_le_bytes());
        body.extend(vec![0; length]);
        reject_decode(&raw(112, &body));
    }
    for declared in [1, MAX_PREVIEW_CHUNK_BYTES as u32, u32::MAX] {
        let mut body = 13u32.to_le_bytes().to_vec();
        body.extend(0u64.to_le_bytes());
        body.extend(declared.to_le_bytes());
        reject_decode(&raw(112, &body));
    }
    let mut body = 13u32.to_le_bytes().to_vec();
    body.extend(0u64.to_le_bytes());
    body.extend(1u32.to_le_bytes());
    body.extend([0, 0]);
    reject_decode(&raw(112, &body));
}

#[test]
fn preview_end_preserves_total_hash_and_global_result_bound() {
    for total_bytes in [0, MAX_PREVIEW_TEXT_BYTES, MAX_PREVIEW_RGBA_BYTES] {
        let bytes = encoded(&Message::PreviewEnd {
            entry_id: 13,
            total_bytes,
            sha256: [47; 32],
        });
        let mut body = 13u32.to_le_bytes().to_vec();
        body.extend(total_bytes.to_le_bytes());
        body.extend([47; 32]);
        assert_eq!(bytes, raw(113, &body));
        match decoded(&bytes) {
            Message::PreviewEnd {
                entry_id,
                total_bytes: actual,
                sha256,
            } => {
                assert_eq!(entry_id, 13);
                assert_eq!(actual, total_bytes);
                assert_eq!(sha256, [47; 32]);
            }
            other => panic!("{other:?}"),
        }
        for length in 32..bytes.len() {
            reject_decode(&raw(113, &bytes[32..length]));
        }
    }
    reject_encode(Message::PreviewEnd {
        entry_id: 13,
        total_bytes: MAX_PREVIEW_RGBA_BYTES + 1,
        sha256: [0; 32],
    });
    let mut body = 13u32.to_le_bytes().to_vec();
    body.extend((MAX_PREVIEW_RGBA_BYTES + 1).to_le_bytes());
    body.extend([0; 32]);
    reject_decode(&raw(113, &body));
}

#[test]
fn preview_stream_carries_utf8_split_inside_a_code_point() {
    let text = "a".repeat(MAX_PREVIEW_CHUNK_BYTES - 1) + "가";
    let info = PreviewInfo {
        entry_id: 13,
        format: PreviewFormat::Utf8Text,
        total_bytes: text.len() as u64,
        width: 0,
        height: 0,
    };
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes, 17);
    writer.send(&Message::PreviewBegin(info)).unwrap();
    for (index, chunk) in text.as_bytes().chunks(MAX_PREVIEW_CHUNK_BYTES).enumerate() {
        writer
            .send(&Message::PreviewChunk {
                entry_id: 13,
                offset: (index * MAX_PREVIEW_CHUNK_BYTES) as u64,
                data: chunk.to_vec(),
            })
            .unwrap();
    }
    let expected: [u8; 32] = Sha256::digest(text.as_bytes()).into();
    writer
        .send(&Message::PreviewEnd {
            entry_id: 13,
            total_bytes: info.total_bytes,
            sha256: expected,
        })
        .unwrap();
    let mut reader = Reader::new(bytes.as_slice());
    assert!(
        matches!(reader.read().unwrap().unwrap().2, Message::PreviewBegin(actual) if actual == info)
    );
    let mut result = Vec::new();
    for sequence in 1..=2 {
        let (job, actual_sequence, message) = reader.read().unwrap().unwrap();
        assert_eq!((job, actual_sequence), (17, sequence));
        let Message::PreviewChunk {
            entry_id,
            offset,
            data,
        } = message
        else {
            panic!("{message:?}");
        };
        assert_eq!(entry_id, 13);
        assert_eq!(offset, result.len() as u64);
        assert!(std::str::from_utf8(&data).is_err());
        result.extend(data);
    }
    assert_eq!(String::from_utf8(result).unwrap(), text);
    match reader.read().unwrap().unwrap().2 {
        Message::PreviewEnd {
            entry_id,
            total_bytes,
            sha256,
        } => {
            assert_eq!(entry_id, 13);
            assert_eq!(total_bytes, info.total_bytes);
            assert_eq!(sha256, expected);
        }
        other => panic!("{other:?}"),
    }
    assert!(reader.read().unwrap().is_none());
}

#[test]
fn preview_action_kind_keeps_all_existing_tags() {
    for (action, tag) in [
        (ActionKind::List, 0),
        (ActionKind::Extract, 1),
        (ActionKind::CreateZip, 2),
        (ActionKind::Preview, 3),
    ] {
        let bytes = encoded(&Message::Accepted {
            action,
            encryption: true,
        });
        assert_eq!(bytes, raw(100, &[tag, 1]));
        assert!(
            matches!(decoded(&bytes), Message::Accepted { action: actual, encryption: true } if actual == action)
        );
    }
    reject_decode(&raw(100, &[5, 1]));
    reject_decode(&raw(8, &[]));
    reject_decode(&raw(114, &[]));
}
