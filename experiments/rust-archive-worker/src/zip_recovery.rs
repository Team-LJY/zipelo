use crate::{
    archive::{self, Member, MetadataBudget},
    create_sources::Input,
    filesystem,
    protocol::*,
    resources::{Monitor, completed},
    spool,
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};
use zlib_rs::{Inflate, InflateFlush, Status, crc32::crc32};

type Result<T> = std::result::Result<T, ErrorCode>;
const INPUT_CAP: u64 = 4 * 1024 * 1024 * 1024;
fn io<T>(value: std::io::Result<T>) -> Result<T> {
    value.map_err(crate::space::io_error)
}
fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).ok_or(ErrorCode::Integrity)
}
fn u16at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(b[i..i + 2].try_into().unwrap())
}
fn u32at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn bytes(source: &mut File, offset: u64, size: usize, end: u64) -> Result<Vec<u8>> {
    if add(offset, size as u64)? > end {
        return Err(ErrorCode::Integrity);
    }
    io(source.seek(SeekFrom::Start(offset)))?;
    let mut data = vec![0; size];
    io(source.read_exact(&mut data))?;
    Ok(data)
}
fn extra(data: &[u8]) -> Result<()> {
    let mut at = 0;
    while at < data.len() {
        if data.len() - at < 4 {
            return Err(ErrorCode::Integrity);
        }
        let tag = u16at(data, at);
        let size = u16at(data, at + 2) as usize;
        if matches!(tag, 1 | 0x0017 | 0x0018) {
            return Err(ErrorCode::UnsupportedFeature);
        }
        at = at
            .checked_add(4 + size)
            .filter(|n| *n <= data.len())
            .ok_or(ErrorCode::Integrity)?;
    }
    Ok(())
}
struct Candidate {
    finding: RecoveryEntry,
    start: u64,
    stop: u64,
    packed: u64,
    crc: u32,
    method: u16,
    supported: bool,
    boundary: bool,
    repaired: bool,
}
fn declared(size: u32, packed: u32) -> Result<(u64, u64)> {
    if size == u32::MAX || packed == u32::MAX {
        return Err(ErrorCode::UnsupportedFeature);
    }
    Ok((u64::from(size), u64::from(packed)))
}
fn named(raw: &[u8], flags: u16, ext: &[u8], kind: EntryKind) -> Result<String> {
    if !matches!(kind, EntryKind::File | EntryKind::Directory) {
        return Err(ErrorCode::UnsafePath);
    }
    let name = archive::zip_name(raw, flags, ext)?;
    archive::valid_name(&name, kind == EntryKind::Directory)
}
fn candidate(
    offset: u64,
    name: String,
    kind: EntryKind,
    size: u64,
    packed: u64,
    crc: u32,
    method: u16,
    flags: u16,
) -> Candidate {
    Candidate {
        finding: RecoveryEntry {
            candidate_id: 0,
            source_offset: offset,
            name: Some(name),
            kind: Some(kind),
            status: RecoveryStatus::Unverified,
            reason: RecoveryReason::BoundaryUnknown,
            declared_size: Some(size),
            verified_size: None,
            sha256: None,
        },
        start: 0,
        stop: offset,
        packed,
        crc,
        method,
        supported: flags & !0x080e == 0 && matches!(method, 0 | 8),
        boundary: false,
        repaired: false,
    }
}
fn charge(
    metadata: &mut MetadataBudget,
    name_len: usize,
    extra_len: usize,
    check: &mut impl Monitor,
) -> Result<()> {
    if name_len > 4096 {
        return Err(ErrorCode::UnsafePath);
    }
    metadata.charge_bytes(
        name_len
            .checked_mul(8)
            .and_then(|n| n.checked_add(extra_len + 1024))
            .ok_or(ErrorCode::Limit)?,
        check.metadata_limits(),
    )
}

