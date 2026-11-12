use super::*;

// Info-ZIP 3.0(`zip -X`, UTF-8 로캘)이 만든 표본은 UTF-8 플래그 없이 UTF-8 이름을 기록함.
const KOREAN: &[u8] = include_bytes!("fixtures/infozip-korean.zip");
const KOREAN_TREE: &[u8] = include_bytes!("fixtures/infozip-korean-tree.zip");
const ZIPCRYPTO_ASCII_TREE: &[u8] = include_bytes!("fixtures/infozip-zipcrypto-ascii-tree.zip");
const ZIPCRYPTO_KOREAN_TREE: &[u8] = include_bytes!("fixtures/infozip-zipcrypto-korean-tree.zip");
const NOTE: &[u8] = b"Zipelo encrypted interoperability\n";

fn crc(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Info-ZIP Unicode Path 확장 필드(0x7075)를 만듦.
fn unicode_path(raw: &[u8], name: &str) -> Vec<u8> {
    let mut value = vec![1];
    value.extend(crc(raw).to_le_bytes());
    value.extend(name.as_bytes());
    let mut extra = 0x7075u16.to_le_bytes().to_vec();
    extra.extend((value.len() as u16).to_le_bytes());
    extra.extend(value);
    extra
}

/// 저장 방식 항목 하나짜리 ZIP을 만듦. 확장 필드는 로컬과 중앙 헤더에 똑같이 기록함.
fn single_entry_zip(raw: &[u8], flags: u16, extra: &[u8], data: &[u8]) -> Vec<u8> {
    let checksum = crc(data);
    let mut output = b"PK\x03\x04".to_vec();
    for value in [20u16, flags, 0, 0, 33] {
        output.extend(value.to_le_bytes());
    }
    for value in [checksum, data.len() as u32, data.len() as u32] {
        output.extend(value.to_le_bytes());
    }
    output.extend((raw.len() as u16).to_le_bytes());
    output.extend((extra.len() as u16).to_le_bytes());
    output.extend(raw);
    output.extend(extra);
    output.extend(data);
    let central_at = output.len() as u32;
    let mut central = b"PK\x01\x02".to_vec();
    for value in [0x0314u16, 20, flags, 0, 0, 33] {
        central.extend(value.to_le_bytes());
    }
    for value in [checksum, data.len() as u32, data.len() as u32] {
        central.extend(value.to_le_bytes());
    }
    central.extend((raw.len() as u16).to_le_bytes());
    for value in [extra.len() as u16, 0, 0, 0] {
        central.extend(value.to_le_bytes());
    }
    central.extend((0o100644u32 << 16).to_le_bytes());
    central.extend(0u32.to_le_bytes());
    central.extend(raw);
    central.extend(extra);
    output.extend(&central);
    output.extend(b"PK\x05\x06");
    for value in [0u16, 0, 1, 1] {
        output.extend(value.to_le_bytes());
    }
    output.extend((central.len() as u32).to_le_bytes());
    output.extend(central_at.to_le_bytes());
    output.extend(0u16.to_le_bytes());
    output
}

fn listed_names(bytes: &[u8]) -> std::result::Result<Vec<String>, ErrorCode> {
    let s = Scratch::new();
    fs::write(s.0.join("input"), bytes).unwrap();
    let events = execute(Start {
        action: Action::List {
            archive: s.path("input"),
        },
        limits: Limits::default(),
    });
    let result = terminal(&events);
    if result.outcome != Outcome::Completed {
        return Err(result.error);
    }
    Ok(events
        .iter()
        .filter_map(|e| match e {
            Message::Entry(entry) => Some(entry.name.clone()),
            _ => None,
        })
        .collect())
}

/// 스트리밍 목록과 메모리 파서가 같은 이름 또는 같은 오류를 내는지 확인함.
fn names_agree(bytes: &[u8]) -> std::result::Result<Vec<String>, ErrorCode> {
    let streamed = listed_names(bytes);
    let parsed = archive::parse(bytes, Limits::default(), false, &mut || Ok(()))
        .map(|c| c.members.into_iter().map(|m| m.entry.name).collect());
    assert_eq!(streamed, parsed);
    streamed
}

#[test]
fn infozip_utf8_names_without_flag_list_and_extract() {
    assert_eq!(names_agree(KOREAN).unwrap(), ["README.md", "회의록.txt"]);
    assert_eq!(
        names_agree(KOREAN_TREE).unwrap(),
        ["README.md", "회의록.txt", "폴더", "폴더/메모.txt"]
    );
    for bytes in [KOREAN, KOREAN_TREE] {
        let s = Scratch::new();
        let events = extract(&s, bytes, None, Selection::All, None, Limits::default());
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        assert_eq!(fs::read(s.0.join("output/회의록.txt")).unwrap(), b"hi\n");
        assert_eq!(fs::read(s.0.join("output/README.md")).unwrap(), b"x\n");
    }
    let s = Scratch::new();
    let events = extract(
        &s,
        KOREAN_TREE,
        None,
        Selection::All,
        None,
        Limits::default(),
    );
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(fs::read(s.0.join("output/폴더/메모.txt")).unwrap(), NOTE);
}

#[test]
fn infozip_multi_entry_zipcrypto_trees_list_and_extract() {
    for (bytes, file) in [
        (ZIPCRYPTO_ASCII_TREE, "docs/notes.txt"),
        (ZIPCRYPTO_KOREAN_TREE, "폴더/메모.txt"),
    ] {
        let names = names_agree(bytes).unwrap();
        assert!(names.iter().any(|n| n == file), "{names:?}");
        let s = Scratch::new();
        let events = interactive(encrypted_start(&s, bytes), |id, _, purpose| {
            assert_eq!(purpose, PasswordPurpose::Decrypt);
            password_reply(id, TEST_PASSWORD)
        });
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        assert_eq!(fs::read(s.0.join("output").join(file)).unwrap(), NOTE);
        assert_eq!(fs::read(s.0.join("output/README.md")).unwrap(), b"x\n");
    }
}

#[test]
fn unicode_path_extra_is_used_only_when_its_crc_matches() {
    // CP949 "회의록.txt": UTF-8이 아니므로 확장 필드 없이는 지원하지 않음.
    let cp949 = b"\xc8\xb8\xc0\xc7\xb7\xcf.txt";
    let extra = unicode_path(cp949, "회의록.txt");
    assert_eq!(
        names_agree(&single_entry_zip(cp949, 0, &extra, b"hi\n")).unwrap(),
        ["회의록.txt"]
    );
    let s = Scratch::new();
    let bytes = single_entry_zip(cp949, 0, &extra, b"hi\n");
    let events = extract(&s, &bytes, None, Selection::All, None, Limits::default());
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(fs::read(s.0.join("output/회의록.txt")).unwrap(), b"hi\n");

    // CRC가 다른 확장 필드는 이름이 바뀐 뒤 남은 낡은 값이므로 무시함.
    let stale = unicode_path(b"old.txt", "회의록.txt");
    assert_eq!(
        names_agree(&single_entry_zip(b"new.txt", 0, &stale, b"hi\n")).unwrap(),
        ["new.txt"]
    );
    let stale = unicode_path(b"other", "회의록.txt");
    assert_eq!(
        names_agree(&single_entry_zip(cp949, 0, &stale, b"hi\n")),
        Err(ErrorCode::UnsupportedFeature)
    );
    // UTF-8 플래그가 있으면 이름 자체가 UTF-8이므로 확장 필드를 보지 않음.
    let ignored = unicode_path("a.txt".as_bytes(), "b.txt");
    assert_eq!(
        names_agree(&single_entry_zip(b"a.txt", 0x0800, &ignored, b"hi\n")).unwrap(),
        ["a.txt"]
    );
}

#[test]
fn legacy_and_hostile_names_stay_rejected() {
    let cp949 = b"\xc8\xb8\xc0\xc7\xb7\xcf.txt";
    // 코드 페이지를 추측하지 않음.
    assert_eq!(
        names_agree(&single_entry_zip(cp949, 0, &[], b"hi\n")),
        Err(ErrorCode::UnsupportedFeature)
    );
    // UTF-8 플래그와 맞지 않는 이름은 손상으로 봄.
    assert_eq!(
        names_agree(&single_entry_zip(cp949, 0x0800, &[], b"hi\n")),
        Err(ErrorCode::Integrity)
    );
    // 확장 필드로 들어온 이름도 같은 경로 검사를 거침.
    for hostile in ["../escape.txt", "/absolute.txt", "C:drive.txt", "CON.txt"] {
        let extra = unicode_path(b"safe.txt", hostile);
        assert_eq!(
            names_agree(&single_entry_zip(b"safe.txt", 0, &extra, b"hi\n")),
            Err(ErrorCode::UnsafePath),
            "{hostile}"
        );
    }
    for hostile in ["../회의록.txt", "폴더/../../x"] {
        assert_eq!(
            names_agree(&single_entry_zip(hostile.as_bytes(), 0, &[], b"hi\n")),
            Err(ErrorCode::UnsafePath),
            "{hostile}"
        );
    }
    // 확장 필드 이름이 UTF-8이 아니거나 형식이 다르면 손상으로 봄.
    let mut broken = unicode_path(b"safe.txt", "x");
    let len = broken.len();
    broken[len - 1] = 0xff;
    assert_eq!(
        names_agree(&single_entry_zip(b"safe.txt", 0, &broken, b"hi\n")),
        Err(ErrorCode::Integrity)
    );
    let mut short = 0x7075u16.to_le_bytes().to_vec();
    short.extend([4, 0, 1, 0, 0, 0]);
    assert_eq!(
        names_agree(&single_entry_zip(b"safe.txt", 0, &short, b"hi\n")),
        Err(ErrorCode::Integrity)
    );
    // APPNOTE 4.6.9에 따라 모르는 버전의 확장 필드는 무시함.
    let mut version = unicode_path(b"safe.txt", "x");
    version[4] = 2;
    assert_eq!(
        names_agree(&single_entry_zip(b"safe.txt", 0, &version, b"hi\n")).unwrap(),
        ["safe.txt"]
    );
}
