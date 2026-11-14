use super::*;
use std::{io::Read, process::Child, sync::mpsc, time::Duration};

fn private_dir(path: &std::path::Path) {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).unwrap();
}
struct Case(PathBuf);
impl Case {
    fn new() -> Self {
        let root = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/work/tmp")
            });
        fs::create_dir_all(&root).unwrap();
        let path = root.canonicalize().unwrap().join(format!(
            "zipelo-7zc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        private_dir(&path);
        private_dir(&path.join("tmp"));
        Self(path)
    }
    fn path(&self, name: &str) -> NativePath {
        NativePath::from_path(&self.0.join(name))
    }
    fn request(&self, names: &[&str]) -> Start {
        Start {
            action: Action::CreateSevenZip {
                sources: names
                    .iter()
                    .map(|name| Source {
                        path: self.path(name),
                        archive_name: (*name).to_owned(),
                    })
                    .collect(),
                output_file: self.path("result.7z"),
                method: SevenZipMethod::Lzma2,
                solid: false,
                encryption: Encryption::None,
            },
            limits: Limits::default(),
        }
    }
    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zipelo-archive-worker"));
        cmd.env("TMPDIR", self.0.join("tmp"))
            .env("TMP", self.0.join("tmp"))
            .env("TEMP", self.0.join("tmp"));
        cmd
    }
    fn run(&self, message: Message) -> Vec<Message> {
        self.run_with(&mut self.command(), message, |_| Vec::new())
    }
    fn run_with(
        &self,
        command: &mut Command,
        message: Message,
        mut respond: impl FnMut(&Message) -> Vec<Message>,
    ) -> Vec<Message> {
        struct Running(Child);
        impl Drop for Running {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
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
        let mut writer = Writer::new(child.0.stdin.take().unwrap(), 17);
        writer.send(&message).unwrap();
        let stdout = child.0.stdout.take().unwrap();
        let mut stderr = child.0.stderr.take().unwrap();
        let diagnostic = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).unwrap();
            bytes
        });
        let (send, receive) = mpsc::sync_channel(128);
        let reader = std::thread::spawn(move || {
            let mut reader = Reader::new(stdout);
            loop {
                let frame = reader.read();
                let end = !matches!(frame, Ok(Some(_)));
                if send.send(frame).is_err() || end {
                    break;
                }
            }
        });
        let mut events = Vec::new();
        while let Some((job, _, event)) = receive
            .recv_timeout(Duration::from_secs(120))
            .expect("worker timeout")
            .unwrap()
        {
            assert_eq!(job, 17);
            for reply in respond(&event) {
                writer.send(&reply).unwrap();
            }
            events.push(event);
        }
        drop(writer);
        reader.join().unwrap();
        assert!(child.0.wait().unwrap().success());
        assert!(diagnostic.join().unwrap().is_empty());
        assert!(matches!(events.last(), Some(Message::Terminal(_))));
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Message::Terminal(_)))
                .count(),
            1
        );
        assert_eq!(fs::read_dir(self.0.join("tmp")).unwrap().count(), 0);
        events
    }
    fn list(&self) -> Vec<Message> {
        self.run(Message::Start(Start {
            action: Action::List {
                archive: self.path("result.7z"),
            },
            limits: Limits::default(),
        }))
    }
    fn extract(&self, destination: &str) -> Vec<Message> {
        private_dir(&self.0.join(destination));
        self.run(Message::Start(Start {
            action: Action::Extract {
                archive: self.path("result.7z"),
                destination: self.path(destination),
                selection: Selection::All,
                list_id: None,
                gzip_output_name: None,
            },
            limits: Limits::default(),
        }))
    }
}
impl Drop for Case {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("7z 작성 시험 폴더 정리에 실패함");
    }
}
fn success(events: &[Message]) {
    let t = terminal(events);
    assert_eq!(t.outcome, Outcome::Completed, "{events:?}");
    assert_eq!(t.error, ErrorCode::None);
    assert!(events.iter().any(|e| matches!(
        e,
        Message::Progress {
            stage: Stage::Commit,
            ..
        }
    )));
    assert_eq!(t.publication, Publication::Complete);
    assert_eq!(t.created_count, 1);
    assert_eq!(t.retained_partial_count, 0);
    assert_eq!(t.cleanup, Cleanup::Complete);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Message::Output { .. }))
            .count(),
        1
    );
    assert!(events.iter().any(|e| matches!(e, Message::Output { entry_id: None, relative_name, disposition: Disposition::Created } if relative_name == "result.7z")));
    assert!(!events.iter().any(|e| matches!(
        e,
        Message::PasswordRequired { .. } | Message::ConflictRequired { .. }
    )));
}
fn failure(events: &[Message], error: ErrorCode) {
    let t = terminal(events);
    assert_eq!(t.error, error, "{events:?}");
    assert_eq!(
        t.outcome,
        if error == ErrorCode::None {
            Outcome::Cancelled
        } else {
            Outcome::Failed
        }
    );
    assert_eq!(t.publication, Publication::None);
    assert_eq!(t.created_count, 0);
    assert_eq!(t.retained_partial_count, 0);
    assert_eq!(t.cleanup, Cleanup::Complete);
    assert!(!events.iter().any(|e| matches!(e, Message::Output { .. })));
}
fn tree(root: &std::path::Path) -> std::collections::BTreeMap<String, Option<[u8; 32]>> {
    use sha2::{Digest, Sha256};
    fn walk(
        root: &std::path::Path,
        at: &std::path::Path,
        result: &mut std::collections::BTreeMap<String, Option<[u8; 32]>>,
    ) {
        for entry in fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            if path.is_dir() {
                result.insert(name, None);
                walk(root, &path, result);
            } else {
                result.insert(name, Some(Sha256::digest(fs::read(path).unwrap()).into()));
            }
        }
    }
    let mut result = std::collections::BTreeMap::new();
    walk(root, root, &mut result);
    result
}

