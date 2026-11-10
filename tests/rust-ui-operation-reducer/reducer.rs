use zipelo_ui_operation_reducer::*;

fn command(model: &Model, command: Command) -> Transition {
    model.reduce(Input::Command(command))
}
fn started() -> Model {
    command(&Model::new(*b"mock-one").unwrap(), Command::StartMock).state
}
fn mapping(model: &Model) -> Mapping {
    model.operation().unwrap().mapping
}
fn event(model: &Model, event: MockEvent) -> Transition {
    model.reduce(Input::Event(Envelope {
        evidence: EvidenceKind::Model,
        mapping: mapping(model),
        sequence: model.operation().unwrap().next_sequence,
        event,
    }))
}
fn failure(origin: &str, classification: &str, code: i64) -> Failure {
    Failure {
        origin: origin.into(),
        policy: "mock-policy-v1".into(),
        classification: classification.into(),
        code,
    }
}
fn settle(model: &Model, terminal: Terminal) -> Model {
    let model = event(model, MockEvent::Terminal(terminal)).state;
    event(&model, MockEvent::CleanupConfirmed).state
}
fn list(model: &Model) -> Model {
    command(
        model,
        Command::ReplaceMockList {
            operation: mapping(model).ui,
            rows: vec![
                Row {
                    id: RowId(7),
                    name: "한글/한.txt".into(),
                },
                Row {
                    id: RowId(9),
                    name: "한글/한.txt".into(),
                },
            ],
        },
    )
    .state
}

#[test]
fn repeated_start_emits_one_effect_and_preserves_mapping() {
    let first = command(&Model::new(*b"mock-one").unwrap(), Command::StartMock);
    assert_eq!(first.effect, Some(MockEffect::Start(mapping(&first.state))));
    let again = command(&first.state, Command::StartMock);
    assert_eq!(again.disposition, Disposition::Ignored(Rejection::Busy));
    assert_eq!(again.effect, None);
    assert_eq!(again.state, first.state);
}

#[test]
fn reducing_does_not_mutate_input_state() {
    let model = started();
    let saved = model.clone();
    let result = command(&model, Command::RequestCancel);
    assert_eq!(model, saved);
    assert!(result.state.operation().unwrap().cancel_requested);
}

#[test]
fn repeated_cancel_emits_only_one_mock_command() {
    let first = command(&started(), Command::RequestCancel);
    assert_eq!(
        first.effect,
        Some(MockEffect::Cancel(mapping(&first.state)))
    );
    let second = command(&first.state, Command::RequestCancel);
    assert_eq!(second.effect, None);
    assert!(second.state.operation().unwrap().terminal.is_none());
    assert!(!second.state.operation().unwrap().cleanup_confirmed());
}

#[test]
fn hundred_percent_is_not_a_terminal_result() {
    let model = event(
        &started(),
        MockEvent::Progress {
            done: 10,
            total: 10,
        },
    )
    .state;
    let model = event(&model, MockEvent::CleanupConfirmed).state;
    assert!(model.operation().unwrap().terminal.is_none());
    assert_eq!(command(&model, Command::StartMock).effect, None);
}

#[test]
fn success_does_not_require_a_final_progress_event() {
    let model = settle(
        &started(),
        Terminal::Completed(CompletionKind::MemoryValidated),
    );
    assert!(model.operation().unwrap().settled());
    assert!(command(&model, Command::StartMock).effect.is_some());
}

#[test]
fn terminal_alone_does_not_enable_retry() {
    let model = event(
        &started(),
        MockEvent::Terminal(Terminal::Completed(CompletionKind::MemoryValidated)),
    )
    .state;
    assert_eq!(command(&model, Command::StartMock).effect, None);
}

#[test]
fn cleanup_first_waits_for_terminal() {
    let model = event(&started(), MockEvent::CleanupConfirmed).state;
    assert_eq!(command(&model, Command::StartMock).effect, None);
    let model = event(
        &model,
        MockEvent::Terminal(Terminal::Completed(CompletionKind::MemoryValidated)),
    )
    .state;
    assert!(model.operation().unwrap().settled());
}

