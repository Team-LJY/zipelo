use std::{
    collections::HashSet,
    io::{Read, Seek, SeekFrom, Write},
};

use serde_json::{Value, json};
use zip::{CompressionMethod, DateTime, ZipArchive, extra_fields::ExtendedTimestamp};

use crate::{
    budget::{Budget, CHUNK, MAX_INPUT, Source},
    error::{Failure, Result},
    wire::{Frames, u16_at, u32_at},
};

struct Entry {
    name: String,
    kind: u8,
    size: u64,
    packed: u64,
    method: u16,
    crc: u32,
    local: u64,
    central: u64,
}

fn extras(bytes: &[u8], is_local: bool) -> Result<()> {
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes.len() - cursor < 4 {
            return Err(Failure::Integrity);
        }
        let id = u16_at(bytes, cursor);
        let size = u16_at(bytes, cursor + 2) as usize;
        if size > bytes.len() - cursor - 4 {
            return Err(Failure::Integrity);
        }
        if id != 0x5455 {
            return Err(Failure::Unsupported);
        }
        // 기존 zip 메타데이터 파서의 추가 필드 검사를 유지함.
        ExtendedTimestamp::try_from_reader(
            &mut &bytes[cursor + 4..cursor + 4 + size],
            size as u16,
            is_local,
        )?;
        cursor += 4 + size;
    }
    Ok(())
}

