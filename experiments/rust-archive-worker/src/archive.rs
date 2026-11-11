use crate::protocol::*;
use std::collections::BTreeMap;
use zlib_rs::{Deflate, DeflateFlush, Inflate, InflateFlush, Status, crc32::crc32};

type Result<T> = std::result::Result<T, ErrorCode>;
#[derive(Debug)]
pub struct Member {
    pub entry: Entry,
    pub data: Vec<u8>,
}
pub struct Catalog {
    pub format: Format,
    pub members: Vec<Member>,
}
fn part(b: &[u8], at: usize, n: usize) -> Result<&[u8]> {
    b.get(at..at.checked_add(n).ok_or(ErrorCode::Integrity)?)
        .ok_or(ErrorCode::Integrity)
}
fn u16at(b: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(part(b, at, 2)?.try_into().unwrap()))
}
fn u32at(b: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(part(b, at, 4)?.try_into().unwrap()))
}

pub fn valid_name(name: &str, directory: bool) -> Result<String> {
    let name = if directory {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        name
    };
    if name.is_empty()
        || name.len() > 4096
        || name.starts_with('/')
        || name.contains('\\')
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err(ErrorCode::UnsafePath);
    }
    for component in name.split('/') {
        if component.is_empty()
            || matches!(component, "." | "..")
            || component.ends_with(['.', ' '])
            || component.len() > 255
        {
            return Err(ErrorCode::UnsafePath);
        }
        let stem = component.split('.').next().unwrap().to_ascii_uppercase();
        if matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|p| {
            stem.strip_prefix(p).is_some_and(|n| {
                matches!(
                    n,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        }) {
            return Err(ErrorCode::UnsafePath);
        }
    }
    Ok(name.to_owned())
}
pub fn validate_members(members: &[Member]) -> Result<()> {
    let mut names = BTreeMap::<String, bool>::new();
    for member in members {
        let directory = member.entry.kind == EntryKind::Directory;
        let name = valid_name(&member.entry.name, directory)?.to_lowercase();
        if names.insert(name, directory).is_some() {
            return Err(ErrorCode::Conflict);
        }
    }
    for name in names.keys() {
        let mut parent = name.as_str();
        while let Some((p, _)) = parent.rsplit_once('/') {
            if names.get(p) == Some(&false) {
                return Err(ErrorCode::Conflict);
            }
            parent = p;
        }
    }
    Ok(())
}
fn bounded_append(output: &mut Vec<u8>, data: &[u8], limit: u64) -> Result<()> {
    if (output.len() as u64)
        .checked_add(data.len() as u64)
        .is_none_or(|n| n > limit)
    {
        return Err(ErrorCode::Limit);
    }
    output
        .try_reserve(data.len())
        .map_err(|_| ErrorCode::Limit)?;
    output.extend_from_slice(data);
    Ok(())
}
fn inflate(
    input: &[u8],
    gzip: bool,
    limit: u64,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<(Vec<u8>, usize)> {
    let mut stream = Inflate::new(gzip, if gzip { 31 } else { 15 });
    let mut out = Vec::new();
    let mut buffer = [0; 65536];
    loop {
        check()?;
        let before_in = stream.total_in();
        let before_out = stream.total_out();
        let status = stream
            .decompress(
                &input[before_in as usize..],
                &mut buffer,
                InflateFlush::NoFlush,
            )
            .map_err(|_| ErrorCode::Integrity)?;
        let written = (stream.total_out() - before_out) as usize;
        bounded_append(&mut out, &buffer[..written], limit)?;
        if status == Status::StreamEnd {
            return Ok((out, stream.total_in() as usize));
        }
        if stream.total_in() == before_in && stream.total_out() == before_out {
            return Err(ErrorCode::Integrity);
        }
    }
}
fn gzip(input: &[u8], limits: Limits, check: &mut impl FnMut() -> Result<()>) -> Result<Vec<u8>> {
    let mut at = 0;
    let mut result = Vec::new();
    while at < input.len() {
        if !input[at..].starts_with(&[31, 139, 8]) {
            return Err(ErrorCode::Integrity);
        }
        let (out, used) = inflate(
            &input[at..],
            true,
            limits.output - result.len() as u64,
            check,
        )?;
        if used == 0 {
            return Err(ErrorCode::Integrity);
        }
        bounded_append(&mut result, &out, limits.output)?;
        at += used;
    }
    Ok(result)
}
fn tar_string(b: &[u8]) -> Result<String> {
    let n = b.iter().position(|v| *v == 0).unwrap_or(b.len());
    if b[n..].iter().any(|v| *v != 0) {
        return Err(ErrorCode::Integrity);
    }
    std::str::from_utf8(&b[..n])
        .map(str::to_owned)
        .map_err(|_| ErrorCode::UnsupportedFeature)
}
fn octal(b: &[u8]) -> Result<u64> {
    if b.first().is_some_and(|x| x & 128 != 0) {
        return Err(ErrorCode::UnsupportedFeature);
    }
    let text = std::str::from_utf8(b)
        .map_err(|_| ErrorCode::Integrity)?
        .trim_matches(['\0', ' ']);
    if text.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(text, 8).map_err(|_| ErrorCode::Integrity)
}
fn tar(
    input: &[u8],
    limits: Limits,
    decode: bool,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Vec<Member>> {
    let mut at = 0;
    let mut members = Vec::new();
    let mut total = 0u64;
    loop {
        check()?;
        let header = part(input, at, 512)?;
        if header.iter().all(|b| *b == 0) {
            if part(input, at + 512, 512)?.iter().any(|b| *b != 0)
                || input[at..].iter().any(|b| *b != 0)
                || input.len() % 512 != 0
            {
                return Err(ErrorCode::Integrity);
            }
            break;
        }
        if members.len() >= limits.entries as usize {
            return Err(ErrorCode::Limit);
        }
        let expected = octal(&header[148..156])?;
        let checksum = header
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32u64
                } else {
                    *b as u64
                }
            })
            .sum::<u64>();
        if checksum != expected {
            return Err(ErrorCode::Integrity);
        }
        let mut name = tar_string(&header[..100])?;
        if &header[257..263] == b"ustar\0" || &header[257..263] == b"ustar " {
            let prefix = tar_string(&header[345..500])?;
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
        let size = octal(&header[124..136])?;
        if kind != EntryKind::File && size != 0 {
            return Err(ErrorCode::Integrity);
        }
        total = total
            .checked_add(size)
            .filter(|n| *n <= limits.output)
            .ok_or(ErrorCode::Limit)?;
        let size = usize::try_from(size).map_err(|_| ErrorCode::Limit)?;
        let bytes = part(input, at + 512, size)?;
        let name = valid_name(&name, kind == EntryKind::Directory)?;
        let data = if decode { bytes.to_vec() } else { Vec::new() };
        members.push(Member {
            entry: Entry {
                entry_id: members.len() as u32,
                name,
                kind,
                size: size as u64,
                compressed_size: size as u64,
                encrypted: false,
            },
            data,
        });
        at = at
            .checked_add(512)
            .and_then(|n| n.checked_add(size.div_ceil(512) * 512))
            .ok_or(ErrorCode::Integrity)?;
    }
    validate_members(&members)?;
    Ok(members)
}
fn zip(
    input: &[u8],
    limits: Limits,
    decode: bool,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Vec<Member>> {
    if input.len() < 22 {
        return Err(ErrorCode::Integrity);
    }
    let end = (input.len().saturating_sub(65557)..=input.len() - 22)
        .rev()
        .find(|&i| {
            input.get(i..i + 4) == Some(b"PK\x05\x06")
                && u16at(input, i + 20)
                    .ok()
                    .is_some_and(|n| i + 22 + n as usize == input.len())
        })
        .ok_or(ErrorCode::Integrity)?;
    let count = u16at(input, end + 10)? as usize;
    if u16at(input, end + 4)? != 0
        || u16at(input, end + 6)? != 0
        || count != u16at(input, end + 8)? as usize
    {
        return Err(ErrorCode::UnsupportedFeature);
    }
    if count == 65535 || u32at(input, end + 12)? == u32::MAX || u32at(input, end + 16)? == u32::MAX
    {
        return Err(ErrorCode::UnsupportedFeature);
    }
    if count > limits.entries as usize {
        return Err(ErrorCode::Limit);
    }
    let central = u32at(input, end + 16)? as usize;
    if central.checked_add(u32at(input, end + 12)? as usize) != Some(end) {
        return Err(ErrorCode::Integrity);
    }
    let mut at = central;
    let mut members = Vec::new();
    let mut spans = Vec::new();
    let mut total = 0u64;
    for id in 0..count {
        check()?;
        let h = part(input, at, 46)?;
        if &h[..4] != b"PK\x01\x02" {
            return Err(ErrorCode::Integrity);
        }
        let flags = u16at(h, 8)?;
        let method = u16at(h, 10)?;
        if flags & !0x080f != 0 || !matches!(method, 0 | 8) || u16at(h, 34)? != 0 {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let crc = u32at(h, 16)?;
        let compressed = u32at(h, 20)? as usize;
        let size = u32at(h, 24)? as usize;
        if compressed == u32::MAX as usize || size == u32::MAX as usize {
            return Err(ErrorCode::UnsupportedFeature);
        }
        total = total
            .checked_add(size as u64)
            .filter(|n| *n <= limits.output)
            .ok_or(ErrorCode::Limit)?;
        let name_len = u16at(h, 28)? as usize;
        let extra_len = u16at(h, 30)? as usize;
        let comment_len = u16at(h, 32)? as usize;
        let raw = part(input, at + 46, name_len)?;
        if flags & 0x0800 == 0 && !raw.is_ascii() {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let name = std::str::from_utf8(raw).map_err(|_| ErrorCode::Integrity)?;
        let mode = u32at(h, 38)? >> 16;
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
        let name = valid_name(name, kind == EntryKind::Directory)?;
        let local = u32at(h, 42)? as usize;
        let lh = part(input, local, 30)?;
        if &lh[..4] != b"PK\x03\x04" || u16at(lh, 6)? != flags || u16at(lh, 8)? != method {
            return Err(ErrorCode::Integrity);
        }
        let ln = u16at(lh, 26)? as usize;
        let le = u16at(lh, 28)? as usize;
        if part(input, local + 30, ln)? != raw {
            return Err(ErrorCode::Integrity);
        }
        if flags & 8 == 0
            && (u32at(lh, 14)? != crc
                || u32at(lh, 18)? as usize != compressed
                || u32at(lh, 22)? as usize != size)
        {
            return Err(ErrorCode::Integrity);
        }
        let start = local
            .checked_add(30 + ln + le)
            .ok_or(ErrorCode::Integrity)?;
        let packed = part(input, start, compressed)?;
        let mut stop = start + compressed;
        if flags & 8 != 0 {
            if input.get(stop..stop + 4) == Some(b"PK\x07\x08") {
                stop += 4;
            }
            if u32at(input, stop)? != crc
                || u32at(input, stop + 4)? as usize != compressed
                || u32at(input, stop + 8)? as usize != size
            {
                return Err(ErrorCode::Integrity);
            }
            stop += 12;
        }
        if stop > central {
            return Err(ErrorCode::Integrity);
        }
        spans.push((local, stop));
        let encrypted = flags & 1 != 0;
        let data = if decode && !encrypted {
            let bytes = if method == 0 {
                if compressed != size {
                    return Err(ErrorCode::Integrity);
                }
                packed.to_vec()
            } else {
                let (out, used) = inflate(packed, false, size as u64, check)?;
                if used != compressed {
                    return Err(ErrorCode::Integrity);
                }
                out
            };
            if bytes.len() != size || crc32(0, &bytes) != crc {
                return Err(ErrorCode::Integrity);
            }
            bytes
        } else {
            Vec::new()
        };
        if kind == EntryKind::Directory && size != 0 {
            return Err(ErrorCode::Integrity);
        }
        members.push(Member {
            entry: Entry {
                entry_id: id as u32,
                name,
                kind,
                size: size as u64,
                compressed_size: compressed as u64,
                encrypted,
            },
            data,
        });
        at = at
            .checked_add(46 + name_len + extra_len + comment_len)
            .ok_or(ErrorCode::Integrity)?;
    }
    if at != end {
        return Err(ErrorCode::Integrity);
    }
    spans.sort_unstable();
    let mut next = 0;
    for (start, stop) in spans {
        if start != next {
            return Err(ErrorCode::Integrity);
        }
        next = stop;
    }
    if next != central {
        return Err(ErrorCode::Integrity);
    }
    validate_members(&members)?;
    Ok(members)
}
pub fn parse(
    input: &[u8],
    limits: Limits,
    decode: bool,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Catalog> {
    if !limits.valid() || input.len() as u64 > limits.input {
        return Err(ErrorCode::Limit);
    }
    if input.starts_with(b"PK\x03\x04") || input.starts_with(b"PK\x05\x06") {
        return Ok(Catalog {
            format: Format::Zip,
            members: zip(input, limits, decode, check)?,
        });
    }
    if input.starts_with(&[31, 139]) {
        let bytes = gzip(input, limits, check)?;
        if bytes.len() >= 512
            && (bytes[257..262] == *b"ustar" || bytes[..512].iter().all(|b| *b == 0))
        {
            return Ok(Catalog {
                format: Format::TarGzip,
                members: tar(&bytes, limits, decode, check)?,
            });
        }
        let len = bytes.len() as u64;
        return Ok(Catalog {
            format: Format::Gzip,
            members: vec![Member {
                entry: Entry {
                    entry_id: 0,
                    name: "GZip stream".to_owned(),
                    kind: EntryKind::File,
                    size: len,
                    compressed_size: input.len() as u64,
                    encrypted: false,
                },
                data: if decode { bytes } else { Vec::new() },
            }],
        });
    }
    if input.len() >= 512 {
        return Ok(Catalog {
            format: Format::Tar,
            members: tar(input, limits, decode, check)?,
        });
    }
    Err(ErrorCode::UnsupportedFormat)
}
fn deflate(input: &[u8], limit: u64, check: &mut impl FnMut() -> Result<()>) -> Result<Vec<u8>> {
    let mut stream = Deflate::new(6, false, 15);
    let mut output = Vec::new();
    let mut buffer = [0; 65536];
    loop {
        check()?;
        let a = stream.total_in();
        let b = stream.total_out();
        let status = stream
            .compress(&input[a as usize..], &mut buffer, DeflateFlush::Finish)
            .map_err(|_| ErrorCode::Internal)?;
        bounded_append(
            &mut output,
            &buffer[..(stream.total_out() - b) as usize],
            limit,
        )?;
        if status == Status::StreamEnd {
            return Ok(output);
        }
        if a == stream.total_in() && b == stream.total_out() {
            return Err(ErrorCode::Internal);
        }
    }
}
fn put16(v: &mut Vec<u8>, n: u16) {
    v.extend(n.to_le_bytes());
}
fn put32(v: &mut Vec<u8>, n: u32) {
    v.extend(n.to_le_bytes());
}
pub fn create_zip(
    members: &[Member],
    method: Method,
    limits: Limits,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Vec<u8>> {
    if members.len() > limits.entries as usize || members.len() >= 65535 {
        return Err(ErrorCode::Limit);
    }
    validate_members(members)?;
    let mut out = Vec::new();
    let mut cd = Vec::new();
    let mut total = 0u64;
    for m in members {
        check()?;
        total = total
            .checked_add(m.data.len() as u64)
            .filter(|n| *n <= limits.output)
            .ok_or(ErrorCode::Limit)?;
        if m.entry.kind != EntryKind::File {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let name = m.entry.name.as_bytes();
        let crc = crc32(0, &m.data);
        let offset = out.len() as u32;
        let packed = match method {
            Method::Store => m.data.clone(),
            Method::Deflate => deflate(&m.data, limits.output, check)?,
        };
        let method = match method {
            Method::Store => 0,
            Method::Deflate => 8,
        };
        out.extend(b"PK\x03\x04");
        put16(&mut out, 20);
        put16(&mut out, 0x0800);
        put16(&mut out, method);
        put16(&mut out, 0);
        put16(&mut out, 33);
        put32(&mut out, crc);
        put32(&mut out, packed.len() as u32);
        put32(&mut out, m.data.len() as u32);
        put16(&mut out, name.len() as u16);
        put16(&mut out, 0);
        out.extend(name);
        bounded_append(&mut out, &packed, limits.output)?;
        cd.extend(b"PK\x01\x02");
        put16(&mut cd, 0x0314);
        put16(&mut cd, 20);
        put16(&mut cd, 0x0800);
        put16(&mut cd, method);
        put16(&mut cd, 0);
        put16(&mut cd, 33);
        put32(&mut cd, crc);
        put32(&mut cd, packed.len() as u32);
        put32(&mut cd, m.data.len() as u32);
        put16(&mut cd, name.len() as u16);
        put16(&mut cd, 0);
        put16(&mut cd, 0);
        put16(&mut cd, 0);
        put16(&mut cd, 0);
        put32(&mut cd, 0o100600 << 16);
        put32(&mut cd, offset);
        cd.extend(name);
    }
    let offset = out.len();
    bounded_append(&mut out, &cd, limits.output)?;
    let mut end = Vec::new();
    end.extend(b"PK\x05\x06");
    put16(&mut end, 0);
    put16(&mut end, 0);
    put16(&mut end, members.len() as u16);
    put16(&mut end, members.len() as u16);
    put32(&mut end, cd.len() as u32);
    put32(&mut end, offset as u32);
    put16(&mut end, 0);
    bounded_append(&mut out, &end, limits.output)?;
    Ok(out)
}
