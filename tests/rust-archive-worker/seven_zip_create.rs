use super::*;

fn request(windows: bool) -> Action {
    let (source, output_file) = if windows {
        (
            NativePath::Windows("C:\\s".encode_utf16().collect()),
            NativePath::Windows("C:\\o.7z".encode_utf16().collect()),
        )
    } else {
        (
            NativePath::Unix(b"/s".to_vec()),
            NativePath::Unix(b"/o.7z".to_vec()),
        )
    };
    Action::CreateSevenZip {
        sources: vec![Source {
            path: source,
            archive_name: "한글".to_owned(),
        }],
        output_file,
        method: SevenZipMethod::Lzma2,
        solid: false,
        encryption: Encryption::None,
    }
}

fn start(action: Action) -> Start {
    Start {
        action,
        limits: Limits::default(),
    }
}

fn guarded(start: Start) -> GuardedStart {
    GuardedStart {
        ceiling: Limits {
            input: start.limits.input,
            output: 64 * 1024 * 1024 * 1024,
            entries: 200_000,
        },
        start,
        interactive: false,
    }
}

fn encode(message: &Message) -> Vec<u8> {
    let mut bytes = Vec::new();
    Writer::new(&mut bytes, 17).send(message).unwrap();
    bytes
}

fn decode(bytes: &[u8]) -> Message {
    let mut reader = Reader::new(bytes);
    let (job, sequence, message) = reader.read().unwrap().unwrap();
    assert_eq!((job, sequence), (17, 0));
    assert!(reader.read().unwrap().is_none());
    message
}

fn raw(kind: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = b"ZAW1\x01\x00".to_vec();
    bytes.extend(kind.to_le_bytes());
    bytes.extend((body.len() as u32).to_le_bytes());
    bytes.extend([0; 4]);
    bytes.extend(17u64.to_le_bytes());
    bytes.extend(0u64.to_le_bytes());
    bytes.extend(body);
    bytes
}

fn reject_encode(message: Message) {
    let mut bytes = Vec::new();
    assert_eq!(
        Writer::new(&mut bytes, 17)
            .send(&message)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidData
    );
    assert!(bytes.is_empty());
}