#[test]
fn failure_retry_keeps_original_error_until_new_operation() {
    let cause = failure("worker", "Integrity", 12);
    let model = settle(&started(), Terminal::Failed(cause.clone()));
    assert_eq!(
        model.operation().unwrap().first_error,
        Some(FirstError::Reported(cause))
    );
    let next = command(&model, Command::StartMock);
    assert_ne!(mapping(&model), mapping(&next.state));
    assert_eq!(next.state.operation().unwrap().first_error, None);
}

#[test]
fn cleanup_failure_is_sticky_after_later_confirmation() {
    let model = event(
        &started(),
        MockEvent::CleanupFailed(failure("host", "Cleanup", 5)),
    )
    .state;
    let model = event(&model, MockEvent::Terminal(Terminal::MockCancelled)).state;
    let late = event(&model, MockEvent::CleanupConfirmed);
    assert_eq!(
        late.disposition,
        Disposition::Ignored(Rejection::CleanupSticky)
    );
    assert!(!late.state.operation().unwrap().settled());
    assert_eq!(command(&late.state, Command::StartMock).effect, None);
}

#[test]
fn original_failure_survives_cleanup_failure_and_link_error() {
    let cause = failure("codec", "MemError", -4);
    let model = event(
        &started(),
        MockEvent::Terminal(Terminal::Failed(cause.clone())),
    )
    .state;
    let model = event(&model, MockEvent::CleanupFailed(failure("host", "Io", 5))).state;
    let model = event(
        &model,
        MockEvent::Terminal(Terminal::Completed(CompletionKind::MemoryValidated)),
    )
    .state;
    assert_eq!(
        model.operation().unwrap().first_error,
        Some(FirstError::Reported(cause.clone()))
    );
    assert_eq!(
        model.operation().unwrap().terminal,
        Some(Terminal::Failed(cause))
    );
    assert_eq!(
        model.operation().unwrap().link_fault,
        Some(LinkFault::ConflictingTerminal)
    );
}

#[test]
fn old_events_do_not_advance_new_sequence_or_latch_error() {
    let old = started();
    let model = command(
        &settle(&old, Terminal::Completed(CompletionKind::MemoryValidated)),
        Command::StartMock,
    )
    .state;
    let old_event = Envelope {
        evidence: EvidenceKind::Model,
        mapping: mapping(&old),
        sequence: u64::MAX,
        event: MockEvent::CleanupFailed(failure("old", "Io", 8)),
    };
    let transition = model.reduce(Input::Event(old_event));
    assert_eq!(transition.state, model);
    assert_eq!(
        transition.disposition,
        Disposition::Ignored(Rejection::StaleOperation)
    );
}

#[test]
fn current_duplicate_sequence_blocks_without_cleanup_attestation() {
    let model = event(&started(), MockEvent::Progress { done: 1, total: 2 }).state;
    let result = model.reduce(Input::Event(Envelope {
        evidence: EvidenceKind::Model,
        mapping: mapping(&model),
        sequence: 0,
        event: MockEvent::CleanupConfirmed,
    }));
    assert_eq!(
        result.disposition,
        Disposition::ConnectionFault(LinkFault::Sequence)
    );
    assert!(!result.state.operation().unwrap().cleanup_confirmed());
}

#[test]
fn current_sequence_gap_blocks_and_does_not_recover_from_later_cleanup() {
    let model = started();
    let model = model
        .reduce(Input::Event(Envelope {
            evidence: EvidenceKind::Model,
            mapping: mapping(&model),
            sequence: 2,
            event: MockEvent::CleanupConfirmed,
        }))
        .state;
    let late = event(&model, MockEvent::CleanupConfirmed);
    assert_eq!(
        late.disposition,
        Disposition::Ignored(Rejection::ConnectionBlocked)
    );
    assert!(!late.state.operation().unwrap().cleanup_confirmed());
}

#[test]
fn sequence_reversal_after_two_events_is_explicit_failure() {
    let model = event(&started(), MockEvent::Progress { done: 0, total: 2 }).state;
    let model = event(&model, MockEvent::Progress { done: 1, total: 2 }).state;
    let bad = model.reduce(Input::Event(Envelope {
        evidence: EvidenceKind::Model,
        mapping: mapping(&model),
        sequence: 0,
        event: MockEvent::Terminal(Terminal::Completed(CompletionKind::MemoryValidated)),
    }));
    assert_eq!(
        bad.disposition,
        Disposition::ConnectionFault(LinkFault::Sequence)
    );
    assert_eq!(bad.state.operation().unwrap().next_sequence, 2);
}

