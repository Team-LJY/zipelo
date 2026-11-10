use crate::support::*;
use app_core::{Metric, Operation, Policy, Progress, gzip::*};
use std::convert::Infallible;

fn run(input: Vec<u8>, selected: bool, budget: &mut Budget) -> GzipTerminal {
    Operation::start().unwrap().0.gzip(
        Request {
            input: input.into_boxed_slice(),
            selected,
        },
        budget,
        |_| Ok::<_, Stop<Infallible>>(()),
    )
}

fn budget() -> Budget {
    Budget::new(Limits::default()).unwrap()
}

#[test]
fn selected_owned_payload_boundary_policy_and_usage() {
    let (events, terminal) = gzip_trace(true);
    assert_stream(&events, &terminal, Policy::GzipSingleMemberV1);
    assert!(!terminal.cancellation_requested());
    assert!(!terminal.cancellation_forwarded());
    let validated = terminal.into_result().unwrap();
    assert_eq!(validated.policy(), Policy::GzipSingleMemberV1);
    let pointer = validated.value().bytes.as_ref().unwrap().as_ptr();
    let done = validated.into_inner();
    assert_eq!(done.bytes.as_ref().unwrap().as_ptr(), pointer);
    assert_eq!(done.bytes.unwrap(), b"123456789");
    assert_eq!(
        done.usage,
        Usage {
            processed: 38,
            output: 9,
            metadata: 10
        }
    );
    assert_eq!(
        done.boundary,
        Boundary {
            header_bytes: 10,
            deflate_bytes: 11,
            read_ahead_bytes: 8,
            inflated_bytes: 9,
            crc32: 0xcbf43926,
            member_bytes: 29,
            offered_bytes: 19,
        }
    );
}

#[test]
fn unselected_still_validates_and_charges_decode() {
    let (_, terminal) = gzip_trace(false);
    let done = terminal.into_result().unwrap().into_inner();
    assert!(done.bytes.is_none());
    assert_eq!(done.boundary.crc32, 0xcbf43926);
    assert_eq!(
        done.usage,
        Usage {
            processed: 38,
            output: 0,
            metadata: 10
        }
    );
}

#[test]
fn empty_selected_payload_is_some_empty() {
    let terminal = run(fixture("gzip_empty"), true, &mut budget());
    assert_eq!(
        terminal.into_result().unwrap().into_inner().bytes,
        Some(Vec::new())
    );
}

#[test]
fn progress_preserves_native_checkpoints_and_cumulative_metric() {
    let (events, terminal) = gzip_trace(true);
    let mut native = Vec::new();
    read(gzip_request("gzip_digits", true), &mut budget(), |point| {
        native.push(point);
        Ok::<_, Stop<Infallible>>(())
    })
    .unwrap();
    assert_eq!(
        events.iter().map(|e| e.checkpoint).collect::<Vec<_>>(),
        native
    );
    for event in &events {
        assert_eq!(
            event.progress,
            Some(Progress {
                metric: Metric::GzipCumulativeProcessedBytes,
                value: event.checkpoint.usage.processed,
                total: None,
            })
        );
    }
    assert_stream(&events, &terminal, Policy::GzipSingleMemberV1);
}

#[test]
fn crc_failure_is_typed_and_never_exposes_payload() {
    let mut input = fixture("gzip_digits");
    input[21] ^= 1;
    assert_gzip_failure(&run(input, true, &mut budget()), Failure::Crc);
}

#[test]
fn size_failure_remains_distinct_from_crc() {
    let mut input = fixture("gzip_digits");
    input[25] ^= 1;
    assert_gzip_failure(&run(input, false, &mut budget()), Failure::Size);
}

#[test]
fn backend_data_error_remains_typed() {
    let mut input = fixture("gzip_digits");
    input[10] = 7;
    let terminal = run(input, true, &mut budget());
    let failure = terminal.result().unwrap_err();
    assert_eq!(failure.adapter, None);
    assert!(matches!(
        failure.native,
        Some(Error::Backend(InflateError::DataError))
    ));
}

#[test]
fn trailing_data_is_not_promoted_to_success() {
    let mut input = fixture("gzip_digits");
    input.push(0);
    assert_gzip_failure(&run(input, true, &mut budget()), Failure::TrailingData);
}

