use super::support::*;
use std::time::Duration;
use zipelo_rust_memory_zip_create::{
    CHUNK, Checkpoint, Failure, MAX_DURATION, Method, Stop, create,
};

#[test]
fn input_checkpoints_partition_bytes_into_at_most_64k() {
    for method in [Method::Stored, Method::Deflate] {
        let mut parts = Vec::new();
        let input = request(
            method,
            vec![entry("a", vec![0; CHUNK * 2 + 1]), entry("b", vec![])],
        );
        let result = create(input, |point| {
            if let Checkpoint::Input { index, offset, len } = point {
                parts.push((index, offset, len));
            }
            Ok(())
        });
        assert!(result.is_ok());
        assert_eq!(parts, [(0, 0, CHUNK), (0, CHUNK, CHUNK), (0, CHUNK * 2, 1)]);
    }
}

fn stop_each_observed_checkpoint(method: Method, stop: Stop) {
    let make = || {
        request(
            method,
            vec![entry("a", vec![0; CHUNK + 1]), entry("b", vec![])],
        )
    };
    let mut baseline = Vec::new();
    let bytes = create(make(), |point| {
        baseline.push(point);
        Ok(())
    })
    .unwrap();
    assert_zip32(&bytes, 2);
    assert_eq!(baseline.first(), Some(&Checkpoint::Start));
    assert_eq!(baseline.last(), Some(&Checkpoint::Return));
    assert!(baseline.contains(&Checkpoint::Finish));
    assert!(baseline.iter().any(|point| matches!(point, Checkpoint::Write { offset, extent, len } if *len > 0 && offset < extent)));
    for target in 0..baseline.len() {
        let mut observed = Vec::new();
        let result = create(make(), |point| {
            observed.push(point);
            if observed.len() - 1 == target {
                Err(stop)
            } else {
                Ok(())
            }
        });
        assert_eq!(
            result,
            Err(failure_for(stop)),
            "checkpoint {target}: {:?}",
            baseline[target]
        );
        assert_eq!(observed, baseline[..=target], "실패 뒤 callback이 재개됨");
    }
}

#[test]
fn stored_cancel_at_every_observed_checkpoint_returns_no_artifact() {
    stop_each_observed_checkpoint(Method::Stored, Stop::Cancelled);
}

#[test]
fn deflate_cancel_at_every_observed_checkpoint_returns_no_artifact() {
    stop_each_observed_checkpoint(Method::Deflate, Stop::Cancelled);
}

#[test]
fn stored_injected_deadline_at_every_observed_checkpoint_is_sticky() {
    stop_each_observed_checkpoint(Method::Stored, Stop::Deadline);
}

#[test]
fn deflate_injected_deadline_at_every_observed_checkpoint_is_sticky() {
    stop_each_observed_checkpoint(Method::Deflate, Stop::Deadline);
}

#[test]
fn cancellation_at_return_discards_finished_zip() {
    let mut finished = false;
    let result = create(small(Method::Stored), |point| {
        if point == Checkpoint::Finish {
            finished = true;
        }
        if point == Checkpoint::Return {
            assert!(finished);
            Err(Stop::Cancelled)
        } else {
            Ok(())
        }
    });
    assert_eq!(result, Err(Failure::Cancelled));
}

#[test]
fn budget_failure_blocks_cleanup_callbacks_and_preserves_cause() {
    let mut input = small(Method::Stored);
    input.limits.output = 1;
    let mut overflow_observed = false;
    let mut after_failure = 0;
    let result = create(input, |point| {
        if overflow_observed {
            after_failure += 1;
            return Err(Stop::Cancelled);
        }
        if let Checkpoint::Write { offset, len, .. } = point {
            overflow_observed = offset + len as u64 > 1;
        }
        Ok(())
    });
    assert_eq!(result, Err(Failure::Budget));
    assert!(overflow_observed);
    assert_eq!(after_failure, 0);
}

#[test]
fn invalid_timeout_rejected_before_callbacks() {
    for timeout in [Duration::ZERO, MAX_DURATION + Duration::from_nanos(1)] {
        let mut input = small(Method::Stored);
        input.timeout = timeout;
        let mut calls = 0;
        assert_eq!(
            create(input, |_| {
                calls += 1;
                Ok(())
            }),
            Err(Failure::InvalidRequest)
        );
        assert_eq!(calls, 0);
    }
}

#[test]
fn cancellation_at_start_precedes_unsupported_option() {
    let mut input = small(Method::Stored);
    input.options.encrypted = true;
    assert_eq!(
        create(input, |_| Err(Stop::Cancelled)),
        Err(Failure::Cancelled)
    );
}
