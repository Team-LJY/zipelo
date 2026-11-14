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

#[test]
fn central_comment_locator_bytes_do_not_reject_zip32_recovery() {
    let source = include_bytes!("../zip-recovery-fixtures/broken-local-header.zip");
    for comment_len in [20usize, 37] {
        for archive_comment_len in [0usize, 41] {
            let mut input = source[..source.len() - 22].to_vec();
            let last = input.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
            input[last + 32..last + 34].copy_from_slice(&(comment_len as u16).to_le_bytes());
            let mut comment = vec![0; comment_len];
            comment[comment_len - 20..comment_len - 16].copy_from_slice(b"PK\x06\x07");
            input.extend(comment);
            let mut end = source[source.len() - 22..].to_vec();
            let size = u32::from_le_bytes(end[12..16].try_into().unwrap());
            end[12..16].copy_from_slice(&(size + comment_len as u32).to_le_bytes());
            end[20..22].copy_from_slice(&(archive_comment_len as u16).to_le_bytes());
            input.extend(end);
            input.extend(vec![0; archive_comment_len]);
            let s = Scratch::new();
            fs::write(s.0.join("damaged.zip"), &input).unwrap();
            let events = execute(start(&s));
            let t = terminal(&events);
            assert_eq!(t.error, ErrorCode::None);
            assert_eq!(t.outcome, Outcome::Completed);
            assert_eq!(t.publication, Publication::Complete);
            assert_eq!(t.created_count, 1);
            let end = recovery_end(&events);
            assert!(end.complete());
            assert_eq!(end.verified_entries, 6);
            assert_eq!(end.source_sha256, <[u8; 32]>::from(Sha256::digest(&input)));
            assert_eq!(fs::read(s.0.join("damaged.zip")).unwrap(), input);
            assert!(
                Command::new("python3")
                    .arg("-B")
                    .arg(
                        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join("../../tests/zip-recovery-fixtures/verify.py")
                    )
                    .args(["--recovered", "broken-local-header.zip"])
                    .arg(s.0.join("recovered.zip"))
                    .status()
                    .unwrap()
                    .success()
            );
        }
    }
}

#[test]
fn fake_eocd_in_archive_comment_keeps_real_central_directory() {
    let source = include_bytes!("../zip-recovery-fixtures/broken-local-header.zip");
    for fake_kind in 0..7 {
        for locator in [false, true] {
            let mut comment = Vec::new();
            if locator {
                comment.extend(b"PK\x06\x07");
                comment.extend([0; 16]);
            }
            let mut fake = [0u8; 22];
            fake[..4].copy_from_slice(b"PK\x05\x06");
            match fake_kind {
                0 => fake[4..6].copy_from_slice(&1u16.to_le_bytes()),
                1 => fake[10..12].copy_from_slice(&u16::MAX.to_le_bytes()),
                2 => fake[16..20].copy_from_slice(&u32::MAX.to_le_bytes()),
                3 => fake[12..16].copy_from_slice(&u32::MAX.to_le_bytes()),
                4 => {
                    fake[12..16].copy_from_slice(&46u32.to_le_bytes());
                    fake[16..20].copy_from_slice(&39u32.to_le_bytes());
                }
                5 => {
                    fake[12..16].copy_from_slice(&340u32.to_le_bytes());
                    fake[16..20].copy_from_slice(&1372u32.to_le_bytes());
                }
                _ => {
                    fake[4..6].copy_from_slice(&1u16.to_le_bytes());
                    let size = (source.len() + comment.len() - 1372) as u32;
                    fake[12..16].copy_from_slice(&size.to_le_bytes());
                    fake[16..20].copy_from_slice(&1372u32.to_le_bytes());
                }
            }
            comment.extend(fake);
            let mut input = source.to_vec();
            let end = input.len() - 22;
            input[end + 20..end + 22].copy_from_slice(&(comment.len() as u16).to_le_bytes());
            input.extend(comment);
            let s = Scratch::new();
            fs::write(s.0.join("damaged.zip"), &input).unwrap();
            let events = execute(start(&s));
            let terminal = terminal(&events);
            assert_eq!(terminal.error, ErrorCode::None, "{fake_kind}/{locator}");
            assert_eq!(terminal.outcome, Outcome::Completed);
            assert_eq!(terminal.publication, Publication::Complete);
            let summary = recovery_end(&events);
            assert!(summary.complete());
            assert_eq!(summary.mode, RecoveryMode::CentralDirectory);
            assert_eq!(summary.known_entries, Some(6));
            assert_eq!(
                summary.source_sha256,
                <[u8; 32]>::from(Sha256::digest(&input))
            );
            assert_eq!(fs::read(s.0.join("damaged.zip")).unwrap(), input);
            let checked = Command::new("python3")
                .arg("-B")
                .arg(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../tests/zip-recovery-fixtures/verify.py"),
                )
                .args(["--recovered", "broken-local-header.zip"])
                .arg(s.0.join("recovered.zip"))
                .output()
                .unwrap();
            assert!(checked.status.success(), "{checked:?}");
        }
    }
}

