use std::time::Duration;
use zipelo_rust_worker_session_model::{
    ApplyError, Channel, ExitStatus, Fault, Limits, Observation, Outcome, Phase, SessionModel,
    wire::{CHUNK_BYTES, Header, Kind, ProtocolError, RequestId, WorkerStatus, encode_header},
};

const NOW: Duration = Duration::ZERO;

fn id() -> RequestId {
    RequestId::new([7; 16]).expect("nonzero id")
}

fn session(input: &[u8]) -> SessionModel<'_> {
    SessionModel::new(id(), input, Limits::default()).expect("valid model")
}

fn sent(model: &mut SessionModel<'_>) {
    while let Some(frame) = model.outbound() {
        let sequence = Header::inspect(&frame.header).expect("header").sequence();
        model
            .request_frame_sent(NOW, sequence)
            .expect("synthetic frame acknowledgement");
    }
}

fn frame(
    model: &mut SessionModel<'_>,
    kind: Kind,
    sequence: u64,
    payload: &[u8],
) -> Result<(), ApplyError> {
    let header = encode_header(kind, id(), sequence, payload.len() as u32);
    model.observe(
        NOW,
        Observation::ResultFrame {
            header: &header,
            payload,
        },
    )
}

fn completed(model: &mut SessionModel<'_>, payload: &[u8]) {
    frame(
        model,
        Kind::ResultBegin,
        0,
        &(payload.len() as u64).to_le_bytes(),
    )
    .unwrap();
    let mut sequence = 1;
    if !payload.is_empty() {
        frame(model, Kind::ResultData, sequence, payload).unwrap();
        sequence += 1;
    }
    frame(
        model,
        Kind::ResultEnd,
        sequence,
        &[WorkerStatus::Completed as u8],
    )
    .unwrap();
}

fn settle(model: &mut SessionModel<'_>, code: u32) {
    settle_status(model, ExitStatus::Code(code));
}

fn settle_status(model: &mut SessionModel<'_>, status: ExitStatus) {
    for event in [
        Observation::Closed(Channel::Request),
        Observation::ResultEof,
        Observation::DiagnosticEof,
        Observation::Exit(status),
        Observation::Reaped(status),
        Observation::Closed(Channel::Result),
        Observation::Closed(Channel::Diagnostic),
        Observation::ResourcesReleased,
    ] {
        model.observe(NOW, event).unwrap();
    }
}

#[test]
fn header_round_trip_uses_opaque_id_and_little_endian_sequence() {
    let bytes = encode_header(Kind::ResultData, id(), 0x0102030405060708, 12);
    let header = Header::inspect(&bytes).unwrap();
    assert_eq!(header.id(), id());
    assert_eq!(header.sequence(), 0x0102030405060708);
    assert_eq!(&bytes[32..40], &[8, 7, 6, 5, 4, 3, 2, 1]);
    assert_eq!(header.payload_length(), 12);
}

#[test]
fn zero_correlation_id_is_rejected() {
    assert_eq!(RequestId::new([0; 16]), None);
}

#[test]
fn wrong_magic_version_and_reserved_bytes_are_rejected() {
    let bytes = encode_header(Kind::ResultBegin, id(), 0, 8);
    for index in [0, 4, 12, 40, 63] {
        let mut bad = bytes;
        bad[index] ^= 1;
        assert_eq!(Header::inspect(&bad), Err(ProtocolError::InvalidHeader));
    }
}

#[test]
fn invalid_frame_lengths_are_rejected_before_body_use() {
    for (kind, length) in [
        (Kind::ResultData, 0),
        (Kind::ResultData, 65537),
        (Kind::ResultBegin, 7),
        (Kind::ResultEnd, 2),
        (Kind::RequestEnd, 1),
    ] {
        assert_eq!(
            Header::inspect(&encode_header(kind, id(), 0, length)),
            Err(ProtocolError::InvalidLength)
        );
    }
}

