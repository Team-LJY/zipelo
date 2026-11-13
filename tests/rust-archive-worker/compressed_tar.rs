use super::*;

fn fixture(kind: &str, case: &str) -> Scratch {
    let scratch = Scratch::new();
    let output = Command::new("python3")
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/rust-archive-worker/compressed_tar_cases.py"),
        )
        .arg(&scratch.0)
        .arg(kind)
        .arg(case)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{kind}/{case}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    scratch
}

fn run_case(s: &Scratch, limits: Limits) -> Vec<Message> {
    extract(
        s,
        &fs::read(s.0.join("input")).unwrap(),
        None,
        Selection::All,
        None,
        limits,
    )
}

fn no_output(s: &Scratch, events: &[Message], error: ErrorCode) {
    let result = terminal(events);
    assert_eq!(result.outcome, Outcome::Failed, "{events:?}");
    assert_eq!(result.error, error, "{events:?}");
    assert_eq!(result.created_count, 0);
    assert_eq!(result.publication, Publication::None);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Message::Output { .. }))
    );
    assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
    assert!(!s.0.join("escape").exists());
}

fn normal(kind: &str, case: &str) {
    let s = fixture(kind, case);
    let events = run_case(&s, Limits::default());
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(
        fs::read(s.0.join("output/folder/payload.bin")).unwrap(),
        fs::read(s.0.join("expected")).unwrap()
    );
    assert_eq!(fs::read(s.0.join("output/한글.txt")).unwrap(), b"end");
    assert!(s.0.join("output/empty").is_dir());
}

#[test]
fn bzip2_reference_tar_and_concatenated_streams() {
    for case in ["normal", "multi"] {
        normal("bz2", case);
    }
}

#[test]
fn xz_reference_tar_checks_filters_and_concatenated_streams() {
    for case in [
        "normal",
        "multi",
        "multi-block",
        "multi-block-filters",
        "stored-chunks",
        "padding",
        "crc32",
        "sha256",
        "no-check",
        "filters",
    ] {
        normal("xz", case);
    }
}

#[test]
fn compressed_empty_tar_is_valid() {
    for kind in ["bz2", "xz"] {
        let s = fixture(kind, "empty");
        let events = run_case(&s, Limits::default());
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Completed,
            "{kind}: {events:?}"
        );
        assert_eq!(terminal(&events).created_count, 0);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
    }
}

#[test]
fn bzip2_truncation_corruption_and_trailing_data_never_publish() {
    for case in [
        "truncated",
        "corrupt",
        "trailing",
        "multi-bad",
        "bad-tar",
        "raw",
        "empty-stream",
    ] {
        let s = fixture("bz2", case);
        let events = run_case(&s, Limits::default());
        no_output(&s, &events, ErrorCode::Integrity);
    }
}

#[test]
fn xz_truncation_corruption_and_trailing_data_never_publish() {
    for case in [
        "truncated",
        "corrupt",
        "trailing",
        "multi-bad",
        "bad-tar",
        "raw",
        "empty-stream",
        "bad-padding",
    ] {
        let s = fixture("xz", case);
        let events = run_case(&s, Limits::default());
        no_output(&s, &events, ErrorCode::Integrity);
    }
}

#[test]
fn xz_index_sizes_cannot_cancel_between_streams() {
    let s = fixture("xz", "index-stream-sizes");
    let events = run_case(&s, Limits::default());
    no_output(&s, &events, ErrorCode::Integrity);
}

#[test]
fn xz_index_sizes_cannot_cancel_between_blocks() {
    let s = fixture("xz", "index-block-sizes");
    let events = run_case(&s, Limits::default());
    no_output(&s, &events, ErrorCode::Integrity);
}

#[test]
fn xz_index_unpadded_sizes_must_match_each_block() {
    let s = fixture("xz", "index-unpadded-sizes");
    let events = run_case(&s, Limits::default());
    no_output(&s, &events, ErrorCode::Integrity);
}

#[test]
fn compressed_tar_bombs_stop_before_publication() {
    for kind in ["bz2", "xz"] {
        let s = fixture(kind, "bomb");
        let events = run_case(
            &s,
            Limits {
                output: 256 * 1024,
                ..Limits::default()
            },
        );
        no_output(&s, &events, ErrorCode::Limit);
    }
}

#[test]
fn xz_huge_dictionary_is_rejected_before_allocation() {
    let s = fixture("xz", "dictionary");
    let events = run_case(&s, Limits::default());
    no_output(&s, &events, ErrorCode::Limit);
}

#[test]
fn compressed_tar_paths_and_links_keep_existing_guards() {
    for kind in ["bz2", "xz"] {
        for (case, error) in [
            ("unsafe", ErrorCode::UnsafePath),
            ("link", ErrorCode::UnsafePath),
        ] {
            let s = fixture(kind, case);
            let events = run_case(&s, Limits::default());
            no_output(&s, &events, error);
        }
    }
}

