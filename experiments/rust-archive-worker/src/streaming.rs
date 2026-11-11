use crate::{
    archive::{self, Member, MetadataBudget},
    crypto,
    protocol::*,
    spool,
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};
use zeroize::Zeroizing;
use zlib_rs::{Inflate, InflateFlush, Status, crc32::crc32};

type Result<T> = std::result::Result<T, ErrorCode>;
const CHUNK: usize = 65536;
fn at(file: &mut File, offset: u64, bytes: &mut [u8]) -> Result<()> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| ErrorCode::Io)?;
    file.read_exact(bytes).map_err(|_| ErrorCode::Integrity)
}
fn bytes(file: &mut File, offset: u64, size: usize) -> Result<Vec<u8>> {
    let mut result = vec![0; size];
    at(file, offset, &mut result)?;
    Ok(result)
}
fn u16at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}
fn u32at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn u64at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).ok_or(ErrorCode::Integrity)
}
fn io<T>(v: std::io::Result<T>) -> Result<T> {
    v.map_err(crate::space::io_error)
}

pub(crate) struct Catalog {
    pub format: Format,
    pub members: Vec<Member>,
    pub offsets: Vec<u64>,
    pub data: File,
}
impl Catalog {
    pub fn open(
        source: &mut File,
        len: u64,
        limits: Limits,
        decode: bool,
        password: Option<&[u8]>,
        check: &mut impl FnMut() -> Result<()>,
    ) -> Result<Self> {
        let mut magic = [0; 6];
        let n = len.min(6) as usize;
        at(source, 0, &mut magic[..n])?;
        if magic.starts_with(b"7z\xbc\xaf\x27\x1c") {
            let bounded = limits.legacy();
            if len > bounded.input {
                return Err(ErrorCode::Limit);
            }
            let input = bytes(source, 0, len as usize)?;
            let catalog = archive::parse_with_password(&input, bounded, decode, password, check)?;
            let mut data = spool::temporary()?;
            let mut members = catalog.members;
            let mut offsets = Vec::new();
            for m in &mut members {
                offsets.push(io(data.stream_position())?);
                io(data.write_all(&m.data))?;
                m.data.clear();
                m.data.shrink_to_fit();
            }
            return Ok(Self {
                format: Format::SevenZip,
                members,
                offsets,
                data,
            });
        }
        if magic.starts_with(b"PK\x03\x04")
            || magic.starts_with(b"PK\x05\x06")
            || magic.starts_with(b"PK\x06\x06")
        {
            return zip(source, len, limits, decode, password, check);
        }
        if magic.starts_with(&[31, 139]) {
            let mut data = spool::temporary()?;
            io(source.rewind())?;
            let expanded = inflate(source, len, true, limits.output, &mut data, check)?.0;
            let mut header = [0; 512];
            if expanded >= 512 {
                at(&mut data, 0, &mut header)?;
                if matches!(&header[257..263], b"ustar\0" | b"ustar ") {
                    return tar(data, expanded, limits, Format::TarGzip, check);
                }
            }
            return Ok(Self {
                format: Format::Gzip,
                members: vec![Member {
                    entry: Entry {
                        entry_id: 0,
                        name: "GZip stream".to_owned(),
                        kind: EntryKind::File,
                        size: expanded,
                        compressed_size: len,
                        encrypted: false,
                    },
                    data: Vec::new(),
                }],
                offsets: vec![0],
                data,
            });
        }
        if len >= 512 {
            return tar(io(source.try_clone())?, len, limits, Format::Tar, check);
        }
        Err(ErrorCode::UnsupportedFormat)
    }
}

