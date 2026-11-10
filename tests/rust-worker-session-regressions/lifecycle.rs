use crate::support::*;
use std::time::Duration;

#[test]
fn all_observations_and_ack_after_settlement_leave_report_unchanged() {
    let mut m = model();
    complete(&mut m);
    cleanup(&mut m, 0);
    let before = m.report();
    let h = encode_header(Kind::ResultEnd, id(), 2, 1);
    for e in [
        Observation::ResultFrame {
            header: &h,
            payload: &[1],
        },
        Observation::DiagnosticBytes(b"x"),
        Observation::ResultEof,
        Observation::DiagnosticEof,
        Observation::Exit(ExitStatus::Code(0)),
        Observation::Reaped(ExitStatus::Code(0)),
        Observation::Closed(Channel::Request),
        Observation::Closed(Channel::Result),
        Observation::Closed(Channel::Diagnostic),
        Observation::ResourcesReleased,
        Observation::StopAttempted { accepted: false },
        Observation::TruncatedFrame,
        Observation::IoFailure,
        Observation::Cancel,
        Observation::Tick,
    ] {
        assert_eq!(
            apply(&mut m, Duration::from_secs(999), e),
            Err(ApplyError::AlreadySettled)
        );
        assert_eq!(m.report(), before);
    }
    assert_eq!(ack(&mut m, ZERO, 0), Err(ApplyError::AlreadySettled));
    assert_eq!(m.report(), before);
}
#[test]
fn blocked_trace_rejects_further_mutations_without_accounting_changes() {
    let mut m = model();
    assert!(apply(&mut m, ZERO, Observation::ResourcesReleased).is_err());
    let before = m.report();
    for e in [
        Observation::Cancel,
        Observation::Tick,
        Observation::ResultEof,
        Observation::Exit(ExitStatus::Code(0)),
    ] {
        assert_eq!(
            apply(&mut m, Duration::from_secs(999), e),
            Err(ApplyError::UnreliableObservations)
        );
        assert_eq!(m.report(), before);
    }
    assert_eq!(
        ack(&mut m, ZERO, 0),
        Err(ApplyError::UnreliableObservations)
    );
    assert_eq!(m.report(), before);
}
#[test]
fn duplicate_eof_observations_block_without_hiding_first_cancel() {
    for e in [Observation::ResultEof, Observation::DiagnosticEof] {
        let mut m = model();
        apply(&mut m, ZERO, Observation::Cancel).unwrap();
        apply(&mut m, ZERO, e).unwrap();
        assert!(apply(&mut m, ZERO, e).is_err());
        fault(&m, Fault::Cancelled);
        blocked(&m);
    }
}
#[test]
fn duplicate_exit_or_reap_blocks_trace() {
    for duplicate_reap in [false, true] {
        let mut m = model();
        sent(&mut m);
        apply(&mut m, ZERO, Observation::Exit(ExitStatus::Code(7))).unwrap();
        let e = if duplicate_reap {
            Observation::Reaped(ExitStatus::Code(7))
        } else {
            Observation::Exit(ExitStatus::Code(7))
        };
        if duplicate_reap {
            apply(&mut m, ZERO, e).unwrap();
        }
        assert!(apply(&mut m, ZERO, e).is_err());
        blocked(&m);
        assert!(!m.report().stop_needed_in_model);
    }
}
#[test]
fn duplicate_channel_closure_is_unreliable() {
    for channel in [Channel::Request, Channel::Result, Channel::Diagnostic] {
        let mut m = model();
        complete(&mut m);
        apply(&mut m, ZERO, Observation::ResultEof).unwrap();
        apply(&mut m, ZERO, Observation::DiagnosticEof).unwrap();
        apply(&mut m, ZERO, Observation::Closed(channel)).unwrap();
        assert!(apply(&mut m, ZERO, Observation::Closed(channel)).is_err());
        blocked(&m);
    }
}
#[test]
fn reader_closure_before_eof_blocks_both_channels() {
    for channel in [Channel::Result, Channel::Diagnostic] {
        let mut m = model();
        complete(&mut m);
        assert!(apply(&mut m, ZERO, Observation::Closed(channel)).is_err());
        fault(&m, Fault::ObservationOrder);
        blocked(&m);
        assert!(m.report().missing.result_eof && m.report().missing.diagnostic_eof);
    }
}
#[test]
fn request_closed_before_end_disables_outbound_and_latches_incomplete() {
    let mut m = model();
    apply(&mut m, ZERO, Observation::Closed(Channel::Request)).unwrap();
    fault(&m, Fault::RequestIncomplete);
    assert!(m.outbound().is_none());
    assert!(!m.report().missing.request_closed);
}
#[test]
fn premature_exit_disables_outbound_and_does_not_recommend_stop() {
    let mut m = model();
    apply(&mut m, ZERO, Observation::Exit(ExitStatus::Code(9))).unwrap();
    fault(&m, Fault::RequestIncomplete);
    assert!(m.outbound().is_none());
    assert!(!m.report().stop_needed_in_model);
    assert!(m.report().missing.reap && m.report().missing.result_eof);
}
#[test]
fn exit_before_buffered_result_is_allowed_after_request_end() {
    let mut m = model();
    sent(&mut m);
    apply(&mut m, ZERO, Observation::Exit(ExitStatus::Code(0))).unwrap();
    begin(&mut m, 0);
    frame(&mut m, Kind::ResultEnd, 1, &[1]).unwrap();
    for e in [
        Observation::ResultEof,
        Observation::DiagnosticEof,
        Observation::Reaped(ExitStatus::Code(0)),
        Observation::Closed(Channel::Request),
        Observation::Closed(Channel::Result),
        Observation::Closed(Channel::Diagnostic),
        Observation::ResourcesReleased,
    ] {
        apply(&mut m, ZERO, e).unwrap();
    }
    assert_eq!(m.report().outcome, Outcome::ProtocolCompletionObserved);
}
#[test]
fn signal_zero_is_invalid_and_other_signals_contradict_completion() {
    let mut m = model();
    sent(&mut m);
    assert!(apply(&mut m, ZERO, Observation::Exit(ExitStatus::Signal(0))).is_err());
    assert_eq!(m.report().observed_exit, None);
    blocked(&m);
    let mut n = model();
    complete(&mut n);
    apply(&mut n, ZERO, Observation::ResultEof).unwrap();
    apply(&mut n, ZERO, Observation::Exit(ExitStatus::Signal(9))).unwrap();
    fault(&n, Fault::ExitContradiction);
}
#[test]
fn worker_status_exit_matrix_preserves_only_coherent_untrusted_reports() {
    for status in [
        WorkerStatus::Completed,
        WorkerStatus::Failed,
        WorkerStatus::Cancelled,
    ] {
        for code in [0, 1, u32::MAX] {
            let mut m = model();
            sent(&mut m);
            begin(&mut m, 0);
            frame(&mut m, Kind::ResultEnd, 1, &[status as u8]).unwrap();
            cleanup(&mut m, code);
            if (status == WorkerStatus::Completed) == (code == 0) {
                assert_eq!(m.report().first_fault, None);
                assert_eq!(m.report().phase, Phase::SettledModel);
            } else {
                fault(&m, Fault::ExitContradiction);
            }
            authority(m.report());
        }
    }
}
#[test]
fn resources_require_matching_reap_and_every_channel_closure() {
    for missing in 0..4 {
        let mut m = model();
        complete(&mut m);
        apply(&mut m, ZERO, Observation::ResultEof).unwrap();
        apply(&mut m, ZERO, Observation::DiagnosticEof).unwrap();
        apply(&mut m, ZERO, Observation::Exit(ExitStatus::Code(0))).unwrap();
        if missing != 0 {
            apply(&mut m, ZERO, Observation::Reaped(ExitStatus::Code(0))).unwrap();
        }
        for (index, channel) in [Channel::Request, Channel::Result, Channel::Diagnostic]
            .into_iter()
            .enumerate()
        {
            if missing != index + 1 {
                apply(&mut m, ZERO, Observation::Closed(channel)).unwrap();
            }
        }
        assert!(apply(&mut m, ZERO, Observation::ResourcesReleased).is_err());
        assert!(m.report().missing.resources_released);
        blocked(&m);
    }
}
#[test]
fn missing_release_keeps_fully_closed_and_reaped_trace_pending() {
    let mut m = model();
    complete(&mut m);
    for e in [
        Observation::ResultEof,
        Observation::DiagnosticEof,
        Observation::Exit(ExitStatus::Code(0)),
        Observation::Reaped(ExitStatus::Code(0)),
        Observation::Closed(Channel::Request),
        Observation::Closed(Channel::Result),
        Observation::Closed(Channel::Diagnostic),
    ] {
        apply(&mut m, ZERO, e).unwrap();
    }
    assert_eq!(m.report().outcome, Outcome::Pending);
    assert!(m.report().missing.resources_released);
    assert!(!m.report().missing.reap);
}
#[test]
fn accepted_stop_never_supplies_exit_reap_or_eof() {
    let mut m = model();
    apply(&mut m, ZERO, Observation::Cancel).unwrap();
    assert!(m.report().stop_needed_in_model);
    apply(&mut m, ZERO, Observation::StopAttempted { accepted: true }).unwrap();
    assert!(!m.report().stop_needed_in_model);
    assert!(
        m.report().missing.exit
            && m.report().missing.reap
            && m.report().missing.result_eof
            && m.report().missing.diagnostic_eof
    );
    assert_eq!(m.report().phase, Phase::Stopping);
}
#[test]
fn failed_or_duplicate_stop_blocks_while_preserving_first_fault() {
    for accepted in [false, true] {
        let mut m = model();
        apply(&mut m, ZERO, Observation::Cancel).unwrap();
        let result = apply(&mut m, ZERO, Observation::StopAttempted { accepted });
        if accepted {
            result.unwrap();
            assert!(apply(&mut m, ZERO, Observation::StopAttempted { accepted: true }).is_err());
        } else {
            assert_eq!(result, Err(ApplyError::Fault(Fault::StopFailed)));
        }
        fault(&m, Fault::Cancelled);
        blocked(&m);
    }
}
#[test]
fn stop_without_fault_or_after_exit_is_rejected() {
    let mut m = model();
    assert!(apply(&mut m, ZERO, Observation::StopAttempted { accepted: true }).is_err());
    blocked(&m);
    let mut n = model();
    apply(&mut n, ZERO, Observation::Cancel).unwrap();
    apply(&mut n, ZERO, Observation::Exit(ExitStatus::Code(9))).unwrap();
    assert!(apply(&mut n, ZERO, Observation::StopAttempted { accepted: true }).is_err());
    fault(&n, Fault::Cancelled);
    blocked(&n);
}
#[test]
fn first_io_or_truncation_fault_survives_cancel_and_cleanup() {
    for (event, expected) in [
        (Observation::IoFailure, Fault::Io),
        (
            Observation::TruncatedFrame,
            Fault::Protocol(ProtocolError::Truncated),
        ),
    ] {
        let mut m = model();
        sent(&mut m);
        assert!(apply(&mut m, ZERO, event).is_err());
        apply(&mut m, ZERO, Observation::Cancel).unwrap();
        cleanup(&mut m, 9);
        fault(&m, expected);
        assert_eq!(m.report().phase, Phase::SettledModel);
    }
}
#[test]
fn empty_diagnostic_and_bytes_after_eof_block_trace() {
    let mut m = model();
    assert!(apply(&mut m, ZERO, Observation::DiagnosticBytes(b"")).is_err());
    blocked(&m);
    let mut n = model();
    apply(&mut n, ZERO, Observation::DiagnosticEof).unwrap();
    assert!(apply(&mut n, ZERO, Observation::DiagnosticBytes(b"x")).is_err());
    blocked(&n);
    assert_eq!(n.report().usage.diagnostic_bytes, 0);
}
#[test]
fn every_missing_confirmation_is_visible_at_construction() {
    let m = model();
    let r = m.report();
    assert!(r.missing.result_eof && r.missing.diagnostic_eof && r.missing.exit && r.missing.reap);
    assert!(
        r.missing.request_closed
            && r.missing.result_closed
            && r.missing.diagnostic_closed
            && r.missing.resources_released
    );
    assert_eq!(r.phase, Phase::SendingRequest);
    assert_eq!(r.outcome, Outcome::Pending);
    authority(r);
}