fn zip64_bounds(source: &mut File, end: u64) -> Result<Option<(u64, u64)>> {
    if end < 20 {
        return Ok(None);
    }
    let locator_at = end - 20;
    let locator = bytes(source, locator_at, 20, end)?;
    if &locator[..4] != b"PK\x06\x07" || u32at(&locator, 4) != 0 || u32at(&locator, 16) != 1 {
        return Ok(None);
    }
    let record_at = u64::from_le_bytes(locator[8..16].try_into().unwrap());
    if record_at.checked_add(56).is_none_or(|n| n > locator_at) {
        return Ok(None);
    }
    let record = bytes(source, record_at, 56, locator_at)?;
    let length = u64::from_le_bytes(record[4..12].try_into().unwrap());
    if &record[..4] != b"PK\x06\x06"
        || length < 44
        || record_at
            .checked_add(12)
            .and_then(|n| n.checked_add(length))
            != Some(locator_at)
    {
        return Ok(None);
    }
    let size = u64::from_le_bytes(record[40..48].try_into().unwrap());
    let offset = u64::from_le_bytes(record[48..56].try_into().unwrap());
    Ok((offset.checked_add(size) == Some(record_at)).then_some((offset, size)))
}

fn directory_span(
    source: &mut File,
    mut at: u64,
    end: u64,
    check: &mut impl Monitor,
) -> Result<bool> {
    let mut count = 0u32;
    while at < end {
        check.check()?;
        if end - at < 46 {
            return Ok(false);
        }
        let header = bytes(source, at, 46, end)?;
        if &header[..4] != b"PK\x01\x02" {
            return Ok(false);
        }
        let next = add(
            at,
            46 + u16at(&header, 28) as u64 + u16at(&header, 30) as u64 + u16at(&header, 32) as u64,
        )?;
        if next > end {
            return Ok(false);
        }
        count += 1;
        if count > u16::MAX as u32 {
            return Err(ErrorCode::UnsupportedFeature);
        }
        at = next;
    }
    Ok(true)
}

struct EndRecord {
    index: usize,
    at: u64,
    directory: u64,
    directory_end: u64,
}

fn metadata_overlaps_member(
    source: &mut File,
    owner: &EndRecord,
    nested: &EndRecord,
    check: &mut impl Monitor,
) -> Result<bool> {
    let mut at = owner.directory;
    while at < owner.directory_end {
        check.check()?;
        let h = bytes(source, at, 46, owner.directory_end)?;
        let nn = u16at(&h, 28) as usize;
        let ne = u16at(&h, 30) as usize;
        let next = add(at, 46 + nn as u64 + ne as u64 + u16at(&h, 32) as u64)?;
        let offset = u32at(&h, 42) as u64;
        let packed = u32at(&h, 20);
        let size = u32at(&h, 24);
        if nn > 0
            && nn <= 4096
            && packed > 0
            && packed != u32::MAX
            && size != u32::MAX
            && offset.checked_add(30).is_some_and(|n| n <= owner.directory)
        {
            let local = bytes(source, offset, 30, owner.directory)?;
            let start = add(
                offset,
                30 + u16at(&local, 26) as u64 + u16at(&local, 28) as u64,
            )?;
            let stop = add(start, packed as u64)?;
            let overlaps = (start < nested.directory_end && nested.directory < stop)
                || (start < nested.at + 22 && nested.at < stop);
            let flags = u16at(&h, 8);
            if overlaps
                && stop <= owner.directory
                && nn == u16at(&local, 26) as usize
                && flags == u16at(&local, 6)
                && u16at(&h, 10) == u16at(&local, 8)
                && bytes(source, at + 46, nn, owner.directory_end)?
                    == bytes(source, offset + 30, nn, owner.directory)?
            {
                // 두 목록이 주장한 범위보다 로컬 필드와 일치한 본문 경계를 우선함.
                if flags & 8 == 0 {
                    if packed == u32at(&local, 18) && size == u32at(&local, 22) {
                        return Ok(true);
                    }
                } else if stop.checked_add(12).is_some_and(|n| n <= owner.directory) {
                    let descriptor = bytes(source, stop, 12, owner.directory)?;
                    let skip = if &descriptor[..4] == b"PK\x07\x08" {
                        4
                    } else {
                        0
                    };
                    if (stop + skip + 12) <= owner.directory {
                        let d = bytes(source, stop + skip, 12, owner.directory)?;
                        if u32at(&d, 0) == u32at(&h, 16)
                            && u32at(&d, 4) == packed
                            && u32at(&d, 8) == size
                        {
                            return Ok(true);
                        }
                    }
                }
            }
        }
        at = next;
    }
    Ok(false)
}

