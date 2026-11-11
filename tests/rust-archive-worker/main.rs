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
    let mut child = Command::new(env!("CARGO_BIN_EXE_zipelo-archive-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
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
fn encrypted_creation_is_explicitly_unsupported() {
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
    assert_eq!(terminal(&e).error, ErrorCode::UnsupportedFeature);
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
fn encrypted_zip_rejects_without_outputs() {
    let mut bytes = include_bytes!("fixtures/store.zip").to_vec();
    bytes[6] |= 1;
    let central = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
    bytes[central + 8] |= 1;
    let s = Scratch::new();
    let e = extract(&s, &bytes, None, Selection::All, None, Limits::default());
    assert_eq!(terminal(&e).error, ErrorCode::UnsupportedFeature);
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
