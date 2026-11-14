use super::*;
use worker_protocol::protocol::EntryKind;

fn begin() -> (Model, FakeFactory) {
    let mut model = Model::default();
    let mut factory = FakeFactory::manual();
    assert!(model.begin(
        Start::RecoverZip {
            archive: path("damaged.zip"),
            output_file: path("recovered.zip"),
        },
        &mut factory
    ));
    factory
        .state
        .borrow_mut()
        .frame(Event::Accepted { encryption: false });
    (model, factory)
}
fn entry() -> RecoveryEntry {
    RecoveryEntry {
        candidate_id: 0,
        source_offset: 0,
        name: Some("한글.txt".into()),
        kind: Some(EntryKind::File),
        status: RecoveryStatus::Verified,
        reason: RecoveryReason::HeaderRebuilt,
        declared_size: Some(4),
        verified_size: Some(4),
        sha256: Some([1; 32]),
    }
}
fn summary(partial: bool) -> RecoveryEnd {
    RecoveryEnd {
        mode: if partial {
            RecoveryMode::LocalChain
        } else {
            RecoveryMode::CentralDirectory
        },
        source_bytes: 128,
        source_sha256: [2; 32],
        known_entries: (!partial).then_some(1),
        verified_entries: 1,
        lost_entries: 0,
        unverified_entries: 0,
        unknown_loss: partial,
    }
}
fn result(partial: bool) -> Terminal {
    Terminal {
        outcome: if partial {
            Outcome::Failed
        } else {
            Outcome::Completed
        },
        error: partial.then_some(ErrorCode::RecoveryIncomplete),
        publication: Publication::Complete,
        created_count: 1,
        retained_partial_count: 0,
        durability: Durability::Synced,
        cleanup: Cleanup::Complete,
    }
}
fn reports(factory: &FakeFactory, partial: bool) {
    let mut s = factory.state.borrow_mut();
    s.frame(Event::RecoveryEntry(entry()));
    s.frame(Event::RecoveryEnd(summary(partial)));
    s.frame(Event::Output {
        entry_id: None,
        relative_name: "recovered.zip".into(),
        disposition: Disposition::Created,
    });
    s.frame(Event::Terminal(result(partial)));
}
fn eof(factory: &FakeFactory) {
    factory.state.borrow_mut().queue.extend([
        HostEvent::StdoutEof,
        HostEvent::StderrEof,
        HostEvent::Exited { success: true },
    ]);
}
#[test]
fn recovery_requires_terminal_both_eofs_and_reap_even_for_saved_partial() {
    for partial in [false, true] {
        let (mut model, factory) = begin();
        reports(&factory, partial);
        for _ in 0..20 {
            model.poll();
        }
        assert!(model.busy());
        assert!(!model.confirmed);
        eof(&factory);
        pump(&mut model);
        assert!(model.confirmed, "{:?}", model.error);
        assert_eq!(model.recovery_end.as_ref().unwrap().unknown_loss, partial);
        assert!(model.entries.is_empty());
        assert!(model.list_id.is_none());
        assert!(!model.can_retry_expanded());
    }
}
#[test]
fn recovery_rejects_wrong_ids_offsets_names_and_unverified_hashes() {
    for change in 0..5 {
        let (mut model, factory) = begin();
        let mut e = entry();
        match change {
            0 => e.candidate_id = 1,
            1 => e.name = Some("../outside".into()),
            2 => e.status = RecoveryStatus::Lost,
            3 => e.verified_size = Some(5),
            _ => e.name = Some("x".repeat(4097)),
        }
        factory.state.borrow_mut().frame(Event::RecoveryEntry(e));
        for _ in 0..5 {
            model.poll();
        }
        assert!(model.error.is_some(), "{change}");
        assert!(model.recovery_entries.is_empty());
    }
}
#[test]
fn recovery_rejects_duplicate_end_wrong_counts_and_false_success() {
    for change in 0..4 {
        let (mut model, factory) = begin();
        let mut s = factory.state.borrow_mut();
        s.frame(Event::RecoveryEntry(entry()));
        let mut end = summary(change == 3);
        if change == 0 {
            end.verified_entries = 2;
        }
        if change == 1 {
            end.source_bytes = 0;
        }
        s.frame(Event::RecoveryEnd(end));
        if change == 2 {
            s.frame(Event::RecoveryEnd(summary(false)));
        }
        s.frame(Event::Output {
            entry_id: None,
            relative_name: "recovered.zip".into(),
            disposition: Disposition::Created,
        });
        s.frame(Event::Terminal(result(false)));
        drop(s);
        eof(&factory);
        pump(&mut model);
        assert!(!model.confirmed, "{change}");
        assert!(model.error.is_some());
    }
}
#[test]
fn recovery_summary_is_not_a_saved_artifact() {
    let (mut model, factory) = begin();
    let mut s = factory.state.borrow_mut();
    s.frame(Event::RecoveryEntry(entry()));
    s.frame(Event::RecoveryEnd(summary(false)));
    s.frame(Event::Terminal(result(false)));
    drop(s);
    eof(&factory);
    pump(&mut model);
    assert!(!model.confirmed);
    assert!(model.outputs.is_empty());
}
