use std::{
    io::{Cursor, Read},
    time::Duration,
};
use zip::{CompressionMethod, DateTime, ZipArchive};
use zipelo_rust_memory_zip_create::{
    Checkpoint, Entry, Failure, Limits, Method, Options, Request, Stop, create,
};

pub fn entry(name: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Entry {
    Entry {
        name: name.into(),
        bytes: bytes.into().into_boxed_slice(),
    }
}

pub fn request(method: Method, entries: Vec<Entry>) -> Request {
    Request {
        entries,
        options: Options {
            method,
            ..Options::default()
        },
        limits: Limits::default(),
        timeout: Duration::from_secs(300),
    }
}

pub fn small(method: Method) -> Request {
    request(method, vec![entry("hello.txt", vec![7, 8, 9])])
}

pub fn run(request: Request) -> Vec<u8> {
    create(request, |_| Ok(())).expect("정상 입력의 생성 실패")
}

pub fn fail_before_output(request: Request, expected: Failure) {
    let mut points = Vec::new();
    let result = create(request, |point| {
        points.push(point);
        Ok(())
    });
    assert_eq!(result, Err(expected));
    assert!(!points.iter().any(|point| matches!(
        point,
        Checkpoint::Entry { .. }
            | Checkpoint::Input { .. }
            | Checkpoint::Write { .. }
            | Checkpoint::Finish
            | Checkpoint::Return
    )));
}

pub fn assert_files(bytes: &[u8], method: Method, expected: &[(&str, &[u8], u32)]) {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert_eq!(archive.len(), expected.len());
    for (index, (name, content, crc)) in expected.iter().enumerate() {
        let mut file = archive.by_index(index).unwrap();
        assert_eq!(file.name(), *name);
        assert!(!file.is_dir());
        assert!(!file.encrypted());
        assert_eq!(file.size(), content.len() as u64);
        assert_eq!(file.crc32(), *crc);
        assert_eq!(file.last_modified(), Some(DateTime::DEFAULT));
        assert_eq!(
            file.compression(),
            match method {
                Method::Stored => CompressionMethod::Stored,
                Method::Deflate => CompressionMethod::Deflated,
            }
        );
        let mut actual = Vec::new();
        file.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, *content);
    }
}

pub fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

pub fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

// 이 함수는 시험에서 새로 만든 정상 ZIP의 구조만 확인함.
pub fn assert_zip32(bytes: &[u8], count: usize) -> usize {
    assert!(bytes.len() >= 22);
    let end = bytes.len() - 22;
    assert_eq!(&bytes[end..end + 4], b"PK\x05\x06");
    assert_eq!(u16_at(bytes, end + 4), 0);
    assert_eq!(u16_at(bytes, end + 6), 0);
    assert_eq!(u16_at(bytes, end + 8) as usize, count);
    assert_eq!(u16_at(bytes, end + 10) as usize, count);
    assert_eq!(u16_at(bytes, end + 20), 0);
    let central = u32_at(bytes, end + 16) as usize;
    assert_eq!(central + u32_at(bytes, end + 12) as usize, end);
    let mut position = central;
    for _ in 0..count {
        assert_eq!(&bytes[position..position + 4], b"PK\x01\x02");
        assert!(u16_at(bytes, position + 6) <= 20);
        assert_eq!(u16_at(bytes, position + 30), 0);
        assert_eq!(u16_at(bytes, position + 32), 0);
        assert_eq!(u16_at(bytes, position + 34), 0);
        assert_ne!(u32_at(bytes, position + 20), u32::MAX);
        assert_ne!(u32_at(bytes, position + 24), u32::MAX);
        let local = u32_at(bytes, position + 42) as usize;
        assert_eq!(&bytes[local..local + 4], b"PK\x03\x04");
        assert!(u16_at(bytes, local + 4) <= 20);
        assert_eq!(u16_at(bytes, local + 28), 0);
        assert_eq!(u16_at(bytes, local + 6) & 9, 0);
        assert_eq!(u32_at(bytes, local + 14), u32_at(bytes, position + 16));
        assert_eq!(u32_at(bytes, local + 18), u32_at(bytes, position + 20));
        assert_eq!(u32_at(bytes, local + 22), u32_at(bytes, position + 24));
        position += 46 + u16_at(bytes, position + 28) as usize;
    }
    assert_eq!(position, end);
    central
}

pub fn failure_for(stop: Stop) -> Failure {
    match stop {
        Stop::Cancelled => Failure::Cancelled,
        Stop::Deadline => Failure::Deadline,
    }
}