#[test]
fn request_chunks_borrow_host_bytes_without_paths() {
    let input = vec![42; CHUNK_BYTES + 1];
    let mut model = session(&input);
    assert_eq!(model.outbound().unwrap().payload.len(), 16);
    model.request_frame_sent(NOW, 0).unwrap();
    let data = model.outbound().unwrap();
    assert_eq!(data.payload.as_ptr(), input.as_ptr());
    assert_eq!(data.payload.len(), CHUNK_BYTES);
    model.request_frame_sent(NOW, 1).unwrap();
    assert_eq!(model.outbound().unwrap().payload, &[42]);
    model.request_frame_sent(NOW, 2).unwrap();
    assert!(model.outbound().unwrap().payload.is_empty());
    model.request_frame_sent(NOW, 3).unwrap();
    assert!(model.outbound().is_none());
}

#[test]
fn request_ack_sequence_mismatch_latches_failure() {
    let mut model = session(b"x");
    assert!(model.request_frame_sent(NOW, 1).is_err());
    assert_eq!(
        model.report().first_fault,
        Some(Fault::Protocol(ProtocolError::SequenceMismatch))
    );
    assert!(model.outbound().is_none());
}

#[test]
fn coherent_trace_has_no_execution_or_publication_authority() {
    let mut model = session(b"synthetic gzip input");
    sent(&mut model);
    completed(&mut model, b"unverified worker bytes");
    settle(&mut model, 0);
    let report = model.report();
    assert_eq!(report.phase, Phase::SettledModel);
    assert_eq!(report.outcome, Outcome::ProtocolCompletionObserved);
    assert!(!report.live_verified());
    assert!(!report.publication_allowed());
}

#[test]
fn missing_eof_stays_pending_after_terminal_and_exit() {
    let mut model = session(b"");
    sent(&mut model);
    completed(&mut model, b"");
    model
        .observe(NOW, Observation::Exit(ExitStatus::Code(0)))
        .unwrap();
    model
        .observe(NOW, Observation::Reaped(ExitStatus::Code(0)))
        .unwrap();
    assert_eq!(model.report().outcome, Outcome::Pending);
    assert!(model.report().missing.result_eof);
}

#[test]
fn closing_reader_does_not_manufacture_eof() {
    let mut model = session(b"");
    assert!(
        model
            .observe(NOW, Observation::Closed(Channel::Result))
            .is_err()
    );
    assert_eq!(model.report().phase, Phase::Blocked);
    assert!(model.report().missing.result_eof);
}

#[test]
fn reap_without_observed_exit_is_contradictory() {
    let mut model = session(b"");
    assert!(
        model
            .observe(NOW, Observation::Reaped(ExitStatus::Code(0)))
            .is_err()
    );
    assert_eq!(model.report().phase, Phase::Blocked);
}

#[test]
fn mismatched_reap_status_is_rejected() {
    let mut model = session(b"");
    model
        .observe(NOW, Observation::Exit(ExitStatus::Code(0)))
        .unwrap();
    assert!(model.outbound().is_none());
    assert!(!model.report().stop_needed_in_model);
    assert!(
        model
            .observe(NOW, Observation::Reaped(ExitStatus::Code(7)))
            .is_err()
    );
    assert_eq!(model.report().phase, Phase::Blocked);
}

#[test]
fn completed_report_with_nonzero_exit_is_failure() {
    let mut model = session(b"");
    sent(&mut model);
    completed(&mut model, b"");
    settle(&mut model, 19);
    assert_eq!(model.report().first_fault, Some(Fault::ExitContradiction));
}

#[test]
fn failed_worker_report_with_zero_exit_is_failure() {
    let mut model = session(b"");
    sent(&mut model);
    frame(&mut model, Kind::ResultBegin, 0, &0_u64.to_le_bytes()).unwrap();
    frame(
        &mut model,
        Kind::ResultEnd,
        1,
        &[WorkerStatus::Failed as u8],
    )
    .unwrap();
    settle(&mut model, 0);
    assert_eq!(model.report().first_fault, Some(Fault::ExitContradiction));
}

#[test]
fn worker_failure_remains_untrusted_worker_report() {
    let mut model = session(b"");
    sent(&mut model);
    frame(&mut model, Kind::ResultBegin, 0, &0_u64.to_le_bytes()).unwrap();
    frame(
        &mut model,
        Kind::ResultEnd,
        1,
        &[WorkerStatus::Failed as u8],
    )
    .unwrap();
    settle(&mut model, 12);
    assert_eq!(
        model.report().outcome,
        Outcome::WorkerReported(WorkerStatus::Failed)
    );
    assert!(!model.report().publication_allowed());
}

