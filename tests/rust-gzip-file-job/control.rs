use crate::{fixtures::*, support::*};
use job::{Event, FileJob, Limits, Outcome, Report, Stop, gzip, publication, snapshot};
use std::{fs, rc::Rc};

#[derive(Clone, Copy)]
enum Target {
    Snapshot(snapshot::Stage),
    Decode(gzip::Stage),
    Publication(publication::Stage),
}

impl Target {
    fn matches(self, event: Event) -> bool {
        match (self, event) {
            (Self::Snapshot(stage), Event::Snapshot(point)) => stage == point.stage,
            (Self::Decode(stage), Event::Decode(point)) => stage == point.stage,
            (Self::Publication(stage), Event::Publication(point)) => stage == point.stage,
            _ => false,
        }
    }
}

fn cancelled(report: &Report<()>, target: Target) {
    match (&report.outcome, target) {
        (Outcome::SnapshotFailed(failure), Target::Snapshot(_)) => {
            assert!(matches!(failure.cause, snapshot::Error::Cancelled))
        }
        (
            Outcome::DecodeFailed(gzip::Error::Reader(gzip::Failure::Cancelled)),
            Target::Decode(_),
        ) => {}
        (Outcome::Publication(native), Target::Publication(stage)) => {
            assert!(matches!(
                native.outcome,
                publication::Outcome::NotCommitted(publication::Error::Cancelled)
            ));
            assert_eq!(native.usage.link_calls, 0);
            cleanup(
                &native.cleanup,
                match stage {
                    publication::Stage::Opening => 0,
                    publication::Stage::BeforeCommit => 4,
                    _ => 2,
                },
            );
        }
        _ => panic!("취소 원인이 해당 단계에 보존되지 않음"),
    }
}

fn cancel_at(target: Target, local_stop: bool) {
    let sandbox = Sandbox::new();
    let archive = stored(b"abc");
    let input = sandbox.file("input.gz", &archive);
    let before = identity(&input);
    let parent = sandbox.parent();
    let (task, cancel) = FileJob::new();
    let mut trace = Trace::default();
    let mut reached = false;
    let report = task.run(request(&input, &parent, Limits::default()), |point| {
        trace.push(point);
        if target.matches(point.event) {
            reached = true;
            if local_stop {
                return Err(Stop::<()>::Cancelled);
            }
            assert_eq!(cancel.request(), publication::CancelDecision::Requested);
            assert_eq!(
                cancel.clone().request(),
                publication::CancelDecision::AlreadyRequested
            );
        }
        Ok(())
    });
    assert!(reached);
    trace.finish(&report);
    assert!(target.matches(trace.points.last().unwrap().event));
    assert_eq!(report.usage, trace.points.last().unwrap().usage);
    cancelled(&report, target);
    assert_eq!(cancel.request(), publication::CancelDecision::Finished);
    assert!(names(&parent).is_empty());
    assert_eq!(identity(&input), before);
    sandbox.finish();
}

#[test]
fn pre_cancel_stops_before_snapshot_open() {
    let sandbox = Sandbox::new();
    let input = sandbox.file("input.gz", &stored(b"abc"));
    let (task, cancel) = FileJob::new();
    assert_eq!(cancel.request(), publication::CancelDecision::Requested);
    assert_eq!(
        cancel.clone().request(),
        publication::CancelDecision::AlreadyRequested
    );
    let mut trace = Trace::default();
    let report = task.run(
        request(&input, &sandbox.parent(), Limits::default()),
        |point| {
            trace.push(point);
            Ok::<_, Stop<()>>(())
        },
    );
    trace.finish(&report);
    assert_eq!(trace.points.len(), 1);
    assert_eq!(report.usage, job::Usage::default());
    cancelled(&report, Target::Snapshot(snapshot::Stage::Opening));
    assert_eq!(cancel.request(), publication::CancelDecision::Finished);
    assert!(names(&sandbox.parent()).is_empty());
    sandbox.finish();
}

fn observer_error(target: Target) {
    let sandbox = Sandbox::new();
    let input = sandbox.file("input.gz", &stored(b"abc"));
    let parent = sandbox.parent();
    let marker = Rc::new(String::from("원래 observer 오류"));
    let (task, cancel) = FileJob::new();
    let mut trace = Trace::default();
    let report = task.run(request(&input, &parent, Limits::default()), |point| {
        trace.push(point);
        if target.matches(point.event) {
            assert_eq!(cancel.request(), publication::CancelDecision::Requested);
            Err(Stop::Callback(marker.clone()))
        } else {
            Ok(())
        }
    });
    trace.finish(&report);
    let actual = match &report.outcome {
        Outcome::SnapshotFailed(snapshot::Failure {
            cause: snapshot::Error::Callback(value),
            ..
        }) => value,
        Outcome::DecodeFailed(gzip::Error::Callback(value)) => value,
        Outcome::Publication(native) => {
            cleanup(&native.cleanup, 4);
            assert_eq!(native.usage.link_calls, 0);
            match &native.outcome {
                publication::Outcome::NotCommitted(publication::Error::Callback(value)) => value,
                _ => panic!("게시 callback 오류 원형이 사라짐"),
            }
        }
        _ => panic!("callback 오류 원형이 사라짐"),
    };
    assert!(Rc::ptr_eq(actual, &marker));
    assert_eq!(report.usage, trace.points.last().unwrap().usage);
    assert!(target.matches(trace.points.last().unwrap().event));
    assert_eq!(cancel.request(), publication::CancelDecision::Finished);
    assert!(names(&parent).is_empty());
    sandbox.finish();
}

