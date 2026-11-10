use crate::support::*;
use std::sync::Arc;
use std::time::Duration;
use zipelo_memory_gzip_reader::{Budget, Error, Failure, Limits, Stage, Stop, Usage};

#[derive(Clone, Copy)]
enum Action {
    Cancel,
    Deadline,
    Callback,
}

fn stop_at(stage: Stage, action: Action, selected: bool) {
    let marker = Arc::new(String::from("원형 오류"));
    let original = marker.clone();
    let mut fired = false;
    let mut budget = budget();
    let result =
        zipelo_memory_gzip_reader::read(request(&normal(), selected), &mut budget, |point| {
            assert!(!fired, "오류 이후 checkpoint가 호출되면 안 됨");
            if point.stage == stage {
                fired = true;
                return Err(match action {
                    Action::Cancel => Stop::Cancelled,
                    Action::Deadline => Stop::Deadline,
                    Action::Callback => Stop::Callback(marker.clone()),
                });
            }
            Ok(())
        });
    assert!(fired, "{stage:?}에 도달해야 함");
    match action {
        Action::Cancel => reader_error(result, Failure::Cancelled),
        Action::Deadline => reader_error(result, Failure::Budget),
        Action::Callback => match result {
            Err(Error::Callback(error)) => assert!(Arc::ptr_eq(&original, &error)),
            other => panic!("콜백 오류 원형이어야 함: {other:?}"),
        },
    }
    let expected = match stage {
        Stage::Start | Stage::Header => Usage::default(),
        Stage::BeforeInflate => Usage {
            processed: 10,
            output: 0,
            metadata: 10,
        },
        Stage::AfterInflate => Usage {
            processed: 38,
            output: 0,
            metadata: 10,
        },
        Stage::AfterOutput | Stage::Trailer | Stage::BeforeReturn => Usage {
            processed: 38,
            output: if selected { 9 } else { 0 },
            metadata: 10,
        },
    };
    assert_eq!(budget.usage(), expected);
}

#[test]
fn start_cancel_selected() {
    stop_at(Stage::Start, Action::Cancel, true);
}

#[test]
fn start_deadline_selected() {
    stop_at(Stage::Start, Action::Deadline, true);
}

#[test]
fn start_callback_selected() {
    stop_at(Stage::Start, Action::Callback, true);
}

#[test]
fn header_cancel_selected() {
    stop_at(Stage::Header, Action::Cancel, true);
}

#[test]
fn header_deadline_selected() {
    stop_at(Stage::Header, Action::Deadline, true);
}

#[test]
fn header_callback_selected() {
    stop_at(Stage::Header, Action::Callback, true);
}

#[test]
fn before_inflate_cancel_selected() {
    stop_at(Stage::BeforeInflate, Action::Cancel, true);
}

#[test]
fn before_inflate_deadline_selected() {
    stop_at(Stage::BeforeInflate, Action::Deadline, true);
}

#[test]
fn before_inflate_callback_selected() {
    stop_at(Stage::BeforeInflate, Action::Callback, true);
}

#[test]
fn after_inflate_cancel_selected() {
    stop_at(Stage::AfterInflate, Action::Cancel, true);
}

#[test]
fn after_inflate_deadline_selected() {
    stop_at(Stage::AfterInflate, Action::Deadline, true);
}

#[test]
fn after_inflate_callback_selected() {
    stop_at(Stage::AfterInflate, Action::Callback, true);
}

#[test]
fn after_output_cancel_selected() {
    stop_at(Stage::AfterOutput, Action::Cancel, true);
}

#[test]
fn after_output_deadline_selected() {
    stop_at(Stage::AfterOutput, Action::Deadline, true);
}

#[test]
fn after_output_callback_selected() {
    stop_at(Stage::AfterOutput, Action::Callback, true);
}

#[test]
fn trailer_cancel_selected() {
    stop_at(Stage::Trailer, Action::Cancel, true);
}

#[test]
fn trailer_deadline_selected() {
    stop_at(Stage::Trailer, Action::Deadline, true);
}

