use crate::support::*;
use snapshot::{Error, LimitKind, Limits, MAX_BYTES, MAX_IO_BYTES, MAX_READ_CALLS};
use std::{fs, os::unix::fs::MetadataExt, path::Path, time::Duration};

fn boundary(length: usize) {
    let sandbox = Sandbox::new();
    let bytes = pattern(length);
    let path = sandbox.file("input", &bytes);
    let before = fs::metadata(&path).unwrap();
    let result = get(&path, Limits::default()).unwrap();
    assert_eq!(result.bytes(), bytes);
    assert_eq!(result.identity().length(), length as u64);
    assert_eq!(result.identity().device(), before.dev());
    assert_eq!(result.identity().inode(), before.ino());
    assert_eq!(result.identity().links(), 1);
    assert_eq!(result.identity().mode(), before.mode());
    assert_eq!(
        result.identity().modified(),
        (before.mtime(), before.mtime_nsec())
    );
    assert_eq!(
        result.identity().changed(),
        (before.ctime(), before.ctime_nsec())
    );
    expect_usage(&result, length);
    assert_eq!(result.into_bytes(), bytes);
    assert_eq!(fs::read(&path).unwrap(), bytes);
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
    boundary(65535);
}

#[test]
fn chunk() {
    boundary(65536);
}

#[test]
fn chunk_plus_one() {
    boundary(65537);
}

#[test]
fn maximum_minus_one() {
    boundary(16777215);
}

#[test]
fn maximum() {
    boundary(16777216);
}

#[test]
fn sha256_known_abc_and_debug_redaction() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("private-input-marker", b"abc");
    let result = get(&path, Limits::default()).unwrap();
    assert_eq!(
        result.sha256(),
        [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ]
    );
    let debug = format!("{result:?}");
    for secret in ["private-input-marker", "abc", "sha256", "identity"] {
        assert!(!debug.contains(secret));
    }
    sandbox.finish();
}

fn invalid(limits: Limits) {
    let mut called = false;
    let result = snapshot::capture(
        snapshot::Request {
            path: Path::new(""),
            limits,
        },
        |_| {
            called = true;
            Err(snapshot::Stop::<()>::Cancelled)
        },
    )
    .unwrap_err();
    assert!(matches!(result.cause, Error::InvalidLimits));
    assert_eq!(result.usage, snapshot::Usage::default());
    assert!(!called);
}

#[test]
fn invalid_bytes_cap() {
    invalid(Limits {
        max_bytes: MAX_BYTES + 1,
        ..Limits::default()
    });
}

#[test]
fn invalid_io_cap() {
    invalid(Limits {
        max_io_bytes: MAX_IO_BYTES + 1,
        ..Limits::default()
    });
}

#[test]
fn invalid_calls_zero() {
    invalid(Limits {
        max_read_calls: 0,
        ..Limits::default()
    });
}

#[test]
fn invalid_calls_cap() {
    invalid(Limits {
        max_read_calls: MAX_READ_CALLS + 1,
        ..Limits::default()
    });
}

#[test]
fn invalid_deadline_zero() {
    invalid(Limits {
        max_duration: Duration::ZERO,
        ..Limits::default()
    });
}

#[test]
fn invalid_deadline_cap() {
    invalid(Limits {
        max_duration: Duration::from_secs(301),
        ..Limits::default()
    });
}

#[test]
fn source_over_maximum_rejected_before_data_open() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", &pattern(MAX_BYTES as usize + 1));
    let failure = get(&path, Limits::default()).unwrap_err();
    assert!(matches!(failure.cause, Error::Limit(LimitKind::Bytes)));
    assert_eq!(failure.usage.open_calls, 1);
    assert_eq!(failure.usage.metadata_calls, 1);
    assert_eq!(failure.usage.read_calls, 0);
    sandbox.finish();
}

#[test]
fn empty_accepts_zero_byte_limit_with_exact_eof_budget() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"");
    let result = get(
        &path,
        Limits {
            max_bytes: 0,
            max_io_bytes: 2,
            max_read_calls: 2,
            ..Limits::default()
        },
    )
    .unwrap();
    expect_usage(&result, 0);
    sandbox.finish();
}

#[test]
fn nonempty_rejects_zero_byte_limit() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"x");
    let failure = get(
        &path,
        Limits {
            max_bytes: 0,
            ..Limits::default()
        },
    )
    .unwrap_err();
    assert!(matches!(failure.cause, Error::Limit(LimitKind::Bytes)));
    assert_eq!(failure.usage.read_calls, 0);
    sandbox.finish();
}

#[test]
fn zero_io_budget_is_valid_but_cannot_probe_eof() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"");
    let failure = get(
        &path,
        Limits {
            max_io_bytes: 0,
            ..Limits::default()
        },
    )
    .unwrap_err();
    assert!(matches!(failure.cause, Error::Limit(LimitKind::IoBytes)));
    assert_eq!(failure.usage.read_calls, 0);
    assert_eq!(failure.usage.reserved_read_bytes, 0);
    sandbox.finish();
}

#[test]
fn exact_io_and_call_budget_includes_both_eof_probes() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", &pattern(65_537));
    let result = get(
        &path,
        Limits {
            max_io_bytes: 131_076,
            max_read_calls: 6,
            ..Limits::default()
        },
    )
    .unwrap();
    expect_usage(&result, 65_537);
    sandbox.finish();
}

#[test]
fn one_less_io_budget_fails_last_eof_without_refund() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", &pattern(65_537));
    let failure = get(
        &path,
        Limits {
            max_io_bytes: 131_075,
            ..Limits::default()
        },
    )
    .unwrap_err();
    assert!(matches!(failure.cause, Error::Limit(LimitKind::IoBytes)));
    assert_eq!(failure.usage.read_calls, 5);
    assert_eq!(failure.usage.reserved_read_bytes, 131_075);
    assert_eq!(failure.usage.actual_read_bytes, 131_074);
    assert_eq!(failure.usage.compared_bytes, 65_537);
    sandbox.finish();
}

#[test]
fn one_less_call_budget_fails_last_eof() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", &pattern(65_537));
    let failure = get(
        &path,
        Limits {
            max_read_calls: 5,
            ..Limits::default()
        },
    )
    .unwrap_err();
    assert!(matches!(failure.cause, Error::Limit(LimitKind::ReadCalls)));
    assert_eq!(failure.usage.read_calls, 5);
    assert_eq!(failure.usage.reserved_read_bytes, 131_075);
    sandbox.finish();
}

#[test]
fn retry_usage_is_per_attempt_not_hidden_cumulative_state() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"x");
    let failure = get(
        &path,
        Limits {
            max_read_calls: 1,
            ..Limits::default()
        },
    )
    .unwrap_err();
    assert!(matches!(failure.cause, Error::Limit(LimitKind::ReadCalls)));
    assert_eq!(failure.usage.actual_read_bytes, 1);
    let result = get(&path, Limits::default()).unwrap();
    expect_usage(&result, 1);
    sandbox.finish();
}
