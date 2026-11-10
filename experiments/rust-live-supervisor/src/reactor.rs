use crate::range::{Config, RangeFault, RangeLimits, RangeMachine, Stats};
use std::time::Duration;
use zipelo_rust_host_errors::{Cause, Event, HostCause, Limits, Outcome, Report, Session};

const IO_QUOTA: usize = 16_384;
const POLL_SLICE: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Launching,
    Running,
    Draining,
    Stopping,
    SettledModel,
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    Cancelled,
    Deadline(Phase),
    Range(RangeFault),
    Raw(Cause),
    Io,
    ExecStatus,
    ExecErrno(i32),
    ReactorOrder,
    ReapUnconfirmed,
    CleanupUnconfirmed,
}

#[derive(Clone, Copy, Debug)]
pub struct Deadlines {
    pub launch: Duration,
    pub operation: Duration,
    pub cleanup: Duration,
}

impl Deadlines {
    fn valid(self) -> bool {
        [self.launch, self.operation, self.cleanup]
            .into_iter()
            .all(|value| !value.is_zero() && value <= Duration::from_secs(300))
    }
}

#[derive(Clone, Copy, Debug)]
pub enum WaitObservation {
    Exited(u8),
    Signalled,
}

#[derive(Clone, Copy, Debug)]
pub struct PollPlan {
    pub stdout_max: usize,
    pub stderr_max: usize,
    pub stdin_max: usize,
    pub exec_status_max: usize,
    pub timeout: Duration,
    pub request_kill: bool,
    pub close_stdin: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ModelReport {
    pub phase: Phase,
    pub first: Option<Reason>,
    pub cleanup_failed: bool,
    pub owned_child_reaped: bool,
    pub range: Stats,
    pub protocol: Report,
    pub live_verified: bool,
    pub publication: bool,
    pub stop_required: bool,
}

pub struct Reactor {
    job: [u8; 16],
    session: Session,
    event_sequence: u64,
    range: RangeMachine,
    deadlines: Deadlines,
    now: Duration,
    deadline: Duration,
    phase: Phase,
    first: Option<Reason>,
    stdout_eof: bool,
    stderr_eof: bool,
    stdin_closed: bool,
    exec: [u8; 4],
    exec_used: usize,
    exec_eof: bool,
    reaped: bool,
    kill_requested: bool,
    cleanup_failed: bool,
    closed_fds: bool,
    stdout_quota: usize,
    stderr_quota: usize,
    stdin_quota: usize,
    exec_quota: usize,
}

impl Reactor {
    // OS adapter가 없는 순수 모델임. 호출자가 준 관측을 실제 실행 증거로 승격하지 않음.
    pub fn new(
        snapshot: Box<[u8]>,
        config: Config,
        limits: RangeLimits,
        deadlines: Deadlines,
    ) -> Result<Self, RangeFault> {
        if !deadlines.valid() || config.duration > deadlines.operation {
            return Err(RangeFault::Config);
        }
        let session = Session::new(
            config.job,
            Limits {
                output_bytes: config.output,
                items: config.entries as u32,
                ..Limits::default()
            },
            None,
        )
        .map_err(|_| RangeFault::Config)?;
        let range = RangeMachine::new(snapshot, config, limits)?;
        Ok(Self {
            job: config.job,
            session,
            event_sequence: 0,
            range,
            deadlines,
            now: Duration::ZERO,
            deadline: deadlines.launch,
            phase: Phase::Launching,
            first: None,
            stdout_eof: false,
            stderr_eof: false,
            stdin_closed: false,
            exec: [0; 4],
            exec_used: 0,
            exec_eof: false,
            reaped: false,
            kill_requested: false,
            cleanup_failed: false,
            closed_fds: false,
            stdout_quota: 0,
            stderr_quota: 0,
            stdin_quota: 0,
            exec_quota: 0,
        })
    }

    fn event(&mut self, event: Event<'_>) {
        self.session.observe(self.job, self.event_sequence, event);
        self.event_sequence = self.event_sequence.saturating_add(1);
    }

    fn cleanup_failure(&mut self) {
        if !self.cleanup_failed {
            self.event(Event::CleanupFailed);
        }
        self.cleanup_failed = true;
        self.first.get_or_insert(Reason::CleanupUnconfirmed);
        self.phase = Phase::Blocked;
    }

