#![forbid(unsafe_code)]

//! 개발용 단일 GZip 파일 작업을 순서대로 연결함.
//! OS worker 격리나 제품 게시 완료를 증명하지 않음.

mod types;
pub use types::*;
pub use zipelo_file_publication as publication;
pub use zipelo_file_snapshot as snapshot;
pub use zipelo_memory_gzip_reader as gzip;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

// native commit gate를 먼저 결정하고 앞 단계의 협력적 취소에 전달함.
#[derive(Clone)]
pub struct Cancellation {
    native: publication::Cancellation,
    accepted: Arc<AtomicBool>,
}

impl Cancellation {
    pub fn request(&self) -> publication::CancelDecision {
        let decision = self.native.request();
        if matches!(
            decision,
            publication::CancelDecision::Requested | publication::CancelDecision::AlreadyRequested
        ) {
            self.accepted.store(true, Ordering::SeqCst);
        }
        decision
    }

    fn observe<E>(
        &self,
        checkpoint: Checkpoint,
        observer: &mut impl FnMut(Checkpoint) -> Result<(), Stop<E>>,
    ) -> Result<(), Stop<E>> {
        // callback 원인을 취소 확인보다 먼저 보존함.
        observer(checkpoint)?;
        if self.accepted.load(Ordering::SeqCst) {
            return Err(Stop::Cancelled);
        }
        Ok(())
    }
}

pub struct FileJob {
    publication: publication::Publication,
    cancellation: Cancellation,
}

impl FileJob {
    pub fn new() -> (Self, Cancellation) {
        let (publication, native) = publication::Publication::new();
        let cancellation = Cancellation {
            native,
            accepted: Arc::new(AtomicBool::new(false)),
        };
        (
            Self {
                publication,
                cancellation: cancellation.clone(),
            },
            cancellation,
        )
    }

    // 한 번 소비하며 실패와 불명확한 게시를 자동 재시도하지 않음.
    pub fn run<E>(
        self,
        request: Request<'_>,
        mut observer: impl FnMut(Checkpoint) -> Result<(), Stop<E>>,
    ) -> Report<E> {
        let mut usage = Usage::default();
        let captured = snapshot::capture(
            snapshot::Request {
                path: request.input,
                limits: request.limits.snapshot,
            },
            |checkpoint| {
                usage.snapshot = checkpoint.usage;
                self.cancellation
                    .observe(
                        Checkpoint {
                            event: Event::Snapshot(checkpoint),
                            usage,
                        },
                        &mut observer,
                    )
                    .map_err(|stop| match stop {
                        Stop::Cancelled => snapshot::Stop::Cancelled,
                        Stop::Callback(error) => snapshot::Stop::Callback(error),
                    })
            },
        );
        let captured = match captured {
            Ok(value) => value,
            Err(failure) => {
                usage.snapshot = failure.usage;
                return Report {
                    outcome: Outcome::SnapshotFailed(failure),
                    usage,
                    snapshot: None,
                    decoded: None,
                };
            }
        };
        usage.snapshot = captured.usage();
        let source = SnapshotObservation::from_capture(&captured);
        let mut budget = match gzip::Budget::new(request.limits.gzip) {
            Ok(value) => value,
            Err(failure) => {
                return Report {
                    outcome: Outcome::DecodeFailed(gzip::Error::Reader(failure)),
                    usage,
                    snapshot: Some(source),
                    decoded: None,
                };
            }
        };
        // 검증된 snapshot의 소유 bytes만 넘기며 원래 pathname을 다시 열지 않음.
        let decoded = gzip::read(
            gzip::Request {
                input: captured.into_bytes().into_boxed_slice(),
                selected: true,
            },
            &mut budget,
            |checkpoint| {
                usage.gzip = checkpoint.usage;
                self.cancellation
                    .observe(
                        Checkpoint {
                            event: Event::Decode(checkpoint),
                            usage,
                        },
                        &mut observer,
                    )
                    .map_err(|stop| match stop {
                        Stop::Cancelled => gzip::Stop::Cancelled,
                        Stop::Callback(error) => gzip::Stop::Callback(error),
                    })
            },
        );
        // 마지막 checkpoint 뒤의 실패 과금도 원래 Budget에서 회수함.
        usage.gzip = budget.usage();
        let decoded = match decoded {
            Ok(value) => value,
            Err(error) => {
                return Report {
                    outcome: Outcome::DecodeFailed(error),
                    usage,
                    snapshot: Some(source),
                    decoded: None,
                };
            }
        };
        let Some(bytes) = decoded.bytes else {
            return Report {
                outcome: Outcome::AdapterFailed(AdapterFailure::MissingSelectedOutput),
                usage,
                snapshot: Some(source),
                decoded: None,
            };
        };
        if bytes.len() as u64 != decoded.boundary.inflated_bytes {
            return Report {
                outcome: Outcome::AdapterFailed(AdapterFailure::SelectedLengthMismatch {
                    expected: decoded.boundary.inflated_bytes,
                    actual: bytes.len() as u64,
                }),
                usage,
                snapshot: Some(source),
                decoded: None,
            };
        }
        let memory = GzipObservation::from_decode(decoded.boundary);
        let native = self.publication.publish(
            publication::Request {
                parent: request.output_parent,
                leaf: request.output_leaf,
                bytes: &bytes,
                limits: request.limits.publication,
            },
            |checkpoint| {
                usage.publication = checkpoint.usage;
                self.cancellation
                    .observe(
                        Checkpoint {
                            event: Event::Publication(checkpoint),
                            usage,
                        },
                        &mut observer,
                    )
                    .map_err(|stop| match stop {
                        Stop::Cancelled => publication::Stop::Cancelled,
                        Stop::Callback(error) => publication::Stop::Callback(error),
                    })
            },
        );
        usage.publication = native.usage;
        // commit 뒤에는 callback, 취소 판정, 재시도와 rollback을 추가하지 않음.
        Report {
            outcome: Outcome::Publication(native),
            usage,
            snapshot: Some(source),
            decoded: Some(memory),
        }
    }
}