#[test]
fn pre_requested_cancel_still_delivers_start_and_forwards() {
    let (operation, cancel) = Operation::start().unwrap();
    cancel.request();
    let mut points = Vec::new();
    let mut budget = budget();
    let terminal = operation.gzip(gzip_request("gzip_digits", true), &mut budget, |event| {
        points.push(event);
        Ok::<_, Stop<Infallible>>(())
    });
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].checkpoint.stage, Stage::Start);
    assert_stream(&points, &terminal, Policy::GzipSingleMemberV1);
    assert_gzip_failure(&terminal, Failure::Cancelled);
    assert!(terminal.cancellation_requested() && terminal.cancellation_forwarded());
    assert_eq!(budget.usage(), Usage::default());
}

#[test]
fn observer_requests_cancel_after_output_and_discards_result() {
    let (operation, cancel) = Operation::start().unwrap();
    let mut budget = budget();
    let terminal = operation.gzip(gzip_request("gzip_digits", true), &mut budget, |event| {
        if event.checkpoint.stage == Stage::AfterOutput {
            cancel.request();
        }
        Ok::<_, Stop<Infallible>>(())
    });
    assert_gzip_failure(&terminal, Failure::Cancelled);
    assert!(terminal.cancellation_requested() && terminal.cancellation_forwarded());
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 38,
            output: 9,
            metadata: 10
        }
    );
}

#[test]
fn observer_cancel_is_not_flag_forwarding() {
    let (operation, _) = Operation::start().unwrap();
    let terminal = operation.gzip(gzip_request("gzip_digits", true), &mut budget(), |_| {
        Err::<(), _>(Stop::<Infallible>::Cancelled)
    });
    assert_gzip_failure(&terminal, Failure::Cancelled);
    assert!(!terminal.cancellation_requested() && !terminal.cancellation_forwarded());
}

#[test]
fn observer_deadline_beats_requested_cancel_and_stays_budget() {
    let (operation, cancel) = Operation::start().unwrap();
    cancel.request();
    let terminal = operation.gzip(gzip_request("gzip_digits", true), &mut budget(), |_| {
        Err::<(), _>(Stop::<Infallible>::Deadline)
    });
    assert_gzip_failure(&terminal, Failure::Budget);
    assert!(terminal.cancellation_requested());
    assert!(!terminal.cancellation_forwarded());
}

#[test]
fn callback_error_identity_beats_cancel_without_stringification() {
    let (operation, cancel) = Operation::start().unwrap();
    cancel.request();
    let mut marker = Some(Marker(Box::from(*b"original")));
    let pointer = marker.as_ref().unwrap().0.as_ptr();
    let terminal = operation.gzip(gzip_request("gzip_digits", true), &mut budget(), |_| {
        Err::<(), _>(Stop::Callback(marker.take().unwrap()))
    });
    assert!(terminal.cancellation_requested());
    assert!(!terminal.cancellation_forwarded());
    let failure = terminal.into_result().unwrap_err();
    assert_eq!(failure.adapter, None);
    match failure.native.unwrap() {
        Error::Callback(marker) => {
            assert_eq!(marker.0.as_ptr(), pointer);
            assert_eq!(&*marker.0, b"original");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn final_callback_error_keeps_cost_and_hides_completed() {
    let (operation, _) = Operation::start().unwrap();
    let mut budget = budget();
    let terminal = operation.gzip(gzip_request("gzip_digits", true), &mut budget, |event| {
        if event.checkpoint.stage == Stage::BeforeReturn {
            Err(Stop::Callback(Marker(Box::from(*b"final"))))
        } else {
            Ok(())
        }
    });
    assert!(matches!(
        terminal.result().unwrap_err().native,
        Some(Error::Callback(_))
    ));
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 38,
            output: 9,
            metadata: 10
        }
    );
}

#[test]
fn late_cancel_does_not_rewrite_success_snapshot() {
    let (operation, cancel) = Operation::start().unwrap();
    let terminal = operation.gzip(gzip_request("gzip_digits", true), &mut budget(), |_| {
        Ok::<_, Stop<Infallible>>(())
    });
    assert!(cancel.request());
    assert!(!terminal.cancellation_requested() && !terminal.cancellation_forwarded());
    assert_eq!(
        terminal.into_result().unwrap().into_inner().bytes.unwrap(),
        b"123456789"
    );
}

#[test]
fn late_cancel_does_not_rewrite_error_snapshot() {
    let (operation, cancel) = Operation::start().unwrap();
    let terminal = operation.gzip(
        Request {
            input: Box::from([0]),
            selected: true,
        },
        &mut budget(),
        |_| Ok::<_, Stop<Infallible>>(()),
    );
    cancel.request();
    assert!(!terminal.cancellation_requested());
    assert_gzip_failure(&terminal, Failure::Header);
}

#[test]
fn shared_budget_retains_failed_crc_cost_across_new_operation() {
    let mut budget = budget();
    let mut corrupt = fixture("gzip_digits");
    corrupt[21] ^= 1;
    let first = run(corrupt, true, &mut budget);
    assert_gzip_failure(&first, Failure::Crc);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 38,
            output: 9,
            metadata: 10
        }
    );
    let (operation, _) = Operation::start().unwrap();
    assert_ne!(operation.id(), first.stamp().operation());
    let mut initial = None;
    let second = operation.gzip(gzip_request("gzip_digits", true), &mut budget, |event| {
        initial.get_or_insert(event.checkpoint.usage);
        Ok::<_, Stop<Infallible>>(())
    });
    assert_eq!(
        initial.unwrap(),
        Usage {
            processed: 38,
            output: 9,
            metadata: 10
        }
    );
    assert_eq!(
        second.into_result().unwrap().into_inner().usage,
        Usage {
            processed: 76,
            output: 18,
            metadata: 20
        }
    );
}

