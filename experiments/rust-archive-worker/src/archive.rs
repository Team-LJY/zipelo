use crate::protocol::*;
use std::collections::{BTreeMap, BTreeSet};
use zlib_rs::{Deflate, DeflateFlush, Status, crc32::crc32};

type Result<T> = std::result::Result<T, ErrorCode>;
pub const MAX_PATH_DEPTH: usize = 64;
pub const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_OUTPUT_NODES: usize = 32_768;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetadataLimits {
    pub(crate) bytes: usize,
    pub(crate) nodes: usize,
    pub(crate) enlarged: bool,
}
impl Default for MetadataLimits {
    fn default() -> Self {
        Self {
            bytes: MAX_METADATA_BYTES,
            nodes: MAX_OUTPUT_NODES,
            enlarged: false,
        }
    }
}
impl MetadataLimits {
    pub(crate) fn approved() -> Self {
        Self {
            bytes: 64 * 1024 * 1024,
            nodes: 262_144,
            enlarged: true,
        }
    }
}

#[derive(Default)]
pub(crate) struct MetadataBudget {
    bytes: usize,
    nodes: usize,
}
impl MetadataBudget {
    fn charge_cost(&mut self, bytes: usize, nodes: usize, limits: MetadataLimits) -> Result<()> {
        let bytes = self
            .bytes
            .checked_add(bytes)
            .filter(|n| *n <= limits.bytes)
            .ok_or(ErrorCode::Limit)?;
        let nodes = self
            .nodes
            .checked_add(nodes)
            .filter(|n| *n <= limits.nodes)
            .ok_or(ErrorCode::Limit)?;
        self.bytes = bytes;
        self.nodes = nodes;
        Ok(())
    }
    pub(crate) fn charge_bytes(&mut self, bytes: usize, limits: MetadataLimits) -> Result<()> {
        self.charge_cost(bytes, 0, limits)
    }
    pub(crate) fn charge_with(&mut self, name: &str, limits: MetadataLimits) -> Result<()> {
        // 이름 복제와 인덱스 노드 비용을 할당 전에 계산함.
        let cost = name
            .len()
            .checked_mul(4)
            .and_then(|n| n.checked_add(256))
            .ok_or(ErrorCode::Limit)?;
        self.charge_cost(cost, 1, limits)
    }
    pub(crate) fn charge_copy(&mut self, name: &str, limits: MetadataLimits) -> Result<()> {
        if !limits.enlarged {
            return self.charge_with(name, limits);
        }
        // 확대 계획의 경로와 대기 이름은 기존 항목 노드에 속한 복사 비용임.
        self.charge_cost(
            name.len().checked_mul(4).ok_or(ErrorCode::Limit)?,
            0,
            limits,
        )
    }
}

