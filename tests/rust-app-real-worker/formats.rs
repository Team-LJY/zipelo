#![allow(dead_code)]
#[path = "../../experiments/rust-ui-memory-app/src/archive_ui.rs"]
mod archive_ui;
use zipelo_ui_memory_app::archive_path;
mod adapter {
    include!("../../experiments/rust-ui-memory-app/src/archive_process.rs");

    #[cfg(windows)]
    #[test]
    fn all_worker_path_fields_use_the_same_safe_drive_conversion() {
        use std::path::{Path, PathBuf};
        let input = PathBuf::from(r"\\?\C:\fixture\archive.zip");
        let output = PathBuf::from(r"\\?\C:\fixture\output");
        let expected_input = Path::new(r"C:\fixture\archive.zip");
        let expected_output = Path::new(r"C:\fixture\output");
        let wire::Action::List { archive } = start_to_wire(ui::Start::List {
            archive: input.clone(),
        })
        .action
        else {
            panic!("목록 요청 누락")
        };
        assert_eq!(archive.to_path().unwrap(), expected_input);
        let wire::Action::Extract {
            archive,
            destination,
            ..
        } = start_to_wire(ui::Start::Extract {
            archive: input.clone(),
            destination: output.clone(),
            selection: ui::Selection::All,
            list_id: token(
                ui::Format::Zip,
                ui::SourceObject::Windows {
                    volume: 1,
                    file_id: [2; 16],
                },
            ),
            gzip_output_name: None,
        })
        .action
        else {
            panic!("추출 요청 누락")
        };
        assert_eq!(archive.to_path().unwrap(), expected_input);
        assert_eq!(destination.to_path().unwrap(), expected_output);
        for aes256 in [false, true] {
            let wire::Action::CreateZip {
                sources,
                output_file,
                encryption,
                ..
            } = start_to_wire(ui::Start::CreateZip {
                sources: vec![(input.clone(), "source.txt".into())],
                output_file: output.clone(),
                method: ui::Method::Deflate,
                aes256,
            })
            .action
            else {
                panic!("생성 요청 누락")
            };
            assert_eq!(sources[0].path.to_path().unwrap(), expected_input);
            assert_eq!(output_file.to_path().unwrap(), expected_output);
            assert_eq!(
                encryption,
                if aes256 {
                    wire::Encryption::Aes256
                } else {
                    wire::Encryption::None
                }
            );
        }
        for text in [
            r"\\?\UNC\server\share\one.zip",
            r"\\server\share\one.zip",
            r"\\?\C:\a\..\one.zip",
        ] {
            let wire::Action::List { archive } = start_to_wire(ui::Start::List {
                archive: text.into(),
            })
            .action
            else {
                panic!("목록 요청 누락")
            };
            assert!(archive.to_path().is_err());
        }
    }