#[test]
fn saving_outside_inside_and_nested_in_source_preserves_the_original_tree() {
    for parent in ["", "source", "source/nested"] {
        let c = Case::new();
        private_dir(&c.0.join("source"));
        private_dir(&c.0.join("source/nested"));
        private_dir(&c.0.join("source/nested/empty"));
        fs::write(c.0.join("source/한글.txt"), b"original").unwrap();
        fs::write(c.0.join("source/nested/empty.txt"), b"").unwrap();
        let expected = tree(&c.0.join("source"));
        let archive = NativePath::from_path(&c.0.join(parent).join("result.7z"));
        let mut request = c.request(&["source"]);
        if let Action::CreateSevenZip { output_file, .. } = &mut request.action {
            *output_file = archive.clone();
        }
        success(&c.run(Message::Start(request)));
        let listed = c.run(Message::Start(Start {
            action: Action::List {
                archive: archive.clone(),
            },
            limits: Limits::default(),
        }));
        assert_eq!(terminal(&listed).outcome, Outcome::Completed, "{listed:?}");
        private_dir(&c.0.join("decoded"));
        let extracted = c.run(Message::Start(Start {
            action: Action::Extract {
                archive: archive.clone(),
                destination: c.path("decoded"),
                selection: Selection::All,
                list_id: None,
                gzip_output_name: None,
            },
            limits: Limits::default(),
        }));
        assert_eq!(
            terminal(&extracted).outcome,
            Outcome::Completed,
            "{extracted:?}"
        );
        assert_eq!(tree(&c.0.join("decoded/source")), expected);
        if let Some(tool) = std::env::var_os("ZIPELO_TEST_7ZIP") {
            let output = Command::new(tool)
                .args(["x", "-y"])
                .arg(archive.to_path().unwrap())
                .arg(format!("-o{}", c.0.join("independent").display()))
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            assert_eq!(tree(&c.0.join("independent/source")), expected);
        }
        assert!(
            !tree(&c.0.join("source"))
                .keys()
                .any(|name| name.contains(".zipelo-"))
        );
    }
}

