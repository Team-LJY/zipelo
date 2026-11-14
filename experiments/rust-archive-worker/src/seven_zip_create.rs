use crate::{create::Plan, protocol::*, resources::Monitor, spool};
use lzma_rust2::{Lzma2Options, Lzma2Writer};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    time::{Duration, Instant},
};
use zlib_rs::crc32::crc32;

type Result<T> = std::result::Result<T, ErrorCode>;
const DICTIONARY: u32 = 1024 * 1024;
const DICTIONARY_PROPERTY: u8 = 16;

fn io<T>(value: std::io::Result<T>) -> Result<T> {
    value.map_err(crate::space::io_error)
}
fn options() -> Result<Lzma2Options> {
    let mut options = Lzma2Options::with_preset(1);
    options.lzma_options.dict_size = DICTIONARY;
    let memory = u64::from(options.lzma_options.get_memory_usage())
        .checked_mul(1024)
        .ok_or(ErrorCode::Limit)?;
    if memory > 32 * 1024 * 1024 {
        return Err(ErrorCode::Limit);
    }
    Ok(options)
}
fn packed_bound(length: u64) -> Result<u64> {
    // 한 바이트 이상인 청크의 최대 순증 3바이트와 마지막 종료 바이트를 포함함.
    length
        .checked_mul(4)
        .and_then(|n| n.checked_add(1))
        .ok_or(ErrorCode::Limit)
}

#[derive(Clone, Copy)]
struct Stream {
    size: u64,
    packed: u64,
    crc: u32,
    packed_crc: u32,
}
struct Sink<'a, M> {
    file: &'a mut File,
    check: &'a mut M,
    error: Option<ErrorCode>,
    written: u64,
    bound: u64,
    crc: u32,
    base: u64,
    entries: u32,
    reported: Instant,
}
impl<M: Monitor> Write for Sink<'_, M> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let result = (|| {
            self.check.check()?;
            let bytes = &bytes[..bytes.len().min(65_536)];
            let end = self
                .written
                .checked_add(bytes.len() as u64)
                .ok_or(ErrorCode::Limit)?;
            if end > self.bound {
                return Err(ErrorCode::Limit);
            }
            crate::space::ensure(self.file, bytes.len() as u64)?;
            let n = io(self.file.write(bytes))?;
            self.written += n as u64;
            self.crc = crc32(self.crc, &bytes[..n]);
            if n != 0 {
                crate::resources::completed(Ok(()), self.check)?;
                if self.reported.elapsed() >= Duration::from_millis(100) {
                    self.check.progress(
                        Stage::Write,
                        self.base + self.written,
                        None,
                        self.entries,
                    )?;
                    self.reported = Instant::now();
                }
            }
            Ok(n)
        })();
        result.map_err(|error| {
            self.error = Some(error);
            std::io::Error::other("7z stream write failed")
        })
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
fn pack(
    input: &mut File,
    length: u64,
    output: &mut File,
    entries: u32,
    check: &mut impl Monitor,
) -> Result<Stream> {
    let settings = options()?;
    let base = io(output.stream_position())?
        .checked_sub(32)
        .ok_or(ErrorCode::Internal)?;
    let mut sink = Sink {
        file: output,
        check,
        error: None,
        written: 0,
        bound: packed_bound(length)?,
        crc: 0,
        base,
        entries,
        reported: Instant::now(),
    };
    let result = (|| {
        io(input.rewind())?;
        let mut input = input.take(length);
        let mut encoder = Lzma2Writer::new(&mut sink, settings);
        let mut buffer = [0; 65_536];
        let mut read = 0u64;
        let mut crc = 0;
        loop {
            encoder.inner_mut().check.check()?;
            let n = io(input.read(&mut buffer))?;
            if n == 0 {
                break;
            }
            crate::resources::completed(Ok(()), encoder.inner_mut().check)?;
            read = read.checked_add(n as u64).ok_or(ErrorCode::Limit)?;
            crc = crc32(crc, &buffer[..n]);
            io(encoder.write_all(&buffer[..n]))?;
        }
        if read != length {
            return Err(ErrorCode::SourceChanged);
        }
        encoder.inner_mut().check.check()?;
        io(encoder.finish())?;
        Ok(crc)
    })();
    if let Some(error) = sink.error {
        return Err(error);
    }
    let crc = result?;
    sink.check.check()?;
    sink.check
        .progress(Stage::Write, base + sink.written, None, entries)?;
    Ok(Stream {
        size: length,
        packed: sink.written,
        crc,
        packed_crc: sink.crc,
    })
}

