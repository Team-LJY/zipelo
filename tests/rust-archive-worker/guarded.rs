use super::*;
use std::{io::Read, process::Child, sync::mpsc, time::Duration};

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn run(
    s: &Scratch,
    start: GuardedStart,
    respond: impl FnMut(&Message) -> Option<Vec<Message>>,
) -> Vec<Message> {
    run_fault(s, start, None, respond)
}
fn run_fault(
    s: &Scratch,
    start: GuardedStart,
    fault: Option<&str>,
    respond: impl FnMut(&Message) -> Option<Vec<Message>>,
) -> Vec<Message> {
    run_record(s, start, fault, true, respond)
}
fn run_record(
    s: &Scratch,
    start: GuardedStart,
    fault: Option<&str>,
    record_entries: bool,
    mut respond: impl FnMut(&Message) -> Option<Vec<Message>>,
) -> Vec<Message> {
    let mut command = s.worker();
    #[cfg(target_os = "linux")]
    if let Some(fault) = fault {
        let shim = s.0.join("io_fault.so");
        assert!(
            Command::new("cc")
                .args(["-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror"])
                .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
                    if fault.starts_with("clock-") {
                        "../../tests/rust-archive-worker/progress_clock.c"
                    } else {
                        "../../tests/rust-archive-worker/io_fault.c"
                    }
                ))
                .args(["-ldl", "-o"])
                .arg(&shim)
                .status()
                .unwrap()
                .success()
        );
        command
            .env("LD_PRELOAD", shim)
            .env("ZIPELO_TEST_FAULT", fault)
            .env("ZIPELO_TEST_MARKER", s.0.join("marker"));
    }
    #[cfg(not(target_os = "linux"))]
    assert!(fault.is_none());
    let mut child = Running(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut writer = Some(Writer::new(child.0.stdin.take().unwrap(), 17));
    writer
        .as_mut()
        .unwrap()
        .send(&Message::StartGuarded(start))
        .unwrap();
    let stdout = child.0.stdout.take().unwrap();
    let mut stderr = child.0.stderr.take().unwrap();
    let (send, receive) = mpsc::sync_channel(128);
    let reader = std::thread::spawn(move || {
        let mut reader = Reader::new(stdout);
        loop {
            let frame = reader.read();
            let done = !matches!(frame, Ok(Some(_)));
            if send.send(frame).is_err() || done {
                break;
            }
        }
    });
    let mut events = Vec::new();
    loop {
        let Some((job, _, event)) = receive
            .recv_timeout(Duration::from_secs(20))
            .expect("worker timeout")
            .unwrap()
        else {
            break;
        };
        assert_eq!(job, 17);
        if let Some(messages) = respond(&event) {
            if messages.is_empty() {
                writer = None;
            } else {
                for message in messages {
                    writer.as_mut().unwrap().send(&message).unwrap();
                }
            }
        }
        let done = matches!(event, Message::Terminal(_));
        if record_entries || !matches!(event, Message::Entry(_)) {
            events.push(event);
        }
        if done {
            writer = None;
        }
    }
    assert!(child.0.wait().unwrap().success(), "{events:?}");
    let mut errors = Vec::new();
    stderr.read_to_end(&mut errors).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    reader.join().unwrap();
    assert!(matches!(events.last(), Some(Message::Terminal(_))));
    events
}
fn request(s: &Scratch, output: u64, ceiling: u64) -> GuardedStart {
    GuardedStart {
        start: Start {
            action: Action::Extract {
                archive: s.path("input"),
                destination: s.path("output"),
                selection: Selection::All,
                list_id: None,
                gzip_output_name: None,
            },
            limits: Limits {
                output,
                ..Limits::default()
            },
        },
        ceiling: Limits {
            output: ceiling,
            entries: 200_000,
            ..Limits::default()
        },
        interactive: false,
    }
}
fn setup() -> Scratch {
    let s = Scratch::new();
    fs::write(s.0.join("input"), include_bytes!("fixtures/deflate.zip")).unwrap();
    fs::create_dir(s.0.join("output")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(s.0.join("output"), fs::Permissions::from_mode(0o700)).unwrap();
    }
    s
}
fn approve(event: &Message) -> Option<Vec<Message>> {
    if let Message::ResourceRequired {
        challenge_id,
        scope,
        ..
    } = event
    {
        Some(vec![Message::ResourceReply {
            challenge_id: *challenge_id,
            scope: *scope,
            allow: true,
        }])
    } else {
        None
    }
}
#[test]
fn scaled_output_boundaries_and_approval() {
    for limit in [
        PAYLOAD.len() as u64 + 1,
        PAYLOAD.len() as u64,
        PAYLOAD.len() as u64 - 1,
    ] {
        let s = setup();
        let events = run(&s, request(&s, limit, 64), approve);
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Message::ResourceRequired { .. }))
                .count(),
            1
        );
        assert_eq!(
            fs::read(s.0.join("output/folder/hello.txt")).unwrap(),
            PAYLOAD
        );
    }
}
#[test]
fn decline_cancel_and_eof_publish_nothing() {
    for mode in 0..3 {
        let s = setup();
        let events = run(&s, request(&s, 16, 64), |event| {
            if let Message::ResourceRequired {
                challenge_id,
                scope,
                usage,
                ..
            } = event
            {
                assert_eq!(usage.estimated_bytes, None);
                assert_eq!(usage.actual_bytes, None);
                assert!(usage.available_bytes.is_some());
                Some(match mode {
                    0 => vec![Message::ResourceReply {
                        challenge_id: *challenge_id,
                        scope: *scope,
                        allow: false,
                    }],
                    1 => vec![Message::Cancel],
                    _ => vec![],
                })
            } else {
                None
            }
        });
        assert_eq!(
            terminal(&events).outcome,
            if mode == 2 {
                Outcome::Failed
            } else {
                Outcome::Cancelled
            },
            "{events:?}"
        );
        assert_eq!(terminal(&events).publication, Publication::None);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
        if mode == 1 {
            assert!(events.iter().any(|e| matches!(
                e,
                Message::CancelAck {
                    status: CancelStatus::Requested,
                    ..
                }
            )));
        }
    }
}
#[test]
fn stale_duplicate_wrong_scope_and_premature_replies_are_rejected() {
    for mode in 0..4 {
        let s = setup();
        let events = run(&s, request(&s, 16, 64), |event| {
            if mode == 3 && matches!(event, Message::Accepted { .. }) {
                return Some(vec![Message::ResourceReply {
                    challenge_id: 99,
                    scope: ResourceScope::default(),
                    allow: true,
                }]);
            }
            if let Message::ResourceRequired {
                challenge_id,
                scope,
                ..
            } = event
            {
                let reply = |id, scope| Message::ResourceReply {
                    challenge_id: id,
                    scope,
                    allow: true,
                };
                Some(match mode {
                    0 => vec![reply(challenge_id + 1, *scope)],
                    1 => vec![reply(*challenge_id, ResourceScope::default())],
                    2 => vec![reply(*challenge_id, *scope), reply(*challenge_id, *scope)],
                    _ => vec![],
                })
            } else {
                None
            }
        });
        assert_eq!(
            terminal(&events).error,
            ErrorCode::Protocol,
            "{mode}: {events:?}"
        );
        assert_eq!(terminal(&events).publication, Publication::None);
    }
}
#[test]
fn ceiling_bytes_and_entries_cannot_be_approved_away() {
    for entries in [false, true] {
        let s = setup();
        let mut start = request(&s, 1, 16);
        if entries {
            start.start.limits.output = 64;
            start.ceiling.output = 64;
            start.start.limits.entries = 1;
            start.ceiling.entries = 2;
            let members = (0..3)
                .map(|id| archive::Member {
                    entry: Entry {
                        entry_id: id,
                        name: format!("empty{id}"),
                        kind: EntryKind::File,
                        size: 0,
                        compressed_size: 0,
                        encrypted: false,
                    },
                    data: Vec::new(),
                })
                .collect::<Vec<_>>();
            let zip =
                archive::create_zip(&members, Method::Store, Limits::default(), &mut || Ok(()))
                    .unwrap();
            fs::write(s.0.join("input"), zip).unwrap();
        }
        let mut approved = false;
        let events = run(&s, start, |event| {
            if matches!(event, Message::ResourceRequired { .. }) {
                approved = true;
            }
            approve(event)
        });
        assert!(approved);
        assert_eq!(terminal(&events).error, ErrorCode::Limit, "{events:?}");
        assert_eq!(terminal(&events).created_count, 0);
    }
}
#[test]
fn entries_have_a_separate_approval() {
    let s = setup();
    let mut start = request(&s, 64, 64);
    start.start.limits.entries = 1;
    start.ceiling.entries = 3;
    let events = run(&s, start, |event| {
        if let Message::ResourceRequired { scope, .. } = event {
            assert!(scope.entries);
            assert!(!scope.output);
        }
        approve(event)
    });
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
}
#[test]
fn resource_then_password_then_conflict_use_distinct_questions() {
    let s = setup();
    fs::write(
        s.0.join("input"),
        include_bytes!("fixtures/aes2-deflate.zip"),
    )
    .unwrap();
    fs::create_dir_all(s.0.join("output/folder")).unwrap();
    fs::write(s.0.join("output/payload.bin"), b"old").unwrap();
    let mut start = request(&s, 16, 8192);
    start.interactive = true;
    let mut ids = Vec::new();
    let events = run(&s, start, |event| match event {
        Message::ResourceRequired { challenge_id, .. } => {
            ids.push(*challenge_id);
            approve(event)
        }
        Message::PasswordRequired { challenge_id, .. } => {
            ids.push(*challenge_id);
            Some(vec![Message::Password {
                challenge_id: *challenge_id,
                secret: Secret(TEST_PASSWORD.to_vec()),
            }])
        }
        Message::ConflictRequired { challenge_id, .. } => {
            ids.push(*challenge_id);
            Some(vec![Message::ConflictReply {
                challenge_id: *challenge_id,
                choice: ConflictChoice::Overwrite,
                apply_to_remaining: false,
            }])
        }
        _ => None,
    });
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert!(ids.len() >= 3, "{events:?}");
    assert!(ids.windows(2).all(|w| w[0] < w[1]));
}
#[test]
fn create_source_total_is_approved_before_password() {
    let s = setup();
    fs::write(s.0.join("source"), b"12345678").unwrap();
    let mut start = request(&s, 7, 8);
    start.start.action = Action::CreateZip {
        sources: vec![Source {
            path: s.path("source"),
            archive_name: "source".into(),
        }],
        output_file: s.path("new.zip"),
        method: Method::Store,
        encryption: Encryption::Aes256,
    };
    let mut approved = false;
    let events = run(&s, start, |event| match event {
        Message::ResourceRequired { action, .. } => {
            assert_eq!(*action, ActionKind::CreateZip);
            approved = true;
            approve(event)
        }
        Message::PasswordRequired { challenge_id, .. } => {
            assert!(approved);
            Some(vec![Message::Password {
                challenge_id: *challenge_id,
                secret: Secret(b"secret".to_vec()),
            }])
        }
        _ => None,
    });
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert!(s.0.join("new.zip").exists());
}
#[test]
fn guarded_wire_round_trip_and_invalid_ceiling() {
    let s = setup();
    let start = request(&s, 16, 64);
    let mut bytes = Vec::new();
    Writer::new(&mut bytes, 17)
        .send(&Message::StartGuarded(start.clone()))
        .unwrap();
    assert_eq!(&bytes[6..8], &6u16.to_le_bytes());
    match Reader::new(bytes.as_slice()).read().unwrap().unwrap().2 {
        Message::StartGuarded(decoded) => assert_eq!(decoded, start),
        other => panic!("{other:?}"),
    }
    let mut invalid = start;
    invalid.ceiling.output = 64 * 1024 * 1024 * 1024 + 1;
    assert!(!invalid.valid());
    assert!(
        Writer::new(Vec::new(), 17)
            .send(&Message::StartGuarded(invalid))
            .is_err()
    );
    let mut reply = Vec::new();
    Writer::new(&mut reply, 17)
        .send(&Message::ResourceReply {
            challenge_id: 4,
            scope: ResourceScope {
                ratio: true,
                ..ResourceScope::default()
            },
            allow: true,
        })
        .unwrap();
    assert_eq!(&reply[6..8], &7u16.to_le_bytes());
    reply[43] = 2;
    assert!(Reader::new(reply.as_slice()).read().is_err());
}

