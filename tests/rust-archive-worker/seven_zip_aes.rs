use super::*;

/// 실제 7-Zip 26.00이 `-p`로 만든 분할 표본(split7z)의 공개 시험 암호.
const SPLIT_PASSWORD: &[u8] = b"zipelo-split-test";
/// seven_zip_aes_fixtures.py가 만든 합성 표본의 공개 시험 암호.
const SYNTHETIC_PASSWORD: &str = "zipelo-암호";

/// 실제 7-Zip 분할 표본의 조각을 이어 붙이면 같은 단일 7z가 됨.
fn joined_reference(name: &str) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/rust-archive-worker/fixtures/split7z");
    let mut bytes = Vec::new();
    for index in 1..=999 {
        match fs::read(root.join(format!("{name}.7z.{index:03}"))) {
            Ok(part) => bytes.extend(part),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => panic!("{error:?}"),
        }
    }
    assert!(bytes.starts_with(b"7z\xbc\xaf\x27\x1c"));
    bytes
}
fn reference_files() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("folder/hello.txt", SEVEN_PAYLOAD.repeat(100)),
        (
            "second.bin",
            (0..4096usize)
                .map(|index| ((index * 37 + (index >> 3)) % 256) as u8)
                .collect(),
        ),
    ]
}
fn synthetic_files() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("folder/empty", Vec::new()),
        (
            "folder/hello.txt",
            b"Zipelo 7z AES interoperability\n".repeat(60),
        ),
        (
            "한글.bin",
            (0..3000usize)
                .map(|index| ((index * 31 + (index >> 4)) % 256) as u8)
                .collect(),
        ),
    ]
}
fn list_start(s: &Scratch, bytes: &[u8]) -> Start {
    fs::write(s.0.join("input"), bytes).unwrap();
    Start {
        action: Action::List {
            archive: s.path("input"),
        },
        limits: Limits::default(),
    }
}
fn prompts(events: &[Message]) -> Vec<u32> {
    events
        .iter()
        .filter_map(|event| match event {
            Message::PasswordRequired {
                attempt, purpose, ..
            } => {
                assert_eq!(*purpose, PasswordPurpose::Decrypt);
                Some(*attempt)
            }
            _ => None,
        })
        .collect()
}
fn entries(events: &[Message]) -> Vec<Entry> {
    events
        .iter()
        .filter_map(|event| match event {
            Message::Entry(entry) => Some(entry.clone()),
            _ => None,
        })
        .collect()
}
fn assert_files(s: &Scratch, expected: &[(&str, Vec<u8>)]) {
    for (name, data) in expected {
        assert_eq!(
            &fs::read(s.0.join("output").join(name)).unwrap(),
            data,
            "{name}"
        );
    }
}

#[test]
fn real_encrypted_seven_zip_lists_without_password_and_extracts_with_one_prompt() {
    let bytes = joined_reference("encrypted-body");
    let s = Scratch::new();
    let listing = execute(list_start(&s, &bytes));
    assert_eq!(
        terminal(&listing).outcome,
        Outcome::Completed,
        "{listing:?}"
    );
    assert!(prompts(&listing).is_empty());
    let listed = entries(&listing);
    assert!(
        listed
            .iter()
            .filter(|e| e.kind == EntryKind::File)
            .all(|e| e.encrypted)
    );
    let start = encrypted_start(&s, &bytes);
    let events = interactive(start, |id, _, _| password_reply(id, SPLIT_PASSWORD));
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(prompts(&events), [1]);
    assert_files(&s, &reference_files());
}

