//! Linux 이름 정책을 적용하는 메모리 전용 USTAR 후보임.
//! 완료 전 payload를 노출하지 않으며 파일시스템과 worker를 사용하지 않음.

use std::{fmt, time::Duration};

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

mod budget;
mod header;

use budget::Budget;

pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 1024;
pub const MAX_PROCESSED: u64 = 128 * 1024 * 1024;
pub const MAX_METADATA: u64 = 1024 * 1024;
pub const MAX_DURATION: Duration = Duration::from_secs(300);
pub const CHUNK: usize = 65_536;
const BLOCK: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamePolicy {
    Linux,
    Windows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Directory,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub size: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub input: u64,
    pub entries: usize,
    pub processed: u64,
    pub output: u64,
    pub metadata: u64,
    pub duration: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            input: MAX_BYTES,
            entries: MAX_ENTRIES,
            processed: MAX_PROCESSED,
            output: MAX_BYTES,
            metadata: MAX_METADATA,
            duration: MAX_DURATION,
        }
    }
}

#[derive(Debug)]
pub struct Request {
    pub input: Box<[u8]>,
    pub policy: NamePolicy,
    pub limits: Limits,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub entries: usize,
    pub processed: u64,
    pub output: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SelectedFile {
    pub id: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentVerification {
    Unavailable,
}

#[derive(Debug)]
pub struct Completed {
    pub entries: Vec<Entry>,
    pub files: Vec<SelectedFile>,
    pub usage: Usage,
    pub input_sha256: [u8; 32],
    pub content_verification: ContentVerification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    InvalidRequest,
    Unsupported,
    Integrity,
    UnsafePath,
    Conflict,
    Budget,
    Cancelled,
    Allocation,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    Reader(Failure),
    Selection(E),
}

impl<E> From<Failure> for Error<E> {
    fn from(value: Failure) -> Self {
        Self::Reader(value)
    }
}

impl<E: fmt::Display> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reader(value) => write!(f, "{value:?}"),
            Self::Selection(value) => write!(f, "selection: {value}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for Error<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Selection(value) => Some(value),
            Self::Reader(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Cancelled,
    Deadline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checkpoint {
    Start,
    Read { pass: u8, offset: usize },
    Entry { pass: u8, index: usize },
    BeforeSelection,
    AfterSelection,
    SelectionId { index: usize },
    Finish,
}

/// 선택 콜백은 1차 검증 목록만 받음. 실패 시 누적 파일 결과를 전부 폐기함.
/// 콜백 자체의 할당, 실행 시간과 panic은 호출자가 관리함.
pub fn read<E, S, C>(request: Request, select: S, checkpoint: C) -> Result<Completed, Error<E>>
where
    S: FnOnce(&[Entry]) -> Result<Vec<String>, E>,
    C: FnMut(Checkpoint) -> Result<(), Stop>,
{
    let mut budget = Budget::new(request.limits, checkpoint)?;
    budget.check(Checkpoint::Start)?;
    if request.policy != NamePolicy::Linux {
        return Err(Failure::Unsupported.into());
    }
    if request.input.len() as u64 > budget.limits.input {
        return Err(Failure::Budget.into());
    }
    if request.input.len() < 2 * BLOCK || request.input.len() % BLOCK != 0 {
        return Err(Failure::Integrity.into());
    }
    let mut entries = Vec::new();
    let mut files = Vec::new();
    let first = scan(&request.input, &mut entries, None, &mut files, &mut budget)?;
    budget.check(Checkpoint::BeforeSelection)?;
    let requested = select(&entries).map_err(Error::Selection)?;
    budget.check(Checkpoint::AfterSelection)?;
    if requested.len() > entries.len() {
        return Err(Failure::Unsupported.into());
    }
    let mut selected = Vec::new();
    selected
        .try_reserve_exact(entries.len())
        .map_err(|_| Failure::Allocation)?;
    selected.resize(entries.len(), false);
    for (index, id) in requested.iter().enumerate() {
        budget.check(Checkpoint::SelectionId { index })?;
        let position = entries
            .iter()
            .position(|entry| entry.id == *id)
            .ok_or(Failure::Unsupported)?;
        if selected[position] {
            return Err(Failure::Unsupported.into());
        }
        selected[position] = true;
    }
    if !requested.is_empty() {
        let second = scan(
            &request.input,
            &mut entries,
            Some(&selected),
            &mut files,
            &mut budget,
        )?;
        if !bool::from(first[..].ct_eq(&second[..])) {
            return Err(Failure::Integrity.into());
        }
    }
    budget.check(Checkpoint::Finish)?;
    Ok(Completed {
        entries,
        files,
        usage: budget.usage,
        input_sha256: first,
        content_verification: ContentVerification::Unavailable,
    })
}

fn scan<C>(
    input: &[u8],
    entries: &mut Vec<Entry>,
    selected: Option<&[bool]>,
    files: &mut Vec<SelectedFile>,
    budget: &mut Budget<C>,
) -> Result<[u8; 32], Failure>
where
    C: FnMut(Checkpoint) -> Result<(), Stop>,
{
    let pass = if selected.is_some() { 2 } else { 1 };
    let mut hash = Sha256::new();
    let mut offset = 0;
    let mut metadata = 0;
    let mut index = 0;
    loop {
        let block = take(input, &mut offset, BLOCK, pass, &mut hash, budget)?;
        if header::zero(block) {
            let end = take(input, &mut offset, BLOCK, pass, &mut hash, budget)?;
            if !header::zero(end) {
                return Err(Failure::Integrity);
            }
            while offset < input.len() {
                let count = CHUNK.min(input.len() - offset);
                let trailing = take(input, &mut offset, count, pass, &mut hash, budget)?;
                if !header::zero(trailing) {
                    return Err(Failure::Integrity);
                }
            }
            break;
        }
        budget.check(Checkpoint::Entry { pass, index })?;
        if selected.is_none() {
            budget.entry()?;
        }
        if budget.limits.metadata - metadata < BLOCK as u64 {
            return Err(Failure::Budget);
        }
        metadata += BLOCK as u64;
        let entry = header::parse(block, index)?;
        if selected.is_some() && entries.get(index) != Some(&entry) {
            return Err(Failure::Integrity);
        }
        header::unique(&entry, &entries[..index])?;
        let remaining = (input.len() - offset) as u64;
        let padding = (BLOCK as u64 - entry.size % BLOCK as u64) % BLOCK as u64;
        if entry.size > remaining || padding > remaining - entry.size {
            return Err(Failure::Integrity);
        }
        let emit = selected.is_some_and(|ids| ids[index]) && entry.kind == Kind::File;
        let mut bytes = Vec::new();
        if emit {
            budget.output(entry.size)?;
            bytes
                .try_reserve_exact(entry.size as usize)
                .map_err(|_| Failure::Allocation)?;
        }
        let mut body = entry.size as usize;
        while body > 0 {
            let count = CHUNK.min(body);
            let chunk = take(input, &mut offset, count, pass, &mut hash, budget)?;
            if emit {
                bytes.extend_from_slice(chunk);
            }
            budget.check(Checkpoint::Read { pass, offset })?;
            body -= count;
        }
        if padding > 0 {
            let chunk = take(
                input,
                &mut offset,
                padding as usize,
                pass,
                &mut hash,
                budget,
            )?;
            if !header::zero(chunk) {
                return Err(Failure::Integrity);
            }
        }
        if emit {
            files.try_reserve(1).map_err(|_| Failure::Allocation)?;
            files.push(SelectedFile {
                id: header::copy(&entry.id)?,
                bytes,
            });
        }
        if selected.is_none() {
            entries.try_reserve(1).map_err(|_| Failure::Allocation)?;
            entries.push(entry);
        }
        index += 1;
    }
    budget.check(Checkpoint::Read { pass, offset })?;
    if selected.is_some() && entries.len() != index {
        return Err(Failure::Integrity);
    }
    Ok(hash.finalize().into())
}

fn take<'a, C>(
    input: &'a [u8],
    offset: &mut usize,
    count: usize,
    pass: u8,
    hash: &mut Sha256,
    budget: &mut Budget<C>,
) -> Result<&'a [u8], Failure>
where
    C: FnMut(Checkpoint) -> Result<(), Stop>,
{
    budget.check(Checkpoint::Read {
        pass,
        offset: *offset,
    })?;
    if count > input.len() - *offset {
        return Err(Failure::Integrity);
    }
    budget.processed(count as u64)?;
    let bytes = &input[*offset..*offset + count];
    hash.update(bytes);
    *offset += count;
    Ok(bytes)
}