#[test]
fn trailer_callback_selected() {
    stop_at(Stage::Trailer, Action::Callback, true);
}

#[test]
fn before_return_cancel_selected() {
    stop_at(Stage::BeforeReturn, Action::Cancel, true);
}

#[test]
fn before_return_deadline_selected() {
    stop_at(Stage::BeforeReturn, Action::Deadline, true);
}

#[test]
fn before_return_callback_selected() {
    stop_at(Stage::BeforeReturn, Action::Callback, true);
}

#[test]
fn after_inflate_cancel_precedes_trailer_failure_selected() {
    let mut input = normal();
    input[21] ^= 1;
    let mut reached = false;
    let mut budget = budget();
    let result = zipelo_memory_gzip_reader::read(request(&input, true), &mut budget, |point| {
        assert!(!reached);
        if point.stage == Stage::AfterInflate {
            reached = true;
            Err(Stop::<()>::Cancelled)
        } else {
            Ok(())
        }
    });
    assert!(reached);
    reader_error(result, Failure::Cancelled);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 38,
            output: 0,
            metadata: 10
        }
    );
}
#[test]
fn successful_checkpoint_trace_selected() {
    let mut stages = Vec::new();
    let mut previous = Usage::default();
    let mut budget = budget();
    let result = zipelo_memory_gzip_reader::read(request(&normal(), true), &mut budget, |point| {
        assert!(point.usage.processed >= previous.processed);
        assert!(point.usage.output >= previous.output);
        assert!(point.usage.metadata >= previous.metadata);
        previous = point.usage;
        stages.push(point.stage);
        Ok::<_, Stop<()>>(())
    })
    .unwrap();
    assert_eq!(stages.first(), Some(&Stage::Start));
    assert_eq!(stages.last(), Some(&Stage::BeforeReturn));
    let before = stages
        .iter()
        .position(|s| *s == Stage::BeforeInflate)
        .unwrap();
    let after = stages
        .iter()
        .position(|s| *s == Stage::AfterInflate)
        .unwrap();
    let trailer = stages.iter().position(|s| *s == Stage::Trailer).unwrap();
    assert!(before < after && after < trailer);
    assert_eq!(
        stages.iter().filter(|s| **s == Stage::AfterOutput).count(),
        1
    );
    assert_eq!(previous, result.usage);
}

#[test]
fn start_cancel_unselected() {
    stop_at(Stage::Start, Action::Cancel, false);
}

#[test]
fn start_deadline_unselected() {
    stop_at(Stage::Start, Action::Deadline, false);
}

#[test]
fn start_callback_unselected() {
    stop_at(Stage::Start, Action::Callback, false);
}

#[test]
fn header_cancel_unselected() {
    stop_at(Stage::Header, Action::Cancel, false);
}

#[test]
fn header_deadline_unselected() {
    stop_at(Stage::Header, Action::Deadline, false);
}

#[test]
fn header_callback_unselected() {
    stop_at(Stage::Header, Action::Callback, false);
}

#[test]
fn before_inflate_cancel_unselected() {
    stop_at(Stage::BeforeInflate, Action::Cancel, false);
}

#[test]
fn before_inflate_deadline_unselected() {
    stop_at(Stage::BeforeInflate, Action::Deadline, false);
}

#[test]
fn before_inflate_callback_unselected() {
    stop_at(Stage::BeforeInflate, Action::Callback, false);
}

#[test]
fn after_inflate_cancel_unselected() {
    stop_at(Stage::AfterInflate, Action::Cancel, false);
}

#[test]
fn after_inflate_deadline_unselected() {
    stop_at(Stage::AfterInflate, Action::Deadline, false);
}

#[test]
fn after_inflate_callback_unselected() {
    stop_at(Stage::AfterInflate, Action::Callback, false);
}

#[test]
fn trailer_cancel_unselected() {
    stop_at(Stage::Trailer, Action::Cancel, false);
}

#[test]
fn trailer_deadline_unselected() {
    stop_at(Stage::Trailer, Action::Deadline, false);
}

