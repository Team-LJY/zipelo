use crate::support::*;
use publication::{CancelDecision, Error, Limits, Outcome, Publication, Request, Stage, Stop};
use std::{ffi::OsStr, sync::mpsc, thread, time::Duration};

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn two_prepared_publishers_have_exactly_one_complete_winner() {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let (ready_tx, ready_rx) = mpsc::sync_channel(2);
    let (done_tx, done_rx) = mpsc::sync_channel(2);
    let mut releases = Vec::new();
    let mut handles = Vec::new();
    for marker in [65_u8, 66_u8] {
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        releases.push(release_tx);
        let ready = ready_tx.clone();
        let done = done_tx.clone();
        let parent = parent.clone();
        handles.push(thread::spawn(move || {
            let bytes = vec![marker; 65_537];
            let (task, cancel) = Publication::new();
            let report = task.publish(
                Request {
                    parent: &parent,
                    leaf: OsStr::new("output"),
                    bytes: &bytes,
                    limits: Limits::default(),
                },
                |point| {
                    if point.stage == Stage::BeforeCommit {
                        ready.send(marker).unwrap();
                        release_rx
                            .recv_timeout(WAIT)
                            .expect("게시 해제 신호 시간 초과");
                    }
                    Ok::<_, Stop<()>>(())
                },
            );
            assert_eq!(cancel.request(), CancelDecision::Finished);
            assert!(done.send((marker, report)).is_ok());
        }));
    }
    drop(ready_tx);
    drop(done_tx);
    let mut ready = vec![
        ready_rx.recv_timeout(WAIT).unwrap(),
        ready_rx.recv_timeout(WAIT).unwrap(),
    ];
    ready.sort();
    assert_eq!(ready, [65, 66]);
    assert!(names(&parent).is_empty());
    for release in releases {
        release.send(()).unwrap();
    }
    let results = [
        done_rx.recv_timeout(WAIT).unwrap(),
        done_rx.recv_timeout(WAIT).unwrap(),
    ];
    for handle in handles {
        handle.join().expect("게시 스레드 실패");
    }
    assert_eq!(
        results
            .iter()
            .filter(|(_, r)| matches!(&r.outcome, Outcome::Committed { .. }))
            .count(),
        1
    );
    for (marker, report) in results {
        if matches!(&report.outcome, Outcome::Committed { .. }) {
            committed(report, &parent, OsStr::new("output"), &vec![marker; 65_537]);
        } else {
            conflict(report, 65_537);
        }
    }
    sandbox.finish();
}

#[test]
fn acknowledged_cancellation_before_commit_prevents_link() {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let worker_parent = parent.clone();
    let (task, cancel) = Publication::new();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        let report = task.publish(
            Request {
                parent: &worker_parent,
                leaf: OsStr::new("output"),
                bytes: b"abc",
                limits: Limits::default(),
            },
            |point| {
                if point.stage == Stage::BeforeCommit {
                    ready_tx.send(()).unwrap();
                    release_rx
                        .recv_timeout(WAIT)
                        .expect("취소 확인 신호 시간 초과");
                }
                Ok::<_, Stop<()>>(())
            },
        );
        assert!(done_tx.send(report).is_ok());
    });
    ready_rx.recv_timeout(WAIT).unwrap();
    assert_eq!(cancel.request(), CancelDecision::Requested);
    assert_eq!(cancel.request(), CancelDecision::AlreadyRequested);
    release_tx.send(()).unwrap();
    let report = done_rx.recv_timeout(WAIT).unwrap();
    handle.join().expect("취소 스레드 실패");
    let (error, usage, closed) = not_committed(report);
    assert!(matches!(error, Error::Cancelled));
    assert_eq!(usage.link_calls, 0);
    assert_eq!(usage.actual_written_bytes, 3);
    assert_eq!(usage.actual_read_bytes, 3);
    cleanup(&closed, 4);
    assert_eq!(cancel.request(), CancelDecision::Finished);
    assert!(names(&parent).is_empty());
    sandbox.finish();
}
