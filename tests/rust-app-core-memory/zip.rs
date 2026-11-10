use crate::support::*;
use app_core::{Metric, Operation, Policy, Progress, zip_create::*};

#[test]
fn stored_payload_policy_and_allocation_are_preserved() {
    let (events, terminal) = zip_trace();
    assert_stream(&events, &terminal, Policy::ZipAsciiCreateV1);
    let validated = terminal.into_result().unwrap();
    assert_eq!(validated.policy(), Policy::ZipAsciiCreateV1);
    let pointer = validated.value().as_ptr();
    let bytes = validated.into_inner();
    assert_eq!(bytes.as_ptr(), pointer);
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    assert!(bytes.windows(9).any(|part| part == b"123456789"));
    let direct = create(zip_request(), |_| Ok(())).unwrap();
    assert_eq!(bytes, direct);
}

#[test]
fn deflate_output_matches_native_without_policy_translation() {
    let mut request = zip_request();
    request.options.method = Method::Deflate;
    let terminal = Operation::start()
        .unwrap()
        .0
        .create_zip(request, |_| Ok(()));
    let mut native_request = zip_request();
    native_request.options.method = Method::Deflate;
    assert_eq!(
        terminal.into_result().unwrap().into_inner(),
        create(native_request, |_| Ok(())).unwrap()
    );
}

#[test]
fn progress_preserves_native_points_and_extent_semantics() {
    let (events, terminal) = zip_trace();
    let mut native = Vec::new();
    create(zip_request(), |point| {
        native.push(point);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        events.iter().map(|e| e.checkpoint).collect::<Vec<_>>(),
        native
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e.checkpoint, Checkpoint::Write { .. }))
    );
    for event in &events {
        let expected = match event.checkpoint {
            Checkpoint::Input { index, offset, .. } => Some(Progress {
                metric: Metric::ZipInputOffset { entry: index },
                value: offset as u64,
                total: None,
            }),
            Checkpoint::Write { extent, .. } => Some(Progress {
                metric: Metric::ZipOutputExtent,
                value: extent,
                total: None,
            }),
            _ => None,
        };
        assert_eq!(event.progress, expected);
    }
    assert_stream(&events, &terminal, Policy::ZipAsciiCreateV1);
}

#[test]
fn encrypted_request_preserves_unsupported() {
    let mut request = zip_request();
    request.options.encrypted = true;
    assert_zip_failure(
        &Operation::start()
            .unwrap()
            .0
            .create_zip(request, |_| Ok(())),
        Failure::Unsupported,
    );
}

#[test]
fn unicode_name_preserves_ascii_policy_failure() {
    let mut request = zip_request();
    request.entries[0].name = "한글.txt".to_owned();
    assert_zip_failure(
        &Operation::start()
            .unwrap()
            .0
            .create_zip(request, |_| Ok(())),
        Failure::UnsafeName,
    );
}

#[test]
fn case_alias_conflict_is_not_integrity() {
    let mut request = zip_request();
    request.entries.push(Entry {
        name: "DIGITS.txt".to_owned(),
        bytes: Box::from(*b"second"),
    });
    assert_zip_failure(
        &Operation::start()
            .unwrap()
            .0
            .create_zip(request, |_| Ok(())),
        Failure::Conflict,
    );
}

#[test]
fn processed_limit_failure_has_no_archive_payload() {
    let mut request = zip_request();
    request.limits.processed = 8;
    assert_zip_failure(
        &Operation::start()
            .unwrap()
            .0
            .create_zip(request, |_| Ok(())),
        Failure::Budget,
    );
}

#[test]
fn pre_requested_cancel_delivers_start_and_forwards() {
    let (operation, cancel) = Operation::start().unwrap();
    cancel.request();
    let mut events = Vec::new();
    let terminal = operation.create_zip(zip_request(), |event| {
        events.push(event);
        Ok(())
    });
    assert_zip_failure(&terminal, Failure::Cancelled);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].checkpoint, Checkpoint::Start);
    assert_stream(&events, &terminal, Policy::ZipAsciiCreateV1);
    assert!(terminal.cancellation_requested() && terminal.cancellation_forwarded());
}

#[test]
fn invalid_timeout_precedes_observer_and_cancel_poll() {
    let (operation, cancel) = Operation::start().unwrap();
    cancel.request();
    let mut request = zip_request();
    request.timeout = std::time::Duration::ZERO;
    let terminal = operation.create_zip(request, |_| {
        panic!("잘못된 기한에서 observer를 호출하면 안 됨")
    });
    assert_zip_failure(&terminal, Failure::InvalidRequest);
    assert_eq!(terminal.stamp().sequence(), 0);
    assert!(terminal.cancellation_requested());
    assert!(!terminal.cancellation_forwarded());
}

#[test]
fn observer_deadline_beats_flag_and_remains_deadline() {
    let (operation, cancel) = Operation::start().unwrap();
    cancel.request();
    let terminal = operation.create_zip(zip_request(), |_| Err(Stop::Deadline));
    assert_zip_failure(&terminal, Failure::Deadline);
    assert!(terminal.cancellation_requested());
    assert!(!terminal.cancellation_forwarded());
}

#[test]
fn observer_cancel_is_distinct_from_flag_forwarding() {
    let terminal = Operation::start()
        .unwrap()
        .0
        .create_zip(zip_request(), |_| Err(Stop::Cancelled));
    assert_zip_failure(&terminal, Failure::Cancelled);
    assert!(!terminal.cancellation_requested() && !terminal.cancellation_forwarded());
}

#[test]
fn observer_flag_at_return_discards_finished_archive() {
    let (operation, cancel) = Operation::start().unwrap();
    let terminal = operation.create_zip(zip_request(), |event| {
        if event.checkpoint == Checkpoint::Return {
            cancel.request();
        }
        Ok(())
    });
    assert_zip_failure(&terminal, Failure::Cancelled);
    assert!(terminal.cancellation_requested() && terminal.cancellation_forwarded());
}

#[test]
fn late_cancel_keeps_success_snapshot() {
    let (operation, cancel) = Operation::start().unwrap();
    let terminal = operation.create_zip(zip_request(), |_| Ok(()));
    cancel.request();
    assert!(!terminal.cancellation_requested());
    assert!(!terminal.into_result().unwrap().into_inner().is_empty());
}

#[test]
fn late_cancel_keeps_native_error_snapshot() {
    let (operation, cancel) = Operation::start().unwrap();
    let mut request = zip_request();
    request.options.zip64 = true;
    let terminal = operation.create_zip(request, |_| Ok(()));
    cancel.request();
    assert!(!terminal.cancellation_requested());
    assert_zip_failure(&terminal, Failure::Unsupported);
}
