use std::{
    cell::Cell,
    io::{self, Cursor, Read, Write},
};

use zlib_rs::InflateError;

use crate::{
    error::Failure,
    support::{self, CRC, DATA, Spec},
    typed_deflate::{self as typed, Error, Expected, Limits, Work},
};

fn expected(bytes: &[u8], size: u64, crc: u32) -> Expected {
    Expected {
        packed: bytes.len() as u64,
        size,
        crc,
    }
}

fn run(
    bytes: &[u8],
    entry: Expected,
    limits: Limits,
    chunk: usize,
) -> (Result<(), Error>, Vec<u8>, Work) {
    let mut work = Work::new(limits).unwrap();
    let mut output = Vec::new();
    let result = typed::decode(
        &mut Cursor::new(bytes),
        entry,
        &mut work,
        chunk,
        || Ok(()),
        |part| {
            output.extend_from_slice(part);
            Ok(())
        },
    );
    (result, output, work)
}

struct Split<'a> {
    bytes: &'a [u8],
    at: usize,
    boundary: usize,
    max: usize,
}

impl Read for Split<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let end = if self.at < self.boundary {
            self.boundary
        } else {
            self.bytes.len()
        };
        let count = output.len().min(end - self.at).min(self.max);
        output[..count].copy_from_slice(&self.bytes[self.at..self.at + count]);
        self.at += count;
        Ok(count)
    }
}

fn library_deflate(data: &[u8]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer.start_file("a", options).unwrap();
    writer.write_all(data).unwrap();
    let zip = writer.finish().unwrap();
    let mut archive = zip::ZipArchive::new(zip).unwrap();
    let mut packed = Vec::new();
    archive
        .by_index_raw(0)
        .unwrap()
        .read_to_end(&mut packed)
        .unwrap();
    packed
}

#[test]
fn actual_malformed_retains_backend_data_error() {
    let bytes = [7, 0];
    let (result, output, _) = run(&bytes, expected(&bytes, 9, CRC), Limits::default(), 1);
    let error = result.unwrap_err();
    assert!(matches!(error, Error::Backend(InflateError::DataError)));
    assert_eq!(error.failure(), Failure::Integrity);
    assert!(output.is_empty());
}

#[test]
fn every_two_way_split_and_small_output_for_stored_and_compressed_blocks() {
    let repetitive = b"abcabcabcabc".repeat(100);
    for (packed, data) in [
        (Spec::deflate("a").packed, DATA.to_vec()),
        (library_deflate(&repetitive), repetitive),
    ] {
        for boundary in 0..=packed.len() {
            for chunk in [1, 2, 3, 7, 64, typed::CHUNK] {
                let mut reader = Split {
                    bytes: &packed,
                    at: 0,
                    boundary,
                    max: usize::MAX,
                };
                let mut work = Work::new(Limits::default()).unwrap();
                let mut out = Vec::new();
                typed::decode(
                    &mut reader,
                    expected(&packed, data.len() as u64, crc32fast::hash(&data)),
                    &mut work,
                    chunk,
                    || Ok(()),
                    |part| {
                        out.extend_from_slice(part);
                        Ok(())
                    },
                )
                .unwrap();
                assert_eq!(out, data, "boundary={boundary} chunk={chunk}");
                assert_eq!(work.counts().0, packed.len() as u64);
                assert_eq!(work.counts().1, data.len() as u64);
            }
        }
    }
}

