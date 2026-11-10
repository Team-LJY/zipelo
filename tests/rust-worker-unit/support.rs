use std::{
    cell::RefCell,
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    rc::Rc,
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

use crate::{
    budget::{Budget, Limits, Source},
    error::{Failure, Result},
    wire::{Config, Frames},
    zip_adapter,
};

pub const GUID: &str = "00112233-4455-6677-8899-aabbccddeeff";
pub const ID: [u8; 16] = [
    0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
];
pub const DATA: &[u8] = b"123456789";
pub const CRC: u32 = 0xcbf43926;

pub fn limits() -> Limits {
    Limits {
        entries: 1024,
        processed: 128 * 1024 * 1024,
        output: 16 * 1024 * 1024,
        duration: Duration::from_secs(300),
    }
}

pub fn budget() -> Budget {
    Budget::new(limits()).unwrap()
}

pub fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

pub fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

pub fn config(length: u64) -> Config {
    Config {
        id: ID,
        length,
        limits: limits(),
    }
}

pub fn config_bytes() -> Vec<u8> {
    let mut bytes = vec![0; 64];
    bytes[..4].copy_from_slice(b"ZXE2");
    put32(&mut bytes, 4, 2);
    bytes[8..24].copy_from_slice(&ID);
    for (offset, value) in [
        (24, 22),
        (32, 134_217_728),
        (40, 16_777_216),
        (48, 1024),
        (56, 3_000_000_000),
    ] {
        put64(&mut bytes, offset, value);
    }
    bytes
}

pub fn reply(sequence: u64, offset: u64, body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 48];
    bytes[..4].copy_from_slice(b"ZXR2");
    put32(&mut bytes, 4, 2);
    bytes[8..24].copy_from_slice(&ID);
    put64(&mut bytes, 24, sequence);
    put64(&mut bytes, 32, offset);
    put32(&mut bytes, 40, body.len() as u32);
    bytes.extend_from_slice(body);
    bytes
}

#[derive(Clone, Default)]
pub struct Capture(pub Rc<RefCell<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub struct Observed {
    pub data: Cursor<Vec<u8>>,
    pub reads: Rc<RefCell<Vec<usize>>>,
}

impl Observed {
    pub fn new(data: Vec<u8>) -> (Self, Rc<RefCell<Vec<usize>>>) {
        let reads = Rc::new(RefCell::new(Vec::new()));
        (
            Self {
                data: Cursor::new(data),
                reads: reads.clone(),
            },
            reads,
        )
    }
}

impl Read for Observed {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.reads.borrow_mut().push(bytes.len());
        self.data.read(bytes)
    }
}

impl Seek for Observed {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.data.seek(from)
    }
}

