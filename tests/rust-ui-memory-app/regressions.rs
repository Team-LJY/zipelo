use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use zipelo_ui_memory_app::{
    controller::{CloseState, Controller, Status},
    core,
    runtime::{self, Action, DEFAULT_CAPACITY, Task},
    transport::{DeliveryKind, Transport},
};

fn finish(task: &Task) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !task.finished() {
        assert!(Instant::now() < deadline, "작업 종료 기한 초과");
        thread::sleep(Duration::from_millis(1));
    }
}

fn complete(controller: &mut Controller) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while controller.busy() {
        assert!(Instant::now() < deadline);
        controller.poll();
        thread::sleep(Duration::from_millis(1));
    }
}

fn fixture_without_draining(action: Action) {
    let mut task = Task::spawn(action, DEFAULT_CAPACITY).unwrap();
    finish(&task);
    let events: Vec<_> = task.events.as_ref().unwrap().try_iter().collect();
    let terminal = task.completion.as_ref().unwrap().try_recv().unwrap();
    let mut receiver = core::Receiver::new(task.id);
    for event in &events {
        assert_eq!(event.validate(&mut receiver), Ok(core::Delivery::Accepted));
    }
    assert!(terminal.delivery.is_none());
    assert!(terminal.outcome.is_ok());
    assert_eq!(terminal.outcome.next_sequence(), events.len() as u64);
    assert!(events.len() <= DEFAULT_CAPACITY);
    assert_eq!(
        terminal.outcome.validate(&mut receiver),
        Ok(core::Delivery::Accepted)
    );
    assert!(matches!(task.poll_join(), Some(Ok(None))));
}

#[test]
fn zip_default_queue_fits_without_ui_drain() {
    fixture_without_draining(Action::Zip);
}
#[test]
fn ustar_default_queue_fits_without_ui_drain() {
    fixture_without_draining(Action::Ustar);
}
#[test]
fn gzip_default_queue_fits_without_ui_drain() {
    fixture_without_draining(Action::Gzip);
}

#[test]
fn full_queue_is_transport_failure_not_user_cancel() {
    let mut c = Controller::default();
    assert!(c.start_with(Action::Gzip, 0));
    finish(c.task.as_ref().unwrap());
    c.poll();
    assert_eq!(c.status, Status::DeliveryFailure);
    assert_eq!(
        c.terminal.as_ref().unwrap().delivery.unwrap().kind,
        DeliveryKind::Full
    );
    assert!(c.rows().is_empty());
    assert!(c.selected_bytes().is_none());
}

#[test]
fn disconnected_progress_preserves_first_missing_sequence() {
    let (sender, receiver) = mpsc::sync_channel(2);
    drop(receiver);
    let mut transport = Transport::new(sender);
    assert_eq!(
        transport.forward(7, 1, Ok(()), || "delivery"),
        Err("delivery")
    );
    assert_eq!(transport.fault.unwrap().kind, DeliveryKind::Disconnected);
    assert_eq!(transport.fault.unwrap().first_missing_sequence, 7);
    let _ = transport.forward(8, 2, Ok(()), || "delivery");
    assert_eq!(transport.fault.unwrap().first_missing_sequence, 7);
}

#[test]
fn original_observer_error_beats_full_queue() {
    let (sender, _receiver) = mpsc::sync_channel(0);
    let mut transport = Transport::new(sender);
    let cause = Box::new(47);
    let identity = &*cause as *const i32;
    let actual = transport
        .forward(0, 1, Err(cause), || Box::new(99))
        .unwrap_err();
    assert_eq!(&*actual as *const i32, identity);
    assert_eq!(transport.fault.unwrap().kind, DeliveryKind::Full);
}

#[test]
fn original_observer_error_beats_disconnected_queue() {
    let (sender, receiver) = mpsc::sync_channel(1);
    drop(receiver);
    let mut transport = Transport::new(sender);
    assert_eq!(
        transport.forward(0, 1, Err("original"), || "delivery"),
        Err("original")
    );
    assert_eq!(transport.fault.unwrap().kind, DeliveryKind::Disconnected);
}

#[test]
fn original_observer_error_still_delivers_its_raw_event_when_possible() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let mut transport = Transport::new(sender);
    assert_eq!(
        transport.forward(3, 47, Err("original"), || "delivery"),
        Err("original")
    );
    assert_eq!(receiver.try_recv(), Ok(47));
    assert!(transport.fault.is_none());
}