#[test]
fn wrong_job_for_current_ui_is_not_a_valid_mapping() {
    let model = started();
    let mut wrong = mapping(&model);
    wrong.job.0[0] ^= 1;
    let result = model.reduce(Input::Event(Envelope {
        evidence: EvidenceKind::Model,
        mapping: wrong,
        sequence: 0,
        event: MockEvent::CleanupConfirmed,
    }));
    assert_eq!(
        result.disposition,
        Disposition::ConnectionFault(LinkFault::Mapping)
    );
}

#[test]
fn future_ui_id_is_not_treated_as_old_event() {
    let model = started();
    let mut wrong = mapping(&model);
    wrong.ui.0 += 1;
    let result = model.reduce(Input::Event(Envelope {
        evidence: EvidenceKind::Model,
        mapping: wrong,
        sequence: 0,
        event: MockEvent::CleanupConfirmed,
    }));
    assert_eq!(
        result.disposition,
        Disposition::ConnectionFault(LinkFault::Mapping)
    );
}

#[test]
fn conflicting_terminal_blocks_even_after_prior_settlement() {
    let model = settle(
        &started(),
        Terminal::Completed(CompletionKind::MemoryValidated),
    );
    let result = event(&model, MockEvent::Terminal(Terminal::MockCancelled));
    assert_eq!(
        result.disposition,
        Disposition::ConnectionFault(LinkFault::ConflictingTerminal)
    );
    assert!(!result.state.operation().unwrap().settled());
    assert_eq!(command(&result.state, Command::StartMock).effect, None);
}

#[test]
fn repeated_terminal_at_new_sequence_is_explicit_error() {
    let model = event(
        &started(),
        MockEvent::Terminal(Terminal::Completed(CompletionKind::MemoryValidated)),
    )
    .state;
    assert_eq!(
        event(
            &model,
            MockEvent::Terminal(Terminal::Completed(CompletionKind::MemoryValidated))
        )
        .disposition,
        Disposition::ConnectionFault(LinkFault::DuplicateTerminal)
    );
}

#[test]
fn late_progress_after_terminal_is_discarded_but_sequence_is_consumed() {
    let model = event(
        &started(),
        MockEvent::Terminal(Terminal::Completed(CompletionKind::MemoryValidated)),
    )
    .state;
    let result = event(&model, MockEvent::Progress { done: 9, total: 10 });
    assert_eq!(
        result.disposition,
        Disposition::Ignored(Rejection::LateProgress)
    );
    assert_eq!(result.state.operation().unwrap().next_sequence, 2);
    assert_eq!(result.state.operation().unwrap().progress, None);
    assert!(
        event(&result.state, MockEvent::CleanupConfirmed)
            .state
            .operation()
            .unwrap()
            .settled()
    );
}

#[test]
fn success_after_cancel_is_not_adopted() {
    let model = command(&started(), Command::RequestCancel).state;
    let result = event(
        &model,
        MockEvent::Terminal(Terminal::Completed(CompletionKind::MemoryValidated)),
    );
    assert_eq!(
        result.disposition,
        Disposition::ConnectionFault(LinkFault::SuccessAfterCancel)
    );
    assert_eq!(result.state.operation().unwrap().terminal, None);
}

#[test]
fn cancellation_terminal_requires_cleanup_before_retry() {
    let model = command(&started(), Command::RequestCancel).state;
    let model = event(&model, MockEvent::Terminal(Terminal::MockCancelled)).state;
    assert_eq!(command(&model, Command::StartMock).effect, None);
    let model = event(&model, MockEvent::CleanupConfirmed).state;
    assert!(command(&model, Command::StartMock).effect.is_some());
}

#[test]
fn close_then_keep_open_does_not_cancel_operation() {
    let model = started();
    let pending = command(&model, Command::RequestClose);
    assert_eq!(pending.state.close(), CloseState::Confirming);
    assert_eq!(pending.effect, None);
    let kept = command(&pending.state, Command::KeepOpen);
    assert_eq!(kept.state.close(), CloseState::Open);
    assert_eq!(kept.state.operation(), model.operation());
}

