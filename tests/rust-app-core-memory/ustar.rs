use crate::support::*;
use app_core::{Metric, Operation, Policy, Progress, ustar::*};
use std::convert::Infallible;

#[test]
fn selected_payload_owns_bytes_policy_hash_and_usage() {
    let (events, terminal) = ustar_trace(true);
    assert_stream(&events, &terminal, Policy::UstarLinuxV1);
    let validated = terminal.into_result().unwrap();
    assert_eq!(validated.policy(), Policy::UstarLinuxV1);
    let pointer = validated.value().files[0].bytes.as_ptr();
    let done = validated.into_inner();
    assert_eq!(done.files[0].bytes.as_ptr(), pointer);
    assert_eq!(done.entries.len(), 1);
    assert_eq!(done.entries[0].id, "0");
    assert_eq!(done.entries[0].name, "digits.txt");
    assert_eq!(done.entries[0].kind, Kind::File);
    assert_eq!(done.entries[0].size, 9);
    assert_eq!(done.files[0].id, done.entries[0].id);
    assert_eq!(done.files[0].bytes, b"123456789");
    assert_eq!(done.input_sha256.as_slice(), fixture("ustar_sha256"));
    assert_eq!(
        done.usage,
        Usage {
            entries: 1,
            processed: 4096,
            output: 9
        }
    );
    assert_eq!(done.content_verification, ContentVerification::Unavailable);
}

#[test]
fn listing_without_selection_retains_content_verification_limit() {
    let (_, terminal) = ustar_trace(false);
    let done = terminal.into_result().unwrap().into_inner();
    assert_eq!(done.entries.len(), 1);
    assert!(done.files.is_empty());
    assert_eq!(
        done.usage,
        Usage {
            entries: 1,
            processed: 2048,
            output: 0
        }
    );
    assert_eq!(done.content_verification, ContentVerification::Unavailable);
}

#[test]
fn progress_preserves_both_passes_and_native_points() {
    let (events, terminal) = ustar_trace(true);
    let mut native = Vec::new();
    read(
        ustar_request(),
        |entries| Ok::<_, Infallible>(vec![entries[0].id.clone()]),
        |point| {
            native.push(point);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        events.iter().map(|e| e.checkpoint).collect::<Vec<_>>(),
        native
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e.checkpoint, Checkpoint::Read { pass: 2, .. }))
    );
    for event in &events {
        let expected = match event.checkpoint {
            Checkpoint::Read { pass, offset } => Some(Progress {
                metric: Metric::UstarInputOffset { pass },
                value: offset as u64,
                total: None,
            }),
            Checkpoint::Entry { pass, index } => Some(Progress {
                metric: Metric::UstarEntryIndex { pass },
                value: index as u64,
                total: None,
            }),
            _ => None,
        };
        assert_eq!(event.progress, expected);
    }
    assert_stream(&events, &terminal, Policy::UstarLinuxV1);
}

#[test]
fn windows_policy_stays_unsupported() {
    let mut request = ustar_request();
    request.policy = NamePolicy::Windows;
    let terminal = Operation::start().unwrap().0.ustar(
        request,
        |_| Ok::<_, Infallible>(Vec::new()),
        |_| Ok(()),
    );
    assert_ustar_failure(&terminal, Failure::Unsupported);
}

#[test]
fn corrupt_header_preserves_integrity_failure() {
    let mut request = ustar_request();
    request.input[0] ^= 1;
    let terminal = Operation::start().unwrap().0.ustar(
        request,
        |_| panic!("선택 전에 헤더를 거부해야 함"),
        |_| Ok(()),
    );
    assert_ustar_failure::<Infallible>(&terminal, Failure::Integrity);
}

#[test]
fn zero_limit_is_invalid_before_observer_or_cancel_poll() {
    let (operation, cancel) = Operation::start().unwrap();
    cancel.request();
    let mut request = ustar_request();
    request.limits.output = 0;
    let terminal = operation.ustar(
        request,
        |_| Ok::<_, Infallible>(Vec::new()),
        |_| panic!("잘못된 한도에서 observer를 호출하면 안 됨"),
    );
    assert_ustar_failure(&terminal, Failure::InvalidRequest);
    assert_eq!(terminal.stamp().sequence(), 0);
    assert!(terminal.cancellation_requested());
    assert!(!terminal.cancellation_forwarded());
}

