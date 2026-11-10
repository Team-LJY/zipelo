use crate::{
    core::{self, gzip, ustar, zip_create},
    transport::{DeliveryFault, Transport},
};
use std::{
    any::Any,
    sync::mpsc::{self, Receiver, SyncSender},
    thread::{self, JoinHandle},
};

pub const DEFAULT_CAPACITY: usize = 256;
pub const PAYLOAD: &[u8] = include_bytes!("../../../tests/rust-ui-memory-app/fixtures/payload.txt");
pub const GZIP: &[u8] = include_bytes!("../../../tests/rust-ui-memory-app/fixtures/sample.gz");
pub const USTAR: &[u8] = include_bytes!("../../../tests/rust-ui-memory-app/fixtures/sample.tar");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Zip,
    Ustar,
    Gzip,
    InvalidGzip,
}

#[derive(Debug)]
pub enum ProgressEvent {
    Zip(core::Event<zip_create::Checkpoint>),
    Ustar(core::Event<ustar::Checkpoint>),
    Gzip(core::Event<gzip::Checkpoint>),
}

impl ProgressEvent {
    pub fn validate(
        &self,
        receiver: &mut core::Receiver,
    ) -> Result<core::Delivery, core::DeliveryError> {
        match self {
            Self::Zip(e) => receiver.progress(e),
            Self::Ustar(e) => receiver.progress(e),
            Self::Gzip(e) => receiver.progress(e),
        }
    }
    pub fn progress(&self) -> Option<core::Progress> {
        match self {
            Self::Zip(e) => e.progress,
            Self::Ustar(e) => e.progress,
            Self::Gzip(e) => e.progress,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct CallbackFailure(pub &'static str);

#[derive(Debug)]
pub enum Outcome {
    Zip(core::Terminal<Vec<u8>, zip_create::Failure>),
    Ustar(core::Terminal<ustar::Completed, ustar::Error<CallbackFailure>>),
    Gzip(core::Terminal<gzip::Completed, gzip::Error<CallbackFailure>>),
}

impl Outcome {
    pub fn validate(
        &self,
        receiver: &mut core::Receiver,
    ) -> Result<core::Delivery, core::DeliveryError> {
        match self {
            Self::Zip(e) => receiver.terminal(e),
            Self::Ustar(e) => receiver.terminal(e),
            Self::Gzip(e) => receiver.terminal(e),
        }
    }
    pub fn is_ok(&self) -> bool {
        match self {
            Self::Zip(e) => e.result().is_ok(),
            Self::Ustar(e) => e.result().is_ok(),
            Self::Gzip(e) => e.result().is_ok(),
        }
    }
    pub fn cancelled(&self) -> bool {
        match self {
            Self::Zip(t) => matches!(
                t.result(),
                Err(core::Failure {
                    native: Some(zip_create::Failure::Cancelled),
                    ..
                })
            ),
            Self::Ustar(t) => matches!(
                t.result(),
                Err(core::Failure {
                    native: Some(ustar::Error::Reader(ustar::Failure::Cancelled)),
                    ..
                })
            ),
            Self::Gzip(t) => matches!(
                t.result(),
                Err(core::Failure {
                    native: Some(gzip::Error::Reader(gzip::Failure::Cancelled)),
                    ..
                })
            ),
        }
    }
    pub fn next_sequence(&self) -> u64 {
        match self {
            Self::Zip(t) => t.stamp().sequence(),
            Self::Ustar(t) => t.stamp().sequence(),
            Self::Gzip(t) => t.stamp().sequence(),
        }
    }
}

#[derive(Debug)]
pub struct Completion {
    pub outcome: Outcome,
    pub delivery: Option<DeliveryFault>,
}

pub fn execute(
    operation: core::Operation,
    action: Action,
    sender: SyncSender<ProgressEvent>,
) -> Completion {
    let mut transport = Transport::new(sender);
    let outcome = match action {
        Action::Zip => Outcome::Zip(operation.create_zip(
            zip_create::Request {
                entries: vec![zip_create::Entry {
                    name: "sample.txt".into(),
                    bytes: PAYLOAD.into(),
                }],
                options: zip_create::Options::default(),
                limits: zip_create::Limits::default(),
                timeout: std::time::Duration::from_secs(5),
            },
            |event| {
                transport.forward(
                    event.stamp().sequence(),
                    ProgressEvent::Zip(event),
                    Ok(()),
                    || zip_create::Stop::Cancelled,
                )
            },
        )),
        Action::Ustar => Outcome::Ustar(operation.ustar(
            ustar::Request {
                input: USTAR.into(),
                policy: ustar::NamePolicy::Linux,
                limits: ustar::Limits::default(),
            },
            |entries| Ok(entries.iter().map(|entry| entry.id.clone()).collect()),
            |event| {
                transport.forward(
                    event.stamp().sequence(),
                    ProgressEvent::Ustar(event),
                    Ok(()),
                    || ustar::Stop::Cancelled,
                )
            },
        )),
        Action::Gzip | Action::InvalidGzip => {
            let input = if action == Action::InvalidGzip {
                Box::from(&b"invalid"[..])
            } else {
                GZIP.into()
            };
            let mut budget = gzip::Budget::new(gzip::Limits::default()).expect("고정 기본 예산");
            Outcome::Gzip(operation.gzip(
                gzip::Request {
                    input,
                    selected: true,
                },
                &mut budget,
                |event| {
                    transport.forward(
                        event.stamp().sequence(),
                        ProgressEvent::Gzip(event),
                        Ok(()),
                        || gzip::Stop::Cancelled,
                    )
                },
            ))
        }
    };
    Completion {
        outcome,
        delivery: transport.fault,
    }
}

pub fn deliver_completion(
    sender: &SyncSender<Completion>,
    completion: Completion,
) -> Option<Completion> {
    match sender.try_send(completion) {
        Ok(()) => None,
        Err(mpsc::TrySendError::Full(value) | mpsc::TrySendError::Disconnected(value)) => {
            Some(value)
        }
    }
}

#[derive(Debug)]
pub enum StartFailure {
    Capacity,
    Operation(core::AdapterFailure),
    Spawn(std::io::Error),
}

#[must_use = "작업 소유자는 종료까지 핸들을 보존해야 함"]
pub struct Task {
    pub id: core::OperationId,
    pub cancellation: core::Cancellation,
    pub events: Option<Receiver<ProgressEvent>>,
    pub completion: Option<Receiver<Completion>>,
    handle: Option<JoinHandle<Option<Completion>>>,
}

impl Task {
    pub fn spawn(action: Action, capacity: usize) -> Result<Self, StartFailure> {
        Self::spawn_with(capacity, move |op, sender| execute(op, action, sender))
    }
    pub fn spawn_with(
        capacity: usize,
        work: impl FnOnce(core::Operation, SyncSender<ProgressEvent>) -> Completion + Send + 'static,
    ) -> Result<Self, StartFailure> {
        if capacity > DEFAULT_CAPACITY {
            return Err(StartFailure::Capacity);
        }
        let (op, cancellation) = core::Operation::start().map_err(StartFailure::Operation)?;
        let id = op.id();
        let (sender, events) = mpsc::sync_channel(capacity);
        let (done, completion) = mpsc::sync_channel(1);
        let handle = thread::Builder::new()
            .name("zipelo-memory".into())
            .spawn(move || deliver_completion(&done, work(op, sender)))
            .map_err(StartFailure::Spawn)?;
        Ok(Self {
            id,
            cancellation,
            events: Some(events),
            completion: Some(completion),
            handle: Some(handle),
        })
    }
    pub fn finished(&self) -> bool {
        self.handle.as_ref().is_none_or(JoinHandle::is_finished)
    }
    pub fn joined(&self) -> bool {
        self.handle.is_none()
    }
    pub fn poll_join(&mut self) -> Option<Result<Option<Completion>, Box<dyn Any + Send>>> {
        if !self.finished() {
            return None;
        }
        self.handle.take().map(JoinHandle::join)
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        // 정상 frame 경로는 종료 확인 뒤에만 Task를 제거함. 비정상 소유권 폐기도 detach하지 않음.
        if let Some(handle) = self.handle.take() {
            self.cancellation.request();
            let _ = handle.join();
        }
    }
}
