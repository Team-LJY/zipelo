#![forbid(unsafe_code)]

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub use {gzip, ustar, zip_create};

static NEXT_OPERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OperationId(u64);

impl OperationId {
    pub fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    GzipSingleMemberV1,
    UstarLinuxV1,
    ZipAsciiCreateV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterFailure {
    OperationIdExhausted,
    SequenceExhausted,
}

#[derive(Clone, Debug)]
pub struct Cancellation {
    operation: OperationId,
    requested: Arc<AtomicBool>,
}

impl Cancellation {
    pub fn operation(&self) -> OperationId {
        self.operation
    }
    pub fn request(&self) -> bool {
        !self.requested.swap(true, Ordering::SeqCst)
    }
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    operation: OperationId,
    sequence: u64,
}

impl Stamp {
    pub fn operation(self) -> OperationId {
        self.operation
    }
    pub fn sequence(self) -> u64 {
        self.sequence
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    GzipCumulativeProcessedBytes,
    UstarInputOffset { pass: u8 },
    UstarEntryIndex { pass: u8 },
    ZipInputOffset { entry: usize },
    ZipOutputExtent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub metric: Metric,
    pub value: u64,
    pub total: Option<u64>,
}

#[derive(Debug)]
pub struct Event<C> {
    stamp: Stamp,
    pub policy: Policy,
    pub checkpoint: C,
    pub progress: Option<Progress>,
}

impl<C> Event<C> {
    pub fn stamp(&self) -> Stamp {
        self.stamp
    }
}

#[derive(Debug)]
pub struct MemoryValidated<T> {
    policy: Policy,
    value: T,
}

impl<T> MemoryValidated<T> {
    pub fn policy(&self) -> Policy {
        self.policy
    }
    pub fn value(&self) -> &T {
        &self.value
    }
    pub fn into_inner(self) -> T {
        self.value
    }
}

#[derive(Debug)]
pub struct Failure<E> {
    pub native: Option<E>,
    pub adapter: Option<AdapterFailure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessCleanup {
    NotApplicable,
}

#[derive(Debug)]
pub struct Terminal<T, E> {
    stamp: Stamp,
    policy: Policy,
    cancellation_requested: bool,
    cancellation_forwarded: bool,
    result: Result<MemoryValidated<T>, Failure<E>>,
}

impl<T, E> Terminal<T, E> {
    pub fn stamp(&self) -> Stamp {
        self.stamp
    }
    pub fn policy(&self) -> Policy {
        self.policy
    }
    pub fn cancellation_requested(&self) -> bool {
        self.cancellation_requested
    }
    pub fn cancellation_forwarded(&self) -> bool {
        self.cancellation_forwarded
    }
    pub fn process_cleanup(&self) -> ProcessCleanup {
        ProcessCleanup::NotApplicable
    }
    pub fn result(&self) -> Result<&MemoryValidated<T>, &Failure<E>> {
        self.result.as_ref()
    }
    pub fn into_result(self) -> Result<MemoryValidated<T>, Failure<E>> {
        self.result
    }
}

pub struct Operation {
    cancellation: Cancellation,
    next_sequence: u64,
    fault: Option<AdapterFailure>,
    cancellation_forwarded: bool,
}

impl Operation {
    pub fn start() -> Result<(Self, Cancellation), AdapterFailure> {
        let value = NEXT_OPERATION
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |v| v.checked_add(1))
            .map_err(|_| AdapterFailure::OperationIdExhausted)?;
        let cancellation = Cancellation {
            operation: OperationId(value),
            requested: Arc::new(AtomicBool::new(false)),
        };
        Ok((
            Self {
                cancellation: cancellation.clone(),
                next_sequence: 0,
                fault: None,
                cancellation_forwarded: false,
            },
            cancellation,
        ))
    }

    pub fn id(&self) -> OperationId {
        self.cancellation.operation
    }

    fn event<C>(
        &mut self,
        policy: Policy,
        checkpoint: C,
        progress: Option<Progress>,
    ) -> Option<Event<C>> {
        // 마지막 순번은 terminal 반환에 남겨 둠.
        let Some(next) = self.next_sequence.checked_add(1) else {
            self.fault = Some(AdapterFailure::SequenceExhausted);
            return None;
        };
        let stamp = Stamp {
            operation: self.id(),
            sequence: self.next_sequence,
        };
        self.next_sequence = next;
        Some(Event {
            stamp,
            policy,
            checkpoint,
            progress,
        })
    }

    fn cancelled(&mut self) -> bool {
        if self.cancellation.is_requested() {
            self.cancellation_forwarded = true;
            true
        } else {
            false
        }
    }

    fn finish<T, E>(self, policy: Policy, result: Result<T, E>) -> Terminal<T, E> {
        let result = match (result, self.fault) {
            (Ok(value), None) => Ok(MemoryValidated { policy, value }),
            (Ok(_), Some(fault)) => Err(Failure {
                native: None,
                adapter: Some(fault),
            }),
            (Err(error), fault) => Err(Failure {
                native: Some(error),
                adapter: fault,
            }),
        };
        Terminal {
            stamp: Stamp {
                operation: self.id(),
                sequence: self.next_sequence,
            },
            policy,
            cancellation_requested: self.cancellation.is_requested(),
            cancellation_forwarded: self.cancellation_forwarded,
            result,
        }
    }