#[test]
fn trailer_callback_unselected() {
    stop_at(Stage::Trailer, Action::Callback, false);
}

#[test]
fn before_return_cancel_unselected() {
    stop_at(Stage::BeforeReturn, Action::Cancel, false);
}

#[test]
fn before_return_deadline_unselected() {
    stop_at(Stage::BeforeReturn, Action::Deadline, false);
}

#[test]
fn before_return_callback_unselected() {
    stop_at(Stage::BeforeReturn, Action::Callback, false);
}

#[test]
fn after_inflate_cancel_precedes_trailer_failure_unselected() {
    let mut input = normal();
    input[21] ^= 1;
    let mut reached = false;
    let mut budget = budget();
    let result = zipelo_memory_gzip_reader::read(request(&input, false), &mut budget, |point| {
        assert!(!reached);
        if point.stage == Stage::AfterInflate {
            reached = true;
            Err(Stop::<()>::Cancelled)
        } else {
            Ok(())
        }
    });
    assert!(reached);
    reader_error(result, Failure::Cancelled);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 38,
            output: 0,
            metadata: 10
        }
    );
}
#[test]
fn successful_checkpoint_trace_unselected() {
    let mut stages = Vec::new();
    let mut previous = Usage::default();
    let mut budget = budget();
    let result = zipelo_memory_gzip_reader::read(request(&normal(), false), &mut budget, |point| {
        assert!(point.usage.processed >= previous.processed);
        assert!(point.usage.output >= previous.output);
        assert!(point.usage.metadata >= previous.metadata);
        previous = point.usage;
        stages.push(point.stage);
        Ok::<_, Stop<()>>(())
    })
    .unwrap();
    assert_eq!(stages.first(), Some(&Stage::Start));
    assert_eq!(stages.last(), Some(&Stage::BeforeReturn));
    let before = stages
        .iter()
        .position(|s| *s == Stage::BeforeInflate)
        .unwrap();
    let after = stages
        .iter()
        .position(|s| *s == Stage::AfterInflate)
        .unwrap();
    let trailer = stages.iter().position(|s| *s == Stage::Trailer).unwrap();
    assert!(before < after && after < trailer);
    assert_eq!(
        stages.iter().filter(|s| **s == Stage::AfterOutput).count(),
        0
    );
    assert_eq!(previous, result.usage);
}

#[test]
fn output_chunks_are_bounded_and_complete() {
    let mut delivered = 0;
    let mut chunks = Vec::new();
    let mut budget = budget();
    let done = zipelo_memory_gzip_reader::read(
        request(&fixture("large_output"), true),
        &mut budget,
        |point| {
            if point.stage == Stage::AfterOutput {
                let delta = point.usage.output - delivered;
                assert!((1..=65536).contains(&delta));
                chunks.push(delta);
                delivered = point.usage.output;
            }
            Ok::<_, Stop<()>>(())
        },
    )
    .unwrap();
    assert_eq!(chunks, vec![65536, 65536, 65536, 3395]);
    assert_eq!(done.bytes.unwrap(), vec![b'A'; 200003]);
}

#[test]
fn unselected_never_exposes_after_output() {
    let mut budget = budget();
    let mut inflated = 0;
    let done = zipelo_memory_gzip_reader::read(
        request(&fixture("large_output"), false),
        &mut budget,
        |point| {
            assert_ne!(point.stage, Stage::AfterOutput);
            if point.stage == Stage::AfterInflate {
                inflated += 1;
            }
            assert_eq!(point.usage.output, 0);
            Ok::<_, Stop<()>>(())
        },
    )
    .unwrap();
    assert!(inflated >= 4);
    assert_eq!(done.bytes, None);
}

#[test]
fn empty_selected_never_calls_after_output() {
    let mut budget = budget();
    let result = zipelo_memory_gzip_reader::read(
        request(&fixture("empty_fixed"), true),
        &mut budget,
        |point| {
            assert_ne!(point.stage, Stage::AfterOutput);
            Ok::<_, Stop<()>>(())
        },
    )
    .unwrap();
    assert_eq!(result.bytes, Some(Vec::new()));
}

