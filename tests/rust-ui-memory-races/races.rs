#![forbid(unsafe_code)]
#![allow(dead_code)]
include!("assembly.rs");

use controller::{CloseState, Controller, Status};
use runtime::{Action, Completion, DEFAULT_CAPACITY, Outcome, Task};
use std::{
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(5);

struct Gate {
    reached: SyncSender<&'static str>,
    release: Receiver<()>,
}
struct Driver {
    reached: Receiver<&'static str>,
    release: SyncSender<()>,
}
fn gate() -> (Gate, Driver) {
    let (reached, observe) = mpsc::sync_channel(1);
    let (release, resume) = mpsc::sync_channel(1);
    (
        Gate {
            reached,
            release: resume,
        },
        Driver {
            reached: observe,
            release,
        },
    )
}
impl Gate {
    fn hold(&self, stage: &'static str) {
        eprintln!("worker reached {stage}");
        self.reached.try_send(stage).unwrap();
        self.release.recv_timeout(WAIT).expect(stage);
        eprintln!("worker released {stage}");
    }
}
impl Driver {
    fn at(&self, stage: &'static str) {
        assert_eq!(self.reached.recv_timeout(WAIT).expect(stage), stage);
    }
    fn release(&self) {
        self.release.try_send(()).unwrap();
    }
}

fn join_bounded<T>(handle: thread::JoinHandle<T>) -> thread::Result<T> {
    let deadline = Instant::now() + WAIT;
    while !handle.is_finished() {
        assert!(Instant::now() < deadline, "시험 소유 스레드 종료 기한 초과");
        thread::yield_now();
    }
    handle.join()
}

// frame 반환을 받은 뒤에만 driver가 worker 장벽을 해제함.
fn frame(mut c: Controller, op: impl FnOnce(&mut Controller) + Send + 'static) -> Controller {
    let (returned, observed) = mpsc::sync_channel(1);
    let owner = thread::spawn(move || {
        op(&mut c);
        returned.try_send(c).ok().expect("frame 반환 수신자 유지");
    });
    let c = observed
        .recv_timeout(WAIT)
        .expect("worker 해제 전 frame 반환 필요");
    join_bounded(owner).unwrap();
    c
}
fn settle(mut c: Controller) -> Controller {
    let deadline = Instant::now() + WAIT;
    while c.busy() {
        c = frame(c, |c| c.poll());
        assert!(Instant::now() < deadline, "join 관측 기한 초과");
        thread::yield_now();
    }
    assert!(c.task.as_ref().unwrap().joined());
    assert!(c.task.as_mut().unwrap().poll_join().is_none());
    c
}

fn terminal_gates(action: Action) -> (Controller, Driver, Driver) {
    let (before, pre) = gate();
    let (after, post) = gate();
    let task = runtime::assemble(move |op, events, done| {
        let completion = runtime::execute(op, action, events);
        before.hold("terminal-created");
        let fallback = runtime::deliver_completion(&done, completion);
        assert!(fallback.is_none());
        after.hold("terminal-delivered");
        fallback
    });
    (controller::attach(task), pre, post)
}
fn no_payload(c: &Controller) {
    assert!(c.rows().is_empty());
    assert!(c.selected_bytes().is_none());
}
fn current_handle(c: &Controller, id: core::OperationId) {
    assert!(c.busy());
    assert_eq!(c.task.as_ref().unwrap().id, id);
    assert!(!c.task.as_ref().unwrap().finished());
    assert!(!c.task.as_ref().unwrap().joined());
}

#[test]
fn public_spawn_cancel_before_native_entry_is_ordered() {
    let (worker, driver) = gate();
    let task = Task::spawn_with(DEFAULT_CAPACITY, move |op, events| {
        worker.hold("before-native");
        runtime::execute(op, Action::Gzip, events)
    })
    .unwrap();
    driver.at("before-native");
    let c = frame(controller::attach(task), |c| {
        assert!(c.cancel());
        assert!(!c.cancel());
        c.poll();
        assert_eq!(c.status, Status::CancelRequested);
    });
    driver.release();
    let c = settle(c);
    assert_eq!(c.status, Status::Cancelled);
    no_payload(&c);
}

#[test]
fn real_checkpoint_cancel_is_observed_after_frame_handshake() {
    let (worker, driver) = gate();
    let task = Task::spawn_with(DEFAULT_CAPACITY, move |op, events| {
        let mut budget = core::gzip::Budget::new(core::gzip::Limits::default()).unwrap();
        let mut first = true;
        let mut transport = transport::Transport::new(events);
        let terminal = op.gzip(
            core::gzip::Request {
                input: runtime::GZIP.into(),
                selected: true,
            },
            &mut budget,
            |event| {
                let result = transport.forward(
                    event.stamp().sequence(),
                    runtime::ProgressEvent::Gzip(event),
                    Ok(()),
                    || core::gzip::Stop::Cancelled,
                );
                if first {
                    first = false;
                    worker.hold("native-checkpoint");
                }
                result
            },
        );
        Completion {
            outcome: Outcome::Gzip(terminal),
            delivery: transport.fault,
        }
    })
    .unwrap();
    driver.at("native-checkpoint");
    let id = task.id;
    let c = frame(controller::attach(task), move |c| {
        c.poll();
        assert!(c.progress.is_some());
        assert!(c.cancel());
        current_handle(c, id);
        no_payload(c);
    });
    driver.release();
    let c = settle(c);
    assert_eq!(c.status, Status::Cancelled);
    match &c.terminal.as_ref().unwrap().outcome {
        Outcome::Gzip(t) => {
            assert!(t.cancellation_requested());
            assert!(t.cancellation_forwarded());
        }
        _ => panic!("형식 불일치"),
    }
}

#[test]
fn terminal_created_cancel_then_delivered_success_is_preserved() {
    let (c, pre, post) = terminal_gates(Action::Gzip);
    pre.at("terminal-created");
    let c = frame(c, |c| {
        assert!(c.cancel());
        c.poll();
        assert!(c.terminal.is_none());
    });
    pre.release();
    post.at("terminal-delivered");
    let c = frame(c, |c| {
        c.poll();
        assert!(c.terminal.as_ref().unwrap().outcome.is_ok());
        assert!(c.busy());
        no_payload(c);
    });
    post.release();
    let c = settle(c);
    assert_eq!(c.status, Status::Completed);
    assert!(!c.terminal.as_ref().unwrap().outcome.cancelled());
}

#[test]
fn terminal_received_cancel_cannot_apply_payload_before_join() {
    let (c, pre, post) = terminal_gates(Action::Gzip);
    pre.at("terminal-created");
    pre.release();
    post.at("terminal-delivered");
    let c = frame(c, |c| {
        c.poll();
        assert!(c.terminal.is_some());
        assert!(c.cancel());
        assert!(!c.cancel());
        assert_eq!(c.status, Status::CancelRequested);
        assert!(!c.start(Action::Zip));
        assert!(c.terminal_delivery.is_none());
        no_payload(c);
    });
    post.release();
    let c = settle(c);
    assert_eq!(c.status, Status::Completed);
    assert_eq!(c.terminal_delivery, Some(core::Delivery::Accepted));
}

#[test]
fn close_confirmation_retracts_with_terminal_held_before_delivery() {
    let (c, pre, post) = terminal_gates(Action::Gzip);
    pre.at("terminal-created");
    let c = frame(c, |c| {
        c.request_close();
        c.poll();
        assert_eq!(c.close, CloseState::Confirming);
        assert!(!c.task.as_ref().unwrap().cancellation.is_requested());
        c.keep_open();
        assert_eq!(c.close, CloseState::Open);
    });
    pre.release();
    post.at("terminal-delivered");
    post.release();
    let c = settle(c);
    assert_eq!(c.close, CloseState::Open);
    assert_eq!(c.status, Status::Completed);
}

#[test]
fn close_waiting_requires_join_after_terminal_reception() {
    let (c, pre, post) = terminal_gates(Action::Gzip);
    pre.at("terminal-created");
    pre.release();
    post.at("terminal-delivered");
    let c = frame(c, |c| {
        c.poll();
        c.request_close();
        c.confirm_close();
        c.poll();
        assert_eq!(c.close, CloseState::Waiting);
        assert!(c.terminal.is_some());
        assert!(c.busy());
        no_payload(c);
        assert!(!c.start(Action::Ustar));
    });
    post.release();
    let c = settle(c);
    assert_eq!(c.close, CloseState::Allowed);
    assert_eq!(c.status, Status::Completed);
}

#[test]
fn waiting_retraction_before_join_keeps_cancel_and_success() {
    let (c, pre, post) = terminal_gates(Action::Gzip);
    pre.at("terminal-created");
    let c = frame(c, |c| {
        c.request_close();
        c.confirm_close();
        assert_eq!(c.close, CloseState::Waiting);
    });
    pre.release();
    post.at("terminal-delivered");
    let c = frame(c, |c| {
        c.poll();
        c.keep_open();
        c.poll();
        assert_eq!(c.close, CloseState::Open);
        assert!(c.task.as_ref().unwrap().cancellation.is_requested());
        assert!(c.busy());
        no_payload(c);
    });
    post.release();
    let c = settle(c);
    assert_eq!(c.close, CloseState::Open);
    assert_eq!(c.status, Status::Completed);
}

#[test]
fn confirmation_choice_survives_terminal_then_join_then_keep_open() {
    let (c, pre, post) = terminal_gates(Action::Gzip);
    pre.at("terminal-created");
    let c = frame(c, |c| c.request_close());
    pre.release();
    post.at("terminal-delivered");
    let c = frame(c, |c| {
        c.poll();
        assert_eq!(c.close, CloseState::Confirming);
        assert!(c.busy());
    });
    post.release();
    let c = settle(c);
    assert_eq!(c.close, CloseState::Confirming);
    let c = frame(c, |c| c.keep_open());
    assert_eq!(c.close, CloseState::Open);
    assert_eq!(c.status, Status::Completed);
}

#[test]
fn native_failure_held_until_join_is_not_rewritten_by_close_cancel() {
    let (c, pre, post) = terminal_gates(Action::InvalidGzip);
    pre.at("terminal-created");
    pre.release();
    post.at("terminal-delivered");
    let c = frame(c, |c| {
        c.poll();
        c.request_close();
        c.confirm_close();
        assert!(!c.terminal.as_ref().unwrap().outcome.is_ok());
        assert!(!c.terminal.as_ref().unwrap().outcome.cancelled());
        assert_eq!(c.close, CloseState::Waiting);
        no_payload(c);
    });
    post.release();
    let c = settle(c);
    assert_eq!(c.status, Status::NativeFailure);
    assert_eq!(c.close, CloseState::Allowed);
    assert!(
        matches!(&c.terminal.as_ref().unwrap().outcome, Outcome::Gzip(t) if matches!(t.result(), Err(core::Failure { native: Some(core::gzip::Error::Reader(core::gzip::Failure::Header)), .. })))
    );
}

#[test]
fn panic_after_terminal_delivery_preserves_result_without_applying_it() {
    let (worker, driver) = gate();
    let task = runtime::assemble(move |op, events, done| {
        let completion = runtime::execute(op, Action::Gzip, events);
        assert!(runtime::deliver_completion(&done, completion).is_none());
        worker.hold("before-panic");
        std::panic::panic_any(73_u32);
    });
    driver.at("before-panic");
    let c = frame(controller::attach(task), |c| {
        c.poll();
        c.request_close();
        c.confirm_close();
        assert!(c.terminal.is_some());
        no_payload(c);
    });
    driver.release();
    let c = settle(c);
    assert_eq!(c.status, Status::Panicked);
    assert_eq!(c.close, CloseState::Allowed);
    assert_eq!(c.panic.as_ref().unwrap().downcast_ref::<u32>(), Some(&73));
    assert!(c.terminal.as_ref().unwrap().outcome.is_ok());
    no_payload(&c);
}

#[test]
fn stale_progress_is_ignored_while_current_worker_is_held() {
    let (old, _) = core::Operation::start().unwrap();
    let (sender, events) = mpsc::sync_channel(DEFAULT_CAPACITY);
    let old_terminal = runtime::execute(old, Action::Gzip, sender);
    let stale: Vec<_> = events.try_iter().collect();
    let (worker, driver) = gate();
    let task = runtime::assemble(move |op, sender, done| {
        for event in stale {
            sender.try_send(event).unwrap();
        }
        worker.hold("stale-progress-sent");
        runtime::deliver_completion(&done, runtime::execute(op, Action::Gzip, sender))
    });
    driver.at("stale-progress-sent");
    let c = frame(controller::attach(task), |c| {
        c.poll();
        assert!(c.progress.is_none());
        assert!(c.delivery_error.is_none());
        assert!(c.busy());
    });
    driver.release();
    let c = settle(c);
    assert_eq!(c.status, Status::Completed);
    assert_ne!(
        c.task.as_ref().unwrap().id,
        match old_terminal.outcome {
            Outcome::Gzip(t) => t.stamp().operation(),
            _ => unreachable!(),
        }
    );
}

#[test]
fn foreign_terminal_received_before_join_cannot_authorize_close_early() {
    let (old, _) = core::Operation::start().unwrap();
    let (sender, _events) = mpsc::sync_channel(DEFAULT_CAPACITY);
    let foreign = runtime::execute(old, Action::Gzip, sender);
    let (worker, driver) = gate();
    let task = runtime::assemble(move |_, _, done| {
        assert!(runtime::deliver_completion(&done, foreign).is_none());
        worker.hold("foreign-delivered");
        None
    });
    driver.at("foreign-delivered");
    let c = frame(controller::attach(task), |c| {
        c.poll();
        c.request_close();
        c.confirm_close();
        assert_eq!(c.close, CloseState::Waiting);
        no_payload(c);
    });
    driver.release();
    let c = settle(c);
    assert_eq!(c.status, Status::DeliveryFailure);
    assert_eq!(c.terminal_delivery, Some(core::Delivery::IgnoredStale));
    assert_eq!(c.close, CloseState::Allowed);
    no_payload(&c);
}

#[test]
fn lost_completion_after_creation_is_recovered_only_after_join() {
    let (worker, driver) = gate();
    let task = runtime::assemble(move |op, events, done| {
        let completion = runtime::execute(op, Action::Gzip, events);
        worker.hold("completion-created-before-disconnect");
        runtime::deliver_completion(&done, completion)
    });
    driver.at("completion-created-before-disconnect");
    let c = frame(controller::attach(task), |c| {
        c.task.as_mut().unwrap().completion.take();
        c.request_close();
        c.confirm_close();
        c.poll();
        assert_eq!(c.close, CloseState::Waiting);
        assert!(c.terminal.is_none());
        no_payload(c);
    });
    driver.release();
    let c = settle(c);
    assert_eq!(c.status, Status::Completed);
    assert_eq!(c.close, CloseState::Allowed);
    assert!(c.terminal.as_ref().unwrap().outcome.is_ok());
}

#[test]
fn abnormal_owner_drop_waits_for_noncooperative_thread_release() {
    let (worker, driver) = gate();
    let task = Task::spawn_with(DEFAULT_CAPACITY, move |op, events| {
        let completion = runtime::execute(op, Action::Gzip, events);
        worker.hold("noncooperative-after-success");
        completion
    })
    .unwrap();
    driver.at("noncooperative-after-success");
    let cancellation = task.cancellation.clone();
    let (done, observed) = mpsc::sync_channel(1);
    let owner = thread::spawn(move || {
        drop(task);
        done.try_send(()).unwrap();
    });
    let deadline = Instant::now() + WAIT;
    while !cancellation.is_requested() {
        assert!(Instant::now() < deadline, "Drop 취소 진입 기한 초과");
        thread::yield_now();
    }
    assert_eq!(observed.try_recv(), Err(mpsc::TryRecvError::Empty));
    assert!(!owner.is_finished());
    driver.release();
    observed.recv_timeout(WAIT).expect("Drop join 반환 필요");
    join_bounded(owner).unwrap();
}
