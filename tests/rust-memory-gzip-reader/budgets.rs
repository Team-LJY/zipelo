use crate::support::*;
use zipelo_memory_gzip_reader::{Budget, Error, Failure, InflateError, Limits, Stage, Stop, Usage};

#[derive(Clone, Copy)]
enum Field {
    Input,
    Processed,
    Output,
    Metadata,
}

fn limit(field: Field, value: u64) -> Limits {
    let mut limits = Limits::default();
    match field {
        Field::Input => limits.input = value,
        Field::Processed => limits.processed = value,
        Field::Output => limits.output = value,
        Field::Metadata => limits.metadata = value,
    }
    limits
}

fn boundary(field: Field, value: u64, selected: bool, succeeds: bool, expected: Usage) {
    let mut budget = Budget::new(limit(field, value)).unwrap();
    let result = read(&normal(), selected, &mut budget);
    if succeeds {
        assert_eq!(
            result.unwrap().bytes,
            selected.then(|| b"123456789".to_vec())
        );
    } else {
        reader_error(result, Failure::Budget);
    }
    assert_eq!(budget.usage(), expected);
}

fn failed_decode(selected: bool, overrun: bool) {
    let input = bad_after_output();
    let mut budget = Budget::new(limit(
        Field::Processed,
        input.len() as u64 + u64::from(!overrun),
    ))
    .unwrap();
    let mut reached_after = false;
    let result = zipelo_memory_gzip_reader::read(request(&input, selected), &mut budget, |point| {
        if matches!(
            point.stage,
            Stage::AfterInflate | Stage::AfterOutput | Stage::Trailer | Stage::BeforeReturn
        ) {
            reached_after = true;
            return Err(Stop::<()>::Cancelled);
        }
        Ok(())
    });
    if overrun {
        reader_error(result, Failure::Budget);
    } else {
        assert!(
            matches!(result, Err(Error::Backend(InflateError::DataError))),
            "{result:?}"
        );
    }
    assert!(!reached_after);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: input.len() as u64 + 1,
            output: 0,
            metadata: 10
        }
    );
    let previous = budget.usage();
    let mut stages = Vec::new();
    let retry =
        zipelo_memory_gzip_reader::read(request(&normal(), selected), &mut budget, |point| {
            stages.push(point.stage);
            Ok::<_, Stop<()>>(())
        });
    reader_error(retry, Failure::Budget);
    if overrun {
        assert_eq!(stages, vec![Stage::Start]);
        assert_eq!(budget.usage(), previous);
    } else {
        assert!(!stages.contains(&Stage::BeforeInflate));
        assert_eq!(budget.usage().processed, previous.processed);
        assert_eq!(budget.usage().metadata, previous.metadata + 1);
    }
}

#[test]
fn input_29_selected() {
    boundary(
        Field::Input,
        29,
        true,
        true,
        Usage {
            processed: 38,
            output: 9,
            metadata: 10,
        },
    );
}

#[test]
fn input_28_selected() {
    boundary(
        Field::Input,
        28,
        true,
        false,
        Usage {
            processed: 0,
            output: 0,
            metadata: 0,
        },
    );
}

#[test]
fn processed_38_selected() {
    boundary(
        Field::Processed,
        38,
        true,
        true,
        Usage {
            processed: 38,
            output: 9,
            metadata: 10,
        },
    );
}

#[test]
fn processed_37_selected() {
    boundary(
        Field::Processed,
        37,
        true,
        false,
        Usage {
            processed: 38,
            output: 0,
            metadata: 10,
        },
    );
}

#[test]
fn output_9_selected() {
    boundary(
        Field::Output,
        9,
        true,
        true,
        Usage {
            processed: 38,
            output: 9,
            metadata: 10,
        },
    );
}

#[test]
fn output_8_selected() {
    boundary(
        Field::Output,
        8,
        true,
        false,
        Usage {
            processed: 38,
            output: 0,
            metadata: 10,
        },
    );
}

#[test]
fn metadata_10_selected() {
    boundary(
        Field::Metadata,
        10,
        true,
        true,
        Usage {
            processed: 38,
            output: 9,
            metadata: 10,
        },
    );
}