    fn fail(&mut self, reason: Reason) {
        if self.first.is_none() {
            self.first = Some(reason);
            let cause = match reason {
                Reason::Cancelled => HostCause::Cancelled,
                Reason::Deadline(_) => HostCause::Deadline,
                Reason::Range(RangeFault::Budget) => HostCause::Budget,
                _ => HostCause::Io,
            };
            // 상세 range/reactor 원인은 ModelReport에 보존함.
            self.event(Event::HostFailure(cause));
        }
        if matches!(self.phase, Phase::Launching | Phase::Running) {
            self.phase = Phase::Stopping;
            match self.now.checked_add(self.deadlines.cleanup) {
                Some(deadline) => self.deadline = deadline,
                None => self.cleanup_failure(),
            }
            self.range.abort();
        } else if self.phase == Phase::Draining {
            self.phase = Phase::Stopping;
            self.range.abort();
        } else if self.phase == Phase::SettledModel {
            self.cleanup_failure();
        }
    }

    pub fn begin_turn(&mut self, elapsed: Duration, cancelled: bool) -> PollPlan {
        if elapsed < self.now {
            self.fail(Reason::ReactorOrder);
        }
        self.now = elapsed.max(self.now);
        // 같은 관측 회차에서는 취소를 먼저 기록한 뒤 기한을 확인함.
        if cancelled {
            self.fail(Reason::Cancelled);
        }
        if self.now >= self.deadline {
            match self.phase {
                Phase::Launching | Phase::Running => self.fail(Reason::Deadline(self.phase)),
                Phase::Draining | Phase::Stopping => self.cleanup_failure(),
                _ => {}
            }
        }
        self.stdout_quota = IO_QUOTA;
        self.stderr_quota = 48;
        self.stdin_quota = IO_QUOTA;
        self.exec_quota = 4;
        self.plan()
    }

    pub fn plan(&self) -> PollPlan {
        let active = !matches!(self.phase, Phase::SettledModel | Phase::Blocked);
        let reading_range = matches!(
            self.phase,
            Phase::Launching | Phase::Running | Phase::Draining
        );
        PollPlan {
            stdout_max: if active && !self.stdout_eof {
                self.stdout_quota
            } else {
                0
            },
            stderr_max: if !active || self.stderr_eof {
                0
            } else if reading_range {
                self.stderr_quota.min(self.range.request_capacity())
            } else {
                self.stderr_quota
            },
            stdin_max: if active && reading_range && !self.stdin_closed && !self.reaped {
                self.stdin_quota
                    .min(self.range.outbound().map_or(0, <[u8]>::len))
            } else {
                0
            },
            exec_status_max: if active && !self.exec_eof {
                self.exec_quota.min(if self.exec_used == 4 {
                    1
                } else {
                    4 - self.exec_used
                })
            } else {
                0
            },
            timeout: if active {
                self.deadline.saturating_sub(self.now).min(POLL_SLICE)
            } else {
                Duration::ZERO
            },
            request_kill: self.phase == Phase::Stopping && !self.reaped && !self.kill_requested,
            close_stdin: !self.stdin_closed
                && (self.phase == Phase::Stopping
                    || self.reaped
                    || (self.stderr_eof && self.range.outbound().is_none())),
        }
    }

    pub fn stdin_bytes(&self) -> &[u8] {
        let cap = self.plan().stdin_max;
        self.range
            .outbound()
            .map_or(&[], |bytes| &bytes[..bytes.len().min(cap)])
    }

    pub fn stdin_written(&mut self, count: usize) {
        if count == 0 || count > self.plan().stdin_max {
            self.fail(Reason::ReactorOrder);
            return;
        }
        self.stdin_quota -= count;
        if let Err(error) = self.range.written(count) {
            self.fail(Reason::Range(error));
        }
    }

    pub fn stderr_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() || bytes.len() > self.plan().stderr_max {
            self.fail(Reason::ReactorOrder);
            return;
        }
        self.stderr_quota -= bytes.len();
        if matches!(
            self.phase,
            Phase::Launching | Phase::Running | Phase::Draining
        ) {
            if self.reaped {
                self.fail(Reason::Range(RangeFault::OutstandingResponse));
                return;
            }
            if let Err(error) = self.range.feed_request(bytes) {
                self.fail(Reason::Range(error));
            }
        }
        // 정리 중 drain한 bytes는 range 검증 성공으로 세지 않음.
    }