fn tar(
    mut data: File,
    len: u64,
    limits: Limits,
    format: Format,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Catalog> {
    let mut cursor = 0u64;
    let mut total = 0u64;
    let mut members = Vec::new();
    let mut offsets = Vec::new();
    let mut metadata = MetadataBudget::default();
    loop {
        check()?;
        let mut header = [0; 512];
        if add(cursor, 512)? > len {
            return Err(ErrorCode::Integrity);
        }
        at(&mut data, cursor, &mut header)?;
        if header.iter().all(|b| *b == 0) {
            if add(cursor, 1024)? > len || len % 512 != 0 {
                return Err(ErrorCode::Integrity);
            }
            let mut buffer = Zeroizing::new([0; CHUNK]);
            let mut remaining = len - cursor - 512;
            while remaining != 0 {
                check()?;
                let n = remaining.min(CHUNK as u64) as usize;
                io(data.read_exact(&mut buffer[..n]))?;
                if buffer[..n].iter().any(|b| *b != 0) {
                    return Err(ErrorCode::Integrity);
                }
                remaining -= n as u64;
            }
            break;
        }
        if members.len() >= limits.entries as usize {
            return Err(ErrorCode::Limit);
        }
        let sum = header
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    *b as u64
                }
            })
            .sum::<u64>();
        if sum != archive::octal(&header[148..156])? {
            return Err(ErrorCode::Integrity);
        }
        let mut name = archive::tar_string(&header[..100])?;
        if matches!(&header[257..263], b"ustar\0" | b"ustar ") {
            let prefix = archive::tar_string(&header[345..500])?;
            if !prefix.is_empty() {
                name = format!("{prefix}/{name}");
            }
        } else if header[257..263].iter().any(|b| *b != 0) {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let kind = match header[156] {
            0 | b'0' => EntryKind::File,
            b'5' => EntryKind::Directory,
            b'1' | b'2' => EntryKind::Link,
            b'x' | b'g' | b'L' | b'K' => return Err(ErrorCode::UnsupportedFeature),
            _ => EntryKind::Other,
        };
        let size = archive::octal(&header[124..136])?;
        if kind != EntryKind::File && size != 0 {
            return Err(ErrorCode::Integrity);
        }
        total = total
            .checked_add(size)
            .filter(|n| *n <= limits.output)
            .ok_or(ErrorCode::Limit)?;
        let start = add(cursor, 512)?;
        cursor = add(
            start,
            size.checked_add(511).ok_or(ErrorCode::Integrity)? / 512 * 512,
        )?;
        if cursor > len {
            return Err(ErrorCode::Integrity);
        }
        metadata.charge(&name)?;
        let name = archive::valid_name(&name, kind == EntryKind::Directory)?;
        offsets.push(start);
        members.push(Member {
            entry: Entry {
                entry_id: members.len() as u32,
                name,
                kind,
                size,
                compressed_size: size,
                encrypted: false,
            },
            data: Vec::new(),
        });
    }
    archive::validate_members(&members)?;
    Ok(Catalog {
        format,
        members,
        offsets,
        data,
    })
}

