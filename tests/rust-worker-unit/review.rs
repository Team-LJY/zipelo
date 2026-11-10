use std::{io, time::Duration};

use zip::result::ZipError;

use crate::{error::Failure, json_wire, support::*};

#[test]
fn unix_special_empty_slash_entries_cannot_become_directories() {
    for unix_type in [0x1000u32, 0x2000, 0x6000, 0xc000] {
        let mut entry = Spec::empty("special/");
        entry.attributes = unix_type << 16;
        let input = sample(&[entry]).bytes;
        assert_eq!(list(input.clone()).unwrap()[0]["Entry"]["Kind"], 4);
        let (result, output) = extract(input, limits());
        assert_eq!(result, Err(Failure::UnsupportedEntry));
        assert_eq!(frame_records(&output), vec![(1, vec![])]);
    }
}

#[test]
fn unix_allowed_types_preserve_regular_and_directory_extraction() {
    for (unix_type, name, kind) in [(0u32, "dir/", 1), (0x4000, "dir/", 1), (0x8000, "file", 0)] {
        let mut entry = Spec::empty(name);
        entry.attributes = unix_type << 16;
        let input = sample(&[entry]).bytes;
        assert_eq!(list(input.clone()).unwrap()[0]["Entry"]["Kind"], kind);
        let (result, output) = extract(input, limits());
        assert_eq!(result, Ok(()));
        assert_eq!(frame_records(&output).last(), Some(&(5, vec![1])));
    }
}

#[test]
fn duration_length_cap_rejects_long_colon_and_digit_inputs() {
    for input in [
        ":".repeat(32 * 1024 * 1024),
        "0".repeat(1024),
        "00:00:01.00000000".to_owned(),
    ] {
        assert_eq!(json_wire::duration(&input), Err(Failure::Protocol));
    }
    assert_eq!(
        json_wire::duration("00:00:01.1234567"),
        Ok(Duration::from_nanos(1_123_456_700))
    );
}

#[test]
fn duration_fixed_components_reject_extra_separators_within_cap() {
    for input in [
        "00:00:01:",
        "00:00:01:00",
        "::::::::",
        "00::00:01",
        "00:00:::",
    ] {
        assert_eq!(json_wire::duration(input), Err(Failure::Protocol));
    }
    for input in ["00:00:01", "00:00:01.0", "00:00:01.0000000"] {
        assert_eq!(json_wire::duration(input), Ok(Duration::from_secs(1)));
    }
}

fn classify_direct_and_wrapped(make: impl Fn() -> ZipError, expected: Failure) {
    assert_eq!(Failure::from(make()), expected);
    let borrowed = make();
    assert_eq!(Failure::from(&borrowed), expected);
    let wrapped = io::Error::other(make());
    assert_eq!(Failure::from(&wrapped), expected);
    assert_eq!(Failure::from(wrapped), expected);
    // 상류 From<ZipError>가 보존하는 내부 오류도 대조함.
    let wrapped: io::Error = make().into();
    assert_eq!(Failure::from(wrapped), expected);
}

#[test]
fn errors_direct_and_nested_observed_eof_preserved() {
    assert_eq!(
        Failure::from(io::Error::from(io::ErrorKind::UnexpectedEof)),
        Failure::Truncated
    );
    classify_direct_and_wrapped(
        || ZipError::Io(io::Error::from(io::ErrorKind::UnexpectedEof)),
        Failure::Truncated,
    );
    classify_direct_and_wrapped(
        || {
            ZipError::Io(io::Error::other(ZipError::Io(io::Error::from(
                io::ErrorKind::UnexpectedEof,
            ))))
        },
        Failure::Truncated,
    );
}

#[test]
fn errors_direct_and_nested_budget_evidence_preserved() {
    assert_eq!(
        Failure::from(io::Error::other(Failure::Budget)),
        Failure::Budget
    );
    classify_direct_and_wrapped(
        || ZipError::Io(io::Error::other(Failure::Budget)),
        Failure::Budget,
    );
    classify_direct_and_wrapped(
        || ZipError::DecompressedSizeLimitExceeded { size: 2, limit: 1 },
        Failure::Budget,
    );
    classify_direct_and_wrapped(
        || {
            ZipError::Io(io::Error::other(ZipError::DecompressedSizeLimitExceeded {
                size: 2,
                limit: 1,
            }))
        },
        Failure::Budget,
    );
}

#[test]
fn errors_generic_malformed_and_io_do_not_infer_eof_or_budget() {
    classify_direct_and_wrapped(
        || ZipError::InvalidArchive("UnexpectedEof BudgetExceeded".into()),
        Failure::Integrity,
    );
    classify_direct_and_wrapped(
        || ZipError::Io(io::Error::from(io::ErrorKind::InvalidData)),
        Failure::Integrity,
    );
    classify_direct_and_wrapped(
        || ZipError::Io(io::Error::other("UnexpectedEof BudgetExceeded")),
        Failure::Engine,
    );
    classify_direct_and_wrapped(|| ZipError::DecompressedSizeUnknown, Failure::Integrity);
}

#[test]
fn errors_direct_and_wrapped_unsupported_classification_matches() {
    classify_direct_and_wrapped(|| ZipError::InvalidPassword, Failure::Unsupported);
    classify_direct_and_wrapped(
        || ZipError::CompressionMethodNotSupported(99),
        Failure::Unsupported,
    );
    classify_direct_and_wrapped(
        || ZipError::UnsupportedArchive("unsupported"),
        Failure::Unsupported,
    );
}