struct Header(Vec<u8>);
impl Header {
    fn new(capacity: usize) -> Result<Self> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| ErrorCode::Limit)?;
        Ok(Self(bytes))
    }
    fn put(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }
    fn integer(&mut self, value: u64) {
        for extra in 0..8 {
            if value < (1u64 << (7 * (extra + 1))) {
                let prefix = if extra == 0 { 0 } else { 0xffu8 << (8 - extra) };
                self.0.push(prefix | (value >> (8 * extra)) as u8);
                self.put(&value.to_le_bytes()[..extra]);
                return;
            }
        }
        self.0.push(0xff);
        self.put(&value.to_le_bytes());
    }
    fn property(&mut self, id: u8, size: usize) {
        self.0.push(id);
        self.integer(size as u64);
    }
    fn bits(&mut self, values: impl Iterator<Item = bool>) {
        let mut byte = 0;
        let mut used = 0;
        for value in values {
            if value {
                byte |= 0x80 >> used;
            }
            used += 1;
            if used == 8 {
                self.0.push(byte);
                byte = 0;
                used = 0;
            }
        }
        if used != 0 {
            self.0.push(byte);
        }
    }
}
fn empty(item: &crate::create_sources::Planned) -> bool {
    item.member.entry.size == 0
}
fn header(
    plan: &Plan,
    streams: &[Stream],
    names: usize,
    capacity: usize,
    check: &mut impl Monitor,
) -> Result<Header> {
    let mut out = Header::new(capacity)?;
    out.put(&[1]);
    if !streams.is_empty() {
        out.put(&[4, 6, 0]);
        out.integer(streams.len() as u64);
        out.put(&[9]);
        for stream in streams {
            out.integer(stream.packed);
            check.check()?;
        }
        out.put(&[10, 1]);
        for stream in streams {
            out.put(&stream.packed_crc.to_le_bytes());
        }
        out.put(&[0, 7, 11]);
        out.integer(streams.len() as u64);
        out.put(&[0]);
        for _ in streams {
            out.put(&[1, 0x21, 0x21, 1, DICTIONARY_PROPERTY]);
            check.check()?;
        }
        out.put(&[12]);
        for stream in streams {
            out.integer(stream.size);
        }
        out.put(&[10, 1]);
        for stream in streams {
            out.put(&stream.crc.to_le_bytes());
        }
        out.put(&[0, 8, 0, 0]);
    }
    out.put(&[5]);
    out.integer(plan.items.len() as u64);
    let empties = plan.items.iter().filter(|p| empty(p)).count();
    if empties != 0 {
        out.property(14, plan.items.len().div_ceil(8));
        out.bits(plan.items.iter().map(empty));
        out.property(15, empties.div_ceil(8));
        out.bits(
            plan.items
                .iter()
                .filter(|p| empty(p))
                .map(|p| p.member.entry.kind == EntryKind::File),
        );
    }
    out.property(17, names.checked_add(1).ok_or(ErrorCode::Limit)?);
    out.put(&[0]);
    for item in &plan.items {
        for unit in item
            .member
            .entry
            .name
            .encode_utf16()
            .chain(std::iter::once(0))
        {
            out.put(&unit.to_le_bytes());
        }
        crate::resources::completed(Ok(()), check)?;
    }
    out.property(
        21,
        plan.items
            .len()
            .checked_mul(4)
            .and_then(|n| n.checked_add(2))
            .ok_or(ErrorCode::Limit)?,
    );
    out.put(&[1, 0]);
    for item in &plan.items {
        let attributes: u32 = if item.member.entry.kind == EntryKind::Directory {
            0x10
        } else {
            0x20
        };
        out.put(&attributes.to_le_bytes());
        check.check()?;
    }
    out.put(&[0, 0]);
    if out.0.len() > capacity {
        return Err(ErrorCode::Internal);
    }
    Ok(out)
}
fn write(output: &mut File, bytes: &[u8], check: &mut impl Monitor) -> Result<()> {
    for chunk in bytes.chunks(65_536) {
        check.check()?;
        crate::space::ensure(output, chunk.len() as u64)?;
        io(output.write_all(chunk))?;
        crate::resources::completed(Ok(()), check)?;
    }
    Ok(())
}
struct Verify<'a, M>(&'a mut M);
impl<M: Monitor> Monitor for Verify<'_, M> {
    fn metadata_limits(&self) -> crate::archive::MetadataLimits {
        self.0.metadata_limits()
    }
    fn check(&mut self) -> Result<()> {
        self.0.check()
    }
    fn activity(&mut self) -> Result<()> {
        self.0.activity()
    }
}

