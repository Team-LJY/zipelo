use std::{
    io::{Cursor, Read, Seek, SeekFrom},
    time::Duration,
};

use crate::{
    budget::{Budget, CHUNK, MAX_INPUT, MAX_METADATA, Source},
    error::Failure,
    json_wire,
    support::*,
};

#[test]
fn budget_exact_processed_and_output_then_sticky_failure() {
    let mut l = limits();
    l.processed = 10;
    l.output = 8;
    let b = Budget::new(l).unwrap();
    let shared = b.clone();
    assert_eq!(b.charge(4, 3), Ok(()));
    assert_eq!(shared.charge(6, 5), Ok(()));
    assert_eq!(b.charge(1, 0), Err(Failure::Budget));
    assert_eq!(shared.charge(0, 0), Err(Failure::Budget));
}

#[test]
fn budget_u64_max_charge_does_not_wrap() {
    let mut l = limits();
    l.processed = u64::MAX;
    let b = Budget::new(l).unwrap();
    assert_eq!(b.charge(u64::MAX, 0), Ok(()));
    assert_eq!(b.charge(1, 0), Err(Failure::Budget));
    assert_eq!(budget().charge(0, u64::MAX), Err(Failure::Budget));
}

#[test]
fn budget_output_cap_applies_to_larger_caller_limit() {
    let mut l = limits();
    l.output = u64::MAX;
    let b = Budget::new(l).unwrap();
    assert_eq!(b.charge(0, MAX_INPUT), Ok(()));
    assert_eq!(b.charge(0, 1), Err(Failure::Budget));
}

#[test]
fn budget_entry_cap_is_shared_and_sticky() {
    let b = budget();
    assert_eq!(b.entries(1024), Ok(()));
    assert_eq!(b.entries(1025), Err(Failure::Budget));
    assert_eq!(b.clone().check(), Err(Failure::Budget));
    let mut l = limits();
    l.entries = 1;
    let b = Budget::new(l).unwrap();
    assert_eq!(b.entries(1), Ok(()));
    assert_eq!(b.entries(2), Err(Failure::Budget));
}

#[test]
fn invalid_and_streaming_limits_are_distinct() {
    for field in 0..4 {
        let mut l = limits();
        match field {
            0 => l.entries = 0,
            1 => l.processed = 0,
            2 => l.output = 0,
            _ => l.duration = Duration::ZERO,
        }
        assert!(matches!(l.validate(), Err(Failure::Unsupported)));
    }
    let mut l = limits();
    l.duration += Duration::from_nanos(1);
    assert!(matches!(l.validate(), Err(Failure::Unsupported)));
    for field in 0..3 {
        let mut l = limits();
        match field {
            0 => l.entries += 1,
            1 => l.processed += 1,
            _ => l.output += 1,
        }
        assert!(l.validate().is_ok());
        assert!(matches!(l.streaming(), Err(Failure::Budget)));
    }
}

#[test]
fn source_length_minimum_maximum_and_overflow() {
    for length in [0, 21, MAX_INPUT + 1, u64::MAX] {
        assert!(matches!(
            Source::new(Cursor::new(Vec::<u8>::new()), length, budget()),
            Err(Failure::Unsupported)
        ));
    }
    for length in [22, MAX_INPUT] {
        let source = Source::new(Cursor::new(Vec::<u8>::new()), length, budget()).unwrap();
        assert_eq!(source.length(), length);
    }
}

#[test]
fn source_at_rejects_size_and_offset_before_inner_read() {
    let length = (CHUNK + 22) as u64;
    let (inner, calls) = Observed::new(vec![0; length as usize]);
    let mut source = Source::new(inner, length, budget()).unwrap();
    for (offset, count) in [(0, usize::MAX), (u64::MAX, 1), (length, 1), (0, CHUNK + 22)] {
        assert_eq!(source.at(offset, count), Err(Failure::Integrity));
    }
    assert!(calls.borrow().is_empty());
    assert_eq!(source.at(length, 0).unwrap(), Vec::<u8>::new());
}