#[test]
fn confirm_close_emits_cancel_once_and_waits_for_both_observations() {
    let model = command(&started(), Command::RequestClose).state;
    let cancelled = command(&model, Command::ConfirmCloseCancel);
    assert!(matches!(cancelled.effect, Some(MockEffect::Cancel(_))));
    assert_eq!(
        command(&cancelled.state, Command::ConfirmCloseCancel).effect,
        None
    );
    let cleanup = event(&cancelled.state, MockEvent::CleanupConfirmed);
    assert_eq!(cleanup.effect, None);
    let terminal = event(&cleanup.state, MockEvent::Terminal(Terminal::MockCancelled));
    assert_eq!(terminal.effect, Some(MockEffect::PermitClose));
    assert_eq!(terminal.state.close(), CloseState::MockClosed);
}

#[test]
fn keep_open_after_confirm_does_not_undo_cancellation() {
    let model = command(&started(), Command::RequestClose).state;
    let model = command(&model, Command::ConfirmCloseCancel).state;
    let model = command(&model, Command::KeepOpen).state;
    assert!(model.operation().unwrap().cancel_requested);
    let model = settle(&model, Terminal::MockCancelled);
    assert_eq!(model.close(), CloseState::Open);
}

#[test]
fn terminal_pending_cleanup_close_does_not_send_cancel() {
    let model = event(
        &started(),
        MockEvent::Terminal(Terminal::Completed(CompletionKind::MemoryValidated)),
    )
    .state;
    assert_eq!(
        command(&model, Command::RequestCancel).disposition,
        Disposition::Ignored(Rejection::CancelAfterTerminal)
    );
    let model = command(&model, Command::RequestClose).state;
    let waiting = command(&model, Command::ConfirmCloseCancel);
    assert_eq!(waiting.effect, None);
    assert_eq!(
        event(&waiting.state, MockEvent::CleanupConfirmed).effect,
        Some(MockEffect::PermitClose)
    );
}

#[test]
fn close_on_completed_operation_permits_close_once() {
    let model = settle(
        &started(),
        Terminal::Completed(CompletionKind::MemoryValidated),
    );
    let closed = command(&model, Command::RequestClose);
    assert_eq!(closed.effect, Some(MockEffect::PermitClose));
    assert_eq!(command(&closed.state, Command::RequestClose).effect, None);
    assert_eq!(
        command(&closed.state, Command::StartMock).disposition,
        Disposition::Ignored(Rejection::Closed)
    );
}

#[test]
fn cleanup_failure_prevents_pending_close() {
    let model = command(&started(), Command::RequestClose).state;
    let model = command(&model, Command::ConfirmCloseCancel).state;
    let model = event(&model, MockEvent::Terminal(Terminal::MockCancelled)).state;
    let result = event(&model, MockEvent::CleanupFailed(failure("host", "Io", 5)));
    assert_eq!(result.effect, None);
    assert_eq!(result.state.close(), CloseState::WaitingForCleanup);
}

#[test]
fn confirmation_without_close_request_has_no_effect() {
    assert_eq!(
        command(&started(), Command::ConfirmCloseCancel).disposition,
        Disposition::Ignored(Rejection::NoConfirmation)
    );
}

#[test]
fn close_without_operation_is_only_a_mock_effect() {
    let model = Model::new(*b"mock-one").unwrap();
    assert_eq!(
        command(&model, Command::RequestClose).effect,
        Some(MockEffect::PermitClose)
    );
    assert_eq!(model.close(), CloseState::Open);
}

#[test]
fn names_preserve_hangul_and_jamo_original_bytes() {
    let model = list(&started());
    assert_eq!(model.rows()[0].name.as_bytes(), "한글/한.txt".as_bytes());
    assert_eq!(model.rows()[1].name.as_bytes(), "한글/한.txt".as_bytes());
    assert_ne!(model.rows()[0].name, model.rows()[1].name);
}

