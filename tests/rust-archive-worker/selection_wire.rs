use super::*;

const ARCHIVE: &[u8] = b"/archive";
const DESTINATION: &[u8] = b"/destination";

fn request(count: usize, kind: u16, ceiling: u32) -> Message {
    let start = Start {
        action: Action::Extract {
            archive: NativePath::Unix(ARCHIVE.to_vec()),
            destination: NativePath::Unix(DESTINATION.to_vec()),
            selection: Selection::EntryIds((0..count as u32).collect()),
            list_id: None,
            gzip_output_name: None,
        },
        limits: Limits::default(),
    };
    match kind {
        1 => Message::Start(start),
        4 => Message::StartInteractive(start),
        6 => Message::StartGuarded(GuardedStart {
            ceiling: Limits {
                entries: ceiling,
                ..start.limits
            },
            start,
            interactive: true,
        }),
        _ => unreachable!(),
    }
}

fn encoded(message: &Message) -> Vec<u8> {
    let mut frame = Vec::new();
    Writer::new(&mut frame, 17).send(message).unwrap();
    frame
}

fn resize_body(frame: &mut [u8]) {
    let length = (frame.len() - 32) as u32;
    frame[8..12].copy_from_slice(&length.to_le_bytes());
}

fn ids_offset() -> usize {
    32 + 1 + 1 + 4 + ARCHIVE.len() + 1 + 4 + DESTINATION.len() + 1
}

fn unchecked_frame(count: usize, kind: u16, ceiling: u32) -> Vec<u8> {
    let mut frame = encoded(&request(0, kind, ceiling));
    let at = ids_offset();
    let mut ids = (count as u32).to_le_bytes().to_vec();
    for id in 0..count as u32 {
        ids.extend(id.to_le_bytes());
    }
    frame.splice(at..at + 4, ids);
    resize_body(&mut frame);
    frame
}

fn boundary(count: usize, kind: u16, ceiling: u32, accepted: bool) {
    let mut frame = Vec::new();
    let result = Writer::new(&mut frame, 17).send(&request(count, kind, ceiling));
    assert_eq!(result.is_ok(), accepted, "encode {kind} {ceiling} {count}");
    if !accepted {
        assert!(frame.is_empty());
    }
    let raw = unchecked_frame(count, kind, ceiling);
    let result = Reader::new(raw.as_slice()).read();
    assert_eq!(result.is_ok(), accepted, "decode {kind} {ceiling} {count}");
    if accepted {
        assert_eq!(frame, raw);
        assert!(frame.len() - 32 <= MAX_BODY);
        let (_, _, decoded) = result.unwrap().unwrap();
        let start = match decoded {
            Message::Start(start) | Message::StartInteractive(start) => start,
            Message::StartGuarded(start) => start.start,
            other => panic!("{other:?}"),
        };
        let Action::Extract {
            selection: Selection::EntryIds(ids),
            ..
        } = start.action
        else {
            panic!("missing selection");
        };
        assert_eq!(ids.len(), count);
        assert!(
            ids.iter()
                .enumerate()
                .all(|(index, id)| *id as usize == index)
        );
    }
}

#[test]
fn expanded_selection_round_trips_exact_policy_boundaries() {
    for count in [32_768, 32_769, 200_000, 200_001] {
        boundary(count, 6, 200_000, count <= 200_000);
    }
}

#[test]
fn legacy_and_unexpanded_requests_keep_the_original_selection_bound() {
    for kind in [1, 4, 6] {
        for count in [32_768, 32_769, 200_000, 200_001] {
            boundary(count, kind, 10_000, count <= 32_768);
        }
    }
}

#[test]
fn guarded_selection_honors_lower_ceilings_and_rejects_truncated_counts() {
    boundary(40_000, 6, 40_000, true);
    boundary(40_001, 6, 40_000, false);
    let mut short = unchecked_frame(0, 6, 200_000);
    short[ids_offset()..ids_offset() + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Reader::new(short.as_slice()).read().is_err());
    let mut frame = unchecked_frame(200_000, 6, 200_000);
    frame.truncate(frame.len() - 1);
    assert!(Reader::new(frame.as_slice()).read().is_err());
    resize_body(&mut frame);
    assert!(Reader::new(frame.as_slice()).read().is_err());
    let mut invalid_ceiling = unchecked_frame(32_769, 6, 200_000);
    let at = invalid_ceiling.len() - 5;
    invalid_ceiling[at..at + 4].copy_from_slice(&200_001u32.to_le_bytes());
    assert!(Reader::new(invalid_ceiling.as_slice()).read().is_err());
}

#[test]
fn expanded_selection_does_not_expand_paths_sources_or_frame_bounds() {
    for path in [
        NativePath::Unix(vec![b'a'; 32_769]),
        NativePath::Windows(vec![b'a' as u16; 32_769]),
    ] {
        let Message::StartGuarded(mut guarded) = request(0, 6, 200_000) else {
            unreachable!();
        };
        if let Action::Extract { archive, .. } = &mut guarded.start.action {
            *archive = path;
        }
        assert!(
            Writer::new(Vec::new(), 17)
                .send(&Message::StartGuarded(guarded))
                .is_err()
        );
    }
    let mut long_path = unchecked_frame(0, 6, 200_000);
    let at = 32 + 1 + 1;
    let mut value = 32_769u32.to_le_bytes().to_vec();
    value.extend(vec![b'a'; 32_769]);
    long_path.splice(at..at + 4 + ARCHIVE.len(), value);
    resize_body(&mut long_path);
    assert!(Reader::new(long_path.as_slice()).read().is_err());

    let source = Source {
        path: NativePath::Unix(b"/s".to_vec()),
        archive_name: "s".into(),
    };
    let create = |sources| {
        Message::StartGuarded(GuardedStart {
            start: Start {
                action: Action::CreateZip {
                    sources,
                    output_file: NativePath::Unix(b"/new.zip".to_vec()),
                    method: Method::Store,
                    encryption: Encryption::None,
                },
                limits: Limits::default(),
            },
            ceiling: Limits {
                entries: 200_000,
                ..Limits::default()
            },
            interactive: false,
        })
    };
    assert!(
        Writer::new(Vec::new(), 17)
            .send(&create(vec![source; 32_769]))
            .is_err()
    );
    let mut sources = encoded(&create(Vec::new()));
    let mut value = 32_769u32.to_le_bytes().to_vec();
    for _ in 0..32_769 {
        value.extend([1, 2, 0, 0, 0, b'/', b's', 1, 0, 0, 0, b's']);
    }
    sources.splice(33..37, value);
    resize_body(&mut sources);
    assert!(Reader::new(sources.as_slice()).read().is_err());

    let mut oversized = unchecked_frame(0, 6, 200_000);
    oversized[8..12].copy_from_slice(&((MAX_BODY + 1) as u32).to_le_bytes());
    assert!(Reader::new(oversized.as_slice()).read().is_err());
}