// ZIP 항목과 연결 GZip 스트림의 해제 총량을 제한함.
fn inflate(
    input: &mut impl Read,
    length: u64,
    gzip: bool,
    limit: u64,
    output: &mut File,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<(u64, u32)> {
    let mut remaining = length;
    let mut buffer = Zeroizing::new([0; CHUNK]);
    let mut plain = Zeroizing::new([0; CHUNK]);
    let mut have = 0;
    let mut used = 0;
    let mut total = 0u64;
    let mut crc = 0;
    loop {
        let mut stream = Inflate::new(gzip, if gzip { 31 } else { 15 });
        loop {
            check()?;
            if used == have && remaining != 0 {
                have = input
                    .read(&mut buffer[..remaining.min(CHUNK as u64) as usize])
                    .map_err(|_| ErrorCode::Io)?;
                if have == 0 {
                    return Err(ErrorCode::Integrity);
                }
                remaining -= have as u64;
                used = 0;
            }
            let a = stream.total_in();
            let b = stream.total_out();
            let status = stream
                .decompress(&buffer[used..have], &mut plain[..], InflateFlush::NoFlush)
                .map_err(|_| ErrorCode::Integrity)?;
            used += (stream.total_in() - a) as usize;
            let n = (stream.total_out() - b) as usize;
            total = total
                .checked_add(n as u64)
                .filter(|n| *n <= limit)
                .ok_or(ErrorCode::Limit)?;
            crc = crc32(crc, &plain[..n]);
            crate::space::ensure(output, n as u64)?;
            io(output.write_all(&plain[..n]))?;
            if status == Status::StreamEnd {
                break;
            }
            if a == stream.total_in() && b == stream.total_out() {
                return Err(ErrorCode::Integrity);
            }
        }
        if remaining == 0 && used == have {
            break;
        }
        if !gzip {
            return Err(ErrorCode::Integrity);
        }
    }
    Ok((total, crc))
}

struct Packed {
    offset: u64,
    size: u64,
    method: u16,
    crc: Option<u32>,
    flags: u16,
    verifier: u8,
    aes: bool,
}

fn extra(bytes: &[u8]) -> Result<(Option<archive::AesExtra>, Option<Vec<u8>>)> {
    let mut at = 0;
    let mut aes = Vec::new();
    let mut large = None;
    while at < bytes.len() {
        if bytes.len() - at < 4 {
            return Err(ErrorCode::Integrity);
        }
        let tag = u16at(bytes, at);
        let len = u16at(bytes, at + 2) as usize;
        let end = at + 4 + len;
        if end > bytes.len() {
            return Err(ErrorCode::Integrity);
        }
        if tag == 1 {
            if large.is_some() {
                return Err(ErrorCode::Integrity);
            }
            large = Some(bytes[at + 4..end].to_vec());
        } else {
            aes.extend_from_slice(&bytes[at..end]);
        }
        at = end;
    }
    Ok((archive::aes_extra(&aes)?, large))
}
fn large_value(value: u32, extra: &mut &[u8]) -> Result<u64> {
    if value != u32::MAX {
        return Ok(value as u64);
    }
    if extra.len() < 8 {
        return Err(ErrorCode::Integrity);
    }
    let result = u64at(extra, 0);
    *extra = &extra[8..];
    Ok(result)
}
fn expanded(value: u64, maximum: u64, large: u64) -> Result<u64> {
    if value == maximum || value == large {
        Ok(large)
    } else {
        Err(ErrorCode::Integrity)
    }
}
fn disk_start(value: u16, extra: &mut &[u8]) -> Result<u32> {
    if value != u16::MAX {
        return Ok(value as u32);
    }
    if extra.len() < 4 {
        return Err(ErrorCode::Integrity);
    }
    let result = u32at(extra, 0);
    *extra = &extra[4..];
    Ok(result)
}
fn local_sizes(size: u32, compressed: u32, extra: Option<&[u8]>) -> Result<(u64, u64)> {
    if let Some(extra) = extra {
        // 로컬 ZIP64 추가 필드에는 두 크기가 함께 들어감.
        if extra.len() < 16 {
            return Err(ErrorCode::Integrity);
        }
        Ok((
            expanded(size as u64, u32::MAX as u64, u64at(extra, 0))?,
            expanded(compressed as u64, u32::MAX as u64, u64at(extra, 8))?,
        ))
    } else if size == u32::MAX || compressed == u32::MAX {
        Err(ErrorCode::Integrity)
    } else {
        Ok((size as u64, compressed as u64))
    }
}
fn descriptor_size(
    bytes: &[u8],
    crc: u32,
    compressed: u64,
    size: u64,
    wide_required: bool,
) -> Result<usize> {
    // 일반 크기 헤더와 64비트 디스크립터를 함께 기록하는 도구도 있음.
    if bytes.len() >= 20
        && u32at(bytes, 0) == crc
        && u64at(bytes, 4) == compressed
        && u64at(bytes, 12) == size
    {
        return Ok(20);
    }
    if !wide_required
        && bytes.len() >= 12
        && u32at(bytes, 0) == crc
        && u32at(bytes, 4) as u64 == compressed
        && u32at(bytes, 8) as u64 == size
    {
        return Ok(12);
    }
    Err(ErrorCode::Integrity)
}
fn zip(
    source: &mut File,
    len: u64,
    limits: Limits,
    decode: bool,
    password: Option<&[u8]>,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Catalog> {
    if len < 22 {
        return Err(ErrorCode::Integrity);
    }
    let tail_at = len.saturating_sub(65557);
    let tail = bytes(source, tail_at, (len - tail_at) as usize)?;
    let end = (0..=tail.len() - 22)
        .rev()
        .find(|&i| {
            &tail[i..i + 4] == b"PK\x05\x06" && i + 22 + u16at(&tail, i + 20) as usize == tail.len()
        })
        .ok_or(ErrorCode::Integrity)?;
    let h = &tail[end..];
    let mut disk = u16at(h, 4) as u64;
    let mut central_disk = u16at(h, 6) as u64;
    let mut entries_on_disk = u16at(h, 8) as u64;
    let mut count = u16at(h, 10) as u64;
    let mut central_size = u32at(h, 12) as u64;
    let mut central = u32at(h, 16) as u64;
    let mut central_end = tail_at + end as u64;
    let required_zip64 = [disk, central_disk, entries_on_disk, count].contains(&(u16::MAX as u64))
        || central_size == u32::MAX as u64
        || central == u32::MAX as u64;
    let locator = if central_end >= 20 {
        bytes(source, central_end - 20, 20)?
    } else {
        Vec::new()
    };
    // 중앙 디렉터리 주석의 동일 서명을 locator로 오인하지 않음
    let has_locator =
        locator.starts_with(b"PK\x06\x07") && add(central, central_size)? != central_end;
    if required_zip64 || has_locator {
        if locator.len() != 20 {
            return Err(ErrorCode::Integrity);
        }
        if &locator[..4] != b"PK\x06\x07" || u32at(&locator, 4) != 0 || u32at(&locator, 16) != 1 {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let offset = u64at(&locator, 8);
        if add(offset, 56)? > central_end - 20 {
            return Err(ErrorCode::Integrity);
        }
        let large = bytes(source, offset, 56)?;
        if &large[..4] != b"PK\x06\x06"
            || u64at(&large, 4) < 44
            || add(offset, add(12, u64at(&large, 4))?)? != central_end - 20
        {
            return Err(ErrorCode::Integrity);
        }
        // 각 필드의 확장값을 해석한 뒤 단일 디스크 조건을 검사함.
        disk = expanded(disk, u16::MAX as u64, u32at(&large, 16) as u64)?;
        central_disk = expanded(central_disk, u16::MAX as u64, u32at(&large, 20) as u64)?;
        entries_on_disk = expanded(entries_on_disk, u16::MAX as u64, u64at(&large, 24))?;
        count = expanded(count, u16::MAX as u64, u64at(&large, 32))?;
        central_size = expanded(central_size, u32::MAX as u64, u64at(&large, 40))?;
        central = expanded(central, u32::MAX as u64, u64at(&large, 48))?;
        central_end = offset;
    }
    if disk != 0 || central_disk != 0 || entries_on_disk != count {
        return Err(ErrorCode::UnsupportedFeature);
    }
    if count > limits.entries as u64 {
        return Err(ErrorCode::Limit);
    }
    if add(central, central_size)? != central_end {
        return Err(ErrorCode::Integrity);
    }
    let mut cursor = central;
    let mut total = 0u64;
    let mut metadata = MetadataBudget::default();
    let mut members = Vec::new();
    let mut packed = Vec::new();
    let mut spans = Vec::new();
    for id in 0..count {
        check()?;
        if add(cursor, 46)? > central_end {
            return Err(ErrorCode::Integrity);
        }
        let h = bytes(source, cursor, 46)?;
        if &h[..4] != b"PK\x01\x02" {
            return Err(ErrorCode::Integrity);
        }
        let flags = u16at(&h, 8);
        let method = u16at(&h, 10);
        if flags & !0x080f != 0 || !matches!(method, 0 | 8 | 99) {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let crc = u32at(&h, 16);
        let (nn, ne, nc) = (
            u16at(&h, 28) as u64,
            u16at(&h, 30) as u64,
            u16at(&h, 32) as u64,
        );
        let next = add(cursor, 46 + nn + ne + nc)?;
        if next > central_end {
            return Err(ErrorCode::Integrity);
        }
        if nn > 4096 {
            return Err(ErrorCode::UnsafePath);
        }
        let raw = bytes(source, cursor + 46, nn as usize)?;
        if flags & 0x0800 == 0 && !raw.is_ascii() {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let name = std::str::from_utf8(&raw).map_err(|_| ErrorCode::Integrity)?;
        let (aes, large) = extra(&bytes(source, cursor + 46 + nn, ne as usize)?)?;
        let mut large = large.as_deref().unwrap_or(&[]);
        let size = large_value(u32at(&h, 24), &mut large)?;
        let compressed = large_value(u32at(&h, 20), &mut large)?;
        let local = large_value(u32at(&h, 42), &mut large)?;
        if disk_start(u16at(&h, 34), &mut large)? != 0 {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let encrypted = flags & 1 != 0;
        if (method == 99) != aes.is_some()
            || (method == 99 && !encrypted)
            || (aes.is_some_and(|a| a.version == 2) && crc != 0)
        {
            return Err(ErrorCode::Integrity);
        }
        let overhead = if !encrypted {
            0
        } else if aes.is_some() {
            28
        } else {
            12
        };
        compressed
            .checked_sub(overhead)
            .ok_or(ErrorCode::Integrity)?;
        total = total
            .checked_add(size)
            .filter(|n| *n <= limits.output)
            .ok_or(ErrorCode::Limit)?;
        let mode = u32at(&h, 38) >> 16;
        let kind = match mode & 0xf000 {
            0xa000 => EntryKind::Link,
            0x4000 => EntryKind::Directory,
            0 | 0x8000 => {
                if name.ends_with('/') {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                }
            }
            _ => EntryKind::Other,
        };
        if kind == EntryKind::Directory && size != 0 {
            return Err(ErrorCode::Integrity);
        }
        metadata.charge(name)?;
        let name = archive::valid_name(name, kind == EntryKind::Directory)?;
        if add(local, 30)? > central {
            return Err(ErrorCode::Integrity);
        }
        let lh = bytes(source, local, 30)?;
        if &lh[..4] != b"PK\x03\x04"
            || u16at(&lh, 6) != flags
            || u16at(&lh, 8) != method
            || u16at(&lh, 10) != u16at(&h, 12)
        {
            return Err(ErrorCode::Integrity);
        }
        let (ln, le) = (u16at(&lh, 26) as u64, u16at(&lh, 28) as u64);
        let start = add(local, 30 + ln + le)?;
        if start > central || ln != nn {
            return Err(ErrorCode::Integrity);
        }
        if bytes(source, local + 30, ln as usize)? != raw {
            return Err(ErrorCode::Integrity);
        }
        let (local_aes, local_large) = extra(&bytes(source, local + 30 + ln, le as usize)?)?;
        if local_aes != aes || local_large.as_ref().is_some_and(|values| values.len() < 16) {
            return Err(ErrorCode::Integrity);
        }
        let size_zip64 = u32at(&h, 20) == u32::MAX
            || u32at(&h, 24) == u32::MAX
            || u32at(&lh, 18) == u32::MAX
            || u32at(&lh, 22) == u32::MAX
            || local_large.is_some();
        if flags & 8 == 0 {
            let (local_size, local_compressed) =
                local_sizes(u32at(&lh, 22), u32at(&lh, 18), local_large.as_deref())?;
            if u32at(&lh, 14) != crc || local_size != size || local_compressed != compressed {
                return Err(ErrorCode::Integrity);
            }
        }
        let mut stop = add(start, compressed)?;
        if stop > central {
            return Err(ErrorCode::Integrity);
        }
        if flags & 8 != 0 {
            let descriptor_len = if size_zip64 { 20 } else { 12 };
            if add(stop, descriptor_len)? > central {
                return Err(ErrorCode::Integrity);
            }
            let signature = bytes(source, stop, 4)?;
            if signature == b"PK\x07\x08" {
                stop = add(stop, 4)?;
            }
            let d = bytes(source, stop, (central - stop).min(20) as usize)?;
            let descriptor_len = descriptor_size(&d, crc, compressed, size, size_zip64)?;
            stop = add(stop, descriptor_len as u64)?;
        }
        spans.push((local, stop));
        packed.push(Packed {
            offset: start,
            size: compressed,
            method: aes.map_or(method, |a| a.method),
            crc: if aes.is_some_and(|a| a.version == 2) {
                None
            } else {
                Some(crc)
            },
            flags,
            verifier: if flags & 8 != 0 {
                (u16at(&h, 12) >> 8) as u8
            } else {
                (crc >> 24) as u8
            },
            aes: aes.is_some(),
        });
        members.push(Member {
            entry: Entry {
                entry_id: id as u32,
                name,
                kind,
                size,
                compressed_size: compressed,
                encrypted,
            },
            data: Vec::new(),
        });
        cursor = next;
    }
    if cursor != central_end {
        return Err(ErrorCode::Integrity);
    }
    spans.sort_unstable();
    let mut next = 0;
    for (a, b) in spans {
        if a != next {
            return Err(ErrorCode::Integrity);
        }
        next = b;
    }
    if next != central {
        return Err(ErrorCode::Integrity);
    }
    archive::validate_members(&members)?;
    let mut data = spool::temporary()?;
    if decode {
        crate::space::ensure(&data, total)?;
    }
    let mut offsets = Vec::new();
    for (member, packed) in members.iter().zip(packed) {
        check()?;
        offsets.push(io(data.stream_position())?);
        if !decode {
            continue;
        }
        let (decoder, start, length) = if packed.flags & 1 != 0 {
            crypto::Decryptor::prepare(
                source,
                packed.offset,
                packed.size,
                password.ok_or(ErrorCode::WrongPassword)?,
                packed.aes,
                packed.verifier,
                check,
            )?
        } else {
            (crypto::Decryptor::Plain, packed.offset, packed.size)
        };
        io(source.seek(SeekFrom::Start(start)))?;
        let mut reader = Decoded {
            reader: source.take(length),
            decoder,
        };
        let (size, crc) = if packed.method == 8 {
            inflate(
                &mut reader,
                length,
                false,
                member.entry.size,
                &mut data,
                check,
            )?
        } else {
            let mut buffer = Zeroizing::new([0; CHUNK]);
            let mut total = 0u64;
            let mut crc = 0;
            loop {
                check()?;
                let n = io(reader.read(&mut buffer[..]))?;
                if n == 0 {
                    break;
                }
                total = total
                    .checked_add(n as u64)
                    .filter(|n| *n <= member.entry.size)
                    .ok_or(ErrorCode::Integrity)?;
                crc = crc32(crc, &buffer[..n]);
                io(data.write_all(&buffer[..n]))?;
            }
            (total, crc)
        };
        if size != member.entry.size || packed.crc.is_some_and(|c| c != crc) {
            return Err(ErrorCode::Integrity);
        }
    }
    Ok(Catalog {
        format: Format::Zip,
        members,
        offsets,
        data,
    })
}
struct Decoded<R> {
    reader: R,
    decoder: crypto::Decryptor,
}
impl<R: Read> Read for Decoded<R> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let n = self.reader.read(output)?;
        self.decoder.apply(&mut output[..n]);
        Ok(n)
    }
}

#[cfg(test)]
#[path = "../../../tests/rust-archive-worker/streaming_bounds.rs"]
mod bounds;
