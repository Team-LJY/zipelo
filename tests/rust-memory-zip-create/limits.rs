use super::support::*;
use zipelo_rust_memory_zip_create::{
    Checkpoint, Failure, Limits, MAX_BYTES, MAX_INPUTS, Method, create,
};

#[test]
fn caller_input_exact_and_one_over() {
    for method in [Method::Stored, Method::Deflate] {
        let mut input = small(method);
        input.limits.input = 3;
        assert_zip32(&run(input), 1);
        let mut input = small(method);
        input.limits.input = 2;
        fail_before_output(input, Failure::Budget);
    }
}

#[test]
fn aggregate_input_not_just_each_entry_is_bounded() {
    let make = || {
        request(
            Method::Stored,
            vec![entry("a", vec![1, 2]), entry("b", vec![3, 4])],
        )
    };
    let mut input = make();
    input.limits.input = 4;
    assert_zip32(&run(input), 2);
    let mut input = make();
    input.limits.input = 3;
    fail_before_output(input, Failure::Budget);
}

#[test]
fn hard_input_limit_and_larger_caller_cannot_bypass() {
    let mut input = request(
        Method::Deflate,
        vec![entry("a", vec![0; MAX_BYTES as usize])],
    );
    input.limits.input = u64::MAX;
    assert_zip32(&run(input), 1);
    let mut input = request(
        Method::Deflate,
        vec![entry("a", vec![0; MAX_BYTES as usize + 1])],
    );
    input.limits.input = u64::MAX;
    fail_before_output(input, Failure::Budget);
}

#[test]
fn zero_input_budget_allows_empty_file_only() {
    let mut input = request(Method::Stored, vec![entry("a", vec![])]);
    input.limits.input = 0;
    assert_zip32(&run(input), 1);
    let mut input = small(Method::Stored);
    input.limits.input = 0;
    fail_before_output(input, Failure::Budget);
}

#[test]
fn artifact_counts_as_one_additional_entry() {
    let mut input = small(Method::Stored);
    input.limits.entries = 2;
    assert_zip32(&run(input), 1);
    let mut input = small(Method::Stored);
    input.limits.entries = 1;
    fail_before_output(input, Failure::Budget);
    let mut input = small(Method::Stored);
    input.limits.entries = 0;
    fail_before_output(input, Failure::Budget);
}

#[test]
fn hard_input_count_1024_and_one_over() {
    let make = |count| {
        request(
            Method::Stored,
            (0..count)
                .map(|i| entry(format!("file-{i}"), vec![]))
                .collect(),
        )
    };
    let mut input = make(MAX_INPUTS);
    input.limits.entries = usize::MAX;
    assert_zip32(&run(input), MAX_INPUTS);
    let mut input = make(MAX_INPUTS + 1);
    input.limits.entries = usize::MAX;
    fail_before_output(input, Failure::Budget);
}

#[test]
fn output_exact_and_one_byte_over_caller_budget() {
    for method in [Method::Stored, Method::Deflate] {
        let expected = run(small(method));
        let mut input = small(method);
        input.limits.output = expected.len() as u64;
        assert_eq!(run(input), expected);
        let mut input = small(method);
        input.limits.output = expected.len() as u64 - 1;
        assert_eq!(create(input, |_| Ok(())), Err(Failure::Budget));
    }
}

#[test]
fn stored_output_hard_limit_exact_and_one_over() {
    assert_eq!(
        run(request(Method::Stored, vec![entry("a", vec![])])).len(),
        100
    );
    for excess in [0, 1] {
        let mut input = request(
            Method::Stored,
            vec![entry("a", vec![0; MAX_BYTES as usize - 100 + excess])],
        );
        input.limits.output = u64::MAX;
        let result = create(input, |_| Ok(()));
        if excess == 0 {
            assert_eq!(result.unwrap().len() as u64, MAX_BYTES);
        } else {
            assert_eq!(result, Err(Failure::Budget));
        }
    }
}

#[test]
fn processed_is_input_plus_zip_without_rewrite_double_charge() {
    for method in [Method::Stored, Method::Deflate] {
        let expected = run(small(method));
        let total = 3 + expected.len() as u64;
        let mut input = small(method);
        input.limits.processed = total;
        let mut patch_seen = false;
        let result = create(input, |point| {
            if let Checkpoint::Write {
                offset,
                extent,
                len,
            } = point
            {
                patch_seen |= len > 0 && offset < extent;
            }
            Ok(())
        });
        assert_eq!(result.unwrap(), expected);
        assert!(patch_seen);
        let mut input = small(method);
        input.limits.processed = total - 1;
        assert_eq!(create(input, |_| Ok(())), Err(Failure::Budget));
    }
}

#[test]
fn processed_budget_covers_all_input_before_writer_creation() {
    let mut input = small(Method::Stored);
    input.limits.processed = 2;
    fail_before_output(input, Failure::Budget);
    let mut input = small(Method::Stored);
    input.limits.processed = 3;
    assert_eq!(create(input, |_| Ok(())), Err(Failure::Budget));
}

#[test]
fn central_directory_overflow_is_checked_before_write_and_return() {
    for method in [Method::Stored, Method::Deflate] {
        let bytes = run(small(method));
        let central = assert_zip32(&bytes, 1) as u64;
        let mut input = small(method);
        input.limits.output = central + 1;
        let mut after_finish = false;
        let mut rejected_write = false;
        let mut returned = false;
        let result = create(input, |point| {
            match point {
                Checkpoint::Finish => after_finish = true,
                Checkpoint::Write { offset, len, .. } if after_finish && offset >= central => {
                    rejected_write |= offset + len as u64 > central + 1;
                }
                Checkpoint::Return => returned = true,
                _ => (),
            }
            Ok(())
        });
        assert_eq!(result, Err(Failure::Budget));
        assert!(after_finish && rejected_write && !returned);
    }
}

#[test]
fn zero_output_or_processed_budget_cannot_publish_empty_file() {
    for output_zero in [true, false] {
        let mut input = request(Method::Stored, vec![entry("a", vec![])]);
        if output_zero {
            input.limits.output = 0;
        } else {
            input.limits.processed = 0;
        }
        assert_eq!(create(input, |_| Ok(())), Err(Failure::Budget));
    }
}

#[test]
fn maximal_caller_values_do_not_overflow_small_creation() {
    let mut input = small(Method::Stored);
    input.limits = Limits {
        input: u64::MAX,
        output: u64::MAX,
        entries: usize::MAX,
        processed: u64::MAX,
    };
    assert_zip32(&run(input), 1);
}