#[test]
fn one_byte_reads_and_outputs_preserve_stream() {
    let packed = library_deflate(&DATA.repeat(500));
    let data = DATA.repeat(500);
    let mut reader = Split {
        bytes: &packed,
        at: 0,
        boundary: packed.len(),
        max: 1,
    };
    let mut work = Work::new(Limits::default()).unwrap();
    let mut out = Vec::new();
    typed::decode(
        &mut reader,
        expected(&packed, data.len() as u64, crc32fast::hash(&data)),
        &mut work,
        1,
        || Ok(()),
        |part| {
            out.extend_from_slice(part);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(out, data);
}

#[test]
fn valid_empty_stream_and_absent_stream_are_different() {
    let valid = [3, 0];
    let (result, output, _) = run(&valid, expected(&valid, 0, 0), Limits::default(), 1);
    result.unwrap();
    assert!(output.is_empty());
    let (result, _, _) = run(&[], expected(&[], 0, 0), Limits::default(), 1);
    assert!(matches!(result, Err(Error::Incomplete)));
}

#[test]
fn every_incomplete_stored_block_prefix_is_not_completion() {
    let packed = Spec::deflate("a").packed;
    for end in 0..packed.len() {
        let bytes = &packed[..end];
        let (result, _, _) = run(bytes, expected(bytes, 9, CRC), Limits::default(), 2);
        assert!(
            matches!(result, Err(Error::Incomplete)),
            "prefix={end}: {result:?}"
        );
    }
}

#[test]
fn premature_reader_eof_is_incomplete() {
    let packed = Spec::deflate("a").packed;
    let (result, _, _) = run(
        &packed[..4],
        expected(&packed, 9, CRC),
        Limits::default(),
        2,
    );
    assert!(matches!(result, Err(Error::Incomplete)));
}

#[test]
fn stream_end_with_garbage_or_second_stream_is_rejected() {
    for tail in [vec![0], vec![3, 0], vec![7, 0]] {
        let mut packed = Spec::deflate("a").packed;
        packed.extend(tail);
        let (result, _, _) = run(&packed, expected(&packed, 9, CRC), Limits::default(), 1);
        assert!(matches!(result, Err(Error::Trailing)));
    }
}

#[test]
fn crc_mismatch_is_distinct_from_backend_failure() {
    let packed = Spec::deflate("a").packed;
    let (result, _, _) = run(&packed, expected(&packed, 9, CRC ^ 1), Limits::default(), 1);
    assert!(matches!(result, Err(Error::Crc)));
}

#[test]
fn actual_size_overflow_is_not_emitted() {
    let packed = Spec::deflate("a").packed;
    let (result, output, _) = run(&packed, expected(&packed, 8, CRC), Limits::default(), 1);
    assert!(matches!(result, Err(Error::Size)));
    assert!(output.len() <= 8);
}

#[test]
fn declared_size_larger_than_actual_is_rejected() {
    let packed = Spec::deflate("a").packed;
    let (result, _, _) = run(&packed, expected(&packed, 10, CRC), Limits::default(), 2);
    assert!(matches!(result, Err(Error::Size)));
}

#[test]
fn exact_input_output_and_call_budgets_and_one_less() {
    let packed = Spec::deflate("a").packed;
    let entry = expected(&packed, 9, CRC);
    let (result, _, reference) = run(&packed, entry, Limits::default(), 1);
    result.unwrap();
    let (input, output, reads, calls) = reference.counts();
    let limits = Limits {
        input,
        output,
        reads,
        calls,
    };
    run(&packed, entry, limits, 1).0.unwrap();
    for less in [
        Limits {
            input: input - 1,
            ..limits
        },
        Limits {
            output: output - 1,
            ..limits
        },
        Limits {
            reads: reads - 1,
            ..limits
        },
        Limits {
            calls: calls - 1,
            ..limits
        },
    ] {
        assert!(matches!(run(&packed, entry, less, 1).0, Err(Error::Limit)));
    }
}

#[test]
fn operation_budget_is_shared_between_entries() {
    let packed = Spec::deflate("a").packed;
    let mut work = Work::new(Limits {
        output: 9,
        ..Limits::default()
    })
    .unwrap();
    let entry = expected(&packed, 9, CRC);
    typed::decode(
        &mut Cursor::new(&packed),
        entry,
        &mut work,
        1,
        || Ok(()),
        |_| Ok(()),
    )
    .unwrap();
    let mut emitted = false;
    let result = typed::decode(
        &mut Cursor::new(&packed),
        entry,
        &mut work,
        1,
        || Ok(()),
        |_| {
            emitted = true;
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::Limit)));
    assert!(!emitted);
}

#[test]
fn failed_work_cannot_resume_or_emit() {
    let mut work = Work::new(Limits::default()).unwrap();
    let entry = Expected {
        packed: 2,
        size: 0,
        crc: 0,
    };
    assert!(
        typed::decode(
            &mut Cursor::new([7, 0]),
            entry,
            &mut work,
            1,
            || Ok(()),
            |_| Ok(())
        )
        .is_err()
    );
    let result = typed::decode(
        &mut Cursor::new([3, 0]),
        entry,
        &mut work,
        1,
        || panic!("blocked checkpoint"),
        |_| panic!("blocked emit"),
    );
    assert!(matches!(result, Err(Error::Blocked)));
}

#[test]
fn cancellation_before_io_and_after_partial_output() {
    let packed = Spec::deflate("a").packed;
    let entry = expected(&packed, 9, CRC);
    let mut work = Work::new(Limits::default()).unwrap();
    let mut reader = Cursor::new(&packed);
    assert!(matches!(
        typed::decode(
            &mut reader,
            entry,
            &mut work,
            1,
            || Err(Error::Cancelled),
            |_| panic!("cancelled emit")
        ),
        Err(Error::Cancelled)
    ));
    assert_eq!(reader.position(), 0);
    let cancelled = Cell::new(false);
    let mut work = Work::new(Limits::default()).unwrap();
    let mut output = Vec::new();
    let result = typed::decode(
        &mut Cursor::new(&packed),
        entry,
        &mut work,
        1,
        || {
            if cancelled.get() {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        },
        |bytes| {
            output.extend_from_slice(bytes);
            cancelled.set(true);
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(output.len(), 1);
}

#[test]
fn host_budget_failure_is_preserved() {
    let packed = Spec::deflate("a").packed;
    let mut work = Work::new(Limits::default()).unwrap();
    let result = typed::decode(
        &mut Cursor::new(&packed),
        expected(&packed, 9, CRC),
        &mut work,
        1,
        || Err(Error::Host(Failure::Budget)),
        |_| panic!("budget emit"),
    );
    assert_eq!(result.unwrap_err().failure(), Failure::Budget);
}

struct IoFailure(io::ErrorKind);
impl Read for IoFailure {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(self.0.into())
    }
}

#[test]
fn input_io_kinds_are_preserved_and_never_inferred_as_corruption() {
    for kind in [
        io::ErrorKind::InvalidData,
        io::ErrorKind::InvalidInput,
        io::ErrorKind::UnexpectedEof,
        io::ErrorKind::Interrupted,
        io::ErrorKind::Other,
    ] {
        let mut work = Work::new(Limits::default()).unwrap();
        let error = typed::decode(
            &mut IoFailure(kind),
            Expected {
                packed: 2,
                size: 0,
                crc: 0,
            },
            &mut work,
            1,
            || Ok(()),
            |_| panic!("io emit"),
        )
        .unwrap_err();
        assert!(matches!(&error, Error::Input(inner) if inner.kind() == kind));
        assert_eq!(error.failure(), Failure::Engine);
    }
}

#[test]
fn sink_failure_stops_without_retry() {
    let packed = Spec::deflate("a").packed;
    let mut work = Work::new(Limits::default()).unwrap();
    let mut calls = 0;
    let result = typed::decode(
        &mut Cursor::new(&packed),
        expected(&packed, 9, CRC),
        &mut work,
        1,
        || Ok(()),
        |_| {
            calls += 1;
            Err(Error::Host(Failure::Protocol))
        },
    );
    assert_eq!(result.unwrap_err().failure(), Failure::Protocol);
    assert_eq!(calls, 1);
}

// 실제 backend 상태나 메모리 실패 재현이 아닌 오류 매핑 시험임.
#[test]
fn injected_backend_variants_preserve_category_and_dictionary_id() {
    for (error, failure) in [
        (InflateError::StreamError, Failure::DeflateStream),
        (InflateError::MemError, Failure::DeflateMemory),
        (
            InflateError::NeedDict { dict_id: 123 },
            Failure::DeflateDictionary(123),
        ),
    ] {
        assert_eq!(Error::Backend(error).failure(), failure);
        assert_ne!(failure.exit_code(), Failure::Integrity.exit_code());
    }
}

#[test]
fn no_progress_policy_separates_exhausted_input_and_internal_stall() {
    assert!(matches!(
        typed::check_progress(0, 0, false, false),
        Err(Error::NoProgress)
    ));
    assert!(matches!(
        typed::check_progress(0, 0, true, false),
        Err(Error::Incomplete)
    ));
    typed::check_progress(1, 0, false, false).unwrap();
    typed::check_progress(0, 1, true, false).unwrap();
    typed::check_progress(0, 0, true, true).unwrap();
}

#[test]
fn invalid_buffer_and_hard_limits_fail_before_emit() {
    for chunk in [0, typed::CHUNK + 1] {
        assert!(matches!(
            run(
                &[3, 0],
                Expected {
                    packed: 2,
                    size: 0,
                    crc: 0
                },
                Limits::default(),
                chunk
            )
            .0,
            Err(Error::Limit)
        ));
    }
    for limits in [
        Limits {
            input: typed::MAX_BYTES + 1,
            ..Limits::default()
        },
        Limits {
            output: typed::MAX_BYTES + 1,
            ..Limits::default()
        },
        Limits {
            calls: typed::MAX_CALLS + 1,
            ..Limits::default()
        },
        Limits {
            reads: typed::MAX_CALLS + 1,
            ..Limits::default()
        },
    ] {
        assert!(matches!(Work::new(limits), Err(Error::Limit)));
    }
}

#[test]
fn candidate_zip_failures_never_emit_successful_item_or_job_end() {
    let mut malformed = Spec::deflate("a");
    malformed.packed = vec![7, 0];
    let mut trailing = Spec::deflate("a");
    trailing.packed.extend([3, 0]);
    let mut crc = Spec::deflate("a");
    crc.crc ^= 1;
    let mut size = Spec::deflate("a");
    size.size -= 1;
    for spec in [malformed, trailing, crc, size] {
        let (result, output) = support::extract(support::sample(&[spec]).bytes, support::limits());
        assert_eq!(result, Err(Failure::Integrity));
        assert!(
            support::frame_records(&output)
                .iter()
                .all(|frame| frame.kind != 4 && frame.kind != 5)
        );
    }
}

#[test]
fn stored_deflate_and_listing_remain_integrated() {
    let bytes = support::sample(&[Spec::stored("a"), Spec::deflate("b")]).bytes;
    assert_eq!(support::list(bytes.clone()).unwrap().len(), 2);
    let (result, output) = support::extract(bytes, support::limits());
    result.unwrap();
    support::validate_success(&support::frame_records(&output), &[DATA, DATA]).unwrap();
}

#[test]
fn frame_output_budget_checked_before_data() {
    let bytes = support::sample(&[Spec::deflate("a")]).bytes;
    let (result, output) = support::extract(
        bytes,
        crate::budget::Limits {
            output: 8,
            ..support::limits()
        },
    );
    assert_eq!(result, Err(Failure::Budget));
    assert!(
        support::frame_records(&output)
            .iter()
            .all(|frame| frame.kind != 3 && frame.kind != 4 && frame.kind != 5)
    );
}

#[test]
fn zip_evidence_entry_point_keeps_actual_backend_variant() {
    let mut spec = Spec::deflate("a");
    spec.packed = vec![7, 0];
    let bytes = support::sample(&[spec]).bytes;
    let budget = support::budget();
    let source =
        crate::budget::Source::new(Cursor::new(&bytes), bytes.len() as u64, budget.clone())
            .unwrap();
    let mut output = Vec::new();
    let mut frames = crate::wire::Frames::new(&mut output, support::ID, budget.clone()).unwrap();
    frames.begin().unwrap();
    let error =
        crate::zip_adapter::extract_with_evidence(source, &budget, &mut frames).unwrap_err();
    assert!(matches!(error, Error::Backend(InflateError::DataError)));
}

#[test]
fn backend_error_precedes_later_checkpoint_cancellation() {
    let mut work = Work::new(Limits::default()).unwrap();
    let mut checks = 0;
    let error = typed::decode(
        &mut Cursor::new([7, 0]),
        Expected {
            packed: 2,
            size: 9,
            crc: CRC,
        },
        &mut work,
        1,
        || {
            checks += 1;
            if checks > 3 {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        },
        |_| panic!("backend error emit"),
    )
    .unwrap_err();
    assert!(matches!(error, Error::Backend(InflateError::DataError)));
    assert_eq!(checks, 3);
}

#[test]
fn final_checkpoint_can_cancel_after_last_output_before_success() {
    let packed = Spec::deflate("a").packed;
    let cancelled = Cell::new(false);
    let mut work = Work::new(Limits::default()).unwrap();
    let result = typed::decode(
        &mut Cursor::new(&packed),
        expected(&packed, 9, CRC),
        &mut work,
        typed::CHUNK,
        || {
            if cancelled.get() {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        },
        |_| {
            cancelled.set(true);
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::Cancelled)));
}

#[test]
fn read_call_limit_cannot_be_avoided_by_short_reads() {
    let packed = Spec::deflate("a").packed;
    let mut reader = Split {
        bytes: &packed,
        at: 0,
        boundary: packed.len(),
        max: 1,
    };
    let mut work = Work::new(Limits {
        reads: 1,
        ..Limits::default()
    })
    .unwrap();
    let result = typed::decode(
        &mut reader,
        expected(&packed, 9, CRC),
        &mut work,
        1,
        || Ok(()),
        |_| Ok(()),
    );
    assert!(matches!(result, Err(Error::Limit)));
    assert_eq!(reader.at, 1);
    assert_eq!(work.counts().2, 1);
}

#[test]
fn embedded_host_input_budget_is_preserved() {
    struct BudgetFailure;
    impl Read for BudgetFailure {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(Failure::Budget.into())
        }
    }
    let mut work = Work::new(Limits::default()).unwrap();
    let error = typed::decode(
        &mut BudgetFailure,
        Expected {
            packed: 2,
            size: 0,
            crc: 0,
        },
        &mut work,
        1,
        || Ok(()),
        |_| panic!("budget emit"),
    )
    .unwrap_err();
    assert!(matches!(&error, Error::Input(_)));
    assert_eq!(error.failure(), Failure::Budget);
}
