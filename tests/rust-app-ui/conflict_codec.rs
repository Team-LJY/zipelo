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
