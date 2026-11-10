//! 효과를 실행하지 않고 상태와 모의 명령 값만 반환하는 순수 UI 후보임.

mod listing;
pub use listing::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiOperationId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MockJobId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub ui: UiOperationId,
    pub job: MockJobId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub origin: String,
    pub policy: String,
    pub classification: String,
    pub code: i64,
}

impl Failure {
    fn valid(&self) -> bool {
        !self.origin.is_empty()
            && self.origin.len() <= 128
            && !self.policy.is_empty()
            && self.policy.len() <= 128
            && !self.classification.is_empty()
            && self.classification.len() <= 256
    }
}

/// 모든 값은 모의 모델 또는 호출자 진술임. OS 관찰 종류는 제공하지 않음.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceKind {
    Model,
    CallerAttested,
}

/// 단계 구분용 모의 값이며 Published receipt를 발급하지 않음.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionKind {
    MemoryValidated,
    ProtocolObserved,
    Published,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationKind {
    Memory,
    Process,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupKind {
    NotApplicable,
    Confirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CleanupObservation {
    pub kind: CleanupKind,
    pub evidence: EvidenceKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Terminal {
    Completed(CompletionKind),
    MockCancelled,
    Failed(Failure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkFault {
    Mapping,
    Sequence,
    SequenceOverflow,
    ConflictingTerminal,
    DuplicateTerminal,
    SuccessAfterCancel,
    InvalidProgress,
    InvalidEvidence,
    ConflictingCleanup,
    CleanupKindMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirstError {
    Reported(Failure),
    Cleanup(Failure),
    Connection(LinkFault),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MockEvent {
    Progress { done: u64, total: u64 },
    Terminal(Terminal),
    CleanupConfirmed,
    CleanupNotApplicable,
    CleanupFailed(Failure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub mapping: Mapping,
    pub sequence: u64,
    pub evidence: EvidenceKind,
    pub event: MockEvent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
    pub kind: OperationKind,
    pub mapping: Mapping,
    pub next_sequence: u64,
    pub cancel_requested: bool,
    pub progress: Option<(u64, u64)>,
    pub terminal: Option<Terminal>,
    pub terminal_evidence: Option<EvidenceKind>,
    pub cleanup: Option<CleanupObservation>,
    pub cleanup_failure_evidence: Option<EvidenceKind>,
    pub cleanup_failed: Option<Failure>,
    pub link_fault: Option<LinkFault>,
    pub first_error: Option<FirstError>,
}

impl Operation {
    pub fn cleanup_confirmed(&self) -> bool {
        self.cleanup
            .is_some_and(|c| c.kind == CleanupKind::Confirmed)
    }
    pub fn settled(&self) -> bool {
        self.terminal.is_some()
            && self.cleanup.is_some_and(|c| {
                matches!(
                    (self.kind, c.kind),
                    (OperationKind::Memory, CleanupKind::NotApplicable)
                        | (OperationKind::Process, CleanupKind::Confirmed)
                )
            })
            && self.cleanup_failed.is_none()
            && self.link_fault.is_none()
    }

    fn fault(&mut self, fault: LinkFault) {
        self.link_fault.get_or_insert(fault);
        self.first_error
            .get_or_insert(FirstError::Connection(fault));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseState {
    Open,
    Confirming,
    WaitingForCleanup,
    MockClosed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    StartMock,
    StartMockFor(OperationKind),
    RequestCancel,
    RequestClose,
    KeepOpen,
    ConfirmCloseCancel,
    ReplaceMockList {
        operation: UiOperationId,
        rows: Vec<Row>,
    },
    Select(RowKey),
    Key {
        area: InputArea,
        key: Navigation,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Command(Command),
    Event(Envelope),
}

/// 호출자에게 전달하는 모의 값이며 실제 시작/중단/창 종료 증거가 아님.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MockEffect {
    Start(Mapping),
    Cancel(Mapping),
    PermitClose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    Busy,
    Closed,
    SerialOverflow,
    GenerationOverflow,
    InvalidList,
    StaleRow,
    CancelAfterTerminal,
    StaleOperation,
    InputNotList,
    CleanupSticky,
    LateProgress,
    ConnectionBlocked,
    NoConfirmation,
    NoOperation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    Applied,
    Ignored(Rejection),
    ConnectionFault(LinkFault),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    pub state: Model,
    pub effect: Option<MockEffect>,
    pub disposition: Disposition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    namespace: [u8; 8],
    serial: u64,
    generation: u64,
    operation: Option<Operation>,
    close: CloseState,
    rows: Vec<Row>,
    selected: Option<RowKey>,
}

impl Model {
    /// namespace는 같은 모의 이벤트 영역에서 모델마다 달라야 함. 영 값은 거부함.
    pub fn new(namespace: [u8; 8]) -> Option<Self> {
        (namespace != [0; 8]).then_some(Self {
            namespace,
            serial: 0,
            generation: 0,
            operation: None,
            close: CloseState::Open,
            rows: Vec::new(),
            selected: None,
        })
    }
    pub fn operation(&self) -> Option<&Operation> {
        self.operation.as_ref()
    }
    pub fn close(&self) -> CloseState {
        self.close
    }
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
    pub fn selected(&self) -> Option<RowKey> {
        self.selected
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn row_key(&self, id: RowId) -> Option<RowKey> {
        self.rows.iter().any(|r| r.id == id).then_some(RowKey {
            operation: self.operation.as_ref()?.mapping.ui,
            generation: self.generation,
            row: id,
        })
    }

    pub fn reduce(&self, input: Input) -> Transition {
        let mut state = self.clone();
        let mut effect = None;
        let disposition =
            if state.close == CloseState::MockClosed && matches!(&input, Input::Command(_)) {
                Disposition::Ignored(Rejection::Closed)
            } else {
                match input {
                    Input::Command(command) => state.command(command, &mut effect),
                    Input::Event(event) => state.event(event),
                }
            };
        if state.close == CloseState::WaitingForCleanup
            && state.operation.as_ref().is_none_or(Operation::settled)
        {
            state.close = CloseState::MockClosed;
            effect = Some(MockEffect::PermitClose);
        }
        Transition {
            state,
            effect,
            disposition,
        }
    }

    fn cancel(&mut self, effect: &mut Option<MockEffect>) -> Disposition {
        let Some(op) = self.operation.as_mut() else {
            return Disposition::Ignored(Rejection::NoOperation);
        };
        if op.terminal.is_some() {
            return Disposition::Ignored(Rejection::CancelAfterTerminal);
        }
        if op.link_fault.is_some() {
            return Disposition::Ignored(Rejection::ConnectionBlocked);
        }
        if !op.cancel_requested {
            op.cancel_requested = true;
            *effect = Some(MockEffect::Cancel(op.mapping));
        }
        Disposition::Applied
    }

    fn command(&mut self, cmd: Command, effect: &mut Option<MockEffect>) -> Disposition {
        match cmd {
            Command::StartMock | Command::StartMockFor(_) => {
                let kind = match cmd {
                    Command::StartMockFor(kind) => kind,
                    _ => OperationKind::Process,
                };
                if self.close != CloseState::Open
                    || self.operation.as_ref().is_some_and(|op| !op.settled())
                {
                    return Disposition::Ignored(Rejection::Busy);
                }
                let Some(serial) = self.serial.checked_add(1) else {
                    return Disposition::Ignored(Rejection::SerialOverflow);
                };
                let mut bytes = [0; 16];
                bytes[..8].copy_from_slice(&self.namespace);
                bytes[8..].copy_from_slice(&serial.to_be_bytes());
                let mapping = Mapping {
                    ui: UiOperationId(serial),
                    job: MockJobId(bytes),
                };
                self.serial = serial;
                self.rows.clear();
                self.selected = None;
                self.operation = Some(Operation {
                    kind,
                    mapping,
                    next_sequence: 0,
                    cancel_requested: false,
                    progress: None,
                    terminal: None,
                    terminal_evidence: None,
                    cleanup: None,
                    cleanup_failure_evidence: None,
                    cleanup_failed: None,
                    link_fault: None,
                    first_error: None,
                });
                *effect = Some(MockEffect::Start(mapping));
                Disposition::Applied
            }
            Command::RequestCancel => self.cancel(effect),
            Command::RequestClose => {
                if self.close == CloseState::Open {
                    if self.operation.as_ref().is_none_or(Operation::settled) {
                        self.close = CloseState::MockClosed;
                        *effect = Some(MockEffect::PermitClose);
                    } else {
                        self.close = CloseState::Confirming;
                    }
                }
                Disposition::Applied
            }
            Command::KeepOpen => {
                self.close = CloseState::Open;
                Disposition::Applied
            }
            Command::ConfirmCloseCancel => {
                if self.close != CloseState::Confirming {
                    return Disposition::Ignored(Rejection::NoConfirmation);
                }
                self.close = CloseState::WaitingForCleanup;
                self.cancel(effect);
                Disposition::Applied
            }
            Command::ReplaceMockList { operation, rows } => self.replace_list(operation, rows),
            Command::Select(key) => {
                if self.row_key(key.row) != Some(key) {
                    return Disposition::Ignored(Rejection::StaleRow);
                }
                self.selected = Some(key);
                Disposition::Applied
            }
            Command::Key { area, key } => {
                if area != InputArea::List {
                    return Disposition::Ignored(Rejection::InputNotList);
                }
                self.navigate(key);
                Disposition::Applied
            }
        }
    }

    fn event(&mut self, envelope: Envelope) -> Disposition {
        let Some(op) = self.operation.as_mut() else {
            return Disposition::Ignored(Rejection::NoOperation);
        };
        // 이전 작업은 현재 작업의 순번과 오류 근거를 바꾸지 않음.
        if envelope.mapping.ui.0 < op.mapping.ui.0 {
            return Disposition::Ignored(Rejection::StaleOperation);
        }
        let fault = if envelope.mapping != op.mapping {
            Some(LinkFault::Mapping)
        } else if op.link_fault.is_some() {
            return Disposition::Ignored(Rejection::ConnectionBlocked);
        } else if envelope.sequence != op.next_sequence {
            Some(LinkFault::Sequence)
        } else if op.next_sequence.checked_add(1).is_none() {
            Some(LinkFault::SequenceOverflow)
        } else {
            None
        };
        if let Some(fault) = fault {
            op.fault(fault);
            return Disposition::ConnectionFault(fault);
        }
        op.next_sequence += 1;
        match envelope.event {
            MockEvent::Progress { done, total } => {
                if op.terminal.is_some() || op.cancel_requested {
                    return Disposition::Ignored(Rejection::LateProgress);
                }
                if done > total
                    || op
                        .progress
                        .is_some_and(|(old, cap)| done < old || total != cap)
                {
                    op.fault(LinkFault::InvalidProgress);
                    return Disposition::ConnectionFault(LinkFault::InvalidProgress);
                }
                op.progress = Some((done, total));
            }
            MockEvent::Terminal(result) => {
                let fault = if let Some(first) = &op.terminal {
                    Some(if *first == result {
                        LinkFault::DuplicateTerminal
                    } else {
                        LinkFault::ConflictingTerminal
                    })
                } else if matches!(&result, Terminal::Failed(e) if !e.valid()) {
                    Some(LinkFault::InvalidEvidence)
                } else if op.cancel_requested && matches!(result, Terminal::Completed(_)) {
                    Some(LinkFault::SuccessAfterCancel)
                } else {
                    None
                };
                if let Some(fault) = fault {
                    op.fault(fault);
                    return Disposition::ConnectionFault(fault);
                }
                if let Terminal::Failed(failure) = &result {
                    op.first_error
                        .get_or_insert(FirstError::Reported(failure.clone()));
                }
                op.terminal = Some(result);
                op.terminal_evidence = Some(envelope.evidence);
            }
            MockEvent::CleanupConfirmed | MockEvent::CleanupNotApplicable => {
                let kind = match envelope.event {
                    MockEvent::CleanupNotApplicable => CleanupKind::NotApplicable,
                    _ => CleanupKind::Confirmed,
                };
                if !matches!(
                    (op.kind, kind),
                    (OperationKind::Memory, CleanupKind::NotApplicable)
                        | (OperationKind::Process, CleanupKind::Confirmed)
                ) {
                    op.fault(LinkFault::CleanupKindMismatch);
                    return Disposition::ConnectionFault(LinkFault::CleanupKindMismatch);
                }
                if op.cleanup_failed.is_some() {
                    return Disposition::Ignored(Rejection::CleanupSticky);
                }
                if op.cleanup.is_some() {
                    op.fault(LinkFault::ConflictingCleanup);
                    return Disposition::ConnectionFault(LinkFault::ConflictingCleanup);
                }
                op.cleanup = Some(CleanupObservation {
                    kind,
                    evidence: envelope.evidence,
                });
            }
            MockEvent::CleanupFailed(failure) => {
                if !failure.valid() {
                    op.fault(LinkFault::InvalidEvidence);
                    return Disposition::ConnectionFault(LinkFault::InvalidEvidence);
                }
                op.first_error
                    .get_or_insert(FirstError::Cleanup(failure.clone()));
                op.cleanup_failed.get_or_insert(failure);
                op.cleanup_failure_evidence.get_or_insert(envelope.evidence);
            }
        }
        Disposition::Applied
    }
}

#[cfg(test)]
#[path = "../../../tests/rust-ui-operation-reducer/overflow.rs"]
mod overflow;
