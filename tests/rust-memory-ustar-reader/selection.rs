use crate::support::*;
use std::convert::Infallible;
use zipelo_memory_ustar_reader::{Checkpoint, Failure, read};

#[test]
fn empty_selection_scans_once() {
    let bytes = normal();
    let size = bytes.len() as u64;
    let mut pass_two = false;
    let mut selected = 0;
    let result = read(
        request(bytes),
        |entries| {
            selected += 1;
            assert_eq!(entries.len(), 4);
            Ok::<_, Infallible>(vec![])
        },
        |point| {
            if matches!(point, Checkpoint::Read { pass: 2, .. }) {
                pass_two = true;
            }
            Ok(())
        },
    )
    .expect("목록 조회는 성공해야 함");
    assert_eq!(selected, 1);
    assert!(!pass_two);
    assert!(result.files.is_empty());
    assert_eq!(result.usage.processed, size);
}

#[test]
fn reverse_selection_keeps_archive_order() {
    let result = run(normal(), &["3", "1", "2"]);
    assert_eq!(
        result
            .files
            .iter()
            .map(|f| f.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "3"]
    );
    assert_eq!(result.files[0].bytes, b"alpha");
    assert!(result.files[1].bytes.is_empty());
    assert_eq!(result.files[2].bytes, payload());
}

#[test]
fn directory_only_still_scans_twice() {
    let bytes = normal();
    let size = bytes.len() as u64;
    let result = run(bytes, &["0"]);
    assert!(result.files.is_empty());
    assert_eq!(result.usage.processed, size * 2);
    assert_eq!(result.usage.output, 0);
    assert_eq!(result.usage.entries, 4);
}

#[test]
fn empty_selected_file_is_present() {
    let result = run(normal(), &["2"]);
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].id, "2");
    assert!(result.files[0].bytes.is_empty());
}

#[test]
fn selected_payload_and_full_input_hash() {
    let bytes = normal();
    let expected = digest(&bytes);
    let size = bytes.len() as u64;
    let result = run(bytes, &["2", "3"]);
    assert_eq!(result.files.len(), 2);
    assert_eq!(result.files[1].bytes, payload());
    assert_eq!(digest(&result.files[1].bytes), digest(&payload()));
    assert_eq!(result.input_sha256, expected);
    assert_eq!(result.usage.processed, size * 2);
    assert_eq!(result.usage.output, 140000);
    assert_eq!(result.usage.entries, 4);
}

#[test]
fn reject_duplicate() {
    failure(
        limited(normal(), &["1", "1"], Default::default()),
        Failure::Unsupported,
    );
}

#[test]
fn reject_unknown() {
    failure(
        limited(normal(), &["99"], Default::default()),
        Failure::Unsupported,
    );
}

#[test]
fn reject_excess() {
    failure(
        limited(normal(), &["0", "1", "2", "3", "0"], Default::default()),
        Failure::Unsupported,
    );
}

#[test]
fn reject_leading_zero() {
    failure(
        limited(normal(), &["01"], Default::default()),
        Failure::Unsupported,
    );
}

#[test]
fn reject_plus() {
    failure(
        limited(normal(), &["+1"], Default::default()),
        Failure::Unsupported,
    );
}

#[test]
fn reject_leading_space() {
    failure(
        limited(normal(), &[" 1"], Default::default()),
        Failure::Unsupported,
    );
}

#[test]
fn reject_trailing_space() {
    failure(
        limited(normal(), &["1 "], Default::default()),
        Failure::Unsupported,
    );
}

#[test]
fn reject_empty_id() {
    failure(
        limited(normal(), &[""], Default::default()),
        Failure::Unsupported,
    );
}

#[test]
fn decimal_ids_are_exact_above_nine() {
    let names = (0..11).map(|i| format!("f{i}")).collect::<Vec<_>>();
    let fixtures = names
        .iter()
        .map(|n| Fixture::file(n, &[]))
        .collect::<Vec<_>>();
    let bytes = archive(&fixtures);
    assert_eq!(
        run(bytes.clone(), &["10", "0"])
            .files
            .iter()
            .map(|f| f.id.as_str())
            .collect::<Vec<_>>(),
        ["0", "10"]
    );
    failure(
        limited(bytes, &["010"], Default::default()),
        Failure::Unsupported,
    );
}

#[test]
fn selection_after_complete_first_pass() {
    use std::cell::Cell;
    let bytes = normal();
    let size = bytes.len();
    let offset = Cell::new(0);
    let called = Cell::new(false);
    read(
        request(bytes),
        |entries| {
            assert_eq!(offset.get(), size);
            assert_eq!(entries.len(), 4);
            called.set(true);
            Ok::<_, Infallible>(vec![])
        },
        |point| {
            if let Checkpoint::Read {
                pass: 1,
                offset: at,
            } = point
            {
                offset.set(at);
            }
            Ok(())
        },
    )
    .expect("검증 후 선택은 성공해야 함");
    assert!(called.get());
}

#[test]
fn separate_owned_requests_have_separate_usage() {
    let bytes = normal();
    let first = run(bytes.clone(), &["1"]);
    let second = run(bytes, &["1"]);
    assert_eq!(first.usage, second.usage);
    assert_eq!(first.files, second.files);
}