#[test]
fn list_replacement_invalidates_old_row_key_even_when_id_repeats() {
    let model = list(&started());
    let key = model.row_key(RowId(7)).unwrap();
    let model = list(&model);
    let result = command(&model, Command::Select(key));
    assert_eq!(
        result.disposition,
        Disposition::Ignored(Rejection::StaleRow)
    );
    assert_eq!(result.state.selected(), None);
}

#[test]
fn new_operation_cannot_reuse_old_row_selection() {
    let model = list(&started());
    let key = model.row_key(RowId(7)).unwrap();
    let next = command(
        &settle(&model, Terminal::Completed(CompletionKind::MemoryValidated)),
        Command::StartMock,
    )
    .state;
    let next = list(&next);
    assert_eq!(
        command(&next, Command::Select(key)).disposition,
        Disposition::Ignored(Rejection::StaleRow)
    );
}

#[test]
fn old_list_install_cannot_replace_new_operations_rows() {
    let model = list(&started());
    let old = mapping(&model).ui;
    let next = command(
        &settle(&model, Terminal::Completed(CompletionKind::MemoryValidated)),
        Command::StartMock,
    )
    .state;
    let result = command(
        &next,
        Command::ReplaceMockList {
            operation: old,
            rows: model.rows().to_vec(),
        },
    );
    assert_eq!(
        result.disposition,
        Disposition::Ignored(Rejection::StaleOperation)
    );
    assert_eq!(result.state, next);
}

#[test]
fn duplicate_row_identity_rejects_list_atomically() {
    let model = list(&started());
    let mut rows = model.rows().to_vec();
    rows[1].id = rows[0].id;
    let result = command(
        &model,
        Command::ReplaceMockList {
            operation: mapping(&model).ui,
            rows,
        },
    );
    assert_eq!(
        result.disposition,
        Disposition::Ignored(Rejection::InvalidList)
    );
    assert_eq!(result.state, model);
}

#[test]
fn text_and_other_key_events_do_not_move_list_selection() {
    let model = list(&started());
    let model = command(&model, Command::Select(model.row_key(RowId(7)).unwrap())).state;
    for area in [InputArea::Text, InputArea::Other] {
        let result = command(
            &model,
            Command::Key {
                area,
                key: Navigation::Last,
            },
        );
        assert_eq!(
            result.disposition,
            Disposition::Ignored(Rejection::InputNotList)
        );
        assert_eq!(result.state, model);
    }
    let result = command(
        &model,
        Command::Key {
            area: InputArea::List,
            key: Navigation::Next,
        },
    );
    assert_eq!(result.state.selected().unwrap().row, RowId(9));
}

#[test]
fn namespace_partitions_mock_job_ids_but_not_ui_serials() {
    let a = started();
    let b = command(&Model::new(*b"mock-two").unwrap(), Command::StartMock).state;
    assert_eq!(mapping(&a).ui, mapping(&b).ui);
    assert_ne!(mapping(&a).job, mapping(&b).job);
    assert_eq!(mapping(&a).job.0[..8], *b"mock-one");
    assert_eq!(mapping(&a).job.0[8..], 1u64.to_be_bytes());
    assert!(Model::new([0; 8]).is_none());
}

#[test]
fn invalid_failure_evidence_is_not_silently_truncated() {
    let model = started();
    let result = event(
        &model,
        MockEvent::Terminal(Terminal::Failed(failure("", "Integrity", 12))),
    );
    assert_eq!(
        result.disposition,
        Disposition::ConnectionFault(LinkFault::InvalidEvidence)
    );
    assert!(result.state.operation().unwrap().terminal.is_none());
}

#[test]
fn cleanup_failure_after_confirmation_revokes_retry_permission() {
    let model = settle(
        &started(),
        Terminal::Completed(CompletionKind::MemoryValidated),
    );
    let model = event(
        &model,
        MockEvent::CleanupFailed(failure("host", "Unconfirmed", 5)),
    )
    .state;
    assert!(!model.operation().unwrap().settled());
    assert_eq!(command(&model, Command::StartMock).effect, None);
}