#[derive(Debug)]
pub struct Member {
    pub entry: Entry,
    pub data: Vec<u8>,
}
pub struct Catalog {
    pub format: Format,
    pub members: Vec<Member>,
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
    if name.split('/').count() > MAX_PATH_DEPTH {
        return Err(ErrorCode::Limit);
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
    validate_members_with(members, &mut || Ok(()))
}
pub(crate) fn validate_members_with(
    members: &[Member],
    check: &mut impl crate::resources::Monitor,
) -> Result<()> {
    let mut names = BTreeMap::<String, bool>::new();
    let mut budget = MetadataBudget::default();
    for member in members {
        check.check()?;
        budget.charge_with(&member.entry.name, check.metadata_limits())?;
        let directory = member.entry.kind == EntryKind::Directory;
        let name = valid_name(&member.entry.name, directory)?.to_lowercase();
        if names.insert(name, directory).is_some() {
            return Err(ErrorCode::Conflict);
        }
        crate::resources::completed(Ok(()), check)?;
    }
    for name in names.keys() {
        check.check()?;
        let mut parent = name.as_str();
        while let Some((p, _)) = parent.rsplit_once('/') {
            if names.get(p) == Some(&false) {
                return Err(ErrorCode::Conflict);
            }
            parent = p;
            crate::resources::completed(Ok(()), check)?;
        }
        crate::resources::completed(Ok(()), check)?;
    }
    Ok(())
}
pub(crate) fn output_directories(
    members: &[Member],
    check: &mut impl crate::resources::Monitor,
) -> Result<BTreeSet<String>> {
    validate_members_with(members, check)?;
    let mut budget = MetadataBudget::default();
    let mut dirs = BTreeSet::new();
    for member in members {
        check.check()?;
        budget.charge_with(&member.entry.name, check.metadata_limits())?;
        if member.entry.kind == EntryKind::Directory && !dirs.contains(member.entry.name.as_str()) {
            dirs.insert(member.entry.name.clone());
        }
        let mut name = member.entry.name.as_str();
        while let Some((parent, _)) = name.rsplit_once('/') {
            if !dirs.contains(parent) {
                budget.charge_with(parent, check.metadata_limits())?;
                dirs.insert(parent.to_owned());
            }
            name = parent;
            crate::resources::completed(Ok(()), check)?;
        }
        crate::resources::completed(Ok(()), check)?;
    }
    Ok(dirs)
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
pub(crate) fn tar_string(b: &[u8]) -> Result<String> {
    let n = b.iter().position(|v| *v == 0).unwrap_or(b.len());
    if b[n..].iter().any(|v| *v != 0) {
        return Err(ErrorCode::Integrity);
    }
    std::str::from_utf8(&b[..n])
        .map(str::to_owned)
        .map_err(|_| ErrorCode::UnsupportedFeature)
}
pub(crate) fn octal(b: &[u8]) -> Result<u64> {
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
/// ZIP 항목 이름을 해석함.
///
/// - UTF-8 플래그(비트 11)가 있으면 이름은 UTF-8이어야 하며 아니면 손상으로 봄.
/// - 플래그가 없으면 CRC가 원래 이름과 맞는 Info-ZIP Unicode Path 확장 필드(0x7075)를 먼저 씀.
///   CRC가 다르거나 버전이 1이 아니면 APPNOTE 4.6.9에 따라 무시함.
/// - 그다음 이름 바이트가 UTF-8로 유효하면 그대로 씀. Linux와 macOS 도구가 이렇게 기록함.
/// - 그 밖의 바이트(CP437, CP949 등)는 코드 페이지를 추측하지 않고 지원하지 않음으로 거부함.
///
/// 어느 경우든 결과는 호출자가 `valid_name`으로 다시 검사함.
pub(crate) fn zip_name(raw: &[u8], flags: u16, extra: &[u8]) -> Result<String> {
    if flags & 0x0800 != 0 {
        return std::str::from_utf8(raw)
            .map(str::to_owned)
            .map_err(|_| ErrorCode::Integrity);
    }
    let unicode = crate::zip_extra::parse(extra, crate::zip_extra::Policy::Names)?.unicode;
    if let Some(value) = unicode
        && value[0] == 1
        && u32::from_le_bytes(value[1..5].try_into().unwrap()) == crc32(0, raw)
    {
        return std::str::from_utf8(&value[5..])
            .map(str::to_owned)
            .map_err(|_| ErrorCode::Integrity);
    }
    std::str::from_utf8(raw)
        .map(str::to_owned)
        .map_err(|_| ErrorCode::UnsupportedFeature)
}
fn deflate(
    input: &[u8],
    limit: u64,
    check: &mut impl crate::resources::Monitor,
) -> Result<Vec<u8>> {
    let mut stream = Deflate::new(6, false, 15);
    let mut output = Vec::new();
    let mut buffer = [0; 65536];
    loop {
        check.check()?;
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
    check: &mut impl crate::resources::Monitor,
) -> Result<Vec<u8>> {
    create_zip_with_password(members, method, limits, None, check)
}
pub fn create_zip_with_password(
    members: &[Member],
    method: Method,
    limits: Limits,
    password: Option<&[u8]>,
    check: &mut impl crate::resources::Monitor,
) -> Result<Vec<u8>> {
    if !limits.valid() {
        return Err(ErrorCode::Limit);
    }
    let limits = limits.legacy();
    if members.len() > limits.entries as usize || members.len() >= 65535 {
        return Err(ErrorCode::Limit);
    }
    validate_members(members)?;
    let mut out = Vec::new();
    let mut cd = Vec::new();
    let mut total = 0u64;
    for m in members {
        check.check()?;
        total = total
            .checked_add(m.data.len() as u64)
            .filter(|n| *n <= limits.output)
            .ok_or(ErrorCode::Limit)?;
        if m.entry.kind != EntryKind::File {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let name = m.entry.name.as_bytes();
        let crc = if password.is_some() {
            0
        } else {
            crc32(0, &m.data)
        };
        let offset = out.len() as u32;
        let packed = match method {
            Method::Store => m.data.clone(),
            Method::Deflate => deflate(&m.data, limits.output, check)?,
        };
        let method = match method {
            Method::Store => 0,
            Method::Deflate => 8,
        };
        let mut extra = Vec::new();
        let flags = if password.is_some() { 0x0801 } else { 0x0800 };
        let version = if password.is_some() { 51 } else { 20 };
        let (method, packed) = if let Some(password) = password {
            put16(&mut extra, 0x9901);
            put16(&mut extra, 7);
            put16(&mut extra, 2);
            extra.extend(b"AE");
            extra.push(3);
            put16(&mut extra, method);
            (99, crate::crypto::aes_encrypt(&packed, password, check)?)
        } else {
            (method, packed)
        };
        out.extend(b"PK\x03\x04");
        put16(&mut out, version);
        put16(&mut out, flags);
        put16(&mut out, method);
        put16(&mut out, 0);
        put16(&mut out, 33);
        put32(&mut out, crc);
        put32(&mut out, packed.len() as u32);
        put32(&mut out, m.data.len() as u32);
        put16(&mut out, name.len() as u16);
        put16(&mut out, extra.len() as u16);
        out.extend(name);
        out.extend(&extra);
        bounded_append(&mut out, &packed, limits.output)?;
        cd.extend(b"PK\x01\x02");
        put16(&mut cd, 0x0314);
        put16(&mut cd, version);
        put16(&mut cd, flags);
        put16(&mut cd, method);
        put16(&mut cd, 0);
        put16(&mut cd, 33);
        put32(&mut cd, crc);
        put32(&mut cd, packed.len() as u32);
        put32(&mut cd, m.data.len() as u32);
        put16(&mut cd, name.len() as u16);
        put16(&mut cd, extra.len() as u16);
        put16(&mut cd, 0);
        put16(&mut cd, 0);
        put16(&mut cd, 0);
        put32(&mut cd, 0o100600 << 16);
        put32(&mut cd, offset);
        cd.extend(name);
        cd.extend(&extra);
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

#[cfg(test)]
#[path = "../../../tests/rust-archive-worker/metadata_policy.rs"]
mod metadata_tests;