#[test]
fn mixed_unicode_files_and_empty_bitmap_boundaries_round_trip() {
    for count in [7, 8, 9] {
        let c = Case::new();
        fs::create_dir_all(c.0.join("원본/폴더/빈 폴더")).unwrap();
        fs::write(c.0.join("원본/가.txt"), "한글 자료\n".repeat(1000)).unwrap();
        fs::write(
            c.0.join("원본/폴더/z"),
            (0..131_081)
                .map(|n| ((n * 73) % 251) as u8)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        for index in 0..count - 3 {
            fs::write(c.0.join(format!("원본/빈-{index}")), b"").unwrap();
        }
        fs::write(c.0.join("원본/\u{20000}.txt"), b"UTF-16 surrogate pair").unwrap();
        let mut state = 0x1234_5678u32;
        let noise: Vec<u8> = (0..131_081)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        fs::write(c.0.join("원본/noise.bin"), noise).unwrap();
        let expected = tree(&c.0.join("원본"));
        let events = c.run(Message::Start(c.request(&["원본"])));
        success(&events);
        assert!(matches!(
            events[0],
            Message::Accepted {
                action: ActionKind::CreateSevenZip,
                encryption: false
            }
        ));
        let list = c.list();
        assert_eq!(terminal(&list).outcome, Outcome::Completed, "{list:?}");
        let extracted = c.extract("decoded");
        assert_eq!(
            terminal(&extracted).outcome,
            Outcome::Completed,
            "{extracted:?}"
        );
        assert_eq!(tree(&c.0.join("decoded/원본")), expected);
        if let Some(tool) = std::env::var_os("ZIPELO_TEST_7ZIP") {
            for operation in ["t", "l"] {
                let output = Command::new(&tool)
                    .args([operation, "-slt"])
                    .arg(c.0.join("result.7z"))
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stdout)
                );
                if operation == "l" {
                    let text = String::from_utf8_lossy(&output.stdout);
                    assert!(text.contains("Method = LZMA2:20"), "{text}");
                    assert!(text.contains("Solid = -"), "{text}");
                    assert!(!text.contains("Encrypted = +"));
                }
            }
            let output = Command::new(tool)
                .args(["x", "-y"])
                .arg(c.0.join("result.7z"))
                .arg(format!("-o{}", c.0.join("independent").display()))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            assert_eq!(tree(&c.0.join("independent/원본")), expected);
        }
    }
}

