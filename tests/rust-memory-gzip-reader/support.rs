use std::convert::Infallible;
use zipelo_memory_gzip_reader::{Budget, Completed, Error, Failure, Limits, Request};

pub fn fixture(name: &str) -> Vec<u8> {
    let line = include_str!("fixtures.hex")
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':').unwrap();
            (key == name).then_some(value)
        })
        .expect("고정 fixture가 있어야 함");
    line.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

pub fn normal() -> Vec<u8> {
    fixture("flags_00")
}

pub fn bad_after_output() -> Vec<u8> {
    let mut bytes = fixture("byte_a");
    bytes[10] = 0;
    bytes.insert(16, 7);
    bytes
}

pub fn request(bytes: &[u8], selected: bool) -> Request {
    Request {
        input: bytes.to_vec().into_boxed_slice(),
        selected,
    }
}

pub fn budget() -> Budget {
    Budget::new(Limits::default()).unwrap()
}

pub fn read(
    bytes: &[u8],
    selected: bool,
    budget: &mut Budget,
) -> Result<Completed, Error<Infallible>> {
    zipelo_memory_gzip_reader::read(request(bytes, selected), budget, |_| Ok(()))
}

pub fn reader_error<E: std::fmt::Debug>(result: Result<Completed, Error<E>>, expected: Failure) {
    match result {
        Err(Error::Reader(actual)) => assert_eq!(actual, expected),
        other => panic!("완료 결과 없이 {expected:?} 오류여야 함: {other:?}"),
    }
}

pub fn assert_valid(
    name: &str,
    selected: bool,
    payload: &[u8],
    header: usize,
    raw: usize,
    crc: u32,
) {
    let bytes = fixture(name);
    let mut limits = budget();
    let result = read(&bytes, selected, &mut limits).unwrap();
    assert_eq!(result.bytes, selected.then(|| payload.to_vec()));
    assert_eq!(result.boundary.header_bytes, header);
    assert_eq!(result.boundary.deflate_bytes, raw as u64);
    assert_eq!(result.boundary.inflated_bytes, payload.len() as u64);
    assert_eq!(result.boundary.crc32, crc);
    assert_eq!(result.boundary.member_bytes, bytes.len());
    assert!(result.boundary.offered_bytes >= raw as u64);
    assert!(result.boundary.offered_bytes <= (raw + 8) as u64);
    assert_eq!(
        result.boundary.read_ahead_bytes as u64,
        result.boundary.offered_bytes - raw as u64
    );
    assert_eq!(result.usage.processed, (bytes.len() + payload.len()) as u64);
    assert_eq!(
        result.usage.output,
        if selected { payload.len() as u64 } else { 0 }
    );
    assert_eq!(result.usage.metadata, header as u64);
    assert_eq!(result.usage, limits.usage());
}
