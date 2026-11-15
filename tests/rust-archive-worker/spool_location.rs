use super::*;

// 해제 작업의 원본 사본과 단계 임시 파일은 시스템 임시 폴더가 아니라 출력 폴더에 둠.
// 시스템 임시 폴더를 쓸 수 없게 만들어도(/tmp 할당량 초과와 같은 상황) 해제가 성공해야 함.
fn extract_without_system_temp(bytes: &[u8]) -> (Scratch, Vec<Message>) {
    extract_unusable_system_temp(bytes, None, false)
}
fn extract_unusable_system_temp(
    bytes: &[u8],
    fault: Option<&str>,
    file_at_temp_path: bool,
) -> (Scratch, Vec<Message>) {
    let s = Scratch::new();
    let start = encrypted_start(&s, bytes);
    let temp = s.0.join("missing-temp");
    if file_at_temp_path {
        fs::write(&temp, b"keep").unwrap();
    }
    let mut command = s.worker();
    command.env("TMPDIR", &temp);
    if let Some(fault) = fault {
        let shim = s.0.join("io_fault.so");
        assert!(
            Command::new("cc")
                .args(["-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror"])
                .arg(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../tests/rust-archive-worker/io_fault.c")
                )
                .args(["-ldl", "-o"])
                .arg(&shim)
                .status()
                .unwrap()
                .success()
        );
        command
            .env("LD_PRELOAD", shim)
            .env("ZIPELO_TEST_FAULT", fault);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut writer = Some(Writer::new(child.stdin.take().unwrap(), 17));
    writer
        .as_mut()
        .unwrap()
        .send(&Message::StartInteractive(start))
        .unwrap();
    let mut reader = Reader::new(child.stdout.take().unwrap());
    let mut events = Vec::new();
    while let Some((job, _, event)) = reader.read().unwrap() {
        assert_eq!(job, 17);
        if let Message::PasswordRequired { challenge_id, .. } = &event {
            for reply in password_reply(*challenge_id, TEST_PASSWORD) {
                writer.as_mut().unwrap().send(&reply).unwrap();
            }
        }
        if matches!(event, Message::Terminal(_)) {
            writer = None;
        }
        events.push(event);
    }
    drop(writer);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty());
    if file_at_temp_path {
        assert_eq!(fs::read(&temp).unwrap(), b"keep");
    } else {
        assert!(!temp.exists());
    }
    (s, events)
}

fn assert_only_outputs(s: &Scratch, expected: &[&str]) {
    let mut names = fs::read_dir(s.0.join("output"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, expected, "임시 파일이 출력 폴더에 남으면 안 됨");
}

#[test]
fn extract_spools_in_destination_when_system_temp_is_unusable() {
    let (s, events) = extract_without_system_temp(include_bytes!("fixtures/deflate.zip"));
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(terminal(&events).publication, Publication::Complete);
    assert_eq!(
        fs::read(s.0.join("output/folder/hello.txt")).unwrap(),
        PAYLOAD
    );
    assert_only_outputs(&s, &["empty", "folder"]);
}

#[test]
fn encrypted_extract_spools_in_destination_when_system_temp_is_unusable() {
    let (s, events) = extract_without_system_temp(AES_ZIP);
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(
        fs::read(s.0.join("output/payload.bin")).unwrap(),
        ENCRYPTED_PAYLOAD
    );
    assert_only_outputs(&s, &["payload.bin"]);
}

#[test]
fn corrupt_archive_error_wins_over_read_only_destination() {
    // 출력 폴더에 임시 파일을 만들 수 없으면 시스템 임시 폴더를 써서 기존 오류 우선순위를 유지함.
    // 손상된 압축파일은 권한 문제가 아니라 손상으로 안내해야 함.
    let bytes = seven_fixture("bad-trailing.7z");
    let writable = Scratch::new();
    let baseline = extract(
        &writable,
        &bytes,
        None,
        Selection::All,
        None,
        Limits::default(),
    );
    assert_eq!(terminal(&baseline).error, ErrorCode::Integrity);

    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new();
    let output = s.0.join("output");
    fs::create_dir(&output).unwrap();
    fs::set_permissions(&output, fs::Permissions::from_mode(0o500)).unwrap();
    let events = extract(&s, &bytes, None, Selection::All, None, Limits::default());
    fs::set_permissions(&output, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(terminal(&events).error, ErrorCode::Integrity, "{events:?}");
    assert_eq!(terminal(&events).publication, Publication::None);
    assert_eq!(terminal(&events).cleanup, Cleanup::Complete);
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
}

#[test]
fn destination_space_or_quota_failure_survives_unusable_system_temp() {
    let input = include_bytes!("fixtures/deflate.zip");
    for fault in [
        "destination-enospc",
        "destination-edquot",
        "destination-named-enospc",
        "destination-named-edquot",
    ] {
        for file in [false, true] {
            let (s, events) = extract_unusable_system_temp(input, Some(fault), file);
            let terminal = terminal(&events);
            assert_eq!(
                terminal.outcome,
                Outcome::Failed,
                "{fault}/{file}: {events:?}"
            );
            assert_eq!(
                terminal.error,
                ErrorCode::DiskFull,
                "{fault}/{file}: {events:?}"
            );
            assert_eq!(terminal.publication, Publication::None);
            assert_eq!(terminal.cleanup, Cleanup::Complete);
            assert_eq!(terminal.created_count, 0);
            assert_only_outputs(&s, &[]);
            assert_eq!(fs::read(s.0.join("input")).unwrap(), input);
        }
    }
}

#[test]
fn destination_permission_failure_with_unusable_system_temp_stays_io() {
    for file in [false, true] {
        let (s, events) = extract_unusable_system_temp(
            include_bytes!("fixtures/deflate.zip"),
            Some("destination-denied"),
            file,
        );
        assert_eq!(terminal(&events).error, ErrorCode::Io, "{events:?}");
        assert_eq!(terminal(&events).outcome, Outcome::Failed);
        assert_only_outputs(&s, &[]);
    }
}