#[test]
fn nested_central_directories_in_comments_cannot_hide_original_members() {
    let source = include_bytes!("../zip-recovery-fixtures/broken-local-header.zip");
    for entries in [1usize, 3, 6] {
        let mut stop = 1372;
        for _ in 0..entries {
            stop += 46
                + u16::from_le_bytes(source[stop + 28..stop + 30].try_into().unwrap()) as usize
                + u16::from_le_bytes(source[stop + 30..stop + 32].try_into().unwrap()) as usize
                + u16::from_le_bytes(source[stop + 32..stop + 34].try_into().unwrap()) as usize;
        }
        for depth in [1, 3] {
            for locator in [false, true] {
                for maximum_comment in [false, true] {
                    let mut input = source.to_vec();
                    let mut ends = vec![source.len() - 22];
                    for _ in 0..depth {
                        let cd = input.len();
                        input.extend(&source[1372..stop]);
                        if locator {
                            input.extend(b"PK\x06\x07");
                            input.extend([0; 16]);
                        }
                        let mut end = source[source.len() - 22..].to_vec();
                        end[8..10].copy_from_slice(&(entries as u16).to_le_bytes());
                        end[10..12].copy_from_slice(&(entries as u16).to_le_bytes());
                        end[12..16].copy_from_slice(&((stop - 1372) as u32).to_le_bytes());
                        end[16..20].copy_from_slice(&(cd as u32).to_le_bytes());
                        ends.push(input.len());
                        input.extend(end);
                    }
                    if maximum_comment {
                        input.resize(source.len() + u16::MAX as usize, 0);
                    }
                    let len = input.len();
                    for end in ends {
                        input[end + 20..end + 22]
                            .copy_from_slice(&((len - end - 22) as u16).to_le_bytes());
                    }
                    assert_full_comment_recovery(
                        &input,
                        &format!("{entries}/{depth}/{locator}/{maximum_comment}"),
                    );
                }
            }
        }
    }
}

fn assert_full_comment_recovery(input: &[u8], case: &str) {
    let s = Scratch::new();
    fs::write(s.0.join("damaged.zip"), input).unwrap();
    let events = execute(start(&s));
    let terminal = terminal(&events);
    assert_eq!(terminal.error, ErrorCode::None, "{case}");
    assert_eq!(terminal.outcome, Outcome::Completed);
    assert_eq!(terminal.publication, Publication::Complete);
    assert_eq!(terminal.created_count, 1);
    let summary = recovery_end(&events);
    assert!(summary.complete());
    assert_eq!(summary.mode, RecoveryMode::CentralDirectory);
    assert_eq!(summary.known_entries, Some(6));
    assert_eq!(summary.verified_entries, 6);
    assert_eq!(
        summary.source_sha256,
        <[u8; 32]>::from(Sha256::digest(input))
    );
    assert_eq!(fs::read(s.0.join("damaged.zip")).unwrap(), input);
    let checked = Command::new("python3")
        .arg("-B")
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/zip-recovery-fixtures/verify.py"),
        )
        .args(["--recovered", "broken-local-header.zip"])
        .arg(s.0.join("recovered.zip"))
        .output()
        .unwrap();
    assert!(checked.status.success(), "{case}: {checked:?}");
}

#[test]
fn nested_directory_in_central_entry_comment_is_not_an_outer_archive() {
    let source = include_bytes!("../zip-recovery-fixtures/broken-local-header.zip");
    for wraps_real_end in [false, true] {
        let mut fake_cd = source[1372..1427].to_vec();
        if wraps_real_end {
            fake_cd[32..34].copy_from_slice(&22u16.to_le_bytes());
        }
        let mut input = source[..source.len() - 22].to_vec();
        let last = input.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
        let comment_len = fake_cd.len() + if wraps_real_end { 0 } else { 22 };
        input[last + 32..last + 34].copy_from_slice(&(comment_len as u16).to_le_bytes());
        let fake_offset = input.len();
        input.extend(&fake_cd);
        let mut real_end = source[source.len() - 22..].to_vec();
        real_end[12..16].copy_from_slice(&((340 + comment_len) as u32).to_le_bytes());
        let mut fake_end = source[source.len() - 22..].to_vec();
        fake_end[8..10].copy_from_slice(&1u16.to_le_bytes());
        fake_end[10..12].copy_from_slice(&1u16.to_le_bytes());
        fake_end[12..16].copy_from_slice(
            &((fake_cd.len() + if wraps_real_end { 22 } else { 0 }) as u32).to_le_bytes(),
        );
        fake_end[16..20].copy_from_slice(&(fake_offset as u32).to_le_bytes());
        if wraps_real_end {
            real_end[20..22].copy_from_slice(&22u16.to_le_bytes());
            input.extend(real_end);
            input.extend(fake_end);
        } else {
            fake_end[20..22].copy_from_slice(&22u16.to_le_bytes());
            input.extend(fake_end);
            input.extend(real_end);
        }
        assert_full_comment_recovery(&input, &format!("central-comment/{wraps_real_end}"));
    }
}
