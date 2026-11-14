use super::*;
use std::os::unix::fs::DirBuilderExt;

const SOURCE: &[u8] = b"unchanged source";
const SENTINEL: &[u8] = b"unrelated user file";

fn prepare(s: &Scratch, operation: &str) -> (Start, PathBuf) {
    fs::write(s.0.join("source"), SOURCE).unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(s.0.join("tmp"))
        .unwrap();
    let (action, output) = match operation {
        "extract" | "extract-nested" => {
            let created = execute(Start {
                action: Action::CreateZip {
                    sources: vec![Source {
                        path: s.path("source"),
                        archive_name: if operation == "extract-nested" {
                            "folder/payload"
                        } else {
                            "payload"
                        }
                        .into(),
                    }],
                    output_file: s.path("archive"),
                    method: Method::Store,
                    encryption: Encryption::None,
                },
                limits: Limits::default(),
            });
            assert_eq!(terminal(&created).outcome, Outcome::Completed);
            fs::DirBuilder::new()
                .mode(0o700)
                .create(s.0.join("output"))
                .unwrap();
            if operation == "extract-nested" {
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(s.0.join("output/folder"))
                    .unwrap();
            }
            (
                Action::Extract {
                    archive: s.path("archive"),
                    destination: s.path("output"),
                    selection: Selection::All,
                    list_id: None,
                    gzip_output_name: None,
                },
                s.0.join(if operation == "extract-nested" {
                    "output/folder/payload"
                } else {
                    "output/payload"
                }),
            )
        }
        "seven-zip" => (
            Action::CreateSevenZip {
                sources: vec![Source {
                    path: s.path("source"),
                    archive_name: "payload".into(),
                }],
                output_file: s.path("result.7z"),
                method: SevenZipMethod::Lzma2,
                solid: false,
                encryption: Encryption::None,
            },
            s.0.join("result.7z"),
        ),
        "recovery" => {
            fs::write(
                s.0.join("archive"),
                include_bytes!("../zip-recovery-fixtures/broken-local-header.zip"),
            )
            .unwrap();
            (
                Action::RecoverZip {
                    archive: s.path("archive"),
                    output_file: s.path("recovered.zip"),
                },
                s.0.join("recovered.zip"),
            )
        }
        _ => unreachable!(),
    };
    (
        Start {
            action,
            limits: Limits::default(),
        },
        output,
    )
}

fn run(s: &Scratch, start: Start, fault: &str) -> Vec<Message> {
    let library = s.0.join("fault.so");
    assert!(
        Command::new("cc")
            .args(["-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror"])
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/rust-archive-worker/io_fault.c"),
            )
            .args(["-ldl", "-o"])
            .arg(&library)
            .status()
            .unwrap()
            .success()
    );
    let mut child = s
        .worker()
        .env("TMPDIR", s.0.join("tmp"))
        .env("LD_PRELOAD", library)
        .env("ZIPELO_TEST_FAULT", fault)
        .env("ZIPELO_TEST_MARKER", s.0.join("marker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    Writer::new(child.stdin.take().unwrap(), 17)
        .send(&Message::Start(start))
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    let mut reader = Reader::new(output.stdout.as_slice());
    let mut events = Vec::new();
    while let Some((_, _, event)) = reader.read().unwrap() {
        events.push(event);
    }
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Message::Terminal(_)))
            .count(),
        1
    );
    assert_eq!(fs::read_dir(s.0.join("tmp")).unwrap().count(), 0);
    events
}

