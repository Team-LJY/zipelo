use crate::support::*;
use publication::{
    CancelDecision, Checkpoint, Error, Limits, Publication, Request, Stage, Stop, Usage,
};
use std::{
    ffi::OsStr,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    rc::Rc,
    time::Duration,
};

#[test]
fn pre_requested_cancel_is_idempotent_and_stops_before_open() {
    let (task, cancel) = Publication::new();
    assert_eq!(cancel.request(), CancelDecision::Requested);
    assert_eq!(cancel.clone().request(), CancelDecision::AlreadyRequested);
    let mut callbacks = 0;
    let (error, usage, closed) = not_committed(task.publish(
        Request {
            parent: Path::new("/owned"),
            leaf: OsStr::new("unused"),
            bytes: b"x",
            limits: Limits::default(),
        },
        |_| {
            callbacks += 1;
            Ok::<_, Stop<()>>(())
        },
    ));
    assert!(matches!(error, Error::Cancelled));
    assert_eq!(callbacks, 1);
    assert_eq!(usage, Usage::default());
    cleanup(&closed, 0);
    assert_eq!(cancel.request(), CancelDecision::Finished);
}

fn cancel_at(stage: Stage, occurrence: usize, closes: u8, writes: u64, reads: u64) {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let (task, cancel) = Publication::new();
    let mut count = 0;
    let mut requested = false;
    let (error, usage, closed) = not_committed(task.publish(
        Request {
            parent: &parent,
            leaf: OsStr::new("output"),
            bytes: b"abc",
            limits: Limits::default(),
        },
        |point| {
            if point.stage == stage {
                count += 1;
                if count == occurrence {
                    assert_eq!(cancel.request(), CancelDecision::Requested);
                    requested = true;
                }
            }
            Ok::<_, Stop<()>>(())
        },
    ));
    assert!(requested);
    assert!(matches!(error, Error::Cancelled));
    assert_eq!(usage.link_calls, 0);
    assert_eq!(usage.write_calls, writes);
    assert_eq!(usage.read_calls, reads);
    cleanup(&closed, closes);
    assert!(names(&parent).is_empty());
    assert_eq!(cancel.request(), CancelDecision::Finished);
    sandbox.finish();
}

#[test]
fn cancel_first_opening_has_no_handles() {
    cancel_at(Stage::Opening, 1, 0, 0, 0);
}
#[test]
fn cancel_second_opening_closes_parent() {
    cancel_at(Stage::Opening, 2, 1, 0, 0);
}
#[test]
fn cancel_before_write_closes_anonymous_temp() {
    cancel_at(Stage::Writing, 1, 2, 0, 0);
}
#[test]
fn cancel_before_sync_preserves_written_usage() {
    cancel_at(Stage::Verifying, 1, 2, 1, 0);
}
#[test]
fn cancel_before_commit_closes_four_handles_without_link() {
    cancel_at(Stage::BeforeCommit, 1, 4, 1, 2);
}

#[test]
fn cancel_after_first_chunk_preserves_reservations() {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let (task, _) = Publication::new();
    let bytes = pattern(65_537);
    let (error, usage, closed) = not_committed(task.publish(
        Request {
            parent: &parent,
            leaf: OsStr::new("output"),
            bytes: &bytes,
            limits: Limits::default(),
        },
        |point| {
            if point.stage == Stage::Writing && point.offset == 65_536 {
                Err(Stop::<()>::Cancelled)
            } else {
                Ok(())
            }
        },
    ));
    assert!(matches!(error, Error::Cancelled));
    assert_eq!(usage.write_calls, 1);
    assert_eq!(usage.reserved_io_bytes, 65_536);
    assert_eq!(usage.actual_written_bytes, 65_536);
    assert_eq!(usage.read_calls + usage.link_calls + usage.sync_calls, 0);
    cleanup(&closed, 2);
    assert!(names(&parent).is_empty());
    sandbox.finish();
}

#[test]
fn callback_preserves_non_send_error_and_wins_over_cancel() {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let marker = Rc::new(String::from("typed-callback"));
    let (task, cancel) = Publication::new();
    let (error, usage, closed) = not_committed(task.publish(
        Request {
            parent: &parent,
            leaf: OsStr::new("output"),
            bytes: b"abc",
            limits: Limits::default(),
        },
        |point| {
            if point.stage == Stage::BeforeCommit {
                assert_eq!(cancel.request(), CancelDecision::Requested);
                Err(Stop::Callback(marker.clone()))
            } else {
                Ok(())
            }
        },
    ));
    match error {
        Error::Callback(value) => assert!(Rc::ptr_eq(&value, &marker)),
        other => panic!("callback error replaced: {other:?}"),
    }
    assert_eq!(usage.link_calls, 0);
    assert_eq!(usage.actual_read_bytes, 3);
    assert_eq!(usage.actual_written_bytes, 3);
    cleanup(&closed, 4);
    assert!(names(&parent).is_empty());
    assert_eq!(cancel.request(), CancelDecision::Finished);
    sandbox.finish();
}

