use crate::support::*;
use snapshot::{
    Checkpoint, Error, Failure, FileSnapshot, Identity, LimitKind, Limits, Request, Stage, Stop,
    Usage, capture,
};
use std::{
    cell::Cell,
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    rc::Rc,
    time::Duration,
};

fn cancel(stage: Stage, reads: u64, actual: u64) {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"abc");
    let mut stopped = false;
    let failure = capture(
        Request {
            path: &path,
            limits: Limits::default(),
        },
        |point| {
            if point.stage == stage {
                stopped = true;
                Err(Stop::<()>::Cancelled)
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert!(stopped);
    assert!(matches!(failure.cause, Error::Cancelled));
    assert_eq!(failure.usage.read_calls, reads);
    assert_eq!(failure.usage.actual_read_bytes, actual);
    assert_eq!(fs::read(&path).unwrap(), b"abc");
    sandbox.finish();
}

#[test]
fn cancel_at_opening() {
    cancel(Stage::Opening, 0, 0);
}

#[test]
fn cancel_at_pinned() {
    cancel(Stage::Pinned, 0, 0);
}

#[test]
fn cancel_at_copying() {
    cancel(Stage::Copying, 0, 0);
}

#[test]
fn cancel_at_comparing() {
    cancel(Stage::Comparing, 2, 3);
}

#[test]
fn cancel_at_finalizing() {
    cancel(Stage::Finalizing, 4, 6);
}

#[test]
fn cancel_after_first_chunk_preserves_accumulated_usage() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", &pattern(65_537));
    let failure = capture(
        Request {
            path: &path,
            limits: Limits::default(),
        },
        |point| {
            if point.stage == Stage::Copying && point.offset == 65_536 {
                Err(Stop::<()>::Cancelled)
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert!(matches!(failure.cause, Error::Cancelled));
    assert_eq!(failure.usage.read_calls, 1);
    assert_eq!(failure.usage.actual_read_bytes, 65_536);
    assert_eq!(failure.usage.reserved_read_bytes, 65_536);
    sandbox.finish();
}

#[test]
fn callback_preserves_non_send_error_identity() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"abc");
    let marker = Rc::new(String::from("typed"));
    let failure = capture(
        Request {
            path: &path,
            limits: Limits::default(),
        },
        |point| {
            if point.stage == Stage::Comparing {
                Err(Stop::Callback(marker.clone()))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    match failure.cause {
        Error::Callback(value) => assert!(Rc::ptr_eq(&value, &marker)),
        other => panic!("unexpected: {other:?}"),
    }
    assert_eq!(failure.usage.actual_read_bytes, 3);
    sandbox.finish();
}

#[test]
fn callback_error_precedes_next_observed_source_change() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"abc");
    let failure = capture(
        Request {
            path: &path,
            limits: Limits::default(),
        },
        |point| {
            if point.stage == Stage::Comparing {
                fs::write(&path, b"xyz").unwrap();
                Err(Stop::Callback(73_u16))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert!(matches!(failure.cause, Error::Callback(73)));
    assert_eq!(failure.usage.actual_read_bytes, 3);
    assert_eq!(failure.usage.compared_bytes, 0);
    sandbox.finish();
}

fn expired(stop: Option<Stop<u16>>) -> Failure<u16> {
    let mut stop = Some(stop);
    capture(
        Request {
            path: Path::new("invalid"),
            limits: Limits {
                max_duration: Duration::from_millis(1),
                ..Limits::default()
            },
        },
        |_| {
            std::thread::sleep(Duration::from_millis(10));
            match stop.take().unwrap() {
                Some(value) => Err(value),
                None => Ok(()),
            }
        },
    )
    .unwrap_err()
}

#[test]
fn natural_elapsed_deadline_precedes_path_validation() {
    let failure = expired(None);
    assert!(matches!(failure.cause, Error::Limit(LimitKind::Deadline)));
    assert_eq!(failure.usage, Usage::default());
}

#[test]
fn cancellation_precedes_expired_checkpoint_deadline() {
    let failure = expired(Some(Stop::Cancelled));
    assert!(matches!(failure.cause, Error::Cancelled));
    assert_eq!(failure.usage, Usage::default());
}

#[test]
fn callback_error_precedes_expired_checkpoint_deadline() {
    let failure = expired(Some(Stop::Callback(91)));
    assert!(matches!(failure.cause, Error::Callback(91)));
    assert_eq!(failure.usage, Usage::default());
}

#[test]
fn checkpoint_order_offsets_and_usage_are_observable() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"abc");
    let mut points: Vec<Checkpoint> = Vec::new();
    let result = capture(
        Request {
            path: &path,
            limits: Limits::default(),
        },
        |point| {
            points.push(point);
            Ok::<_, Stop<()>>(())
        },
    )
    .unwrap();
    assert_eq!(
        points
            .iter()
            .map(|p| (p.stage, p.offset))
            .collect::<Vec<_>>(),
        vec![
            (Stage::Opening, 0),
            (Stage::Pinned, 0),
            (Stage::Copying, 0),
            (Stage::Copying, 0),
            (Stage::Copying, 3),
            (Stage::Comparing, 0),
            (Stage::Comparing, 3),
            (Stage::Finalizing, 3),
        ]
    );
    assert_eq!(points[0].length, None);
    assert!(points[1..].iter().all(|p| p.length == Some(3)));
    for pair in points.windows(2) {
        assert!(pair[0].usage.open_calls <= pair[1].usage.open_calls);
        assert!(pair[0].usage.metadata_calls <= pair[1].usage.metadata_calls);
        assert!(pair[0].usage.read_calls <= pair[1].usage.read_calls);
        assert!(pair[0].usage.reserved_read_bytes <= pair[1].usage.reserved_read_bytes);
        assert!(pair[0].usage.actual_read_bytes <= pair[1].usage.actual_read_bytes);
        assert!(pair[0].usage.compared_bytes <= pair[1].usage.compared_bytes);
    }
    expect_usage(&result, 3);
    sandbox.finish();
}

#[test]
fn callback_panic_propagates_without_snapshot() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"abc");
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _: Result<FileSnapshot, Failure<()>> = capture(
            Request {
                path: &path,
                limits: Limits::default(),
            },
            |point| {
                if point.stage == Stage::Copying {
                    panic!("intentional-checkpoint-panic");
                }
                Ok(())
            },
        );
    }));
    assert!(result.is_err());
    assert_eq!(fs::read(&path).unwrap(), b"abc");
    sandbox.finish();
}

#[test]
fn cancellation_requested_after_return_does_not_change_owned_result() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"abc");
    let requested = Cell::new(false);
    let result = capture(
        Request {
            path: &path,
            limits: Limits::default(),
        },
        |_| {
            if requested.get() {
                Err(Stop::<()>::Cancelled)
            } else {
                Ok(())
            }
        },
    )
    .unwrap();
    requested.set(true);
    assert_eq!(result.bytes(), b"abc");
    expect_usage(&result, 3);
    sandbox.finish();
}

#[test]
fn selected_public_types_have_send_bounds() {
    fn send<T: Send>() {}
    send::<FileSnapshot>();
    send::<Identity>();
    send::<Request<'static>>();
    send::<Failure<String>>();
    send::<Stop<String>>();
}
