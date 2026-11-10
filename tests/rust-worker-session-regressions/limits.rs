use crate::support::*;
use std::time::Duration;

#[test]
fn every_configuration_upper_and_lower_boundary_is_validated() {
    let d = Limits::default();
    for limits in [
        Limits {
            max_input_bytes: MAX_DATA_BYTES + 1,
            ..d
        },
        Limits {
            max_output_bytes: MAX_DATA_BYTES + 1,
            ..d
        },
        Limits {
            max_wire_bytes_per_direction: 128,
            ..d
        },
        Limits {
            max_wire_bytes_per_direction: MAX_WIRE_BYTES + 1,
            ..d
        },
        Limits {
            max_frames_per_direction: 1,
            ..d
        },
        Limits {
            max_frames_per_direction: MAX_FRAMES + 1,
            ..d
        },
        Limits {
            max_observations: 0,
            ..d
        },
        Limits {
            max_observations: MAX_OBSERVATIONS + 1,
            ..d
        },
        Limits {
            operation_duration: Duration::ZERO,
            ..d
        },
        Limits {
            operation_duration: Duration::from_secs(301),
            ..d
        },
        Limits {
            cleanup_duration: Duration::ZERO,
            ..d
        },
        Limits {
            cleanup_duration: Duration::from_secs(31),
            ..d
        },
    ] {
        assert!(matches!(
            SessionModel::new(id(), b"", limits),
            Err(Fault::InvalidLimits)
        ));
    }
    assert!(SessionModel::new(id(), b"", d).is_ok());
}
#[test]
fn zero_data_limits_support_empty_model_and_deny_nonempty_input() {
    let limits = Limits {
        max_input_bytes: 0,
        max_output_bytes: 0,
        ..Limits::default()
    };
    assert!(matches!(
        SessionModel::new(id(), b"x", limits),
        Err(Fault::InvalidLimits)
    ));
    let mut m = SessionModel::new(id(), b"", limits).unwrap();
    complete(&mut m);
    cleanup(&mut m, 0);
    assert_eq!(m.report().outcome, Outcome::ProtocolCompletionObserved);
}
#[test]
fn canonical_request_cost_exact_and_one_short() {
    let input = vec![5; CHUNK_BYTES + 1];
    let cost = input.len() as u64 + 16 + 4 * 64;
    let limits = Limits {
        max_input_bytes: input.len() as u64,
        max_frames_per_direction: 4,
        max_wire_bytes_per_direction: cost,
        ..Limits::default()
    };
    let mut m = SessionModel::new(id(), &input, limits).unwrap();
    sent(&mut m);
    assert_eq!(m.report().usage.request_frames, 4);
    assert_eq!(m.report().usage.request_wire_bytes, cost);
    for short in [
        Limits {
            max_input_bytes: input.len() as u64 - 1,
            ..limits
        },
        Limits {
            max_frames_per_direction: 3,
            ..limits
        },
        Limits {
            max_wire_bytes_per_direction: cost - 1,
            ..limits
        },
    ] {
        assert!(matches!(
            SessionModel::new(id(), &input, short),
            Err(Fault::InvalidLimits)
        ));
    }
}
#[test]
fn acknowledgement_rejects_sequence_without_charging_request() {
    let mut m = model();
    assert!(ack(&mut m, ZERO, u64::MAX).is_err());
    let r = m.report();
    assert_eq!(r.usage.request_frames, 0);
    assert_eq!(r.usage.request_wire_bytes, 0);
    assert_eq!(r.usage.observations, 1);
    protocol(&m, ProtocolError::SequenceMismatch);
}
#[test]
fn exact_result_wire_budget_succeeds_one_short_retains_prior_cost() {
    for limit in [207, 208] {
        let mut m = SessionModel::new(
            id(),
            b"",
            Limits {
                max_wire_bytes_per_direction: limit,
                ..Limits::default()
            },
        )
        .unwrap();
        sent(&mut m);
        begin(&mut m, 7);
        frame(&mut m, Kind::ResultData, 1, b"1234567").unwrap();
        let end = frame(&mut m, Kind::ResultEnd, 2, &[1]);
        if limit == 208 {
            end.unwrap();
            cleanup(&mut m, 0);
            assert_eq!(m.report().usage.result_wire_bytes, 208);
        } else {
            assert!(end.is_err());
            protocol(&m, ProtocolError::WireExceeded);
            assert_eq!(m.report().usage.result_wire_bytes, 143);
            assert_eq!(m.report().usage.result_frames, 2);
        }
    }
}
#[test]
fn exact_frame_budget_succeeds_one_short_retains_rejected_wire_cost() {
    for cap in [3, 4] {
        let mut m = SessionModel::new(
            id(),
            b"",
            Limits {
                max_frames_per_direction: cap,
                ..Limits::default()
            },
        )
        .unwrap();
        sent(&mut m);
        begin(&mut m, 2);
        frame(&mut m, Kind::ResultData, 1, b"a").unwrap();
        frame(&mut m, Kind::ResultData, 2, b"b").unwrap();
        let end = frame(&mut m, Kind::ResultEnd, 3, &[1]);
        assert_eq!(m.report().usage.result_wire_bytes, 267);
        if cap == 4 {
            end.unwrap();
            cleanup(&mut m, 0);
        } else {
            assert!(end.is_err());
            protocol(&m, ProtocolError::FramesExceeded);
            assert_eq!(m.report().usage.result_frames, 3);
        }
    }
}
#[test]
fn accepted_output_cannot_exceed_cumulative_declaration() {
    let mut m = model();
    sent(&mut m);
    begin(&mut m, 2);
    frame(&mut m, Kind::ResultData, 1, b"a").unwrap();
    assert!(frame(&mut m, Kind::ResultData, 2, b"bc").is_err());
    protocol(&m, ProtocolError::DataExceeded);
    assert_eq!(m.report().usage.result_data_bytes, 1);
    assert_eq!(m.report().usage.result_frames, 3);
    assert_eq!(m.report().usage.result_wire_bytes, 203);
}
#[test]
fn first_malformed_header_cost_is_retained_and_later_frames_not_consumed() {
    let mut m = model();
    sent(&mut m);
    let mut h = encode_header(Kind::ResultBegin, id(), 0, 8);
    h[0] = 0;
    assert!(raw(&mut m, &h, &[0; 8]).is_err());
    let before = m.report().usage;
    assert!(frame(&mut m, Kind::ResultBegin, 0, &0_u64.to_le_bytes()).is_err());
    protocol(&m, ProtocolError::InvalidHeader);
    assert_eq!(m.report().usage.result_wire_bytes, before.result_wire_bytes);
    assert_eq!(m.report().usage.result_frames, 0);
}
#[test]
fn observation_cap_allows_exact_settlement_and_blocks_one_short() {
    for cap in [11, 12] {
        let mut m = SessionModel::new(
            id(),
            b"",
            Limits {
                max_observations: cap,
                ..Limits::default()
            },
        )
        .unwrap();
        complete(&mut m);
        for e in [
            Observation::Closed(Channel::Request),
            Observation::ResultEof,
            Observation::DiagnosticEof,
            Observation::Exit(ExitStatus::Code(0)),
            Observation::Reaped(ExitStatus::Code(0)),
            Observation::Closed(Channel::Result),
            Observation::Closed(Channel::Diagnostic),
        ] {
            apply(&mut m, ZERO, e).unwrap();
        }
        let result = apply(&mut m, ZERO, Observation::ResourcesReleased);
        assert_eq!(m.report().usage.observations, cap);
        if cap == 12 {
            result.unwrap();
            assert_eq!(m.report().phase, Phase::SettledModel);
        } else {
            assert!(result.is_err());
            fault(&m, Fault::ObservationLimit);
            blocked(&m);
            assert!(m.report().missing.resources_released);
        }
    }
}
#[test]
fn operation_deadline_precedes_same_call_cancel_and_prior_cancel_wins() {
    let mut m = model();
    apply(&mut m, Duration::from_secs(300), Observation::Cancel).unwrap();
    fault(&m, Fault::Deadline);
    let mut n = model();
    apply(&mut n, Duration::from_secs(299), Observation::Cancel).unwrap();
    apply(&mut n, Duration::from_secs(300), Observation::Tick).unwrap();
    fault(&n, Fault::Cancelled);
}
#[test]
fn cleanup_deadline_is_not_extended_by_later_events() {
    let mut m = model();
    apply(&mut m, Duration::from_secs(1), Observation::Cancel).unwrap();
    assert!(apply(&mut m, Duration::from_secs(20), Observation::IoFailure).is_err());
    apply(&mut m, Duration::from_secs(30), Observation::Tick).unwrap();
    assert_eq!(
        apply(&mut m, Duration::from_secs(31), Observation::Tick),
        Err(ApplyError::Fault(Fault::CleanupExpired))
    );
    fault(&m, Fault::Cancelled);
    blocked(&m);
}
#[test]
fn exit_starts_cleanup_deadline_without_prior_fault() {
    let mut m = model();
    complete(&mut m);
    apply(
        &mut m,
        Duration::from_secs(5),
        Observation::Exit(ExitStatus::Code(0)),
    )
    .unwrap();
    apply(&mut m, Duration::from_secs(34), Observation::Tick).unwrap();
    assert!(apply(&mut m, Duration::from_secs(35), Observation::Tick).is_err());
    fault(&m, Fault::CleanupExpired);
    blocked(&m);
}
#[test]
fn diagnostic_cap_is_cumulative_and_overflow_not_consumed() {
    let mut m = model();
    apply(&mut m, ZERO, Observation::DiagnosticBytes(&[0; 4095])).unwrap();
    apply(&mut m, ZERO, Observation::DiagnosticBytes(b"x")).unwrap();
    assert!(apply(&mut m, ZERO, Observation::DiagnosticBytes(b"y")).is_err());
    fault(&m, Fault::DiagnosticExceeded);
    assert_eq!(m.report().usage.diagnostic_bytes, MAX_DIAGNOSTIC_BYTES);
    assert_eq!(m.report().usage.result_frames, 0);
}
