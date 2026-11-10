use crate::stream::{Fault, Id, Limits, Stream, StreamEnd, StreamSummary};

pub const PROFILE: &str = "rust-zip32-v2-errors-1";
pub const WORKER_SOURCE: &str = "9626abbf22f3a4c66a996e64601694ed5fe98763";

#[derive(Clone, Debug)]
pub struct Binding {
    pub executable: String,
    pub expected_sha256: [u8; 32],
    pub observed_sha256: [u8; 32],
    pub pid: u32,
    pub source_head: String,
    pub invocation: String,
}

impl Binding {
    fn valid(&self) -> bool {
        !self.executable.is_empty()
            && self.executable.len() <= 4096
            && self.pid != 0
            && self.expected_sha256 != [0; 32]
            && self.expected_sha256 == self.observed_sha256
            && self.source_head == WORKER_SOURCE
            && self.invocation == "--extract-stream-v2"
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostCause {
    Cancelled,
    Deadline,
    Budget,
    Io,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerReport {
    Unsupported,
    UnsupportedEntry,
    Integrity,
    Budget,
    Protocol,
    ObservedUnexpectedEof,
    UnknownEngineCause,
    DuplicateName,
    MetadataMismatch,
}

fn worker_report(code: i32) -> Option<WorkerReport> {
    Some(match code {
        10 => WorkerReport::Unsupported,
        11 => WorkerReport::UnsupportedEntry,
        12 => WorkerReport::Integrity,
        13 => WorkerReport::Budget,
        14 => WorkerReport::Protocol,
        15 => WorkerReport::ObservedUnexpectedEof,
        16 => WorkerReport::UnknownEngineCause,
        17 => WorkerReport::DuplicateName,
        18 => WorkerReport::MetadataMismatch,
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cause {
    RawFrame(Fault),
    SupervisorHost(HostCause),
    SupervisorOrder,
    WorkerReported(WorkerReport),
    UnknownExit(i32),
    CompletionExitContradiction,
    MissingCompletion,
    ForcedOrSignalledExit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Exit {
    Unknown,
    Natural(i32),
    AfterHostStop(i32),
    ForcedOrSignalled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    PendingOrUnconfirmed,
    ProtocolCompletionObserved,
    Failed(Cause),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub outcome: Outcome,
    pub stop_required: bool,
    pub stream: StreamSummary,
    pub exit: Exit,
    pub binding_attested: bool,
    pub stdout_eof_attested: bool,
    pub io_settled_attested: bool,
    pub cleanup_confirmed_attested: bool,
    pub cleanup_failed: bool,
    pub supervisor_invalid: bool,
}

pub enum Event<'a> {
    Stdout(&'a [u8]),
    StdoutEof,
    HostFailure(HostCause),
    NaturalExit(i32),
    ForcedOrSignalledExit,
    IoSettled,
    CleanupComplete,
    CleanupFailed,
}

pub struct Session {
    job: Id,
    stream: Stream,
    binding: Option<Binding>,
    next_event: u64,
    first: Option<Cause>,
    exit: Exit,
    stdout_eof: bool,
    io_settled: bool,
    cleanup_complete: bool,
    cleanup_failed: bool,
    supervisor_invalid: bool,
    host_stopped: bool,
}

impl Session {
    // 실행파일 해시와 PID는 감독자가 제공한 증거임. 이 모듈은 프로세스를 실행하지 않음.
    pub fn new(job: Id, limits: Limits, binding: Option<Binding>) -> Result<Self, Fault> {
        if binding.as_ref().is_some_and(|value| !value.valid()) {
            return Err(Fault::Header);
        }
        Ok(Self {
            job,
            stream: Stream::new(job, limits)?,
            binding,
            next_event: 0,
            first: None,
            exit: Exit::Unknown,
            stdout_eof: false,
            io_settled: false,
            cleanup_complete: false,
            cleanup_failed: false,
            supervisor_invalid: false,
            host_stopped: false,
        })
    }

    fn latch(&mut self, cause: Cause) {
        self.first.get_or_insert(cause);
    }

    fn invalid_supervisor(&mut self) {
        self.supervisor_invalid = true;
        self.latch(Cause::SupervisorOrder);
    }

    pub fn observe(&mut self, job: Id, sequence: u64, event: Event<'_>) {
        if job != self.job || sequence != self.next_event || self.next_event >= 131_072 {
            self.invalid_supervisor();
            return;
        }
        self.next_event += 1;
        if self.supervisor_invalid {
            return;
        }
        match event {
            Event::Stdout(bytes) => {
                if self.stdout_eof || self.io_settled || self.cleanup_complete {
                    self.invalid_supervisor();
                    return;
                }
                if bytes.len() > 65_536 {
                    self.latch(Cause::RawFrame(Fault::Budget));
                    self.supervisor_invalid = true;
                    return;
                }
                if let Err(fault) = self.stream.feed(bytes) {
                    self.latch(Cause::RawFrame(fault));
                }
            }
            Event::StdoutEof => {
                if self.stdout_eof || self.io_settled {
                    self.invalid_supervisor();
                    return;
                }
                self.stdout_eof = true;
                if let Some(fault) = self.stream.finish_input().fault {
                    self.latch(Cause::RawFrame(fault));
                }
            }
            Event::HostFailure(cause) => {
                // 종료 과정의 취소와 I/O 오류로 최초 원인을 덮지 않음.
                self.latch(Cause::SupervisorHost(cause));
                self.host_stopped = true;
            }
            Event::NaturalExit(code) => {
                if self.exit != Exit::Unknown || !(0..=255).contains(&code) {
                    self.invalid_supervisor();
                    return;
                }
                self.exit = if self.host_stopped {
                    Exit::AfterHostStop(code)
                } else {
                    Exit::Natural(code)
                };
            }
            Event::ForcedOrSignalledExit => {
                if self.exit != Exit::Unknown {
                    self.invalid_supervisor();
                    return;
                }
                self.exit = Exit::ForcedOrSignalled;
                self.latch(Cause::ForcedOrSignalledExit);
            }
            Event::IoSettled => {
                if self.io_settled {
                    self.invalid_supervisor();
                    return;
                }
                self.io_settled = true;
            }
            Event::CleanupComplete => {
                if self.exit == Exit::Unknown || !self.io_settled || self.cleanup_complete {
                    self.invalid_supervisor();
                    return;
                }
                self.cleanup_complete = true;
            }
            Event::CleanupFailed => self.cleanup_failed = true,
        }
        self.resolve_failure();
    }

    fn resolve_failure(&mut self) {
        if self.first.is_some() || !self.stdout_eof {
            return;
        }
        let Exit::Natural(code) = self.exit else {
            return;
        };
        let summary = self.stream.summary();
        if summary.completed && code != 0 {
            self.latch(Cause::CompletionExitContradiction);
        } else if code == 0 && !summary.completed {
            self.latch(Cause::MissingCompletion);
        } else if code != 0 && self.binding.is_some() && summary.end == StreamEnd::Boundary {
            self.latch(match worker_report(code) {
                Some(report) => Cause::WorkerReported(report),
                None => Cause::UnknownExit(code),
            });
        }
    }

    pub fn report(&self) -> Report {
        let stream = self.stream.summary();
        let stop_required = self.binding.is_none()
            || (!self.stdout_eof && self.first.is_none())
            || self.exit == Exit::Unknown
            || !self.io_settled
            || !self.cleanup_complete
            || self.cleanup_failed
            || self.supervisor_invalid;
        let outcome = if let Some(cause) = self.first {
            Outcome::Failed(cause)
        } else if !stop_required
            && self.binding.is_some()
            && self.stdout_eof
            && self.exit == Exit::Natural(0)
            && stream.completed
            && stream.fault.is_none()
            && stream.end == StreamEnd::Boundary
        {
            Outcome::ProtocolCompletionObserved
        } else {
            Outcome::PendingOrUnconfirmed
        };
        Report {
            outcome,
            stop_required,
            stream,
            exit: self.exit,
            binding_attested: self.binding.is_some(),
            stdout_eof_attested: self.stdout_eof,
            io_settled_attested: self.io_settled,
            cleanup_confirmed_attested: self.cleanup_complete,
            cleanup_failed: self.cleanup_failed,
            supervisor_invalid: self.supervisor_invalid,
        }
    }
}
