use crate::support::*;
use publication::{
    Error, Limits, MAX_BYTES, MAX_IO_BYTES, MAX_IO_CALLS, Publication, Request, Stop, Usage,
};
use std::{ffi::OsStr, path::Path, time::Duration};

fn boundary(length: usize) {
    let sandbox = Sandbox::new();
    let bytes = pattern(length);
    let before = bytes.clone();
    let parent = sandbox.parent();
    let report = publish(&parent, OsStr::new("output"), &bytes, Limits::default());
    committed(report, &parent, OsStr::new("output"), &before);
    assert_eq!(bytes, before);
    sandbox.finish();
}

#[test]
fn empty() {
    boundary(0);
}
#[test]
fn one() {
    boundary(1);
}
#[test]
fn chunk_minus_one() {
    boundary(65_535);
}
#[test]
fn chunk() {
    boundary(65_536);
}
#[test]
fn chunk_plus_one() {
    boundary(65_537);
}
#[test]
fn maximum_minus_one() {
    boundary(16_777_215);
}
#[test]
fn maximum() {
    boundary(16_777_216);
}

#[test]
fn invalid_limits_precede_paths_and_callback() {
    for limits in [
        Limits {
            max_bytes: MAX_BYTES + 1,
            ..Limits::default()
        },
        Limits {
            max_io_bytes: MAX_IO_BYTES + 1,
            ..Limits::default()
        },
        Limits {
            max_io_calls: MAX_IO_CALLS + 1,
            ..Limits::default()
        },
        Limits {
            max_duration: Duration::ZERO,
            ..Limits::default()
        },
        Limits {
            max_duration: Duration::from_secs(301),
            ..Limits::default()
        },
    ] {
        let (task, cancel) = Publication::new();
        let report = task.publish(
            Request {
                parent: Path::new("invalid"),
                leaf: OsStr::new(""),
                bytes: &[],
                limits,
            },
            |_| -> Result<(), Stop<()>> { panic!("callback must not run") },
        );
        let (error, usage, closed) = not_committed(report);
        assert!(matches!(error, Error::InvalidLimits));
        assert_eq!(usage, Usage::default());
        cleanup(&closed, 0);
        assert_eq!(cancel.request(), publication::CancelDecision::Finished);
    }
}

#[test]
fn oversized_input_rejected_without_io() {
    let bytes = pattern(MAX_BYTES as usize + 1);
    let report = publish(
        Path::new("invalid"),
        OsStr::new("output"),
        &bytes,
        Limits::default(),
    );
    let (error, usage, closed) = not_committed(report);
    assert!(matches!(error, Error::LimitExceeded));
    assert_eq!(usage, Usage::default());
    cleanup(&closed, 0);
}

#[test]
fn zero_byte_limit_accepts_empty_with_exact_eof_budget() {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let limits = Limits {
        max_bytes: 0,
        max_io_bytes: 1,
        max_io_calls: 1,
        ..Limits::default()
    };
    committed(
        publish(&parent, OsStr::new("output"), &[], limits),
        &parent,
        OsStr::new("output"),
        &[],
    );
    sandbox.finish();
}

#[test]
fn zero_byte_limit_rejects_payload_before_path_validation() {
    let (error, usage, closed) = not_committed(publish(
        Path::new("invalid"),
        OsStr::new("output"),
        b"x",
        Limits {
            max_bytes: 0,
            ..Limits::default()
        },
    ));
    assert!(matches!(error, Error::LimitExceeded));
    assert_eq!(usage, Usage::default());
    cleanup(&closed, 0);
}

fn empty_budget(limits: Limits) {
    let sandbox = Sandbox::new();
    let (error, usage, closed) = not_committed(publish(
        &sandbox.parent(),
        OsStr::new("output"),
        &[],
        limits,
    ));
    assert!(matches!(error, Error::LimitExceeded));
    assert_eq!(usage.open_calls, 2);
    assert_eq!(usage.metadata_calls, 3);
    assert_eq!(usage.sync_calls, 1);
    assert_eq!(
        usage.write_calls + usage.read_calls + usage.reserved_io_bytes + usage.link_calls,
        0
    );
    cleanup(&closed, 2);
    assert!(names(&sandbox.parent()).is_empty());
    sandbox.finish();
}

#[test]
fn zero_io_budget_fails_eof_without_refund() {
    empty_budget(Limits {
        max_io_bytes: 0,
        ..Limits::default()
    });
}

#[test]
fn zero_call_budget_is_valid_but_cannot_probe_eof() {
    empty_budget(Limits {
        max_io_calls: 0,
        ..Limits::default()
    });
}

#[test]
fn exact_io_and_call_budget_includes_eof() {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let bytes = pattern(65_537);
    let limits = Limits {
        max_io_bytes: 131_075,
        max_io_calls: 5,
        ..Limits::default()
    };
    committed(
        publish(&parent, OsStr::new("output"), &bytes, limits),
        &parent,
        OsStr::new("output"),
        &bytes,
    );
    sandbox.finish();
}

fn last_eof_limit(limits: Limits) {
    let sandbox = Sandbox::new();
    let (error, usage, closed) = not_committed(publish(
        &sandbox.parent(),
        OsStr::new("output"),
        &pattern(65_537),
        limits,
    ));
    assert!(matches!(error, Error::LimitExceeded));
    assert_eq!(usage.write_calls, 2);
    assert_eq!(usage.read_calls, 2);
    assert_eq!(usage.reserved_io_bytes, 131_074);
    assert_eq!(usage.actual_written_bytes, 65_537);
    assert_eq!(usage.actual_read_bytes, 65_537);
    assert_eq!(usage.link_calls, 0);
    assert_eq!(usage.sync_calls, 1);
    cleanup(&closed, 2);
    assert!(names(&sandbox.parent()).is_empty());
    sandbox.finish();
}

#[test]
fn one_less_io_budget_stops_before_last_eof() {
    last_eof_limit(Limits {
        max_io_bytes: 131_074,
        ..Limits::default()
    });
}

#[test]
fn one_less_call_budget_stops_before_last_eof() {
    last_eof_limit(Limits {
        max_io_calls: 4,
        ..Limits::default()
    });
}
