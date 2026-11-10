use crate::support::*;
use zipelo_memory_ustar_reader::{Failure, Limits, MAX_BYTES, MAX_ENTRIES};

#[test]
fn input_exact_limit() {
    let bytes = normal();
    let limits = Limits {
        input: bytes.len() as u64,
        ..Limits::default()
    };
    let result = limited(bytes, &[], limits).expect("정확한 한도는 허용되어야 함");
    assert_eq!(result.usage.entries, 4);
}

#[test]
fn input_one_below_required() {
    let bytes = normal();
    let limits = Limits {
        input: (bytes.len() as u64) - 1,
        ..Limits::default()
    };
    failure(limited(bytes, &[], limits), Failure::Budget);
}

#[test]
fn entries_exact_limit() {
    let bytes = normal();
    let limits = Limits {
        entries: 4,
        ..Limits::default()
    };
    let result = limited(bytes, &[], limits).expect("정확한 한도는 허용되어야 함");
    assert_eq!(result.usage.entries, 4);
}

#[test]
fn entries_one_below_required() {
    let bytes = normal();
    let limits = Limits {
        entries: 3,
        ..Limits::default()
    };
    failure(limited(bytes, &[], limits), Failure::Budget);
}

#[test]
fn metadata_exact_limit() {
    let bytes = normal();
    let limits = Limits {
        metadata: 2048,
        ..Limits::default()
    };
    let result = limited(bytes, &[], limits).expect("정확한 한도는 허용되어야 함");
    assert_eq!(result.usage.entries, 4);
}

#[test]
fn metadata_one_below_required() {
    let bytes = normal();
    let limits = Limits {
        metadata: 2047,
        ..Limits::default()
    };
    failure(limited(bytes, &[], limits), Failure::Budget);
}

#[test]
fn processed_exact_limit() {
    let bytes = normal();
    let limits = Limits {
        processed: bytes.len() as u64,
        ..Limits::default()
    };
    let result = limited(bytes, &[], limits).expect("정확한 한도는 허용되어야 함");
    assert_eq!(result.usage.entries, 4);
}

#[test]
fn processed_one_below_required() {
    let bytes = normal();
    let limits = Limits {
        processed: (bytes.len() as u64) - 1,
        ..Limits::default()
    };
    failure(limited(bytes, &[], limits), Failure::Budget);
}

#[test]
fn output_exact_limit() {
    let bytes = normal();
    let limits = Limits {
        output: 140005,
        ..Limits::default()
    };
    let result = limited(bytes, &["1", "2", "3"], limits).expect("정확한 한도는 허용되어야 함");
    assert_eq!(result.usage.entries, 4);
}

#[test]
fn output_one_below_required() {
    let bytes = normal();
    let limits = Limits {
        output: 140004,
        ..Limits::default()
    };
    failure(limited(bytes, &["1", "2", "3"], limits), Failure::Budget);
}

#[test]
fn processed_second_pass_exact() {
    let bytes = normal();
    let expected = bytes.len() as u64 * 2;
    let result = limited(
        bytes,
        &["1"],
        Limits {
            processed: expected,
            ..Limits::default()
        },
    )
    .expect("두 순회 정확한 한도는 허용되어야 함");
    assert_eq!(result.usage.processed, expected);
    assert_eq!(result.usage.entries, 4);
}

#[test]
fn processed_second_pass_one_below() {
    let bytes = normal();
    let limits = Limits {
        processed: bytes.len() as u64 * 2 - 1,
        ..Limits::default()
    };
    failure(limited(bytes, &["1"], limits), Failure::Budget);
}

#[test]
fn metadata_per_pass_not_cumulative() {
    let bytes = normal();
    let result = limited(
        bytes,
        &["0"],
        Limits {
            metadata: 2048,
            ..Limits::default()
        },
    )
    .expect("메타데이터는 순회별 한도임");
    assert_eq!(result.entries.len(), 4);
    assert!(result.files.is_empty());
}