pub fn io_failure<T>(result: io::Result<T>) -> Failure {
    match result {
        Err(error) => error.into(),
        Ok(_) => panic!("오류가 필요함"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameRecord {
    pub kind: u16,
    pub item_id: [u8; 16],
    pub payload_length: usize,
    pub item_length: Option<u64>,
    pub payload: Vec<u8>,
}

pub fn frame_records(bytes: &[u8]) -> Vec<FrameRecord> {
    let mut at = 0;
    let mut result = Vec::new();
    while at < bytes.len() {
        assert!(bytes.len() - at >= 64);
        let h = &bytes[at..at + 64];
        assert_eq!(&h[..4], b"ZIP2");
        assert_eq!(&h[4..6], &[2, 0]);
        assert_eq!(&h[12..16], &[0; 4]);
        assert_eq!(&h[16..32], &ID);
        assert_eq!(&h[56..64], &[0; 8]);
        assert_eq!(
            u64::from_le_bytes(h[48..56].try_into().unwrap()),
            result.len() as u64
        );
        let size = u32::from_le_bytes(h[8..12].try_into().unwrap()) as usize;
        assert!(size <= 65_536 && size <= bytes.len() - at - 64);
        let kind = u16::from_le_bytes(h[6..8].try_into().unwrap());
        let payload = bytes[at + 64..at + 64 + size].to_vec();
        let item_length = if matches!(kind, 2 | 4) {
            assert_eq!(size, 8);
            Some(u64::from_le_bytes(payload[..].try_into().unwrap()))
        } else {
            None
        };
        result.push(FrameRecord {
            kind,
            item_id: h[32..48].try_into().unwrap(),
            payload_length: size,
            item_length,
            payload,
        });
        at += 64 + size;
    }
    result
}

pub fn frame_shapes(records: &[FrameRecord]) -> Vec<(u16, Vec<u8>)> {
    records
        .iter()
        .map(|r| (r.kind, r.payload.clone()))
        .collect()
}

// 항목별 ID와 길이 및 순서를 고정 계약으로 대조함.
pub fn validate_success(
    records: &[FrameRecord],
    expected: &[&[u8]],
) -> std::result::Result<(), &'static str> {
    if records.iter().any(|r| r.payload_length != r.payload.len()) {
        return Err("payload length");
    }
    let mut records = records.iter();
    let begin = records.next().ok_or("missing BeginJob")?;
    if begin.kind != 1
        || begin.item_id != [0; 16]
        || !begin.payload.is_empty()
        || begin.item_length.is_some()
    {
        return Err("BeginJob");
    }
    for (index, bytes) in expected.iter().enumerate() {
        let mut id = [0; 16];
        id[..4].copy_from_slice(&((index + 1) as u32).to_le_bytes());
        let begin = records.next().ok_or("missing BeginItem")?;
        if begin.kind != 2
            || begin.item_id != id
            || begin.item_length != Some(bytes.len() as u64)
            || begin.payload != (bytes.len() as u64).to_le_bytes()
        {
            return Err("BeginItem");
        }
        let mut actual = 0;
        loop {
            let record = records.next().ok_or("missing EndItem")?;
            if record.item_id != id {
                return Err("item ownership");
            }
            match record.kind {
                3 => {
                    if record.payload.is_empty()
                        || record.payload.len() > 65_536
                        || record.item_length.is_some()
                        || record.payload.len() > bytes.len() - actual
                        || record.payload != bytes[actual..actual + record.payload.len()]
                    {
                        return Err("Data");
                    }
                    actual += record.payload.len();
                }
                4 => {
                    if actual != bytes.len()
                        || record.item_length != Some(actual as u64)
                        || record.payload != (actual as u64).to_le_bytes()
                    {
                        return Err("EndItem");
                    }
                    break;
                }
                _ => return Err("item order"),
            }
        }
    }
    let end = records.next().ok_or("missing EndJob")?;
    if end.kind != 5
        || end.item_id != [0; 16]
        || end.payload != [1]
        || end.item_length.is_some()
        || records.next().is_some()
    {
        return Err("EndJob");
    }
    Ok(())
}

#[derive(Clone)]
pub struct Spec {
    pub name: String,
    pub method: u16,
    pub packed: Vec<u8>,
    pub size: u32,
    pub crc: u32,
    pub descriptor: u8,
    pub attributes: u32,
}

impl Spec {
    pub fn stored(name: &str) -> Self {
        Self {
            name: name.into(),
            method: 0,
            packed: DATA.to_vec(),
            size: 9,
            crc: CRC,
            descriptor: 0,
            attributes: 0,
        }
    }

    pub fn deflate(name: &str) -> Self {
        let mut s = Self::stored(name);
        s.method = 8;
        // RFC 1951의 최종 비압축 블록을 고정 벡터로 작성함.
        s.packed = [vec![1, 9, 0, 246, 255], DATA.to_vec()].concat();
        s
    }

    pub fn empty(name: &str) -> Self {
        Self {
            packed: Vec::new(),
            size: 0,
            crc: 0,
            ..Self::stored(name)
        }
    }
}

pub struct Sample {
    pub bytes: Vec<u8>,
    pub local: Vec<usize>,
    pub data: Vec<usize>,
    pub descriptor: Vec<usize>,
    pub central: Vec<usize>,
    pub end: usize,
}

// 시험 표본만 작성함. 제품 파서와 CRC 및 압축 구현은 복제하지 않음.
pub fn sample(entries: &[Spec]) -> Sample {
    let mut s = Sample {
        bytes: Vec::new(),
        local: Vec::new(),
        data: Vec::new(),
        descriptor: Vec::new(),
        central: Vec::new(),
        end: 0,
    };
    for e in entries {
        s.local.push(s.bytes.len());
        let mut h = vec![0; 30];
        put32(&mut h, 0, 0x04034b50);
        put16(&mut h, 4, 20);
        put16(&mut h, 6, 0x0800 | if e.descriptor > 0 { 8 } else { 0 });
        put16(&mut h, 8, e.method);
        if e.descriptor == 0 {
            put32(&mut h, 14, e.crc);
            put32(&mut h, 18, e.packed.len() as u32);
            put32(&mut h, 22, e.size);
        }
        put16(&mut h, 26, e.name.len() as u16);
        s.bytes.extend(h);
        s.bytes.extend(e.name.as_bytes());
        s.data.push(s.bytes.len());
        s.bytes.extend(&e.packed);
        s.descriptor.push(s.bytes.len());
        if e.descriptor > 0 {
            if e.descriptor == 2 {
                s.bytes.extend(0x08074b50u32.to_le_bytes());
            }
            s.bytes.extend(e.crc.to_le_bytes());
            s.bytes.extend((e.packed.len() as u32).to_le_bytes());
            s.bytes.extend(e.size.to_le_bytes());
        }
    }
    let start = s.bytes.len();
    for (i, e) in entries.iter().enumerate() {
        s.central.push(s.bytes.len());
        let mut h = vec![0; 46];
        put32(&mut h, 0, 0x02014b50);
        put16(&mut h, 4, 0x0314);
        put16(&mut h, 6, 20);
        put16(&mut h, 8, 0x0800 | if e.descriptor > 0 { 8 } else { 0 });
        put16(&mut h, 10, e.method);
        put32(&mut h, 16, e.crc);
        put32(&mut h, 20, e.packed.len() as u32);
        put32(&mut h, 24, e.size);
        put16(&mut h, 28, e.name.len() as u16);
        put32(&mut h, 38, e.attributes);
        put32(&mut h, 42, s.local[i] as u32);
        s.bytes.extend(h);
        s.bytes.extend(e.name.as_bytes());
    }
    s.end = s.bytes.len();
    let mut h = vec![0; 22];
    put32(&mut h, 0, 0x06054b50);
    put16(&mut h, 8, entries.len() as u16);
    put16(&mut h, 10, entries.len() as u16);
    put32(&mut h, 12, (s.end - start) as u32);
    put32(&mut h, 16, start as u32);
    s.bytes.extend(h);
    s
}

pub fn list(bytes: Vec<u8>) -> Result<Vec<Value>> {
    let b = budget();
    let source = Source::new(Cursor::new(&bytes), bytes.len() as u64, b.clone())?;
    zip_adapter::list(source, &b)
}

pub fn extract(bytes: Vec<u8>, limits: Limits) -> (Result<()>, Vec<u8>) {
    let b = Budget::new(limits).unwrap();
    let mut output = Vec::new();
    let result = {
        let mut frames = Frames::new(&mut output, ID, b.clone()).unwrap();
        frames.begin().unwrap();
        match Source::new(Cursor::new(&bytes), bytes.len() as u64, b.clone()) {
            Ok(source) => zip_adapter::extract(source, &b, &mut frames),
            Err(error) => Err(error),
        }
    };
    (result, output)
}

pub fn request(bytes: &[u8]) -> Value {
    json!({
        "JobId": GUID, "Action": 0, "Inputs": [STANDARD.encode(bytes)], "Names": [],
        "Limits": {
            "MaxEntries": 1024, "MaxProcessedBytes": 134217728,
            "MaxOutputBytes": 16777216, "MaxPasswordAttempts": 1,
            "MaxDuration": "00:05:00"
        }
    })
}

pub fn json_bytes(value: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(value).unwrap();
    [(body.len() as i32).to_le_bytes().to_vec(), body].concat()
}

pub fn json_result(input: &mut impl Read) -> Value {
    let mut bytes = Vec::new();
    crate::json_wire::run(input, &mut bytes).unwrap();
    assert!(bytes.len() >= 4);
    assert_eq!(
        i32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize,
        bytes.len() - 4
    );
    serde_json::from_slice(&bytes[4..]).unwrap()
}
