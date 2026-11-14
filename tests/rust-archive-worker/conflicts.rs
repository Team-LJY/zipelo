use super::*;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    process::{Child, ChildStdin},
    sync::mpsc,
    time::{Duration, Instant},
};

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
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
fn request(s: &Scratch) -> Start {
    Start {
        action: Action::Extract {
            archive: s.path("input"),
            destination: s.path("output"),
            selection: Selection::All,
            list_id: None,
            gzip_output_name: None,
        },
        limits: Limits::default(),
    }
}
fn fixture(s: &Scratch, names: &[&str]) {
    let members = names
        .iter()
        .enumerate()
        .map(|(id, name)| archive::Member {
            entry: Entry {
                entry_id: id as u32,
                name: (*name).into(),
                kind: EntryKind::File,
                size: PAYLOAD.len() as u64,
                compressed_size: PAYLOAD.len() as u64,
                encrypted: false,
            },
            data: PAYLOAD.to_vec(),
        })
        .collect::<Vec<_>>();
    let zip =
        archive::create_zip(&members, Method::Store, Limits::default(), &mut || Ok(())).unwrap();
    fs::write(s.0.join("input"), zip).unwrap();
    directory(s, "output");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(s.0.join("output"), fs::Permissions::from_mode(0o700)).unwrap();
    }
    for name in names {
        let path = s.0.join("output").join(name);
        directory(
            s,
            path.parent()
                .unwrap()
                .strip_prefix(&s.0)
                .unwrap()
                .to_str()
                .unwrap(),
        );
        fs::write(path, b"original").unwrap();
    }
}
fn reply(id: u64, choice: ConflictChoice, apply: bool) -> Message {
    Message::ConflictReply {
        challenge_id: id,
        choice,
        apply_to_remaining: apply,
    }
}
fn run(
    s: &Scratch,
    start: Start,
    fault: Option<&str>,
    initial: Vec<Message>,
    mut respond: impl FnMut(&Message) -> Option<Vec<Message>>,
) -> Vec<Message> {
    let mut command = s.worker();
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "linux")]
    if let Some(fault) = fault {
        let shim = s.0.join("io_fault.so");
        assert!(
            Command::new("cc")
                .args(["-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror"])
                .arg(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../tests/rust-archive-worker/io_fault.c")
                )
                .arg("-ldl")
                .arg("-o")
                .arg(&shim)
                .status()
                .unwrap()
                .success()
        );
        command
            .env("LD_PRELOAD", shim)
            .env("ZIPELO_TEST_FAULT", fault)
            .env("ZIPELO_TEST_MARKER", s.0.join("paused"))
            .env("ZIPELO_TEST_RELEASE", s.0.join("release"));
    }
    #[cfg(not(target_os = "linux"))]
    assert!(fault.is_none());
    let mut child = Running(command.spawn().unwrap());
    let mut writer: Option<Writer<ChildStdin>> =
        Some(Writer::new(child.0.stdin.take().unwrap(), 17));
    writer
        .as_mut()
        .unwrap()
        .send(&Message::StartInteractive(start))
        .unwrap();
    for message in initial {
        writer.as_mut().unwrap().send(&message).unwrap();
    }
    let stdout = child.0.stdout.take().unwrap();
    let mut stderr = child.0.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let (send, receive) = mpsc::channel();
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
            .recv_timeout(Duration::from_secs(15))
            .expect("worker response timeout")
            .unwrap()
        else {
            break;
        };
        assert_eq!(job, 17);
        if let Some(commands) = respond(&event) {
            if commands.is_empty() {
                writer = None;
            } else {
                for command in commands {
                    writer.as_mut().unwrap().send(&command).unwrap();
                }
            }
        }
        let done = matches!(event, Message::Terminal(_));
        events.push(event);
        if done {
            writer = None;
        }
    }
    assert!(child.0.wait().unwrap().success(), "{events:?}");
    reader.join().unwrap();
    assert!(errors.join().unwrap().is_empty());
    assert!(matches!(events.last(), Some(Message::Terminal(_))));
    events
}
fn choose(s: &Scratch, choice: ConflictChoice, apply: bool) -> Vec<Message> {
    run(s, request(s), None, vec![], |event| match event {
        Message::ConflictRequired { challenge_id, .. } => {
            Some(vec![reply(*challenge_id, choice, apply)])
        }
        _ => None,
    })
}
fn count(events: &[Message], predicate: impl Fn(&Message) -> bool) -> usize {
    events.iter().filter(|event| predicate(event)).count()
}
fn questions(events: &[Message]) -> usize {
    count(events, |e| matches!(e, Message::ConflictRequired { .. }))
}
fn created(events: &[Message]) -> usize {
    count(events, |e| {
        matches!(
            e,
            Message::Output {
                disposition: Disposition::Created,
                ..
            }
        )
    })
}
fn hash(path: &std::path::Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).unwrap()).to_vec()
}

