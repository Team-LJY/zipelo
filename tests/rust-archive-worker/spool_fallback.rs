use super::*;
use std::{
    io::Read,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    process::Child,
    sync::mpsc,
    time::Duration,
};

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const PAYLOAD: &[u8] = b"spool fallback preview\n";
struct Case {
    scratch: Scratch,
    archive_names: Vec<String>,
}
impl Case {
    fn new() -> Self {
        let scratch = Scratch::new();
        for name in ["cache", "archive", "system"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(scratch.0.join(name))
                .unwrap();
        }
        assert!(
            Command::new("python3")
                .arg("-B")
                .arg(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../tests/rust-archive-worker/spool_fallback_cases.py")
                )
                .arg(&scratch.0)
                .status()
                .unwrap()
                .success()
        );
        Self {
            scratch,
            archive_names: vec!["input.7z".into(), "input.zip".into()],
        }
    }
    fn split(&mut self) -> (NativePath, Vec<(PathBuf, Vec<u8>)>) {
        let bytes = fs::read(self.archive("7z").to_path().unwrap()).unwrap();
        let mut originals = Vec::new();
        for (index, chunk) in bytes.chunks(bytes.len().div_ceil(9)).enumerate() {
            let name = format!("split.7z.{:03}", index + 1);
            let path = self.scratch.0.join("archive").join(&name);
            fs::write(&path, chunk).unwrap();
            originals.push((path, chunk.to_vec()));
            self.archive_names.push(name);
        }
        self.archive_names.sort();
        (self.scratch.path("archive/split.7z.001"), originals)
    }
    fn library(&self) -> PathBuf {
        let library = self.scratch.0.join("location.so");
        if !library.exists() {
            assert!(
                Command::new("cc")
                    .args(["-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror"])
                    .arg(
                        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join("../../tests/rust-archive-worker/spool_location_fault.c")
                    )
                    .args(["-ldl", "-o"])
                    .arg(&library)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        library
    }
    fn run(&self, action: Action, phase: &str, mask: u8, named: bool) -> Vec<Message> {
        let s = &self.scratch;
        let mut command = s.worker();
        command
            .env("XDG_CACHE_HOME", s.0.join("cache"))
            .env("TMPDIR", s.0.join("system"))
            .env("HOME", &s.0);
        if phase == "home" {
            command.env_remove("XDG_CACHE_HOME");
        }
        if phase.ends_with("changed") {
            command.env(
                "ZIPELO_SPOOL_SOURCE",
                match &action {
                    Action::List { archive } | Action::Preview { archive, .. } => {
                        archive.to_path().unwrap()
                    }
                    _ => panic!("source mutation requires an archive"),
                },
            );
        }
        let library = self.library();
        command
            .env("LD_PRELOAD", library)
            .env("ZIPELO_SPOOL_PHASE", phase)
            .env("ZIPELO_SPOOL_MASK", mask.to_string())
            .env("ZIPELO_SPOOL_TRACE", s.0.join("trace"));
        if named {
            command.env("ZIPELO_SPOOL_NAMED", "1");
        }
        let mut child = Running(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let preview = matches!(action, Action::Preview { .. });
        let start = Start {
            action,
            limits: Limits::default(),
        };
        let message = if preview {
            Message::StartGuarded(GuardedStart {
                start,
                ceiling: Limits::default(),
                interactive: true,
            })
        } else {
            Message::Start(start)
        };
        let mut writer = Some(Writer::new(child.0.stdin.take().unwrap(), 17));
        writer.as_mut().unwrap().send(&message).unwrap();
        let stdout = child.0.stdout.take().unwrap();
        let mut stderr = child.0.stderr.take().unwrap();
        let (send, receive) = mpsc::sync_channel(128);
        let reader = std::thread::spawn(move || {
            let mut reader = Reader::new(stdout);
            loop {
                let frame = reader.read();
                let done = !matches!(frame, Ok(Some(_)));
                if send.send(frame).is_err() || done {
                    break;
                }
            }
        });
        let mut events = Vec::new();
        loop {
            let Some((job, _, event)) = receive
                .recv_timeout(Duration::from_secs(30))
                .expect("worker timeout")
                .unwrap()
            else {
                break;
            };
            assert_eq!(job, 17);
            assert!(
                !matches!(
                    event,
                    Message::ResourceRequired { .. } | Message::PasswordRequired { .. }
                ),
                "{event:?}"
            );
            if matches!(event, Message::Terminal(_)) {
                drop(writer.take());
            }
            events.push(event);
        }
        assert!(child.0.wait().unwrap().success(), "{events:?}");
        let mut errors = Vec::new();
        stderr.read_to_end(&mut errors).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        reader.join().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Message::Terminal(_)))
                .count(),
            1
        );
        self.clean();
        events
    }
    fn archive(&self, format: &str) -> NativePath {
        self.scratch.path(&format!("archive/input.{format}"))
    }
    fn list(&self, format: &str, phase: &str, mask: u8, named: bool) -> Vec<Message> {
        self.run(
            Action::List {
                archive: self.archive(format),
            },
            phase,
            mask,
            named,
        )
    }
    fn clean(&self) {
        let root = &self.scratch.0;
        for directory in ["cache/zipelo", ".cache/zipelo", "system"] {
            let path = root.join(directory);
            if path.is_dir() {
                assert_eq!(fs::read_dir(&path).unwrap().count(), 0, "{path:?}");
            }
        }
        let mut names: Vec<_> = fs::read_dir(root.join("archive"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, self.archive_names);
    }
    fn preview(&self, format: &str, phase: &str, mask: u8, named: bool) -> Vec<Message> {
        let events = self.list(format, phase, mask, named);
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Completed,
            "{format}/{phase}/{mask}: {events:?}"
        );
        let list_id = events
            .iter()
            .find_map(|e| match e {
                Message::ListEnd { list_id, .. } => Some(list_id.clone()),
                _ => None,
            })
            .unwrap();
        let entry_id = events
            .iter()
            .find_map(|e| match e {
                Message::Entry(entry) if entry.name == "preview.txt" => Some(entry.entry_id),
                _ => None,
            })
            .unwrap();
        self.run(
            Action::Preview {
                archive: self.archive(format),
                list_id,
                entry_id,
            },
            phase,
            mask,
            named,
        )
    }
    fn assert_preview(&self, format: &str, phase: &str, mask: u8, named: bool) {
        let path = self.scratch.0.join(format!("archive/input.{format}"));
        let original = fs::read(&path).unwrap();
        assert!(original.len() > 16 * 1024 * 1024);
        let events = self.preview(format, phase, mask, named);
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Completed,
            "{format}/{phase}/{mask}: {events:?}"
        );
        let mut preview = Vec::new();
        for event in &events {
            if let Message::PreviewChunk { data, .. } = event {
                preview.extend_from_slice(data);
            }
        }
        assert_eq!(preview, PAYLOAD);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Message::PreviewEnd { .. }))
        );
        assert_eq!(fs::read(path).unwrap(), original);
    }
}
#[test]
fn cache_source_and_system_locations_retry_creation_space_and_partial_quota_writes() {
    for named in [false, true] {
        for phase in ["create", "space", "write"] {
            for mask in [0, 1, 3] {
                let c = Case::new();
                for format in ["zip", "7z"] {
                    c.assert_preview(format, phase, mask, named);
                }
                let trace = fs::read_to_string(c.scratch.0.join("trace")).unwrap();
                let expected = match mask {
                    0 => '0',
                    1 => '1',
                    _ => '2',
                };
                assert!(trace.contains(expected), "{phase}/{mask}: {trace}");
                if mask == 0 {
                    assert!(!trace.contains('1') && !trace.contains('2'), "{trace}");
                }
                if mask == 1 {
                    assert!(!trace.contains('2'), "{trace}");
                }
            }
        }
    }
}
#[test]
fn split_seven_zip_lists_without_archive_sized_temporary_storage() {
    use sha2::{Digest, Sha256};
    for named in [false, true] {
        for phase in ["write", "space", "create"] {
            for mask in [1, 3, 7] {
                let mut c = Case::new();
                let (archive, originals) = c.split();
                assert_eq!(originals.len(), 9);
                let bytes = fs::read(c.archive("7z").to_path().unwrap()).unwrap();
                let zip = fs::read(c.archive("zip").to_path().unwrap()).unwrap();
                let events = c.run(Action::List { archive }, phase, mask, named);
                if mask == 7 && phase == "create" {
                    assert_eq!(
                        terminal(&events).outcome,
                        Outcome::Failed,
                        "{phase}/{named}: {events:?}"
                    );
                    assert_eq!(terminal(&events).error, ErrorCode::DiskFull, "{events:?}");
                    assert!(!events.iter().any(|e| matches!(e, Message::ListEnd { .. })));
                } else {
                    assert_eq!(
                        terminal(&events).outcome,
                        Outcome::Completed,
                        "{phase}/{mask}/{named}: {events:?}"
                    );
                    let id = events
                        .iter()
                        .find_map(|e| match e {
                            Message::ListEnd { list_id, .. } => Some(list_id),
                            _ => None,
                        })
                        .unwrap();
                    assert_eq!(id.catalog_version, 2);
                    assert_eq!(id.format, Format::SevenZip);
                    assert_eq!(id.archive_bytes, bytes.len() as u64);
                    assert_eq!(
                        id.archive_sha256.as_slice(),
                        Sha256::digest(&bytes).as_slice()
                    );
                    assert!(matches!(
                        id.source_object,
                        SourceObject::SplitSevenZip { .. }
                    ));
                    assert!(events.iter().any(
                        |e| matches!(e, Message::Entry(entry) if entry.name == "preview.txt")
                    ));
                }
                let trace = fs::read_to_string(c.scratch.0.join("trace")).unwrap();
                if phase == "create" {
                    for at in if mask == 1 {
                        &['0', '1'][..]
                    } else {
                        &['0', '1', '2'][..]
                    } {
                        assert!(trace.contains(*at), "{phase}/{mask}/{named}: {trace}");
                    }
                } else {
                    assert!(!trace.contains('1') && !trace.contains('2'), "{trace}");
                }
                for (path, original) in originals {
                    assert_eq!(fs::read(path).unwrap(), original);
                }
                assert_eq!(fs::read(c.archive("7z").to_path().unwrap()).unwrap(), bytes);
                assert_eq!(fs::read(c.archive("zip").to_path().unwrap()).unwrap(), zip);
            }
        }
    }
}

