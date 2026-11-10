use std::time::Instant;

use crate::{
    Checkpoint, Failure, Limits, MAX_BYTES, MAX_DURATION, MAX_ENTRIES, MAX_METADATA, MAX_PROCESSED,
    Stop, Usage,
};

pub(crate) struct Budget<C> {
    pub(crate) limits: Limits,
    pub(crate) usage: Usage,
    started: Instant,
    checkpoint: C,
}

impl<C: FnMut(Checkpoint) -> Result<(), Stop>> Budget<C> {
    pub(crate) fn new(mut limits: Limits, checkpoint: C) -> Result<Self, Failure> {
        if limits.input == 0
            || limits.entries == 0
            || limits.processed == 0
            || limits.output == 0
            || limits.metadata == 0
            || limits.duration.is_zero()
        {
            return Err(Failure::InvalidRequest);
        }
        limits.input = limits.input.min(MAX_BYTES);
        limits.entries = limits.entries.min(MAX_ENTRIES);
        limits.processed = limits.processed.min(MAX_PROCESSED);
        limits.output = limits.output.min(MAX_BYTES);
        limits.metadata = limits.metadata.min(MAX_METADATA);
        limits.duration = limits.duration.min(MAX_DURATION);
        Ok(Self {
            limits,
            usage: Usage::default(),
            started: Instant::now(),
            checkpoint,
        })
    }

    pub(crate) fn check(&mut self, point: Checkpoint) -> Result<(), Failure> {
        // 체크 콜백 시간도 같은 기한에 포함함. 호출 자체의 강제 중단은 제공하지 않음.
        match (self.checkpoint)(point) {
            Err(Stop::Cancelled) => return Err(Failure::Cancelled),
            Err(Stop::Deadline) => return Err(Failure::Budget),
            Ok(()) => {}
        }
        if self.started.elapsed() >= self.limits.duration {
            return Err(Failure::Budget);
        }
        Ok(())
    }

    pub(crate) fn entry(&mut self) -> Result<(), Failure> {
        if self.usage.entries >= self.limits.entries {
            return Err(Failure::Budget);
        }
        self.usage.entries += 1;
        Ok(())
    }

    pub(crate) fn processed(&mut self, count: u64) -> Result<(), Failure> {
        if count > self.limits.processed - self.usage.processed {
            return Err(Failure::Budget);
        }
        self.usage.processed += count;
        Ok(())
    }

    pub(crate) fn output(&mut self, count: u64) -> Result<(), Failure> {
        if count > self.limits.output - self.usage.output {
            return Err(Failure::Budget);
        }
        self.usage.output += count;
        Ok(())
    }
}