#[test]
fn three_choices_and_apply_remaining_report_every_entry() {
    for choice in [
        ConflictChoice::Overwrite,
        ConflictChoice::Skip,
        ConflictChoice::Rename,
    ] {
        for apply in [false, true] {
            let s = Scratch::new();
            fixture(&s, &["a.txt", "b.txt"]);
            let before = hash(&s.0.join("output/a.txt"));
            let events = choose(&s, choice, apply);
            let t = terminal(&events);
            assert_eq!(t.outcome, Outcome::Completed, "{events:?}");
            assert_eq!(questions(&events), if apply { 1 } else { 2 });
            assert_eq!(
                count(&events, |e| matches!(e, Message::ConflictResolved { .. })),
                2
            );
            for name in ["a", "b"] {
                let original = s.0.join(format!("output/{name}.txt"));
                if choice == ConflictChoice::Overwrite {
                    assert_eq!(fs::read(original).unwrap(), PAYLOAD);
                } else {
                    assert_eq!(hash(&original), before);
                }
                if choice == ConflictChoice::Rename {
                    assert_eq!(
                        fs::read(s.0.join(format!("output/{name} (1).txt"))).unwrap(),
                        PAYLOAD
                    );
                }
            }
            let skipped = choice == ConflictChoice::Skip;
            assert_eq!(t.created_count, if skipped { 0 } else { 2 });
            assert_eq!(created(&events), t.created_count as usize);
            assert_eq!(
                t.publication,
                if skipped {
                    Publication::None
                } else {
                    Publication::Complete
                }
            );
            assert_eq!(t.cleanup, Cleanup::Complete);
            if skipped {
                assert_eq!(t.durability, Durability::NotApplicable);
            }
            assert_eq!(
                fs::read_dir(s.0.join("output")).unwrap().count(),
                if choice == ConflictChoice::Rename {
                    4
                } else {
                    2
                }
            );
        }
    }
}
#[test]
fn rename_avoids_existing_names_and_later_archive_paths() {
    let s = Scratch::new();
    fixture(&s, &["폴더/자료.txt", "폴더/자료 (1).txt"]);
    fs::write(s.0.join("output/폴더/자료 (2).txt"), b"keep").unwrap();
    let events = choose(&s, ConflictChoice::Rename, true);
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(
        fs::read(s.0.join("output/폴더/자료 (3).txt")).unwrap(),
        PAYLOAD
    );
    assert_eq!(
        fs::read(s.0.join("output/폴더/자료 (1) (1).txt")).unwrap(),
        PAYLOAD
    );
    assert_eq!(
        fs::read(s.0.join("output/폴더/자료 (2).txt")).unwrap(),
        b"keep"
    );
}
#[test]
fn mixed_choices_and_non_conflicting_files() {
    let s = Scratch::new();
    fixture(&s, &["a", "b", "c", "new"]);
    fs::remove_file(s.0.join("output/new")).unwrap();
    let mut index = 0;
    let events = run(&s, request(&s), None, vec![], |event| match event {
        Message::ConflictRequired {
            challenge_id,
            incoming_size,
            existing_size,
            ..
        } => {
            assert_eq!(*incoming_size, PAYLOAD.len() as u64);
            assert_eq!(*existing_size, 8);
            let choice = [
                ConflictChoice::Overwrite,
                ConflictChoice::Skip,
                ConflictChoice::Rename,
            ][index];
            index += 1;
            Some(vec![reply(*challenge_id, choice, false)])
        }
        _ => None,
    });
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(terminal(&events).created_count, 3);
    assert_eq!(fs::read(s.0.join("output/a")).unwrap(), PAYLOAD);
    assert_eq!(fs::read(s.0.join("output/b")).unwrap(), b"original");
    assert_eq!(fs::read(s.0.join("output/c (1)")).unwrap(), PAYLOAD);
    assert_eq!(fs::read(s.0.join("output/new")).unwrap(), PAYLOAD);
}
#[test]
fn cancel_waiting_for_choice_preserves_originals() {
    let s = Scratch::new();
    fixture(&s, &["a"]);
    let before = hash(&s.0.join("output/a"));
    let events = run(&s, request(&s), None, vec![], |e| {
        matches!(e, Message::ConflictRequired { .. }).then(|| vec![Message::Cancel])
    });
    assert_eq!(terminal(&events).outcome, Outcome::Cancelled, "{events:?}");
    assert_eq!(terminal(&events).publication, Publication::None);
    assert_eq!(created(&events), 0);
    assert_eq!(hash(&s.0.join("output/a")), before);
    assert!(events.iter().any(|e| matches!(
        e,
        Message::CancelAck {
            status: CancelStatus::Requested,
            ..
        }
    )));
}
#[test]
fn cancellation_keeps_earlier_replacement() {
    let s = Scratch::new();
    fixture(&s, &["a", "b"]);
    let events = run(&s, request(&s), None, vec![], |e| match e {
        Message::ConflictRequired {
            challenge_id,
            entry_id: 0,
            ..
        } => Some(vec![reply(*challenge_id, ConflictChoice::Overwrite, false)]),
        Message::ConflictRequired { .. } => Some(vec![Message::Cancel]),
        _ => None,
    });
    assert_eq!(terminal(&events).outcome, Outcome::Cancelled, "{events:?}");
    assert_eq!(terminal(&events).publication, Publication::Partial);
    assert_eq!(terminal(&events).created_count, 1);
    assert_eq!(fs::read(s.0.join("output/a")).unwrap(), PAYLOAD);
    assert_eq!(fs::read(s.0.join("output/b")).unwrap(), b"original");
}
#[test]
fn eof_stale_duplicate_and_wrong_kind_replies_fail_protocol() {
    for mode in 0..4 {
        let s = Scratch::new();
        fixture(&s, &["a", "b"]);
        let events = run(&s, request(&s), None, vec![], |e| match e {
            Message::ConflictRequired { challenge_id, .. } => Some(match mode {
                0 => vec![],
                1 => vec![reply(challenge_id + 1, ConflictChoice::Skip, false)],
                2 => vec![
                    reply(*challenge_id, ConflictChoice::Skip, false),
                    reply(*challenge_id, ConflictChoice::Skip, false),
                ],
                _ => vec![Message::Password {
                    challenge_id: *challenge_id,
                    secret: Secret(vec![]),
                }],
            }),
            _ => None,
        });
        assert_eq!(
            terminal(&events).error,
            ErrorCode::Protocol,
            "{mode}: {events:?}"
        );
        assert_eq!(created(&events), 0);
    }
}
#[test]
fn premature_reply_is_rejected() {
    let s = Scratch::new();
    fixture(&s, &["a"]);
    let events = run(
        &s,
        request(&s),
        None,
        vec![reply(999, ConflictChoice::Skip, true)],
        |_| None,
    );
    assert_eq!(terminal(&events).error, ErrorCode::Protocol, "{events:?}");
    assert_eq!(created(&events), 0);
}
#[test]
fn password_and_conflict_share_distinct_challenges() {
    let s = Scratch::new();
    fs::write(s.0.join("input"), AES_ZIP).unwrap();
    directory(&s, "output");
    fs::write(s.0.join("output/payload.bin"), b"original").unwrap();
    let mut password_id = None;
    let events = run(&s, request(&s), None, vec![], |e| match e {
        Message::PasswordRequired { challenge_id, .. } => {
            password_id = Some(*challenge_id);
            Some(password_reply(*challenge_id, TEST_PASSWORD))
        }
        Message::ConflictRequired { challenge_id, .. } => {
            assert_ne!(Some(*challenge_id), password_id);
            Some(vec![reply(*challenge_id, ConflictChoice::Skip, false)])
        }
        _ => None,
    });
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(questions(&events), 1);
    assert_eq!(created(&events), 0);
}
#[test]
fn legacy_start_still_aborts_and_policy_does_not_cross_jobs() {
    let s = Scratch::new();
    fixture(&s, &["a", "b"]);
    assert_eq!(terminal(&execute(request(&s))).error, ErrorCode::Conflict);
    assert_eq!(questions(&choose(&s, ConflictChoice::Skip, true)), 1);
    assert_eq!(questions(&choose(&s, ConflictChoice::Skip, false)), 2);
}
#[test]
fn corruption_is_rejected_before_any_question_or_output() {
    let s = Scratch::new();
    fixture(&s, &["a"]);
    let mut bytes = fs::read(s.0.join("input")).unwrap();
    let at = bytes
        .windows(PAYLOAD.len())
        .position(|b| b == PAYLOAD)
        .unwrap();
    bytes[at] ^= 1;
    fs::write(s.0.join("input"), bytes).unwrap();
    let events = run(&s, request(&s), None, vec![], |_| None);
    assert_eq!(terminal(&events).error, ErrorCode::Integrity, "{events:?}");
    assert_eq!(questions(&events), 0);
    assert_eq!(created(&events), 0);
}
#[test]
fn raw_gzip_conflicts_use_explicit_name() {
    let s = Scratch::new();
    fixture(&s, &["chosen.txt"]);
    fs::write(s.0.join("input"), include_bytes!("fixtures/sample.gz")).unwrap();
    let mut start = request(&s);
    if let Action::Extract {
        gzip_output_name, ..
    } = &mut start.action
    {
        *gzip_output_name = Some("chosen.txt".into());
    }
    let events = run(&s, start, None, vec![], |e| match e {
        Message::ConflictRequired {
            challenge_id,
            relative_name,
            ..
        } => {
            assert_eq!(relative_name, "chosen.txt");
            Some(vec![reply(*challenge_id, ConflictChoice::Overwrite, false)])
        }
        _ => None,
    });
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(fs::read(s.0.join("output/chosen.txt")).unwrap(), PAYLOAD);
}
#[test]
fn file_directory_collisions_are_rejected_before_questions() {
    for path in ["a", "parent"] {
        let s = Scratch::new();
        fixture(&s, &[if path == "a" { "a" } else { "parent/a" }]);
        let target = s.0.join("output").join(path);
        if path == "a" {
            fs::remove_file(&target).unwrap();
            fs::create_dir(&target).unwrap();
        } else {
            fs::remove_dir_all(&target).unwrap();
            fs::write(&target, b"keep").unwrap();
        }
        let events = run(&s, request(&s), None, vec![], |_| None);
        assert_eq!(terminal(&events).error, ErrorCode::UnsafePath, "{events:?}");
        assert_eq!(questions(&events), 0);
        assert_eq!(created(&events), 0);
    }
}
#[test]
fn newly_created_empty_directory_is_counted_when_files_are_skipped() {
    let s = Scratch::new();
    directory(&s, "empty");
    fs::write(s.0.join("source"), PAYLOAD).unwrap();
    let events = execute(Start {
        action: Action::CreateZip {
            sources: vec![
                Source {
                    path: s.path("source"),
                    archive_name: "a".into(),
                },
                Source {
                    path: s.path("empty"),
                    archive_name: "empty".into(),
                },
            ],
            output_file: s.path("input"),
            method: Method::Store,
            encryption: Encryption::None,
        },
        limits: Limits::default(),
    });
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    directory(&s, "output");
    fs::write(s.0.join("output/a"), b"original").unwrap();
    let events = choose(&s, ConflictChoice::Skip, true);
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(terminal(&events).publication, Publication::Complete);
    assert_eq!(terminal(&events).created_count, 1);
    assert_eq!(created(&events), 1);
    assert!(s.0.join("output/empty").is_dir());
}

