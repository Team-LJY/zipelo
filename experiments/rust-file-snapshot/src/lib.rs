#![deny(unsafe_op_in_unsafe_fn)]

use sha2::{Digest, Sha256};
use std::{
    collections::TryReserveError,
    io,
    path::Path,
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
mod linux;

pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_IO_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_READ_CALLS: u64 = 131_072;
pub const CHUNK_BYTES: usize = 65_536;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_bytes: u64,
    pub max_io_bytes: u64,
    pub max_read_calls: u64,
    pub max_duration: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: MAX_BYTES,
            max_io_bytes: MAX_IO_BYTES,
            max_read_calls: MAX_READ_CALLS,
            max_duration: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Usage {
    pub open_calls: u64,
    pub metadata_calls: u64,
    pub read_calls: u64,
    pub reserved_read_bytes: u64,
    pub actual_read_bytes: u64,
    pub compared_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    Opening,
    Pinned,
    Copying,
    Comparing,
    Finalizing,
}

#[derive(Clone, Copy, Debug)]
pub struct Checkpoint {
    pub stage: Stage,
    pub offset: u64,
    pub length: Option<u64>,
    pub usage: Usage,
}

#[derive(Debug)]
pub enum Stop<E> {
    Cancelled,
    Callback(E),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitKind {
    Bytes,
    IoBytes,
    ReadCalls,
    Deadline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Changed {
    Metadata,
    Contents,
    Length,
    Identity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unsupported {
    Platform,
    Procfs,
    FileType,
    MultipleLinks,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IoOperation {
    PinPath,
    Inspect,
    OpenProc,
    InspectProc,
    OpenProcDescriptors,
    ReopenPinned,
    Read,
}

#[derive(Debug)]
pub enum Error<E> {
    InvalidLimits,
    InvalidPath,
    Unsupported(Unsupported),
    Limit(LimitKind),
    Changed(Changed),
    Cancelled,
    Callback(E),
    Allocation(TryReserveError),
    Io {
        operation: IoOperation,
        source: io::Error,
    },
}

#[derive(Debug)]
pub struct Failure<E> {
    pub cause: Error<E>,
    pub usage: Usage,
}

/// 열린 파일의 관측값임. 경로 지속성이나 원자적 snapshot 증거가 아님.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    device: u64,
    inode: u64,
    length: u64,
    links: u64,
    mode: u32,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl Identity {
    pub fn device(&self) -> u64 {
        self.device
    }
    pub fn inode(&self) -> u64 {
        self.inode
    }
    pub fn length(&self) -> u64 {
        self.length
    }
    pub fn links(&self) -> u64 {
        self.links
    }
    pub fn mode(&self) -> u32 {
        self.mode
    }
    pub fn modified(&self) -> (i64, i64) {
        self.modified
    }
    pub fn changed(&self) -> (i64, i64) {
        self.changed
    }
}

/// 두 번 읽어 일치한 메모리 사본임. 동시 변경 전체 탐지나 게시 증거가 아님.
pub struct FileSnapshot {
    bytes: Vec<u8>,
    identity: Identity,
    sha256: [u8; 32],
    usage: Usage,
}

impl FileSnapshot {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn identity(&self) -> Identity {
        self.identity
    }
    pub fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }
    /// 원본 bytes를 이동하며 snapshot 증명 객체는 소비함.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl std::fmt::Debug for FileSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSnapshot")
            .field("length", &self.bytes.len())
            .field("usage", &self.usage)
            .finish_non_exhaustive()
    }
}

pub struct Request<'a> {
    pub path: &'a Path,
    pub limits: Limits,
}

struct Run {
    limits: Limits,
    usage: Usage,
    started: Instant,
}

impl Run {
    fn checkpoint<E>(
        &self,
        stage: Stage,
        offset: u64,
        length: Option<u64>,
        callback: &mut impl FnMut(Checkpoint) -> Result<(), Stop<E>>,
    ) -> Result<(), Error<E>> {
        callback(Checkpoint {
            stage,
            offset,
            length,
            usage: self.usage,
        })
        .map_err(|stop| match stop {
            Stop::Cancelled => Error::Cancelled,
            Stop::Callback(error) => Error::Callback(error),
        })?;
        if self.started.elapsed() >= self.limits.max_duration {
            return Err(Error::Limit(LimitKind::Deadline));
        }
        Ok(())
    }

    fn reserve_read<E>(&mut self, count: usize) -> Result<(), Error<E>> {
        if self.usage.read_calls >= self.limits.max_read_calls {
            return Err(Error::Limit(LimitKind::ReadCalls));
        }
        if count as u64 > self.limits.max_io_bytes - self.usage.reserved_read_bytes {
            return Err(Error::Limit(LimitKind::IoBytes));
        }
        self.usage.read_calls += 1;
        self.usage.reserved_read_bytes += count as u64;
        Ok(())
    }
}

/// 일반 파일을 처음부터 캡처함. callback과 파일 I/O의 강제 종료 시간은 보장하지 않음.
pub fn capture<E>(
    request: Request<'_>,
    mut checkpoint: impl FnMut(Checkpoint) -> Result<(), Stop<E>>,
) -> Result<FileSnapshot, Failure<E>> {
    let mut run = Run {
        limits: request.limits,
        usage: Usage::default(),
        started: Instant::now(),
    };
    let limits = request.limits;
    if limits.max_bytes > MAX_BYTES
        || limits.max_io_bytes > MAX_IO_BYTES
        || limits.max_read_calls == 0
        || limits.max_read_calls > MAX_READ_CALLS
        || limits.max_duration.is_zero()
        || limits.max_duration > Duration::from_secs(300)
    {
        return Err(Failure {
            cause: Error::InvalidLimits,
            usage: run.usage,
        });
    }
    #[cfg(target_os = "linux")]
    let result = linux::capture(request.path, &mut run, &mut checkpoint);
    #[cfg(not(target_os = "linux"))]
    let result: Result<FileSnapshot, Error<E>> = {
        let _ = (&request.path, &mut checkpoint);
        Err(Error::Unsupported(Unsupported::Platform))
    };
    result.map_err(|cause| Failure {
        cause,
        usage: run.usage,
    })
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
