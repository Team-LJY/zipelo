use crate::protocol::*;
use std::time::Duration;

type Result<T> = std::result::Result<T, ErrorCode>;

pub trait Monitor {
    fn metadata_limits(&self) -> crate::archive::MetadataLimits {
        crate::archive::MetadataLimits::default()
    }
    fn check(&mut self) -> Result<()>;
    fn activity(&mut self) -> Result<()> {
        self.check()
    }
    fn observe(&mut self, _usage: ResourceUsage, _storage: Option<&std::fs::File>) -> Result<()> {
        self.check()
    }
}
impl<F: FnMut() -> Result<()>> Monitor for F {
    fn check(&mut self) -> Result<()> {
        self()
    }
}

pub(crate) struct Budget {
    pub soft: Limits,
    pub ceiling: Limits,
    pub granted: ResourceScope,
}
impl Budget {
    pub fn metadata_limits(&self) -> crate::archive::MetadataLimits {
        if self.granted.any() {
            crate::archive::MetadataLimits::approved()
        } else {
            crate::archive::MetadataLimits::default()
        }
    }
    pub fn required(&self, usage: ResourceUsage) -> Result<ResourceScope> {
        let bytes = usage
            .estimated_bytes
            .into_iter()
            .chain(usage.actual_bytes)
            .max();
        let entries = usage
            .estimated_entries
            .into_iter()
            .chain(usage.actual_entries)
            .max();
        if bytes.is_some_and(|n| n > self.ceiling.output)
            || entries.is_some_and(|n| n > self.ceiling.entries)
        {
            return Err(ErrorCode::Limit);
        }
        let ratio = match (usage.ratio_output_bytes, usage.compressed_bytes) {
            (Some(output), Some(input)) => {
                output >= 256 * 1024 * 1024 && u128::from(output) > u128::from(input) * 1000
            }
            _ => false,
        };
        Ok(ResourceScope {
            output: !self.granted.output && bytes.is_some_and(|n| n > self.soft.output),
            entries: !self.granted.entries && entries.is_some_and(|n| n > self.soft.entries),
            ratio: !self.granted.ratio && ratio,
        })
    }
    pub fn grant(&mut self, scope: ResourceScope) {
        self.granted.output |= scope.output;
        self.granted.entries |= scope.entries;
        self.granted.ratio |= scope.ratio;
    }
}

pub(crate) struct Watchdog {
    last_activity: Duration,
    approved: bool,
}
impl Watchdog {
    pub fn new() -> Self {
        Self {
            last_activity: Duration::ZERO,
            approved: false,
        }
    }
    pub fn check(&self, elapsed: Duration) -> Result<()> {
        if if self.approved {
            elapsed.saturating_sub(self.last_activity) >= Duration::from_secs(60)
        } else {
            elapsed > Duration::from_secs(300)
        } {
            Err(ErrorCode::Limit)
        } else {
            Ok(())
        }
    }
    pub fn activity(&mut self, elapsed: Duration) -> Result<()> {
        self.check(elapsed)?;
        self.last_activity = elapsed;
        Ok(())
    }
    pub fn resume(&mut self, elapsed: Duration, approved: bool) {
        self.approved |= approved;
        self.last_activity = elapsed;
    }
}

#[cfg(test)]
#[path = "../../../tests/rust-archive-worker/resource_budget.rs"]
mod tests;