    fn token(format: ui::Format, source_object: ui::SourceObject) -> ui::ListId {
        ui::ListId {
            catalog_version: 1,
            format,
            source_object,
            archive_bytes: 33,
            archive_sha256: [44; 32],
        }
    }
    fn formats() -> [ui::Format; 5] {
        [
            ui::Format::Zip,
            ui::Format::Tar,
            ui::Format::Gzip,
            ui::Format::TarGzip,
            ui::Format::SevenZip,
        ]
    }
    #[test]
    fn list_end_preserves_existing_tags_and_assigns_four_to_seven_zip() {
        for (tag, format) in formats().into_iter().enumerate() {
            let id = token(
                format,
                ui::SourceObject::Unix {
                    device: 11,
                    inode: 22,
                },
            );
            let mut actual = Vec::new();
            codec::Writer::new(&mut actual, 99)
                .send(&wire::Message::ListEnd {
                    entry_count: 7,
                    list_id: list_to_wire(id.clone()),
                })
                .unwrap();
            let mut body = Vec::new();
            body.extend(7u32.to_le_bytes());
            body.extend(1u16.to_le_bytes());
            body.push(tag as u8);
            body.push(1);
            body.extend(11u64.to_le_bytes());
            body.extend(22u64.to_le_bytes());
            body.extend(33u64.to_le_bytes());
            body.extend([44; 32]);
            let mut expected = Vec::from(*b"ZAW1");
            expected.extend(1u16.to_le_bytes());
            expected.extend(102u16.to_le_bytes());
            expected.extend((body.len() as u32).to_le_bytes());
            expected.extend([0; 4]);
            expected.extend(99u64.to_le_bytes());
            expected.extend(0u64.to_le_bytes());
            expected.extend(body);
            assert_eq!(actual, expected);
            let (_, _, message) = codec::Reader::new(actual.as_slice())
                .read()
                .unwrap()
                .unwrap();
            match event_to_ui(message, None).unwrap() {
                ui::Event::ListEnd {
                    entry_count,
                    list_id,
                } => {
                    assert_eq!(entry_count, 7);
                    assert_eq!(list_id, id);
                }
                _ => panic!("목록 완료 응답이 아님"),
            }
        }
    }
    #[test]
    fn extraction_roundtrip_preserves_both_platform_identities_and_all_formats() {
        for source in [
            ui::SourceObject::Unix {
                device: 11,
                inode: 22,
            },
            ui::SourceObject::Windows {
                volume: 55,
                file_id: [66; 16],
            },
        ] {
            for format in formats() {
                let id = token(format, source.clone());
                assert_eq!(list_to_ui(list_to_wire(id.clone())), id);
                let start = start_to_wire(ui::Start::Extract {
                    archive: PathBuf::from("input.7z"),
                    destination: PathBuf::from("output"),
                    selection: ui::Selection::EntryIds(vec![7]),
                    list_id: id.clone(),
                    gzip_output_name: if format == ui::Format::Gzip {
                        Some("content".into())
                    } else {
                        None
                    },
                });
                let mut bytes = Vec::new();
                codec::Writer::new(&mut bytes, 99)
                    .send(&wire::Message::Start(start))
                    .unwrap();
                assert_eq!(&bytes[6..8], &1u16.to_le_bytes());
                match codec::Reader::new(bytes.as_slice())
                    .read()
                    .unwrap()
                    .unwrap()
                    .2
                {
                    wire::Message::Start(wire::Start {
                        action:
                            wire::Action::Extract {
                                list_id: Some(decoded),
                                selection,
                                ..
                            },
                        ..
                    }) => {
                        assert_eq!(list_to_ui(decoded), id);
                        assert_eq!(selection, wire::Selection::EntryIds(vec![7]));
                    }
                    _ => panic!("압축 해제 요청이 아님"),
                }
            }
        }
    }
    #[test]
    fn disk_full_terminal_roundtrip_displays_space_guidance_and_preserves_cleanup() {
        for (error, expected, guidance) in [
            (
                wire::ErrorCode::DiskFull,
                ui::ErrorCode::DiskFull,
                "디스크 여유 공간이 부족합니다. 공간을 확보한 뒤 다시 시도하세요",
            ),
            (
                wire::ErrorCode::Io,
                ui::ErrorCode::Io,
                "파일을 읽거나 쓰지 못했습니다. 파일과 폴더의 접근 권한을 확인하세요",
            ),
        ] {
            let message = wire::Message::Terminal(wire::Terminal {
                outcome: wire::Outcome::Failed,
                error,
                publication: wire::Publication::Partial,
                created_count: 2,
                retained_partial_count: 0,
                durability: wire::Durability::Synced,
                cleanup: wire::Cleanup::Complete,
                detail: "internal path must not appear".into(),
            });
            let mut frame = Vec::new();
            codec::Writer::new(&mut frame, 99).send(&message).unwrap();
            let (_, _, decoded) = codec::Reader::new(frame.as_slice())
                .read()
                .unwrap()
                .unwrap();
            let ui::Event::Terminal(terminal) = event_to_ui(decoded, None).unwrap() else {
                panic!("종료 응답이 아님");
            };
            assert_eq!(terminal.error, Some(expected));
            assert_eq!(terminal.outcome, ui::Outcome::Failed);
            assert_eq!(terminal.publication, ui::Publication::Partial);
            assert_eq!(terminal.created_count, 2);
            assert_eq!(terminal.retained_partial_count, 0);
            assert_eq!(terminal.cleanup, ui::Cleanup::Complete);
            let lines = ui::terminal_lines(&terminal);
            assert!(lines.iter().any(|line| line == guidance));
            assert!(lines.iter().any(|line| line.contains("일부 파일 게시됨")));
            assert!(!lines.iter().any(|line| line.contains("internal path")));
        }
    }
}
