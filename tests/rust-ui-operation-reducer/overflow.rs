use super::*;

fn start() -> Model {
    Model::new(*b"overflow")
        .unwrap()
        .reduce(Input::Command(Command::StartMock))
        .state
}

#[test]
fn serial_max_is_rejected_without_wrapping_or_effect() {
    let mut model = Model::new(*b"overflow").unwrap();
    model.serial = u64::MAX;
    let result = model.reduce(Input::Command(Command::StartMock));
    assert_eq!(
        result.disposition,
        Disposition::Ignored(Rejection::SerialOverflow)
    );
    assert_eq!(result.state, model);
    assert_eq!(result.effect, None);
}

#[test]
fn last_serial_maps_injectively_then_next_start_is_rejected() {
    let mut model = Model::new(*b"overflow").unwrap();
    model.serial = u64::MAX - 1;
    let model = model.reduce(Input::Command(Command::StartMock)).state;
    assert_eq!(model.operation().unwrap().mapping.ui.0, u64::MAX);
    assert_eq!(
        model.operation().unwrap().mapping.job.0[8..],
        u64::MAX.to_be_bytes()
    );
    let mut model = model;
    let op = model.operation.as_mut().unwrap();
    op.terminal = Some(Terminal::Completed(CompletionKind::MemoryValidated));
    op.cleanup = Some(CleanupObservation {
        kind: CleanupKind::Confirmed,
        evidence: EvidenceKind::Model,
    });
    let result = model.reduce(Input::Command(Command::StartMock));
    assert_eq!(
        result.disposition,
        Disposition::Ignored(Rejection::SerialOverflow)
    );
    assert_eq!(result.effect, None);
}

#[test]
fn sequence_max_latches_fault_without_accepting_cleanup() {
    let mut model = start();
    let op = model.operation.as_mut().unwrap();
    op.next_sequence = u64::MAX;
    let mapping = op.mapping;
    let result = model.reduce(Input::Event(Envelope {
        evidence: EvidenceKind::Model,
        mapping,
        sequence: u64::MAX,
        event: MockEvent::CleanupConfirmed,
    }));
    assert_eq!(
        result.disposition,
        Disposition::ConnectionFault(LinkFault::SequenceOverflow)
    );
    assert!(!result.state.operation().unwrap().cleanup_confirmed());
    assert_eq!(result.state.operation().unwrap().next_sequence, u64::MAX);
}

#[test]
fn sequence_last_representable_increment_is_accepted() {
    let mut model = start();
    let op = model.operation.as_mut().unwrap();
    op.next_sequence = u64::MAX - 1;
    let mapping = op.mapping;
    let result = model.reduce(Input::Event(Envelope {
        evidence: EvidenceKind::Model,
        mapping,
        sequence: u64::MAX - 1,
        event: MockEvent::CleanupConfirmed,
    }));
    assert_eq!(result.disposition, Disposition::Applied);
    assert_eq!(result.state.operation().unwrap().next_sequence, u64::MAX);
}

#[test]
fn generation_max_preserves_previous_rows_and_selection() {
    let mut model = start();
    model.generation = u64::MAX;
    let mapping = model.operation().unwrap().mapping;
    let result = model.reduce(Input::Command(Command::ReplaceMockList {
        operation: mapping.ui,
        rows: vec![],
    }));
    assert_eq!(
        result.disposition,
        Disposition::Ignored(Rejection::GenerationOverflow)
    );
    assert_eq!(result.state, model);
}

#[test]
fn last_generation_can_be_installed_without_wrapping() {
    let mut model = start();
    model.generation = u64::MAX - 1;
    let mapping = model.operation().unwrap().mapping;
    let result = model.reduce(Input::Command(Command::ReplaceMockList {
        operation: mapping.ui,
        rows: vec![],
    }));
    assert_eq!(result.disposition, Disposition::Applied);
    assert_eq!(result.state.generation(), u64::MAX);
}

#[test]
fn generation_overflow_keeps_nonempty_rows_and_selected_key() {
    let mut model = start();
    let ui = model.operation().unwrap().mapping.ui;
    model = model
        .reduce(Input::Command(Command::ReplaceMockList {
            operation: ui,
            rows: vec![Row {
                id: RowId(7),
                name: "한글/한".into(),
            }],
        }))
        .state;
    model.generation = u64::MAX;
    let key = model.row_key(RowId(7)).unwrap();
    model = model.reduce(Input::Command(Command::Select(key))).state;
    assert_eq!(model.selected(), Some(key));
    let result = model.reduce(Input::Command(Command::ReplaceMockList {
        operation: ui,
        rows: vec![Row {
            id: RowId(9),
            name: "교체".into(),
        }],
    }));
    assert_eq!(
        result.disposition,
        Disposition::Ignored(Rejection::GenerationOverflow)
    );
    assert_eq!(result.state, model);
    assert_eq!(result.state.selected(), Some(key));
    assert_eq!(result.state.rows()[0].name, "한글/한");
}
