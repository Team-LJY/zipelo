use super::*;

// 해제 작업의 원본 사본과 단계 임시 파일은 시스템 임시 폴더가 아니라 출력 폴더에 둠.
// 시스템 임시 폴더를 쓸 수 없게 만들어도(/tmp 할당량 초과와 같은 상황) 해제가 성공해야 함.
fn extract_without_system_temp(bytes: &[u8]) -> (Scratch, Vec<Message>) {
    let s = Scratch::new();
    let start = encrypted_start(&s, bytes);
    let mut child = s
        .worker()
        .env("TMPDIR", s.0.join("missing-temp"))
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
    assert!(!s.0.join("missing-temp").exists());
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