#[test]
fn selection_error_keeps_allocation_identity() {
    let marker = Marker(Box::from(*b"selection"));
    let pointer = marker.0.as_ptr();
    let terminal =
        Operation::start()
            .unwrap()
            .0
            .ustar(ustar_request(), |_| Err(marker), |_| Ok(()));
    let failure = terminal.into_result().unwrap_err();
    assert_eq!(failure.adapter, None);
    match failure.native.unwrap() {
        Error::Selection(marker) => {
            assert_eq!(marker.0.as_ptr(), pointer);
            assert_eq!(&*marker.0, b"selection");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn selection_error_and_late_cancel_coexist_without_overwrite() {
    let (operation, cancel) = Operation::start().unwrap();
    let terminal = operation.ustar(
        ustar_request(),
        |_| {
            cancel.request();
            Err(Marker(Box::from(*b"original")))
        },
        |_| Ok(()),
    );
    assert!(terminal.cancellation_requested());
    assert!(!terminal.cancellation_forwarded());
    assert!(matches!(
        terminal.result().unwrap_err().native,
        Some(Error::Selection(_))
    ));
}

#[test]
fn selection_ids_are_not_numeric_normalized() {
    let terminal = Operation::start().unwrap().0.ustar(
        ustar_request(),
        |_| Ok::<_, Infallible>(vec!["00".to_owned()]),
        |_| Ok(()),
    );
    assert_ustar_failure(&terminal, Failure::Unsupported);
}

#[test]
fn pre_requested_cancel_stops_after_observed_start() {
    let (operation, cancel) = Operation::start().unwrap();
    cancel.request();
    let mut events = Vec::new();
    let terminal = operation.ustar(
        ustar_request(),
        |_| panic!("취소 후 선택을 호출하면 안 됨"),
        |event| {
            events.push(event);
            Ok(())
        },
    );
    assert_ustar_failure::<Infallible>(&terminal, Failure::Cancelled);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].checkpoint, Checkpoint::Start);
    assert_stream(&events, &terminal, Policy::UstarLinuxV1);
    assert!(terminal.cancellation_forwarded());
}

#[test]
fn observer_requested_cancel_at_finish_discards_payload() {
    let (operation, cancel) = Operation::start().unwrap();
    let terminal = operation.ustar(
        ustar_request(),
        |entries| Ok::<_, Infallible>(vec![entries[0].id.clone()]),
        |event| {
            if event.checkpoint == Checkpoint::Finish {
                cancel.request();
            }
            Ok(())
        },
    );
    assert_ustar_failure(&terminal, Failure::Cancelled);
    assert!(terminal.cancellation_requested() && terminal.cancellation_forwarded());
}

#[test]
fn observer_deadline_precedes_flag_and_preserves_budget_class() {
    let (operation, cancel) = Operation::start().unwrap();
    cancel.request();
    let terminal = operation.ustar(
        ustar_request(),
        |_| Ok::<_, Infallible>(Vec::new()),
        |_| Err(Stop::Deadline),
    );
    assert_ustar_failure(&terminal, Failure::Budget);
    assert!(terminal.cancellation_requested());
    assert!(!terminal.cancellation_forwarded());
}

#[test]
fn observer_cancel_does_not_claim_flag_forwarding() {
    let terminal = Operation::start().unwrap().0.ustar(
        ustar_request(),
        |_| Ok::<_, Infallible>(Vec::new()),
        |_| Err(Stop::Cancelled),
    );
    assert_ustar_failure(&terminal, Failure::Cancelled);
    assert!(!terminal.cancellation_requested() && !terminal.cancellation_forwarded());
}

#[test]
fn late_cancel_keeps_completed_snapshot() {
    let (operation, cancel) = Operation::start().unwrap();
    let terminal = operation.ustar(
        ustar_request(),
        |entries| Ok::<_, Infallible>(vec![entries[0].id.clone()]),
        |_| Ok(()),
    );
    cancel.request();
    assert!(!terminal.cancellation_requested());
    assert_eq!(
        terminal.into_result().unwrap().into_inner().files[0].bytes,
        b"123456789"
    );
}

#[test]
fn selected_output_budget_failure_has_no_partial_files() {
    let mut request = ustar_request();
    request.limits.output = 8;
    let terminal = Operation::start().unwrap().0.ustar(
        request,
        |entries| Ok::<_, Infallible>(vec![entries[0].id.clone()]),
        |_| Ok(()),
    );
    assert_ustar_failure(&terminal, Failure::Budget);
}
