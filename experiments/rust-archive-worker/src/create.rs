use crate::{protocol::*, spool};
use std::{
    fs::File,
    io::{Read, Seek, Write},
};
use zeroize::Zeroizing;
use zlib_rs::{Deflate, DeflateFlush, Status, crc32::crc32};

type Result<T> = std::result::Result<T, ErrorCode>;
fn io<T>(result: std::io::Result<T>) -> Result<T> {
    result.map_err(crate::space::io_error)
}
fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend(value.to_le_bytes());
}
fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_le_bytes());
}
fn put64(out: &mut Vec<u8>, value: u64) {
    out.extend(value.to_le_bytes());
}

fn zip64_extra(size: u64, packed: u64, offset: Option<u64>) -> Vec<u8> {
    let mut values = Vec::new();
    if size >= u32::MAX as u64 || packed >= u32::MAX as u64 {
        put64(&mut values, size);
        put64(&mut values, packed);
    }
    if let Some(offset) = offset.filter(|n| *n >= u32::MAX as u64) {
        put64(&mut values, offset);
    }
    let mut extra = Vec::new();
    if !values.is_empty() {
        put16(&mut extra, 1);
        put16(&mut extra, values.len() as u16);
        extra.extend(values);
    }
    extra
}
fn pack(
    input: &mut File,
    length: u64,
    method: Method,
    output: &mut File,
    check: &mut impl crate::resources::Monitor,
) -> Result<(u64, u32)> {
    io(input.rewind())?;
    let mut input = input.take(length);
    let mut buffer = Zeroizing::new([0; 65536]);
    let mut encoded = Zeroizing::new([0; 65536]);
    let mut crc = 0;
    let mut size = 0u64;
    if method == Method::Store {
        loop {
            check.check()?;
            let n = io(input.read(&mut buffer[..]))?;
            if n == 0 {
                break;
            }
            check.activity()?;
            crate::space::ensure(output, n as u64)?;
            crc = crc32(crc, &buffer[..n]);
            io(output.write_all(&buffer[..n]))?;
            size += n as u64;
        }
        if size != length {
            return Err(ErrorCode::SourceChanged);
        }
        return Ok((size, crc));
    }
    let mut stream = Deflate::new(6, false, 15);
    loop {
        check.check()?;
        let n = io(input.read(&mut buffer[..]))?;
        crc = crc32(crc, &buffer[..n]);
        let mut used = 0;
        loop {
            check.check()?;
            let a = stream.total_in();
            let b = stream.total_out();
            let status = stream
                .compress(
                    &buffer[used..n],
                    &mut encoded[..],
                    if n == 0 {
                        DeflateFlush::Finish
                    } else {
                        DeflateFlush::NoFlush
                    },
                )
                .map_err(|_| ErrorCode::Internal)?;
            used += (stream.total_in() - a) as usize;
            let written = (stream.total_out() - b) as usize;
            crate::space::ensure(output, written as u64)?;
            io(output.write_all(&encoded[..written]))?;
            if used != 0 || written != 0 {
                check.activity()?;
            }
            size = size.checked_add(written as u64).ok_or(ErrorCode::Limit)?;
            if status == Status::StreamEnd {
                if stream.total_in() != length {
                    return Err(ErrorCode::SourceChanged);
                }
                return Ok((size, crc));
            }
            if used == n && n != 0 {
                break;
            }
            if a == stream.total_in() && b == stream.total_out() {
                return Err(ErrorCode::Internal);
            }
        }
    }
}
pub(crate) use crate::create_sources::Plan;
pub(crate) fn plan(
    sources: &[Source],
    limits: Limits,
    check: &mut impl crate::resources::Monitor,
) -> Result<Plan> {
    Plan::new(sources, limits, check)
}
pub(crate) fn zip(
    plan: &Plan,
    method: Method,
    limits: Limits,
    password: Option<&[u8]>,
    check: &mut impl crate::resources::Monitor,
) -> Result<(File, u64)> {
    let mut output = spool::temporary()?;
    let mut central = Vec::new();
    let mut total = 0u64;
    let mut have_zip64 = false;
    for source in &plan.items {
        let member = &source.member;
        let directory = member.entry.kind == EntryKind::Directory;
        let method = if directory { Method::Store } else { method };
        let password = if directory { None } else { password };
        check.check()?;
        let remaining = limits
            .input
            .min(limits.output)
            .checked_sub(total)
            .ok_or(ErrorCode::Limit)?;
        let mut input = source.open()?;
        let mut snapshot = if directory {
            None
        } else {
            Some(spool::Snapshot::capture(&mut input.file, remaining, check)?)
        };
        let length = snapshot.as_ref().map_or(0, |s| s.len);
        total = total
            .checked_add(length)
            .filter(|n| *n <= limits.output)
            .ok_or(ErrorCode::Limit)?;
        // 압축 결과의 보수적 상한과 ZIP에 복사할 공간을 함께 확인함.
        let bound = length
            .checked_add(length / 16383 * 5)
            .and_then(|n| n.checked_add(131072))
            .ok_or(ErrorCode::Limit)?;
        crate::space::ensure(&output, bound.checked_mul(2).ok_or(ErrorCode::Limit)?)?;
        let mut packed = spool::temporary()?;
        let (packed_size, crc) = if let Some(snapshot) = snapshot.as_mut() {
            pack(&mut snapshot.file, length, method, &mut packed, check)?
        } else {
            (0, 0)
        };
        let packed_size = packed_size
            .checked_add(if password.is_some() { 28 } else { 0 })
            .ok_or(ErrorCode::Limit)?;
        let offset = io(output.stream_position())?;
        let zip_name = if directory {
            format!("{}/", member.entry.name)
        } else {
            member.entry.name.clone()
        };
        let name = zip_name.as_bytes();
        let real_method = if method == Method::Store { 0 } else { 8 };
        let flags = if password.is_some() { 0x0801 } else { 0x0800 };
        let wide_sizes = length >= u32::MAX as u64 || packed_size >= u32::MAX as u64;
        let wide_offset = offset >= u32::MAX as u64;
        have_zip64 |= wide_sizes || wide_offset;
        let version = if password.is_some() {
            51
        } else if wide_sizes || wide_offset {
            45
        } else {
            20
        };
        let method = if password.is_some() { 99 } else { real_method };
        let crc = if password.is_some() { 0 } else { crc };
        let mut aes = Vec::new();
        if password.is_some() {
            put16(&mut aes, 0x9901);
            put16(&mut aes, 7);
            put16(&mut aes, 2);
            aes.extend(b"AE");
            aes.push(3);
            put16(&mut aes, real_method);
        }
        let mut extra = zip64_extra(length, packed_size, None);
        extra.extend(&aes);
        let mut header = Vec::new();
        header.extend(b"PK\x03\x04");
        for v in [version, flags, method, 0, 33] {
            put16(&mut header, v);
        }
        put32(&mut header, crc);
        put32(
            &mut header,
            if wide_sizes {
                u32::MAX
            } else {
                packed_size as u32
            },
        );
        put32(
            &mut header,
            if wide_sizes { u32::MAX } else { length as u32 },
        );
        put16(&mut header, name.len() as u16);
        put16(&mut header, extra.len() as u16);
        header.extend(name);
        header.extend(extra);
        io(output.write_all(&header))?;
        check.activity()?;
        io(packed.rewind())?;
        if let Some(password) = password {
            crate::crypto::aes_encrypt_file(
                &mut packed,
                packed_size - 28,
                &mut output,
                password,
                check,
            )?;
        } else {
            let mut buffer = Zeroizing::new([0; 65536]);
            loop {
                check.check()?;
                let n = io(packed.read(&mut buffer[..]))?;
                if n == 0 {
                    break;
                }
                crate::space::ensure(&output, n as u64)?;
                io(output.write_all(&buffer[..n]))?;
                check.activity()?;
            }
        }
        let mut extra = zip64_extra(length, packed_size, Some(offset));
        extra.extend(&aes);
        central.extend(b"PK\x01\x02");
        put16(&mut central, 0x032d);
        for v in [version, flags, method, 0, 33] {
            put16(&mut central, v);
        }
        put32(&mut central, crc);
        put32(
            &mut central,
            if wide_sizes {
                u32::MAX
            } else {
                packed_size as u32
            },
        );
        put32(
            &mut central,
            if wide_sizes { u32::MAX } else { length as u32 },
        );
        put16(&mut central, name.len() as u16);
        put16(&mut central, extra.len() as u16);
        for _ in 0..3 {
            put16(&mut central, 0);
        }
        put32(
            &mut central,
            if directory {
                (0o40700 << 16) | 0x10
            } else {
                0o100600 << 16
            },
        );
        put32(
            &mut central,
            if wide_offset { u32::MAX } else { offset as u32 },
        );
        central.extend(name);
        central.extend(extra);
    }
    check.check()?;
    let at = io(output.stream_position())?;
    crate::space::ensure(&output, central.len() as u64 + 98)?;
    io(output.write_all(&central))?;
    let large_at = io(output.stream_position())?;
    let mut end = Vec::new();
    if have_zip64
        || plan.items.len() >= u16::MAX as usize
        || at >= u32::MAX as u64
        || central.len() >= u32::MAX as usize
    {
        end.extend(b"PK\x06\x06");
        put64(&mut end, 44);
        put16(&mut end, 45);
        put16(&mut end, 45);
        put32(&mut end, 0);
        put32(&mut end, 0);
        put64(&mut end, plan.items.len() as u64);
        put64(&mut end, plan.items.len() as u64);
        put64(&mut end, central.len() as u64);
        put64(&mut end, at);
        end.extend(b"PK\x06\x07");
        put32(&mut end, 0);
        put64(&mut end, large_at);
        put32(&mut end, 1);
        end.extend(b"PK\x05\x06");
        put16(&mut end, 0);
        put16(&mut end, 0);
        put16(&mut end, u16::MAX);
        put16(&mut end, u16::MAX);
        put32(&mut end, u32::MAX);
        put32(&mut end, u32::MAX);
        put16(&mut end, 0);
    } else {
        end.extend(b"PK\x05\x06");
        put16(&mut end, 0);
        put16(&mut end, 0);
        put16(&mut end, plan.items.len() as u16);
        put16(&mut end, plan.items.len() as u16);
        put32(&mut end, central.len() as u32);
        put32(&mut end, at as u32);
        put16(&mut end, 0);
    }
    io(output.write_all(&end))?;
    plan.verify(check)?;
    let size = io(output.stream_position())?;
    io(output.rewind())?;
    Ok((output, size))
}

#[cfg(test)]
#[path = "../../../tests/rust-archive-worker/create_zip_fields.rs"]
mod tests;