#[test]
fn completion_receiver_loss_recovers_payload_from_join() {
    let (release, gate) = mpsc::sync_channel(1);
    let mut task = Task::spawn_with(DEFAULT_CAPACITY, move |op, sender| {
        gate.recv_timeout(Duration::from_secs(5)).unwrap();
        runtime::execute(op, Action::Gzip, sender)
    })
    .unwrap();
    task.completion.take();
    release.try_send(()).unwrap();
    finish(&task);
    let completion = task.poll_join().unwrap().unwrap().unwrap();
    assert!(completion.outcome.is_ok());
    if let runtime::Outcome::Gzip(t) = completion.outcome {
        assert_eq!(
            t.into_result().unwrap().into_inner().bytes.unwrap(),
            runtime::PAYLOAD
        );
    } else {
        panic!();
    }
    assert!(task.poll_join().is_none());
}

#[test]
fn full_completion_channel_returns_ownership_instead_of_dropping() {
    let (sender, _receiver) = mpsc::sync_channel(0);
    let (op, _) = core::Operation::start().unwrap();
    let (progress, _events) = mpsc::sync_channel(DEFAULT_CAPACITY);
    let terminal = runtime::execute(op, Action::Gzip, progress);
    let recovered = runtime::deliver_completion(&sender, terminal).unwrap();
    assert!(recovered.outcome.is_ok());
}

#[test]
fn progress_gap_keeps_real_terminal_but_cannot_apply_payload() {
    let mut c = Controller::default();
    assert!(c.start_with(Action::Gzip, 0));
    complete(&mut c);
    assert!(c.terminal.is_some());
    assert_eq!(
        c.delivery_error,
        Some(core::DeliveryError::Sequence {
            expected: 0,
            received: 1
        })
    );
    assert_eq!(c.status, Status::DeliveryFailure);
    assert!(c.rows().is_empty());
}

#[test]
fn thread_panic_is_joined_without_synthesized_terminal() {
    let mut c = Controller::default();
    c.task = Some(Task::spawn_with(DEFAULT_CAPACITY, |_, _| panic!("합성 panic")).unwrap());
    complete(&mut c);
    assert_eq!(c.status, Status::Panicked);
    assert!(c.panic.is_some());
    assert!(c.terminal.is_none());
    assert!(c.task.as_ref().unwrap().joined());
}

#[test]
fn cancellation_before_work_returns_real_cancelled_result() {
    let (op, cancel) = core::Operation::start().unwrap();
    cancel.request();
    let (sender, _events) = mpsc::sync_channel(DEFAULT_CAPACITY);
    let terminal = runtime::execute(op, Action::Gzip, sender);
    assert!(terminal.outcome.cancelled());
    assert!(terminal.delivery.is_none());
}

#[test]
fn cancellation_after_terminal_does_not_rewrite_success() {
    let (op, cancel) = core::Operation::start().unwrap();
    let (sender, _events) = mpsc::sync_channel(DEFAULT_CAPACITY);
    let terminal = runtime::execute(op, Action::Gzip, sender);
    cancel.request();
    assert!(terminal.outcome.is_ok());
    assert!(!terminal.outcome.cancelled());
}

#[test]
fn repeated_start_is_rejected_until_join_even_if_thread_finished() {
    let mut c = Controller::default();
    assert!(c.start(Action::Gzip));
    let id = c.task.as_ref().unwrap().id;
    assert!(!c.start(Action::Zip));
    finish(c.task.as_ref().unwrap());
    assert!(!c.start(Action::Zip));
    assert_eq!(c.task.as_ref().unwrap().id, id);
    c.poll();
    assert!(c.start(Action::Zip));
    complete(&mut c);
}

#[test]
fn repeated_cancel_sets_request_once() {
    let mut c = Controller::default();
    assert!(c.start(Action::Gzip));
    assert!(c.cancel());
    assert!(!c.cancel());
    complete(&mut c);
}

#[test]
fn close_confirmation_does_not_cancel_and_can_be_retracted() {
    let mut c = Controller::default();
    assert!(c.start(Action::Gzip));
    c.request_close();
    assert_eq!(c.close, CloseState::Confirming);
    assert!(!c.task.as_ref().unwrap().cancellation.is_requested());
    c.keep_open();
    assert_eq!(c.close, CloseState::Open);
    complete(&mut c);
}

#[test]
fn close_cancel_retraction_keeps_existing_cancel_request() {
    let mut c = Controller::default();
    assert!(c.start(Action::Gzip));
    c.request_close();
    c.confirm_close();
    assert_eq!(c.close, CloseState::Waiting);
    c.keep_open();
    assert_eq!(c.close, CloseState::Open);
    assert!(c.task.as_ref().unwrap().cancellation.is_requested());
    complete(&mut c);
    assert_eq!(c.close, CloseState::Open);
}