#[test]
fn snapshot_observer_error_beats_accepted_cancel() {
    observer_error(Target::Snapshot(snapshot::Stage::Finalizing));
}
#[test]
fn decode_observer_error_beats_accepted_cancel() {
    observer_error(Target::Decode(gzip::Stage::AfterOutput));
}
#[test]
fn publication_observer_error_beats_accepted_cancel() {
    observer_error(Target::Publication(publication::Stage::BeforeCommit));
}

#[test]
fn local_snapshot_stop_does_not_need_cancel_request() {
    cancel_at(Target::Snapshot(snapshot::Stage::Comparing), true);
}
#[test]
fn local_decode_stop_does_not_need_cancel_request() {
    cancel_at(Target::Decode(gzip::Stage::AfterOutput), true);
}
#[test]
fn local_publication_stop_does_not_need_cancel_request() {
    cancel_at(Target::Publication(publication::Stage::BeforeCommit), true);
}

#[test]
fn cancellation_after_return_preserves_committed_bytes() {
    let sandbox = Sandbox::new();
    let archive = stored(b"abc");
    let input = sandbox.file("input.gz", &archive);
    let parent = sandbox.parent();
    let (task, cancel) = FileJob::new();
    let report = task.run(request(&input, &parent, Limits::default()), |_| {
        Ok::<_, Stop<()>>(())
    });
    let before = identity(&parent.join("output"));
    assert_eq!(cancel.request(), publication::CancelDecision::Finished);
    assert_eq!(
        cancel.clone().request(),
        publication::CancelDecision::Finished
    );
    committed(&report, &parent, b"abc");
    assert_eq!(identity(&parent.join("output")), before);
    assert_eq!(fs::read(&input).unwrap(), archive);
    sandbox.finish();
}

#[test]
fn dropping_unused_job_finishes_its_gate() {
    let (task, cancel) = FileJob::new();
    drop(task);
    assert_eq!(cancel.request(), publication::CancelDecision::Finished);
}

#[test]
fn cancel_snapshot_opening() {
    cancel_at(Target::Snapshot(snapshot::Stage::Opening), false);
}

#[test]
fn cancel_snapshot_pinned() {
    cancel_at(Target::Snapshot(snapshot::Stage::Pinned), false);
}

#[test]
fn cancel_snapshot_copying() {
    cancel_at(Target::Snapshot(snapshot::Stage::Copying), false);
}

#[test]
fn cancel_snapshot_comparing() {
    cancel_at(Target::Snapshot(snapshot::Stage::Comparing), false);
}

#[test]
fn cancel_snapshot_finalizing() {
    cancel_at(Target::Snapshot(snapshot::Stage::Finalizing), false);
}

#[test]
fn cancel_decode_start() {
    cancel_at(Target::Decode(gzip::Stage::Start), false);
}

#[test]
fn cancel_decode_header() {
    cancel_at(Target::Decode(gzip::Stage::Header), false);
}

#[test]
fn cancel_decode_before_inflate() {
    cancel_at(Target::Decode(gzip::Stage::BeforeInflate), false);
}

#[test]
fn cancel_decode_after_inflate() {
    cancel_at(Target::Decode(gzip::Stage::AfterInflate), false);
}

#[test]
fn cancel_decode_after_output() {
    cancel_at(Target::Decode(gzip::Stage::AfterOutput), false);
}

#[test]
fn cancel_decode_trailer() {
    cancel_at(Target::Decode(gzip::Stage::Trailer), false);
}

#[test]
fn cancel_decode_before_return() {
    cancel_at(Target::Decode(gzip::Stage::BeforeReturn), false);
}

#[test]
fn cancel_publication_opening() {
    cancel_at(Target::Publication(publication::Stage::Opening), false);
}

#[test]
fn cancel_publication_writing() {
    cancel_at(Target::Publication(publication::Stage::Writing), false);
}

#[test]
fn cancel_publication_verifying() {
    cancel_at(Target::Publication(publication::Stage::Verifying), false);
}

#[test]
fn cancel_publication_before_commit() {
    cancel_at(Target::Publication(publication::Stage::BeforeCommit), false);
}
