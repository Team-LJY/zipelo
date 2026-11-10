use crate::Failure;
use std::time::{Duration, Instant};

pub const CHUNK_SIZE: usize = 65_536;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub input: u64,
    pub processed: u64,
    pub output: u64,
    pub metadata: u64,
    pub duration: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            input: 16 * 1024 * 1024,
            processed: 128 * 1024 * 1024,
            output: 16 * 1024 * 1024,
            metadata: 1024 * 1024,
            duration: Duration::from_secs(300),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub processed: u64,
    pub output: u64,
    pub metadata: u64,
}

#[derive(Debug)]
pub struct Budget {
    limits: Limits,
    usage: Usage,
    started: Instant,
}

impl Budget {
    pub fn new(requested: Limits) -> Result<Self, Failure> {
        if requested.duration.is_zero() {
            return Err(Failure::InvalidRequest);
        }
        let cap = Limits::default();
        Ok(Self {
            limits: Limits {
                input: requested.input.min(cap.input),
                processed: requested.processed.min(cap.processed),
                output: requested.output.min(cap.output),
                metadata: requested.metadata.min(cap.metadata),
                duration: requested.duration.min(cap.duration),
            },
            usage: Usage::default(),
            started: Instant::now(),
        })
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    pub(crate) fn check(&self) -> Result<(), Failure> {
        if self.usage.processed > self.limits.processed || self.elapsed() >= self.limits.duration {
            return Err(Failure::Budget);
        }
        Ok(())
    }

    pub(crate) fn input(&mut self, count: usize) -> Result<(), Failure> {
        charge(
            &mut self.usage.processed,
            count as u64,
            self.limits.processed,
        )
    }

    pub(crate) fn decoded(&mut self, count: u64) -> Result<(), Failure> {
        if count > CHUNK_SIZE as u64 {
            return Err(Failure::Boundary);
        }
        if self.usage.processed > self.limits.processed {
            return Err(Failure::Budget);
        }
        // 이미 생성된 출력은 한도 초과 시에도 누적함.
        self.usage.processed = self
            .usage
            .processed
            .checked_add(count)
            .ok_or(Failure::Boundary)?;
        if self.usage.processed > self.limits.processed {
            return Err(Failure::Budget);
        }
        Ok(())
    }

    pub(crate) fn output(&mut self, count: usize) -> Result<(), Failure> {
        charge(&mut self.usage.output, count as u64, self.limits.output)
    }

    pub(crate) fn metadata(&mut self) -> Result<(), Failure> {
        charge(&mut self.usage.metadata, 1, self.limits.metadata)
    }
}

fn charge(value: &mut u64, count: u64, limit: u64) -> Result<(), Failure> {
    let next = value.checked_add(count).ok_or(Failure::Budget)?;
    if next > limit {
        return Err(Failure::Budget);
    }
    *value = next;
    Ok(())
}