#[test]
fn result_identity_and_sequence_mismatches_are_rejected() {
    for wrong_id in [false, true] {
        let mut model = session(b"");
        sent(&mut model);
        let other = RequestId::new([9; 16]).unwrap();
        let header = encode_header(
            Kind::ResultBegin,
            if wrong_id { other } else { id() },
            if wrong_id { 0 } else { 1 },
            8,
        );
        assert!(
            model
                .observe(
                    NOW,
                    Observation::ResultFrame {
                        header: &header,
                        payload: &0_u64.to_le_bytes()
                    }
                )
                .is_err()
        );
        assert_eq!(
            model.report().first_fault,
            Some(Fault::Protocol(if wrong_id {
                ProtocolError::IdMismatch
            } else {
                ProtocolError::SequenceMismatch
            }))
        );
    }
}

#[test]
fn request_frame_on_result_channel_is_rejected() {
    let mut model = session(b"");
    sent(&mut model);
    assert!(frame(&mut model, Kind::RequestEnd, 0, b"").is_err());
    assert_eq!(
        model.report().first_fault,
        Some(Fault::Protocol(ProtocolError::UnexpectedFrame))
    );
}

#[test]
fn output_declaration_above_limit_is_rejected() {
    let mut model = session(b"");
    sent(&mut model);
    assert!(
        frame(
            &mut model,
            Kind::ResultBegin,
            0,
            &(16_u64 * 1024 * 1024 + 1).to_le_bytes()
        )
        .is_err()
    );
    assert_eq!(model.report().usage.result_data_bytes, 0);
}

#[test]
fn output_data_above_declaration_is_rejected() {
    let mut model = session(b"");
    sent(&mut model);
    frame(&mut model, Kind::ResultBegin, 0, &0_u64.to_le_bytes()).unwrap();
    assert!(frame(&mut model, Kind::ResultData, 1, b"x").is_err());
    assert_eq!(model.report().usage.result_data_bytes, 0);
}

#[test]
fn completed_with_short_output_is_truncated() {
    let mut model = session(b"");
    sent(&mut model);
    frame(&mut model, Kind::ResultBegin, 0, &1_u64.to_le_bytes()).unwrap();
    assert!(frame(&mut model, Kind::ResultEnd, 1, &[1]).is_err());
    assert_eq!(
        model.report().first_fault,
        Some(Fault::Protocol(ProtocolError::Truncated))
    );
}

#[test]
fn frame_after_terminal_is_trailing_data() {
    let mut model = session(b"");
    sent(&mut model);
    completed(&mut model, b"");
    let before = model.report().usage;
    assert!(frame(&mut model, Kind::ResultEnd, 2, &[1]).is_err());
    let after = model.report().usage;
    assert_eq!(after.result_wire_bytes, before.result_wire_bytes);
    assert_eq!(after.result_frames, before.result_frames);
    assert_eq!(after.result_data_bytes, before.result_data_bytes);
    assert_eq!(after.request_wire_bytes, before.request_wire_bytes);
    assert_eq!(after.request_frames, before.request_frames);
    assert_eq!(after.diagnostic_bytes, before.diagnostic_bytes);
    assert_eq!(after.observations, before.observations + 1);
    assert_eq!(
        model.report().first_fault,
        Some(Fault::Protocol(ProtocolError::TrailingData))
    );
}

#[test]
fn cancellation_first_cause_survives_io_and_signal_observations() {
    let mut model = session(b"");
    sent(&mut model);
    model.observe(NOW, Observation::Cancel).unwrap();
    model
        .observe(NOW, Observation::StopAttempted { accepted: true })
        .unwrap();
    assert!(model.report().missing.reap);
    assert!(model.observe(NOW, Observation::IoFailure).is_err());
    settle_status(&mut model, ExitStatus::Signal(9));
    assert_eq!(model.report().observed_exit, Some(ExitStatus::Signal(9)));
    assert!(!model.report().missing.reap);
    assert_eq!(model.report().first_fault, Some(Fault::Cancelled));
    assert_eq!(model.report().phase, Phase::SettledModel);
}