#[test]
fn output_failure_after_prior_file_returns_no_completed() {
    let bytes = archive(&[Fixture::file("a", &[1]), Fixture::file("b", &[2, 3])]);
    failure(
        limited(
            bytes,
            &["0", "1"],
            Limits {
                output: 2,
                ..Limits::default()
            },
        ),
        Failure::Budget,
    );
}

#[test]
fn unselected_large_file_does_not_charge_output() {
    let bytes = normal();
    let result = limited(
        bytes,
        &["2"],
        Limits {
            output: 1,
            ..Limits::default()
        },
    )
    .expect("선택한 빈 파일의 출력은 0임");
    assert_eq!(result.usage.output, 0);
    assert_eq!(result.files.len(), 1);
}

#[test]
fn input_hard_cap_exact() {
    let result = limited(
        vec![0; MAX_BYTES as usize],
        &[],
        Limits {
            input: u64::MAX,
            ..Limits::default()
        },
    )
    .expect("입력 상한의 정렬된 0 블록은 허용되어야 함");
    assert_eq!(result.usage.processed, MAX_BYTES);
}

#[test]
fn input_hard_cap_over() {
    failure(
        limited(
            vec![0; MAX_BYTES as usize + 512],
            &[],
            Limits {
                input: u64::MAX,
                ..Limits::default()
            },
        ),
        Failure::Budget,
    );
}

#[test]
fn entry_hard_cap_over() {
    let names = (0..=MAX_ENTRIES)
        .map(|i| format!("f{i}"))
        .collect::<Vec<_>>();
    let fixtures = names
        .iter()
        .map(|name| Fixture::file(name, &[]))
        .collect::<Vec<_>>();
    failure(
        limited(
            archive(&fixtures),
            &[],
            Limits {
                entries: usize::MAX,
                ..Limits::default()
            },
        ),
        Failure::Budget,
    );
}

#[test]
fn zero_input_invalid() {
    let limits = Limits {
        input: 0,
        ..Limits::default()
    };
    failure(limited(normal(), &[], limits), Failure::InvalidRequest);
}

#[test]
fn zero_entries_invalid() {
    let limits = Limits {
        entries: 0,
        ..Limits::default()
    };
    failure(limited(normal(), &[], limits), Failure::InvalidRequest);
}

#[test]
fn zero_processed_invalid() {
    let limits = Limits {
        processed: 0,
        ..Limits::default()
    };
    failure(limited(normal(), &[], limits), Failure::InvalidRequest);
}

#[test]
fn zero_output_invalid() {
    let limits = Limits {
        output: 0,
        ..Limits::default()
    };
    failure(limited(normal(), &[], limits), Failure::InvalidRequest);
}

#[test]
fn zero_metadata_invalid() {
    let limits = Limits {
        metadata: 0,
        ..Limits::default()
    };
    failure(limited(normal(), &[], limits), Failure::InvalidRequest);
}

#[test]
fn zero_duration_invalid() {
    let limits = Limits {
        duration: std::time::Duration::ZERO,
        ..Limits::default()
    };
    failure(limited(normal(), &[], limits), Failure::InvalidRequest);
}

#[test]
fn metadata_below_first_header() {
    failure(
        limited(
            normal(),
            &[],
            Limits {
                metadata: 511,
                ..Limits::default()
            },
        ),
        Failure::Budget,
    );
}

#[test]
fn output_reservation_before_body_read() {
    use std::convert::Infallible;
    use zipelo_memory_ustar_reader::{Checkpoint, read};
    let mut input = request(archive(&[Fixture::file("a", &[1, 2])]));
    input.limits.output = 1;
    let mut body_read = false;
    let result = read(
        input,
        |_| Ok::<_, Infallible>(ids(&["0"])),
        |point| {
            if point
                == (Checkpoint::Read {
                    pass: 2,
                    offset: 512,
                })
            {
                body_read = true;
            }
            Ok(())
        },
    );
    failure(result, Failure::Budget);
    assert!(!body_read);
}

#[test]
fn output_limit_below_first_selected_file() {
    failure(
        limited(
            normal(),
            &["1", "2", "3"],
            Limits {
                output: 4,
                ..Limits::default()
            },
        ),
        Failure::Budget,
    );
}