#[test]
fn encrypted_seven_zip_headers_ask_once_for_listing_and_reuse_for_extraction() {
    for (bytes, password, expected) in [
        (
            joined_reference("encrypted-header"),
            SPLIT_PASSWORD.to_vec(),
            reference_files(),
        ),
        (
            seven_fixture("aes-lzma-header.7z"),
            SYNTHETIC_PASSWORD.as_bytes().to_vec(),
            synthetic_files(),
        ),
    ] {
        let s = Scratch::new();
        let listing = interactive(list_start(&s, &bytes), |id, _, _| {
            password_reply(id, &password)
        });
        assert_eq!(
            terminal(&listing).outcome,
            Outcome::Completed,
            "{listing:?}"
        );
        assert_eq!(prompts(&listing), [1]);
        let listed = entries(&listing);
        assert!(!listed.is_empty());
        assert!(
            listed
                .iter()
                .filter(|e| e.kind == EntryKind::File && e.size != 0)
                .all(|e| e.encrypted)
        );
        // 암호 없이 목록을 요청하면 이름을 보이지 않고 틀린 암호로 끝남.
        let refused = interactive(list_start(&s, &bytes), |_, _, _| Vec::new());
        assert_eq!(terminal(&refused).outcome, Outcome::Failed);
        assert!(entries(&refused).is_empty());
        let start = encrypted_start(&s, &bytes);
        let events = interactive(start, |id, _, _| password_reply(id, &password));
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        assert_eq!(prompts(&events), [1]);
        assert_files(&s, &expected);
    }
}

#[test]
fn synthetic_copy_aes_folder_extracts_with_utf16_password() {
    let bytes = seven_fixture("aes-copy.7z");
    let s = Scratch::new();
    let listing = execute(list_start(&s, &bytes));
    assert_eq!(
        terminal(&listing).outcome,
        Outcome::Completed,
        "{listing:?}"
    );
    let listed = entries(&listing);
    assert_eq!(listed.len(), 4);
    assert!(
        listed
            .iter()
            .any(|e| e.kind == EntryKind::Directory && !e.encrypted)
    );
    assert!(listed.iter().any(|e| e.name == "한글.bin" && e.encrypted));
    let start = encrypted_start(&s, &bytes);
    let events = interactive(start, |id, _, _| {
        password_reply(id, SYNTHETIC_PASSWORD.as_bytes())
    });
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_files(&s, &synthetic_files());
}

#[test]
fn wrong_seven_zip_passwords_retry_then_fail_without_output() {
    for (bytes, password) in [
        (joined_reference("encrypted-body"), SPLIT_PASSWORD.to_vec()),
        (
            joined_reference("encrypted-header"),
            SPLIT_PASSWORD.to_vec(),
        ),
        (
            seven_fixture("aes-copy.7z"),
            SYNTHETIC_PASSWORD.as_bytes().to_vec(),
        ),
        (
            seven_fixture("aes-lzma-header.7z"),
            SYNTHETIC_PASSWORD.as_bytes().to_vec(),
        ),
    ] {
        let s = Scratch::new();
        let events = interactive(encrypted_start(&s, &bytes), |id, _, _| {
            password_reply(id, b"wrong-password")
        });
        assert_eq!(prompts(&events), [1, 2, 3]);
        no_encrypted_output(&s, &events, ErrorCode::WrongPassword);
        // 같은 작업 안에서 틀린 뒤 맞는 암호를 넣으면 이어서 풂.
        fs::remove_dir(s.0.join("output")).unwrap();
        let events = interactive(encrypted_start(&s, &bytes), |id, attempt, _| {
            password_reply(id, if attempt == 1 { b"wrong" } else { &password })
        });
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        assert_eq!(prompts(&events), [1, 2]);
    }
}

#[test]
fn damaged_or_costly_seven_zip_aes_is_not_reported_as_wrong_password() {
    let s = Scratch::new();
    let bytes = seven_fixture("aes-bad-packed.7z");
    let events = interactive(encrypted_start(&s, &bytes), |id, _, _| {
        password_reply(id, SYNTHETIC_PASSWORD.as_bytes())
    });
    no_encrypted_output(&s, &events, ErrorCode::Integrity);
    // 반복 지수가 상한(24)을 넘으면 키를 유도하기 전에 암호도 묻지 않고 거부함.
    let s = Scratch::new();
    let events = interactive(
        list_start(&s, &seven_fixture("aes-cycles.7z")),
        |id, _, _| password_reply(id, SYNTHETIC_PASSWORD.as_bytes()),
    );
    assert_eq!(terminal(&events).error, ErrorCode::UnsupportedFeature);
    assert!(prompts(&events).is_empty());
}