#[test]
fn archives_with_only_empty_files_and_directories_are_valid() {
    for directory in [false, true] {
        let c = Case::new();
        if directory {
            fs::create_dir_all(c.0.join("empty/nested")).unwrap();
        } else {
            fs::write(c.0.join("empty"), b"").unwrap();
        }
        success(&c.run(Message::Start(c.request(&["empty"]))));
        let extracted = c.extract("decoded");
        assert_eq!(
            terminal(&extracted).outcome,
            Outcome::Completed,
            "{extracted:?}"
        );
        assert_eq!(c.0.join("decoded/empty").is_dir(), directory);
        if let Some(tool) = std::env::var_os("ZIPELO_TEST_7ZIP") {
            assert!(
                Command::new(tool)
                    .arg("t")
                    .arg(c.0.join("result.7z"))
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
    }
}

#[test]
fn existing_output_empty_sources_limits_and_links_are_rejected() {
    let c = Case::new();
    fs::write(c.0.join("source"), b"abcdef").unwrap();
    fs::write(c.0.join("result.7z"), b"preserve").unwrap();
    failure(
        &c.run(Message::Start(c.request(&["source"]))),
        ErrorCode::Conflict,
    );
    assert_eq!(fs::read(c.0.join("result.7z")).unwrap(), b"preserve");
    fs::remove_file(c.0.join("result.7z")).unwrap();
    failure(
        &c.run(Message::Start(c.request(&[]))),
        ErrorCode::InvalidRequest,
    );
    for bytes in [5, 6, 7] {
        let mut start = c.request(&["source"]);
        start.limits.input = bytes;
        start.limits.output = bytes;
        let events = c.run(Message::Start(start));
        if bytes == 5 {
            failure(&events, ErrorCode::Limit);
        } else {
            success(&events);
            fs::remove_file(c.0.join("result.7z")).unwrap();
        }
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(c.0.join("source"), c.0.join("link")).unwrap();
        failure(
            &c.run(Message::Start(c.request(&["link"]))),
            ErrorCode::UnsafePath,
        );
    }
    assert!(!c.0.join("result.7z").exists());
}

#[test]
fn guarded_approval_decline_and_cancel_preserve_creation_rules() {
    let c = Case::new();
    fs::write(c.0.join("source"), b"abcdef").unwrap();
    for allow in [false, true] {
        let mut start = c.request(&["source"]);
        start.limits.output = 5;
        let message = Message::StartGuarded(GuardedStart {
            ceiling: Limits {
                output: 6,
                ..start.limits
            },
            start,
            interactive: false,
        });
        let mut questions = 0;
        let events = c.run_with(&mut c.command(), message, |event| {
            if let Message::ResourceRequired {
                challenge_id,
                action,
                scope,
                ..
            } = event
            {
                assert_eq!(*action, ActionKind::CreateSevenZip);
                questions += 1;
                vec![Message::ResourceReply {
                    challenge_id: *challenge_id,
                    scope: *scope,
                    allow,
                }]
            } else {
                Vec::new()
            }
        });
        assert_eq!(questions, 1);
        if allow {
            success(&events);
            fs::remove_file(c.0.join("result.7z")).unwrap();
        } else {
            failure(&events, ErrorCode::None);
            assert!(!c.0.join("result.7z").exists());
        }
    }
}

#[test]
fn cancel_during_snapshot_or_encoding_publishes_nothing() {
    for stage in [Stage::Read, Stage::Write] {
        let c = Case::new();
        fs::File::create(c.0.join("source"))
            .unwrap()
            .set_len(128 * 1024 * 1024)
            .unwrap();
        let mut sent = false;
        let events = c.run_with(
            &mut c.command(),
            Message::Start(c.request(&["source"])),
            |event| {
                if !sent
                    && matches!(event, Message::Progress { stage: actual, processed_bytes, .. } if *actual == stage && (stage == Stage::Read || *processed_bytes > 0))
                {
                    sent = true;
                    vec![Message::Cancel]
                } else {
                    Vec::new()
                }
            },
        );
        assert!(sent);
        failure(&events, ErrorCode::None);
        assert!(!c.0.join("result.7z").exists());
        assert!(events.iter().any(|e| matches!(
            e,
            Message::CancelAck {
                status: CancelStatus::Requested,
                ..
            }
        )));
    }
}

#[cfg(target_os = "linux")]
fn shim(c: &Case, file: &str) -> PathBuf {
    let output = c.0.join("fault.so");
    let status = Command::new("cc")
        .args(["-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror"])
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/rust-archive-worker")
                .join(file),
        )
        .args(["-ldl", "-o"])
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    output
}

#[cfg(target_os = "linux")]
#[test]
fn storage_failures_keep_real_publication_and_cleanup_state() {
    for fault in [
        "temp-full",
        "space-stage",
        "create-full",
        "create-denied",
        "full",
        "write-io",
        "sync-full",
        "dir-sync-full",
    ] {
        let c = Case::new();
        fs::write(c.0.join("source"), b"a".repeat(100_000)).unwrap();
        let mut command = c.command();
        command
            .env("LD_PRELOAD", shim(&c, "io_fault.c"))
            .env("ZIPELO_TEST_FAULT", fault)
            .env("ZIPELO_TEST_MARKER", c.0.join("marker"));
        let events = c.run_with(&mut command, Message::Start(c.request(&["source"])), |_| {
            Vec::new()
        });
        let error = if matches!(fault, "create-denied" | "write-io") {
            ErrorCode::Io
        } else {
            ErrorCode::DiskFull
        };
        if fault == "dir-sync-full" {
            let t = terminal(&events);
            assert_eq!(t.error, error, "{events:?}");
            assert_eq!(t.outcome, Outcome::Failed);
            assert_eq!(t.created_count, 1);
            assert_eq!(t.durability, Durability::Failed);
            assert!(c.0.join("result.7z").is_file());
        } else {
            failure(&events, error);
            assert!(!c.0.join("result.7z").exists());
        }
        assert!(!fs::read_dir(&c.0).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".zipelo-")
        }));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn source_changes_new_destination_and_cancel_before_publication_are_preserved() {
    for operation in ["source", "destination", "cancel"] {
        let c = Case::new();
        fs::write(c.0.join("source"), b"a".repeat(100_000)).unwrap();
        let marker = c.0.join("marker");
        let release = c.0.join("release");
        let mut command = c.command();
        command
            .env("LD_PRELOAD", shim(&c, "io_fault.c"))
            .env("ZIPELO_TEST_FAULT", "pause")
            .env("ZIPELO_TEST_MARKER", &marker)
            .env("ZIPELO_TEST_RELEASE", &release);
        let mut changed = false;
        let events = c.run_with(
            &mut command,
            Message::Start(c.request(&["source"])),
            |event| {
                if !changed
                    && matches!(
                        event,
                        Message::Progress {
                            stage: Stage::Write,
                            total_bytes: Some(_),
                            ..
                        }
                    )
                {
                    let until = std::time::Instant::now() + Duration::from_secs(5);
                    while !marker.exists() {
                        assert!(std::time::Instant::now() < until);
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    match operation {
                        "source" => fs::write(c.0.join("source"), b"changed").unwrap(),
                        "destination" => fs::write(c.0.join("result.7z"), b"user data").unwrap(),
                        _ => {}
                    }
                    changed = true;
                    let replies = if operation == "cancel" {
                        vec![Message::Cancel]
                    } else {
                        Vec::new()
                    };
                    fs::write(&release, b"resume").unwrap();
                    replies
                } else {
                    Vec::new()
                }
            },
        );
        assert!(changed);
        failure(
            &events,
            match operation {
                "source" => ErrorCode::SourceChanged,
                "destination" => ErrorCode::Conflict,
                _ => ErrorCode::None,
            },
        );
        if operation == "destination" {
            assert_eq!(fs::read(c.0.join("result.7z")).unwrap(), b"user data");
        } else {
            assert!(!c.0.join("result.7z").exists());
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn internal_destination_still_rejects_real_source_changes_and_cancellation() {
    for parent in ["source", "source/nested"] {
        for operation in [
            "add",
            "foreign-temp",
            "remove",
            "rename",
            "modify",
            "replace-dir",
            "cancel",
        ] {
            let c = Case::new();
            private_dir(&c.0.join("source"));
            private_dir(&c.0.join("source/nested"));
            let directory = c.0.join(parent);
            private_dir(&directory.join("empty"));
            fs::write(directory.join("file"), b"a".repeat(100_000)).unwrap();
            let mut request = c.request(&["source"]);
            if let Action::CreateSevenZip { output_file, .. } = &mut request.action {
                *output_file = NativePath::from_path(&directory.join("result.7z"));
            }
            let marker = c.0.join("marker");
            let release = c.0.join("release");
            let mut command = c.command();
            command
                .env("LD_PRELOAD", shim(&c, "io_fault.c"))
                .env("ZIPELO_TEST_FAULT", "pause")
                .env("ZIPELO_TEST_MARKER", &marker)
                .env("ZIPELO_TEST_RELEASE", &release);
            let mut changed = false;
            let events = c.run_with(&mut command, Message::Start(request), |event| {
                if changed
                    || !matches!(
                        event,
                        Message::Progress {
                            stage: Stage::Write,
                            total_bytes: Some(_),
                            ..
                        }
                    )
                {
                    return Vec::new();
                }
                let until = std::time::Instant::now() + Duration::from_secs(5);
                while !marker.exists() {
                    assert!(std::time::Instant::now() < until);
                    std::thread::sleep(Duration::from_millis(2));
                }
                match operation {
                    "add" => fs::write(directory.join("new-file"), b"user addition").unwrap(),
                    "foreign-temp" => {
                        fs::write(directory.join(".zipelo-user.tmp"), b"user file").unwrap()
                    }
                    "remove" => fs::remove_file(directory.join("file")).unwrap(),
                    "rename" => {
                        fs::rename(directory.join("file"), directory.join("renamed")).unwrap()
                    }
                    "modify" => fs::write(directory.join("file"), b"user modification").unwrap(),
                    "replace-dir" => {
                        fs::rename(directory.join("empty"), c.0.join("old-empty")).unwrap();
                        private_dir(&directory.join("empty"));
                    }
                    _ => {}
                }
                changed = true;
                fs::write(&release, b"resume").unwrap();
                if operation == "cancel" {
                    vec![Message::Cancel]
                } else {
                    Vec::new()
                }
            });
            assert!(changed);
            failure(
                &events,
                if operation == "cancel" {
                    ErrorCode::None
                } else {
                    ErrorCode::SourceChanged
                },
            );
            assert!(!directory.join("result.7z").exists());
            let expected_temp = if operation == "foreign-temp" { 1 } else { 0 };
            assert_eq!(
                fs::read_dir(&directory)
                    .unwrap()
                    .filter(|entry| {
                        entry
                            .as_ref()
                            .unwrap()
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".zipelo-")
                    })
                    .count(),
                expected_temp
            );
            if operation == "foreign-temp" {
                assert_eq!(
                    fs::read(directory.join(".zipelo-user.tmp")).unwrap(),
                    b"user file"
                );
            }
            if operation == "modify" {
                assert_eq!(
                    fs::read(directory.join("file")).unwrap(),
                    b"user modification"
                );
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn internal_destination_write_failures_clean_only_the_owned_temporary() {
    for fault in ["full", "sync-full"] {
        let c = Case::new();
        private_dir(&c.0.join("source"));
        fs::write(c.0.join("source/file"), b"original").unwrap();
        fs::write(c.0.join("source/.zipelo-user.tmp"), b"keep").unwrap();
        let expected = tree(&c.0.join("source"));
        let mut request = c.request(&["source"]);
        if let Action::CreateSevenZip { output_file, .. } = &mut request.action {
            *output_file = c.path("source/result.7z");
        }
        let mut command = c.command();
        command
            .env("LD_PRELOAD", shim(&c, "io_fault.c"))
            .env("ZIPELO_TEST_FAULT", fault);
        failure(
            &c.run_with(&mut command, Message::Start(request), |_| Vec::new()),
            ErrorCode::DiskFull,
        );
        assert_eq!(tree(&c.0.join("source")), expected);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn encoder_finish_header_write_seek_and_patch_errors_publish_nothing() {
    for fault in [
        "encode-full",
        "finish-full",
        "header-write",
        "header-seek",
        "header-patch",
    ] {
        let c = Case::new();
        fs::write(
            c.0.join("source"),
            b"x".repeat(if fault == "encode-full" {
                3 * 1024 * 1024
            } else {
                123
            }),
        )
        .unwrap();
        let mut command = c.command();
        command
            .env("LD_PRELOAD", shim(&c, "seven_zip_create_fault.c"))
            .env("ZIPELO_7Z_FAULT", fault);
        let events = c.run_with(&mut command, Message::Start(c.request(&["source"])), |_| {
            Vec::new()
        });
        failure(
            &events,
            if fault.ends_with("full") {
                ErrorCode::DiskFull
            } else {
                ErrorCode::Io
            },
        );
        assert!(!c.0.join("result.7z").exists());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn gigabyte_creation_stays_below_64_mib_peak_rss() {
    assert!(
        Command::new("python3")
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/rust-archive-worker/seven_zip_create_memory.py")
            )
            .arg(env!("CARGO_BIN_EXE_zipelo-archive-worker"))
            .status()
            .unwrap()
            .success()
    );
}
