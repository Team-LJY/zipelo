use std::time::Duration;

use crate::wire::{
    CHUNK_BYTES, HEADER_BYTES, Header, Kind, ProtocolError, RequestId, WorkerStatus, encode_header,
};

pub const MAX_DATA_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_WIRE_BYTES: u64 = 21 * 1024 * 1024;
pub const MAX_FRAMES: u64 = 65_536;
pub const MAX_OBSERVATIONS: u64 = 131_072;
pub const MAX_DIAGNOSTIC_BYTES: u64 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
    pub max_wire_bytes_per_direction: u64,
    pub max_frames_per_direction: u64,
    pub max_observations: u64,
    pub operation_duration: Duration,
    pub cleanup_duration: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_bytes: MAX_DATA_BYTES,
            max_output_bytes: MAX_DATA_BYTES,
            max_wire_bytes_per_direction: MAX_WIRE_BYTES,
            max_frames_per_direction: MAX_FRAMES,
            max_observations: MAX_OBSERVATIONS,
            operation_duration: Duration::from_secs(300),
            cleanup_duration: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    InvalidLimits,
    Protocol(ProtocolError),
    RequestIncomplete,
    Cancelled,
    Deadline,
    Io,
    DiagnosticExceeded,
    ObservationOrder,
    ObservationLimit,
    ClockReversed,
    CleanupExpired,
    StopFailed,
    ExitContradiction,
    MissingCompletion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Request,
    Result,
    Diagnostic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    Code(u32),
    Signal(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    SendingRequest,
    ReceivingResult,
    Draining,
    Stopping,
    SettledModel,
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pending,
    ProtocolCompletionObserved,
    WorkerReported(WorkerStatus),
    Failed(Fault),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyError {
    Fault(Fault),
    AlreadySettled,
    UnreliableObservations,
}

#[derive(Clone, Copy, Debug)]
pub enum Observation<'a> {
    ResultFrame {
        header: &'a [u8; HEADER_BYTES],
        payload: &'a [u8],
    },
    DiagnosticBytes(&'a [u8]),
    ResultEof,
    DiagnosticEof,
    Exit(ExitStatus),
    Reaped(ExitStatus),
    Closed(Channel),
    ResourcesReleased,
    StopAttempted {
        accepted: bool,
    },
    TruncatedFrame,
    IoFailure,
    Cancel,
    Tick,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub request_frames: u64,
    pub request_wire_bytes: u64,
    pub result_frames: u64,
    pub result_wire_bytes: u64,
    pub result_data_bytes: u64,
    pub diagnostic_bytes: u64,
    pub observations: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Missing {
    pub result_eof: bool,
    pub diagnostic_eof: bool,
    pub exit: bool,
    pub reap: bool,
    pub request_closed: bool,
    pub result_closed: bool,
    pub diagnostic_closed: bool,
    pub resources_released: bool,
}

impl Missing {
    fn any(self) -> bool {
        self.result_eof
            || self.diagnostic_eof
            || self.exit
            || self.reap
            || self.request_closed
            || self.result_closed
            || self.diagnostic_closed
            || self.resources_released
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelReport {
    pub request: RequestId,
    pub phase: Phase,
    pub outcome: Outcome,
    pub first_fault: Option<Fault>,
    pub worker_report: Option<WorkerStatus>,
    pub observed_exit: Option<ExitStatus>,
    pub missing: Missing,
    pub usage: Usage,
    pub observations_unreliable: bool,
    pub stop_needed_in_model: bool,
}

impl ModelReport {
    pub fn live_verified(&self) -> bool {
        false
    }

    pub fn publication_allowed(&self) -> bool {
        false
    }
}

pub struct OutboundFrame<'a> {
    pub header: [u8; HEADER_BYTES],
    pub payload: &'a [u8],
}

// snapshot과 목적지는 호스트가 소유함. 이 모델은 경로와 게시 객체를 받지 않음.
pub struct SessionModel<'a> {
    id: RequestId,
    input: &'a [u8],
    limits: Limits,
    begin_payload: [u8; 16],
    sent_begin: bool,
    sent_end: bool,
    input_offset: usize,
    declared_output: Option<u64>,
    terminal: Option<WorkerStatus>,
    usage: Usage,
    result_eof: bool,
    diagnostic_eof: bool,
    exit: Option<ExitStatus>,
    reaped: bool,
    closed: [bool; 3],
    released: bool,
    stop_attempted: bool,
    last_time: Duration,
    cleanup_started: Option<Duration>,
    first_fault: Option<Fault>,
    unreliable: bool,
}

impl<'a> SessionModel<'a> {
    pub fn new(id: RequestId, input: &'a [u8], limits: Limits) -> Result<Self, Fault> {
        let frames = (input.len() as u64).div_ceil(CHUNK_BYTES as u64) + 2;
        let wire = input.len() as u64 + 16 + frames * HEADER_BYTES as u64;
        if limits.max_input_bytes > MAX_DATA_BYTES
            || limits.max_output_bytes > MAX_DATA_BYTES
            || input.len() as u64 > limits.max_input_bytes
            || !(129..=MAX_WIRE_BYTES).contains(&limits.max_wire_bytes_per_direction)
            || !(2..=MAX_FRAMES).contains(&limits.max_frames_per_direction)
            || !(1..=MAX_OBSERVATIONS).contains(&limits.max_observations)
            || limits.operation_duration.is_zero()
            || limits.operation_duration > Duration::from_secs(300)
            || limits.cleanup_duration.is_zero()
            || limits.cleanup_duration > Duration::from_secs(30)
            || frames > limits.max_frames_per_direction
            || wire > limits.max_wire_bytes_per_direction
        {
            return Err(Fault::InvalidLimits);
        }
        let mut begin_payload = [0; 16];
        begin_payload[..8].copy_from_slice(&(input.len() as u64).to_le_bytes());
        begin_payload[8..].copy_from_slice(&limits.max_output_bytes.to_le_bytes());
        Ok(Self {
            id,
            input,
            limits,
            begin_payload,
            sent_begin: false,
            sent_end: false,
            input_offset: 0,
            declared_output: None,
            terminal: None,
            usage: Usage::default(),
            result_eof: false,
            diagnostic_eof: false,
            exit: None,
            reaped: false,
            closed: [false; 3],
            released: false,
            stop_attempted: false,
            last_time: Duration::ZERO,
            cleanup_started: None,
            first_fault: None,
            unreliable: false,
        })
    }

    // 반환 프레임은 불변 입력을 빌림. 부분 I/O 재조립은 향후 adapter 책임임.
    pub fn outbound(&self) -> Option<OutboundFrame<'_>> {
        if self.sent_end
            || self.first_fault.is_some()
            || self.unreliable
            || self.closed[0]
            || self.exit.is_some()
        {
            return None;
        }
        let (kind, payload) = if !self.sent_begin {
            (Kind::RequestBegin, self.begin_payload.as_slice())
        } else if self.input_offset < self.input.len() {
            let end = self
                .input_offset
                .saturating_add(CHUNK_BYTES)
                .min(self.input.len());
            (Kind::RequestData, &self.input[self.input_offset..end])
        } else {
            (Kind::RequestEnd, &[][..])
        };
        Some(OutboundFrame {
            header: encode_header(
                kind,
                self.id,
                self.usage.request_frames,
                payload.len() as u32,
            ),
            payload,
        })
    }

    // 실제 write 증명이 아닌 완전 프레임 송신 관측임.
    pub fn request_frame_sent(&mut self, at: Duration, sequence: u64) -> Result<(), ApplyError> {
        self.guard(at)?;
        let Some(frame) = self.outbound() else {
            return Err(self.reject(Fault::ObservationOrder, true));
        };
        if sequence != self.usage.request_frames {
            return Err(self.reject(Fault::Protocol(ProtocolError::SequenceMismatch), false));
        }
        let payload_len = frame.payload.len();
        self.usage.request_frames += 1;
        self.usage.request_wire_bytes += HEADER_BYTES as u64 + payload_len as u64;
        if !self.sent_begin {
            self.sent_begin = true;
        } else if self.input_offset < self.input.len() {
            self.input_offset += payload_len;
        } else {
            self.sent_end = true;
        }
        Ok(())
    }

    fn reject(&mut self, fault: Fault, unreliable: bool) -> ApplyError {
        self.first_fault.get_or_insert(fault);
        self.cleanup_started.get_or_insert(self.last_time);
        self.unreliable |= unreliable;
        ApplyError::Fault(fault)
    }

    fn guard(&mut self, at: Duration) -> Result<(), ApplyError> {
        if self.unreliable {
            return Err(ApplyError::UnreliableObservations);
        }
        if self.report().phase == Phase::SettledModel {
            return Err(ApplyError::AlreadySettled);
        }
        if at < self.last_time {
            return Err(self.reject(Fault::ClockReversed, true));
        }
        self.last_time = at;
        if self.usage.observations >= self.limits.max_observations {
            return Err(self.reject(Fault::ObservationLimit, true));
        }
        self.usage.observations += 1;
        if let Some(started) = self.cleanup_started {
            if at - started >= self.limits.cleanup_duration {
                return Err(self.reject(Fault::CleanupExpired, true));
            }
        } else if at >= self.limits.operation_duration {
            self.reject(Fault::Deadline, false);
        }
        Ok(())
    }

    fn accept_result(&mut self, bytes: &[u8; HEADER_BYTES], payload: &[u8]) -> Result<(), Fault> {
        if self.result_eof || self.terminal.is_some() {
            return Err(Fault::Protocol(ProtocolError::TrailingData));
        }
        let cost = HEADER_BYTES as u64 + payload.len() as u64;
        if cost > self.limits.max_wire_bytes_per_direction - self.usage.result_wire_bytes {
            return Err(Fault::Protocol(ProtocolError::WireExceeded));
        }
        self.usage.result_wire_bytes += cost;
        let header = Header::inspect(bytes).map_err(Fault::Protocol)?;
        if header.payload_length() != payload.len() {
            return Err(Fault::Protocol(ProtocolError::InvalidLength));
        }
        if header.id() != self.id {
            return Err(Fault::Protocol(ProtocolError::IdMismatch));
        }
        if header.sequence() != self.usage.result_frames {
            return Err(Fault::Protocol(ProtocolError::SequenceMismatch));
        }
        if self.usage.result_frames >= self.limits.max_frames_per_direction {
            return Err(Fault::Protocol(ProtocolError::FramesExceeded));
        }
        self.usage.result_frames += 1;
        match header.kind() {
            Kind::ResultBegin if self.sent_end && self.declared_output.is_none() => {
                let length =
                    u64::from_le_bytes(payload.try_into().expect("validated begin length"));
                if length > self.limits.max_output_bytes {
                    return Err(Fault::Protocol(ProtocolError::DataExceeded));
                }
                self.declared_output = Some(length);
            }
            Kind::ResultData if self.declared_output.is_some() => {
                let left = self.declared_output.expect("checked declaration")
                    - self.usage.result_data_bytes;
                if payload.len() as u64 > left {
                    return Err(Fault::Protocol(ProtocolError::DataExceeded));
                }
                self.usage.result_data_bytes += payload.len() as u64;
            }
            Kind::ResultEnd if self.declared_output.is_some() => {
                let status = WorkerStatus::parse(payload[0]).map_err(Fault::Protocol)?;
                if status == WorkerStatus::Completed
                    && self.declared_output != Some(self.usage.result_data_bytes)
                {
                    return Err(Fault::Protocol(ProtocolError::Truncated));
                }
                self.terminal = Some(status);
            }
            _ => return Err(Fault::Protocol(ProtocolError::UnexpectedFrame)),
        }
        Ok(())
    }

    pub fn observe(
        &mut self,
        at: Duration,
        observation: Observation<'_>,
    ) -> Result<(), ApplyError> {
        self.guard(at)?;
        match observation {
            Observation::ResultFrame { header, payload } => {
                if self.first_fault.is_some() {
                    return Err(ApplyError::Fault(self.first_fault.expect("latched fault")));
                }
                if let Err(fault) = self.accept_result(header, payload) {
                    return Err(self.reject(fault, false));
                }
            }
            Observation::DiagnosticBytes(bytes) => {
                if self.diagnostic_eof || self.closed[2] || bytes.is_empty() {
                    return Err(self.reject(Fault::ObservationOrder, true));
                }
                if bytes.len() as u64 > MAX_DIAGNOSTIC_BYTES - self.usage.diagnostic_bytes {
                    return Err(self.reject(Fault::DiagnosticExceeded, false));
                }
                self.usage.diagnostic_bytes += bytes.len() as u64;
            }
            Observation::ResultEof => {
                if self.result_eof || self.closed[1] {
                    return Err(self.reject(Fault::ObservationOrder, true));
                }
                self.result_eof = true;
                if self.terminal.is_none() {
                    self.reject(Fault::Protocol(ProtocolError::Truncated), false);
                }
            }
            Observation::DiagnosticEof => {
                if self.diagnostic_eof || self.closed[2] {
                    return Err(self.reject(Fault::ObservationOrder, true));
                }
                self.diagnostic_eof = true;
            }
            Observation::Exit(status) => {
                if self.exit.is_some() || matches!(status, ExitStatus::Signal(0)) {
                    return Err(self.reject(Fault::ObservationOrder, true));
                }
                self.exit = Some(status);
                self.cleanup_started.get_or_insert(at);
                if !self.sent_end {
                    self.reject(Fault::RequestIncomplete, false);
                }
            }
            Observation::Reaped(status) => {
                if self.reaped || self.exit != Some(status) {
                    return Err(self.reject(Fault::ObservationOrder, true));
                }
                self.reaped = true;
            }
            Observation::Closed(channel) => {
                let index = match channel {
                    Channel::Request => 0,
                    Channel::Result => 1,
                    Channel::Diagnostic => 2,
                };
                if self.closed[index]
                    || (index == 1 && !self.result_eof)
                    || (index == 2 && !self.diagnostic_eof)
                {
                    return Err(self.reject(Fault::ObservationOrder, true));
                }
                self.closed[index] = true;
                if index == 0 && !self.sent_end {
                    self.reject(Fault::RequestIncomplete, false);
                }
            }
            Observation::ResourcesReleased => {
                if self.released || !self.reaped || !self.closed.iter().all(|closed| *closed) {
                    return Err(self.reject(Fault::ObservationOrder, true));
                }
                self.released = true;
            }
            Observation::StopAttempted { accepted } => {
                if self.first_fault.is_none()
                    || self.stop_attempted
                    || self.reaped
                    || self.exit.is_some()
                {
                    return Err(self.reject(Fault::ObservationOrder, true));
                }
                self.stop_attempted = true;
                if !accepted {
                    return Err(self.reject(Fault::StopFailed, true));
                }
            }
            Observation::TruncatedFrame => {
                return Err(self.reject(Fault::Protocol(ProtocolError::Truncated), false));
            }
            Observation::IoFailure => {
                return Err(self.reject(Fault::Io, false));
            }
            Observation::Cancel => {
                self.reject(Fault::Cancelled, false);
            }
            Observation::Tick => {}
        }
        if self.result_eof && self.exit.is_some() && self.first_fault.is_none() {
            match (self.terminal, self.exit) {
                (Some(WorkerStatus::Completed), Some(ExitStatus::Code(0))) => {}
                (
                    Some(WorkerStatus::Failed | WorkerStatus::Cancelled),
                    Some(ExitStatus::Code(code)),
                ) if code != 0 => {}
                (None, _) => {
                    self.reject(Fault::MissingCompletion, false);
                }
                _ => {
                    self.reject(Fault::ExitContradiction, false);
                }
            }
        }
        Ok(())
    }

    // 모순 없는 합성 관측의 정착만 보고함. OS 실행과 게시 증명을 생성하지 않음.
    pub fn report(&self) -> ModelReport {
        let missing = Missing {
            result_eof: !self.result_eof,
            diagnostic_eof: !self.diagnostic_eof,
            exit: self.exit.is_none(),
            reap: !self.reaped,
            request_closed: !self.closed[0],
            result_closed: !self.closed[1],
            diagnostic_closed: !self.closed[2],
            resources_released: !self.released,
        };
        let settled = !missing.any() && !self.unreliable;
        let phase = if self.unreliable {
            Phase::Blocked
        } else if settled {
            Phase::SettledModel
        } else if self.first_fault.is_some() {
            Phase::Stopping
        } else if self.exit.is_some() {
            Phase::Draining
        } else if self.sent_end {
            Phase::ReceivingResult
        } else {
            Phase::SendingRequest
        };
        let outcome = match self.first_fault {
            Some(fault) => Outcome::Failed(fault),
            None if settled => match self.terminal {
                Some(WorkerStatus::Completed) => Outcome::ProtocolCompletionObserved,
                Some(status) => Outcome::WorkerReported(status),
                None => Outcome::Failed(Fault::MissingCompletion),
            },
            None => Outcome::Pending,
        };
        ModelReport {
            request: self.id,
            phase,
            outcome,
            first_fault: self.first_fault,
            worker_report: self.terminal,
            observed_exit: self.exit,
            missing,
            usage: self.usage,
            observations_unreliable: self.unreliable,
            stop_needed_in_model: self.first_fault.is_some()
                && self.exit.is_none()
                && !self.reaped
                && !self.stop_attempted,
        }
    }
}
