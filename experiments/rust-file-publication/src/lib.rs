#![deny(unsafe_op_in_unsafe_fn)]

//! 파일 계층의 단일 이름 게시만 담당함.
//! 원래 경로 유지, 압축 검증, OS worker 실행과 원자적 입력 snapshot을 증명하지 않음.

use std::{
    ffi::OsStr,
    io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
mod linux;

pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_IO_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_IO_CALLS: u64 = 131_072;
pub const CHUNK_BYTES: usize = 65_536;
const PENDING: u8 = 0;
const CANCELLED: u8 = 1;
const COMMITTING: u8 = 2;
const FINISHED: u8 = 3;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_bytes: u64,
    pub max_io_bytes: u64,
    pub max_io_calls: u64,
    pub max_duration: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: MAX_BYTES,
            max_io_bytes: MAX_IO_BYTES,
            max_io_calls: MAX_IO_CALLS,
            max_duration: Duration::from_secs(30),
        }
    }
}

pub struct Request<'a> {
    pub parent: &'a Path,
    pub leaf: &'a OsStr,
    pub bytes: &'a [u8],
    pub limits: Limits,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Usage {
    pub open_calls: u64,
    pub metadata_calls: u64,
    pub sync_calls: u64,
    pub link_calls: u64,
    pub read_calls: u64,
    pub write_calls: u64,
    pub reserved_io_bytes: u64,
    pub actual_read_bytes: u64,
    pub actual_written_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    Opening,
    Writing,
    Verifying,
    BeforeCommit,
}

#[derive(Clone, Copy, Debug)]
pub struct Checkpoint {
    pub stage: Stage,
    pub offset: u64,
    pub length: u64,
    pub usage: Usage,
}

#[derive(Debug)]
pub enum Stop<E> {
    Cancelled,
    Callback(E),
}

#[derive(Debug)]
pub enum Error<E> {
    InvalidLimits,
    InvalidPath,
    LimitExceeded,
    DeadlineExceeded,
    Cancelled,
    Callback(E),
    UnsupportedPlatform,
    UnsupportedFilesystem,
    UntrustedParent,
    InvalidTemporaryFile,
    VerificationMismatch,
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

// 재사용이나 초기화 API를 제공하지 않는 작업별 취소 핸들임.
#[derive(Clone)]
pub struct Cancellation {
    state: Arc<AtomicU8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancelDecision {
    Requested,
    AlreadyRequested,
    TooLate,
    Finished,
}

impl Cancellation {
    pub fn request(&self) -> CancelDecision {
        match self
            .state
            .compare_exchange(PENDING, CANCELLED, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => CancelDecision::Requested,
            Err(CANCELLED) => CancelDecision::AlreadyRequested,
            Err(COMMITTING) => CancelDecision::TooLate,
            Err(_) => CancelDecision::Finished,
        }
    }
}

pub struct Publication {
    state: Arc<AtomicU8>,
}

impl Publication {
    pub fn new() -> (Self, Cancellation) {
        let state = Arc::new(AtomicU8::new(PENDING));
        (
            Self {
                state: state.clone(),
            },
            Cancellation { state },
        )
    }

