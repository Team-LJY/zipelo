use crate::{fixtures::*, support::*};
use job::{Event, FileJob, Limits, Outcome, Stop, publication};
use std::{sync::mpsc, thread, time::Duration};

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn two_file_jobs_for_one_name_have_one_complete_winner() {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let (ready_tx, ready_rx) = mpsc::sync_channel(2);
    let (done_tx, done_rx) = mpsc::sync_channel(2);
    let mut releases = Vec::new();
    let mut handles = Vec::new();
    let mut inputs = Vec::new();
    for marker in [65_u8, 66_u8] {
        let archive = stored(&vec![marker; 65_537]);
        let input = sandbox.file(format!("input-{marker}.gz"), &archive);
        inputs.push((input.clone(), identity(&input), archive));
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        releases.push(release_tx);
        let ready = ready_tx.clone();
        let done = done_tx.clone();
        let parent = parent.clone();
        handles.push(thread::spawn(move || {
            let (task, cancel) = FileJob::new();
            let mut trace = Trace::default();
            let report = task.run(request(&input, &parent, Limits::default()), |point| {
                trace.push(point);
                if matches!(point.event, Event::Publication(native) if native.stage == publication::Stage::BeforeCommit) {
                    ready.send(marker).unwrap();
                    release_rx.recv_timeout(WAIT).expect("게시 해제 신호 시간 초과");
                }
                Ok::<_, Stop<()>>(())
            });
            trace.finish(&report);
            assert_eq!(cancel.request(), publication::CancelDecision::Finished);
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
        handle.join().expect("파일 작업 스레드 실패");
    }
    assert_eq!(results.iter().filter(|(_, report)| matches!(
        &report.outcome, Outcome::Publication(native) if matches!(&native.outcome, publication::Outcome::Committed { .. })
    )).count(), 1);
    for (marker, report) in results {
        let (_, before, archive) = &inputs[(marker - 65) as usize];
        observations(
            &report,
            (before.0, before.1),
            archive,
            &vec![marker; 65_537],
            10,
        );
        if matches!(&report.outcome, Outcome::Publication(native) if matches!(&native.outcome, publication::Outcome::Committed { .. }))
        {
            committed(&report, &parent, &vec![marker; 65_537]);
        } else {
            conflict(&report, 65_537);
        }
    }
    for (input, before, archive) in inputs {
        assert_eq!(identity(&input), before);
        assert_eq!(std::fs::read(input).unwrap(), archive);
    }
    sandbox.finish();
}

#[test]
fn acknowledged_external_cancel_before_commit_prevents_publication() {
    let sandbox = Sandbox::new();
    let input = sandbox.file("input.gz", &stored(b"abc"));
    let parent = sandbox.parent();
    let worker_parent = parent.clone();
    let (task, cancel) = FileJob::new();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        let mut trace = Trace::default();
        let report = task.run(request(&input, &worker_parent, Limits::default()), |point| {
            trace.push(point);
            if matches!(point.event, Event::Publication(native) if native.stage == publication::Stage::BeforeCommit) {
                ready_tx.send(()).unwrap();
                release_rx.recv_timeout(WAIT).expect("취소 확인 신호 시간 초과");
            }
            Ok::<_, Stop<()>>(())
        });
        trace.finish(&report);
        assert!(done_tx.send(report).is_ok());
    });
    ready_rx.recv_timeout(WAIT).unwrap();
    assert_eq!(cancel.request(), publication::CancelDecision::Requested);
    assert_eq!(
        cancel.clone().request(),
        publication::CancelDecision::AlreadyRequested
    );
    release_tx.send(()).unwrap();
    let report = done_rx.recv_timeout(WAIT).unwrap();
    handle.join().expect("취소 스레드 실패");
    match &report.outcome {
        Outcome::Publication(native) => {
            assert!(matches!(
                native.outcome,
                publication::Outcome::NotCommitted(publication::Error::Cancelled)
            ));
            assert_eq!(native.usage.link_calls, 0);
            assert_eq!(native.usage.actual_written_bytes, 3);
            assert_eq!(native.usage.actual_read_bytes, 3);
            cleanup(&native.cleanup, 4);
        }
        _ => panic!("게시 전 취소가 보존되지 않음"),
    }
    assert_eq!(cancel.request(), publication::CancelDecision::Finished);
    assert!(names(&parent).is_empty());
    sandbox.finish();
}