fn generate(s: &Scratch, case: &str) {
    let result = Command::new("python3")
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/rust-archive-worker/guarded_cases.py"),
        )
        .arg(&s.0)
        .arg(case)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
#[test]
fn compressed_formats_keep_limits_after_permission() {
    for kind in ["gz", "tar", "targz", "bz2", "xz", "7z"] {
        let s = setup();
        match kind {
            "gz" => fs::write(s.0.join("input"), include_bytes!("fixtures/sample.gz")).unwrap(),
            "tar" => fs::write(s.0.join("input"), include_bytes!("fixtures/sample.tar")).unwrap(),
            "targz" => {
                fs::write(s.0.join("input"), include_bytes!("fixtures/sample.tar.gz")).unwrap()
            }
            "bz2" | "xz" => generate(&s, kind),
            "7z" => fs::write(
                s.0.join("input"),
                fs::read(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../tests/rust-archive-worker/fixtures/7z/copy.7z"),
                )
                .unwrap(),
            )
            .unwrap(),
            _ => unreachable!(),
        }
        let mut start = request(&s, 16, 65536);
        if kind == "gz" {
            if let Action::Extract {
                gzip_output_name, ..
            } = &mut start.start.action
            {
                *gzip_output_name = Some("payload".into());
            }
        }
        let events = run(&s, start, approve);
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Completed,
            "{kind}: {events:?}"
        );
    }
}
#[test]
fn small_high_ratio_stream_requires_another_scope_and_cancels_promptly() {
    let s = setup();
    generate(&s, "bomb");
    assert!(fs::metadata(s.0.join("input")).unwrap().len() < 1024 * 1024);
    let mut start = request(&s, 16 * 1024 * 1024 * 1024, 64 * 1024 * 1024 * 1024);
    if let Action::Extract {
        gzip_output_name, ..
    } = &mut start.start.action
    {
        *gzip_output_name = Some("payload".into());
    }
    let mut ratio = false;
    let mut cancelled = None;
    let events = run(&s, start, |event| {
        if let Message::ResourceRequired { scope, usage, .. } = event {
            if scope.ratio {
                ratio = true;
                assert!(!scope.output && !scope.entries);
                assert!(usage.actual_bytes.unwrap() >= 256 * 1024 * 1024);
                assert!(usage.compressed_bytes.unwrap() < 1024 * 1024);
                cancelled = Some(std::time::Instant::now());
                return Some(vec![Message::Cancel]);
            }
        }
        if matches!(event, Message::CancelAck { .. }) {
            assert!(cancelled.unwrap().elapsed() < Duration::from_secs(2));
        }
        approve(event)
    });
    assert!(ratio);
    assert_eq!(terminal(&events).outcome, Outcome::Cancelled, "{events:?}");
    assert_eq!(terminal(&events).created_count, 0);
    assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn disk_space_and_write_failures_remain_enforced_after_permission() {
    for fault in ["space-destination", "codec-full", "full"] {
        let s = setup();
        let mut permitted = false;
        let events = run_fault(&s, request(&s, 16, 64), Some(fault), |event| {
            if matches!(event, Message::ResourceRequired { .. }) {
                permitted = true;
            }
            approve(event)
        });
        assert!(permitted);
        assert_eq!(
            terminal(&events).error,
            ErrorCode::DiskFull,
            "{fault}: {events:?}"
        );
        assert_eq!(terminal(&events).cleanup, Cleanup::Complete, "{events:?}");
        assert!(!s.0.join("output/folder/hello.txt").exists());
        assert!(!fs::read_dir(s.0.join("output")).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".zipelo-")
        }));
    }
}

