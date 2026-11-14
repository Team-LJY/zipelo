use super::*;
use sha2::{Digest, Sha256};

fn directory(s: &Scratch, name: &str) {
    let path = s.0.join(name);
    fs::create_dir_all(&path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut current = path.as_path();
        while current != s.0 {
            fs::set_permissions(current, fs::Permissions::from_mode(0o700)).unwrap();
            current = current.parent().unwrap();
        }
    }
}
fn pieces(s: &Scratch, bytes: &[u8], ends: &[usize]) -> Vec<PathBuf> {
    let mut begin = 0;
    let mut paths = Vec::new();
    for (index, end) in ends.iter().copied().chain(Some(bytes.len())).enumerate() {
        assert!(begin <= end && end <= bytes.len());
        let path = s.0.join(format!("sample.7z.{:03}", index + 1));
        fs::write(&path, &bytes[begin..end]).unwrap();
        paths.push(path);
        begin = end;
    }
    paths
}
fn setup(s: &Scratch, count: usize, fixture: &str) -> (Vec<u8>, Vec<PathBuf>) {
    let bytes = seven_fixture(fixture);
    let ends = (1..count)
        .map(|i| i * bytes.len() / count)
        .collect::<Vec<_>>();
    let paths = pieces(s, &bytes, &ends);
    (bytes, paths)
}
fn list(s: &Scratch) -> Vec<Message> {
    execute(Start {
        action: Action::List {
            archive: s.path("sample.7z.001"),
        },
        limits: Limits::default(),
    })
}
fn id(events: &[Message]) -> ListId {
    assert_eq!(terminal(events).outcome, Outcome::Completed, "{events:?}");
    events
        .iter()
        .find_map(|event| match event {
            Message::ListEnd { list_id, .. } => Some(list_id.clone()),
            _ => None,
        })
        .unwrap()
}
fn request(s: &Scratch, list_id: Option<ListId>, selection: Selection) -> Start {
    Start {
        action: Action::Extract {
            archive: s.path("sample.7z.001"),
            destination: s.path("output"),
            selection,
            list_id,
            gzip_output_name: None,
        },
        limits: Limits::default(),
    }
}
fn failure(events: &[Message], error: ErrorCode) {
    assert_eq!(terminal(events).outcome, Outcome::Failed, "{events:?}");
    assert_eq!(terminal(events).error, error, "{events:?}");
    assert_eq!(terminal(events).created_count, 0, "{events:?}");
    assert_eq!(
        terminal(events).publication,
        Publication::None,
        "{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Message::Output { .. } | Message::ListEnd { .. })),
        "{events:?}"
    );
}
fn hashes(paths: &[PathBuf]) -> Vec<[u8; 32]> {
    paths
        .iter()
        .map(|p| Sha256::digest(fs::read(p).unwrap()).into())
        .collect()
}
fn problem(events: &[Message], reason: VolumeProblemReason, leaf: &str) {
    let problems = events
        .iter()
        .filter_map(|event| match event {
            Message::VolumeProblem { reason, volume } => Some((reason, volume)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(problems.len(), 1, "{events:?}");
    assert_eq!(
        problems[0],
        (&reason, &NativePath::from_path(std::path::Path::new(leaf)))
    );
    let accepted = events
        .iter()
        .position(|event| matches!(event, Message::Accepted { .. }))
        .unwrap();
    let reported = events
        .iter()
        .position(|event| matches!(event, Message::VolumeProblem { .. }))
        .unwrap();
    assert!(accepted < reported && reported < events.len() - 1);
}

#[test]
fn headers_and_data_cross_variable_part_boundaries_for_reference_archives() {
    for fixture in [
        "copy.7z",
        "lzma.7z",
        "lzma2.7z",
        "libarchive-lzma1.7z",
        "libarchive-lzma2.7z",
    ] {
        let s = Scratch::new();
        let bytes = seven_fixture(fixture);
        let paths = pieces(&s, &bytes, &[1, 6, 13, 31, 32, 40]);
        let before = hashes(&paths);
        let listed = list(&s);
        let token = id(&listed);
        assert_eq!(token.catalog_version, 2);
        assert_eq!(token.format, Format::SevenZip);
        assert!(matches!(
            token.source_object,
            SourceObject::SplitSevenZip { .. }
        ));
        assert_eq!(token.archive_bytes, bytes.len() as u64);
        assert_eq!(
            token.archive_sha256,
            <[u8; 32]>::from(Sha256::digest(&bytes))
        );
        directory(&s, "output");
        let result = execute(request(&s, Some(token), Selection::EntryIds(vec![0])));
        assert_eq!(
            terminal(&result).outcome,
            Outcome::Completed,
            "{fixture}: {result:?}"
        );
        assert_eq!(
            fs::read(s.0.join("output/folder/hello.txt")).unwrap(),
            SEVEN_PAYLOAD.repeat(100)
        );
        assert_eq!(hashes(&paths), before);
    }
}

#[test]
fn numeric_order_crosses_009_010_and_accepts_exactly_999_parts() {
    for count in [11, 999] {
        let s = Scratch::new();
        let (bytes, paths) = setup(&s, count, "copy.7z");
        let before = hashes(&paths);
        let token = id(&list(&s));
        assert_eq!(token.archive_bytes, bytes.len() as u64);
        directory(&s, "output");
        let result = execute(request(&s, Some(token), Selection::All));
        assert_eq!(
            terminal(&result).outcome,
            Outcome::Completed,
            "{count}: {result:?}"
        );
        assert_eq!(
            fs::read(s.0.join("output/folder/hello.txt")).unwrap(),
            SEVEN_PAYLOAD.repeat(100)
        );
        assert_eq!(hashes(&paths), before);
        fs::write(s.0.join("sample.7z.1000"), [0]).unwrap();
        failure(&list(&s), ErrorCode::Limit);
    }
}

#[test]
fn nonfirst_selection_and_definite_middle_gaps_name_the_exact_required_part() {
    let s = Scratch::new();
    let (_, paths) = setup(&s, 11, "copy.7z");
    let result = execute(Start {
        action: Action::List {
            archive: s.path("sample.7z.002"),
        },
        limits: Limits::default(),
    });
    failure(&result, ErrorCode::InvalidRequest);
    problem(
        &result,
        VolumeProblemReason::FirstVolumeRequired,
        "sample.7z.001",
    );
    fs::remove_file(&paths[8]).unwrap();
    let result = list(&s);
    failure(&result, ErrorCode::MissingVolume);
    problem(&result, VolumeProblemReason::MissingVolume, "sample.7z.009");
}

#[test]
fn missing_tail_truncated_empty_and_extra_bytes_never_guess_a_missing_name() {
    for mode in 0..4 {
        let s = Scratch::new();
        let (_, paths) = setup(&s, 3, "copy.7z");
        match mode {
            0 => fs::remove_file(&paths[2]).unwrap(),
            1 => {
                let mut bytes = fs::read(&paths[2]).unwrap();
                bytes.pop();
                fs::write(&paths[2], bytes).unwrap();
            }
            2 => fs::write(&paths[1], []).unwrap(),
            _ => {
                let mut bytes = fs::read(&paths[2]).unwrap();
                bytes.push(0);
                fs::write(&paths[2], bytes).unwrap();
            }
        }
        let result = list(&s);
        failure(&result, ErrorCode::Integrity);
        problem(&result, VolumeProblemReason::IncompleteSet, "sample.7z.001");
    }
}

#[test]
fn detectable_mixed_reordered_and_corrupt_payloads_never_publish() {
    for mode in 0..3 {
        let s = Scratch::new();
        let (_, paths) = setup(&s, 11, "copy.7z");
        if mode == 0 {
            let a = fs::read(&paths[4]).unwrap();
            let b = fs::read(&paths[5]).unwrap();
            fs::write(&paths[4], b).unwrap();
            fs::write(&paths[5], a).unwrap();
        } else {
            let mut bytes = fs::read(&paths[4]).unwrap();
            if mode == 1 {
                bytes.fill(0x5a);
            } else {
                bytes[0] ^= 1;
            }
            fs::write(&paths[4], bytes).unwrap();
        }
        directory(&s, "output");
        let result = execute(request(&s, None, Selection::All));
        failure(&result, ErrorCode::Integrity);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
    }
}

#[test]
fn changing_middle_tail_length_object_or_count_after_listing_invalidates_the_token() {
    for mode in 0..8 {
        let s = Scratch::new();
        let (_, paths) = setup(&s, 3, "copy.7z");
        let token = id(&list(&s));
        let selected = if mode == 0 { &paths[1] } else { &paths[2] };
        match mode {
            0 | 1 => {
                let mut data = fs::read(selected).unwrap();
                data[0] ^= 1;
                fs::write(selected, data).unwrap();
            }
            2 => {
                let data = fs::read(selected).unwrap();
                fs::rename(selected, s.0.join("old-part")).unwrap();
                fs::write(selected, data).unwrap();
            }
            3 => {
                let mut data = fs::read(selected).unwrap();
                data.pop();
                fs::write(selected, data).unwrap();
            }
            4 => fs::remove_file(selected).unwrap(),
            5 => fs::write(s.0.join("sample.7z.004"), [0]).unwrap(),
            6 => fs::remove_file(&paths[1]).unwrap(),
            _ => {
                for path in &paths {
                    fs::remove_file(path).unwrap();
                }
            }
        }
        directory(&s, "output");
        let result = execute(request(&s, Some(token), Selection::All));
        failure(
            &result,
            if mode == 6 {
                ErrorCode::MissingVolume
            } else {
                ErrorCode::SourceChanged
            },
        );
        if mode == 6 {
            problem(&result, VolumeProblemReason::MissingVolume, "sample.7z.002");
        } else {
            assert!(
                !result
                    .iter()
                    .any(|event| matches!(event, Message::VolumeProblem { .. }))
            );
        }
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
    }
}

#[test]
fn replacement_or_gap_during_conflict_wait_is_rechecked_before_first_file_commit() {
    for mode in 0..3 {
        let s = Scratch::new();
        let (_, paths) = setup(&s, 3, "copy.7z");
        let token = id(&list(&s));
        directory(&s, "output/folder");
        fs::write(s.0.join("output/folder/hello.txt"), b"keep").unwrap();
        let mut changed = false;
        let result = guarded::run(
            &s,
            GuardedStart {
                start: request(&s, Some(token), Selection::All),
                ceiling: Limits::default(),
                interactive: true,
            },
            |event| {
                if let Message::ConflictRequired { challenge_id, .. } = event {
                    changed = true;
                    if mode == 2 {
                        fs::remove_file(&paths[1]).unwrap();
                    } else {
                        let target = &paths[if mode == 0 { 1 } else { 2 }];
                        let mut bytes = fs::read(target).unwrap();
                        bytes[0] ^= 1;
                        fs::write(target, bytes).unwrap();
                    }
                    return Some(vec![Message::ConflictReply {
                        challenge_id: *challenge_id,
                        choice: ConflictChoice::Overwrite,
                        apply_to_remaining: false,
                    }]);
                }
                None
            },
        );
        assert!(changed, "{result:?}");
        failure(
            &result,
            if mode == 2 {
                ErrorCode::MissingVolume
            } else {
                ErrorCode::SourceChanged
            },
        );
        assert_eq!(
            fs::read(s.0.join("output/folder/hello.txt")).unwrap(),
            b"keep"
        );
        assert_eq!(terminal(&result).cleanup, Cleanup::Complete);
        assert_eq!(fs::read_dir(s.0.join("output/folder")).unwrap().count(), 1);
    }
}

#[test]
fn split_capture_uses_aggregate_input_budget_and_existing_output_permission() {
    for allow in [false, true] {
        let s = Scratch::new();
        let (bytes, paths) = setup(&s, 11, "copy.7z");
        let before = hashes(&paths);
        directory(&s, "output");
        let mut start = request(&s, None, Selection::All);
        start.limits.input = bytes.len() as u64;
        start.limits.output = 1;
        let ceiling = Limits {
            input: bytes.len() as u64,
            output: 8192,
            ..Limits::default()
        };
        let mut questions = 0;
        let result = guarded::run(
            &s,
            GuardedStart {
                start,
                ceiling,
                interactive: true,
            },
            |event| {
                if let Message::ResourceRequired {
                    challenge_id,
                    scope,
                    ..
                } = event
                {
                    questions += 1;
                    return Some(vec![Message::ResourceReply {
                        challenge_id: *challenge_id,
                        scope: *scope,
                        allow,
                    }]);
                }
                None
            },
        );
        assert_eq!(questions, 1);
        assert_eq!(
            terminal(&result).outcome,
            if allow {
                Outcome::Completed
            } else {
                Outcome::Cancelled
            },
            "{result:?}"
        );
        if allow {
            assert_eq!(
                fs::read(s.0.join("output/folder/hello.txt")).unwrap(),
                SEVEN_PAYLOAD.repeat(100)
            );
        } else {
            assert_eq!(terminal(&result).created_count, 0);
            assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
        }
        assert_eq!(hashes(&paths), before);
    }
    let s = Scratch::new();
    let (bytes, _) = setup(&s, 11, "copy.7z");
    directory(&s, "output");
    let mut start = request(&s, None, Selection::All);
    start.limits.input = bytes.len() as u64 - 1;
    failure(&execute(start), ErrorCode::Limit);
}

#[test]
fn cancellation_at_conflict_keeps_source_and_existing_output() {
    let s = Scratch::new();
    let (_, paths) = setup(&s, 3, "copy.7z");
    let before = hashes(&paths);
    directory(&s, "output/folder");
    fs::write(s.0.join("output/folder/hello.txt"), b"keep").unwrap();
    let result = guarded::run(
        &s,
        GuardedStart {
            start: request(&s, None, Selection::All),
            ceiling: Limits::default(),
            interactive: true,
        },
        |event| matches!(event, Message::ConflictRequired { .. }).then(|| vec![Message::Cancel]),
    );
    assert_eq!(terminal(&result).outcome, Outcome::Cancelled);
    assert_eq!(terminal(&result).created_count, 0);
    assert_eq!(
        fs::read(s.0.join("output/folder/hello.txt")).unwrap(),
        b"keep"
    );
    assert_eq!(hashes(&paths), before);
}

#[test]
fn split_preview_and_excluded_zip_volume_names_are_not_retried_as_single_inputs() {
    let s = Scratch::new();
    let (_, paths) = setup(&s, 3, "copy.7z");
    let token = id(&list(&s));
    let preview = Start {
        action: Action::Preview {
            archive: s.path("sample.7z.001"),
            entry_id: 0,
            list_id: token,
        },
        limits: Limits::default(),
    };
    let result = guarded::run(
        &s,
        GuardedStart {
            start: preview,
            ceiling: Limits::default(),
            interactive: true,
        },
        |_| None,
    );
    failure(&result, ErrorCode::UnsupportedFeature);
    assert!(
        !result
            .iter()
            .any(|event| matches!(event, Message::PreviewBegin(_)))
    );
    for name in ["sample.zip.001", "sample.z01"] {
        fs::write(s.0.join(name), include_bytes!("fixtures/store.zip")).unwrap();
        let result = execute(Start {
            action: Action::List {
                archive: s.path(name),
            },
            limits: Limits::default(),
        });
        failure(&result, ErrorCode::UnsupportedFeature);
    }
    assert_eq!(paths.len(), 3);
}

fn reference_fixture(s: &Scratch, name: &str) -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/rust-archive-worker/fixtures/split7z");
    let mut paths = Vec::new();
    for index in 1..=999 {
        let source = root.join(format!("{name}.7z.{index:03}"));
        match fs::read(source) {
            Ok(data) => {
                let destination = s.0.join(format!("sample.7z.{index:03}"));
                fs::write(&destination, data).unwrap();
                paths.push(destination);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => panic!("{error:?}"),
        }
    }
    assert!(paths.len() > 1);
    paths
}

#[test]
fn seven_zip_created_volumes_list_and_extract_with_matching_reference_hashes() {
    let s = Scratch::new();
    let paths = reference_fixture(&s, "reference");
    let before = hashes(&paths);
    let token = id(&list(&s));
    directory(&s, "output");
    let result = execute(request(&s, Some(token), Selection::All));
    assert_eq!(terminal(&result).outcome, Outcome::Completed, "{result:?}");
    let expected = SEVEN_PAYLOAD.repeat(100);
    let actual = fs::read(s.0.join("output/folder/hello.txt")).unwrap();
    assert_eq!(Sha256::digest(&actual), Sha256::digest(&expected));
    assert_eq!(actual, expected);
    let expected = (0..4096usize)
        .map(|index| ((index * 37 + (index >> 3)) % 256) as u8)
        .collect::<Vec<_>>();
    let actual = fs::read(s.0.join("output/second.bin")).unwrap();
    assert_eq!(Sha256::digest(&actual), Sha256::digest(&expected));
    assert_eq!(actual, expected);
    assert_eq!(hashes(&paths), before);
}

#[test]
fn encrypted_seven_zip_volumes_keep_the_existing_unsupported_policy() {
    for name in ["encrypted-header", "encrypted-body"] {
        let s = Scratch::new();
        let paths = reference_fixture(&s, name);
        let before = hashes(&paths);
        directory(&s, "output");
        let result = execute(request(&s, None, Selection::All));
        failure(&result, ErrorCode::UnsupportedFeature);
        assert!(
            !result
                .iter()
                .any(|event| matches!(event, Message::PasswordRequired { .. }))
        );
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
        assert_eq!(hashes(&paths), before);
    }
}