#[test]
fn shared_processed_limit_is_not_reset_on_retry() {
    let mut budget = Budget::new(Limits {
        processed: 38,
        ..Default::default()
    })
    .unwrap();
    assert!(
        run(fixture("gzip_digits"), false, &mut budget)
            .result()
            .is_ok()
    );
    assert_gzip_failure(
        &run(fixture("gzip_digits"), false, &mut budget),
        Failure::Budget,
    );
    assert_eq!(budget.usage().processed, 38);
}

#[test]
fn zero_output_limit_allows_unselected_and_rejects_selected() {
    let mut unselected = Budget::new(Limits {
        output: 0,
        ..Default::default()
    })
    .unwrap();
    assert!(
        run(fixture("gzip_digits"), false, &mut unselected)
            .result()
            .is_ok()
    );
    let mut selected = Budget::new(Limits {
        output: 0,
        ..Default::default()
    })
    .unwrap();
    assert_gzip_failure(
        &run(fixture("gzip_digits"), true, &mut selected),
        Failure::Budget,
    );
    assert_eq!(selected.usage().output, 0);
}

#[test]
fn observer_cancel_then_callback_error_preserves_original_at_final_checkpoint() {
    let (operation, cancel) = Operation::start().unwrap();
    let mut marker = Some(Marker(Box::from(*b"original")));
    let pointer = marker.as_ref().unwrap().0.as_ptr();
    let mut budget = budget();
    let mut events = Vec::new();
    let terminal = operation.gzip(gzip_request("gzip_digits", true), &mut budget, |event| {
        let at_final = event.checkpoint.stage == Stage::BeforeReturn;
        events.push(event);
        if at_final {
            assert!(!cancel.is_requested());
            assert!(cancel.request());
            Err(Stop::Callback(marker.take().unwrap()))
        } else {
            Ok(())
        }
    });
    assert!(marker.is_none());
    assert_eq!(events.last().unwrap().checkpoint.stage, Stage::BeforeReturn);
    assert_stream(&events, &terminal, Policy::GzipSingleMemberV1);
    assert!(terminal.cancellation_requested());
    assert!(!terminal.cancellation_forwarded());
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 38,
            output: 9,
            metadata: 10
        }
    );
    let failure = terminal.into_result().unwrap_err();
    assert_eq!(failure.adapter, None);
    match failure.native.unwrap() {
        Error::Callback(marker) => {
            assert_eq!(marker.0.as_ptr(), pointer);
            assert_eq!(&*marker.0, b"original");
        }
        other => panic!("{other:?}"),
    }
}