#[test]
fn closed_model_still_records_current_operation_contradiction() {
    let model = settle(
        &started(),
        Terminal::Completed(CompletionKind::MemoryValidated),
    );
    let model = command(&model, Command::RequestClose).state;
    let result = event(&model, MockEvent::Terminal(Terminal::MockCancelled));
    assert_eq!(
        result.disposition,
        Disposition::ConnectionFault(LinkFault::ConflictingTerminal)
    );
    assert_eq!(result.effect, None);
    assert_eq!(result.state.close(), CloseState::MockClosed);
    assert!(!result.state.operation().unwrap().settled());
}

#[test]
fn cancel_after_finished_error_has_no_effect_and_keeps_origin() {
    let cause = failure("adapter", "Allocation", 99);
    let model = settle(&started(), Terminal::Failed(cause.clone()));
    let result = command(&model, Command::RequestCancel);
    assert_eq!(result.effect, None);
    assert_eq!(
        result.state.operation().unwrap().first_error,
        Some(FirstError::Reported(cause))
    );
}

#[test]
fn cleanup_error_first_is_not_overwritten_by_terminal_error() {
    let cause = failure("supervisor", "Io", 5);
    let model = event(&started(), MockEvent::CleanupFailed(cause.clone())).state;
    let model = event(
        &model,
        MockEvent::Terminal(Terminal::Failed(failure("worker", "Integrity", 12))),
    )
    .state;
    assert_eq!(
        model.operation().unwrap().first_error,
        Some(FirstError::Cleanup(cause))
    );
    assert!(matches!(
        model.operation().unwrap().terminal,
        Some(Terminal::Failed(_))
    ));
}

#[test]
fn namespace_reuse_is_a_caller_collision_not_global_identity_proof() {
    let a = started();
    let b = started();
    assert_eq!(mapping(&a), mapping(&b));
}

#[test]
fn progress_after_cancel_does_not_overwrite_displayed_progress() {
    let model = event(&started(), MockEvent::Progress { done: 2, total: 10 }).state;
    let model = command(&model, Command::RequestCancel).state;
    let result = event(
        &model,
        MockEvent::Progress {
            done: 10,
            total: 10,
        },
    );
    assert_eq!(
        result.disposition,
        Disposition::Ignored(Rejection::LateProgress)
    );
    assert_eq!(result.state.operation().unwrap().progress, Some((2, 10)));
    assert!(result.state.operation().unwrap().terminal.is_none());
}

#[test]
fn completion_kinds_and_evidence_are_preserved_without_upgrade() {
    for kind in [
        CompletionKind::MemoryValidated,
        CompletionKind::ProtocolObserved,
        CompletionKind::Published,
    ] {
        for evidence in [EvidenceKind::Model, EvidenceKind::CallerAttested] {
            let model = started();
            let result = model.reduce(Input::Event(Envelope {
                mapping: mapping(&model),
                sequence: 0,
                evidence,
                event: MockEvent::Terminal(Terminal::Completed(kind)),
            }));
            assert_eq!(
                result.state.operation().unwrap().terminal,
                Some(Terminal::Completed(kind))
            );
            assert_eq!(
                result.state.operation().unwrap().terminal_evidence,
                Some(evidence)
            );
            assert_eq!(result.effect, None);
            assert!(!result.state.operation().unwrap().settled());
        }
    }
}

#[test]
fn protocol_terminal_cannot_be_replaced_by_published_terminal() {
    let model = event(
        &started(),
        MockEvent::Terminal(Terminal::Completed(CompletionKind::ProtocolObserved)),
    )
    .state;
    let result = event(
        &model,
        MockEvent::Terminal(Terminal::Completed(CompletionKind::Published)),
    );
    assert_eq!(
        result.disposition,
        Disposition::ConnectionFault(LinkFault::ConflictingTerminal)
    );
    assert_eq!(
        result.state.operation().unwrap().terminal,
        Some(Terminal::Completed(CompletionKind::ProtocolObserved))
    );
}