    pub fn gzip<E>(
        mut self,
        request: gzip::Request,
        budget: &mut gzip::Budget,
        mut observe: impl FnMut(Event<gzip::Checkpoint>) -> Result<(), gzip::Stop<E>>,
    ) -> Terminal<gzip::Completed, gzip::Error<E>> {
        let policy = Policy::GzipSingleMemberV1;
        let result = gzip::read(request, budget, |checkpoint| {
            let progress = Some(Progress {
                metric: Metric::GzipCumulativeProcessedBytes,
                value: checkpoint.usage.processed,
                total: None,
            });
            let event = self
                .event(policy, checkpoint, progress)
                .ok_or(gzip::Stop::Cancelled)?;
            observe(event)?;
            if self.cancelled() {
                Err(gzip::Stop::Cancelled)
            } else {
                Ok(())
            }
        });
        self.finish(policy, result)
    }

    pub fn ustar<E>(
        mut self,
        request: ustar::Request,
        select: impl FnOnce(&[ustar::Entry]) -> Result<Vec<String>, E>,
        mut observe: impl FnMut(Event<ustar::Checkpoint>) -> Result<(), ustar::Stop>,
    ) -> Terminal<ustar::Completed, ustar::Error<E>> {
        let policy = Policy::UstarLinuxV1;
        let result = ustar::read(request, select, |checkpoint| {
            let progress = match checkpoint {
                ustar::Checkpoint::Read { pass, offset } => Some(Progress {
                    metric: Metric::UstarInputOffset { pass },
                    value: offset as u64,
                    total: None,
                }),
                ustar::Checkpoint::Entry { pass, index } => Some(Progress {
                    metric: Metric::UstarEntryIndex { pass },
                    value: index as u64,
                    total: None,
                }),
                _ => None,
            };
            let event = self
                .event(policy, checkpoint, progress)
                .ok_or(ustar::Stop::Cancelled)?;
            observe(event)?;
            if self.cancelled() {
                Err(ustar::Stop::Cancelled)
            } else {
                Ok(())
            }
        });
        self.finish(policy, result)
    }

    pub fn create_zip(
        mut self,
        request: zip_create::Request,
        mut observe: impl FnMut(Event<zip_create::Checkpoint>) -> Result<(), zip_create::Stop>,
    ) -> Terminal<Vec<u8>, zip_create::Failure> {
        let policy = Policy::ZipAsciiCreateV1;
        let result = zip_create::create(request, |checkpoint| {
            let progress = match checkpoint {
                zip_create::Checkpoint::Input { index, offset, .. } => Some(Progress {
                    metric: Metric::ZipInputOffset { entry: index },
                    value: offset as u64,
                    total: None,
                }),
                zip_create::Checkpoint::Write { extent, .. } => Some(Progress {
                    metric: Metric::ZipOutputExtent,
                    value: extent,
                    total: None,
                }),
                _ => None,
            };
            let event = self
                .event(policy, checkpoint, progress)
                .ok_or(zip_create::Stop::Cancelled)?;
            observe(event)?;
            if self.cancelled() {
                Err(zip_create::Stop::Cancelled)
            } else {
                Ok(())
            }
        });
        self.finish(policy, result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Accepted,
    IgnoredStale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryError {
    Sequence { expected: u64, received: u64 },
    AlreadyTerminal,
    Faulted,
}

pub struct Receiver {
    operation: OperationId,
    next_sequence: u64,
    terminal: bool,
    faulted: bool,
}

impl Receiver {
    pub fn new(operation: OperationId) -> Self {
        Self {
            operation,
            next_sequence: 0,
            terminal: false,
            faulted: false,
        }
    }

    pub fn progress<C>(&mut self, event: &Event<C>) -> Result<Delivery, DeliveryError> {
        self.accept(event.stamp, false)
    }

    pub fn terminal<T, E>(&mut self, event: &Terminal<T, E>) -> Result<Delivery, DeliveryError> {
        self.accept(event.stamp, true)
    }

    fn accept(&mut self, stamp: Stamp, terminal: bool) -> Result<Delivery, DeliveryError> {
        // 이전 작업 이벤트는 현재 순번이나 종료 상태를 바꾸지 않음.
        if stamp.operation != self.operation {
            return Ok(Delivery::IgnoredStale);
        }
        if self.faulted {
            return Err(DeliveryError::Faulted);
        }
        if self.terminal {
            return Err(DeliveryError::AlreadyTerminal);
        }
        if stamp.sequence != self.next_sequence || (!terminal && stamp.sequence == u64::MAX) {
            self.faulted = true;
            return Err(DeliveryError::Sequence {
                expected: self.next_sequence,
                received: stamp.sequence,
            });
        }
        if terminal {
            self.terminal = true;
        } else {
            self.next_sequence = self
                .next_sequence
                .checked_add(1)
                .ok_or(DeliveryError::Faulted)?;
        }
        Ok(Delivery::Accepted)
    }
}