    pub fn stdout_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() || bytes.len() > self.plan().stdout_max {
            self.fail(Reason::ReactorOrder);
            return;
        }
        self.stdout_quota -= bytes.len();
        self.event(Event::Stdout(bytes));
        if let Outcome::Failed(cause) = self.session.report().outcome {
            if self.first.is_none() {
                self.fail(Reason::Raw(cause));
            }
        }
    }

    pub fn stdout_eof(&mut self) {
        if self.stdout_eof || self.closed_fds {
            self.fail(Reason::ReactorOrder);
            return;
        }
        self.stdout_eof = true;
        self.event(Event::StdoutEof);
        if let Outcome::Failed(cause) = self.session.report().outcome {
            if self.first.is_none() {
                self.fail(Reason::Raw(cause));
            }
        }
    }

    pub fn stderr_eof(&mut self) {
        if self.stderr_eof || self.closed_fds {
            self.fail(Reason::ReactorOrder);
            return;
        }
        self.stderr_eof = true;
        if let Err(error) = self.range.request_eof() {
            self.fail(Reason::Range(error));
        }
    }

    pub fn exec_status_bytes(&mut self, bytes: &[u8]) {
        if self.exec_used == 4 || bytes.is_empty() || bytes.len() > self.plan().exec_status_max {
            self.fail(Reason::ExecStatus);
            return;
        }
        self.exec[self.exec_used..self.exec_used + bytes.len()].copy_from_slice(bytes);
        self.exec_used += bytes.len();
        self.exec_quota -= bytes.len();
        if self.exec_used == 4 {
            let errno = i32::from_le_bytes(self.exec);
            self.fail(if errno > 0 {
                Reason::ExecErrno(errno)
            } else {
                Reason::ExecStatus
            });
        }
    }

    pub fn exec_status_eof(&mut self) {
        if self.exec_eof || self.closed_fds {
            self.fail(Reason::ReactorOrder);
            return;
        }
        self.exec_eof = true;
        if self.exec_used != 0 {
            self.fail(Reason::ExecStatus);
        } else if self.phase == Phase::Launching {
            self.phase = Phase::Running;
            match self.now.checked_add(self.deadlines.operation) {
                Some(deadline) => self.deadline = deadline,
                None => self.fail(Reason::ReactorOrder),
            }
        }
    }

    pub fn wait_observation(&mut self, observation: WaitObservation) {
        if self.reaped {
            self.fail(Reason::ReactorOrder);
            return;
        }
        self.reaped = true;
        if matches!(self.phase, Phase::Launching | Phase::Running) {
            self.phase = Phase::Draining;
            match self.now.checked_add(self.deadlines.cleanup) {
                Some(deadline) => self.deadline = deadline,
                None => self.cleanup_failure(),
            }
        }
        match observation {
            WaitObservation::Exited(code) => self.event(Event::NaturalExit(code as i32)),
            WaitObservation::Signalled => self.event(Event::ForcedOrSignalledExit),
        }
        if self.range.outbound().is_some() {
            self.fail(Reason::Range(RangeFault::OutstandingResponse));
        }
        if let Outcome::Failed(cause) = self.session.report().outcome {
            if self.first.is_none() {
                self.fail(Reason::Raw(cause));
            }
        }
    }

    pub fn kill_result(&mut self, accepted: bool) {
        if !self.plan().request_kill {
            self.fail(Reason::ReactorOrder);
            return;
        }
        self.kill_requested = true;
        if !accepted {
            self.cleanup_failure();
        }
        // kill 수락을 wait/reap 증거로 바꾸지 않음.
    }

    pub fn io_failure(&mut self) {
        self.fail(Reason::Io);
    }

    pub fn reap_failure(&mut self) {
        self.fail(Reason::ReapUnconfirmed);
        self.cleanup_failure();
    }

    pub fn stdin_closed(&mut self) {
        if self.stdin_closed || !self.plan().close_stdin {
            self.fail(Reason::ReactorOrder);
            return;
        }
        self.stdin_closed = true;
    }

    pub fn fds_closed(&mut self) {
        if self.closed_fds
            || !self.stdin_closed
            || !self.stdout_eof
            || !self.stderr_eof
            || !self.exec_eof
            || !self.reaped
        {
            self.cleanup_failure();
            return;
        }
        self.closed_fds = true;
        self.event(Event::IoSettled);
        self.event(Event::CleanupComplete);
        if !self.cleanup_failed {
            self.phase = Phase::SettledModel;
        }
    }

    pub fn report(&self) -> ModelReport {
        ModelReport {
            phase: self.phase,
            first: self.first,
            cleanup_failed: self.cleanup_failed,
            owned_child_reaped: self.reaped,
            range: self.range.stats(),
            protocol: self.session.report(),
            live_verified: false,
            publication: false,
            stop_required: true,
        }
    }
}