#[test]
fn source_tail_read_limit_and_chunk_limit() {
    let (inner, calls) = Observed::new(vec![1; CHUNK + 21]);
    let mut source = Source::new(inner, (CHUNK + 21) as u64, budget()).unwrap();
    assert_eq!(source.at(0, CHUNK + 21).unwrap().len(), CHUNK + 21);
    assert_eq!(&*calls.borrow(), &[CHUNK, 21]);
}

#[test]
fn source_seek_extremes_and_eof_do_not_read() {
    let (inner, calls) = Observed::new(vec![0; 22]);
    let mut source = Source::new(inner, 22, budget()).unwrap();
    for from in [
        SeekFrom::Start(u64::MAX),
        SeekFrom::Current(i64::MIN),
        SeekFrom::End(i64::MAX),
    ] {
        assert_eq!(io_failure(source.seek(from)), Failure::Protocol);
    }
    assert_eq!(source.seek(SeekFrom::End(0)).unwrap(), 22);
    assert_eq!(source.read(&mut [0; 1]).unwrap(), 0);
    assert!(calls.borrow().is_empty());
}

#[test]
fn source_actual_eof_is_truncated_not_format_error() {
    let mut source = Source::new(Cursor::new(vec![0; 21]), 22, budget()).unwrap();
    let mut bytes = [0; 22];
    assert_eq!(
        io_failure(source.read_exact(&mut bytes)),
        Failure::Truncated
    );
}

#[test]
fn source_metadata_limit_rejects_before_next_read() {
    let (inner, calls) = Observed::new(vec![0; MAX_METADATA as usize + 1]);
    let mut source = Source::new(inner, MAX_METADATA + 1, budget()).unwrap();
    let mut chunk = [0; CHUNK];
    for _ in 0..32 {
        source.read_exact(&mut chunk).unwrap();
    }
    assert_eq!(calls.borrow().len(), 32);
    assert_eq!(io_failure(source.read(&mut [0; 1])), Failure::Budget);
    assert_eq!(calls.borrow().len(), 32);
    source.metadata_switch().set(false);
    assert_eq!(io_failure(source.read(&mut [0; 1])), Failure::Budget);
    assert_eq!(calls.borrow().len(), 32);
}

#[test]
fn source_listing_charges_length_once_instead_of_each_read() {
    let mut l = limits();
    l.processed = 22;
    let b = Budget::new(l).unwrap();
    let mut source = Source::new(Cursor::new(vec![0; 22]), 22, b.clone()).unwrap();
    source.list_accounting().unwrap();
    for _ in 0..2 {
        source.at(0, 22).unwrap();
    }
    assert_eq!(b.charge(1, 0), Err(Failure::Budget));
}

#[test]
fn base64_padding_reference_vectors() {
    for (encoded, expected) in [
        ("", &b""[..]),
        ("Zg==", &b"f"[..]),
        ("Zm8=", &b"fo"[..]),
        ("Zm9v", &b"foo"[..]),
    ] {
        assert_eq!(json_wire::decode_input(encoded).unwrap(), expected);
    }
}

#[test]
fn base64_malformed_and_noncanonical_rejected() {
    for encoded in ["====", "A===", "Zm=v", "Zg?=", "Zh==", "Zm9="] {
        assert_eq!(
            json_wire::decode_input(encoded),
            Err(Failure::Protocol),
            "{encoded}"
        );
    }
    for encoded in ["Z", "Zg", "Zg="] {
        assert_eq!(json_wire::decode_input(encoded), Err(Failure::Budget));
    }
}

#[test]
fn base64_decoded_exact_cap_and_same_encoded_length_over_cap() {
    let groups = (MAX_INPUT as usize).div_ceil(3);
    let mut encoded = "AAAA".repeat(groups);
    encoded.replace_range(encoded.len() - 2.., "==");
    assert_eq!(
        json_wire::decode_input(&encoded).unwrap().len(),
        MAX_INPUT as usize
    );
    encoded.replace_range(encoded.len() - 1.., "A");
    assert_eq!(json_wire::decode_input(&encoded), Err(Failure::Budget));
}

#[test]
fn base64_encoded_cap_rejected_before_decode() {
    let encoded = "AAAA".repeat((MAX_INPUT as usize).div_ceil(3) + 1);
    assert_eq!(json_wire::decode_input(&encoded), Err(Failure::Budget));
}

