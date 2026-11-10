use super::support::*;
use zipelo_rust_memory_zip_create::{Method, Options};

#[test]
fn stored_empty_file_roundtrip() {
    let bytes = run(request(Method::Stored, vec![entry("empty", vec![])]));
    assert_files(&bytes, Method::Stored, &[("empty", b"", 0)]);
    assert_zip32(&bytes, 1);
}

#[test]
fn deflate_empty_file_roundtrip() {
    let bytes = run(request(Method::Deflate, vec![entry("empty", vec![])]));
    assert_files(&bytes, Method::Deflate, &[("empty", b"", 0)]);
    assert_zip32(&bytes, 1);
}

fn multiple(method: Method) {
    let bytes = run(request(
        method,
        vec![
            entry("z/first.txt", b"123456789".to_vec()),
            entry("middle", vec![]),
            entry("a/last.txt", b"123456789".to_vec()),
        ],
    ));
    assert_files(
        &bytes,
        method,
        &[
            ("z/first.txt", b"123456789", 0xcbf43926),
            ("middle", b"", 0),
            ("a/last.txt", b"123456789", 0xcbf43926),
        ],
    );
    assert_zip32(&bytes, 3);
}

#[test]
fn stored_preserves_order_contents_crc_method_and_fixed_time() {
    multiple(Method::Stored);
}

#[test]
fn deflate_preserves_order_contents_crc_method_and_fixed_time() {
    multiple(Method::Deflate);
}

#[test]
fn default_method_is_deflate() {
    let mut input = request(Method::Stored, vec![entry("f", b"123456789".to_vec())]);
    input.options = Options::default();
    assert_files(
        &run(input),
        Method::Deflate,
        &[("f", b"123456789", 0xcbf43926)],
    );
}

#[test]
fn csharp_ascii_hello_fixture_roundtrip() {
    use std::io::{Cursor, Read};
    let bytes = run(small(Method::Deflate));
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert_eq!(archive.len(), 1);
    let mut file = archive.by_index(0).unwrap();
    assert_eq!(file.name(), "hello.txt");
    let mut content = Vec::new();
    file.read_to_end(&mut content).unwrap();
    assert_eq!(content, [7, 8, 9]);
}

#[test]
fn empty_archive_is_invalid() {
    fail_before_output(
        request(Method::Stored, vec![]),
        zipelo_rust_memory_zip_create::Failure::InvalidRequest,
    );
}

#[test]
fn unsupported_flags_stop_before_input_or_output() {
    for mask in 1..8 {
        let mut input = small(Method::Stored);
        input.options.encrypted = mask & 1 != 0;
        input.options.split = mask & 2 != 0;
        input.options.zip64 = mask & 4 != 0;
        fail_before_output(input, zipelo_rust_memory_zip_create::Failure::Unsupported);
    }
}