#[test]
fn compressed_tar_list_identity_and_selection() {
    for kind in ["bz2", "xz"] {
        let s = fixture(kind, "normal");
        let events = execute(Start {
            action: Action::List {
                archive: s.path("input"),
            },
            limits: Limits::default(),
        });
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        let token = events
            .iter()
            .find_map(|event| match event {
                Message::ListEnd { list_id, .. } => Some(list_id.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(token.format, Format::Tar);
        let bytes = fs::read(s.0.join("input")).unwrap();
        let events = extract(
            &s,
            &bytes,
            None,
            Selection::EntryIds(vec![2]),
            Some(token),
            Limits::default(),
        );
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        assert_eq!(fs::read(s.0.join("output/한글.txt")).unwrap(), b"end");
        assert!(!s.0.join("output/folder").exists());
    }
}

#[test]
fn compressed_tar_entry_quota_is_preserved() {
    for kind in ["bz2", "xz"] {
        let s = fixture(kind, "normal");
        let events = run_case(
            &s,
            Limits {
                entries: 1,
                ..Limits::default()
            },
        );
        no_output(&s, &events, ErrorCode::Limit);
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        io::Read,
        time::{Duration, Instant},
    };

    struct Running(std::process::Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn inject(s: &Scratch, mode: &str) -> (Running, Writer<std::process::ChildStdin>) {
        let library = s.0.join("compressed_fault.so");
        assert!(
            Command::new("cc")
                .args(["-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror"])
                .arg(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../tests/rust-archive-worker/compressed_fault.c")
                )
                .arg("-ldl")
                .arg("-o")
                .arg(&library)
                .status()
                .unwrap()
                .success()
        );
        let bytes = fs::read(s.0.join("input")).unwrap();
        let preparation = extract(
            s,
            &bytes,
            None,
            Selection::EntryIds(vec![999]),
            None,
            Limits::default(),
        );
        assert_ne!(terminal(&preparation).outcome, Outcome::Completed);
        let mut command = s.worker();
        command
            .env("LD_PRELOAD", library)
            .env("ZIPELO_COMPRESSED_FAULT", mode)
            .env("ZIPELO_COMPRESSED_MARKER", s.0.join("paused"))
            .env("ZIPELO_COMPRESSED_RELEASE", s.0.join("release"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let mut writer = Writer::new(child.stdin.take().unwrap(), 17);
        writer
            .send(&Message::Start(Start {
                action: Action::Extract {
                    archive: s.path("input"),
                    destination: s.path("output"),
                    selection: Selection::All,
                    list_id: None,
                    gzip_output_name: None,
                },
                limits: Limits::default(),
            }))
            .unwrap();
        (Running(child), writer)
    }

    fn finish(child: &mut Running) -> Vec<Message> {
        drop(child.0.stdin.take());
        let mut out = child.0.stdout.take().unwrap();
        let mut err = child.0.stderr.take().unwrap();
        let mut bytes = Vec::new();
        out.read_to_end(&mut bytes).unwrap();
        let mut errors = Vec::new();
        err.read_to_end(&mut errors).unwrap();
        assert!(child.0.wait().unwrap().success());
        assert!(errors.is_empty(), "{errors:?}");
        let mut reader = Reader::new(bytes.as_slice());
        let mut events = Vec::new();
        while let Some((_, _, event)) = reader.read().unwrap() {
            events.push(event);
        }
        events
    }

    #[test]
    fn compressed_tar_codec_space_failure_never_publishes() {
        for kind in ["bz2", "xz"] {
            for (mode, error) in [("full", ErrorCode::DiskFull), ("io", ErrorCode::Io)] {
                let s = fixture(kind, "normal");
                let (mut child, writer) = inject(&s, mode);
                drop(writer);
                let events = finish(&mut child);
                assert!(s.0.join("paused").exists());
                no_output(&s, &events, error);
            }
        }
    }

    #[test]
    fn compressed_tar_cancels_during_decode_and_cleans_staging() {
        for kind in ["bz2", "xz"] {
            let s = fixture(kind, "normal");
            let (mut child, mut writer) = inject(&s, "pause");
            let deadline = Instant::now() + Duration::from_secs(10);
            while !s.0.join("paused").exists() {
                assert!(Instant::now() < deadline, "decoder did not pause");
                std::thread::sleep(Duration::from_millis(5));
            }
            writer.send(&Message::Cancel).unwrap();
            drop(writer);
            std::thread::sleep(Duration::from_millis(25));
            fs::write(s.0.join("release"), b"").unwrap();
            let events = finish(&mut child);
            assert_eq!(terminal(&events).outcome, Outcome::Cancelled, "{events:?}");
            assert_eq!(terminal(&events).created_count, 0);
            assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, Message::Output { .. }))
            );
        }
    }
}

#[test]
fn xz_index_and_footer_claims_are_checked_before_publication() {
    for (case, error) in [
        ("index-size", ErrorCode::Integrity),
        ("footer-offset", ErrorCode::Integrity),
        ("index-huge", ErrorCode::Limit),
    ] {
        let s = fixture("xz", case);
        let events = run_case(&s, Limits::default());
        no_output(&s, &events, error);
    }
}

#[test]
fn compressed_tar_output_quota_boundary() {
    for kind in ["bz2", "xz"] {
        for below in [false, true] {
            let s = fixture(kind, "normal");
            let size: u64 = fs::read_to_string(s.0.join("plain-size"))
                .unwrap()
                .parse()
                .unwrap();
            let events = run_case(
                &s,
                Limits {
                    output: size - u64::from(below),
                    ..Limits::default()
                },
            );
            if below {
                no_output(&s, &events, ErrorCode::Limit);
            } else {
                assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
            }
        }
    }
}
