use super::*;
use sha2::{Digest, Sha256};

fn fixture(format: &str, name: &str, bytes: &[u8]) -> (Scratch, String, ListId, u32) {
    let s = Scratch::new();
    fs::write(s.0.join("payload"), bytes).unwrap();
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/rust-archive-worker/preview_cases.py");
    let result = Command::new("python3")
        .arg("-B")
        .arg(script)
        .arg(&s.0)
        .arg(format)
        .arg(name)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let path = if format == "gzip" {
        format!("{name}.gz")
    } else {
        "input".into()
    };
    let events = execute(Start {
        action: Action::List {
            archive: s.path(&path),
        },
        limits: Limits::default(),
    });
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    let id = events
        .iter()
        .find_map(|e| match e {
            Message::Entry(entry) if entry.name == name || format == "gzip" => Some(entry.entry_id),
            _ => None,
        })
        .unwrap();
    let list = events
        .into_iter()
        .find_map(|e| match e {
            Message::ListEnd { list_id, .. } => Some(list_id),
            _ => None,
        })
        .unwrap();
    (s, path, list, id)
}
fn request(s: &Scratch, path: &str, list_id: ListId, entry_id: u32) -> GuardedStart {
    GuardedStart {
        start: Start {
            action: Action::Preview {
                archive: s.path(path),
                entry_id,
                list_id,
            },
            limits: Limits::default(),
        },
        ceiling: Limits::default(),
        interactive: true,
    }
}
fn run(s: &Scratch, start: GuardedStart) -> Vec<Message> {
    guarded::run(s, start, |event| match event {
        Message::ResourceRequired {
            challenge_id,
            scope,
            ..
        } => Some(vec![Message::ResourceReply {
            challenge_id: *challenge_id,
            scope: *scope,
            allow: true,
        }]),
        Message::PasswordRequired { challenge_id, .. } => Some(vec![Message::Password {
            challenge_id: *challenge_id,
            secret: Secret(b"fixture-password".to_vec()),
        }]),
        _ => None,
    })
}
fn result(events: &[Message]) -> (PreviewInfo, Vec<u8>) {
    let terminal = terminal(events);
    assert_eq!(terminal.outcome, Outcome::Completed, "{events:?}");
    assert_eq!(terminal.publication, Publication::None);
    assert_eq!(terminal.created_count, 0);
    assert_eq!(terminal.cleanup, Cleanup::Complete);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Message::Output { .. } | Message::ConflictRequired { .. }))
    );
    let mut info = None;
    let mut data = Vec::new();
    let mut ended = false;
    for event in events {
        match event {
            Message::PreviewBegin(value) => {
                assert!(info.replace(*value).is_none());
                assert!(value.valid());
            }
            Message::PreviewChunk {
                entry_id,
                offset,
                data: bytes,
            } => {
                assert!(!ended);
                assert_eq!(Some(*entry_id), info.map(|i| i.entry_id));
                assert_eq!(*offset, data.len() as u64);
                assert!(bytes.len() <= MAX_PREVIEW_CHUNK_BYTES);
                data.extend(bytes);
            }
            Message::PreviewEnd {
                entry_id,
                total_bytes,
                sha256,
            } => {
                assert!(!ended);
                ended = true;
                let info = info.unwrap();
                assert_eq!(*entry_id, info.entry_id);
                assert_eq!(*total_bytes, info.total_bytes);
                assert_eq!(*total_bytes, data.len() as u64);
                assert_eq!(*sha256, <[u8; 32]>::from(Sha256::digest(&data)));
            }
            _ => {}
        }
    }
    assert!(ended);
    (info.unwrap(), data)
}
fn no_result(events: &[Message], error: ErrorCode) {
    assert_eq!(terminal(events).outcome, Outcome::Failed, "{events:?}");
    assert_eq!(terminal(events).error, error, "{events:?}");
    assert_eq!(terminal(events).publication, Publication::None);
    assert_eq!(terminal(events).created_count, 0);
    assert!(!events.iter().any(|e| matches!(
        e,
        Message::PreviewBegin(_)
            | Message::PreviewChunk { .. }
            | Message::PreviewEnd { .. }
            | Message::Output { .. }
    )));
}
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_image_data(&vec![20; (width * height * 4) as usize])
        .unwrap();
    drop(writer);
    bytes
}
#[test]
fn preview_actual_formats_keep_source_and_publish_nothing() {
    for format in [
        "zip",
        "zip-selected",
        "zip-selected-damaged",
        "7z-folders",
        "tar",
        "gz",
        "bz2",
        "xz",
        "gzip",
        "7z-copy",
        "7z-lzma",
        "7z-lzma2",
        "aes",
    ] {
        let expected = "가나다\n".repeat(10000);
        let (s, path, list, id) = fixture(format, "payload.txt", expected.as_bytes());
        let original = fs::read(s.0.join(&path)).unwrap();
        let (_, data) = result(&run(&s, request(&s, &path, list, id)));
        assert_eq!(data, expected.as_bytes(), "{format}");
        assert_eq!(fs::read(s.0.join(&path)).unwrap(), original);
        assert_eq!(fs::read_dir(&s.0).unwrap().count(), 2);
    }
}
#[test]
fn preview_actual_png_and_jpeg_have_exact_rgba_metadata() {
    for (name, bytes, width, height) in [
        ("payload.png", png(3, 2), 3, 2),
        (
            "payload.jpg",
            include_bytes!("fixtures/preview-baseline.jpg").to_vec(),
            3,
            2,
        ),
        (
            "payload.jpeg",
            include_bytes!("fixtures/preview-progressive.jpg").to_vec(),
            3,
            2,
        ),
    ] {
        for format in ["zip", "tar", "7z-lzma2"] {
            let (s, path, list, id) = fixture(format, name, &bytes);
            let (info, data) = result(&run(&s, request(&s, &path, list, id)));
            assert_eq!(
                (info.width, info.height, info.total_bytes),
                (width, height, u64::from(width * height * 4))
            );
            assert_eq!(data.len(), (width * height * 4) as usize);
        }
    }
}
#[test]
fn preview_own_limit_is_not_expanded_by_permission() {
    for (name, bytes) in [
        (
            "payload.txt",
            vec![b'x'; MAX_PREVIEW_TEXT_BYTES as usize + 1],
        ),
        ("payload.png", vec![0; MAX_PREVIEW_IMAGE_BYTES as usize + 1]),
    ] {
        let (s, path, list, id) = fixture("zip", name, &bytes);
        let mut start = request(&s, &path, list, id);
        start.ceiling.output = 64 * 1024 * 1024 * 1024;
        let events = run(&s, start);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Message::ResourceRequired { .. }))
        );
        no_result(&events, ErrorCode::Limit);
    }
    let expected = vec![b'x'; MAX_PREVIEW_TEXT_BYTES as usize];
    let (s, path, list, id) = fixture("zip", "payload.txt", &expected);
    assert_eq!(result(&run(&s, request(&s, &path, list, id))).1, expected);
}
#[test]
fn preview_rejects_corruption_unsupported_and_stale_selection() {
    for (name, bytes, error) in [
        ("payload.txt", vec![0xff], ErrorCode::Integrity),
        (
            "payload.svg",
            b"<svg/>".to_vec(),
            ErrorCode::UnsupportedFeature,
        ),
        (
            "payload.png",
            b"GIF89a".to_vec(),
            ErrorCode::UnsupportedFormat,
        ),
        ("payload.jpg", vec![0xff, 0xd8], ErrorCode::Integrity),
    ] {
        let (s, path, list, id) = fixture("zip", name, &bytes);
        no_result(&run(&s, request(&s, &path, list, id)), error);
    }
    let (s, path, list, id) = fixture("zip", "payload.txt", b"text");
    let mut changed = list.clone();
    changed.archive_sha256[0] ^= 1;
    no_result(
        &run(&s, request(&s, &path, changed, id)),
        ErrorCode::SourceChanged,
    );
    no_result(
        &run(&s, request(&s, &path, list, u32::MAX)),
        ErrorCode::InvalidRequest,
    );
}
#[test]
fn preview_solid_budget_still_applies_to_preceding_content() {
    let (s, path, list, id) = fixture("7z-lzma2", "payload.txt", b"text");
    let mut start = request(&s, &path, list, id);
    start.start.limits.output = 100_000;
    start.ceiling = start.start.limits;
    no_result(&run(&s, start), ErrorCode::Limit);
}
#[test]
fn preview_cancels_before_decode_and_during_chunk_transmission() {
    for stage in [0, 1, 2] {
        let (s, path, list, id) = fixture("zip", "payload.png", &png(4000, 4000));
        let mut cancelled = false;
        let events = guarded::run(&s, request(&s, &path, list, id), |event| {
            let target = match stage {
                0 => matches!(event, Message::Accepted { .. }),
                1 => matches!(
                    event,
                    Message::Progress {
                        stage: Stage::Decode,
                        ..
                    }
                ),
                _ => matches!(event, Message::PreviewChunk { .. }),
            };
            if target && !cancelled {
                cancelled = true;
                Some(vec![Message::Cancel])
            } else {
                None
            }
        });
        assert!(cancelled);
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Cancelled,
            "stage={stage}"
        );
        assert_eq!(terminal(&events).publication, Publication::None);
        assert_eq!(terminal(&events).cleanup, Cleanup::Complete);
        assert!(!events.iter().any(|e| matches!(e, Message::Output { .. })));
    }
}