#[test]
fn callback_panic_has_no_report_or_named_output() {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let (task, cancel) = Publication::new();
    let panicked = catch_unwind(AssertUnwindSafe(|| {
        task.publish(
            Request {
                parent: &parent,
                leaf: OsStr::new("output"),
                bytes: b"abc",
                limits: Limits::default(),
            },
            |point| {
                if point.stage == Stage::BeforeCommit {
                    panic!("intentional-publication-callback-panic");
                }
                Ok::<_, Stop<()>>(())
            },
        )
    }));
    assert!(panicked.is_err());
    assert_eq!(cancel.request(), CancelDecision::Finished);
    assert!(names(&parent).is_empty());
    assert!(owned_descriptors(&sandbox.root).is_empty());
    sandbox.finish();
}

fn expired(stop: Option<Stop<u16>>) -> Error<u16> {
    let (task, _) = Publication::new();
    let mut stop = Some(stop);
    let (error, usage, closed) = not_committed(task.publish(
        Request {
            parent: Path::new("/owned"),
            leaf: OsStr::new("unused"),
            bytes: &[],
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
    ));
    assert_eq!(usage, Usage::default());
    cleanup(&closed, 0);
    error
}

#[test]
fn natural_elapsed_deadline_stops_before_open() {
    assert!(matches!(expired(None), Error::DeadlineExceeded));
}
#[test]
fn callback_cancel_precedes_expired_deadline() {
    assert!(matches!(expired(Some(Stop::Cancelled)), Error::Cancelled));
}
#[test]
fn callback_error_precedes_expired_deadline() {
    assert!(matches!(
        expired(Some(Stop::Callback(91))),
        Error::Callback(91)
    ));
}

#[test]
fn cancellation_after_return_preserves_committed_output() {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let (task, cancel) = Publication::new();
    let report = task.publish(
        Request {
            parent: &parent,
            leaf: OsStr::new("output"),
            bytes: b"abc",
            limits: Limits::default(),
        },
        |_| Ok::<_, Stop<()>>(()),
    );
    assert_eq!(cancel.request(), CancelDecision::Finished);
    assert_eq!(cancel.request(), CancelDecision::Finished);
    committed(report, &parent, OsStr::new("output"), b"abc");
    sandbox.finish();
}

#[test]
fn checkpoints_keep_output_absent_and_close_owned_descriptors() {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let (task, _) = Publication::new();
    let mut points: Vec<Checkpoint> = Vec::new();
    let report = task.publish(
        Request {
            parent: &parent,
            leaf: OsStr::new("output"),
            bytes: b"abc",
            limits: Limits::default(),
        },
        |point| {
            assert!(names(&parent).is_empty());
            if point.stage == Stage::BeforeCommit {
                assert_eq!(owned_descriptors(&sandbox.root).len(), 2);
                assert_eq!(point.usage.link_calls, 0);
                assert_eq!(point.usage.reserved_io_bytes, 7);
            }
            points.push(point);
            Ok::<_, Stop<()>>(())
        },
    );
    assert_eq!(
        points
            .iter()
            .map(|p| (p.stage, p.offset))
            .collect::<Vec<_>>(),
        vec![
            (Stage::Opening, 0),
            (Stage::Opening, 0),
            (Stage::Writing, 0),
            (Stage::Verifying, 0),
            (Stage::Verifying, 0),
            (Stage::Verifying, 3),
            (Stage::BeforeCommit, 3),
        ]
    );
    assert!(points.iter().all(|p| p.length == 3));
    for pair in points.windows(2) {
        assert!(pair[0].usage.reserved_io_bytes <= pair[1].usage.reserved_io_bytes);
        assert!(pair[0].usage.actual_written_bytes <= pair[1].usage.actual_written_bytes);
        assert!(pair[0].usage.actual_read_bytes <= pair[1].usage.actual_read_bytes);
    }
    assert!(owned_descriptors(&sandbox.root).is_empty());
    committed(report, &parent, OsStr::new("output"), b"abc");
    sandbox.finish();
}

#[test]
fn dropping_unused_publication_marks_finished_without_io() {
    let (task, cancel) = Publication::new();
    drop(task);
    assert_eq!(cancel.request(), CancelDecision::Finished);
}