fn end_record(
    source: &mut File,
    tail: &[u8],
    tail_at: u64,
    check: &mut impl Monitor,
) -> Result<Option<usize>> {
    let mut selected: Option<EndRecord> = None;
    for end in (0..=tail.len() - 22).rev() {
        if &tail[end..end + 4] != b"PK\x05\x06"
            || end + 22 + u16at(tail, end + 20) as usize != tail.len()
        {
            continue;
        }
        check.check()?;
        let h = &tail[end..];
        let at = tail_at + end as u64;
        let mut offset = u32at(h, 16) as u64;
        let mut size = u32at(h, 12) as u64;
        let zip64 = offset == u32::MAX as u64 || size == u32::MAX as u64;
        if zip64 {
            let Some(bounds) = zip64_bounds(source, at)? else {
                continue;
            };
            (offset, size) = bounds;
        }
        let Some(stop) = offset.checked_add(size).filter(|&n| n <= at) else {
            continue;
        };
        if size < 46 || bytes(source, offset, 4, stop)? != b"PK\x01\x02" {
            continue;
        }
        // 주석 안 후보는 중앙 목록 또는 실제 ZIP64 경계와 이어지지 않으면 제외함.
        if stop != at && !zip64 {
            let adjacent_locator =
                at.checked_sub(20) == Some(stop) && bytes(source, stop, 4, at)? == b"PK\x06\x07";
            if !adjacent_locator && zip64_bounds(source, at)? != Some((offset, size)) {
                continue;
            }
        }
        if !directory_span(source, offset, stop, check)? {
            continue;
        }
        let current = EndRecord {
            index: end,
            at,
            directory: offset,
            directory_end: stop,
        };
        if let Some(previous) = &selected {
            let current_in_member = metadata_overlaps_member(source, previous, &current, check)?;
            let previous_in_member = metadata_overlaps_member(source, &current, previous, check)?;
            if current_in_member && previous_in_member {
                return Err(ErrorCode::Integrity);
            }
            if current_in_member {
                continue;
            }
            // 본문 안의 메타데이터를 제거한 뒤 중앙 주석과 파일 주석의 소속을 비교함.
            if !previous_in_member
                && !(offset < previous.directory && previous.directory < stop)
                && !(at + 22 <= previous.directory && end + 22 <= previous.index)
            {
                continue;
            }
        }
        selected = Some(current);
    }
    Ok(selected.map(|record| record.index))
}

