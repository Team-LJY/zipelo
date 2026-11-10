use crate::support::*;
use app_core::{Delivery, DeliveryError, Operation, Receiver};
use std::convert::Infallible;

#[test]
fn gzip_complete_stream_is_accepted_and_payload_applied_once() {
    let (events, terminal) = gzip_trace(true);
    let mut receiver = Receiver::new(id_of(&terminal));
    for event in &events {
        assert_eq!(receiver.progress(event), Ok(Delivery::Accepted));
    }
    assert_eq!(
        accepted_payload(&mut receiver, terminal).unwrap(),
        b"123456789"
    );
    assert_eq!(
        receiver.progress(&events[0]),
        Err(DeliveryError::AlreadyTerminal)
    );
}

#[test]
fn ustar_complete_stream_and_terminal_are_accepted() {
    let (events, terminal) = ustar_trace(true);
    let mut receiver = Receiver::new(id_of(&terminal));
    for event in &events {
        assert_eq!(receiver.progress(event), Ok(Delivery::Accepted));
    }
    assert_eq!(receiver.terminal(&terminal), Ok(Delivery::Accepted));
    assert_eq!(
        terminal.into_result().unwrap().into_inner().files[0].bytes,
        b"123456789"
    );
}

#[test]
fn zip_complete_stream_and_terminal_are_accepted() {
    let (events, terminal) = zip_trace();
    let mut receiver = Receiver::new(id_of(&terminal));
    for event in &events {
        assert_eq!(receiver.progress(event), Ok(Delivery::Accepted));
    }
    assert_eq!(receiver.terminal(&terminal), Ok(Delivery::Accepted));
}

#[test]
fn zero_event_failure_terminal_is_valid_delivery() {
    let (operation, _) = Operation::start().unwrap();
    let mut receiver = Receiver::new(operation.id());
    let mut request = zip_request();
    request.timeout = std::time::Duration::ZERO;
    let terminal = operation.create_zip(request, |_| panic!("observer가 호출되면 안 됨"));
    assert_eq!(receiver.terminal(&terminal), Ok(Delivery::Accepted));
    assert!(terminal.result().is_err());
}

#[test]
fn failed_native_operation_has_valid_sequence_stream() {
    let (operation, _) = Operation::start().unwrap();
    let mut receiver = Receiver::new(operation.id());
    let mut budget = app_core::gzip::Budget::new(Default::default()).unwrap();
    let mut events = Vec::new();
    let terminal = operation.gzip(
        app_core::gzip::Request {
            input: Box::from([0]),
            selected: true,
        },
        &mut budget,
        |event| {
            events.push(event);
            Ok::<_, app_core::gzip::Stop<Infallible>>(())
        },
    );
    for event in &events {
        assert_eq!(receiver.progress(event), Ok(Delivery::Accepted));
    }
    assert_eq!(receiver.terminal(&terminal), Ok(Delivery::Accepted));
    assert_gzip_failure(&terminal, app_core::gzip::Failure::Header);
}

#[test]
fn stale_high_sequence_and_terminal_leave_current_sequence_untouched() {
    let (old_events, old_terminal) = gzip_trace(true);
    let (current_events, current_terminal) = gzip_trace(true);
    let mut receiver = Receiver::new(id_of(&current_terminal));
    assert_eq!(
        receiver.progress(old_events.last().unwrap()),
        Ok(Delivery::IgnoredStale)
    );
    assert_eq!(receiver.terminal(&old_terminal), Ok(Delivery::IgnoredStale));
    for event in &current_events {
        assert_eq!(receiver.progress(event), Ok(Delivery::Accepted));
    }
    assert_eq!(receiver.terminal(&current_terminal), Ok(Delivery::Accepted));
}

#[test]
fn stale_stream_from_another_format_is_ignored() {
    let (old_events, old_terminal) = zip_trace();
    let (current_events, terminal) = ustar_trace(false);
    let mut receiver = Receiver::new(id_of(&terminal));
    assert_eq!(
        receiver.progress(&old_events[0]),
        Ok(Delivery::IgnoredStale)
    );
    assert_eq!(receiver.terminal(&old_terminal), Ok(Delivery::IgnoredStale));
    assert_eq!(
        receiver.progress(&current_events[0]),
        Ok(Delivery::Accepted)
    );
}

#[test]
fn duplicate_progress_latches_fault_and_blocks_payload() {
    let (events, terminal) = gzip_trace(true);
    let mut receiver = Receiver::new(id_of(&terminal));
    assert_eq!(receiver.progress(&events[0]), Ok(Delivery::Accepted));
    assert_eq!(
        receiver.progress(&events[0]),
        Err(DeliveryError::Sequence {
            expected: 1,
            received: 0
        })
    );
    assert_eq!(receiver.progress(&events[1]), Err(DeliveryError::Faulted));
    assert_eq!(receiver.terminal(&terminal), Err(DeliveryError::Faulted));
    assert!(accepted_payload(&mut receiver, terminal).is_none());
}