    // 반환 receipt는 파일 이름 생성만 증명하며 내용의 업무상 유효성을 증명하지 않음.
    pub fn publish<E>(
        self,
        request: Request<'_>,
        mut checkpoint: impl FnMut(Checkpoint) -> Result<(), Stop<E>>,
    ) -> Report<E> {
        let mut run = Run {
            limits: request.limits,
            usage: Usage::default(),
            started: Instant::now(),
            state: &self.state,
            length: request.bytes.len() as u64,
        };
        if let Err(error) = run.validate() {
            return Report {
                outcome: Outcome::NotCommitted(error),
                usage: run.usage,
                cleanup: Cleanup::empty(),
            };
        }
        #[cfg(target_os = "linux")]
        {
            linux::publish(request, &mut run, &mut checkpoint)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = &mut checkpoint;
            Report {
                outcome: Outcome::NotCommitted(Error::UnsupportedPlatform),
                usage: run.usage,
                cleanup: Cleanup::empty(),
            }
        }
    }
}

impl Drop for Publication {
    fn drop(&mut self) {
        self.state.store(FINISHED, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    device: u64,
    inode: u64,
}

impl Identity {
    pub fn device(&self) -> u64 {
        self.device
    }
    pub fn inode(&self) -> u64 {
        self.inode
    }
}

// 외부 생성자와 역직렬화를 제공하지 않음.
pub struct FileCommitReceipt {
    parent: Identity,
    file: Identity,
    length: u64,
}

impl FileCommitReceipt {
    pub fn parent_identity(&self) -> Identity {
        self.parent
    }
    pub fn file_identity(&self) -> Identity {
        self.file
    }
    pub fn length(&self) -> u64 {
        self.length
    }
}

#[derive(Debug)]
pub enum Durability {
    FileAndParentSynced,
    ParentSyncFailed(io::Error),
}

pub enum Outcome<E> {
    NotCommitted(Error<E>),
    // 링크 호출 실패의 원격 또는 파일시스템 부작용을 추측하지 않음.
    CommitUncertain {
        source: io::Error,
    },
    Committed {
        receipt: FileCommitReceipt,
        durability: Durability,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resource {
    TemporaryFile,
    ProcFdDirectory,
    ProcRoot,
    ParentDirectory,
}

#[derive(Debug)]
pub struct CloseFailure {
    pub resource: Resource,
    pub source: io::Error,
}

#[derive(Debug)]
pub struct Cleanup {
    pub attempted: u8,
    pub failures: [Option<CloseFailure>; 4],
}

impl Cleanup {
    fn empty() -> Self {
        Self {
            attempted: 0,
            failures: [None, None, None, None],
        }
    }

    // 성공한 close 호출만 뜻하며 물리 삭제나 저장 내구성 증거가 아님.
    pub fn all_close_calls_succeeded(&self) -> bool {
        self.failures.iter().all(Option::is_none)
    }
}

pub struct Report<E> {
    pub outcome: Outcome<E>,
    pub usage: Usage,
    pub cleanup: Cleanup,
}

struct Run<'a> {
    limits: Limits,
    usage: Usage,
    started: Instant,
    state: &'a AtomicU8,
    length: u64,
}

impl Run<'_> {
    fn validate<E>(&self) -> Result<(), Error<E>> {
        if self.limits.max_bytes > MAX_BYTES
            || self.limits.max_io_bytes > MAX_IO_BYTES
            || self.limits.max_io_calls > MAX_IO_CALLS
            || self.limits.max_duration.is_zero()
            || self.limits.max_duration > Duration::from_secs(300)
        {
            return Err(Error::InvalidLimits);
        }
        if self.length > self.limits.max_bytes {
            return Err(Error::LimitExceeded);
        }
        Ok(())
    }

    fn check<E>(
        &self,
        stage: Stage,
        offset: usize,
        callback: &mut impl FnMut(Checkpoint) -> Result<(), Stop<E>>,
    ) -> Result<(), Error<E>> {
        match callback(Checkpoint {
            stage,
            offset: offset as u64,
            length: self.length,
            usage: self.usage,
        }) {
            Err(Stop::Callback(error)) => return Err(Error::Callback(error)),
            Err(Stop::Cancelled) => return Err(Error::Cancelled),
            Ok(()) => {}
        }
        if self.state.load(Ordering::SeqCst) == CANCELLED {
            return Err(Error::Cancelled);
        }
        if self.started.elapsed() >= self.limits.max_duration {
            return Err(Error::DeadlineExceeded);
        }
        Ok(())
    }

    fn reserve<E>(&mut self, count: usize, write: bool) -> Result<(), Error<E>> {
        let total = self
            .usage
            .reserved_io_bytes
            .checked_add(count as u64)
            .ok_or(Error::LimitExceeded)?;
        if total > self.limits.max_io_bytes
            || self.usage.read_calls + self.usage.write_calls >= self.limits.max_io_calls
        {
            return Err(Error::LimitExceeded);
        }
        self.usage.reserved_io_bytes = total;
        if write {
            self.usage.write_calls += 1;
        } else {
            self.usage.read_calls += 1;
        }
        Ok(())
    }

    fn begin_commit<E>(&self) -> Result<(), Error<E>> {
        self.state
            .compare_exchange(PENDING, COMMITTING, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| ())
            .map_err(|_| Error::Cancelled)
    }
}