#[test]
fn close_waits_for_join_and_preserves_terminal() {
    let mut c = Controller::default();
    assert!(c.start(Action::Gzip));
    c.request_close();
    c.confirm_close();
    assert_eq!(c.close, CloseState::Waiting);
    assert!(!c.start(Action::Zip));
    complete(&mut c);
    assert_eq!(c.close, CloseState::Allowed);
    assert!(c.terminal.is_some());
    assert!(c.task.as_ref().unwrap().joined());
}

#[test]
fn completed_during_confirmation_still_requires_choice() {
    let mut c = Controller::default();
    assert!(c.start(Action::Gzip));
    c.request_close();
    complete(&mut c);
    assert_eq!(c.close, CloseState::Confirming);
    c.confirm_close();
    c.poll();
    assert_eq!(c.close, CloseState::Allowed);
}

#[test]
fn atomic_generation_replacement_rejects_old_selection() {
    let mut c = Controller::default();
    assert!(c.start(Action::Ustar));
    complete(&mut c);
    assert_eq!(c.status, Status::Completed);
    let id = c.task.as_ref().unwrap().id;
    let generation = c.generation;
    assert!(c.select(id, generation, 0));
    assert_eq!(c.selected_bytes(), Some(runtime::PAYLOAD));
    assert!(c.start(Action::Gzip));
    assert!(c.selected.is_none());
    assert!(c.terminal.is_none());
    assert!(!c.select(id, generation, 0));
    complete(&mut c);
}

#[test]
fn ustar_exact_ids_and_content_limitation_are_retained() {
    let mut c = Controller::default();
    assert!(c.start(Action::Ustar));
    complete(&mut c);
    assert_eq!(c.rows(), vec!["자료.txt", "empty.txt"]);
    if let runtime::Outcome::Ustar(t) = &c.terminal.as_ref().unwrap().outcome {
        assert_eq!(
            t.result().unwrap().value().content_verification,
            core::ustar::ContentVerification::Unavailable
        );
        assert_eq!(t.result().unwrap().value().files.len(), 2);
    } else {
        panic!();
    }
}

#[test]
fn error_retry_uses_new_operation_and_clears_previous_payload() {
    let mut c = Controller::default();
    assert!(c.start(Action::InvalidGzip));
    complete(&mut c);
    assert_eq!(c.status, Status::NativeFailure);
    let old = c.task.as_ref().unwrap().id;
    assert!(c.start(Action::Gzip));
    assert_ne!(c.task.as_ref().unwrap().id, old);
    complete(&mut c);
    assert_eq!(c.status, Status::Completed);
}

#[test]
fn headless_values_satisfy_thread_transfer_bounds() {
    fn send<T: Send + 'static>() {}
    send::<Task>();
    send::<runtime::Completion>();
    send::<runtime::ProgressEvent>();
}

#[test]
fn oversized_queue_is_rejected_without_spawning() {
    assert!(Task::spawn(Action::Zip, DEFAULT_CAPACITY + 1).is_err());
}

#[test]
fn gzip_preview_preserves_original_bytes() {
    let mut c = Controller::default();
    assert!(c.start(Action::Gzip));
    complete(&mut c);
    assert!(c.select(c.task.as_ref().unwrap().id, c.generation, 0));
    assert_eq!(c.selected_bytes(), Some(runtime::PAYLOAD));
    assert_eq!(c.preview().as_bytes(), runtime::PAYLOAD);
}

#[test]
fn ui_event_wrapper_excludes_old_operation_before_current_sequence() {
    let (old, _) = core::Operation::start().unwrap();
    let (sender, events) = mpsc::sync_channel(DEFAULT_CAPACITY);
    let old_result = runtime::execute(old, Action::Gzip, sender);
    let (current, _) = core::Operation::start().unwrap();
    let mut receiver = core::Receiver::new(current.id());
    for event in events.try_iter() {
        assert_eq!(
            event.validate(&mut receiver),
            Ok(core::Delivery::IgnoredStale)
        );
    }
    assert_eq!(
        old_result.outcome.validate(&mut receiver),
        Ok(core::Delivery::IgnoredStale)
    );
    let (sender, events) = mpsc::sync_channel(DEFAULT_CAPACITY);
    let result = runtime::execute(current, Action::Gzip, sender);
    for event in events.try_iter() {
        assert_eq!(event.validate(&mut receiver), Ok(core::Delivery::Accepted));
    }
    assert_eq!(
        result.outcome.validate(&mut receiver),
        Ok(core::Delivery::Accepted)
    );
}