#[cfg(target_os = "linux")]
#[test]
fn links_and_special_entries_are_not_interactive() {
    use std::os::unix::{fs::symlink, net::UnixListener};
    for mode in 0..3 {
        let s = Scratch::new();
        fixture(&s, &["a"]);
        fs::remove_file(s.0.join("output/a")).unwrap();
        fs::write(s.0.join("outside"), b"keep").unwrap();
        let listener;
        match mode {
            0 => symlink(s.0.join("outside"), s.0.join("output/a")).unwrap(),
            1 => symlink(s.0.join("missing"), s.0.join("output/a")).unwrap(),
            _ => {
                use std::os::fd::AsRawFd;
                let directory = fs::File::open(s.0.join("output")).unwrap();
                listener = UnixListener::bind(format!("/proc/self/fd/{}/a", directory.as_raw_fd()))
                    .unwrap();
                let _ = &listener;
            }
        }
        let events = run(&s, request(&s), None, vec![], |_| None);
        assert_eq!(terminal(&events).error, ErrorCode::UnsafePath, "{events:?}");
        assert_eq!(questions(&events), 0);
        assert_eq!(fs::read(s.0.join("outside")).unwrap(), b"keep");
    }
}
#[cfg(target_os = "linux")]
#[test]
fn pre_replacement_failures_preserve_original_hash_and_remove_temps() {
    for mode in [
        "create-full",
        "full",
        "sync-full",
        "write-io",
        "replace-fail",
    ] {
        let s = Scratch::new();
        fixture(&s, &["a"]);
        let before = hash(&s.0.join("output/a"));
        let events = run(&s, request(&s), Some(mode), vec![], |e| match e {
            Message::ConflictRequired { challenge_id, .. } => {
                Some(vec![reply(*challenge_id, ConflictChoice::Overwrite, false)])
            }
            _ => None,
        });
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Failed,
            "{mode}: {events:?}"
        );
        assert_eq!(created(&events), 0);
        assert_eq!(hash(&s.0.join("output/a")), before);
        assert_eq!(terminal(&events).cleanup, Cleanup::Complete);
        assert_eq!(terminal(&events).retained_partial_count, 0);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 1);
    }
}
#[cfg(target_os = "linux")]
#[test]
fn changed_target_is_not_replaced() {
    let s = Scratch::new();
    fixture(&s, &["a"]);
    let events = run(&s, request(&s), None, vec![], |e| match e {
        Message::ConflictRequired { challenge_id, .. } => {
            fs::write(s.0.join("output/a"), b"changed by user").unwrap();
            Some(vec![reply(*challenge_id, ConflictChoice::Overwrite, false)])
        }
        _ => None,
    });
    assert_eq!(terminal(&events).error, ErrorCode::Conflict, "{events:?}");
    assert_eq!(created(&events), 0);
    assert_eq!(fs::read(s.0.join("output/a")).unwrap(), b"changed by user");
}
#[cfg(target_os = "linux")]
#[test]
fn cancel_during_temporary_write_preserves_original_hash() {
    let s = Scratch::new();
    fixture(&s, &["a"]);
    let before = hash(&s.0.join("output/a"));
    let mut cancelled = false;
    let events = run(&s, request(&s), Some("pause"), vec![], |e| match e {
        Message::ConflictRequired { challenge_id, .. } => {
            Some(vec![reply(*challenge_id, ConflictChoice::Overwrite, false)])
        }
        Message::Progress {
            stage: Stage::Write,
            ..
        } if !cancelled => {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !s.0.join("paused").exists() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            cancelled = true;
            Some(vec![Message::Cancel])
        }
        Message::CancelAck {
            status: CancelStatus::Requested,
            ..
        } => {
            fs::write(s.0.join("release"), []).unwrap();
            None
        }
        _ => None,
    });
    assert!(cancelled);
    assert_eq!(terminal(&events).outcome, Outcome::Cancelled, "{events:?}");
    assert_eq!(hash(&s.0.join("output/a")), before);
    assert_eq!(created(&events), 0);
    assert_eq!(terminal(&events).cleanup, Cleanup::Complete);
}
#[cfg(target_os = "linux")]
#[test]
fn post_replacement_sync_error_reports_actual_publication() {
    let s = Scratch::new();
    fixture(&s, &["a"]);
    let events = run(
        &s,
        request(&s),
        Some("dir-sync-full"),
        vec![],
        |e| match e {
            Message::ConflictRequired { challenge_id, .. } => {
                Some(vec![reply(*challenge_id, ConflictChoice::Overwrite, false)])
            }
            _ => None,
        },
    );
    assert_eq!(terminal(&events).error, ErrorCode::DiskFull, "{events:?}");
    assert_eq!(terminal(&events).publication, Publication::Partial);
    assert_eq!(terminal(&events).created_count, 1);
    assert_eq!(terminal(&events).durability, Durability::Failed);
    assert_eq!(terminal(&events).cleanup, Cleanup::Complete);
    assert_eq!(fs::read(s.0.join("output/a")).unwrap(), PAYLOAD);
    assert_eq!(created(&events), 1);
}