#[test]
fn cleanup_expiry_keeps_missing_reap_blocked() {
    let mut model = session(b"");
    model.observe(NOW, Observation::Cancel).unwrap();
    assert!(
        model
            .observe(Duration::from_secs(30), Observation::Tick)
            .is_err()
    );
    assert_eq!(model.report().phase, Phase::Blocked);
    assert_eq!(model.report().first_fault, Some(Fault::Cancelled));
    assert!(model.report().missing.reap);
}

#[test]
fn observation_cap_does_not_manufacture_cleanup_settlement() {
    let mut model = SessionModel::new(
        id(),
        b"",
        Limits {
            max_observations: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    model.observe(NOW, Observation::Tick).unwrap();
    assert!(model.observe(NOW, Observation::ResourcesReleased).is_err());
    assert_eq!(model.report().phase, Phase::Blocked);
    assert!(model.report().missing.resources_released);
}

#[test]
fn reversed_clock_blocks_trace() {
    let mut model = session(b"");
    model
        .observe(Duration::from_secs(1), Observation::Tick)
        .unwrap();
    assert!(model.observe(NOW, Observation::Tick).is_err());
    assert_eq!(model.report().phase, Phase::Blocked);
}

#[test]
fn diagnostic_bytes_are_bounded_and_never_parsed_as_result() {
    let mut model = session(b"");
    model
        .observe(NOW, Observation::DiagnosticBytes(&[0; 4096]))
        .unwrap();
    assert!(
        model
            .observe(NOW, Observation::DiagnosticBytes(b"x"))
            .is_err()
    );
    assert_eq!(model.report().first_fault, Some(Fault::DiagnosticExceeded));
    assert_eq!(model.report().usage.result_frames, 0);
}

#[test]
fn small_frame_budget_counts_begin_and_end() {
    let mut model = SessionModel::new(
        id(),
        b"",
        Limits {
            max_frames_per_direction: 2,
            ..Limits::default()
        },
    )
    .unwrap();
    sent(&mut model);
    frame(&mut model, Kind::ResultBegin, 0, &1_u64.to_le_bytes()).unwrap();
    frame(&mut model, Kind::ResultData, 1, b"x").unwrap();
    assert!(frame(&mut model, Kind::ResultEnd, 2, &[1]).is_err());
    assert_eq!(
        model.report().first_fault,
        Some(Fault::Protocol(ProtocolError::FramesExceeded))
    );
}

#[test]
fn small_wire_budget_counts_header_overhead() {
    let mut model = SessionModel::new(
        id(),
        b"",
        Limits {
            max_wire_bytes_per_direction: 144,
            ..Limits::default()
        },
    )
    .unwrap();
    sent(&mut model);
    frame(&mut model, Kind::ResultBegin, 0, &10_u64.to_le_bytes()).unwrap();
    assert!(frame(&mut model, Kind::ResultData, 1, &[0; 10]).is_err());
    assert_eq!(
        model.report().first_fault,
        Some(Fault::Protocol(ProtocolError::WireExceeded))
    );
}

#[test]
fn empty_output_and_empty_input_are_valid_model_lengths() {
    let mut model = session(b"");
    sent(&mut model);
    completed(&mut model, b"");
    settle(&mut model, 0);
    assert_eq!(model.report().phase, Phase::SettledModel);
    assert_eq!(model.report().usage.result_data_bytes, 0);
}

#[test]
fn late_cancellation_does_not_rewrite_settled_report() {
    let mut model = session(b"");
    sent(&mut model);
    completed(&mut model, b"");
    settle(&mut model, 0);
    let before = model.report();
    assert_eq!(
        model.observe(Duration::from_secs(999), Observation::Cancel),
        Err(ApplyError::AlreadySettled)
    );
    assert_eq!(model.report(), before);
}

#[test]
fn invalid_limit_configuration_is_rejected() {
    for limits in [
        Limits {
            max_input_bytes: 0,
            ..Limits::default()
        },
        Limits {
            operation_duration: Duration::ZERO,
            ..Limits::default()
        },
        Limits {
            cleanup_duration: Duration::from_secs(31),
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            SessionModel::new(id(), b"x", limits),
            Err(Fault::InvalidLimits)
        ));
    }
}
