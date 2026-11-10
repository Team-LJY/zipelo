use crate::{fixtures::*, support::*};
use job::{Limits, Outcome, gzip, publication, snapshot};
use std::time::Duration;

fn snapshot_limit(kind: snapshot::LimitKind) {
    let sandbox = Sandbox::new();
    let archive = stored(b"abc");
    let length = archive.len() as u64;
    let input = sandbox.file("input.gz", &archive);
    let parent = sandbox.parent();
    let mut limits = Limits::default();
    match kind {
        snapshot::LimitKind::Bytes => limits.snapshot.max_bytes = length - 1,
        snapshot::LimitKind::IoBytes => limits.snapshot.max_io_bytes = 2 * length + 1,
        snapshot::LimitKind::ReadCalls => limits.snapshot.max_read_calls = 3,
        _ => unreachable!(),
    }
    let (report, _) = run(&input, &parent, limits);
    match &report.outcome {
        Outcome::SnapshotFailed(snapshot::Failure {
            cause: snapshot::Error::Limit(actual),
            usage,
        }) => {
            assert_eq!(*actual, kind);
            assert_eq!(*usage, report.usage.snapshot);
        }
        _ => panic!("snapshot 한도 오류가 아님"),
    }
    let expected = if kind == snapshot::LimitKind::Bytes {
        snapshot::Usage {
            open_calls: 1,
            metadata_calls: 1,
            ..snapshot::Usage::default()
        }
    } else {
        snapshot::Usage {
            open_calls: 4,
            metadata_calls: 4,
            read_calls: 3,
            reserved_read_bytes: 2 * length + 1,
            actual_read_bytes: 2 * length,
            compared_bytes: length,
        }
    };
    assert_eq!(report.usage.snapshot, expected);
    assert!(names(&parent).is_empty());
    sandbox.finish();
}
#[test]
fn snapshot_byte_limit_preserves_partial_usage() {
    snapshot_limit(snapshot::LimitKind::Bytes);
}
#[test]
fn snapshot_io_limit_retains_last_eof_reservation_boundary() {
    snapshot_limit(snapshot::LimitKind::IoBytes);
}
#[test]
fn snapshot_call_limit_retains_last_eof_reservation_boundary() {
    snapshot_limit(snapshot::LimitKind::ReadCalls);
}

fn gzip_limit(field: &str) {
    let sandbox = Sandbox::new();
    let archive = stored(b"abc");
    let length = archive.len() as u64;
    let input = sandbox.file("input.gz", &archive);
    let parent = sandbox.parent();
    let mut limits = Limits::default();
    let expected = match field {
        "input" => {
            limits.gzip.input = length - 1;
            gzip::Usage::default()
        }
        "processed" => {
            limits.gzip.processed = length + 2;
            gzip::Usage {
                processed: length + 3,
                output: 0,
                metadata: 10,
            }
        }
        "output" => {
            limits.gzip.output = 2;
            gzip::Usage {
                processed: length + 3,
                output: 0,
                metadata: 10,
            }
        }
        "metadata" => {
            limits.gzip.metadata = 9;
            gzip::Usage {
                processed: 9,
                output: 0,
                metadata: 9,
            }
        }
        _ => unreachable!(),
    };
    let (report, trace) = run(&input, &parent, limits);
    assert!(matches!(
        report.outcome,
        Outcome::DecodeFailed(gzip::Error::Reader(gzip::Failure::Budget))
    ));
    assert_eq!(report.usage.gzip, expected);
    snapshot_usage(report.usage.snapshot, archive.len());
    if field == "processed" {
        assert!(report.usage.gzip.processed > trace.points.last().unwrap().usage.gzip.processed);
    }
    assert!(names(&parent).is_empty());
    sandbox.finish();
}
#[test]
fn gzip_input_limit_stops_after_snapshot() {
    gzip_limit("input");
}
#[test]
fn gzip_processed_failure_includes_post_checkpoint_charge() {
    gzip_limit("processed");
}
#[test]
fn gzip_selected_output_limit_preserves_consumed_cost() {
    gzip_limit("output");
}
#[test]
fn gzip_metadata_limit_preserves_header_cost() {
    gzip_limit("metadata");
}