#[test]
fn metadata_9_selected() {
    boundary(
        Field::Metadata,
        9,
        true,
        false,
        Usage {
            processed: 9,
            output: 0,
            metadata: 9,
        },
    );
}

#[test]
fn input_0_selected() {
    boundary(
        Field::Input,
        0,
        true,
        false,
        Usage {
            processed: 0,
            output: 0,
            metadata: 0,
        },
    );
}

#[test]
fn processed_0_selected() {
    boundary(
        Field::Processed,
        0,
        true,
        false,
        Usage {
            processed: 0,
            output: 0,
            metadata: 1,
        },
    );
}

#[test]
fn output_0_selected() {
    boundary(
        Field::Output,
        0,
        true,
        false,
        Usage {
            processed: 38,
            output: 0,
            metadata: 10,
        },
    );
}

#[test]
fn metadata_0_selected() {
    boundary(
        Field::Metadata,
        0,
        true,
        false,
        Usage {
            processed: 0,
            output: 0,
            metadata: 0,
        },
    );
}

#[test]
fn backend_error_output_charged_selected() {
    failed_decode(true, false);
}
#[test]
fn output_overrun_precedes_backend_selected() {
    failed_decode(true, true);
}
#[test]
fn exact_all_limits_selected() {
    let mut budget = Budget::new(Limits {
        input: 29,
        processed: 38,
        output: 9,
        metadata: 10,
        ..Limits::default()
    })
    .unwrap();
    assert!(read(&normal(), true, &mut budget).is_ok());
}
#[test]
fn crc_failure_retains_cost_and_retry_selected() {
    let mut input = normal();
    input[21] ^= 1;
    let mut budget = Budget::new(Limits {
        processed: 76,
        output: 18,
        metadata: 20,
        ..Limits::default()
    })
    .unwrap();
    reader_error(read(&input, true, &mut budget), Failure::Crc);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 38,
            output: 9,
            metadata: 10
        }
    );
    let completed = read(&normal(), true, &mut budget).unwrap();
    assert_eq!(
        completed.usage,
        Usage {
            processed: 76,
            output: 18,
            metadata: 20
        }
    );
}
#[test]
fn cumulative_processed_selected() {
    let mut budget = Budget::new(limit(Field::Processed, 75)).unwrap();
    read(&normal(), true, &mut budget).unwrap();
    reader_error(read(&normal(), true, &mut budget), Failure::Budget);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 76,
            output: 9,
            metadata: 20
        }
    );
}
#[test]
fn cumulative_metadata_selected() {
    let mut budget = Budget::new(limit(Field::Metadata, 19)).unwrap();
    read(&normal(), true, &mut budget).unwrap();
    reader_error(read(&normal(), true, &mut budget), Failure::Budget);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 47,
            output: 9,
            metadata: 19
        }
    );
}
#[test]
fn input_cap_is_per_request_selected() {
    let mut budget = Budget::new(limit(Field::Input, 29)).unwrap();
    read(&normal(), true, &mut budget).unwrap();
    read(&normal(), true, &mut budget).unwrap();
    assert_eq!(budget.usage().processed, 76);
}
#[test]
fn selected_empty_zero_output_selected() {
    let mut budget = Budget::new(limit(Field::Output, 0)).unwrap();
    let result = read(&fixture("empty_fixed"), true, &mut budget).unwrap();
    assert_eq!(result.bytes, true.then(Vec::new));
    assert_eq!(result.usage.output, 0);
}

#[test]
fn input_29_unselected() {
    boundary(
        Field::Input,
        29,
        false,
        true,
        Usage {
            processed: 38,
            output: 0,
            metadata: 10,
        },
    );
}

#[test]
fn input_28_unselected() {
    boundary(
        Field::Input,
        28,
        false,
        false,
        Usage {
            processed: 0,
            output: 0,
            metadata: 0,
        },
    );
}

#[test]
fn processed_38_unselected() {
    boundary(
        Field::Processed,
        38,
        false,
        true,
        Usage {
            processed: 38,
            output: 0,
            metadata: 10,
        },
    );
}

