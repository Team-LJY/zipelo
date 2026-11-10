use crate::support::*;
use std::{cell::Cell, convert::Infallible, fmt};
use zipelo_memory_ustar_reader::{Checkpoint, Error, Failure, Stop, read};

#[derive(Debug)]
struct Marker {
    payload: Box<u64>,
}

impl fmt::Display for Marker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "선택 콜백 오류 {}", self.payload)
    }
}

impl std::error::Error for Marker {}

fn trace() -> Vec<Checkpoint> {
    let mut points = Vec::new();
    read(
        request(normal()),
        |_| Ok::<_, Infallible>(ids(&["2", "3"])),
        |point| {
            points.push(point);
            Ok(())
        },
    )
    .expect("기준 순회는 성공해야 함");
    points
}

#[test]
fn read_checkpoints_cover_both_passes_with_bounded_steps() {
    let points = trace();
    assert_eq!(points.first(), Some(&Checkpoint::Start));
    assert_eq!(points.last(), Some(&Checkpoint::Finish));
    for pass in [1, 2] {
        let offsets = points
            .iter()
            .filter_map(|point| match point {
                Checkpoint::Read {
                    pass: current,
                    offset,
                } if *current == pass => Some(*offset),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(offsets.first(), Some(&0));
        assert_eq!(offsets.last(), Some(&normal().len()));
        assert!(
            offsets
                .windows(2)
                .all(|pair| pair[1] >= pair[0] && pair[1] - pair[0] <= 65_536)
        );
        assert!(offsets.windows(2).any(|pair| pair[1] - pair[0] == 65_536));
    }
    assert_eq!(
        points
            .iter()
            .filter(|p| **p == Checkpoint::BeforeSelection)
            .count(),
        1
    );
    assert_eq!(
        points
            .iter()
            .filter(|p| **p == Checkpoint::AfterSelection)
            .count(),
        1
    );
}

fn stop_at_each_checkpoint(stop: Stop, expected: Failure) {
    let reference = trace();
    for stop_index in 0..reference.len() {
        let mut observed = Vec::new();
        let result = read(
            request(normal()),
            |_| Ok::<_, Infallible>(ids(&["2", "3"])),
            |point| {
                observed.push(point);
                if observed.len() - 1 == stop_index {
                    Err(stop)
                } else {
                    Ok(())
                }
            },
        );
        failure(result, expected);
        assert_eq!(
            observed,
            reference[..=stop_index],
            "중단 뒤에 추가 체크가 실행됨"
        );
    }
}

#[test]
fn cancellation_at_every_checkpoint_returns_no_completed() {
    stop_at_each_checkpoint(Stop::Cancelled, Failure::Cancelled);
}

// Deadline 신호 분류 시험이며 실제 시계 만료 시험이 아님.
#[test]
fn injected_deadline_at_every_checkpoint_is_budget() {
    stop_at_each_checkpoint(Stop::Deadline, Failure::Budget);
}

#[test]
fn cancellation_before_selection_prevents_callback() {
    let called = Cell::new(false);
    let result = read(
        request(normal()),
        |_| {
            called.set(true);
            Ok::<_, Infallible>(ids(&["1"]))
        },
        |point| {
            if point == Checkpoint::BeforeSelection {
                Err(Stop::Cancelled)
            } else {
                Ok(())
            }
        },
    );
    failure(result, Failure::Cancelled);
    assert!(!called.get());
}

#[test]
fn selection_can_request_cancellation_before_second_pass() {
    let cancelled = Cell::new(false);
    let mut second = false;
    let result = read(
        request(normal()),
        |_| {
            cancelled.set(true);
            Ok::<_, Infallible>(ids(&["1"]))
        },
        |point| {
            if matches!(point, Checkpoint::Read { pass: 2, .. }) {
                second = true;
            }
            if cancelled.get() {
                Err(Stop::Cancelled)
            } else {
                Ok(())
            }
        },
    );
    failure(result, Failure::Cancelled);
    assert!(!second);
}

#[test]
fn empty_selection_after_callback_cancellation_still_fails() {
    let result = read(
        request(normal()),
        |_| Ok::<_, Infallible>(vec![]),
        |point| {
            if point == Checkpoint::AfterSelection {
                Err(Stop::Cancelled)
            } else {
                Ok(())
            }
        },
    );
    failure(result, Failure::Cancelled);
}

#[test]
fn final_checkpoint_failure_discards_accumulated_results() {
    let mut saw_end = false;
    let size = normal().len();
    let result = read(
        request(normal()),
        |_| Ok::<_, Infallible>(ids(&["1", "3"])),
        |point| {
            if point
                == (Checkpoint::Read {
                    pass: 2,
                    offset: size,
                })
            {
                saw_end = true;
            }
            if point == Checkpoint::Finish {
                Err(Stop::Cancelled)
            } else {
                Ok(())
            }
        },
    );
    failure(result, Failure::Cancelled);
    assert!(saw_end);
}

#[test]
fn callback_error_preserves_owned_identity_and_source() {
    let marker = Marker {
        payload: Box::new(42),
    };
    let pointer = marker.payload.as_ref() as *const u64;
    let mut second = false;
    let error = read(
        request(normal()),
        |_| Err(marker),
        |point| {
            if matches!(point, Checkpoint::Read { pass: 2, .. }) {
                second = true;
            }
            Ok(())
        },
    )
    .expect_err("선택 오류가 성공으로 바뀌면 안 됨");
    let source = std::error::Error::source(&error).expect("원래 오류가 source로 남아야 함");
    assert_eq!(
        source
            .downcast_ref::<Marker>()
            .expect("오류 형식이 보존되어야 함")
            .payload
            .as_ref() as *const u64,
        pointer
    );
    match error {
        Error::Selection(marker) => assert_eq!(marker.payload.as_ref() as *const u64, pointer),
        other => panic!("원래 선택 오류가 사라짐: {other:?}"),
    }
    assert!(!second);
}

#[test]
fn callback_error_stops_before_after_checkpoint() {
    let marker = Marker {
        payload: Box::new(43),
    };
    let pointer = marker.payload.as_ref() as *const u64;
    let selection_failed = Cell::new(false);
    let mut after_error = Vec::new();
    let error = read(
        request(normal()),
        |_| {
            selection_failed.set(true);
            Err(marker)
        },
        |point| {
            if selection_failed.get() {
                after_error.push(point);
                Err(Stop::Deadline)
            } else {
                Ok(())
            }
        },
    )
    .expect_err("선택 오류를 반환해야 함");
    match error {
        Error::Selection(marker) => assert_eq!(marker.payload.as_ref() as *const u64, pointer),
        other => panic!("원래 오류가 교체됨: {other:?}"),
    }
    assert!(
        after_error.is_empty(),
        "선택 오류 뒤에 checkpoint가 호출됨: {after_error:?}"
    );
}