#[test]
fn output_failure_stops_later_callbacks() {
    let mut budget = Budget::new(Limits {
        output: 0,
        ..Limits::default()
    })
    .unwrap();
    let mut after_inflate = false;
    let result = zipelo_memory_gzip_reader::read(request(&normal(), true), &mut budget, |point| {
        assert!(!after_inflate);
        after_inflate = point.stage == Stage::AfterInflate;
        Ok::<_, Stop<()>>(())
    });
    assert!(after_inflate);
    reader_error(result, Failure::Budget);
}

#[test]
fn cancellation_precedes_input_cap() {
    let mut budget = Budget::new(Limits {
        input: 0,
        ..Limits::default()
    })
    .unwrap();
    let result = zipelo_memory_gzip_reader::read(request(&normal(), true), &mut budget, |_| {
        Err(Stop::<()>::Cancelled)
    });
    reader_error(result, Failure::Cancelled);
    assert_eq!(budget.usage(), Usage::default());
}

#[test]
fn cancellation_precedes_existing_processed_overrun() {
    let input = bad_after_output();
    let mut budget = Budget::new(Limits {
        processed: input.len() as u64,
        ..Limits::default()
    })
    .unwrap();
    reader_error(read(&input, true, &mut budget), Failure::Budget);
    let before = budget.usage();
    let result = zipelo_memory_gzip_reader::read(request(&normal(), true), &mut budget, |_| {
        Err(Stop::<()>::Cancelled)
    });
    reader_error(result, Failure::Cancelled);
    assert_eq!(budget.usage(), before);
}

fn expired_callback(action: Option<Action>) {
    let marker = Arc::new(7);
    let mut budget = Budget::new(Limits {
        duration: Duration::from_millis(20),
        ..Limits::default()
    })
    .unwrap();
    let mut entered = false;
    let result = zipelo_memory_gzip_reader::read(request(&normal(), true), &mut budget, |point| {
        assert!(!entered);
        assert_eq!(point.stage, Stage::Start);
        entered = true;
        std::thread::sleep(Duration::from_millis(30));
        match action {
            Some(Action::Cancel) => Err(Stop::Cancelled),
            Some(Action::Callback) => Err(Stop::Callback(marker.clone())),
            Some(Action::Deadline) => Err(Stop::Deadline),
            None => Ok(()),
        }
    });
    assert!(entered);
    assert!(budget.elapsed() >= Duration::from_millis(20));
    match action {
        Some(Action::Cancel) => reader_error(result, Failure::Cancelled),
        Some(Action::Callback) => match result {
            Err(Error::Callback(error)) => assert!(Arc::ptr_eq(&marker, &error)),
            other => panic!("콜백 원형이 유지되어야 함: {other:?}"),
        },
        _ => reader_error(result, Failure::Budget),
    }
    assert_eq!(budget.usage(), Usage::default());
    reader_error(read(&normal(), true, &mut budget), Failure::Budget);
    assert_eq!(budget.usage(), Usage::default());
}

#[test]
fn elapsed_deadline_after_callback_and_retry() {
    expired_callback(None);
}

#[test]
fn cancelled_callback_precedes_elapsed_deadline() {
    expired_callback(Some(Action::Cancel));
}

#[test]
fn callback_identity_precedes_elapsed_deadline() {
    expired_callback(Some(Action::Callback));
}

#[test]
fn completed_usage_snapshot_is_not_mutated_by_retry() {
    let mut budget = budget();
    let first = read(&normal(), true, &mut budget).unwrap();
    read(&normal(), false, &mut budget).unwrap();
    assert_eq!(
        first.usage,
        Usage {
            processed: 38,
            output: 9,
            metadata: 10
        }
    );
    assert_eq!(first.bytes, Some(b"123456789".to_vec()));
    assert_eq!(budget.usage().processed, 76);
}
