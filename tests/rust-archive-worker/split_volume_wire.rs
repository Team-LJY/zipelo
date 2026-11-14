use super::*;

fn frame(kind: u16, body: &[u8]) -> Vec<u8> {
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

fn reject_encode(message: &Message) {
    let mut bytes = Vec::new();
    assert_eq!(
        Writer::new(&mut bytes, 17)
            .send(message)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidData
    );
    assert!(bytes.is_empty());
}

fn reject_decode(bytes: &[u8]) {
    assert!(Reader::new(bytes).read().is_err());
}

fn identity() -> ListId {
    ListId {
        catalog_version: 2,
        format: Format::SevenZip,
        source_object: SourceObject::SplitSevenZip {
            manifest_sha256: [31; 32],
        },
        archive_bytes: 0x0102_0304_0506_0708,
        archive_sha256: [37; 32],
    }
}

fn identity_bytes(id: &ListId) -> Vec<u8> {
    let mut bytes = id.catalog_version.to_le_bytes().to_vec();
    bytes.push(match id.format {
        Format::Zip => 0,
        Format::Tar => 1,
        Format::Gzip => 2,
        Format::TarGzip => 3,
        Format::SevenZip => 4,
    });
    match id.source_object {
        SourceObject::Unix { device, inode } => {
            bytes.push(1);
            bytes.extend(device.to_le_bytes());
            bytes.extend(inode.to_le_bytes());
        }
        SourceObject::Windows {
            volume_serial,
            file_id,
        } => {
            bytes.push(2);
            bytes.extend(volume_serial.to_le_bytes());
            bytes.extend(file_id);
        }
        SourceObject::SplitSevenZip { manifest_sha256 } => {
            bytes.push(3);
            bytes.extend(manifest_sha256);
        }
    }
    bytes.extend(id.archive_bytes.to_le_bytes());
    bytes.extend(id.archive_sha256);
    bytes
}

fn list_end(id: &ListId) -> Vec<u8> {
    let mut body = 7u32.to_le_bytes().to_vec();
    body.extend(identity_bytes(id));
    frame(102, &body)
}

#[test]
fn split_list_identity_keeps_header_and_exact_digest_field_order() {
    let id = identity();
    let raw = list_end(&id);
    assert_eq!(raw.len(), 32 + 4 + 2 + 1 + 1 + 32 + 8 + 32);
    assert_eq!(&raw[36..40], &[2, 0, 4, 3]);
    assert_eq!(&raw[40..72], &[31; 32]);
    assert_eq!(&raw[72..80], &id.archive_bytes.to_le_bytes());
    assert_eq!(&raw[80..112], &[37; 32]);
    assert_eq!(
        encoded(&Message::ListEnd {
            entry_count: 7,
            list_id: id.clone()
        }),
        raw
    );
    match decoded(&raw) {
        Message::ListEnd {
            entry_count,
            list_id,
        } => {
            assert_eq!(entry_count, 7);
            assert_eq!(list_id, id);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn split_identity_is_preserved_in_all_existing_extract_requests() {
    for windows in [false, true] {
        for selection in [Selection::All, Selection::EntryIds(vec![2, 5])] {
            let start = Start {
                action: Action::Extract {
                    archive: if windows {
                        NativePath::Windows("C:\\set.7z.001".encode_utf16().collect())
                    } else {
                        NativePath::Unix(b"/set.7z.001".to_vec())
                    },
                    destination: if windows {
                        NativePath::Windows("C:\\output".encode_utf16().collect())
                    } else {
                        NativePath::Unix(b"/output".to_vec())
                    },
                    selection,
                    list_id: Some(identity()),
                    gzip_output_name: None,
                },
                limits: Limits::default(),
            };
            for kind in [1u16, 4, 6] {
                let message = match kind {
                    1 => Message::Start(start.clone()),
                    4 => Message::StartInteractive(start.clone()),
                    6 => Message::StartGuarded(GuardedStart {
                        start: start.clone(),
                        ceiling: start.limits,
                        interactive: true,
                    }),
                    _ => unreachable!(),
                };
                let bytes = encoded(&message);
                assert_eq!(&bytes[6..8], &kind.to_le_bytes());
                let actual = match decoded(&bytes) {
                    Message::Start(value) | Message::StartInteractive(value) => value,
                    Message::StartGuarded(value) => value.start,
                    other => panic!("{other:?}"),
                };
                assert_eq!(actual, start);
            }
        }
    }
}

#[test]
fn list_identity_accepts_only_agreed_version_format_and_object_combinations() {
    let objects = [
        SourceObject::Unix {
            device: 11,
            inode: 13,
        },
        SourceObject::Windows {
            volume_serial: 17,
            file_id: [19; 16],
        },
        SourceObject::SplitSevenZip {
            manifest_sha256: [23; 32],
        },
    ];
    for catalog_version in [0, 1, 2, 3, u16::MAX] {
        for format in [
            Format::Zip,
            Format::Tar,
            Format::Gzip,
            Format::TarGzip,
            Format::SevenZip,
        ] {
            for source_object in &objects {
                let id = ListId {
                    catalog_version,
                    format,
                    source_object: source_object.clone(),
                    ..identity()
                };
                let split = matches!(source_object, SourceObject::SplitSevenZip { .. });
                let valid = (catalog_version == 1 && !split)
                    || (catalog_version == 2 && format == Format::SevenZip && split);
                assert_eq!(id.valid(), valid, "{id:?}");
                let message = Message::ListEnd {
                    entry_count: 7,
                    list_id: id.clone(),
                };
                let raw = list_end(&id);
                if valid {
                    assert_eq!(encoded(&message), raw);
                    assert!(
                        matches!(decoded(&raw), Message::ListEnd { list_id, .. } if list_id == id)
                    );
                } else {
                    reject_encode(&message);
                    reject_decode(&raw);
                }
            }
        }
    }
}

#[test]
fn split_identity_rejects_unknown_tags_truncation_and_trailing_bytes() {
    let raw = list_end(&identity());
    for (offset, value) in [(38, 5), (39, 0), (39, 4)] {
        let mut invalid = raw.clone();
        invalid[offset] = value;
        reject_decode(&invalid);
    }
    for length in 32..raw.len() {
        reject_decode(&frame(102, &raw[32..length]));
    }
    let mut body = raw[32..].to_vec();
    body.push(0);
    reject_decode(&frame(102, &body));
}

fn volume_body(reason: u8, path: &NativePath) -> Vec<u8> {
    let mut body = vec![reason];
    match path {
        NativePath::Unix(bytes) => {
            body.push(1);
            body.extend((bytes.len() as u32).to_le_bytes());
            body.extend(bytes);
        }
        NativePath::Windows(units) => {
            body.push(2);
            body.extend((units.len() as u32).to_le_bytes());
            for unit in units {
                body.extend(unit.to_le_bytes());
            }
        }
    }
    body
}

#[test]
fn volume_problem_preserves_native_names_reason_tags_and_error_mapping() {
    for (tag, reason, error) in [
        (
            0,
            VolumeProblemReason::FirstVolumeRequired,
            ErrorCode::InvalidRequest,
        ),
        (
            1,
            VolumeProblemReason::MissingVolume,
            ErrorCode::MissingVolume,
        ),
        (2, VolumeProblemReason::IncompleteSet, ErrorCode::Integrity),
    ] {
        assert_eq!(reason.terminal_error(), error);
        for volume in [
            NativePath::Unix(b"set.7z.001".to_vec()),
            NativePath::Unix(b"\xff\x01.7z.009".to_vec()),
            NativePath::Windows("한글.7z.010".encode_utf16().collect()),
            NativePath::Windows(vec![0xd800, 46, 55, 122, 46, 48, 48, 49]),
        ] {
            assert!(volume.is_basename());
            let raw = frame(114, &volume_body(tag, &volume));
            assert_eq!(
                encoded(&Message::VolumeProblem {
                    reason,
                    volume: volume.clone()
                }),
                raw
            );
            match decoded(&raw) {
                Message::VolumeProblem {
                    reason: actual_reason,
                    volume: actual_volume,
                } => {
                    assert_eq!(actual_reason, reason);
                    assert_eq!(actual_volume, volume);
                }
                other => panic!("{other:?}"),
            }
        }
    }
}

#[test]
fn volume_problem_rejects_paths_nul_empty_and_parent_names_in_both_directions() {
    for name in [
        "",
        ".",
        "..",
        "/set.7z.001",
        "../set.7z.001",
        "a/set.7z.001",
        "a\\set.7z.001",
        "set\0.7z.001",
    ] {
        for volume in [
            NativePath::Unix(name.as_bytes().to_vec()),
            NativePath::Windows(name.encode_utf16().collect()),
        ] {
            assert!(!volume.is_basename(), "{volume:?}");
            reject_encode(&Message::VolumeProblem {
                reason: VolumeProblemReason::MissingVolume,
                volume: volume.clone(),
            });
            reject_decode(&frame(114, &volume_body(1, &volume)));
        }
    }
    for name in ["C:set.7z.001", "C:\\set.7z.001", "\\\\server\\set.7z.001"] {
        let volume = NativePath::Windows(name.encode_utf16().collect());
        assert!(!volume.is_basename());
        reject_encode(&Message::VolumeProblem {
            reason: VolumeProblemReason::MissingVolume,
            volume: volume.clone(),
        });
        reject_decode(&frame(114, &volume_body(1, &volume)));
    }
}

#[test]
fn volume_problem_keeps_native_path_limits_and_strict_body_lengths() {
    for count in [32_768, 32_769] {
        for volume in [
            NativePath::Unix(vec![b'a'; count]),
            NativePath::Windows(vec![b'a' as u16; count]),
        ] {
            let message = Message::VolumeProblem {
                reason: VolumeProblemReason::IncompleteSet,
                volume: volume.clone(),
            };
            let raw = frame(114, &volume_body(2, &volume));
            if count == 32_768 {
                assert_eq!(encoded(&message), raw);
                assert!(matches!(decoded(&raw), Message::VolumeProblem { .. }));
                assert!(raw.len() - 32 < zipelo_archive_worker::wire::MAX_BODY);
            } else {
                reject_encode(&message);
                reject_decode(&raw);
            }
        }
    }
    let body = volume_body(0, &NativePath::Unix(b"set.7z.001".to_vec()));
    for length in 0..body.len() {
        reject_decode(&frame(114, &body[..length]));
    }
    for (offset, value) in [(0, 3), (1, 0), (1, 3)] {
        let mut invalid = body.clone();
        invalid[offset] = value;
        reject_decode(&frame(114, &invalid));
    }
    let mut trailing = body.clone();
    trailing.push(0);
    reject_decode(&frame(114, &trailing));
    let mut huge = body;
    huge[2..6].copy_from_slice(&u32::MAX.to_le_bytes());
    reject_decode(&frame(114, &huge));
}

#[test]
fn missing_volume_terminal_uses_tag_14_and_rejects_unknown_tag_17() {
    let message = Message::Terminal(Terminal {
        outcome: Outcome::Failed,
        error: ErrorCode::MissingVolume,
        publication: Publication::None,
        created_count: 0,
        retained_partial_count: 0,
        durability: Durability::NotApplicable,
        cleanup: Cleanup::Complete,
        detail: String::new(),
    });
    let mut bytes = encoded(&message);
    assert_eq!(&bytes[6..8], &106u16.to_le_bytes());
    assert_eq!(bytes[33], 14);
    assert!(matches!(decoded(&bytes), Message::Terminal(t) if t.error == ErrorCode::MissingVolume));
    bytes[33] = 17;
    reject_decode(&bytes);
}