// 할당 전 ZIP32 하위집합을 제한함. 압축 해독은 zip crate에 위임함.
fn preflight<R: Read + Seek>(
    source: &mut Source<R>,
    budget: &Budget,
    streaming: bool,
) -> Result<(Vec<Entry>, u64)> {
    let tail_start = source.length().saturating_sub(65_557);
    let tail = source.at(tail_start, (source.length() - tail_start) as usize)?;
    let end_index = (0..=tail.len() - 22)
        .rev()
        .find(|&i| {
            u32_at(&tail, i) == 0x06054b50 && i + 22 + u16_at(&tail, i + 20) as usize == tail.len()
        })
        .ok_or(Failure::Integrity)?;
    let end = tail_start + end_index as u64;
    let count = u16_at(&tail, end_index + 10) as usize;
    let central_size = u32_at(&tail, end_index + 12) as u64;
    let central = u32_at(&tail, end_index + 16) as u64;
    if u16_at(&tail, end_index + 4) != 0
        || u16_at(&tail, end_index + 6) != 0
        || u16_at(&tail, end_index + 8) as usize != count
        || count == 65_535
        || central == u32::MAX as u64
        || central_size == u32::MAX as u64
    {
        return Err(Failure::Unsupported);
    }
    budget.entries(count as u64)?;
    if central + central_size != end {
        return Err(Failure::Integrity);
    }
    let mut entries = Vec::with_capacity(count);
    let mut names = HashSet::with_capacity(count);
    let mut ranges = Vec::with_capacity(count);
    let mut cursor = central;
    let mut declared = 0u64;
    for _ in 0..count {
        budget.check()?;
        if cursor > end || end - cursor < 46 {
            return Err(Failure::Integrity);
        }
        let header = source.at(cursor, 46)?;
        if u32_at(&header, 0) != 0x02014b50 {
            return Err(Failure::Integrity);
        }
        let flags = u16_at(&header, 8);
        let method = u16_at(&header, 10);
        let creator = u16_at(&header, 4) >> 8;
        let size = u32_at(&header, 24) as u64;
        let packed = u32_at(&header, 20) as u64;
        let local = u32_at(&header, 42) as u64;
        if !matches!(creator, 0 | 3)
            || u16_at(&header, 6) > 20
            || u16_at(&header, 34) != 0
            || flags & !0x080e != 0
            || !matches!(method, 0 | 8)
            || size == u32::MAX as u64
            || packed == u32::MAX as u64
            || local == u32::MAX as u64
        {
            return Err(Failure::Unsupported);
        }
        if size > MAX_INPUT - declared || packed > MAX_INPUT {
            return Err(Failure::Budget);
        }
        declared += size;
        if (method == 0 && size != packed) || (method == 8 && packed < 2) {
            return Err(Failure::Integrity);
        }
        let name_len = u16_at(&header, 28) as usize;
        let extra_len = u16_at(&header, 30) as usize;
        let comment_len = u16_at(&header, 32) as usize;
        if name_len == 0 || name_len > 4096 {
            return Err(Failure::Budget);
        }
        let next = cursor + 46 + name_len as u64 + extra_len as u64 + comment_len as u64;
        if next > end {
            return Err(Failure::Integrity);
        }
        let name_raw = source.at(cursor + 46, name_len)?;
        if flags & 0x0800 == 0 && !name_raw.is_ascii() {
            return Err(Failure::Unsupported);
        }
        let name = std::str::from_utf8(&name_raw)
            .map_err(|_| Failure::Integrity)?
            .to_owned();
        if name.encode_utf16().count() > 1024 {
            return Err(Failure::Budget);
        }
        if streaming && !names.insert(name.clone()) {
            return Err(Failure::DuplicateName);
        }
        extras(&source.at(cursor + 46 + name_len as u64, extra_len)?, false)?;
        let attributes = u32_at(&header, 38);
        let unix_type = (attributes >> 16) & 0xf000;
        let kind = if unix_type == 0xa000 {
            2
        } else if !matches!(unix_type, 0 | 0x4000 | 0x8000) {
            4
        } else if name.ends_with('/') || unix_type == 0x4000 {
            1
        } else {
            0
        };
        let crc = u32_at(&header, 16);
        if streaming
            && (kind > 1
                || attributes & 0x400 != 0
                || ((unix_type == 0x4000 || attributes & 0x10 != 0) && !name.ends_with('/'))
                || (name.ends_with('/') && (unix_type == 0x8000 || size != 0 || crc != 0)))
        {
            return Err(Failure::UnsupportedEntry);
        }
        let local_header = source.at(local, 30)?;
        if u32_at(&local_header, 0) != 0x04034b50
            || u16_at(&local_header, 4) != u16_at(&header, 6)
            || u16_at(&local_header, 6) != flags
            || u16_at(&local_header, 8) != method
            || u16_at(&local_header, 26) as usize != name_len
            || source.at(local + 30, name_len)? != name_raw
        {
            return Err(Failure::MetadataMismatch);
        }
        // zip의 유효한 DOS 시각 또는 None 비교 계약을 그대로 사용함.
        let local_time =
            DateTime::try_from_msdos(u16_at(&local_header, 12), u16_at(&local_header, 10)).ok();
        let central_time = DateTime::try_from_msdos(u16_at(&header, 14), u16_at(&header, 12)).ok();
        if local_time != central_time {
            return Err(Failure::Integrity);
        }
        for (local_offset, expected) in [(14, crc), (18, packed as u32), (22, size as u32)] {
            let value = u32_at(&local_header, local_offset);
            if value != expected && !(flags & 8 != 0 && value == 0) {
                return Err(Failure::MetadataMismatch);
            }
        }
        let local_extra = u16_at(&local_header, 28) as usize;
        extras(&source.at(local + 30 + name_len as u64, local_extra)?, true)?;
        let data = local + 30 + name_len as u64 + local_extra as u64;
        let mut finish = data + packed;
        if finish > central {
            return Err(Failure::Integrity);
        }
        if flags & 8 != 0 {
            let marker = source.at(finish, 4)?;
            let start = finish
                + if u32_at(&marker, 0) == 0x08074b50 {
                    4
                } else {
                    0
                };
            let descriptor = source.at(start, 12)?;
            if u32_at(&descriptor, 0) != crc
                || u32_at(&descriptor, 4) as u64 != packed
                || u32_at(&descriptor, 8) as u64 != size
            {
                return Err(Failure::MetadataMismatch);
            }
            finish = start + 12;
        }
        if finish > central {
            return Err(Failure::Integrity);
        }
        ranges.push((local, finish));
        entries.push(Entry {
            name,
            kind,
            size,
            packed,
            method,
            crc,
            local,
            central: cursor,
        });
        cursor = next;
    }
    if cursor != end {
        return Err(Failure::Integrity);
    }
    ranges.sort_unstable();
    let mut next = 0;
    for (start, finish) in ranges {
        if start != next {
            return Err(Failure::Integrity);
        }
        next = finish;
    }
    if next != central {
        return Err(Failure::Integrity);
    }
    source.seek(SeekFrom::Start(0))?;
    Ok((entries, central))
}

