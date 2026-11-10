use crate::{controller, core, runtime, transport};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::Duration,
};

pub const GATE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Starting,
    Checkpoint,
    TerminalDelivered,
    JoinReleased,
}

pub struct Evidence {
    pub fault: AtomicBool,
    pub joined: AtomicBool,
    pub final_result: AtomicU8,
}

pub struct Control {
    reached: Receiver<Stage>,
    checkpoint_release: SyncSender<()>,
    join_release: SyncSender<()>,
    pub stage: Stage,
    checkpoint_released: bool,
    join_released: bool,
    pub evidence: Arc<Evidence>,
}

fn hold(stage: Stage, reached: &SyncSender<Stage>, release: &Receiver<()>, evidence: &Evidence) {
    eprintln!("harness worker reached {stage:?}");
    if reached.try_send(stage).is_err() || release.recv_timeout(GATE_TIMEOUT).is_err() {
        evidence.fault.store(true, Ordering::SeqCst);
        panic!("시험 장벽 실패: {stage:?}");
    }
    eprintln!("harness worker released {stage:?}");
}

pub fn create() -> (controller::Controller, Control) {
    let evidence = Arc::new(Evidence {
        fault: AtomicBool::new(false),
        joined: AtomicBool::new(false),
        final_result: AtomicU8::new(0),
    });
    let worker_evidence = evidence.clone();
    let (reached, observe) = mpsc::sync_channel(2);
    let (checkpoint_release, checkpoint_gate) = mpsc::sync_channel(1);
    let (join_release, join_gate) = mpsc::sync_channel(1);
    let task = runtime::assemble(move |operation, sender, done| {
        let mut budget = core::gzip::Budget::new(core::gzip::Limits::default()).unwrap();
        let mut first = true;
        let mut transport = transport::Transport::new(sender);
        let terminal = operation.gzip(
            core::gzip::Request {
                input: runtime::GZIP.into(),
                selected: true,
            },
            &mut budget,
            |event| {
                let result = transport.forward(
                    event.stamp().sequence(),
                    runtime::ProgressEvent::Gzip(event),
                    Ok(()),
                    || core::gzip::Stop::Cancelled,
                );
                if first {
                    first = false;
                    hold(
                        Stage::Checkpoint,
                        &reached,
                        &checkpoint_gate,
                        &worker_evidence,
                    );
                }
                result
            },
        );
        let fallback = runtime::deliver_completion(
            &done,
            runtime::Completion {
                outcome: runtime::Outcome::Gzip(terminal),
                delivery: transport.fault,
            },
        );
        if fallback.is_some() {
            worker_evidence.fault.store(true, Ordering::SeqCst);
            eprintln!("harness failure: completion receiver unavailable");
        }
        hold(
            Stage::TerminalDelivered,
            &reached,
            &join_gate,
            &worker_evidence,
        );
        fallback
    });
    (
        controller::attach(task),
        Control {
            reached: observe,
            checkpoint_release,
            join_release,
            stage: Stage::Starting,
            checkpoint_released: false,
            join_released: false,
            evidence,
        },
    )
}

impl Control {
    pub fn observe(&mut self) {
        for stage in self.reached.try_iter() {
            self.stage = stage;
        }
    }
    pub fn release_checkpoint(&mut self) {
        if self.stage == Stage::Checkpoint && !self.checkpoint_released {
            if self.checkpoint_release.try_send(()).is_err() {
                self.evidence.fault.store(true, Ordering::SeqCst);
            } else {
                self.checkpoint_released = true;
            }
        }
    }
    pub fn release_join(&mut self) {
        if self.stage == Stage::TerminalDelivered && !self.join_released {
            if self.join_release.try_send(()).is_err() {
                self.evidence.fault.store(true, Ordering::SeqCst);
            } else {
                self.join_released = true;
                self.stage = Stage::JoinReleased;
            }
        }
    }
    pub fn checkpoint_enabled(&self) -> bool {
        self.stage == Stage::Checkpoint && !self.checkpoint_released
    }
    pub fn join_enabled(&self) -> bool {
        self.stage == Stage::TerminalDelivered && !self.join_released
    }
}