#[test]
fn preview_password_retry_and_cancel_never_publish_plaintext() {
    let (s, path, list, id) = fixture("aes", "payload.txt", b"secret preview");
    let mut attempts = Vec::new();
    let events = guarded::run(
        &s,
        request(&s, &path, list.clone(), id),
        |event| match event {
            Message::PasswordRequired {
                challenge_id,
                attempt,
                ..
            } => {
                attempts.push(*attempt);
                Some(vec![Message::Password {
                    challenge_id: *challenge_id,
                    secret: Secret(if *attempt == 1 {
                        b"wrong".to_vec()
                    } else {
                        b"fixture-password".to_vec()
                    }),
                }])
            }
            _ => None,
        },
    );
    assert_eq!(attempts, [1, 2]);
    assert_eq!(result(&events).1, b"secret preview");
    let events = guarded::run(&s, request(&s, &path, list, id), |event| {
        if matches!(event, Message::PasswordRequired { .. }) {
            Some(vec![Message::Cancel])
        } else {
            None
        }
    });
    assert_eq!(terminal(&events).outcome, Outcome::Cancelled);
    assert!(!events.iter().any(|e| matches!(
        e,
        Message::PreviewBegin(_)
            | Message::PreviewChunk { .. }
            | Message::PreviewEnd { .. }
            | Message::Output { .. }
    )));
}

#[test]
fn preview_empty_text_uses_begin_and_end_without_chunks() {
    for format in ["zip", "tar", "7z-lzma2", "gzip"] {
        let (s, path, list, id) = fixture(format, "payload.txt", b"");
        let events = run(&s, request(&s, &path, list, id));
        assert!(result(&events).1.is_empty());
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Message::PreviewChunk { .. }))
        );
    }
}
#[test]
fn preview_rejects_directory_link_and_changed_source_object() {
    for format in ["tar-folder", "tar-link"] {
        let (s, path, list, id) = fixture(format, "payload.txt", b"");
        no_result(
            &run(&s, request(&s, &path, list, id)),
            ErrorCode::InvalidRequest,
        );
    }
    let (s, path, list, id) = fixture("zip", "payload.txt", b"original");
    fs::write(s.0.join(&path), b"changed").unwrap();
    no_result(
        &run(&s, request(&s, &path, list, id)),
        ErrorCode::SourceChanged,
    );
    assert_eq!(fs::read(s.0.join(&path)).unwrap(), b"changed");
}