#[test]
fn missing_first_progress_latches_fault_even_if_replayed() {
    let (events, terminal) = gzip_trace(true);
    let mut receiver = Receiver::new(id_of(&terminal));
    assert_eq!(
        receiver.progress(&events[1]),
        Err(DeliveryError::Sequence {
            expected: 0,
            received: 1
        })
    );
    assert_eq!(receiver.progress(&events[0]), Err(DeliveryError::Faulted));
    assert!(accepted_payload(&mut receiver, terminal).is_none());
}

#[test]
fn reversed_progress_after_prefix_cannot_repair_fault() {
    let (events, terminal) = gzip_trace(true);
    let mut receiver = Receiver::new(id_of(&terminal));
    assert_eq!(receiver.progress(&events[0]), Ok(Delivery::Accepted));
    assert_eq!(
        receiver.progress(&events[2]),
        Err(DeliveryError::Sequence {
            expected: 1,
            received: 2
        })
    );
    assert_eq!(receiver.progress(&events[1]), Err(DeliveryError::Faulted));
    assert!(accepted_payload(&mut receiver, terminal).is_none());
}

#[test]
fn premature_terminal_rejects_success_without_applying_payload() {
    let (events, terminal) = gzip_trace(true);
    let mut receiver = Receiver::new(id_of(&terminal));
    assert_eq!(receiver.progress(&events[0]), Ok(Delivery::Accepted));
    assert_eq!(
        receiver.terminal(&terminal),
        Err(DeliveryError::Sequence {
            expected: 1,
            received: events.len() as u64
        })
    );
    assert!(accepted_payload(&mut receiver, terminal).is_none());
}

#[test]
fn duplicate_terminal_and_progress_after_terminal_are_rejected() {
    let (events, terminal) = gzip_trace(true);
    let mut receiver = Receiver::new(id_of(&terminal));
    for event in &events {
        receiver.progress(event).unwrap();
    }
    assert_eq!(receiver.terminal(&terminal), Ok(Delivery::Accepted));
    assert_eq!(
        receiver.terminal(&terminal),
        Err(DeliveryError::AlreadyTerminal)
    );
    assert_eq!(
        receiver.progress(events.last().unwrap()),
        Err(DeliveryError::AlreadyTerminal)
    );
    assert!(accepted_payload(&mut receiver, terminal).is_none());
}

#[test]
fn stale_inputs_are_ignored_before_sticky_fault_check() {
    let (stale_events, stale_terminal) = gzip_trace(false);
    let (events, terminal) = gzip_trace(true);
    let mut receiver = Receiver::new(id_of(&terminal));
    assert!(matches!(
        receiver.progress(&events[1]),
        Err(DeliveryError::Sequence { .. })
    ));
    assert_eq!(
        receiver.progress(&stale_events[0]),
        Ok(Delivery::IgnoredStale)
    );
    assert_eq!(
        receiver.terminal(&stale_terminal),
        Ok(Delivery::IgnoredStale)
    );
    assert_eq!(receiver.progress(&events[0]), Err(DeliveryError::Faulted));
    assert!(accepted_payload(&mut receiver, terminal).is_none());
}

#[test]
fn stale_inputs_are_ignored_before_already_terminal_check() {
    let (stale_events, stale_terminal) = gzip_trace(false);
    let (events, terminal) = gzip_trace(true);
    let mut receiver = Receiver::new(id_of(&terminal));
    for event in &events {
        receiver.progress(event).unwrap();
    }
    receiver.terminal(&terminal).unwrap();
    assert_eq!(
        receiver.progress(&stale_events[0]),
        Ok(Delivery::IgnoredStale)
    );
    assert_eq!(
        receiver.terminal(&stale_terminal),
        Ok(Delivery::IgnoredStale)
    );
    assert_eq!(
        receiver.terminal(&terminal),
        Err(DeliveryError::AlreadyTerminal)
    );
}

#[test]
fn stale_success_payload_is_never_applied_to_new_receiver() {
    let (_, stale) = gzip_trace(true);
    let (events, current) = gzip_trace(true);
    let mut receiver = Receiver::new(id_of(&current));
    assert!(accepted_payload(&mut receiver, stale).is_none());
    for event in &events {
        receiver.progress(event).unwrap();
    }
    assert_eq!(
        accepted_payload(&mut receiver, current).unwrap(),
        b"123456789"
    );
}