#[test]
fn memory_operation_requires_explicit_not_applicable_cleanup() {
    let model = command(
        &Model::new(*b"memory-1").unwrap(),
        Command::StartMockFor(OperationKind::Memory),
    )
    .state;
    let model = event(
        &model,
        MockEvent::Terminal(Terminal::Completed(CompletionKind::MemoryValidated)),
    )
    .state;
    assert_eq!(command(&model, Command::StartMock).effect, None);
    let model = event(&model, MockEvent::CleanupNotApplicable).state;
    let op = model.operation().unwrap();
    assert_eq!(
        op.cleanup,
        Some(CleanupObservation {
            kind: CleanupKind::NotApplicable,
            evidence: EvidenceKind::Model
        })
    );
    assert!(!op.cleanup_confirmed());
    assert!(op.settled());
    assert!(command(&model, Command::StartMock).effect.is_some());
}

#[test]
fn process_operation_cannot_use_memory_cleanup_not_applicable() {
    let result = event(&started(), MockEvent::CleanupNotApplicable);
    assert_eq!(
        result.disposition,
        Disposition::ConnectionFault(LinkFault::CleanupKindMismatch)
    );
    assert_eq!(result.state.operation().unwrap().cleanup, None);
}

#[test]
fn memory_operation_cannot_mislabel_cleanup_as_process_confirmation() {
    let model = command(
        &Model::new(*b"memory-1").unwrap(),
        Command::StartMockFor(OperationKind::Memory),
    )
    .state;
    let result = event(&model, MockEvent::CleanupConfirmed);
    assert_eq!(
        result.disposition,
        Disposition::ConnectionFault(LinkFault::CleanupKindMismatch)
    );
    assert_eq!(result.state.operation().unwrap().cleanup, None);
}

#[test]
fn terminal_and_cleanup_retain_their_independent_evidence() {
    let model = event(
        &started(),
        MockEvent::Terminal(Terminal::Completed(CompletionKind::ProtocolObserved)),
    )
    .state;
    let result = model.reduce(Input::Event(Envelope {
        mapping: mapping(&model),
        sequence: 1,
        evidence: EvidenceKind::CallerAttested,
        event: MockEvent::CleanupConfirmed,
    }));
    let op = result.state.operation().unwrap();
    assert_eq!(op.terminal_evidence, Some(EvidenceKind::Model));
    assert_eq!(
        op.cleanup,
        Some(CleanupObservation {
            kind: CleanupKind::Confirmed,
            evidence: EvidenceKind::CallerAttested
        })
    );
    assert!(op.settled());
}

#[test]
fn first_failure_policy_and_later_cleanup_failure_are_separate() {
    let cause = Failure {
        origin: "typed-adapter".into(),
        policy: "mock-profile-only".into(),
        classification: "StreamError".into(),
        code: 19,
    };
    let model = event(
        &started(),
        MockEvent::Terminal(Terminal::Failed(cause.clone())),
    )
    .state;
    let cleanup = failure("mock-supervisor", "CleanupFailed", 5);
    let result = model.reduce(Input::Event(Envelope {
        mapping: mapping(&model),
        sequence: 1,
        evidence: EvidenceKind::CallerAttested,
        event: MockEvent::CleanupFailed(cleanup.clone()),
    }));
    let op = result.state.operation().unwrap();
    assert_eq!(op.first_error, Some(FirstError::Reported(cause.clone())));
    assert_eq!(op.terminal, Some(Terminal::Failed(cause)));
    assert_eq!(op.cleanup_failed, Some(cleanup));
    assert_eq!(
        op.cleanup_failure_evidence,
        Some(EvidenceKind::CallerAttested)
    );
}

#[test]
fn missing_policy_is_invalid_evidence_not_a_default_profile() {
    let mut cause = failure("adapter", "Integrity", 12);
    cause.policy.clear();
    assert_eq!(
        event(&started(), MockEvent::Terminal(Terminal::Failed(cause))).disposition,
        Disposition::ConnectionFault(LinkFault::InvalidEvidence)
    );
}

#[test]
fn cancellation_blocks_all_mock_completion_kinds() {
    for kind in [
        CompletionKind::MemoryValidated,
        CompletionKind::ProtocolObserved,
        CompletionKind::Published,
    ] {
        let model = command(&started(), Command::RequestCancel).state;
        let result = event(&model, MockEvent::Terminal(Terminal::Completed(kind)));
        assert_eq!(
            result.disposition,
            Disposition::ConnectionFault(LinkFault::SuccessAfterCancel)
        );
        assert!(result.state.operation().unwrap().terminal.is_none());
    }
}