fn reject_decode(bytes: &[u8]) {
    assert_eq!(
        Reader::new(bytes).read().unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn request_preserves_agreed_field_order_and_native_encodings() {
    for windows in [false, true] {
        let start = start(request(windows));
        assert_eq!(start.action.kind(), ActionKind::CreateSevenZip);
        let mut expected = vec![4, 1, 0, 0, 0];
        if windows {
            expected.extend(b"\x02\x04\x00\x00\x00C\x00:\x00\\\x00s\x00");
        } else {
            expected.extend(b"\x01\x02\x00\x00\x00/s");
        }
        expected.extend(b"\x06\x00\x00\x00\xed\x95\x9c\xea\xb8\x80");
        if windows {
            expected.extend(b"\x02\x07\x00\x00\x00C\x00:\x00\\\x00o\x00.\x007\x00z\x00");
        } else {
            expected.extend(b"\x01\x05\x00\x00\x00/o.7z");
        }
        expected.extend([0, 0, 0]);
        expected.extend(u64::MAX.to_le_bytes());
        expected.extend((16u64 * 1024 * 1024 * 1024).to_le_bytes());
        expected.extend(10_000u32.to_le_bytes());
        let bytes = encode(&Message::Start(start.clone()));
        assert_eq!(bytes, raw(1, &expected));
        match decode(&bytes) {
            Message::Start(actual) => assert_eq!(actual, start),
            other => panic!("{other:?}"),
        }
        let guarded = guarded(start);
        assert!(guarded.valid());
        expected.extend(u64::MAX.to_le_bytes());
        expected.extend((64u64 * 1024 * 1024 * 1024).to_le_bytes());
        expected.extend(200_000u32.to_le_bytes());
        expected.push(0);
        let bytes = encode(&Message::StartGuarded(guarded.clone()));
        assert_eq!(bytes, raw(6, &expected));
        match decode(&bytes) {
            Message::StartGuarded(actual) => assert_eq!(actual, guarded),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn unsupported_options_remain_structurally_valid_without_silent_conversion() {
    for solid in [false, true] {
        for encryption in [Encryption::None, Encryption::Aes256] {
            let mut action = request(false);
            if let Action::CreateSevenZip {
                solid: actual_solid,
                encryption: actual_encryption,
                ..
            } = &mut action
            {
                *actual_solid = solid;
                *actual_encryption = encryption;
            }
            let start = start(action);
            match decode(&encode(&Message::Start(start.clone()))) {
                Message::Start(actual) => assert_eq!(actual, start),
                other => panic!("{other:?}"),
            }
        }
    }
}

#[test]
fn creation_never_accepts_interactive_conflict_requests() {
    let start = start(request(false));
    reject_encode(Message::StartInteractive(start.clone()));
    let bytes = encode(&Message::Start(start.clone()));
    reject_decode(&raw(4, &bytes[32..]));
    let mut guarded = guarded(start);
    guarded.interactive = true;
    assert!(!guarded.valid());
    reject_encode(Message::StartGuarded(guarded));
    let mut bytes = encode(&Message::StartGuarded(self::guarded(self::start(request(
        false,
    )))));
    *bytes.last_mut().unwrap() = 1;
    reject_decode(&bytes);
}

#[test]
fn truncated_unknown_and_trailing_creation_fields_are_rejected() {
    for windows in [false, true] {
        let bytes = encode(&Message::Start(start(request(windows))));
        let body = &bytes[32..];
        for length in 0..body.len() {
            reject_decode(&raw(1, &body[..length]));
        }
        let mut trailing = body.to_vec();
        trailing.push(0);
        reject_decode(&raw(1, &trailing));
        let method = body.len() - 20 - 3;
        for (at, value) in [(0, 5), (method, 1), (method + 1, 2), (method + 2, 2)] {
            let mut body = body.to_vec();
            body[at] = value;
            reject_decode(&raw(1, &body));
        }
    }
}

#[test]
fn guarded_approval_never_expands_source_vector_string_or_path_wire_limits() {
    let mut action = request(false);
    if let Action::CreateSevenZip { sources, .. } = &mut action {
        sources[0].archive_name = "a".repeat(32_768);
    }
    let guarded = guarded(start(action.clone()));
    assert!(matches!(
        decode(&encode(&Message::StartGuarded(guarded))),
        Message::StartGuarded(_)
    ));
    if let Action::CreateSevenZip { sources, .. } = &mut action {
        sources[0].archive_name.push('a');
    }
    reject_encode(Message::StartGuarded(self::guarded(start(action))));
    for windows in [false, true] {
        let mut action = request(windows);
        if let Action::CreateSevenZip { sources, .. } = &mut action {
            sources[0].path = if windows {
                NativePath::Windows(vec![b'a' as u16; 32_769])
            } else {
                NativePath::Unix(vec![b'a'; 32_769])
            };
        }
        reject_encode(Message::StartGuarded(self::guarded(start(action))));
    }
    let mut action = request(false);
    if let Action::CreateSevenZip { sources, .. } = &mut action {
        *sources = vec![sources[0].clone(); 32_769];
    }
    reject_encode(Message::StartGuarded(self::guarded(start(action))));
    let bytes = encode(&Message::StartGuarded(self::guarded(start(request(false)))));
    for at in [1, 6, 12] {
        let mut body = bytes[32..].to_vec();
        body[at..at + 4].copy_from_slice(&32_769u32.to_le_bytes());
        reject_decode(&raw(6, &body));
    }
}

#[test]
fn action_kind_events_keep_existing_tags_and_append_creation_tag_four() {
    for (tag, action) in [
        ActionKind::List,
        ActionKind::Extract,
        ActionKind::CreateZip,
        ActionKind::Preview,
        ActionKind::CreateSevenZip,
    ]
    .into_iter()
    .enumerate()
    {
        let encryption = action != ActionKind::CreateSevenZip;
        let bytes = encode(&Message::Accepted { action, encryption });
        assert_eq!(bytes, raw(100, &[tag as u8, u8::from(encryption)]));
        assert!(
            matches!(decode(&bytes), Message::Accepted { action: actual, encryption: value } if actual == action && value == encryption)
        );
        let scope = ResourceScope {
            entries: true,
            ..ResourceScope::default()
        };
        let bytes = encode(&Message::ResourceRequired {
            challenge_id: 23,
            action,
            scope,
            usage: ResourceUsage::default(),
            soft_limits: Limits::default(),
            ceiling: guarded(start(request(false))).ceiling,
        });
        assert_eq!(bytes[32 + 8], tag as u8);
        assert!(
            matches!(decode(&bytes), Message::ResourceRequired { action: actual, scope: value, .. } if actual == action && value == scope)
        );
        let mut body = bytes[32..].to_vec();
        body[8] = 6;
        reject_decode(&raw(110, &body));
    }
    reject_decode(&raw(100, &[6, 0]));
}

#[test]
fn zip_methods_and_existing_request_bytes_stay_independent() {
    for (tag, method) in [(0, Method::Store), (1, Method::Deflate)] {
        let action = Action::CreateZip {
            sources: Vec::new(),
            output_file: NativePath::Unix(b"/a.zip".to_vec()),
            method,
            encryption: Encryption::Aes256,
        };
        let start = start(action);
        let mut body = b"\x02\x00\x00\x00\x00\x01\x06\x00\x00\x00/a.zip".to_vec();
        body.extend([tag, 1]);
        body.extend(u64::MAX.to_le_bytes());
        body.extend((16u64 * 1024 * 1024 * 1024).to_le_bytes());
        body.extend(10_000u32.to_le_bytes());
        assert_eq!(encode(&Message::Start(start.clone())), raw(1, &body));
        assert!(matches!(decode(&raw(1, &body)), Message::Start(actual) if actual == start));
        body[16] = 2;
        reject_decode(&raw(1, &body));
    }
}

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let root = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/work/tmp")
            });
        fs::create_dir_all(&root).unwrap();
        let path = root.canonicalize().unwrap().join(format!(
            "zipelo-seven-create-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, message: Message) -> Vec<Message> {
        let output = self.0.join("saved.7z");
        fs::write(&output, b"existing user data").unwrap();
        let bytes = encode(&message);
        let mut child = Command::new(env!("CARGO_BIN_EXE_zipelo-archive-worker"))
            .env("TMPDIR", self.0.join("unavailable-temp"))
            .env("TMP", self.0.join("unavailable-temp"))
            .env("TEMP", self.0.join("unavailable-temp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&bytes).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        assert!(output.stderr.is_empty());
        let mut reader = Reader::new(output.stdout.as_slice());
        let mut events = Vec::new();
        while let Some((job, _, message)) = reader.read().unwrap() {
            assert_eq!(job, 17);
            events.push(message);
        }
        assert_eq!(
            fs::read(self.0.join("saved.7z")).unwrap(),
            b"existing user data"
        );
        assert_eq!(fs::read_dir(&self.0).unwrap().count(), 1);
        events
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("7z 생성 시험 임시 폴더 정리에 실패함");
    }
}

#[test]
fn unsupported_options_fail_before_questions_or_file_access() {
    let temporary = Temporary::new();
    for solid in [false, true] {
        for encryption in [Encryption::None, Encryption::Aes256] {
            if !solid && encryption == Encryption::None {
                continue;
            }
            for kind in [1, 6, 7] {
                let action = Action::CreateSevenZip {
                    sources: vec![Source {
                        path: NativePath::from_path(&temporary.0.join("missing/source")),
                        archive_name: "source".to_owned(),
                    }],
                    output_file: NativePath::from_path(&temporary.0.join("saved.7z")),
                    method: SevenZipMethod::Lzma2,
                    solid,
                    encryption,
                };
                let start = start(action);
                let message = if kind == 1 {
                    Message::Start(start)
                } else {
                    let mut guarded = guarded(start);
                    if kind == 7 {
                        guarded.ceiling = guarded.start.limits;
                    }
                    Message::StartGuarded(guarded)
                };
                let events = temporary.run(message);
                assert_eq!(events.len(), 2, "{events:?}");
                assert!(matches!(
                    events[0],
                    Message::Accepted {
                        action: ActionKind::CreateSevenZip,
                        encryption: false
                    }
                ));
                let terminal = terminal(&events);
                assert_eq!(terminal.outcome, Outcome::Failed);
                assert_eq!(terminal.error, ErrorCode::UnsupportedFeature);
                assert_eq!(terminal.publication, Publication::None);
                assert_eq!(terminal.created_count, 0);
                assert_eq!(terminal.retained_partial_count, 0);
                assert_eq!(terminal.cleanup, Cleanup::Complete);
                assert_eq!(terminal.durability, Durability::NotApplicable);
            }
        }
    }
}

#[test]
fn existing_runtime_encryption_capability_is_preserved() {
    let temporary = Temporary::new();
    let events = temporary.run(Message::Start(start(Action::List {
        archive: NativePath::from_path(&temporary.0.join("missing.zip")),
    })));
    assert!(matches!(
        events[0],
        Message::Accepted {
            action: ActionKind::List,
            encryption: true
        }
    ));
    assert_eq!(terminal(&events).outcome, Outcome::Failed);
}