#[test]
fn split_zip_ratio_requires_permission_before_decoding() {
    let s = setup();
    generate(&s, "split-zip");
    assert!(fs::metadata(s.0.join("input")).unwrap().len() < 300_000);
    let mut start = request(&s, Limits::default().output, Limits::default().output);
    start.ceiling.entries = Limits::default().entries;
    for mode in 0..3 {
        let outcome = if mode == 2 {
            Outcome::Completed
        } else {
            Outcome::Cancelled
        };
        let mut asked = false;
        let events = run(&s, start.clone(), |event| {
            if let Message::ResourceRequired {
                challenge_id,
                scope,
                usage,
                ..
            } = event
            {
                assert!(!asked);
                asked = true;
                assert_eq!(
                    *scope,
                    ResourceScope {
                        ratio: true,
                        ..ResourceScope::default()
                    }
                );
                assert_eq!(usage.estimated_bytes, Some(258 * 1024 * 1024));
                assert_eq!(usage.ratio_output_bytes, usage.estimated_bytes);
                assert_eq!(usage.actual_bytes, None);
                assert!(usage.compressed_bytes.unwrap() < 300_000);
                assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
                if mode == 0 {
                    return Some(vec![Message::Cancel]);
                }
                return Some(vec![Message::ResourceReply {
                    challenge_id: *challenge_id,
                    scope: *scope,
                    allow: outcome == Outcome::Completed,
                }]);
            }
            None
        });
        assert!(asked);
        assert_eq!(terminal(&events).outcome, outcome, "{events:?}");
        assert_eq!(
            terminal(&events).created_count,
            if outcome == Outcome::Completed { 2 } else { 0 }
        );
    }
    for name in ["first.bin", "second.bin"] {
        let mut file = fs::File::open(s.0.join("output").join(name)).unwrap();
        assert_eq!(file.metadata().unwrap().len(), 129 * 1024 * 1024);
        let mut block = [0; 65536];
        loop {
            let n = file.read(&mut block).unwrap();
            if n == 0 {
                break;
            }
            assert!(block[..n].iter().all(|b| *b == 0));
        }
    }
}