#[test]
fn processed_37_unselected() {
    boundary(
        Field::Processed,
        37,
        false,
        false,
        Usage {
            processed: 38,
            output: 0,
            metadata: 10,
        },
    );
}

#[test]
fn output_9_unselected() {
    boundary(
        Field::Output,
        9,
        false,
        true,
        Usage {
            processed: 38,
            output: 0,
            metadata: 10,
        },
    );
}

#[test]
fn output_8_unselected() {
    boundary(
        Field::Output,
        8,
        false,
        true,
        Usage {
            processed: 38,
            output: 0,
            metadata: 10,
        },
    );
}

#[test]
fn metadata_10_unselected() {
    boundary(
        Field::Metadata,
        10,
        false,
        true,
        Usage {
            processed: 38,
            output: 0,
            metadata: 10,
        },
    );
}

#[test]
fn metadata_9_unselected() {
    boundary(
        Field::Metadata,
        9,
        false,
        false,
        Usage {
            processed: 9,
            output: 0,
            metadata: 9,
        },
    );
}

#[test]
fn input_0_unselected() {
    boundary(
        Field::Input,
        0,
        false,
        false,
        Usage {
            processed: 0,
            output: 0,
            metadata: 0,
        },
    );
}

#[test]
fn processed_0_unselected() {
    boundary(
        Field::Processed,
        0,
        false,
        false,
        Usage {
            processed: 0,
            output: 0,
            metadata: 1,
        },
    );
}

#[test]
fn output_0_unselected() {
    boundary(
        Field::Output,
        0,
        false,
        true,
        Usage {
            processed: 38,
            output: 0,
            metadata: 10,
        },
    );
}

#[test]
fn metadata_0_unselected() {
    boundary(
        Field::Metadata,
        0,
        false,
        false,
        Usage {
            processed: 0,
            output: 0,
            metadata: 0,
        },
    );
}

#[test]
fn backend_error_output_charged_unselected() {
    failed_decode(false, false);
}
#[test]
fn output_overrun_precedes_backend_unselected() {
    failed_decode(false, true);
}
#[test]
fn exact_all_limits_unselected() {
    let mut budget = Budget::new(Limits {
        input: 29,
        processed: 38,
        output: 0,
        metadata: 10,
        ..Limits::default()
    })
    .unwrap();
    assert!(read(&normal(), false, &mut budget).is_ok());
}
#[test]
fn crc_failure_retains_cost_and_retry_unselected() {
    let mut input = normal();
    input[21] ^= 1;
    let mut budget = Budget::new(Limits {
        processed: 76,
        output: 18,
        metadata: 20,
        ..Limits::default()
    })
    .unwrap();
    reader_error(read(&input, false, &mut budget), Failure::Crc);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 38,
            output: 0,
            metadata: 10
        }
    );
    let completed = read(&normal(), false, &mut budget).unwrap();
    assert_eq!(
        completed.usage,
        Usage {
            processed: 76,
            output: 0,
            metadata: 20
        }
    );
}
#[test]
fn cumulative_processed_unselected() {
    let mut budget = Budget::new(limit(Field::Processed, 75)).unwrap();
    read(&normal(), false, &mut budget).unwrap();
    reader_error(read(&normal(), false, &mut budget), Failure::Budget);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 76,
            output: 0,
            metadata: 20
        }
    );
}
#[test]
fn cumulative_metadata_unselected() {
    let mut budget = Budget::new(limit(Field::Metadata, 19)).unwrap();
    read(&normal(), false, &mut budget).unwrap();
    reader_error(read(&normal(), false, &mut budget), Failure::Budget);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 47,
            output: 0,
            metadata: 19
        }
    );
}
#[test]
fn input_cap_is_per_request_unselected() {
    let mut budget = Budget::new(limit(Field::Input, 29)).unwrap();
    read(&normal(), false, &mut budget).unwrap();
    read(&normal(), false, &mut budget).unwrap();
    assert_eq!(budget.usage().processed, 76);
}
#[test]
fn selected_empty_zero_output_unselected() {
    let mut budget = Budget::new(limit(Field::Output, 0)).unwrap();
    let result = read(&fixture("empty_fixed"), false, &mut budget).unwrap();
    assert_eq!(result.bytes, false.then(Vec::new));
    assert_eq!(result.usage.output, 0);
}