#[test]
fn storage_failure_matrix_preserves_outputs_and_reports_remaining_files() {
    for operation in ["extract", "extract-nested", "seven-zip", "recovery"] {
        for fault in [
            "none",
            "temp-full",
            "create-full",
            "full",
            "sync-full",
            "sync-io",
            "dir-sync-full",
            "dir-sync-io",
            "cleanup-fail",
            "full-cleanup-fail",
            "sync-cleanup-fail",
        ] {
            let s = Scratch::new();
            let (request, output) = prepare(&s, operation);
            let parent = output.parent().unwrap();
            let root = if operation.starts_with("extract") {
                s.0.join("output")
            } else {
                s.0.clone()
            };
            let unrelated = parent.join(".zipelo-unrelated.tmp");
            fs::write(&unrelated, SENTINEL).unwrap();
            let archive = fs::read(s.0.join("archive")).ok();
            let events = run(&s, request, fault);
            let end = terminal(&events);
            let context = format!("{operation}/{fault}: {events:?}");
            let published = matches!(
                fault,
                "none" | "cleanup-fail" | "dir-sync-full" | "dir-sync-io"
            );
            assert_eq!(output.exists(), published, "{context}");
            assert_eq!(end.created_count, u64::from(published), "{context}");
            assert_eq!(
                end.outcome,
                if fault == "none" {
                    Outcome::Completed
                } else {
                    Outcome::Failed
                },
                "{context}"
            );
            let expected_error = if fault == "none" {
                ErrorCode::None
            } else if matches!(
                fault,
                "sync-io" | "dir-sync-io" | "cleanup-fail" | "sync-cleanup-fail"
            ) {
                ErrorCode::Io
            } else {
                ErrorCode::DiskFull
            };
            assert_eq!(end.error, expected_error, "{context}");
            assert_eq!(
                end.publication,
                if fault == "none" {
                    Publication::Complete
                } else if published {
                    Publication::Partial
                } else {
                    Publication::None
                },
                "{context}"
            );
            assert_eq!(
                end.durability,
                if fault.starts_with("dir-sync") {
                    Durability::Failed
                } else if published {
                    Durability::Synced
                } else {
                    Durability::NotApplicable
                },
                "{context}"
            );
            let retained: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Message::Output {
                        relative_name,
                        disposition: Disposition::PartialRetained,
                        ..
                    } => Some(relative_name),
                    _ => None,
                })
                .collect();
            let cleanup_failed = fault.ends_with("cleanup-fail");
            assert_eq!(
                end.cleanup,
                if cleanup_failed {
                    Cleanup::Failed
                } else {
                    Cleanup::Complete
                },
                "{context}"
            );
            assert_eq!(
                end.retained_partial_count,
                u64::from(cleanup_failed),
                "{context}"
            );
            assert_eq!(retained.len(), usize::from(cleanup_failed), "{context}");
            for name in retained {
                let path = root.join(name);
                assert!(path.is_file(), "{context}: {path:?}");
                if published {
                    assert_eq!(fs::read(path).unwrap(), fs::read(&output).unwrap());
                }
            }
            assert_eq!(fs::read(&unrelated).unwrap(), SENTINEL, "{context}");
            assert_eq!(fs::read(s.0.join("source")).unwrap(), SOURCE, "{context}");
            assert_eq!(fs::read(s.0.join("archive")).ok(), archive, "{context}");
            assert_eq!(
                fs::read_dir(parent)
                    .unwrap()
                    .filter(|e| {
                        let name = e.as_ref().unwrap().file_name();
                        let name = name.to_string_lossy();
                        name.starts_with(".zipelo-") && name != ".zipelo-unrelated.tmp"
                    })
                    .count(),
                usize::from(cleanup_failed),
                "{context}"
            );
            if published && operation.starts_with("extract") {
                assert_eq!(fs::read(output).unwrap(), SOURCE, "{context}");
            }
        }
    }
}