#[test]
fn rename_handles_maximum_utf8_filename_length() {
    let s = Scratch::new();
    let name = format!("{}ab.txt", "가".repeat(83));
    fixture(&s, &[&name]);
    let events = choose(&s, ConflictChoice::Rename, false);
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    let renamed = events
        .iter()
        .find_map(|event| match event {
            Message::ConflictResolved {
                relative_name,
                choice: ConflictChoice::Rename,
                ..
            } => Some(relative_name),
            _ => None,
        })
        .unwrap();
    assert!(renamed.len() <= 255);
    assert!(renamed.ends_with(" (1).txt"));
    assert_eq!(fs::read(s.0.join("output").join(renamed)).unwrap(), PAYLOAD);
    assert_eq!(
        fs::read(s.0.join("output").join(name)).unwrap(),
        b"original"
    );
}
#[cfg(target_os = "linux")]
#[test]
fn overwrite_does_not_modify_other_hard_links() {
    let s = Scratch::new();
    fixture(&s, &["a"]);
    fs::hard_link(s.0.join("output/a"), s.0.join("outside")).unwrap();
    let events = choose(&s, ConflictChoice::Overwrite, false);
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(fs::read(s.0.join("output/a")).unwrap(), PAYLOAD);
    assert_eq!(fs::read(s.0.join("outside")).unwrap(), b"original");
}