fn publication_limit(field: &str) {
    let sandbox = Sandbox::new();
    let archive = stored(b"abc");
    let input = sandbox.file("input.gz", &archive);
    let parent = sandbox.parent();
    let mut limits = Limits::default();
    match field {
        "bytes" => limits.publication.max_bytes = 2,
        "io" => limits.publication.max_io_bytes = 6,
        "calls" => limits.publication.max_io_calls = 2,
        _ => unreachable!(),
    }
    let (report, _) = run(&input, &parent, limits);
    let native = match &report.outcome {
        Outcome::Publication(native) => native,
        _ => panic!("게시 단계 미도달"),
    };
    assert!(matches!(
        native.outcome,
        publication::Outcome::NotCommitted(publication::Error::LimitExceeded)
    ));
    let expected = if field == "bytes" {
        cleanup(&native.cleanup, 0);
        publication::Usage::default()
    } else {
        cleanup(&native.cleanup, 2);
        publication::Usage {
            open_calls: 2,
            metadata_calls: 3,
            sync_calls: 1,
            write_calls: 1,
            read_calls: 1,
            reserved_io_bytes: 6,
            actual_written_bytes: 3,
            actual_read_bytes: 3,
            ..publication::Usage::default()
        }
    };
    assert_eq!(report.usage.publication, expected);
    snapshot_usage(report.usage.snapshot, archive.len());
    gzip_usage(report.usage.gzip, &archive, b"abc", 10);
    assert!(names(&parent).is_empty());
    sandbox.finish();
}
#[test]
fn publication_byte_limit_preserves_completed_prior_stages() {
    publication_limit("bytes");
}
#[test]
fn publication_io_limit_retains_pre_eof_usage() {
    publication_limit("io");
}
#[test]
fn publication_call_limit_retains_pre_eof_usage() {
    publication_limit("calls");
}

#[test]
fn exact_limits_in_all_stages_allow_commit() {
    let sandbox = Sandbox::new();
    let archive = stored(b"abc");
    let length = archive.len() as u64;
    let input = sandbox.file("input.gz", &archive);
    let before = identity(&input);
    let parent = sandbox.parent();
    let limits = Limits {
        snapshot: snapshot::Limits {
            max_bytes: length,
            max_io_bytes: 2 * length + 2,
            max_read_calls: 4,
            ..snapshot::Limits::default()
        },
        gzip: gzip::Limits {
            input: length,
            processed: length + 3,
            output: 3,
            metadata: 10,
            ..gzip::Limits::default()
        },
        publication: publication::Limits {
            max_bytes: 3,
            max_io_bytes: 7,
            max_io_calls: 3,
            ..publication::Limits::default()
        },
    };
    let (report, _) = run(&input, &parent, limits);
    observations(&report, (before.0, before.1), &archive, b"abc", 10);
    committed(&report, &parent, b"abc");
    sandbox.finish();
}

fn invalid_duration(stage: u8) {
    let sandbox = Sandbox::new();
    let archive = stored(b"abc");
    let input = sandbox.file("input.gz", &archive);
    let parent = sandbox.parent();
    let mut limits = Limits::default();
    match stage {
        0 => limits.snapshot.max_duration = Duration::ZERO,
        1 => limits.gzip.duration = Duration::ZERO,
        2 => limits.publication.max_duration = Duration::ZERO,
        _ => unreachable!(),
    }
    let (report, trace) = run(&input, &parent, limits);
    match (&report.outcome, stage) {
        (Outcome::SnapshotFailed(failure), 0) => {
            assert!(matches!(failure.cause, snapshot::Error::InvalidLimits));
            assert_eq!(report.usage, job::Usage::default());
            assert!(trace.points.is_empty());
        }
        (Outcome::DecodeFailed(gzip::Error::Reader(gzip::Failure::InvalidRequest)), 1) => {
            snapshot_usage(report.usage.snapshot, archive.len());
            assert_eq!(report.usage.gzip, gzip::Usage::default());
            assert!(trace.points.iter().all(|p| phase(p.event) == 0));
        }
        (Outcome::Publication(native), 2) => {
            snapshot_usage(report.usage.snapshot, archive.len());
            gzip_usage(report.usage.gzip, &archive, b"abc", 10);
            assert!(matches!(
                native.outcome,
                publication::Outcome::NotCommitted(publication::Error::InvalidLimits)
            ));
            assert_eq!(native.usage, publication::Usage::default());
            cleanup(&native.cleanup, 0);
            assert!(trace.points.iter().all(|p| phase(p.event) <= 1));
        }
        _ => panic!("단계별 잘못된 한도 오류가 아님"),
    }
    assert!(names(&parent).is_empty());
    sandbox.finish();
}
#[test]
fn invalid_snapshot_duration_has_zero_cost() {
    invalid_duration(0);
}
#[test]
fn invalid_decode_duration_retains_snapshot_cost() {
    invalid_duration(1);
}
#[test]
fn invalid_publication_duration_retains_both_prior_costs() {
    invalid_duration(2);
}