#[test]
fn split_source_change_during_direct_read_prevents_success() {
    for named in [false, true] {
        let mut c = Case::new();
        let (archive, originals) = c.split();
        let events = c.run(Action::List { archive }, "changed", 1, named);
        assert_eq!(terminal(&events).outcome, Outcome::Failed, "{events:?}");
        assert_eq!(
            terminal(&events).error,
            ErrorCode::SourceChanged,
            "{events:?}"
        );
        assert!(!events.iter().any(|e| matches!(e, Message::ListEnd { .. })));
        for (index, (path, mut bytes)) in originals.into_iter().enumerate() {
            if index == 0 {
                bytes.push(b'x');
            }
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
}

#[test]
fn archive_larger_than_every_temporary_quota_lists_previews_and_extracts_with_same_id() {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::MetadataExt;
    for named in [false, true] {
        let mut c = Case::new();
        let solid = c.list("7z", "quota", 7, named);
        assert_eq!(terminal(&solid).outcome, Outcome::Completed, "{solid:?}");
        assert!(
            Command::new("python3")
                .arg("-B")
                .arg(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../tests/rust-archive-worker/spool_fallback_cases.py")
                )
                .arg(&c.scratch.0)
                .arg("non-solid")
                .status()
                .unwrap()
                .success()
        );
        for format in ["zip", "7z"] {
            c.assert_preview(format, "quota", 7, named);
            let events = c.list(format, "quota", 7, named);
            let id = events
                .iter()
                .find_map(|event| match event {
                    Message::ListEnd { list_id, .. } => Some(list_id.clone()),
                    _ => None,
                })
                .unwrap();
            let path = c.archive(format).to_path().unwrap();
            let bytes = fs::read(&path).unwrap();
            let metadata = fs::metadata(path).unwrap();
            assert!(bytes.len() > 16 * 1024 * 1024);
            assert_eq!(id.catalog_version, 1);
            assert_eq!(id.archive_bytes, bytes.len() as u64);
            assert_eq!(
                id.archive_sha256.as_slice(),
                Sha256::digest(&bytes).as_slice()
            );
            assert_eq!(
                id.source_object,
                SourceObject::Unix {
                    device: metadata.dev(),
                    inode: metadata.ino()
                }
            );
            let output = c.scratch.0.join(format!("output-{format}"));
            fs::DirBuilder::new().mode(0o700).create(&output).unwrap();
            let extracted = execute(Start {
                action: Action::Extract {
                    archive: c.archive(format),
                    destination: NativePath::from_path(&output),
                    selection: Selection::EntryIds(vec![0]),
                    list_id: Some(id),
                    gzip_output_name: None,
                },
                limits: Limits::default(),
            });
            assert_eq!(
                terminal(&extracted).outcome,
                Outcome::Completed,
                "{extracted:?}"
            );
            assert_eq!(fs::read(output.join("preview.txt")).unwrap(), PAYLOAD);
        }
        let (archive, originals) = c.split();
        let events = c.run(
            Action::List {
                archive: archive.clone(),
            },
            "quota",
            7,
            named,
        );
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        let id = events
            .iter()
            .find_map(|event| match event {
                Message::ListEnd { list_id, .. } => Some(list_id.clone()),
                _ => None,
            })
            .unwrap();
        let output = c.scratch.0.join("output-split");
        fs::DirBuilder::new().mode(0o700).create(&output).unwrap();
        let extracted = execute(Start {
            action: Action::Extract {
                archive: archive.clone(),
                destination: NativePath::from_path(&output),
                selection: Selection::EntryIds(vec![0]),
                list_id: Some(id.clone()),
                gzip_output_name: None,
            },
            limits: Limits::default(),
        });
        assert_eq!(
            terminal(&extracted).outcome,
            Outcome::Completed,
            "{extracted:?}"
        );
        assert_eq!(fs::read(output.join("preview.txt")).unwrap(), PAYLOAD);
        let preview = c.run(
            Action::Preview {
                archive,
                list_id: id,
                entry_id: 0,
            },
            "quota",
            7,
            named,
        );
        assert_eq!(terminal(&preview).error, ErrorCode::UnsupportedFeature);
        for (path, bytes) in originals {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
}

#[test]
fn source_change_after_initial_hash_rejects_list_and_preview_results() {
    for named in [false, true] {
        for format in ["zip", "7z"] {
            let c = Case::new();
            let events = c.list(format, "late-changed", 7, named);
            assert_eq!(
                terminal(&events).error,
                ErrorCode::SourceChanged,
                "{events:?}"
            );
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, Message::ListEnd { .. }))
            );
            let c = Case::new();
            let events = c.list(format, "quota", 7, named);
            let id = events
                .iter()
                .find_map(|event| match event {
                    Message::ListEnd { list_id, .. } => Some(list_id.clone()),
                    _ => None,
                })
                .unwrap();
            let events = c.run(
                Action::Preview {
                    archive: c.archive(format),
                    list_id: id,
                    entry_id: 0,
                },
                "late-changed",
                7,
                named,
            );
            assert_eq!(
                terminal(&events).error,
                ErrorCode::SourceChanged,
                "{events:?}"
            );
            assert!(!events.iter().any(|event| matches!(
                event,
                Message::PreviewBegin(_) | Message::PreviewEnd { .. }
            )));
        }
        let mut c = Case::new();
        let (archive, _) = c.split();
        let events = c.run(Action::List { archive }, "late-changed", 7, named);
        assert_eq!(
            terminal(&events).error,
            ErrorCode::SourceChanged,
            "{events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Message::ListEnd { .. }))
        );
    }
}

#[test]
fn disk_full_requires_exhausting_all_three_locations_and_io_errors_stay_io() {
    for phase in ["create", "space", "write"] {
        let c = Case::new();
        for format in ["zip", "7z"] {
            let events = if phase == "create" {
                c.list(format, phase, 7, false)
            } else {
                c.preview(
                    format,
                    if phase == "space" {
                        "stage-space"
                    } else {
                        phase
                    },
                    7,
                    false,
                )
            };
            assert_eq!(
                terminal(&events).error,
                ErrorCode::DiskFull,
                "{phase}: {events:?}"
            );
            assert_eq!(terminal(&events).outcome, Outcome::Failed);
            assert!(!events.iter().any(|e| matches!(e, Message::ListEnd { .. })));
        }
        let trace = fs::read_to_string(c.scratch.0.join("trace")).unwrap();
        for at in ['0', '1', '2'] {
            assert!(trace.contains(at), "{phase}: {trace}");
        }
    }
    let c = Case::new();
    for phase in ["io", "denied"] {
        let events = if phase == "io" {
            c.preview("zip", phase, 7, false)
        } else {
            c.list("zip", phase, 7, false)
        };
        assert_eq!(terminal(&events).error, ErrorCode::Io, "{events:?}");
    }
}
#[test]
fn unsafe_cache_is_skipped_without_following_its_link() {
    use std::os::unix::fs::symlink;
    let c = Case::new();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(c.scratch.0.join("outside"))
        .unwrap();
    symlink(
        c.scratch.0.join("outside"),
        c.scratch.0.join("cache/zipelo"),
    )
    .unwrap();
    let events = c.list("zip", "create", 0, false);
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(
        fs::read_dir(c.scratch.0.join("outside")).unwrap().count(),
        0
    );
    fs::remove_file(c.scratch.0.join("cache/zipelo")).unwrap();
    c.clean();
}
#[test]
fn preview_decode_retries_when_snapshot_fits_but_decoded_staging_does_not() {
    for named in [false, true] {
        for phase in ["stage-space", "stage-write"] {
            for mask in [1, 3] {
                let c = Case::new();
                for format in ["zip", "7z"] {
                    c.assert_preview(format, phase, mask, named);
                }
            }
            let c = Case::new();
            for format in ["zip", "7z"] {
                let events = c.preview(format, phase, 7, named);
                assert_eq!(terminal(&events).outcome, Outcome::Failed, "{events:?}");
                assert_eq!(terminal(&events).error, ErrorCode::DiskFull, "{events:?}");
                assert!(
                    !events
                        .iter()
                        .any(|event| matches!(event, Message::PreviewEnd { .. }))
                );
            }
        }
    }
}

#[test]
fn default_home_cache_is_created_privately() {
    let c = Case::new();
    c.assert_preview("zip", "home", 0, false);
    for name in [".cache", ".cache/zipelo"] {
        assert_eq!(
            fs::metadata(c.scratch.0.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn source_change_during_direct_read_prevents_success() {
    let c = Case::new();
    let events = c.list("zip", "changed", 1, false);
    assert_eq!(terminal(&events).outcome, Outcome::Failed, "{events:?}");
    assert_eq!(
        terminal(&events).error,
        ErrorCode::SourceChanged,
        "{events:?}"
    );
    assert!(!c.scratch.0.join("trace").exists());
}

#[test]
fn small_temporary_quota_lists_and_previews_large_zip_and_seven_zip() {
    for named in [false, true] {
        let c = Case::new();
        let library = c.library();
        let probe = Command::new("python3")
            .arg("-B")
            .arg("-c")
            .arg(
                r#"
import errno, os, sys
from pathlib import Path
root = Path(sys.argv[1])
a = os.open(root / "quota-a", os.O_CREAT | os.O_RDWR, 0o600)
b = os.open(root / "quota-b", os.O_CREAT | os.O_RDWR, 0o600)
duplicate = os.dup(a)
try:
    assert os.write(a, bytes(1024 * 1024)) == 1024 * 1024
    assert os.write(b, bytes(2 * 1024 * 1024)) == 1024 * 1024
    try:
        os.write(b, b"x")
    except OSError as error:
        assert error.errno == errno.ENOSPC
    else:
        raise AssertionError("공유 임시 용량 한도가 적용되지 않음")
    os.close(duplicate)
    duplicate = -1
    os.close(a)
    a = -1
    (root / "quota-a").unlink()
    assert os.write(b, b"x") == 1
finally:
    if duplicate >= 0:
        os.close(duplicate)
    if a >= 0:
        os.close(a)
    os.close(b)
    for name in ["quota-a", "quota-b"]:
        (root / name).unlink(missing_ok=True)
"#,
            )
            .arg(c.scratch.0.join("system"))
            .env("LD_PRELOAD", library)
            .env("ZIPELO_SPOOL_PHASE", "tiny-tmpfs")
            .env("ZIPELO_SPOOL_TRACE", c.scratch.0.join("trace"))
            .output()
            .unwrap();
        assert!(probe.status.success(), "{:?}", probe.stderr);
        assert!(
            fs::read_to_string(c.scratch.0.join("trace"))
                .unwrap()
                .contains('F')
        );
        for format in ["zip", "7z"] {
            c.assert_preview(format, "tiny-tmpfs", 0, named);
        }
        assert!(
            Command::new("python3")
                .arg("-B")
                .arg(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../tests/rust-archive-worker/spool_fallback_cases.py")
                )
                .arg(&c.scratch.0)
                .arg("non-solid")
                .status()
                .unwrap()
                .success()
        );
        for format in ["zip", "7z"] {
            c.assert_preview(format, "tiny-tmpfs", 3, named);
        }
        let trace = fs::read_to_string(c.scratch.0.join("trace")).unwrap();
        assert!(trace.contains('2'), "{trace}");
        assert_eq!(fs::read_dir(c.scratch.0.join("system")).unwrap().count(), 0);
        c.clean();
    }
}