fn open<R: Read + Seek>(
    mut source: Source<R>,
    budget: &Budget,
    streaming: bool,
) -> Result<(ZipArchive<Source<R>>, Vec<Entry>)> {
    let (expected, central) = preflight(&mut source, budget, streaming)?;
    let metadata_mode = source.metadata_switch();
    let mut archive = ZipArchive::new(source)?;
    if archive.has_duplicate_names() {
        return Err(Failure::DuplicateName);
    }
    if archive.offset() != 0
        || archive.central_directory_start() != central
        || archive.raw_zip64_extensible_data_sector().is_some()
        || archive.len() != expected.len()
    {
        return Err(Failure::MetadataMismatch);
    }
    archive.validate_header_consistency()?;
    for (index, item) in expected.iter().enumerate() {
        budget.check()?;
        let actual = archive.by_index_data(index)?;
        let method = match actual.compression() {
            CompressionMethod::Stored => 0,
            CompressionMethod::Deflated => 8,
            _ => return Err(Failure::Unsupported),
        };
        if actual.encrypted() {
            return Err(Failure::Unsupported);
        }
        if actual.name_raw() != item.name.as_bytes()
            || actual.name()?.as_ref() != item.name
            || actual.size() != item.size
            || actual.compressed_size() != item.packed
            || actual.crc32() != item.crc
            || method != item.method
            || actual.header_start() != item.local
            || actual.central_header_start() != item.central
        {
            return Err(Failure::MetadataMismatch);
        }
    }
    metadata_mode.set(false);
    Ok((archive, expected))
}

pub fn list<R: Read + Seek>(mut source: Source<R>, budget: &Budget) -> Result<Vec<Value>> {
    // 이름으로 축약하지 않고 검증된 중앙 디렉터리 순서와 항목 수를 보존함.
    let (entries, _) = preflight(&mut source, budget, false)?;
    let mut results = Vec::with_capacity(entries.len());
    for (index, item) in entries.into_iter().enumerate() {
        budget.check()?;
        results.push(json!({
            "Entry": {
                "Id": format!("entry-{index}"), "Name": item.name,
                "Kind": item.kind, "DeclaredSize": item.size
            },
            "EntryId": format!("entry-{index}"), "Outcome": 0, "Verification": 0, "Error": 0
        }));
    }
    Ok(results)
}

pub fn extract<R: Read + Seek, W: Write>(
    source: Source<R>,
    budget: &Budget,
    output: &mut Frames<W>,
) -> Result<()> {
    let (mut archive, expected) = open(source, budget, true)?;
    let mut buffer = [0; CHUNK];
    for (index, item) in expected.iter().enumerate() {
        budget.check()?;
        output.begin_item(index, item.size)?;
        let mut file = archive.by_index(index)?;
        let mut actual = 0u64;
        loop {
            budget.check()?;
            let got = file.read(&mut buffer)?;
            if got == 0 {
                break;
            }
            if got as u64 > item.size - actual {
                return Err(Failure::Integrity);
            }
            actual += got as u64;
            output.data(&buffer[..got])?;
        }
        // EOF까지 읽어 zip crate의 CRC 검사 결과를 확인한 뒤 항목을 끝냄.
        if actual != item.size {
            return Err(Failure::Integrity);
        }
        output.end_item()?;
    }
    budget.check()?;
    output.complete()
}
