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
        Self { scratch }
    }
    fn run(&self, action: Action, phase: &str, mask: u8, named: bool) -> Vec<Message> {
        let s = &self.scratch;
        let mut command = s.worker();
        command
            .env("XDG_CACHE_HOME", s.0.join("cache"))
            .env(
                "TMPDIR",
                if phase == "real" {
                    PathBuf::from(std::env::var_os("ZIPELO_SPOOL_REAL_TMP").unwrap())
                } else {
                    s.0.join("system")
                },
            )
            .env("HOME", &s.0);
        if phase == "home" {
            command.env_remove("XDG_CACHE_HOME");
        }
        if phase == "changed" {
            command.env(
                "ZIPELO_SPOOL_SOURCE",
                self.archive("zip").to_path().unwrap(),
            );
        }
        if phase != "real" {
            let library = s.0.join("location.so");
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
            command
                .env("LD_PRELOAD", library)
                .env("ZIPELO_SPOOL_PHASE", phase)
                .env("ZIPELO_SPOOL_MASK", mask.to_string())
                .env("ZIPELO_SPOOL_TRACE", s.0.join("trace"));
            if named {
                command.env("ZIPELO_SPOOL_NAMED", "1");
            }
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
        assert_eq!(fs::read_dir(root.join("archive")).unwrap().count(), 2);
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
fn disk_full_requires_exhausting_all_three_locations_and_io_errors_stay_io() {
    for phase in ["create", "space", "write"] {
        let c = Case::new();
        for format in ["zip", "7z"] {
            let events = c.list(format, phase, 7, false);
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
        let events = c.list("zip", phase, 7, false);
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
fn source_change_during_failed_copy_prevents_retry_success() {
    let c = Case::new();
    let events = c.list("zip", "changed", 1, false);
    assert_eq!(terminal(&events).outcome, Outcome::Failed, "{events:?}");
    assert_eq!(
        terminal(&events).error,
        ErrorCode::SourceChanged,
        "{events:?}"
    );
    let trace = fs::read_to_string(c.scratch.0.join("trace")).unwrap();
    assert!(
        trace.contains('0') && !trace.contains('1') && !trace.contains('2'),
        "{trace}"
    );
}

#[test]
#[ignore = "별도 VM의 작은 tmpfs 경로를 ZIPELO_SPOOL_REAL_TMP로 지정해야 함"]
fn actual_tmpfs_quota_lists_and_previews_large_zip_and_seven_zip() {
    let path = PathBuf::from(std::env::var_os("ZIPELO_SPOOL_REAL_TMP").unwrap());
    let filesystem = Command::new("stat")
        .args(["-f", "-c", "%T"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(filesystem.stdout).unwrap().trim(),
        "tmpfs"
    );
    let capacity = Command::new("stat")
        .args(["-f", "-c", "%S %b"])
        .arg(&path)
        .output()
        .unwrap();
    let fields: Vec<u64> = String::from_utf8(capacity.stdout)
        .unwrap()
        .split_whitespace()
        .map(|value| value.parse().unwrap())
        .collect();
    let bytes = fields[0] * fields[1];
    assert!(bytes > 0 && bytes < 16 * 1024 * 1024, "{bytes}");
    let probe = path.join("capacity-probe");
    let error = fs::write(&probe, vec![0; bytes as usize + 4096]).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::StorageFull);
    fs::remove_file(probe).unwrap();
    let c = Case::new();
    for format in ["zip", "7z"] {
        c.assert_preview(format, "real", 0, false);
    }
    assert_eq!(fs::read_dir(path).unwrap().count(), 0);
    assert_eq!(
        fs::metadata(c.scratch.0.join("cache/zipelo"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}