#[cfg(target_os = "linux")]
fn progress_archive(s: &Scratch, kind: EntryKind, size: u64) {
    if kind == EntryKind::Directory {
        generate(s, "empty-directories");
        return;
    }
    let members = (0..36)
        .map(|entry_id| archive::Member {
            entry: Entry {
                entry_id,
                name: format!("entry-{entry_id}"),
                kind,
                size,
                compressed_size: size,
                encrypted: false,
            },
            data: vec![0; size as usize],
        })
        .collect::<Vec<_>>();
    let zip =
        archive::create_zip(&members, Method::Store, Limits::default(), &mut || Ok(())).unwrap();
    fs::write(s.0.join("input"), zip).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn completed_empty_files_and_directories_refresh_idle_deadline() {
    for (kind, size) in [
        (EntryKind::File, 0),
        (EntryKind::File, 1),
        (EntryKind::Directory, 0),
    ] {
        let s = setup();
        progress_archive(&s, kind, size);
        let events = run_fault(&s, request(&s, 64, 128), Some("clock-progress"), approve);
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Completed,
            "{kind:?}: {events:?}"
        );
        assert_eq!(terminal(&events).created_count, 36);
        assert_eq!(terminal(&events).durability, Durability::Synced);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 36);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn publication_still_stops_after_sixty_seconds_without_progress() {
    let s = setup();
    progress_archive(&s, EntryKind::File, 0);
    let events = run_fault(&s, request(&s, 64, 128), Some("clock-stall"), approve);
    assert_eq!(terminal(&events).outcome, Outcome::Failed, "{events:?}");
    assert_eq!(terminal(&events).error, ErrorCode::Limit);
    assert_eq!(terminal(&events).created_count, 0);
    assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
}

#[test]
fn approved_list_reaches_two_hundred_thousand_short_entries() {
    let s = setup();
    generate(&s, "many-entries");
    let mut start = request(&s, Limits::default().output, Limits::default().output);
    start.start.action = Action::List {
        archive: s.path("input"),
    };
    start.ceiling.entries = start.start.limits.entries;
    let events = run(&s, start.clone(), |_| None);
    assert_eq!(terminal(&events).error, ErrorCode::Limit);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Message::ResourceRequired { .. }))
    );
    start.ceiling.entries = 200_000;
    let mut count = 0;
    // 대량 목록을 시험 부모에 모아 이후 자식의 RSS 측정을 오염시키지 않음.
    let events = run_record(&s, start, None, false, |event| {
        if let Message::Entry(entry) = event {
            assert_eq!(entry.entry_id, count);
            assert_eq!(entry.name, format!("e{count}"));
            count += 1;
        }
        approve(event)
    });
    assert_eq!(
        terminal(&events).outcome,
        Outcome::Completed,
        "{:?}",
        terminal(&events)
    );
    assert_eq!(count, 200_000);
    assert!(events.iter().any(|event| matches!(
        event,
        Message::ListEnd {
            entry_count: 200_000,
            ..
        }
    )));
    assert_eq!(terminal(&events).created_count, 0);
}

#[test]
fn metadata_growth_requires_a_grant_even_below_entry_and_output_caps() {
    let s = setup();
    generate(&s, "wide-names");
    let mut start = request(&s, 1, 1);
    start.start.action = Action::List {
        archive: s.path("input"),
    };
    start.ceiling.entries = start.start.limits.entries;
    let events = run(&s, start.clone(), |_| None);
    assert_eq!(terminal(&events).error, ErrorCode::Limit);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Message::ResourceRequired { .. } | Message::Entry(_)))
    );
    start.ceiling.output = 2;
    let events = run(&s, start, approve);
    assert_eq!(
        terminal(&events).outcome,
        Outcome::Completed,
        "{:?}",
        terminal(&events)
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Message::Entry(_)))
            .count(),
        9000
    );
}
