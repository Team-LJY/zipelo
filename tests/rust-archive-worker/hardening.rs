use super::*;
use std::{fs::File, io::Read};

fn generate(s: &Scratch, kind: &str, size: u64) {
    assert!(
        Command::new("python3")
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/rust-archive-worker/hardening_cases.py")
            )
            .arg(&s.0)
            .arg(kind)
            .arg(size.to_string())
            .status()
            .unwrap()
            .success()
    );
}
fn start(s: &Scratch, gzip: bool, output: u64) -> Start {
    fs::DirBuilder::new().create(s.0.join("output")).unwrap();
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
            gzip_output_name: gzip.then(|| "payload.bin".to_owned()),
        },
        limits: Limits {
            output,
            ..Limits::default()
        },
    }
}
fn assert_zero_file(path: &std::path::Path, length: u64) {
    let mut file = File::open(path).unwrap();
    assert_eq!(file.metadata().unwrap().len(), length);
    let mut buffer = [0; 65536];
    let mut total = 0;
    loop {
        let n = file.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        assert!(buffer[..n].iter().all(|b| *b == 0));
        total += n as u64;
    }
    assert_eq!(total, length);
}
#[test]
fn streaming_defaults_and_legacy_seven_zip_caps() {
    assert_eq!(Limits::default().input, u64::MAX);
    assert_eq!(Limits::default().output, 16 * 1024 * 1024 * 1024);
    assert!(
        !Limits {
            output: 16 * 1024 * 1024 * 1024 + 1,
            ..Limits::default()
        }
        .valid()
    );
    let s = Scratch::new();
    let file = File::create(s.0.join("input")).unwrap();
    (&file).write_all(b"7z\xbc\xaf\x27\x1c").unwrap();
    file.set_len(1024 * 1024 * 1024).unwrap();
    let events = execute(start(&s, false, Limits::default().output));
    assert_eq!(terminal(&events).error, ErrorCode::Limit);
    assert_eq!(terminal(&events).created_count, 0);
}
#[test]
fn streaming_zip64_local_headers_and_chunk_boundaries() {
    let s = Scratch::new();
    generate(&s, "zip64", 9 * 1024 * 1024 + 13);
    let events = execute(start(&s, false, Limits::default().output));
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_zero_file(&s.0.join("output/payload.bin"), 9 * 1024 * 1024 + 13);
}
#[test]
fn streaming_ratio_rejects_bomb_without_publication() {
    let s = Scratch::new();
    generate(&s, "bomb", 16 * 1024 * 1024);
    let events = execute(start(&s, false, Limits::default().output));
    assert_eq!(terminal(&events).error, ErrorCode::Limit, "{events:?}");
    assert_eq!(terminal(&events).publication, Publication::None);
    assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
}
#[test]
fn streaming_tar_and_gzip_truncation_never_publish() {
    for kind in ["tar", "targz", "gzip"] {
        let s = Scratch::new();
        generate(&s, kind, 1024 * 1024 + 1);
        let file = fs::OpenOptions::new()
            .write(true)
            .open(s.0.join("input"))
            .unwrap();
        file.set_len(file.metadata().unwrap().len() - 1).unwrap();
        let events = execute(start(&s, kind == "gzip", Limits::default().output));
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Failed,
            "{kind}: {events:?}"
        );
        assert_eq!(terminal(&events).created_count, 0);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
    }
}