#[cfg(target_os = "linux")]
#[test]
fn skip_all_ignores_destination_space_for_zip_tar_and_gzip() {
    for format in ["zip", "tar", "gzip"] {
        for apply in [false, true] {
            let s = Scratch::new();
            let names = match format {
                "zip" => vec!["a", "b"],
                "tar" => vec!["folder/hello.txt"],
                _ => vec!["chosen.txt"],
            };
            fixture(&s, &names);
            let mut start = request(&s);
            if format == "tar" {
                fs::write(s.0.join("input"), include_bytes!("fixtures/sample.tar")).unwrap();
            } else if format == "gzip" {
                fs::write(s.0.join("input"), include_bytes!("fixtures/sample.gz")).unwrap();
                if let Action::Extract {
                    gzip_output_name, ..
                } = &mut start.action
                {
                    *gzip_output_name = Some("chosen.txt".into());
                }
            }
            let before = names
                .iter()
                .map(|name| hash(&s.0.join("output").join(name)))
                .collect::<Vec<_>>();
            let events = run(
                &s,
                start,
                Some("space-destination"),
                vec![],
                |event| match event {
                    Message::ConflictRequired { challenge_id, .. } => {
                        Some(vec![reply(*challenge_id, ConflictChoice::Skip, apply)])
                    }
                    _ => None,
                },
            );
            assert_eq!(
                terminal(&events).outcome,
                Outcome::Completed,
                "{format}/{apply}: {events:?}"
            );
            assert_eq!(terminal(&events).error, ErrorCode::None);
            assert_eq!(terminal(&events).publication, Publication::None);
            assert_eq!(terminal(&events).created_count, 0);
            assert_eq!(terminal(&events).durability, Durability::NotApplicable);
            assert_eq!(terminal(&events).cleanup, Cleanup::Complete);
            assert_eq!(created(&events), 0);
            let conflicts = if format == "zip" { 2 } else { 1 };
            assert_eq!(questions(&events), if apply { 1 } else { conflicts });
            assert_eq!(
                count(&events, |event| matches!(
                    event,
                    Message::ConflictResolved {
                        choice: ConflictChoice::Skip,
                        ..
                    }
                )),
                conflicts
            );
            for (name, before) in names.iter().zip(before) {
                assert_eq!(hash(&s.0.join("output").join(name)), before);
            }
            if format == "gzip" {
                assert_eq!(
                    fs::read(s.0.join("output/chosen.txt")).unwrap(),
                    b"original"
                );
            }
            assert!(
                !s.0.join("paused").exists(),
                "skipped files must not charge destination space"
            );
        }
    }
}
#[cfg(target_os = "linux")]
#[test]
fn destination_space_is_checked_after_overwrite_or_rename_choice() {
    for choice in [ConflictChoice::Overwrite, ConflictChoice::Rename] {
        let s = Scratch::new();
        fixture(&s, &["a", "b"]);
        let before = hash(&s.0.join("output/a"));
        let events = run(
            &s,
            request(&s),
            Some("space-destination"),
            vec![],
            |event| match event {
                Message::ConflictRequired { challenge_id, .. } => {
                    Some(vec![reply(*challenge_id, choice, true)])
                }
                _ => None,
            },
        );
        assert_eq!(questions(&events), 1, "{events:?}");
        assert_eq!(
            count(&events, |event| matches!(
                event,
                Message::ConflictResolved { .. }
            )),
            1
        );
        assert_eq!(terminal(&events).error, ErrorCode::DiskFull, "{events:?}");
        assert_eq!(terminal(&events).publication, Publication::None);
        assert_eq!(terminal(&events).created_count, 0);
        assert_eq!(terminal(&events).cleanup, Cleanup::Complete);
        assert_eq!(hash(&s.0.join("output/a")), before);
        assert_eq!(hash(&s.0.join("output/b")), before);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 2);
        assert!(s.0.join("paused").exists());
    }
}
#[cfg(target_os = "linux")]
#[test]
fn skipped_files_do_not_charge_space_for_remaining_publication() {
    for choice in [
        Some(ConflictChoice::Overwrite),
        Some(ConflictChoice::Rename),
        None,
    ] {
        let s = Scratch::new();
        fixture(&s, &["a", "b", "c"]);
        assert!(PAYLOAD.len() <= 64 && PAYLOAD.len() * 3 > 64);
        if choice.is_none() {
            fs::remove_file(s.0.join("output/c")).unwrap();
        }
        let events = run(
            &s,
            request(&s),
            Some("space-one-output"),
            vec![],
            |event| match event {
                Message::ConflictRequired {
                    challenge_id,
                    entry_id,
                    ..
                } => {
                    let choice = if *entry_id < 2 {
                        ConflictChoice::Skip
                    } else {
                        choice.unwrap()
                    };
                    Some(vec![reply(*challenge_id, choice, false)])
                }
                _ => None,
            },
        );
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Completed,
            "{choice:?}: {events:?}"
        );
        assert_eq!(terminal(&events).publication, Publication::Complete);
        assert_eq!(terminal(&events).created_count, 1);
        assert_eq!(created(&events), 1);
        assert_eq!(questions(&events), if choice.is_some() { 3 } else { 2 });
        assert_eq!(fs::read(s.0.join("output/a")).unwrap(), b"original");
        assert_eq!(fs::read(s.0.join("output/b")).unwrap(), b"original");
        let output = if choice == Some(ConflictChoice::Rename) {
            "c (1)"
        } else {
            "c"
        };
        assert_eq!(fs::read(s.0.join("output").join(output)).unwrap(), PAYLOAD);
        if choice == Some(ConflictChoice::Rename) {
            assert_eq!(fs::read(s.0.join("output/c")).unwrap(), b"original");
        }
        assert!(s.0.join("paused").exists());
    }
}
#[cfg(target_os = "linux")]
#[test]
fn insufficient_destination_space_does_not_prevent_cancellation() {
    let s = Scratch::new();
    fixture(&s, &["a"]);
    let events = run(
        &s,
        request(&s),
        Some("space-destination"),
        vec![],
        |event| matches!(event, Message::ConflictRequired { .. }).then(|| vec![Message::Cancel]),
    );
    assert_eq!(questions(&events), 1, "{events:?}");
    assert_eq!(terminal(&events).outcome, Outcome::Cancelled, "{events:?}");
    assert_eq!(terminal(&events).publication, Publication::None);
    assert_eq!(fs::read(s.0.join("output/a")).unwrap(), b"original");
}
#[cfg(target_os = "linux")]
#[test]
fn interactive_staging_space_and_integrity_checks_remain_before_questions() {
    for corrupt in [false, true] {
        let s = Scratch::new();
        fixture(&s, &["a"]);
        if corrupt {
            let mut bytes = fs::read(s.0.join("input")).unwrap();
            let at = bytes
                .windows(PAYLOAD.len())
                .position(|bytes| bytes == PAYLOAD)
                .unwrap();
            bytes[at] ^= 1;
            fs::write(s.0.join("input"), bytes).unwrap();
        }
        let fault = if corrupt {
            "space-destination"
        } else {
            "space-stage"
        };
        let events = run(&s, request(&s), Some(fault), vec![], |_| None);
        assert_eq!(
            terminal(&events).error,
            if corrupt {
                ErrorCode::Integrity
            } else {
                ErrorCode::DiskFull
            },
            "{events:?}"
        );
        assert_eq!(questions(&events), 0);
        assert_eq!(created(&events), 0);
        assert_eq!(terminal(&events).publication, Publication::None);
        assert_eq!(fs::read(s.0.join("output/a")).unwrap(), b"original");
    }
}
