use std::io::{self, Read};

use crc32fast::Hasher;
use zlib_rs::{Inflate, InflateError, InflateFlush, Status};

use crate::error::Failure;

pub const CHUNK: usize = 65_536;
pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_CALLS: u64 = 131_072;

#[derive(Debug)]
pub enum Error {
    Backend(InflateError),
    Input(io::Error),
    Host(Failure),
    Cancelled,
    Incomplete,
    Trailing,
    Size,
    Crc,
    Limit,
    NoProgress,
    InvalidAccounting,
    Blocked,
}

impl Error {
    pub fn failure(&self) -> Failure {
        match self {
            Self::Backend(InflateError::DataError) | Self::Trailing | Self::Size | Self::Crc => {
                Failure::Integrity
            }
            Self::Backend(InflateError::StreamError) => Failure::DeflateStream,
            Self::Backend(InflateError::MemError) => Failure::DeflateMemory,
            Self::Backend(InflateError::NeedDict { dict_id }) => {
                Failure::DeflateDictionary(*dict_id)
            }
            Self::Input(error) => error
                .get_ref()
                .and_then(|cause| cause.downcast_ref::<Failure>())
                .copied()
                .unwrap_or(Failure::Engine),
            Self::Host(failure) => *failure,
            Self::Cancelled => Failure::Cancelled,
            Self::Incomplete => Failure::DeflateIncomplete,
            Self::Limit => Failure::Budget,
            Self::NoProgress | Self::InvalidAccounting | Self::Blocked => Failure::Engine,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub input: u64,
    pub output: u64,
    pub reads: u64,
    pub calls: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            input: MAX_BYTES,
            output: MAX_BYTES,
            reads: MAX_CALLS,
            calls: MAX_CALLS,
        }
    }
}

pub struct Work {
    limits: Limits,
    input: u64,
    output: u64,
    reads: u64,
    calls: u64,
    failed: bool,
}

impl Work {
    pub fn new(limits: Limits) -> Result<Self, Error> {
        if limits.input > MAX_BYTES
            || limits.output > MAX_BYTES
            || limits.reads > MAX_CALLS
            || limits.calls > MAX_CALLS
        {
            return Err(Error::Limit);
        }
        Ok(Self {
            limits,
            input: 0,
            output: 0,
            reads: 0,
            calls: 0,
            failed: false,
        })
    }

    pub fn counts(&self) -> (u64, u64, u64, u64) {
        (self.input, self.output, self.reads, self.calls)
    }
}

#[derive(Clone, Copy)]
pub struct Expected {
    pub packed: u64,
    pub size: u64,
    pub crc: u32,
}

// 압축 상태와 입출력 크기를 함께 확인함. 진행 불가는 손상으로 추정하지 않음.
pub fn check_progress(
    consumed: usize,
    produced: usize,
    eof: bool,
    ended: bool,
) -> Result<(), Error> {
    if !ended && consumed == 0 && produced == 0 {
        return Err(if eof {
            Error::Incomplete
        } else {
            Error::NoProgress
        });
    }
    Ok(())
}

// 체크포인트는 호출 사이의 협조 취소만 제공함. codec 내부 강제 중단은 보장하지 않음.
pub fn decode<R, C, E>(
    reader: &mut R,
    expected: Expected,
    work: &mut Work,
    output_chunk: usize,
    mut checkpoint: C,
    mut emit: E,
) -> Result<(), Error>
where
    R: Read,
    C: FnMut() -> Result<(), Error>,
    E: FnMut(&[u8]) -> Result<(), Error>,
{
    if work.failed {
        return Err(Error::Blocked);
    }
    let result = decode_inner(
        reader,
        expected,
        work,
        output_chunk,
        &mut checkpoint,
        &mut emit,
    );
    if result.is_err() {
        work.failed = true;
    }
    result
}

fn decode_inner<R, C, E>(
    reader: &mut R,
    expected: Expected,
    work: &mut Work,
    output_chunk: usize,
    checkpoint: &mut C,
    emit: &mut E,
) -> Result<(), Error>
where
    R: Read,
    C: FnMut() -> Result<(), Error>,
    E: FnMut(&[u8]) -> Result<(), Error>,
{
    checkpoint()?;
    if output_chunk == 0 || output_chunk > CHUNK {
        return Err(Error::Limit);
    }
    if expected.packed > work.limits.input - work.input
        || expected.size > work.limits.output - work.output
    {
        return Err(Error::Limit);
    }
    // 공개 생성자는 fallible API가 아님. 초기화 panic과 OOM은 반환 오류로 보장하지 않음.
    let mut decoder = Inflate::new(false, 15);
    let mut input = [0; CHUNK];
    let mut output = [0; CHUNK];
    let (mut start, mut end) = (0usize, 0usize);
    let (mut loaded, mut actual) = (0u64, 0u64);
    let mut crc = Hasher::new();
    loop {
        checkpoint()?;
        if start == end && loaded < expected.packed {
            if work.reads == work.limits.reads {
                return Err(Error::Limit);
            }
            work.reads += 1;
            let wanted = (expected.packed - loaded).min(CHUNK as u64) as usize;
            let got = reader.read(&mut input[..wanted]).map_err(Error::Input)?;
            checkpoint()?;
            if got > wanted {
                return Err(Error::InvalidAccounting);
            }
            if got == 0 {
                return Err(Error::Incomplete);
            }
            if got as u64 > work.limits.input - work.input {
                return Err(Error::Limit);
            }
            work.input += got as u64;
            loaded += got as u64;
            start = 0;
            end = got;
        }
        if work.calls == work.limits.calls {
            return Err(Error::Limit);
        }
        work.calls += 1;
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let result = decoder.decompress(
            &input[start..end],
            &mut output[..output_chunk],
            InflateFlush::NoFlush,
        );
        // 같은 호출에서 확정된 backend 오류를 뒤의 취소가 덮어쓰지 않음.
        let status = result.map_err(Error::Backend)?;
        let consumed = decoder
            .total_in()
            .checked_sub(before_in)
            .ok_or(Error::InvalidAccounting)?;
        let produced = decoder
            .total_out()
            .checked_sub(before_out)
            .ok_or(Error::InvalidAccounting)?;
        if consumed > (end - start) as u64 || produced > output_chunk as u64 {
            return Err(Error::InvalidAccounting);
        }
        start += consumed as usize;
        if produced > expected.size - actual {
            return Err(Error::Size);
        }
        if produced > work.limits.output - work.output {
            return Err(Error::Limit);
        }
        let ended = status == Status::StreamEnd;
        check_progress(
            consumed as usize,
            produced as usize,
            loaded == expected.packed && start == end,
            ended,
        )?;
        if ended && decoder.total_in() != expected.packed {
            return Err(Error::Trailing);
        }
        checkpoint()?;
        if produced != 0 {
            actual += produced;
            work.output += produced;
            crc.update(&output[..produced as usize]);
            emit(&output[..produced as usize])?;
        }
        if ended {
            if actual != expected.size {
                return Err(Error::Size);
            }
            if crc.finalize() != expected.crc {
                return Err(Error::Crc);
            }
            checkpoint()?;
            return Ok(());
        }
    }
}

impl From<Failure> for Error {
    fn from(value: Failure) -> Self {
        Self::Host(value)
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Host(Failure::from(value))
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(value: zip::result::ZipError) -> Self {
        Self::Host(Failure::from(value))
    }
}