#[test]
fn cleanup_does_not_delete_a_replacement_at_the_temporary_name() {
    for operation in ["extract", "extract-nested", "seven-zip", "recovery"] {
        for fault in ["swap-sync", "swap-publish"] {
            let s = Scratch::new();
            let (request, output) = prepare(&s, operation);
            let events = run(&s, request, fault);
            let end = terminal(&events);
            assert_eq!(end.outcome, Outcome::Failed, "{operation}: {events:?}");
            assert_eq!(
                end.error,
                if fault == "swap-publish" {
                    ErrorCode::Conflict
                } else {
                    ErrorCode::Io
                }
            );
            assert!(!output.exists());
            let replacements: Vec<_> = fs::read_dir(output.parent().unwrap())
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.file_name().unwrap().to_string_lossy().ends_with(".tmp"))
                .collect();
            assert_eq!(replacements.len(), 1, "{operation}: {events:?}");
            assert_eq!(
                fs::read(&replacements[0]).unwrap(),
                b"unrelated replacement"
            );
            assert_eq!(end.cleanup, Cleanup::Unconfirmed);
            assert_eq!(end.retained_partial_count, 0);
            assert!(!events.iter().any(|e| matches!(
                e,
                Message::Output {
                    disposition: Disposition::PartialRetained,
                    ..
                }
            )));
            assert_eq!(fs::read(s.0.join("source")).unwrap(), SOURCE);
        }
    }
}

#[test]
fn late_temporary_name_swap_is_never_reported_as_created_output() {
    for operation in ["extract", "extract-nested", "seven-zip", "recovery"] {
        let s = Scratch::new();
        let (request, output) = prepare(&s, operation);
        let original = fs::read(s.0.join("archive")).ok();
        let events = run(&s, request, "swap-on-link");
        let end = terminal(&events);
        assert_eq!(end.outcome, Outcome::Failed, "{operation}: {events:?}");
        assert_eq!(end.created_count, 0, "{operation}: {events:?}");
        assert_eq!(end.error, ErrorCode::Conflict);
        assert_eq!(end.publication, Publication::Uncertain);
        assert_eq!(end.durability, Durability::Unconfirmed);
        assert_eq!(end.cleanup, Cleanup::Unconfirmed);
        assert_eq!(end.retained_partial_count, 0);
        assert!(!events.iter().any(|e| matches!(
            e,
            Message::Output {
                disposition: Disposition::Created,
                ..
            }
        )));
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(
                    e,
                    Message::Output {
                        disposition: Disposition::CommitUncertain,
                        ..
                    }
                ))
                .count(),
            1
        );
        assert_eq!(fs::read(&output).unwrap(), b"unrelated replacement");
        assert_eq!(fs::read(s.0.join("source")).unwrap(), SOURCE);
        assert_eq!(fs::read(s.0.join("archive")).ok(), original);
        let replacements: Vec<_> = fs::read_dir(output.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert_eq!(replacements.len(), 1);
        assert_eq!(
            fs::read(&replacements[0]).unwrap(),
            b"unrelated replacement"
        );
    }
}

#[test]
fn moved_temporary_file_is_not_reported_cleaned() {
    for operation in ["extract", "extract-nested", "seven-zip", "recovery"] {
        let s = Scratch::new();
        let (request, output) = prepare(&s, operation);
        let events = run(&s, request, "move-on-sync");
        let end = terminal(&events);
        assert!(!output.exists());
        assert_eq!(end.outcome, Outcome::Failed, "{operation}: {events:?}");
        assert_eq!(end.error, ErrorCode::Conflict);
        assert_eq!(end.publication, Publication::None);
        assert_eq!(end.created_count, 0);
        let owned: Vec<_> = fs::read_dir(output.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().ends_with(".owned"))
            .collect();
        assert_eq!(owned.len(), 1);
        assert!(!fs::read(&owned[0]).unwrap().is_empty());
        if operation.starts_with("extract") {
            assert_eq!(fs::read(&owned[0]).unwrap(), SOURCE);
        }
        assert_eq!(end.cleanup, Cleanup::Unconfirmed, "{events:?}");
        assert_eq!(end.retained_partial_count, 0);
        assert_eq!(fs::read(s.0.join("source")).unwrap(), SOURCE);
    }
}
