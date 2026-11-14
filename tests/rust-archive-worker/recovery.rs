use super::*;
use sha2::{Digest, Sha256};

fn start(s: &Scratch) -> Start {
    Start {
        action: Action::RecoverZip {
            archive: s.path("damaged.zip"),
            output_file: s.path("recovered.zip"),
        },
        limits: Limits::default(),
    }
}
fn hash(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn recovery_end(events: &[Message]) -> &RecoveryEnd {
    events
        .iter()
        .find_map(|e| {
            if let Message::RecoveryEnd(end) = e {
                Some(end)
            } else {
                None
            }
        })
        .unwrap()
}
#[test]
fn six_damaged_corpora_preserve_source_and_independent_expected_hashes() {
    let cases: &[(&str, &[u8], usize, bool)] = &[
        (
            "broken-local-header.zip",
            include_bytes!("../zip-recovery-fixtures/broken-local-header.zip"),
            6,
            true,
        ),
        (
            "missing-directory.zip",
            include_bytes!("../zip-recovery-fixtures/missing-directory.zip"),
            6,
            false,
        ),
        (
            "broken-eocd.zip",
            include_bytes!("../zip-recovery-fixtures/broken-eocd.zip"),
            6,
            false,
        ),
        (
            "middle-truncated.zip",
            include_bytes!("../zip-recovery-fixtures/middle-truncated.zip"),
            4,
            false,
        ),
        (
            "body-corrupt.zip",
            include_bytes!("../zip-recovery-fixtures/body-corrupt.zip"),
            5,
            false,
        ),
        (
            "unrecoverable.zip",
            include_bytes!("../zip-recovery-fixtures/unrecoverable.zip"),
            0,
            false,
        ),
    ];
    let expected = [
        (
            "hello.txt",
            "29020da72fa4030c58e0303f558b922f6ed5b6766600e67d79e0abf13971f9da",
        ),
        (
            "한글/자료.txt",
            "3fd2605633caf264a9cf9eb1fc23abc4a60b3191fc808a6a0f71863c64690be5",
        ),
        (
            "empty.txt",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            "empty-dir",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            "middle.bin",
            "785b0751fc2c53dc14a4ce3d800e69ef9ce1009eb327ccf458afe09c242c26c9",
        ),
        (
            "after.txt",
            "7ea5f891c2d1be8db73893862b433f6680e33e11a7246baeea0addd8d296975c",
        ),
    ];
    for &(sample, input, count, complete) in cases {
        let s = Scratch::new();
        fs::write(s.0.join("damaged.zip"), input).unwrap();
        let events = execute(start(&s));
        let t = terminal(&events);
        assert_eq!(fs::read(s.0.join("damaged.zip")).unwrap(), input);
        let end = recovery_end(&events);
        assert_eq!(end.source_sha256, <[u8; 32]>::from(Sha256::digest(input)));
        assert_eq!(end.verified_entries as usize, count);
        assert_eq!(end.unknown_loss, !complete);
        assert_eq!(end.known_entries, if complete { Some(6) } else { None });
        assert!(!events.iter().any(|e| matches!(
            e,
            Message::Entry(_)
                | Message::ListEnd { .. }
                | Message::PasswordRequired { .. }
                | Message::ConflictRequired { .. }
        )));
        assert!(matches!(
            events[0],
            Message::Accepted {
                action: ActionKind::RecoverZip,
                encryption: false
            }
        ));
        if count == 0 {
            assert_eq!(t.error, ErrorCode::RecoveryImpossible);
            assert_eq!(t.publication, Publication::None);
            assert!(!s.0.join("recovered.zip").exists());
            continue;
        }
        assert_eq!(
            t.error,
            if complete {
                ErrorCode::None
            } else {
                ErrorCode::RecoveryIncomplete
            }
        );
        assert_eq!(
            t.outcome,
            if complete {
                Outcome::Completed
            } else {
                Outcome::Failed
            }
        );
        assert_eq!(t.created_count, 1);
        assert_eq!(t.publication, Publication::Complete);
        let bytes = fs::read(s.0.join("recovered.zip")).unwrap();
        let catalog = archive::parse(&bytes, Limits::default(), true, &mut || Ok(())).unwrap();
        assert_eq!(catalog.members.len(), count);
        for member in &catalog.members {
            let (_, sha) = expected
                .iter()
                .find(|(name, _)| *name == member.entry.name)
                .unwrap();
            assert_eq!(&hash(&member.data), sha);
        }
        let checker = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/zip-recovery-fixtures/verify.py");
        let checked = Command::new("python3")
            .arg("-B")
            .arg(checker)
            .arg("--recovered")
            .arg(sample)
            .arg(s.0.join("recovered.zip"))
            .output()
            .unwrap();
        assert!(checked.status.success(), "{:?}", checked);
        let findings: Vec<_> = events
            .iter()
            .filter_map(|e| {
                if let Message::RecoveryEntry(f) = e {
                    Some(f)
                } else {
                    None
                }
            })
            .collect();
        for (i, f) in findings.iter().enumerate() {
            assert_eq!(f.candidate_id, i as u32);
            assert!(f.valid());
            assert!(f.source_offset < end.source_bytes);
            if i > 0 {
                assert!(findings[i - 1].source_offset < f.source_offset);
            }
        }
        if count == 4 {
            assert!(
                !findings
                    .iter()
                    .any(|f| f.name.as_deref() == Some("after.txt"))
            );
        }
        if count == 5 {
            assert!(findings.iter().any(
                |f| f.name.as_deref() == Some("middle.bin") && f.status == RecoveryStatus::Lost
            ));
        }
    }
}
#[test]
fn healthy_source_and_existing_destination_are_never_rewritten() {
    let s = Scratch::new();
    let bytes = include_bytes!("../zip-recovery-fixtures/control.zip");
    fs::write(s.0.join("damaged.zip"), bytes).unwrap();
    let events = execute(start(&s));
    assert_eq!(terminal(&events).error, ErrorCode::InvalidRequest);
    assert!(!s.0.join("recovered.zip").exists());
    fs::write(s.0.join("recovered.zip"), b"keep").unwrap();
    let events = execute(start(&s));
    assert_eq!(terminal(&events).error, ErrorCode::Conflict);
    assert_eq!(fs::read(s.0.join("recovered.zip")).unwrap(), b"keep");
    assert_eq!(fs::read(s.0.join("damaged.zip")).unwrap(), bytes);
}
#[test]
fn lower_input_output_and_candidate_quotas_leave_no_artifact() {
    for kind in 0..3 {
        let s = Scratch::new();
        fs::write(
            s.0.join("damaged.zip"),
            include_bytes!("../zip-recovery-fixtures/missing-directory.zip"),
        )
        .unwrap();
        let mut request = start(&s);
        match kind {
            0 => request.limits.input = 100,
            1 => request.limits.output = 100,
            _ => request.limits.entries = 2,
        }
        let events = execute(request);
        assert_eq!(terminal(&events).error, ErrorCode::Limit);
        assert!(!s.0.join("recovered.zip").exists());
    }
}
#[test]
fn unsafe_names_unsupported_input_and_payload_signatures_do_not_trigger_scanning() {
    let original = include_bytes!("../zip-recovery-fixtures/missing-directory.zip");
    for kind in 0..4 {
        let s = Scratch::new();
        let mut bytes = original.to_vec();
        let expected = match kind {
            0 => {
                bytes[30..39].copy_from_slice(b"../x.txt/");
                ErrorCode::UnsafePath
            }
            1 => {
                bytes[18..22].copy_from_slice(&u32::MAX.to_le_bytes());
                ErrorCode::UnsupportedFeature
            }
            2 => {
                bytes[..2].copy_from_slice(b"MZ");
                ErrorCode::UnsupportedFeature
            }
            _ => {
                bytes[39..43].copy_from_slice(b"PK\x03\x04");
                ErrorCode::RecoveryIncomplete
            }
        };
        fs::write(s.0.join("damaged.zip"), &bytes).unwrap();
        let events = execute(start(&s));
        assert_eq!(terminal(&events).error, expected);
        if kind == 3 {
            assert_eq!(recovery_end(&events).verified_entries, 5);
        } else {
            assert!(!s.0.join("recovered.zip").exists());
        }
        assert_eq!(fs::read(s.0.join("damaged.zip")).unwrap(), bytes);
    }
}
#[test]
fn recovery_wire_rejects_interactive_modes_invalid_hashes_and_summary_overflow() {
    let s = Scratch::new();
    let request = start(&s);
    assert!(
        Writer::new(Vec::new(), 17)
            .send(&Message::StartInteractive(request.clone()))
            .is_err()
    );
    let guarded = GuardedStart {
        start: request,
        ceiling: Limits::default(),
        interactive: true,
    };
    assert!(!guarded.valid());
    assert!(
        Writer::new(Vec::new(), 17)
            .send(&Message::StartGuarded(guarded))
            .is_err()
    );
    let good = RecoveryEntry {
        candidate_id: 0,
        source_offset: 0,
        name: Some("ok".into()),
        kind: Some(EntryKind::File),
        status: RecoveryStatus::Verified,
        reason: RecoveryReason::HeaderRebuilt,
        declared_size: Some(0),
        verified_size: Some(0),
        sha256: Some([0; 32]),
    };
    let mut bytes = Vec::new();
    Writer::new(&mut bytes, 17)
        .send(&Message::RecoveryEntry(good.clone()))
        .unwrap();
    assert_eq!(u16::from_le_bytes(bytes[6..8].try_into().unwrap()), 115);
    assert!(
        matches!(Reader::new(bytes.as_slice()).read().unwrap().unwrap().2,Message::RecoveryEntry(f) if f==good)
    );
    let mut bad = good;
    bad.sha256 = None;
    assert!(
        Writer::new(Vec::new(), 17)
            .send(&Message::RecoveryEntry(bad))
            .is_err()
    );
    let summary = RecoveryEnd {
        mode: RecoveryMode::LocalChain,
        source_bytes: 10,
        source_sha256: [0; 32],
        known_entries: None,
        verified_entries: u32::MAX,
        lost_entries: 1,
        unverified_entries: 0,
        unknown_loss: true,
    };
    assert!(!summary.valid());
    assert!(
        Writer::new(Vec::new(), 17)
            .send(&Message::RecoveryEnd(summary))
            .is_err()
    );
}

#[cfg(target_os = "linux")]
fn shim(s: &Scratch) -> PathBuf {
    let library = s.0.join("fault.so");
    assert!(
        Command::new("cc")
            .args(["-shared", "-fPIC", "-O2"])
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/rust-archive-worker/io_fault.c")
            )
            .arg("-ldl")
            .arg("-o")
            .arg(&library)
            .status()
            .unwrap()
            .success()
    );
    library
}
#[cfg(target_os = "linux")]
#[test]
fn recovery_disk_errors_preserve_publication_truth_and_source() {
    for mode in [
        "temp-full",
        "codec-full",
        "create-full",
        "full",
        "write-io",
        "sync-full",
        "dir-sync-full",
    ] {
        let s = Scratch::new();
        let original = include_bytes!("../zip-recovery-fixtures/missing-directory.zip");
        fs::write(s.0.join("damaged.zip"), original).unwrap();
        let mut command = s.worker();
        command
            .env("LD_PRELOAD", shim(&s))
            .env("ZIPELO_TEST_FAULT", mode)
            .env("ZIPELO_TEST_MARKER", s.0.join("marker"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        Writer::new(child.stdin.take().unwrap(), 17)
            .send(&Message::Start(start(&s)))
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let mut reader = Reader::new(output.stdout.as_slice());
        let mut events = Vec::new();
        while let Some((_, _, event)) = reader.read().unwrap() {
            events.push(event);
        }
        let t = terminal(&events);
        assert_eq!(t.outcome, Outcome::Failed, "{mode}: {events:?}");
        assert_eq!(
            t.error,
            if mode == "write-io" {
                ErrorCode::Io
            } else {
                ErrorCode::DiskFull
            },
            "{mode}: {events:?}"
        );
        assert_eq!(fs::read(s.0.join("damaged.zip")).unwrap(), original);
        assert_eq!(s.0.join("recovered.zip").exists(), mode == "dir-sync-full");
        assert_eq!(t.created_count, u64::from(mode == "dir-sync-full"));
        assert!(!fs::read_dir(&s.0).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".zipelo-")
        }));
    }
}
#[cfg(target_os = "linux")]
#[test]
fn recovery_publication_rechecks_source_destination_and_cancel() {
    use std::time::{Duration, Instant};
    struct Running(std::process::Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for mode in 0..3 {
        let s = Scratch::new();
        let original = include_bytes!("../zip-recovery-fixtures/missing-directory.zip");
        fs::write(s.0.join("damaged.zip"), original).unwrap();
        let marker = s.0.join("marker");
        let release = s.0.join("release");
        let mut cmd = s.worker();
        cmd.env("LD_PRELOAD", shim(&s))
            .env("ZIPELO_TEST_FAULT", "pause")
            .env("ZIPELO_TEST_MARKER", &marker)
            .env("ZIPELO_TEST_RELEASE", &release)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = Running(cmd.spawn().unwrap());
        let mut writer = Writer::new(child.0.stdin.take().unwrap(), 17);
        writer.send(&Message::Start(start(&s))).unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while !marker.exists() {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut reader = Reader::new(child.0.stdout.take().unwrap());
        let mut events = Vec::new();
        let mut expected = original.to_vec();
        if mode == 0 {
            expected[40] ^= 1;
            fs::write(s.0.join("damaged.zip"), &expected).unwrap();
        }
        if mode == 1 {
            fs::write(s.0.join("recovered.zip"), b"user save").unwrap();
        }
        if mode == 2 {
            writer.send(&Message::Cancel).unwrap();
            loop {
                let event = reader.read().unwrap().unwrap().2;
                let ack = matches!(
                    event,
                    Message::CancelAck {
                        status: CancelStatus::Requested,
                        ..
                    }
                );
                events.push(event);
                if ack {
                    break;
                }
            }
        }
        fs::write(&release, b"go").unwrap();
        while let Some((_, _, event)) = reader.read().unwrap() {
            events.push(event);
        }
        drop(writer);
        assert!(child.0.wait().unwrap().success());
        let t = terminal(&events);
        assert_eq!(
            t.error,
            match mode {
                0 => ErrorCode::SourceChanged,
                1 => ErrorCode::Conflict,
                _ => ErrorCode::None,
            }
        );
        assert_eq!(
            t.outcome,
            if mode == 2 {
                Outcome::Cancelled
            } else {
                Outcome::Failed
            }
        );
        assert_eq!(t.created_count, 0);
        assert_eq!(t.publication, Publication::None);
        assert_eq!(fs::read(s.0.join("damaged.zip")).unwrap(), expected);
        if mode == 1 {
            assert_eq!(fs::read(s.0.join("recovered.zip")).unwrap(), b"user save");
        } else {
            assert!(!s.0.join("recovered.zip").exists());
        }
    }
}
