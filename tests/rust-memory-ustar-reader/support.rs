use std::convert::Infallible;

use sha2::{Digest, Sha256};
use zipelo_memory_ustar_reader::{Completed, Error, Failure, Limits, NamePolicy, Request, read};

pub const BLOCK: usize = 512;

pub struct Fixture<'a> {
    pub name: &'a str,
    pub prefix: &'a str,
    pub data: Vec<u8>,
    pub kind: u8,
}

impl<'a> Fixture<'a> {
    pub fn file(name: &'a str, data: &[u8]) -> Self {
        Self {
            name,
            prefix: "",
            data: data.to_vec(),
            kind: b'0',
        }
    }

    pub fn directory(name: &'a str) -> Self {
        Self {
            name,
            prefix: "",
            data: Vec::new(),
            kind: b'5',
        }
    }

    pub fn prefix(mut self, prefix: &'a str) -> Self {
        self.prefix = prefix;
        self
    }
}

// 정상 USTAR 표본만 만들며 거부 사례는 각 시험에서 한 조건씩 변경함.
pub fn archive(entries: &[Fixture<'_>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for entry in entries {
        assert!(entry.name.len() <= 100 && entry.prefix.len() <= 155);
        let mut header = [0_u8; BLOCK];
        header[..entry.name.len()].copy_from_slice(entry.name.as_bytes());
        header[345..345 + entry.prefix.len()].copy_from_slice(entry.prefix.as_bytes());
        number(&mut header, 100, 8, 420);
        number(&mut header, 108, 8, 0);
        number(&mut header, 116, 8, 0);
        number(&mut header, 124, 12, entry.data.len() as u64);
        number(&mut header, 136, 12, 0);
        header[156] = entry.kind;
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        checksum(&mut header);
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&entry.data);
        bytes.resize(bytes.len() + (BLOCK - entry.data.len() % BLOCK) % BLOCK, 0);
    }
    bytes.resize(bytes.len() + 2 * BLOCK, 0);
    bytes
}

fn number(header: &mut [u8], offset: usize, width: usize, value: u64) {
    let text = format!("{value:0digits$o}", digits = width - 1);
    assert_eq!(text.len(), width - 1);
    header[offset..offset + width - 1].copy_from_slice(text.as_bytes());
    header[offset + width - 1] = 0;
}

pub fn checksum(header: &mut [u8]) {
    assert!(header.len() >= BLOCK);
    header[148..156].fill(b' ');
    let sum: u64 = header[..BLOCK].iter().map(|v| u64::from(*v)).sum();
    let text = format!("{sum:06o}");
    assert_eq!(text.len(), 6);
    header[148..154].copy_from_slice(text.as_bytes());
    header[154] = 0;
    header[155] = b' ';
}

pub fn field(mut bytes: Vec<u8>, offset: usize, width: usize, value: &[u8]) -> Vec<u8> {
    assert!(value.len() <= width);
    bytes[offset..offset + width].fill(0);
    bytes[offset..offset + value.len()].copy_from_slice(value);
    checksum(&mut bytes[..BLOCK]);
    bytes
}

pub fn normal() -> Vec<u8> {
    archive(&[
        Fixture::directory("folder/"),
        Fixture::file("folder/a.txt", b"alpha"),
        Fixture::file("empty", &[]),
        Fixture::file("b.bin", &payload()),
    ])
}

pub fn payload() -> Vec<u8> {
    (0..140_000).map(|i| i as u8).collect()
}

pub fn request(bytes: Vec<u8>) -> Request {
    Request {
        input: bytes.into_boxed_slice(),
        policy: NamePolicy::Linux,
        limits: Limits::default(),
    }
}

pub fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

pub fn run(bytes: Vec<u8>, selection: &[&str]) -> Completed {
    read(
        request(bytes),
        |_| Ok::<_, Infallible>(ids(selection)),
        |_| Ok(()),
    )
    .expect("정상 입력은 완료되어야 함")
}

pub fn limited(
    bytes: Vec<u8>,
    selection: &[&str],
    limits: Limits,
) -> Result<Completed, Error<Infallible>> {
    let mut request = request(bytes);
    request.limits = limits;
    read(request, |_| Ok(ids(selection)), |_| Ok(()))
}

pub fn failure<E: std::fmt::Debug>(result: Result<Completed, Error<E>>, expected: Failure) {
    match result {
        Err(Error::Reader(actual)) => assert_eq!(actual, expected),
        other => panic!("예상한 오류 {expected:?} 대신 {other:?} 반환됨"),
    }
}

pub fn reject(bytes: Vec<u8>, expected: Failure) {
    let mut selected = false;
    let result = read(
        request(bytes),
        |_| {
            selected = true;
            Ok::<_, Infallible>(Vec::new())
        },
        |_| Ok(()),
    );
    failure(result, expected);
    assert!(!selected, "전체 검증 실패 전 선택 콜백이 호출됨");
}

pub fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
