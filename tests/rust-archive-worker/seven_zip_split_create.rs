// 분할 7z 만들기: 볼륨 이름/크기, 분할 읽기 왕복, 이름 충돌·취소·게시 실패 되돌리기.
use super::*;

const MIB: u64 = 1024 * 1024;

fn noise(length: usize, mut state: u32) -> Vec<u8> {
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect()
}
fn split_request(c: &Case, names: &[&str], output: &str, size: u64) -> Start {
    let mut start = c.request(names);
    if let Action::CreateSevenZip {
        output_file,
        volume_size,
        ..
    } = &mut start.action
    {
        *output_file = NativePath::from_path(&c.0.join(output));
        *volume_size = Some(size);
    }
    start
}
fn volume_files(dir: &std::path::Path, prefix: &str) -> Vec<String> {
    let mut names = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(prefix))
        .collect::<Vec<_>>();
    names.sort();
    names
}
fn created(events: &[Message]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            Message::Output {
                entry_id: None,
                relative_name,
                disposition: Disposition::Created,
            } => Some(relative_name.clone()),
            _ => None,
        })
        .collect()
}
fn no_temporaries(dir: &std::path::Path) {
    assert!(!tree(dir).keys().any(|name| name.contains(".zipelo-")));
}

#[test]
fn split_volumes_have_requested_sizes_and_round_trip_through_the_split_reader() {
    for parent in ["", "source"] {
        let c = Case::new();
        private_dir(&c.0.join("source"));
        private_dir(&c.0.join("source/빈 폴더"));
        fs::write(
            c.0.join("source/noise.bin"),
            noise(2 * MIB as usize + 300_000, 7),
        )
        .unwrap();
        fs::write(c.0.join("source/한글.txt"), "분할 압축\n".repeat(2000)).unwrap();
        fs::write(c.0.join("source/empty"), b"").unwrap();
        let expected = tree(&c.0.join("source"));
        let output = if parent.is_empty() {
            "result.7z".to_owned()
        } else {
            format!("{parent}/result.7z")
        };
        let events = c.run(Message::Start(split_request(&c, &["source"], &output, MIB)));
        let t = terminal(&events);
        assert_eq!(t.outcome, Outcome::Completed, "{events:?}");
        assert_eq!(t.publication, Publication::Complete);
        assert_eq!(t.created_count, 3);
        assert_eq!(t.cleanup, Cleanup::Complete);
        assert_eq!(t.retained_partial_count, 0);
        assert_eq!(
            created(&events),
            ["result.7z.001", "result.7z.002", "result.7z.003"]
        );
        let dir = c.0.join(parent);
        assert!(!dir.join("result.7z").exists());
        assert_eq!(
            volume_files(&dir, "result.7z"),
            ["result.7z.001", "result.7z.002", "result.7z.003"]
        );
        let sizes = ["001", "002", "003"].map(|n| {
            fs::metadata(dir.join(format!("result.7z.{n}")))
                .unwrap()
                .len()
        });
        assert_eq!(sizes[0], MIB);
        assert_eq!(sizes[1], MIB);
        assert!(sizes[2] > 0 && sizes[2] <= MIB, "{sizes:?}");
        // 볼륨을 이어 붙이면 그대로 하나의 7z임.
        let mut joined = Vec::new();
        for n in ["001", "002", "003"] {
            joined.extend(fs::read(dir.join(format!("result.7z.{n}"))).unwrap());
        }
        assert_eq!(&joined[..6], b"7z\xbc\xaf\x27\x1c");
        no_temporaries(&c.0);

        let first = NativePath::from_path(&dir.join("result.7z.001"));
        let listed = c.run(Message::Start(Start {
            action: Action::List {
                archive: first.clone(),
            },
            limits: Limits::default(),
        }));
        assert_eq!(terminal(&listed).outcome, Outcome::Completed, "{listed:?}");
        assert!(listed.iter().any(|event| matches!(
            event,
            Message::ListEnd {
                list_id: ListId {
                    source_object: SourceObject::SplitSevenZip { .. },
                    ..
                },
                ..
            }
        )));
        private_dir(&c.0.join("decoded"));
        let extracted = c.run(Message::Start(Start {
            action: Action::Extract {
                archive: first,
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
            let output = Command::new(&tool)
                .arg("t")
                .arg(dir.join("result.7z.001"))
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            let output = Command::new(tool)
                .args(["x", "-y"])
                .arg(dir.join("result.7z.001"))
                .arg(format!("-o{}", c.0.join("independent").display()))
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            assert_eq!(tree(&c.0.join("independent/source")), expected);
        }
    }
}

#[test]
fn small_archives_still_use_a_single_first_volume_name() {
    let c = Case::new();
    fs::write(c.0.join("source"), b"small payload").unwrap();
    let events = c.run(Message::Start(split_request(
        &c,
        &["source"],
        "작은.7z",
        4 * MIB,
    )));
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(terminal(&events).created_count, 1);
    assert_eq!(created(&events), ["작은.7z.001"]);
    assert_eq!(volume_files(&c.0, "작은.7z"), ["작은.7z.001"]);
    let listed = c.run(Message::Start(Start {
        action: Action::List {
            archive: c.path("작은.7z.001"),
        },
        limits: Limits::default(),
    }));
    assert_eq!(terminal(&listed).outcome, Outcome::Completed, "{listed:?}");
}

#[test]
fn invalid_sizes_names_and_existing_volume_names_are_rejected() {
    let c = Case::new();
    fs::write(c.0.join("source"), b"abcdef").unwrap();
    for size in [0, 1, MIB - 1] {
        failure(
            &c.run(Message::Start(split_request(
                &c,
                &["source"],
                "result.7z",
                size,
            ))),
            ErrorCode::InvalidRequest,
        );
    }
    failure(
        &c.run(Message::Start(split_request(
            &c,
            &["source"],
            "result.zip",
            MIB,
        ))),
        ErrorCode::InvalidRequest,
    );
    // 분할 읽기는 자릿수와 관계없이 같은 접두사의 숫자 이름을 한 묶음으로 보므로 모두 거부함.
    for existing in [
        "result.7z.001",
        "result.7z.002",
        "result.7z.0001",
        "result.7z.5",
    ] {
        fs::write(c.0.join(existing), b"preserve").unwrap();
        failure(
            &c.run(Message::Start(split_request(
                &c,
                &["source"],
                "result.7z",
                MIB,
            ))),
            ErrorCode::Conflict,
        );
        assert_eq!(fs::read(c.0.join(existing)).unwrap(), b"preserve");
        assert_eq!(volume_files(&c.0, "result.7z"), [existing]);
        fs::remove_file(c.0.join(existing)).unwrap();
    }
    // 숫자가 아닌 비슷한 이름과 분할하지 않은 같은 이름은 그대로 두고 만듦.
    fs::write(c.0.join("result.7z"), b"single").unwrap();
    fs::write(c.0.join("result.7z.bak"), b"backup").unwrap();
    let events = c.run(Message::Start(split_request(
        &c,
        &["source"],
        "result.7z",
        MIB,
    )));
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    assert_eq!(fs::read(c.0.join("result.7z")).unwrap(), b"single");
    assert!(c.0.join("result.7z.001").is_file());
    no_temporaries(&c.0);
}

#[test]
fn cancel_while_writing_volumes_publishes_nothing() {
    let c = Case::new();
    fs::write(c.0.join("source"), noise(3 * MIB as usize, 11)).unwrap();
    let mut sent = false;
    let events = c.run_with(
        &mut c.command(),
        Message::Start(split_request(&c, &["source"], "result.7z", MIB)),
        |event| {
            // 볼륨 게시 단계의 첫 쓰기 진행(전체 길이가 있는 Write)에서 취소함.
            if !sent
                && matches!(
                    event,
                    Message::Progress {
                        stage: Stage::Write,
                        total_bytes: Some(_),
                        ..
                    }
                )
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
    assert!(volume_files(&c.0, "result.7z").is_empty());
    no_temporaries(&c.0);
}

#[cfg(target_os = "linux")]
#[test]
fn a_volume_name_taken_during_publication_rolls_back_every_volume() {
    for operation in ["second", "source", "cancel"] {
        let c = Case::new();
        fs::write(c.0.join("source"), noise(2 * MIB as usize + 1000, 3)).unwrap();
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
            Message::Start(split_request(&c, &["source"], "result.7z", MIB)),
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
                        // 첫 볼륨은 게시할 수 있지만 둘째 이름이 막혀 첫 볼륨도 되돌려야 함.
                        "second" => fs::write(c.0.join("result.7z.002"), b"user data").unwrap(),
                        "source" => fs::write(c.0.join("source"), b"changed").unwrap(),
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
                "second" => ErrorCode::Conflict,
                "source" => ErrorCode::SourceChanged,
                _ => ErrorCode::None,
            },
        );
        if operation == "second" {
            assert_eq!(volume_files(&c.0, "result.7z"), ["result.7z.002"]);
            assert_eq!(fs::read(c.0.join("result.7z.002")).unwrap(), b"user data");
        } else {
            assert!(volume_files(&c.0, "result.7z").is_empty());
        }
        no_temporaries(&c.0);
    }
}