#[test]
fn zip64_directory_and_descriptor_interoperate() {
    for kind in ["central64", "descriptor64"] {
        let s = Scratch::new();
        generate(&s, kind, 65536 + 19);
        let events = execute(start(&s, false, Limits::default().output));
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Completed,
            "{kind}: {events:?}"
        );
        assert_zero_file(&s.0.join("output/payload.bin"), 65536 + 19);
    }
}
#[test]
fn zip64_offset_only_keeps_32_bit_descriptors() {
    for kind in ["offset64-descriptor-signed", "offset64-descriptor-unsigned"] {
        let s = Scratch::new();
        generate(&s, kind, 65536 + 19);
        let events = execute(Start {
            action: Action::List {
                archive: s.path("input"),
            },
            limits: Limits::default(),
        });
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Completed,
            "{kind}: {events:?}"
        );
        let events = execute(start(&s, false, Limits::default().output));
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Completed,
            "{kind}: {events:?}"
        );
        assert_zero_file(&s.0.join("output/payload.bin"), 65536 + 19);
    }
}
#[test]
fn zip64_locator_with_ordinary_eocd_fields_interoperates() {
    let s = Scratch::new();
    generate(&s, "locator64", 20);
    assert_eq!(fs::metadata(s.0.join("input")).unwrap().len(), 260);
    let events = execute(Start {
        action: Action::List {
            archive: s.path("input"),
        },
        limits: Limits::default(),
    });
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    let events = execute(start(&s, false, Limits::default().output));
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_zero_file(&s.0.join("output/payload.bin"), 20);
}
#[test]
fn zip64_locator_corruption_never_publishes() {
    for (kind, error) in [
        ("locator64-bad-offset", ErrorCode::Integrity),
        ("locator64-bad-size", ErrorCode::Integrity),
        ("locator64-conflicting-count", ErrorCode::Integrity),
        ("locator64-multidisk", ErrorCode::UnsupportedFeature),
    ] {
        let s = Scratch::new();
        generate(&s, kind, 20);
        let events = execute(start(&s, false, Limits::default().output));
        assert_eq!(terminal(&events).error, error, "{kind}: {events:?}");
        assert_eq!(terminal(&events).publication, Publication::None);
        assert_eq!(terminal(&events).cleanup, Cleanup::Complete);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
    }
}
#[test]
fn classic_directory_comment_is_not_a_zip64_locator() {
    let s = Scratch::new();
    generate(&s, "classic-locator-decoy", 20);
    let events = execute(start(&s, false, Limits::default().output));
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_zero_file(&s.0.join("output/payload.bin"), 20);
}
#[test]
fn untrusted_zip64_u64_sizes_and_offsets_never_allocate_or_publish() {
    for (kind, error) in [
        ("central64-size-overflow", ErrorCode::Limit),
        ("central64-offset-overflow", ErrorCode::Integrity),
    ] {
        let s = Scratch::new();
        generate(&s, kind, 1024);
        let events = execute(start(&s, false, Limits::default().output));
        assert_eq!(terminal(&events).error, error, "{kind}: {events:?}");
        assert_eq!(terminal(&events).publication, Publication::None);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
    }
}
#[test]
fn gzip_ratio_is_per_member_even_with_low_ratio_suffix() {
    let s = Scratch::new();
    generate(&s, "gzip-bomb-joined", 16 * 1024 * 1024);
    let events = execute(start(&s, true, Limits::default().output));
    assert_eq!(terminal(&events).error, ErrorCode::Limit, "{events:?}");
    assert_eq!(terminal(&events).created_count, 0);
    assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
}
#[test]
fn streamed_aes_counter_survives_many_read_chunks() {
    let s = Scratch::new();
    generate(&s, "store", 2 * 1024 * 1024 + 17);
    fs::rename(s.0.join("input"), s.0.join("source")).unwrap();
    let events = interactive(
        Start {
            action: Action::CreateZip {
                sources: vec![Source {
                    path: s.path("source"),
                    archive_name: "payload.bin".to_owned(),
                }],
                output_file: s.path("input"),
                method: Method::Store,
                encryption: Encryption::Aes256,
            },
            limits: Limits::default(),
        },
        |id, _, _| password_reply(id, TEST_PASSWORD),
    );
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    let events = interactive(start(&s, false, Limits::default().output), |id, _, _| {
        password_reply(id, TEST_PASSWORD)
    });
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    let mut source = File::open(s.0.join("source")).unwrap();
    let mut output = File::open(s.0.join("output/payload.bin")).unwrap();
    let mut a = [0; 65536];
    let mut b = [0; 65536];
    loop {
        let n = source.read(&mut a).unwrap();
        output.read_exact(&mut b[..n]).unwrap();
        assert_eq!(a[..n], b[..n]);
        if n == 0 {
            break;
        }
    }
    assert_eq!(
        source.metadata().unwrap().len(),
        output.metadata().unwrap().len()
    );
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        os::raw::c_long,
        time::{Duration, Instant},
    };
    #[repr(C)]
    #[derive(Default)]
    struct Usage {
        user: [c_long; 2],
        system: [c_long; 2],
        max_rss: c_long,
        rest: [c_long; 13],
    }
    unsafe extern "C" {
        fn wait4(pid: i32, status: *mut i32, options: i32, usage: *mut Usage) -> i32;
        fn prctl(option: i32, ...) -> i32;
        fn kill(pid: i32, signal: i32) -> i32;
    }
    fn command() -> Command {
        let worker = std::env::var_os("ZIPELO_BASELINE_WORKER")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_zipelo-archive-worker")));
        Command::new(worker)
    }
    fn staging(s: &Scratch) -> PathBuf {
        use std::os::unix::fs::DirBuilderExt;
        let path = s.0.join("staging");
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        path
    }
    fn spawn(s: &Scratch, mut start: Start, mode: Option<&str>) -> std::process::Child {
        if std::env::var_os("ZIPELO_BASELINE_WORKER").is_some() {
            start.limits = Limits {
                input: 64 * 1024 * 1024,
                output: 128 * 1024 * 1024,
                entries: 10000,
            };
        }
        let mut cmd = command();
        cmd.env("TMPDIR", staging(s))
            .stdin(Stdio::piped())
            .stdout(File::create(s.0.join("events")).unwrap())
            .stderr(File::create(s.0.join("stderr")).unwrap());
        if let Some(mode) = mode {
            cmd.env("LD_PRELOAD", shim(s))
                .env("ZIPELO_TEST_FAULT", mode)
                .env("ZIPELO_TEST_MARKER", s.0.join("paused"))
                .env("ZIPELO_TEST_RELEASE", s.0.join("release"));
        }
        let mut child = cmd.spawn().unwrap();
        Writer::new(child.stdin.take().unwrap(), 17)
            .send(&Message::Start(start))
            .unwrap();
        child
    }
    fn wait(pid: u32) -> (i32, u64) {
        let deadline = Instant::now() + Duration::from_secs(320);
        loop {
            let mut status = 0;
            let mut usage = Usage::default();
            let r = unsafe { wait4(pid as i32, &mut status, 1, &mut usage) };
            if r == pid as i32 {
                return (status, usage.max_rss as u64 * 1024);
            }
            assert!(
                r >= 0 || std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            );
            if Instant::now() > deadline {
                unsafe {
                    kill(pid as i32, 9);
                    wait4(pid as i32, &mut status, 0, &mut usage);
                }
                panic!("worker timeout");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn events(s: &Scratch) -> Vec<Message> {
        assert!(fs::read(s.0.join("stderr")).unwrap().is_empty());
        let mut reader = Reader::new(File::open(s.0.join("events")).unwrap());
        let mut result = Vec::new();
        while let Some((_, _, message)) = reader.read().unwrap() {
            result.push(message);
        }
        result
    }
    fn shim(s: &Scratch) -> PathBuf {
        let path = s.0.join("io_fault.so");
        assert!(
            Command::new("cc")
                .args(["-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror"])
                .arg(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../tests/rust-archive-worker/io_fault.c")
                )
                .arg("-ldl")
                .arg("-o")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        path
    }
    #[test]
    fn gigabyte_formats_stay_below_64_mib_peak_rss() {
        let mut measurements = Vec::new();
        for kind in ["store", "deflate", "tar", "gzip", "targz"] {
            let s = Scratch::new();
            generate(&s, kind, 1024 * 1024 * 1024);
            let request = start(&s, kind == "gzip", Limits::default().output);
            let child = spawn(&s, request, None);
            let (status, rss) = wait(child.id());
            assert_eq!(status, 0, "{kind}");
            let events = events(&s);
            assert_eq!(
                terminal(&events).outcome,
                Outcome::Completed,
                "{kind}: {events:?}"
            );
            assert_eq!(terminal(&events).publication, Publication::Complete);
            assert!(rss <= 64 * 1024 * 1024, "{kind}: RSS {rss}");
            assert_zero_file(&s.0.join("output/payload.bin"), 1024 * 1024 * 1024);
            measurements.push(format!("{kind}: {rss}"));
        }
        println!(
            "1 GiB extraction peak RSS bytes: {}",
            measurements.join(", ")
        );
        fs::write(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/ci/streaming-rss.log"),
            measurements.join("\n") + "\n",
        )
        .unwrap();
    }
    #[test]
    fn enospc_at_create_or_after_partial_write_cleans_output() {
        for mode in ["create-full", "full", "stage-full"] {
            let s = Scratch::new();
            generate(&s, "store", 1024 * 1024);
            fs::write(s.0.join("preserve"), b"original").unwrap();
            let request = start(&s, false, Limits::default().output);
            let child = spawn(&s, request, Some(mode));
            assert_eq!(wait(child.id()).0, 0);
            let events = events(&s);
            let t = terminal(&events);
            assert_eq!(t.error, ErrorCode::Io, "{mode}: {events:?}");
            assert_eq!(t.publication, Publication::None);
            assert_eq!(t.retained_partial_count, 0);
            assert_eq!(t.cleanup, Cleanup::Complete);
            assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
            assert_eq!(fs::read(s.0.join("preserve")).unwrap(), b"original");
        }
    }
    #[test]
    fn anonymous_tempfile_fallback_unlinks_before_use() {
        let s = Scratch::new();
        generate(&s, "store", 1024 * 1024);
        let request = start(&s, false, Limits::default().output);
        let child = spawn(&s, request, Some("fallback"));
        assert_eq!(wait(child.id()).0, 0);
        let events = events(&s);
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        assert!(s.0.join("paused").exists());
        assert_eq!(fs::read_dir(s.0.join("staging")).unwrap().count(), 0);
        assert_zero_file(&s.0.join("output/payload.bin"), 1024 * 1024);
    }
    struct Subreaper(i32);
    impl Subreaper {
        fn new() -> Self {
            let mut old = 0;
            assert_eq!(unsafe { prctl(37, &mut old as *mut i32, 0, 0, 0) }, 0);
            assert_eq!(unsafe { prctl(36, 1, 0, 0, 0) }, 0);
            Self(old)
        }
    }
    impl Drop for Subreaper {
        fn drop(&mut self) {
            assert_eq!(unsafe { prctl(36, self.0, 0, 0, 0) }, 0);
        }
    }
    #[test]
    fn orphan_app_helper() {
        let Some(path) = std::env::var_os("ZIPELO_TEST_APP_HELPER") else {
            return;
        };
        let s = std::mem::ManuallyDrop::new(Scratch(PathBuf::from(path)));
        let mut request = Start {
            action: Action::Extract {
                archive: s.path("input"),
                destination: s.path("output"),
                selection: Selection::All,
                list_id: None,
                gzip_output_name: None,
            },
            limits: Limits::default(),
        };
        if std::env::var_os("ZIPELO_BASELINE_WORKER").is_some() {
            request.limits = Limits {
                input: 64 * 1024 * 1024,
                output: 128 * 1024 * 1024,
                entries: 10000,
            };
        }
        let mut cmd = command();
        cmd.env("TMPDIR", staging(&s))
            .stdin(Stdio::piped())
            .stdout(File::create(s.0.join("events")).unwrap())
            .stderr(File::create(s.0.join("stderr")).unwrap())
            .env("LD_PRELOAD", s.0.join("io_fault.so"))
            .env("ZIPELO_TEST_FAULT", "pause")
            .env("ZIPELO_TEST_MARKER", s.0.join("paused"))
            .env("ZIPELO_TEST_RELEASE", s.0.join("release"));
        let mut child = cmd.spawn().unwrap();
        fs::write(s.0.join("worker.pid"), child.id().to_string()).unwrap();
        let mut writer = Writer::new(child.stdin.take().unwrap(), 17);
        writer.send(&Message::Start(request)).unwrap();
        loop {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    #[test]
    fn app_death_stops_worker_and_removes_partial_temp() {
        let _guard = Subreaper::new();
        let s = Scratch::new();
        generate(&s, "store", 1024 * 1024);
        let _ = start(&s, false, Limits::default().output);
        shim(&s);
        let mut helper = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "hardening::linux::orphan_app_helper",
                "--nocapture",
            ])
            .env("ZIPELO_TEST_APP_HELPER", &s.0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !s.0.join("paused").exists() {
            if Instant::now() > deadline {
                helper.kill().unwrap();
                helper.wait().unwrap();
                if let Ok(pid) = fs::read_to_string(s.0.join("worker.pid")) {
                    unsafe {
                        kill(pid.parse().unwrap(), 9);
                    }
                    wait(pid.trim().parse().unwrap());
                }
                panic!("worker did not reach publication");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let pid = fs::read_to_string(s.0.join("worker.pid"))
            .unwrap()
            .parse::<u32>()
            .unwrap();
        helper.kill().unwrap();
        helper.wait().unwrap();
        fs::write(s.0.join("release"), b"").unwrap();
        assert_eq!(wait(pid).0, 0);
        let events = events(&s);
        assert_eq!(terminal(&events).error, ErrorCode::Io, "{events:?}");
        assert_eq!(terminal(&events).publication, Publication::None);
        assert_eq!(terminal(&events).cleanup, Cleanup::Complete);
        assert_eq!(fs::read_dir(s.0.join("output")).unwrap().count(), 0);
    }
}