#[test]
fn timespan_tick_precision_and_upper_boundary() {
    for (value, nanos) in [
        ("00:00:00.0000001", 100),
        ("00:00:01.2", 1_200_000_000),
        ("00:04:59.9999999", 299_999_999_900),
        ("00:05:00", 300_000_000_000),
    ] {
        assert_eq!(json_wire::duration(value), Ok(Duration::from_nanos(nanos)));
    }
    assert_eq!(json_wire::duration("00:00:00"), Err(Failure::Unsupported));
    assert_eq!(
        json_wire::duration("00:05:00.0000001"),
        Err(Failure::Unsupported)
    );
}

#[test]
fn timespan_invalid_syntax_and_components() {
    for value in [
        "-00:00:01",
        "1.00:00:01",
        "00:6:00",
        "01:00:00",
        "00:06:00",
        "00:00:60",
        "00:00:01.12345678",
        "00:aa:01",
        "00:00:0",
        "",
    ] {
        assert_eq!(
            json_wire::duration(value),
            Err(Failure::Protocol),
            "{value}"
        );
    }
}

#[test]
fn source_metadata_rejection_latches_shared_budget_before_smaller_retry() {
    let (inner, calls) = Observed::new(vec![0; MAX_METADATA as usize + 2]);
    let shared = budget();
    let mut source = Source::new(inner, MAX_METADATA + 2, shared.clone()).unwrap();
    let mut output = Vec::new();
    let mut frames = crate::wire::Frames::new(&mut output, ID, shared.clone()).unwrap();
    frames.begin().unwrap();
    let mut chunk = [0; CHUNK];
    for _ in 0..31 {
        source.read_exact(&mut chunk).unwrap();
    }
    source.read_exact(&mut chunk[..CHUNK - 1]).unwrap();
    assert_eq!(shared.check(), Ok(()));
    assert_eq!(calls.borrow().len(), 32);
    // 잔여 metadata 1바이트보다 큰 요청을 거부한 뒤 재시도도 차단함.
    assert_eq!(io_failure(source.read(&mut [0; 2])), Failure::Budget);
    assert_eq!(shared.check(), Err(Failure::Budget));
    assert_eq!(shared.entries(0), Err(Failure::Budget));
    assert_eq!(shared.charge(0, 0), Err(Failure::Budget));
    assert_eq!(io_failure(source.read(&mut [0; 1])), Failure::Budget);
    source.metadata_switch().set(false);
    assert_eq!(io_failure(source.read(&mut [0; 1])), Failure::Budget);
    assert_eq!(io_failure(source.read(&mut [])), Failure::Budget);
    assert_eq!(io_failure(source.seek(SeekFrom::Start(0))), Failure::Budget);
    assert_eq!(calls.borrow().len(), 32);
    assert_eq!(frames.complete(), Err(Failure::Budget));
    drop(frames);
    assert_eq!(frame_records(&output), vec![(1, vec![])]);

    let (other, other_calls) = Observed::new(vec![0; 22]);
    let mut other = Source::new(other, 22, shared).unwrap();
    assert_eq!(io_failure(other.read(&mut [0; 1])), Failure::Budget);
    assert!(other_calls.borrow().is_empty());
}

#[test]
fn source_metadata_disabled_before_failure_still_charges_processed_bytes() {
    let mut l = limits();
    l.processed = MAX_METADATA + 1;
    let shared = Budget::new(l).unwrap();
    let (inner, calls) = Observed::new(vec![0; MAX_METADATA as usize + 1]);
    let mut source = Source::new(inner, MAX_METADATA + 1, shared.clone()).unwrap();
    let mut chunk = [0; CHUNK];
    for _ in 0..32 {
        source.read_exact(&mut chunk).unwrap();
    }
    assert_eq!(shared.check(), Ok(()));
    source.metadata_switch().set(false);
    assert_eq!(source.read(&mut [0; 1]).unwrap(), 1);
    assert_eq!(calls.borrow().len(), 33);
    assert_eq!(shared.check(), Ok(()));
    assert_eq!(shared.charge(1, 0), Err(Failure::Budget));
}
