use std::{
    cell::{Cell, RefCell},
    io::{self, Read, Seek, SeekFrom},
    rc::Rc,
    time::{Duration, Instant},
};

use crate::error::{Failure, Result};

pub const MAX_INPUT: u64 = 16 * 1024 * 1024;
pub const MAX_METADATA: u64 = 2 * 1024 * 1024;
pub const CHUNK: usize = 65_536;

#[derive(Clone, Copy)]
pub struct Limits {
    pub entries: u64,
    pub processed: u64,
    pub output: u64,
    pub duration: Duration,
}

impl Limits {
    pub fn validate(self) -> Result<Self> {
        if self.entries == 0
            || self.processed == 0
            || self.output == 0
            || self.duration.is_zero()
            || self.duration > Duration::from_secs(300)
        {
            return Err(Failure::Unsupported);
        }
        Ok(self)
    }

    pub fn streaming(self) -> Result<Self> {
        self.validate()?;
        if self.entries > 1024 || self.processed > 128 * 1024 * 1024 || self.output > MAX_INPUT {
            return Err(Failure::Budget);
        }
        Ok(self)
    }
}

struct Usage {
    processed: u64,
    output: u64,
}

#[derive(Clone)]
pub struct Budget {
    limits: Limits,
    start: Instant,
    usage: Rc<RefCell<Usage>>,
    failed: Rc<Cell<bool>>,
}

impl Budget {
    pub fn new(limits: Limits) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            limits,
            start: Instant::now(),
            usage: Rc::new(RefCell::new(Usage {
                processed: 0,
                output: 0,
            })),
            failed: Rc::new(Cell::new(false)),
        })
    }

    pub fn check(&self) -> Result<()> {
        if self.failed.get() || self.start.elapsed() >= self.limits.duration {
            self.failed.set(true);
            return Err(Failure::Budget);
        }
        Ok(())
    }

    pub fn entries(&self, count: u64) -> Result<()> {
        self.check()?;
        if count > self.limits.entries.min(1024) {
            self.failed.set(true);
            return Err(Failure::Budget);
        }
        Ok(())
    }

    pub fn charge(&self, processed: u64, output: u64) -> Result<()> {
        self.check()?;
        let mut usage = self.usage.borrow_mut();
        if processed > self.limits.processed - usage.processed
            || output > self.limits.output.min(MAX_INPUT) - usage.output
        {
            self.failed.set(true);
            return Err(Failure::Budget);
        }
        usage.processed += processed;
        usage.output += output;
        Ok(())
    }
}

pub struct Source<R> {
    inner: R,
    length: u64,
    position: u64,
    budget: Budget,
    metadata: Rc<Cell<bool>>,
    metadata_bytes: u64,
    charge_reads: bool,
}

impl<R: Read + Seek> Source<R> {
    pub fn new(inner: R, length: u64, budget: Budget) -> Result<Self> {
        if !(22..=MAX_INPUT).contains(&length) {
            return Err(Failure::Unsupported);
        }
        Ok(Self {
            inner,
            length,
            position: 0,
            budget,
            metadata: Rc::new(Cell::new(true)),
            metadata_bytes: 0,
            charge_reads: true,
        })
    }

    pub fn metadata_switch(&self) -> Rc<Cell<bool>> {
        self.metadata.clone()
    }

    pub fn list_accounting(&mut self) -> Result<()> {
        self.budget.charge(self.length, 0)?;
        self.charge_reads = false;
        Ok(())
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn at(&mut self, offset: u64, count: usize) -> Result<Vec<u8>> {
        if count > CHUNK + 21 || offset > self.length || count as u64 > self.length - offset {
            return Err(Failure::Integrity);
        }
        self.seek(SeekFrom::Start(offset))?;
        let mut data = vec![0; count];
        self.read_exact(&mut data)?;
        Ok(data)
    }
}

impl<R: Read + Seek> Read for Source<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.budget.check()?;
        let count = buffer
            .len()
            .min(CHUNK)
            .min((self.length - self.position) as usize);
        if count == 0 {
            return Ok(0);
        }
        if self.metadata.get() && count as u64 > MAX_METADATA - self.metadata_bytes {
            self.budget.failed.set(true);
            return Err(Failure::Budget.into());
        }
        let got = self.inner.read(&mut buffer[..count])?;
        if got == 0 {
            return Err(Failure::Truncated.into());
        }
        if self.charge_reads {
            self.budget.charge(got as u64, 0)?;
        }
        self.position += got as u64;
        if self.metadata.get() {
            self.metadata_bytes += got as u64;
        }
        Ok(got)
    }
}

impl<R: Read + Seek> Seek for Source<R> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.budget.check()?;
        let position = match from {
            SeekFrom::Start(value) => value as i128,
            SeekFrom::Current(value) => self.position as i128 + value as i128,
            SeekFrom::End(value) => self.length as i128 + value as i128,
        };
        if position < 0 || position > self.length as i128 {
            return Err(Failure::Protocol.into());
        }
        let actual = self.inner.seek(SeekFrom::Start(position as u64))?;
        if actual != position as u64 {
            return Err(Failure::Protocol.into());
        }
        self.position = actual;
        Ok(actual)
    }
}