fn central(
    source: &mut File,
    len: u64,
    limits: Limits,
    check: &mut impl Monitor,
) -> Result<Option<(Vec<Candidate>, bool)>> {
    if len < 22 {
        return Ok(None);
    }
    let mut metadata = MetadataBudget::default();
    metadata.charge_bytes(len.min(65557) as usize, check.metadata_limits())?;
    let tail_at = len.saturating_sub(65557);
    let tail = bytes(source, tail_at, (len - tail_at) as usize, len)?;
    let Some(end) = end_record(source, &tail, tail_at, check)? else {
        return Ok(None);
    };
    let h = &tail[end..];
    let count = u16at(h, 10) as u32;
    if u16at(h, 4) != 0
        || u16at(h, 6) != 0
        || u16at(h, 8) as u32 != count
        || count == u16::MAX as u32
        || u32at(h, 12) == u32::MAX
        || u32at(h, 16) == u32::MAX
    {
        return Err(ErrorCode::UnsupportedFeature);
    }
    let cd = u32at(h, 16) as u64;
    let cd_end = tail_at + end as u64;
    let declared_cd_end = add(cd, u32at(h, 12) as u64)?;
    // 중앙 목록의 주석을 ZIP64 locator로 오인하지 않음.
    if cd_end >= 20
        && declared_cd_end <= cd_end - 20
        && bytes(source, cd_end - 20, 4, len)? == b"PK\x06\x07"
    {
        return Err(ErrorCode::UnsupportedFeature);
    }
    if declared_cd_end != cd_end {
        return Err(ErrorCode::Integrity);
    }
    if count > limits.entries {
        return Err(ErrorCode::Limit);
    }
    check.observe(
        ResourceUsage {
            estimated_entries: Some(count),
            ..ResourceUsage::default()
        },
        Some(source),
    )?;
    let mut items = Vec::new();
    let mut at = cd;
    for _ in 0..count {
        check.check()?;
        let h = bytes(source, at, 46, cd_end)?;
        if &h[..4] != b"PK\x01\x02" {
            return Err(ErrorCode::Integrity);
        }
        if u16at(&h, 34) != 0 || u32at(&h, 42) == u32::MAX {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let flags = u16at(&h, 8);
        if flags & 0x2040 != 0 {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let nn = u16at(&h, 28) as usize;
        let ne = u16at(&h, 30) as usize;
        let next = add(at, 46 + nn as u64 + ne as u64 + u16at(&h, 32) as u64)?;
        if next > cd_end {
            return Err(ErrorCode::Integrity);
        }
        charge(&mut metadata, nn, ne, check)?;
        let raw = bytes(source, at + 46, nn, cd_end)?;
        let ext = bytes(source, at + 46 + nn as u64, ne, cd_end)?;
        extra(&ext)?;
        let (size, packed) = declared(u32at(&h, 24), u32at(&h, 20))?;
        let unix_kind = (u32at(&h, 38) >> 16) & 0xf000;
        let kind = match unix_kind {
            0 | 0x8000 => {
                if raw.ends_with(b"/") {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                }
            }
            0x4000 => EntryKind::Directory,
            _ => return Err(ErrorCode::UnsafePath),
        };
        let name = named(&raw, flags, &ext, kind)?;
        metadata.charge_with(&name, check.metadata_limits())?;
        let offset = u32at(&h, 42) as u64;
        let mut item = candidate(
            offset,
            name,
            kind,
            size,
            packed,
            u32at(&h, 16),
            u16at(&h, 10),
            flags,
        );
        if add(offset, 30)? > cd {
            return Err(ErrorCode::Integrity);
        }
        let local = bytes(source, offset, 30, cd)?;
        declared(u32at(&local, 22), u32at(&local, 18))?;
        let ln = u16at(&local, 26) as usize;
        let le = u16at(&local, 28) as usize;
        charge(&mut metadata, ln, le, check)?;
        let start = add(offset, 30 + ln as u64 + le as u64)?;
        if start > cd {
            return Err(ErrorCode::Integrity);
        }
        let local_raw = bytes(source, offset + 30, ln, cd)?;
        let local_extra = bytes(source, offset + 30 + ln as u64, le, cd)?;
        extra(&local_extra)?;
        let mut stop = add(start, packed)?;
        if stop > cd {
            return Err(ErrorCode::Integrity);
        }
        item.start = start;
        item.boundary =
            raw == local_raw && flags == u16at(&local, 6) && item.method == u16at(&local, 8);
        item.repaired = &local[..4] != b"PK\x03\x04"
            || (flags & 8 == 0
                && (u32at(&local, 14) != item.crc
                    || u32at(&local, 18) as u64 != packed
                    || u32at(&local, 22) as u64 != size));
        if flags & 8 != 0 {
            let descriptor = bytes(source, stop, 12, cd)?;
            let skip = if &descriptor[..4] == b"PK\x07\x08" {
                4
            } else {
                0
            };
            let d = bytes(source, stop + skip, 12, cd)?;
            if u32at(&d, 0) != item.crc
                || u32at(&d, 4) as u64 != packed
                || u32at(&d, 8) as u64 != size
            {
                item.boundary = false;
            }
            stop = add(stop, 12 + skip)?;
        }
        item.stop = stop;
        if kind == EntryKind::Directory && size != 0 {
            item.boundary = false;
        }
        items.push(item);
        at = next;
        completed(Ok(()), check)?;
    }
    if at != cd_end {
        return Err(ErrorCode::Integrity);
    }
    items.sort_by_key(|c| c.finding.source_offset);
    let mut cursor = 0;
    let mut complete = true;
    for c in &items {
        if c.finding.source_offset < cursor {
            return Err(ErrorCode::Integrity);
        }
        complete &= c.finding.source_offset == cursor && c.boundary;
        cursor = c.stop;
    }
    complete &= cursor == cd;
    Ok(Some((items, complete)))
}
fn local(
    source: &mut File,
    len: u64,
    limits: Limits,
    check: &mut impl Monitor,
) -> Result<Vec<Candidate>> {
    let mut at = 0;
    let mut items = Vec::new();
    let mut metadata = MetadataBudget::default();
    while at < len {
        check.check()?;
        if len - at < 30 {
            break;
        }
        let h = bytes(source, at, 30, len)?;
        if &h[..4] != b"PK\x03\x04" {
            if at == 0
                && (h.starts_with(b"MZ")
                    || h.starts_with(b"PK\x07\x08")
                    || h.starts_with(b"PK\x06"))
            {
                return Err(ErrorCode::UnsupportedFeature);
            }
            break;
        }
        let count = u32::try_from(items.len() + 1).map_err(|_| ErrorCode::Limit)?;
        if count > limits.entries {
            return Err(ErrorCode::Limit);
        }
        check.observe(
            ResourceUsage {
                actual_entries: Some(count),
                ..ResourceUsage::default()
            },
            Some(source),
        )?;
        let flags = u16at(&h, 6);
        if flags & 0x2040 != 0 {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let nn = u16at(&h, 26) as usize;
        let ne = u16at(&h, 28) as usize;
        charge(&mut metadata, nn, ne, check)?;
        let start = add(at, 30 + nn as u64 + ne as u64)?;
        if start > len {
            break;
        }
        let raw = bytes(source, at + 30, nn, len)?;
        let ext = bytes(source, at + 30 + nn as u64, ne, len)?;
        extra(&ext)?;
        let kind = if raw.ends_with(b"/") {
            EntryKind::Directory
        } else {
            EntryKind::File
        };
        let name = named(&raw, flags, &ext, kind)?;
        metadata.charge_with(&name, check.metadata_limits())?;
        let (size, packed) = declared(u32at(&h, 22), u32at(&h, 18))?;
        let mut c = candidate(
            at,
            name,
            kind,
            size,
            packed,
            u32at(&h, 14),
            u16at(&h, 8),
            flags,
        );
        c.start = start;
        c.stop = add(start, packed)?;
        if flags & 8 != 0 {
            c.finding.declared_size = None;
            items.push(c);
            break;
        }
        c.boundary = true;
        let next = c.stop;
        items.push(c);
        if next > len {
            break;
        }
        at = next;
        completed(Ok(()), check)?;
    }
    Ok(items)
}

pub(crate) struct Recovery {
    source: Input,
    path: PathBuf,
    modified: Option<SystemTime>,
    pub snapshot: spool::Snapshot,
    pub findings: Vec<RecoveryEntry>,
    pub summary: RecoveryEnd,
    pub data: File,
    pub offsets: Vec<u64>,
}
impl Recovery {
    pub fn open(path: &Path, limits: Limits, check: &mut impl Monitor) -> Result<Self> {
        let mut source = completed(Input::open(path), check)?;
        let meta = io(source.file.metadata())?;
        if !meta.is_file() {
            return Err(ErrorCode::InvalidRequest);
        }
        if meta.len() > INPUT_CAP {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let prefix = bytes(&mut source.file, 0, meta.len().min(4) as usize, meta.len())?;
        if prefix.starts_with(b"MZ")
            || prefix.starts_with(b"PK\x07\x08")
            || prefix.starts_with(b"PK\x06")
        {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let mut snapshot =
            spool::Snapshot::capture(&mut source.file, limits.input.min(INPUT_CAP), check)?;
        let named = completed(Input::open(path), check)?;
        if filesystem::object(&named.file)? != snapshot.object {
            return Err(ErrorCode::SourceChanged);
        }
        let discovered = central(&mut snapshot.file, snapshot.len, limits, check);
        let (mut candidates, mode, known) = match discovered {
            Ok(Some((items, complete))) => (items, RecoveryMode::CentralDirectory, complete),
            Ok(None) | Err(ErrorCode::Integrity) => (
                local(&mut snapshot.file, snapshot.len, limits, check)?,
                RecoveryMode::LocalChain,
                false,
            ),
            Err(error) => return Err(error),
        };
        let mut names = Vec::new();
        let mut budget = MetadataBudget::default();
        for (id, c) in candidates.iter_mut().enumerate() {
            c.finding.candidate_id = id as u32;
            let name = c.finding.name.as_ref().ok_or(ErrorCode::Internal)?;
            budget.charge_with(name, check.metadata_limits())?;
            names.push(Member {
                entry: Entry {
                    entry_id: id as u32,
                    name: name.clone(),
                    kind: c.finding.kind.unwrap(),
                    size: c.finding.declared_size.unwrap_or(0),
                    compressed_size: c.packed,
                    encrypted: false,
                },
                data: Vec::new(),
            });
        }
        archive::validate_members_with(&names, check)?;
        drop(names);
        let mut data = spool::temporary()?;
        let mut findings = Vec::new();
        let mut offsets = Vec::new();
        let mut used = 0u64;
        let mut verified = 0;
        let mut lost = 0;
        let mut unverified = 0;
        let mut repaired = false;
        for c in candidates {
            check.check()?;
            let base = io(data.stream_position())?;
            offsets.push(base);
            let mut finding = c.finding;
            if !c.boundary {
                finding.reason = RecoveryReason::BoundaryUnknown;
            } else if !c.supported {
                finding.reason = RecoveryReason::UnsupportedFeature;
            } else if c.stop > snapshot.len {
                finding.status = RecoveryStatus::Lost;
                finding.reason = RecoveryReason::Truncated;
            } else {
                let size = finding.declared_size.ok_or(ErrorCode::Internal)?;
                if used.checked_add(size).is_none_or(|n| n > limits.output) {
                    return Err(ErrorCode::Limit);
                }
                check.observe(
                    ResourceUsage {
                        estimated_bytes: Some(used + size),
                        actual_entries: Some(finding.candidate_id + 1),
                        compressed_bytes: Some(c.packed),
                        ratio_output_bytes: Some(size),
                        ..ResourceUsage::default()
                    },
                    Some(&data),
                )?;
                io(snapshot.file.seek(SeekFrom::Start(c.start)))?;
                let decoded = decode(
                    &mut snapshot.file,
                    c.packed,
                    c.method,
                    size,
                    &mut data,
                    &mut used,
                    limits,
                    check,
                );
                match decoded {
                    Ok((length, crc, hash))
                        if length == size
                            && crc == c.crc
                            && (finding.kind != Some(EntryKind::Directory) || size == 0) =>
                    {
                        finding.status = RecoveryStatus::Verified;
                        finding.reason = if c.repaired {
                            RecoveryReason::HeaderRebuilt
                        } else if mode == RecoveryMode::LocalChain {
                            RecoveryReason::MetadataMissing
                        } else {
                            RecoveryReason::None
                        };
                        finding.verified_size = Some(size);
                        finding.sha256 = if finding.kind == Some(EntryKind::File) {
                            Some(hash)
                        } else {
                            None
                        };
                    }
                    Ok((length, _, _)) => {
                        finding.status = RecoveryStatus::Lost;
                        finding.reason = if length == size {
                            RecoveryReason::CrcMismatch
                        } else {
                            RecoveryReason::DecodeFailure
                        };
                    }
                    Err(ErrorCode::Integrity) => {
                        finding.status = RecoveryStatus::Lost;
                        finding.reason = RecoveryReason::DecodeFailure;
                    }
                    Err(error) => return Err(error),
                }
                if finding.status != RecoveryStatus::Verified {
                    io(data.set_len(base))?;
                    io(data.seek(SeekFrom::Start(base)))?;
                }
            }
            repaired |= c.repaired;
            match finding.status {
                RecoveryStatus::Verified => verified += 1,
                RecoveryStatus::Lost => lost += 1,
                RecoveryStatus::Unverified => unverified += 1,
            }
            check.progress(Stage::Verify, used, None, finding.candidate_id + 1)?;
            findings.push(finding);
        }
        if known && verified as usize == findings.len() && !repaired {
            return Err(ErrorCode::InvalidRequest);
        }
        let summary = RecoveryEnd {
            mode,
            source_bytes: snapshot.len,
            source_sha256: snapshot.hash,
            known_entries: if known {
                Some(findings.len() as u32)
            } else {
                None
            },
            verified_entries: verified,
            lost_entries: lost,
            unverified_entries: unverified,
            unknown_loss: !known,
        };
        Ok(Self {
            source,
            path: path.to_owned(),
            modified: meta.modified().ok(),
            snapshot,
            findings,
            summary,
            data,
            offsets,
        })
    }
    pub fn build(&mut self, limits: Limits, check: &mut impl Monitor) -> Result<(File, u64)> {
        let (mut output, len) =
            crate::create::recovered_zip(&self.findings, &self.offsets, &mut self.data, check)?;
        let mut replay = Replay(check);
        let mut catalog =
            crate::streaming::Catalog::open(&mut output, len, limits, true, None, &mut replay)?;
        if catalog.format != Format::Zip
            || catalog.members.len() != self.summary.verified_entries as usize
        {
            return Err(ErrorCode::Integrity);
        }
        for ((member, offset), finding) in catalog.members.iter().zip(&catalog.offsets).zip(
            self.findings
                .iter()
                .filter(|e| e.status == RecoveryStatus::Verified),
        ) {
            if Some(&member.entry.name) != finding.name.as_ref()
                || Some(member.entry.kind) != finding.kind
                || Some(member.entry.size) != finding.verified_size
            {
                return Err(ErrorCode::Integrity);
            }
            if member.entry.kind == EntryKind::File {
                io(catalog.data.seek(SeekFrom::Start(*offset)))?;
                let mut input = (&mut catalog.data).take(member.entry.size);
                let mut hash = Sha256::new();
                let mut buffer = [0; 65536];
                loop {
                    replay.check()?;
                    let n = completed(io(input.read(&mut buffer)), &mut replay)?;
                    if n == 0 {
                        break;
                    }
                    hash.update(&buffer[..n]);
                }
                if input.limit() != 0 || Some(<[u8; 32]>::from(hash.finalize())) != finding.sha256 {
                    return Err(ErrorCode::Integrity);
                }
            }
        }
        io(output.rewind())?;
        Ok((output, len))
    }
    pub fn verify_source(&mut self, check: &mut impl Monitor) -> Result<()> {
        let mut named = Input::open(&self.path).map_err(|_| ErrorCode::SourceChanged)?;
        let before = io(named.file.metadata())?;
        let pinned = io(self.source.file.metadata())?;
        if filesystem::object(&named.file)? != self.snapshot.object
            || before.len() != self.snapshot.len
            || before.modified().ok() != self.modified
            || pinned.len() != self.snapshot.len
            || pinned.modified().ok() != self.modified
        {
            return Err(ErrorCode::SourceChanged);
        }
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        let mut length = 0;
        loop {
            check.check()?;
            let n = completed(io(named.file.read(&mut buffer)), check)?;
            if n == 0 {
                break;
            }
            length = add(length, n as u64)?;
            hash.update(&buffer[..n]);
            check.progress(
                Stage::Verify,
                length,
                Some(self.snapshot.len),
                self.findings.len() as u32,
            )?;
        }
        let after = io(named.file.metadata())?;
        let current = Input::open(&self.path).map_err(|_| ErrorCode::SourceChanged)?;
        if length != self.snapshot.len
            || <[u8; 32]>::from(hash.finalize()) != self.snapshot.hash
            || after.len() != self.snapshot.len
            || after.modified().ok() != self.modified
            || filesystem::object(&current.file)? != self.snapshot.object
        {
            return Err(ErrorCode::SourceChanged);
        }
        Ok(())
    }
}
fn decode(
    input: &mut File,
    length: u64,
    method: u16,
    declared: u64,
    output: &mut File,
    used: &mut u64,
    limits: Limits,
    check: &mut impl Monitor,
) -> Result<(u64, u32, [u8; 32])> {
    let mut input = input.take(length);
    let mut stream = Inflate::new(false, 15);
    let mut buffer = [0; 65536];
    let mut plain = [0; 65536];
    let mut have = 0;
    let mut at = 0;
    let mut total = 0u64;
    let mut crc = 0;
    let mut hash = Sha256::new();
    loop {
        check.check()?;
        if at == have && input.limit() != 0 {
            have = completed(io(input.read(&mut buffer)), check)?;
            at = 0;
            if have == 0 {
                return Err(ErrorCode::Integrity);
            }
        }
        let (n, status, consumed, corrupt) = if method == 0 {
            plain[..have].copy_from_slice(&buffer[..have]);
            (
                have,
                if input.limit() == 0 {
                    Status::StreamEnd
                } else {
                    Status::Ok
                },
                have,
                false,
            )
        } else {
            let before_in = stream.total_in();
            let before_out = stream.total_out();
            let status = stream.decompress(&buffer[at..have], &mut plain, InflateFlush::NoFlush);
            let corrupt = status.is_err();
            (
                (stream.total_out() - before_out) as usize,
                status.unwrap_or(Status::Ok),
                (stream.total_in() - before_in) as usize,
                corrupt,
            )
        };
        at += consumed;
        total = total.checked_add(n as u64).ok_or(ErrorCode::Limit)?;
        *used = used
            .checked_add(n as u64)
            .filter(|n| *n <= limits.output)
            .ok_or(ErrorCode::Limit)?;
        check.observe(
            ResourceUsage {
                actual_bytes: Some(*used),
                compressed_bytes: Some(length),
                ratio_output_bytes: Some(total),
                ..ResourceUsage::default()
            },
            Some(output),
        )?;
        if corrupt || total > declared {
            return Err(ErrorCode::Integrity);
        }
        crc = crc32(crc, &plain[..n]);
        hash.update(&plain[..n]);
        crate::space::ensure(output, n as u64)?;
        completed(io(output.write_all(&plain[..n])), check)?;
        if status == Status::StreamEnd {
            if input.limit() != 0 || at != have {
                return Err(ErrorCode::Integrity);
            }
            break;
        }
        if n == 0 && consumed == 0 {
            return Err(ErrorCode::Integrity);
        }
    }
    Ok((total, crc, hash.finalize().into()))
}

struct Replay<'a, M>(&'a mut M);
impl<M: Monitor> Monitor for Replay<'_, M> {
    fn metadata_limits(&self) -> archive::MetadataLimits {
        self.0.metadata_limits()
    }
    fn check(&mut self) -> Result<()> {
        self.0.check()
    }
    fn activity(&mut self) -> Result<()> {
        self.0.activity()
    }
    fn observe(&mut self, _usage: ResourceUsage, _storage: Option<&File>) -> Result<()> {
        self.0.check()
    }
}

#[cfg(test)]
#[path = "../../../tests/rust-archive-worker/recovery_checks.rs"]
mod tests;