#[test]
fn native_gzip_callback_error_survives_simultaneous_queue_full() {
    let (operation, cancel) = core::Operation::start().unwrap();
    let (sender, _events) = mpsc::sync_channel(0);
    let mut transport = Transport::new(sender);
    let mut budget = core::gzip::Budget::new(core::gzip::Limits::default()).unwrap();
    let terminal = operation.gzip(
        core::gzip::Request {
            input: runtime::GZIP.into(),
            selected: true,
        },
        &mut budget,
        |event| {
            cancel.request();
            transport.forward(
                event.stamp().sequence(),
                runtime::ProgressEvent::Gzip(event),
                Err(core::gzip::Stop::Callback(runtime::CallbackFailure(
                    "original",
                ))),
                || core::gzip::Stop::Cancelled,
            )
        },
    );
    assert!(matches!(
        terminal.result(),
        Err(core::Failure {
            native: Some(core::gzip::Error::Callback(runtime::CallbackFailure(
                "original"
            ))),
            ..
        })
    ));
    assert_eq!(transport.fault.unwrap().kind, DeliveryKind::Full);
    assert!(terminal.cancellation_requested());
    assert!(!terminal.cancellation_forwarded());
}

#[test]
fn controller_rejects_real_foreign_terminal_without_applying_payload() {
    let (foreign, _) = core::Operation::start().unwrap();
    let (sender, _events) = mpsc::sync_channel(DEFAULT_CAPACITY);
    let foreign_terminal = runtime::execute(foreign, Action::Gzip, sender);
    assert!(foreign_terminal.outcome.is_ok());
    let mut c = Controller::default();
    assert!(c.start(Action::Gzip));
    finish(c.task.as_ref().unwrap());
    c.terminal = Some(foreign_terminal);
    c.poll();
    assert_eq!(c.terminal_delivery, Some(core::Delivery::IgnoredStale));
    assert_eq!(c.status, Status::DeliveryFailure);
    assert!(c.rows().is_empty());
    assert!(!c.select(c.task.as_ref().unwrap().id, c.generation, 0));
    assert!(c.selected_bytes().is_none());
    assert!(c.terminal.as_ref().unwrap().outcome.is_ok());
    assert!(c.task.as_ref().unwrap().joined());
}

#[test]
fn joined_task_without_received_terminal_is_delivery_failure() {
    let mut c = Controller::default();
    assert!(c.start(Action::Gzip));
    finish(c.task.as_ref().unwrap());
    let withheld = c
        .task
        .as_ref()
        .unwrap()
        .completion
        .as_ref()
        .unwrap()
        .try_recv()
        .unwrap();
    assert!(withheld.outcome.is_ok());
    c.poll();
    assert_eq!(c.status, Status::DeliveryFailure);
    assert!(c.terminal.is_none());
    assert!(c.terminal_delivery.is_none());
    assert!(c.rows().is_empty());
    assert!(c.selected_bytes().is_none());
}

#[test]
fn normal_frame_controls_retain_running_handle_until_explicit_join() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    struct RunningProbe(Arc<AtomicBool>);
    impl Drop for RunningProbe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let worker_probe = dropped.clone();
    let (ready, entered) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let task = Task::spawn_with(DEFAULT_CAPACITY, move |_, _| {
        let _probe = RunningProbe(worker_probe);
        ready.try_send(()).unwrap();
        gate.recv_timeout(Duration::from_secs(5)).unwrap();
        panic!("수명 회귀의 종료 경로");
    })
    .unwrap();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let id = task.id;
    let mut c = Controller::default();
    c.task = Some(task);
    c.poll();
    assert!(!c.start(Action::Zip));
    assert!(c.cancel());
    c.request_close();
    c.confirm_close();
    assert_eq!(c.close, CloseState::Waiting);
    c.poll();
    c.keep_open();
    c.poll();
    assert!(c.busy());
    assert_eq!(c.task.as_ref().unwrap().id, id);
    assert!(!c.task.as_ref().unwrap().joined());
    assert!(!dropped.load(Ordering::SeqCst));
    release.try_send(()).unwrap();
    finish(c.task.as_ref().unwrap());
    c.poll();
    assert!(dropped.load(Ordering::SeqCst));
    assert!(c.task.as_ref().unwrap().joined());
    assert_eq!(c.status, Status::Panicked);
    assert_eq!(c.close, CloseState::Open);
}