pub(crate) fn archive(
    plan: &mut Plan,
    limits: Limits,
    check: &mut impl Monitor,
) -> Result<(File, u64)> {
    if plan.items.is_empty() {
        return Err(ErrorCode::InvalidRequest);
    }
    let mut names = 0usize;
    let mut total = 0u64;
    for item in &plan.items {
        check.check()?;
        names = item
            .member
            .entry
            .name
            .encode_utf16()
            .count()
            .checked_add(1)
            .and_then(|n| n.checked_mul(2))
            .and_then(|n| names.checked_add(n))
            .ok_or(ErrorCode::Limit)?;
        total = total
            .checked_add(item.member.entry.size)
            .filter(|n| *n <= limits.input.min(limits.output))
            .ok_or(ErrorCode::Limit)?;
    }
    let capacity = plan
        .items
        .len()
        .checked_mul(96)
        .and_then(|n| n.checked_add(names))
        .and_then(|n| n.checked_add(128))
        .ok_or(ErrorCode::Limit)?;
    // 헤더 두 사본, 검증 목록과 스트림 배열을 기존 계획 예산에 함께 반영함.
    let metadata = capacity
        .checked_mul(3)
        .and_then(|n| n.checked_add(plan.items.len().checked_mul(256)?))
        .ok_or(ErrorCode::Limit)?;
    plan.reserve_metadata(metadata, check)?;
    let mut streams = Vec::new();
    streams
        .try_reserve_exact(plan.items.len())
        .map_err(|_| ErrorCode::Limit)?;
    let mut output = spool::temporary()?;
    write(&mut output, &[0; 32], check)?;
    let mut read = 0u64;
    for (index, source) in plan.items.iter().enumerate() {
        check.check()?;
        let length = source.member.entry.size;
        let mut input = crate::resources::completed(source.open(), check)?;
        if !empty(source) {
            let required = length
                .checked_add(packed_bound(length)?)
                .and_then(|n| n.checked_add(capacity as u64))
                .ok_or(ErrorCode::Limit)?;
            crate::space::ensure(&output, required)?;
            check.progress(Stage::Read, read, Some(total), index as u32)?;
            let mut snapshot = spool::Snapshot::capture_reporting(
                &mut input.file,
                length,
                &mut |step| match step {
                    spool::Step::Check => check.check(),
                    spool::Step::Activity => crate::resources::completed(Ok(()), check),
                    spool::Step::Read { bytes, .. } => {
                        check.progress(Stage::Read, read + bytes, Some(total), index as u32)
                    }
                },
            )?;
            if snapshot.len != length {
                return Err(ErrorCode::SourceChanged);
            }
            check.progress(
                Stage::Write,
                io(output.stream_position())? - 32,
                None,
                index as u32,
            )?;
            streams.push(pack(
                &mut snapshot.file,
                length,
                &mut output,
                index as u32,
                check,
            )?);
            read += length;
        }
        crate::resources::completed(Ok(()), check)?;
    }
    let at = io(output.stream_position())?;
    let header = header(plan, &streams, names, capacity, check)?;
    let mut start = [0; 32];
    start[..8].copy_from_slice(b"7z\xbc\xaf\x27\x1c\x00\x04");
    start[12..20].copy_from_slice(&at.checked_sub(32).ok_or(ErrorCode::Internal)?.to_le_bytes());
    start[20..28].copy_from_slice(&(header.0.len() as u64).to_le_bytes());
    start[28..32].copy_from_slice(&crc32(0, &header.0).to_le_bytes());
    let crc = crc32(0, &start[12..]);
    start[8..12].copy_from_slice(&crc.to_le_bytes());
    write(&mut output, &header.0, check)?;
    drop(header);
    let length = io(output.stream_position())?;
    check.check()?;
    io(output.seek(SeekFrom::Start(0)))?;
    write(&mut output, &start, check)?;
    check.progress(Stage::Verify, 0, None, plan.items.len() as u32)?;
    let catalog = crate::seven_zip::open(
        &mut output,
        length,
        Limits {
            input: length,
            ..limits
        },
        false,
        &mut Verify(check),
    )?;
    if catalog.members.len() != plan.items.len() {
        return Err(ErrorCode::Integrity);
    }
    for (actual, expected) in catalog.members.iter().zip(&plan.items) {
        let expected = &expected.member.entry;
        if actual.entry.name != expected.name
            || actual.entry.kind != expected.kind
            || actual.entry.size != expected.size
        {
            return Err(ErrorCode::Integrity);
        }
        crate::resources::completed(Ok(()), check)?;
    }
    drop(catalog);
    plan.verify(check)?;
    io(output.rewind())?;
    Ok((output, length))
}

#[cfg(test)]
#[path = "../../../tests/rust-archive-worker/seven_zip_create_fields.rs"]
mod tests;
