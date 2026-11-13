use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};
use zipelo_archive_worker::{
    archive,
    protocol::*,
    wire::{Reader, Writer},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests/rust-archive-worker/runs")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let builder = fs::DirBuilder::new();
        #[cfg(unix)]
        let mut builder = builder;
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> NativePath {
        NativePath::from_path(&self.0.join(name))
    }
    fn worker(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zipelo-archive-worker"));
        // 공유 임시 폴더의 할당량과 다른 시험의 사용량에 의존하지 않음.
        command
            .env("TMPDIR", &self.0)
            .env("TMP", &self.0)
            .env("TEMP", &self.0);
        command
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn execute(start: Start) -> Vec<Message> {
    let mut data = Vec::new();
    Writer::new(&mut data, 17)
        .send(&Message::Start(start))
        .unwrap();
    let temporary = Scratch::new();
    let mut command = temporary.worker();
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        #[repr(C)]
        struct Limit {
            soft: usize,
            hard: usize,
        }
        unsafe extern "C" {
            fn setrlimit(resource: i32, limit: *const Limit) -> i32;
        }
        // 자식 프로세스의 주소 공간만 512 MiB로 제한함.
        unsafe {
            command.pre_exec(|| {
                let limit = Limit {
                    soft: 512 * 1024 * 1024,
                    hard: 512 * 1024 * 1024,
                };
                if setrlimit(9, &limit) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(&data).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let mut reader = Reader::new(output.stdout.as_slice());
    let mut events = Vec::new();
    while let Some((job, _, event)) = reader.read().unwrap() {
        assert_eq!(job, 17);
        events.push(event);
    }
    assert!(matches!(events.last(), Some(Message::Terminal(_))));
    events
}
fn terminal(events: &[Message]) -> &Terminal {
    match events.last().unwrap() {
        Message::Terminal(t) => t,
        _ => panic!("missing terminal"),
    }
}
fn extract(
    s: &Scratch,
    bytes: &[u8],
    gzip: Option<&str>,
    selection: Selection,
    list_id: Option<ListId>,
    limits: Limits,
) -> Vec<Message> {
    fs::write(s.0.join("input"), bytes).unwrap();
    if !s.0.join("output").exists() {
        let d = fs::DirBuilder::new();
        #[cfg(unix)]
        let mut d = d;
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            d.mode(0o700);
        }
        d.create(s.0.join("output")).unwrap();
    }
    execute(Start {
        action: Action::Extract {
            archive: s.path("input"),
            destination: s.path("output"),
            selection,
            list_id,
            gzip_output_name: gzip.map(str::to_owned),
        },
        limits,
    })
}
const PAYLOAD: &[u8] = b"Zipelo worker interoperability\n";
fn basic(bytes: &[u8], gzip: Option<&str>, output: &str) {
    let s = Scratch::new();
    let events = extract(&s, bytes, gzip, Selection::All, None, Limits::default());
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(terminal(&events).publication, Publication::Complete);
    assert_eq!(fs::read(s.0.join("output").join(output)).unwrap(), PAYLOAD);
}
#[test]
fn stored_zip() {
    basic(
        include_bytes!("fixtures/store.zip"),
        None,
        "folder/hello.txt",
    );
}
#[test]
fn deflated_zip() {
    basic(
        include_bytes!("fixtures/deflate.zip"),
        None,
        "folder/hello.txt",
    );
}
#[test]
fn tar() {
    basic(
        include_bytes!("fixtures/sample.tar"),
        None,
        "folder/hello.txt",
    );
}
#[test]
fn tar_gzip() {
    basic(
        include_bytes!("fixtures/sample.tar.gz"),
        None,
        "folder/hello.txt",
    );
}
#[test]
fn gzip_explicit_name_ignores_fname() {
    basic(
        include_bytes!("fixtures/sample.gz"),
        Some("chosen.txt"),
        "chosen.txt",
    );
}
#[test]
fn gzip_concatenated_members() {
    let s = Scratch::new();
    let events = extract(
        &s,
        include_bytes!("fixtures/concat.gz"),
        Some("combined"),
        Selection::All,
        None,
        Limits::default(),
    );
    assert_eq!(terminal(&events).outcome, Outcome::Completed);
    assert_eq!(fs::read(s.0.join("output/combined")).unwrap(), b"onetwo");
}
#[test]
fn gzip_requires_leaf() {
    for name in [None, Some("../escape"), Some("a/b")] {
        let s = Scratch::new();
        let e = extract(
            &s,
            include_bytes!("fixtures/sample.gz"),
            name,
            Selection::All,
            None,
            Limits::default(),
        );
        assert_eq!(terminal(&e).error, ErrorCode::InvalidRequest);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
    }
}
#[test]
fn archive_rejects_gzip_name() {
    let s = Scratch::new();
    let e = extract(
        &s,
        include_bytes!("fixtures/store.zip"),
        Some("a"),
        Selection::All,
        None,
        Limits::default(),
    );
    assert_eq!(terminal(&e).error, ErrorCode::InvalidRequest);
}
#[test]
fn no_clobber_and_preflight_no_partial_outputs() {
    let s = Scratch::new();
    fs::create_dir(s.0.join("output")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(s.0.join("output"), fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::create_dir(s.0.join("output/folder")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(s.0.join("output/folder"), fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(s.0.join("output/folder/hello.txt"), b"preserve").unwrap();
    let e = extract(
        &s,
        include_bytes!("fixtures/store.zip"),
        None,
        Selection::All,
        None,
        Limits::default(),
    );
    assert_eq!(terminal(&e).error, ErrorCode::Conflict);
    assert_eq!(
        fs::read(s.0.join("output/folder/hello.txt")).unwrap(),
        b"preserve"
    );
    assert!(!s.0.join("output/empty").exists());
}
#[test]
fn unsafe_archives_reject_without_writes() {
    for bytes in [
        include_bytes!("fixtures/traversal.zip").as_slice(),
        include_bytes!("fixtures/absolute.zip"),
        include_bytes!("fixtures/drive.zip"),
        include_bytes!("fixtures/reserved.zip"),
        include_bytes!("fixtures/link.zip"),
        include_bytes!("fixtures/collision.zip"),
        include_bytes!("fixtures/parent-collision.zip"),
    ] {
        let s = Scratch::new();
        let e = extract(&s, bytes, None, Selection::All, None, Limits::default());
        assert_eq!(terminal(&e).outcome, Outcome::Failed, "{e:?}");
        assert_eq!(terminal(&e).created_count, 0);
        assert_eq!(terminal(&e).publication, Publication::None);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
    }
}
#[test]
fn bomb_rejects_before_writes() {
    let s = Scratch::new();
    let e = extract(
        &s,
        include_bytes!("fixtures/bomb.zip"),
        None,
        Selection::All,
        None,
        Limits {
            output: 1024,
            ..Limits::default()
        },
    );
    assert_eq!(terminal(&e).error, ErrorCode::Limit);
    assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
}
#[test]
fn corrupt_crc_and_truncation_reject_without_writes() {
    let mut zip = include_bytes!("fixtures/store.zip").to_vec();
    let at = zip
        .windows(PAYLOAD.len())
        .position(|b| b == PAYLOAD)
        .unwrap();
    zip[at] ^= 1;
    for bytes in [zip.as_slice(), &include_bytes!("fixtures/store.zip")[..40]] {
        let s = Scratch::new();
        let e = extract(&s, bytes, None, Selection::All, None, Limits::default());
        assert_eq!(terminal(&e).outcome, Outcome::Failed);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
    }
}
fn listing(s: &Scratch) -> ListId {
    let e = execute(Start {
        action: Action::List {
            archive: s.path("input"),
        },
        limits: Limits::default(),
    });
    assert_eq!(terminal(&e).outcome, Outcome::Completed);
    e.into_iter()
        .find_map(|m| {
            if let Message::ListEnd { list_id, .. } = m {
                Some(list_id)
            } else {
                None
            }
        })
        .unwrap()
}
#[test]
fn list_and_selected_extract() {
    let s = Scratch::new();
    let bytes = include_bytes!("fixtures/store.zip");
    fs::write(s.0.join("input"), bytes).unwrap();
    let id = listing(&s);
    let e = extract(
        &s,
        bytes,
        None,
        Selection::EntryIds(vec![0]),
        Some(id),
        Limits::default(),
    );
    assert_eq!(terminal(&e).outcome, Outcome::Completed);
    assert!(!s.0.join("output/empty").exists());
    assert_eq!(
        fs::read(s.0.join("output/folder/hello.txt")).unwrap(),
        PAYLOAD
    );
}
#[test]
fn stale_list_content_rejected() {
    let s = Scratch::new();
    fs::write(s.0.join("input"), include_bytes!("fixtures/store.zip")).unwrap();
    let mut id = listing(&s);
    id.archive_sha256[0] ^= 1;
    let e = extract(
        &s,
        include_bytes!("fixtures/store.zip"),
        None,
        Selection::EntryIds(vec![0]),
        Some(id),
        Limits::default(),
    );
    assert_eq!(terminal(&e).error, ErrorCode::SourceChanged);
    assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
}
#[test]
fn missing_duplicate_and_out_of_range_selection() {
    for ids in [vec![], vec![0, 0], vec![999]] {
        let s = Scratch::new();
        fs::write(s.0.join("input"), include_bytes!("fixtures/store.zip")).unwrap();
        let id = listing(&s);
        let e = extract(
            &s,
            include_bytes!("fixtures/store.zip"),
            None,
            Selection::EntryIds(ids),
            Some(id),
            Limits::default(),
        );
        assert_eq!(terminal(&e).error, ErrorCode::InvalidRequest);
    }
    let s = Scratch::new();
    let e = extract(
        &s,
        include_bytes!("fixtures/store.zip"),
        None,
        Selection::EntryIds(vec![0]),
        None,
        Limits::default(),
    );
    assert_eq!(terminal(&e).error, ErrorCode::InvalidRequest);
}
#[test]
fn create_store_and_deflate_zip() {
    for method in [Method::Store, Method::Deflate] {
        let s = Scratch::new();
        fs::write(s.0.join("source"), PAYLOAD).unwrap();
        let e = execute(Start {
            action: Action::CreateZip {
                sources: vec![Source {
                    path: s.path("source"),
                    archive_name: "dir/hello.txt".into(),
                }],
                output_file: s.path("result.zip"),
                method,
                encryption: Encryption::None,
            },
            limits: Limits::default(),
        });
        assert_eq!(terminal(&e).outcome, Outcome::Completed, "{e:?}");
        let bytes = fs::read(s.0.join("result.zip")).unwrap();
        let parsed = archive::parse(&bytes, Limits::default(), true, &mut || Ok(())).unwrap();
        assert_eq!(parsed.members[0].data, PAYLOAD);
    }
}
#[test]
fn encryption_password_eof_is_protocol_failure() {
    let s = Scratch::new();
    let e = execute(Start {
        action: Action::CreateZip {
            sources: vec![],
            output_file: s.path("result.zip"),
            method: Method::Store,
            encryption: Encryption::Aes256,
        },
        limits: Limits::default(),
    });
    assert_eq!(terminal(&e).error, ErrorCode::Protocol);
    assert!(!s.0.join("result.zip").exists());
}
#[test]
fn name_policy() {
    for name in [
        "../a", "/a", "a\\b", "C:a", "a/../b", "a.", "a ", "COM1.txt", "a//b", "a/./b",
    ] {
        assert!(archive::valid_name(name, false).is_err(), "{name}");
    }
    assert!(archive::valid_name("folder/한글.txt", false).is_ok());
}
#[test]
fn frame_round_trip_and_secret_redaction() {
    let mut b = Vec::new();
    Writer::new(&mut b, 42).send(&Message::Cancel).unwrap();
    let mut reader = Reader::new(b.as_slice());
    assert!(matches!(
        reader.read().unwrap(),
        Some((42, 0, Message::Cancel))
    ));
    assert!(reader.read().unwrap().is_none());
    assert!(
        !format!(
            "{:?}",
            Message::Password {
                challenge_id: 1,
                secret: Secret(b"private-password".to_vec())
            }
        )
        .contains("private-password")
    );
}
#[test]
fn invalid_headers_are_rejected() {
    let mut b = Vec::new();
    Writer::new(&mut b, 42).send(&Message::Cancel).unwrap();
    for (offset, value) in [(0, 0), (4, 2), (12, 1), (16, 0), (24, 1)] {
        let mut bad = b.clone();
        bad[offset] = value;
        assert!(Reader::new(bad.as_slice()).read().is_err());
    }
    let mut big = b.clone();
    big[8..12].copy_from_slice(&1_048_577u32.to_le_bytes());
    assert!(Reader::new(big.as_slice()).read().is_err());
    assert!(Reader::new(&b[..16]).read().is_err());
}
#[test]
fn terminal_is_final_frame() {
    let t = Terminal {
        outcome: Outcome::Failed,
        error: ErrorCode::Io,
        publication: Publication::Uncertain,
        created_count: 0,
        retained_partial_count: 0,
        durability: Durability::Unconfirmed,
        cleanup: Cleanup::Unconfirmed,
        detail: "uncertain".into(),
    };
    let mut b = Vec::new();
    let mut w = Writer::new(&mut b, 1);
    w.send(&Message::Terminal(t)).unwrap();
    assert!(w.send(&Message::Cancel).is_err());
}
#[cfg(unix)]
#[test]
fn symlink_parent_escape_is_rejected() {
    use std::os::unix::fs::symlink;
    let s = Scratch::new();
    let d = fs::DirBuilder::new();
    #[cfg(unix)]
    let mut d = d;
    use std::os::unix::fs::DirBuilderExt;
    d.mode(0o700);
    d.create(s.0.join("output")).unwrap();
    d.create(s.0.join("outside")).unwrap();
    symlink(s.0.join("outside"), s.0.join("output/folder")).unwrap();
    let e = extract(
        &s,
        include_bytes!("fixtures/store.zip"),
        None,
        Selection::All,
        None,
        Limits::default(),
    );
    assert_eq!(terminal(&e).error, ErrorCode::UnsafePath);
    assert_eq!(fs::read_dir(s.0.join("outside")).unwrap().count(), 0);
}

#[test]
fn changed_source_is_reopen_error_before_decode() {
    let s = Scratch::new();
    let bytes = include_bytes!("fixtures/store.zip");
    fs::write(s.0.join("input"), bytes).unwrap();
    let id = listing(&s);
    let mut changed = bytes.to_vec();
    changed[40] ^= 1;
    let e = extract(
        &s,
        &changed,
        None,
        Selection::EntryIds(vec![0]),
        Some(id),
        Limits::default(),
    );
    assert_eq!(terminal(&e).error, ErrorCode::SourceChanged);
    assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
}
#[test]
fn cancel_ack_and_terminal_are_distinct() {
    let s = Scratch::new();
    fs::write(s.0.join("source"), vec![0u8; 16 * 1024 * 1024]).unwrap();
    let start = Start {
        action: Action::CreateZip {
            sources: vec![Source {
                path: s.path("source"),
                archive_name: "data".into(),
            }],
            output_file: s.path("result.zip"),
            method: Method::Deflate,
            encryption: Encryption::None,
        },
        limits: Limits::default(),
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_zipelo-archive-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut writer = Writer::new(child.stdin.take().unwrap(), 19);
    let mut reader = Reader::new(child.stdout.take().unwrap());
    writer.send(&Message::Start(start)).unwrap();
    assert!(matches!(
        reader.read().unwrap(),
        Some((19, 0, Message::Accepted { .. }))
    ));
    writer.send(&Message::Cancel).unwrap();
    writer.send(&Message::Cancel).unwrap();
    let mut events = Vec::new();
    while let Some((_, _, m)) = reader.read().unwrap() {
        events.push(m);
    }
    assert!(child.wait().unwrap().success());
    let terminal = terminal(&events);
    assert!(matches!(
        terminal.outcome,
        Outcome::Cancelled | Outcome::Completed
    ));
    for m in &events {
        if let Message::CancelAck {
            request_sequence,
            status,
        } = m
        {
            assert!([1, 2].contains(request_sequence));
            assert!(matches!(
                status,
                CancelStatus::Requested
                    | CancelStatus::AlreadyRequested
                    | CancelStatus::TooLate
                    | CancelStatus::Finished
            ));
        }
    }
    assert!(
        events
            .iter()
            .any(|m| matches!(m, Message::CancelAck { .. })),
        "cancel must be consumed during bounded source read: {events:?}"
    );
}

#[test]
fn encrypted_zip_password_eof_preserves_outputs() {
    let mut bytes = include_bytes!("fixtures/store.zip").to_vec();
    bytes[6] |= 1;
    let central = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
    bytes[central + 8] |= 1;
    let s = Scratch::new();
    let e = extract(&s, &bytes, None, Selection::All, None, Limits::default());
    assert_eq!(terminal(&e).error, ErrorCode::Protocol);
    assert_eq!(terminal(&e).publication, Publication::None);
    assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
}
#[test]
fn uncertain_report_survives_codec() {
    let t = Terminal {
        outcome: Outcome::Failed,
        error: ErrorCode::Io,
        publication: Publication::Uncertain,
        created_count: 2,
        retained_partial_count: 1,
        durability: Durability::Failed,
        cleanup: Cleanup::Failed,
        detail: "publication uncertain".into(),
    };
    let mut bytes = Vec::new();
    Writer::new(&mut bytes, 88)
        .send(&Message::Terminal(t.clone()))
        .unwrap();
    let mut reader = Reader::new(bytes.as_slice());
    match reader.read().unwrap() {
        Some((88, 0, Message::Terminal(actual))) => assert_eq!(actual, t),
        other => panic!("{other:?}"),
    };
    assert!(reader.read().unwrap().is_none());
}

fn path_fixture_zip(names: &[String]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut central = Vec::new();
    for name in names {
        let offset = output.len() as u32;
        output.extend(b"PK\x03\x04");
        for value in [20u16, 0x0800, 0, 0, 33] {
            output.extend(value.to_le_bytes());
        }
        for value in [0u32, 0, 0] {
            output.extend(value.to_le_bytes());
        }
        output.extend((name.len() as u16).to_le_bytes());
        output.extend(0u16.to_le_bytes());
        output.extend(name.as_bytes());
        central.extend(b"PK\x01\x02");
        for value in [0x0314u16, 20, 0x0800, 0, 0, 33] {
            central.extend(value.to_le_bytes());
        }
        for value in [0u32, 0, 0] {
            central.extend(value.to_le_bytes());
        }
        central.extend((name.len() as u16).to_le_bytes());
        for value in [0u16, 0, 0, 0] {
            central.extend(value.to_le_bytes());
        }
        central.extend((0o100600u32 << 16).to_le_bytes());
        central.extend(offset.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let offset = output.len() as u32;
    output.extend(&central);
    output.extend(b"PK\x05\x06");
    for value in [0u16, 0, names.len() as u16, names.len() as u16] {
        output.extend(value.to_le_bytes());
    }
    output.extend((central.len() as u32).to_le_bytes());
    output.extend(offset.to_le_bytes());
    output.extend(0u16.to_le_bytes());
    output
}

fn assert_metadata_limit(names: &[String]) {
    let scratch = Scratch::new();
    let events = extract(
        &scratch,
        &path_fixture_zip(names),
        None,
        Selection::All,
        None,
        Limits::default(),
    );
    let result = terminal(&events);
    assert_eq!(result.outcome, Outcome::Failed, "{events:?}");
    assert_eq!(result.error, ErrorCode::Limit, "{events:?}");
    assert_eq!(result.publication, Publication::None);
    assert_eq!(result.created_count, 0);
    assert_eq!(fs::read_dir(scratch.0.join("output")).unwrap().count(), 0);
}

#[test]
fn deep_zip_returns_limit_without_abort_or_writes() {
    let names = (0..100)
        .map(|i| format!("{}file{i:03}", "a/".repeat(2000)))
        .collect::<Vec<_>>();
    assert_metadata_limit(&names);
}

#[test]
fn shallow_zip_parent_metadata_is_bounded() {
    let names = (0..500)
        .map(|i| {
            format!(
                "d{i:04}{}/{}file",
                "a".repeat(180),
                format!("{}/", "b".repeat(200)).repeat(7)
            )
        })
        .collect::<Vec<_>>();
    assert!(
        names
            .iter()
            .all(|n| n.split('/').count() <= archive::MAX_PATH_DEPTH)
    );
    assert!(
        archive::parse(
            &path_fixture_zip(&names),
            Limits::default(),
            false,
            &mut || Ok(())
        )
        .is_ok()
    );
    assert_metadata_limit(&names);
}

#[test]
fn aggregate_archive_name_metadata_is_bounded() {
    let names = (0..1100)
        .map(|i| {
            format!(
                "d{i:04}{}/{}file",
                "a".repeat(180),
                format!("{}/", "b".repeat(200)).repeat(9)
            )
        })
        .collect::<Vec<_>>();
    let bytes = path_fixture_zip(&names);
    assert!((bytes.len() as u64) < Limits::default().input);
    assert!(matches!(
        archive::parse(&bytes, Limits::default(), false, &mut || Ok(())),
        Err(ErrorCode::Limit)
    ));
    assert_metadata_limit(&names);
}

#[test]
fn path_depth_boundary_is_explicit() {
    let accepted = format!("{}file", "a/".repeat(archive::MAX_PATH_DEPTH - 1));
    let rejected = format!("{}file", "a/".repeat(archive::MAX_PATH_DEPTH));
    assert!(archive::valid_name(&accepted, false).is_ok());
    assert_eq!(archive::valid_name(&rejected, false), Err(ErrorCode::Limit));
}

fn zeros_gzip(length: usize) -> Vec<u8> {
    let mut gzip = vec![31, 139, 8, 0, 0, 0, 0, 0, 0, 255];
    let mut left = length;
    loop {
        let size = left.min(u16::MAX as usize) as u16;
        left -= size as usize;
        gzip.push(u8::from(left == 0));
        gzip.extend(size.to_le_bytes());
        gzip.extend((!size).to_le_bytes());
        gzip.resize(gzip.len() + size as usize, 0);
        if left == 0 {
            break;
        }
    }
    gzip.extend(zlib_rs::crc32::crc32(0, &vec![0; length]).to_le_bytes());
    gzip.extend((length as u32).to_le_bytes());
    gzip
}

#[test]
fn zero_payload_gzip_is_not_tar() {
    for length in [512, 1024, 1536] {
        let scratch = Scratch::new();
        let bytes = zeros_gzip(length);
        let parsed = archive::parse(&bytes, Limits::default(), true, &mut || Ok(())).unwrap();
        assert_eq!(parsed.format, Format::Gzip);
        let events = extract(
            &scratch,
            &bytes,
            Some("zeros.bin"),
            Selection::All,
            None,
            Limits::default(),
        );
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        assert_eq!(
            fs::read(scratch.0.join("output/zeros.bin")).unwrap(),
            vec![0; length]
        );
        fs::write(scratch.0.join("input"), &bytes).unwrap();
        let list_id = listing(&scratch);
        assert_eq!(list_id.format, Format::Gzip);
    }
}

const ENCRYPTED_PAYLOAD: &[u8] = b"Zipelo encrypted interoperability\nZipelo encrypted interoperability\nZipelo encrypted interoperability\nZipelo encrypted interoperability\nZipelo encrypted interoperability\nZipelo encrypted interoperability\nZipelo encrypted interoperability\nZipelo encrypted interoperability\n";
const TEST_PASSWORD: &[u8] = b"fixture-password";
const AES_ZIP: &[u8] = include_bytes!("fixtures/aes2-deflate.zip");
fn interactive(
    start: Start,
    mut respond: impl FnMut(u64, u32, PasswordPurpose) -> Vec<Message>,
) -> Vec<Message> {
    use std::{io::Read, sync::mpsc, time::Duration};
    let temporary = Scratch::new();
    let mut child = temporary
        .worker()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut writer = Some(Writer::new(child.stdin.take().unwrap(), 17));
    writer
        .as_mut()
        .unwrap()
        .send(&Message::Start(start))
        .unwrap();
    let output = child.stdout.take().unwrap();
    let mut errors = child.stderr.take().unwrap();
    let error_thread = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        errors.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let (sender, receiver) = mpsc::channel();
    let reader_thread = std::thread::spawn(move || {
        let mut reader = Reader::new(output);
        loop {
            let event = reader.read().map(|v| {
                v.map(|(job, _, message)| {
                    assert_eq!(job, 17);
                    message
                })
            });
            let done = !matches!(event, Ok(Some(_)));
            if sender.send(event).is_err() || done {
                break;
            }
        }
    });
    let mut events = Vec::new();
    loop {
        let event = match receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(event) => event.unwrap(),
            Err(error) => {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("worker timeout: {error}");
            }
        };
        let Some(event) = event else {
            break;
        };
        if let Message::PasswordRequired {
            challenge_id,
            attempt,
            purpose,
        } = &event
        {
            let commands = respond(*challenge_id, *attempt, *purpose);
            if commands.is_empty() {
                writer = None;
            } else {
                for command in commands {
                    writer.as_mut().unwrap().send(&command).unwrap();
                }
            }
        }
        if matches!(event, Message::Terminal(_)) {
            writer = None;
        }
        events.push(event);
    }
    assert!(child.wait().unwrap().success());
    reader_thread.join().unwrap();
    assert!(error_thread.join().unwrap().is_empty());
    assert!(matches!(events.last(), Some(Message::Terminal(_))));
    events
}
fn password_reply(id: u64, password: &[u8]) -> Vec<Message> {
    vec![Message::Password {
        challenge_id: id,
        secret: Secret(password.to_vec()),
    }]
}
fn encrypted_start(s: &Scratch, bytes: &[u8]) -> Start {
    fs::write(s.0.join("input"), bytes).unwrap();
    fs::create_dir(s.0.join("output")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(s.0.join("output"), fs::Permissions::from_mode(0o700)).unwrap();
    }
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
fn no_encrypted_output(s: &Scratch, events: &[Message], error: ErrorCode) {
    assert_eq!(terminal(events).error, error, "{events:?}");
    assert_eq!(terminal(events).publication, Publication::None);
    assert_eq!(terminal(events).created_count, 0);
    assert!(!events.iter().any(|e| matches!(e, Message::Output { .. })));
    assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
}
#[test]
fn independent_encrypted_fixtures_list_and_extract() {
    for bytes in [
        include_bytes!("fixtures/aes1-store.zip").as_slice(),
        include_bytes!("fixtures/aes1-deflate.zip").as_slice(),
        include_bytes!("fixtures/aes2-store.zip").as_slice(),
        AES_ZIP,
        include_bytes!("fixtures/zipcrypto-store.zip").as_slice(),
        include_bytes!("fixtures/zipcrypto-deflate.zip").as_slice(),
    ] {
        let s = Scratch::new();
        let start = encrypted_start(&s, bytes);
        let listing = execute(Start {
            action: Action::List {
                archive: s.path("input"),
            },
            limits: Limits::default(),
        });
        assert_eq!(
            terminal(&listing).outcome,
            Outcome::Completed,
            "{listing:?}"
        );
        assert!(listing.iter().any(|e| matches!(
            e,
            Message::Entry(Entry {
                encrypted: true,
                ..
            })
        )));
        assert!(
            !listing
                .iter()
                .any(|e| matches!(e, Message::PasswordRequired { .. }))
        );
        let events = interactive(start, |id, attempt, purpose| {
            assert_eq!(purpose, PasswordPurpose::Decrypt);
            assert_eq!(attempt, 1);
            password_reply(id, TEST_PASSWORD)
        });
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        assert_eq!(
            fs::read(s.0.join("output/payload.bin")).unwrap(),
            ENCRYPTED_PAYLOAD
        );
    }
}
#[test]
fn aes_creation_uses_password_flow_and_independent_verification() {
    for method in [Method::Store, Method::Deflate] {
        let s = Scratch::new();
        fs::write(s.0.join("source"), ENCRYPTED_PAYLOAD).unwrap();
        let events = interactive(
            Start {
                action: Action::CreateZip {
                    sources: vec![Source {
                        path: s.path("source"),
                        archive_name: "payload.bin".into(),
                    }],
                    output_file: s.path("encrypted.zip"),
                    method,
                    encryption: Encryption::Aes256,
                },
                limits: Limits::default(),
            },
            |id, attempt, purpose| {
                assert_eq!(purpose, PasswordPurpose::Encrypt);
                assert_eq!(attempt, 1);
                password_reply(id, TEST_PASSWORD)
            },
        );
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        assert!(events.iter().any(|e| matches!(
            e,
            Message::Accepted {
                encryption: true,
                ..
            }
        )));
        #[cfg(target_os = "linux")]
        {
            let helper = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/rust-archive-worker/encryption_fixtures.py");
            assert!(
                Command::new("python3")
                    .arg(helper)
                    .arg("--verify")
                    .arg(s.0.join("encrypted.zip"))
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let bytes = fs::read(s.0.join("encrypted.zip")).unwrap();
        let start = encrypted_start(&s, &bytes);
        let events = interactive(start, |id, _, _| password_reply(id, TEST_PASSWORD));
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        assert_eq!(
            fs::read(s.0.join("output/payload.bin")).unwrap(),
            ENCRYPTED_PAYLOAD
        );
    }
}
#[test]
fn wrong_password_retries_with_new_challenge_then_succeeds() {
    let s = Scratch::new();
    let start = encrypted_start(&s, AES_ZIP);
    let mut previous = 0;
    let events = interactive(start, |id, attempt, _| {
        assert!(id > previous);
        previous = id;
        password_reply(
            id,
            if attempt == 1 {
                b"wrong"
            } else {
                TEST_PASSWORD
            },
        )
    });
    assert_eq!(terminal(&events).outcome, Outcome::Completed);
    assert_eq!(previous, 2);
}
#[test]
fn wrong_password_attempts_are_bounded_without_writes() {
    let s = Scratch::new();
    let start = encrypted_start(&s, AES_ZIP);
    let mut attempts = 0;
    let events = interactive(start, |id, attempt, _| {
        attempts = attempt;
        password_reply(id, b"wrong")
    });
    assert_eq!(attempts, 3);
    no_encrypted_output(&s, &events, ErrorCode::WrongPassword);
}
#[test]
fn cancellation_while_waiting_for_password_is_acknowledged() {
    let s = Scratch::new();
    let start = encrypted_start(&s, AES_ZIP);
    let events = interactive(start, |_, _, _| vec![Message::Cancel]);
    assert_eq!(terminal(&events).outcome, Outcome::Cancelled);
    assert!(events.iter().any(|e| matches!(
        e,
        Message::CancelAck {
            status: CancelStatus::Requested,
            ..
        }
    )));
    no_encrypted_output(&s, &events, ErrorCode::None);
}
#[test]
fn stale_and_mismatched_password_challenges_fail_without_writes() {
    for stale in [false, true] {
        let s = Scratch::new();
        let start = encrypted_start(&s, AES_ZIP);
        let events = interactive(start, |id, attempt, _| {
            if stale && attempt == 1 {
                password_reply(id, b"wrong")
            } else {
                password_reply(if stale { id - 1 } else { id + 1 }, TEST_PASSWORD)
            }
        });
        no_encrypted_output(&s, &events, ErrorCode::Protocol);
    }
}
#[test]
fn password_input_eof_fails_without_writes() {
    let s = Scratch::new();
    let start = encrypted_start(&s, AES_ZIP);
    let events = interactive(start, |_, _, _| vec![]);
    no_encrypted_output(&s, &events, ErrorCode::Protocol);
}
#[test]
fn aes_authentication_failure_never_publishes_plaintext() {
    for tamper_tag in [false, true] {
        let mut bytes = AES_ZIP.to_vec();
        let central = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
        if tamper_tag {
            bytes[central - 1] ^= 1;
        } else {
            bytes[30 + b"payload.bin".len() + 11 + 18] ^= 1;
        }
        let s = Scratch::new();
        let start = encrypted_start(&s, &bytes);
        let events = interactive(start, |id, _, _| password_reply(id, TEST_PASSWORD));
        no_encrypted_output(&s, &events, ErrorCode::Integrity);
    }
}
#[test]
fn aes1_crc_and_extra_consistency_are_checked() {
    let original = include_bytes!("fixtures/aes1-store.zip");
    for corrupt_extra in [false, true] {
        let mut bytes = original.to_vec();
        let central = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
        if corrupt_extra {
            bytes[30 + b"payload.bin".len() + 4] = 2;
        } else {
            bytes[14] ^= 1;
            bytes[central + 16] ^= 1;
        }
        let s = Scratch::new();
        let start = encrypted_start(&s, &bytes);
        let events = interactive(start, |id, _, _| password_reply(id, TEST_PASSWORD));
        no_encrypted_output(&s, &events, ErrorCode::Integrity);
    }
}
#[test]
fn empty_aes_members_and_random_salts_round_trip() {
    let members = vec![archive::Member {
        entry: Entry {
            entry_id: 0,
            name: "empty".into(),
            kind: EntryKind::File,
            size: 0,
            compressed_size: 0,
            encrypted: false,
        },
        data: vec![],
    }];
    let a = archive::create_zip_with_password(
        &members,
        Method::Store,
        Limits::default(),
        Some(TEST_PASSWORD),
        &mut || Ok(()),
    )
    .unwrap();
    let b = archive::create_zip_with_password(
        &members,
        Method::Store,
        Limits::default(),
        Some(TEST_PASSWORD),
        &mut || Ok(()),
    )
    .unwrap();
    assert_ne!(a, b);
    let decoded = archive::parse_with_password(
        &a,
        Limits::default(),
        true,
        Some(TEST_PASSWORD),
        &mut || Ok(()),
    )
    .unwrap();
    assert!(decoded.members[0].data.is_empty());
}

#[test]
fn password_racing_after_cancel_does_not_override_cancellation() {
    let s = Scratch::new();
    let start = encrypted_start(&s, AES_ZIP);
    let events = interactive(start, |id, _, _| {
        vec![
            Message::Cancel,
            Message::Password {
                challenge_id: id,
                secret: Secret(TEST_PASSWORD.to_vec()),
            },
        ]
    });
    assert_eq!(terminal(&events).outcome, Outcome::Cancelled);
    no_encrypted_output(&s, &events, ErrorCode::None);
}
#[test]
fn empty_encryption_password_never_creates_zip() {
    let s = Scratch::new();
    let events = interactive(
        Start {
            action: Action::CreateZip {
                sources: vec![],
                output_file: s.path("empty.zip"),
                method: Method::Store,
                encryption: Encryption::Aes256,
            },
            limits: Limits::default(),
        },
        |id, _, _| password_reply(id, b""),
    );
    assert_eq!(terminal(&events).error, ErrorCode::InvalidRequest);
    assert_eq!(terminal(&events).publication, Publication::None);
    assert!(!s.0.join("empty.zip").exists());
}

const SEVEN_PAYLOAD: &[u8] = b"Zipelo 7z interoperability\n";
fn seven_fixture(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/rust-archive-worker/fixtures/7z")
            .join(name),
    )
    .unwrap()
}
#[test]
fn seven_zip_lzma_lzma2_and_copy_interoperate() {
    for name in [
        "copy.7z",
        "lzma.7z",
        "lzma2.7z",
        "properties-0.7z",
        "properties-1.7z",
        "properties-2.7z",
        "properties-3.7z",
        "libarchive-lzma1.7z",
        "libarchive-lzma2.7z",
    ] {
        let bytes = seven_fixture(name);
        let catalog = archive::parse(&bytes, Limits::default(), true, &mut || Ok(()))
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(catalog.format, Format::SevenZip);
        assert_eq!(catalog.members[0].data, SEVEN_PAYLOAD.repeat(100), "{name}");
        let s = Scratch::new();
        let events = extract(&s, &bytes, None, Selection::All, None, Limits::default());
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Completed,
            "{name}: {events:?}"
        );
        assert_eq!(
            fs::read(s.0.join("output/folder/hello.txt")).unwrap(),
            SEVEN_PAYLOAD.repeat(100)
        );
    }
}
#[test]
fn seven_zip_encoded_header_solid_and_empty_entries() {
    let s = Scratch::new();
    let bytes = seven_fixture("encoded-solid.7z");
    let catalog = archive::parse(&bytes, Limits::default(), false, &mut || Ok(())).unwrap();
    assert_eq!(catalog.members.len(), 4);
    let events = extract(&s, &bytes, None, Selection::All, None, Limits::default());
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(fs::read(s.0.join("output/folder/empty")).unwrap(), b"");
    assert_eq!(
        fs::read(s.0.join("output/folder/hello.txt")).unwrap(),
        SEVEN_PAYLOAD.repeat(100)
    );
    let mut reverse = SEVEN_PAYLOAD.repeat(100);
    reverse.reverse();
    assert_eq!(fs::read(s.0.join("output/한글.txt")).unwrap(), reverse);
}
#[test]
fn seven_zip_long_matches_and_lzma2_chunks() {
    for name in ["long-match.7z", "lzma2-chunks.7z"] {
        let bytes = seven_fixture(name);
        let catalog = archive::parse(&bytes, Limits::default(), true, &mut || Ok(()))
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        if name == "long-match.7z" {
            assert_eq!(catalog.members[0].data, SEVEN_PAYLOAD.repeat(10000));
        } else {
            let expected = (0..100000u64)
                .map(|i| (((i * 1103515245 + (i >> 2) * 12345) >> ((i % 4) * 8)) & 255) as u8)
                .collect::<Vec<_>>();
            assert_eq!(catalog.members[0].data, expected);
        }
    }
}
#[test]
fn seven_zip_malicious_inputs_never_write() {
    for name in [
        "unsafe-dotdot.7z",
        "unsafe-absolute.7z",
        "unsafe-drive.7z",
        "unsafe-backslash.7z",
        "unsafe-deep.7z",
        "symlink.7z",
        "collision.7z",
        "bad-payload.7z",
        "bad-crc.7z",
        "bad-dictionary.7z",
    ] {
        let s = Scratch::new();
        let bytes = seven_fixture(name);
        let events = extract(&s, &bytes, None, Selection::All, None, Limits::default());
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Failed,
            "{name}: {events:?}"
        );
        assert_eq!(terminal(&events).publication, Publication::None);
        assert_eq!(terminal(&events).created_count, 0);
        assert!(!events.iter().any(|e| matches!(e, Message::Output { .. })));
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
        assert!(!s.0.join("outside.txt").exists());
    }
}

#[test]
fn seven_zip_format_wire_tag_preserves_existing_encodings() {
    for (tag, format) in [
        Format::Zip,
        Format::Tar,
        Format::Gzip,
        Format::TarGzip,
        Format::SevenZip,
    ]
    .into_iter()
    .enumerate()
    {
        let token = ListId {
            catalog_version: 1,
            format,
            source_object: SourceObject::Unix {
                device: 1,
                inode: 2,
            },
            archive_bytes: 3,
            archive_sha256: [4; 32],
        };
        let mut bytes = Vec::new();
        Writer::new(&mut bytes, 17)
            .send(&Message::ListEnd {
                entry_count: 5,
                list_id: token.clone(),
            })
            .unwrap();
        assert_eq!(bytes[38], tag as u8);
        match Reader::new(bytes.as_slice()).read().unwrap().unwrap().2 {
            Message::ListEnd {
                entry_count,
                list_id,
            } => {
                assert_eq!(entry_count, 5);
                assert_eq!(list_id, token);
            }
            _ => panic!("expected ListEnd"),
        }
    }
}
#[test]
fn seven_zip_listing_token_and_selection() {
    let s = Scratch::new();
    let bytes = seven_fixture("encoded-solid.7z");
    fs::write(s.0.join("input"), &bytes).unwrap();
    let events = execute(Start {
        action: Action::List {
            archive: s.path("input"),
        },
        limits: Limits::default(),
    });
    assert_eq!(terminal(&events).outcome, Outcome::Completed);
    let list_id = events
        .into_iter()
        .find_map(|e| {
            if let Message::ListEnd { list_id, .. } = e {
                Some(list_id)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(list_id.format, Format::SevenZip);
    let events = extract(
        &s,
        &bytes,
        None,
        Selection::EntryIds(vec![3]),
        Some(list_id),
        Limits::default(),
    );
    assert_eq!(terminal(&events).outcome, Outcome::Completed);
    assert_eq!(terminal(&events).created_count, 1);
    assert!(!s.0.join("output/folder").exists());
    assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 1);
}
#[test]
fn seven_zip_limits_truncation_and_existing_files_are_preserved() {
    let bytes = seven_fixture("lzma2.7z");
    for cut in [0, 1, 6, 12, 31, 32, bytes.len() - 1] {
        let s = Scratch::new();
        let events = extract(
            &s,
            &bytes[..cut],
            None,
            Selection::All,
            None,
            Limits::default(),
        );
        assert_eq!(terminal(&events).outcome, Outcome::Failed);
        assert_eq!(terminal(&events).publication, Publication::None);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
    }
    let s = Scratch::new();
    let events = extract(
        &s,
        &bytes,
        None,
        Selection::All,
        None,
        Limits {
            output: 100,
            ..Limits::default()
        },
    );
    assert_eq!(terminal(&events).error, ErrorCode::Limit);
    assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
    fs::create_dir(s.0.join("output/folder")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(s.0.join("output/folder"), fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(s.0.join("output/folder/hello.txt"), b"preserve").unwrap();
    let events = extract(&s, &bytes, None, Selection::All, None, Limits::default());
    assert_eq!(terminal(&events).error, ErrorCode::Conflict);
    assert_eq!(
        fs::read(s.0.join("output/folder/hello.txt")).unwrap(),
        b"preserve"
    );
    let s = Scratch::new();
    let events = extract(
        &s,
        &seven_fixture("bad-trailing.7z"),
        None,
        Selection::All,
        None,
        Limits::default(),
    );
    assert_eq!(terminal(&events).error, ErrorCode::Integrity);
    assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
}
#[test]
fn seven_zip_parser_honors_cancellation_checkpoints() {
    let bytes = seven_fixture("long-match.7z");
    let mut calls = 0;
    let result = archive::parse(&bytes, Limits::default(), true, &mut || {
        calls += 1;
        if calls >= 5 {
            Err(ErrorCode::None)
        } else {
            Ok(())
        }
    });
    assert!(matches!(result, Err(ErrorCode::None)));
}
#[test]
fn pr64_malicious_corpus_is_rejected_without_writes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/malicious-archive-fixtures/fixtures");
    let cases = [
        ("zip-dotdot.zip", ErrorCode::UnsafePath, 131072, None),
        (
            "zip-absolute-posix.zip",
            ErrorCode::UnsafePath,
            131072,
            None,
        ),
        (
            "zip-absolute-drive.zip",
            ErrorCode::UnsafePath,
            131072,
            None,
        ),
        (
            "zip-backslash-dotdot.zip",
            ErrorCode::UnsafePath,
            131072,
            None,
        ),
        ("tar-dotdot.tar", ErrorCode::UnsafePath, 131072, None),
        ("targz-dotdot.tar.gz", ErrorCode::UnsafePath, 131072, None),
        (
            "tar-absolute-posix.tar",
            ErrorCode::UnsafePath,
            131072,
            None,
        ),
        ("zip-symlink-escape.zip", ErrorCode::Conflict, 131072, None),
        ("tar-symlink-escape.tar", ErrorCode::Conflict, 131072, None),
        ("zip-output-limit.zip", ErrorCode::Limit, 1024, None),
        (
            "gzip-output-limit.gz",
            ErrorCode::Limit,
            1024,
            Some("payload.bin"),
        ),
        ("targz-output-limit.tar.gz", ErrorCode::Limit, 1024, None),
        (
            "zip-bad-local-header.zip",
            ErrorCode::Integrity,
            131072,
            None,
        ),
        ("tar-bad-checksum.tar", ErrorCode::Integrity, 131072, None),
        (
            "gzip-reserved-flags.gz",
            ErrorCode::Integrity,
            131072,
            Some("payload.bin"),
        ),
    ];
    for (name, expected, output, gzip) in cases {
        let s = Scratch::new();
        let bytes = fs::read(root.join(name)).unwrap();
        fs::write(s.0.join("outside.txt"), b"preserve").unwrap();
        let events = extract(
            &s,
            &bytes,
            gzip,
            Selection::All,
            None,
            Limits {
                input: 1048576,
                output,
                entries: 64,
            },
        );
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Failed,
            "{name}: {events:?}"
        );
        assert_eq!(terminal(&events).error, expected, "{name}");
        assert_eq!(terminal(&events).publication, Publication::None);
        assert_eq!(terminal(&events).created_count, 0);
        assert_eq!(terminal(&events).retained_partial_count, 0);
        assert!(!events.iter().any(|e| matches!(e, Message::Output { .. })));
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
        assert_eq!(fs::read(s.0.join("outside.txt")).unwrap(), b"preserve");
        assert_eq!(fs::read(s.0.join("input")).unwrap(), bytes);
    }
}
#[cfg(target_os = "linux")]
#[test]
fn seven_zip_reference_reader_accepts_synthetic_fixtures() {
    let helper = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/rust-archive-worker/seven_zip_reference.py");
    assert!(
        Command::new("python3")
            .arg(helper)
            .status()
            .unwrap()
            .success()
    );
}

mod hardening;

#[test]
fn disk_full_error_tag_preserves_previous_tags_and_rejects_unknown() {
    for (tag, error) in [
        ErrorCode::None,
        ErrorCode::Integrity,
        ErrorCode::UnsupportedFormat,
        ErrorCode::UnsupportedFeature,
        ErrorCode::WrongPassword,
        ErrorCode::UnsafePath,
        ErrorCode::Conflict,
        ErrorCode::Limit,
        ErrorCode::Io,
        ErrorCode::Protocol,
        ErrorCode::Internal,
        ErrorCode::SourceChanged,
        ErrorCode::InvalidRequest,
        ErrorCode::DiskFull,
    ]
    .into_iter()
    .enumerate()
    {
        let message = Message::Terminal(Terminal {
            outcome: Outcome::Failed,
            error,
            publication: Publication::None,
            created_count: 0,
            retained_partial_count: 0,
            durability: Durability::NotApplicable,
            cleanup: Cleanup::Complete,
            detail: String::new(),
        });
        let mut bytes = Vec::new();
        Writer::new(&mut bytes, 17).send(&message).unwrap();
        assert_eq!(bytes[33], tag as u8);
        let decoded = Reader::new(bytes.as_slice()).read().unwrap().unwrap().2;
        assert_eq!(terminal(&[decoded]).error, error);
        bytes[33] = 14;
        assert!(Reader::new(bytes.as_slice()).read().is_err());
    }
}

mod folder_inputs;

mod zip_names;

mod conflict_wire;

mod conflicts;

mod compressed_tar;
