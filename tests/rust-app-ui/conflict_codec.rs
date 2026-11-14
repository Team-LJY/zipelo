use super::*;

#[test]
fn conflict_adapter_preserves_sizes_names_ids_and_choices() {
    let required = || wire::Message::ConflictRequired {
        challenge_id: 53,
        entry_id: 7,
        relative_name: "사진/기록.txt".into(),
        incoming_size: u64::MAX,
        existing_size: 1 << 34,
    };
    match event_to_ui(required(), Some(wire::ActionKind::Extract)).unwrap() {
        ui::Event::ConflictRequired(c) => {
            assert_eq!(c.challenge_id, 53);
            assert_eq!(c.entry_id, 7);
            assert_eq!(c.relative_name, "사진/기록.txt");
            assert_eq!(c.incoming_size, u64::MAX);
            assert_eq!(c.existing_size, 1 << 34);
        }
        _ => panic!("충돌 요청 변환 누락"),
    }
    for expected in [
        None,
        Some(wire::ActionKind::List),
        Some(wire::ActionKind::CreateZip),
    ] {
        assert!(event_to_ui(required(), expected).is_err());
    }
    for choice in [
        ui::ConflictChoice::Overwrite,
        ui::ConflictChoice::Skip,
        ui::ConflictChoice::Rename,
    ] {
        match event_to_ui(
            wire::Message::ConflictResolved {
                entry_id: 7,
                relative_name: "사진/기록 (1).txt".into(),
                choice: conflict_to_wire(choice),
            },
            Some(wire::ActionKind::Extract),
        )
        .unwrap()
        {
            ui::Event::ConflictResolved(r) => {
                assert_eq!(r.choice, choice);
                assert_eq!(r.entry_id, 7);
                assert_eq!(r.relative_name, "사진/기록 (1).txt");
            }
            _ => panic!("충돌 결과 변환 누락"),
        }
    }
}

#[test]
fn split_list_id_adapter_round_trip_preserves_manifest_and_logical_input() {
    let id = ui::ListId {
        catalog_version: 2,
        format: ui::Format::SevenZip,
        source_object: ui::SourceObject::SplitSevenZip {
            manifest_sha256: [0xa5; 32],
        },
        archive_bytes: u64::MAX,
        archive_sha256: [0x5a; 32],
    };
    let wire_id = list_to_wire(id.clone());
    assert!(wire_id.valid());
    assert_eq!(list_to_ui(wire_id), id);
}
#[test]
fn volume_problem_adapter_preserves_native_filename_and_rejects_other_actions() {
    for volume in [
        wire::NativePath::Unix(vec![0xff, 46, 55, 122]),
        wire::NativePath::Windows(vec![0xd800, 46, 55, 122]),
    ] {
        for action in [
            Some(wire::ActionKind::List),
            Some(wire::ActionKind::Extract),
            Some(wire::ActionKind::Preview),
            Some(wire::ActionKind::CreateZip),
            None,
        ] {
            let event = event_to_ui(
                wire::Message::VolumeProblem {
                    reason: wire::VolumeProblemReason::MissingVolume,
                    volume: volume.clone(),
                },
                action,
            );
            if matches!(
                action,
                Some(wire::ActionKind::List | wire::ActionKind::Extract)
            ) {
                match event.unwrap() {
                    ui::Event::VolumeProblem(problem) => {
                        assert_eq!(problem.volume, volume);
                        assert_eq!(problem.reason, wire::VolumeProblemReason::MissingVolume);
                    }
                    _ => panic!("조각 오류 변환 누락"),
                }
            } else {
                assert!(event.is_err());
            }
        }
    }
    assert_eq!(
        error_to_ui(wire::ErrorCode::MissingVolume),
        Some(ui::ErrorCode::MissingVolume)
    );
}

#[test]
fn sevenzip_creation_adapter_preserves_paths_and_fixed_options() {
    let source = std::env::current_dir().unwrap().join("원본");
    let output = source.with_file_name("결과.7z");
    let converted = start_to_wire(ui::Start::CreateSevenZip {
        sources: vec![(source.clone(), "원본".into())],
        output_file: output.clone(),
    });
    match converted.action {
        wire::Action::CreateSevenZip {
            sources,
            output_file,
            method,
            solid,
            encryption,
        } => {
            assert_eq!(sources[0].archive_name, "원본");
            assert_eq!(
                sources[0].path,
                wire::NativePath::from_path(&crate::archive_path::worker_path(&source))
            );
            assert_eq!(
                output_file,
                wire::NativePath::from_path(&crate::archive_path::worker_path(&output))
            );
            assert_eq!(method, wire::SevenZipMethod::Lzma2);
            assert!(!solid);
            assert_eq!(encryption, wire::Encryption::None);
        }
        other => panic!("잘못된 생성 형식: {other:?}"),
    }
}