#[test]
fn cumulative_selected_output() {
    let mut budget = Budget::new(limit(Field::Output, 17)).unwrap();
    read(&normal(), true, &mut budget).unwrap();
    reader_error(read(&normal(), true, &mut budget), Failure::Budget);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 76,
            output: 9,
            metadata: 20
        }
    );
}

#[test]
fn shared_budget_selected_then_unselected() {
    let mut budget = Budget::new(limit(Field::Output, 9)).unwrap();
    read(&normal(), true, &mut budget).unwrap();
    let done = read(&normal(), false, &mut budget).unwrap();
    assert_eq!(done.bytes, None);
    assert_eq!(
        done.usage,
        Usage {
            processed: 76,
            output: 9,
            metadata: 20
        }
    );
}

#[test]
fn shared_budget_unselected_then_selected() {
    let mut budget = Budget::new(limit(Field::Output, 9)).unwrap();
    read(&normal(), false, &mut budget).unwrap();
    let done = read(&normal(), true, &mut budget).unwrap();
    assert_eq!(done.bytes, Some(b"123456789".to_vec()));
    assert_eq!(
        done.usage,
        Usage {
            processed: 76,
            output: 9,
            metadata: 20
        }
    );
}

#[test]
fn limits_are_clamped_and_do_not_overflow() {
    let budget = Budget::new(Limits {
        input: u64::MAX,
        processed: u64::MAX,
        output: u64::MAX,
        metadata: u64::MAX,
        duration: std::time::Duration::MAX,
    })
    .unwrap();
    let limits = budget.limits();
    assert_eq!(limits.input, 16 * 1024 * 1024);
    assert_eq!(limits.processed, 128 * 1024 * 1024);
    assert_eq!(limits.output, 16 * 1024 * 1024);
    assert_eq!(limits.metadata, 1024 * 1024);
    assert_eq!(limits.duration, std::time::Duration::from_secs(300));
    assert_eq!(budget.usage(), Usage::default());
}

#[test]
fn zero_duration_is_invalid() {
    assert!(matches!(
        Budget::new(Limits {
            duration: std::time::Duration::ZERO,
            ..Limits::default()
        }),
        Err(Failure::InvalidRequest)
    ));
}

#[test]
fn zero_byte_limits_are_valid_requests() {
    let budget = Budget::new(Limits {
        input: 0,
        processed: 0,
        output: 0,
        metadata: 0,
        ..Limits::default()
    })
    .unwrap();
    assert_eq!(budget.usage(), Usage::default());
}

#[test]
fn output_failure_is_not_refunded_by_retry() {
    let mut budget = Budget::new(limit(Field::Output, 8)).unwrap();
    for attempt in 1..=2 {
        reader_error(read(&normal(), true, &mut budget), Failure::Budget);
        assert_eq!(
            budget.usage(),
            Usage {
                processed: 38 * attempt,
                output: 0,
                metadata: 10 * attempt
            }
        );
    }
}

#[test]
fn unselected_expansion_is_charged() {
    let input = fixture("large_output");
    let mut budget = Budget::new(limit(Field::Processed, input.len() as u64 + 65535)).unwrap();
    reader_error(read(&input, false, &mut budget), Failure::Budget);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: input.len() as u64 + 65536,
            output: 0,
            metadata: 10
        }
    );
}

#[test]
fn selected_large_output_limit_precedes_later_chunks() {
    let input = fixture("large_output");
    let mut budget = Budget::new(limit(Field::Output, 65535)).unwrap();
    reader_error(read(&input, true, &mut budget), Failure::Budget);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: input.len() as u64 + 65536,
            output: 0,
            metadata: 10
        }
    );
}

#[test]
fn failed_header_retry_keeps_attempt_metadata() {
    let mut budget = Budget::new(limit(Field::Metadata, 1)).unwrap();
    reader_error(read(&[], false, &mut budget), Failure::Truncated);
    reader_error(read(&[], false, &mut budget), Failure::Budget);
    assert_eq!(
        budget.usage(),
        Usage {
            processed: 0,
            output: 0,
            metadata: 1
        }
    );
}
