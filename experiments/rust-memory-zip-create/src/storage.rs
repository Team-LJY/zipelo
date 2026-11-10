use std::io::{self, Seek, SeekFrom, Write};

use super::{Checkpoint, Failure, Gate, Stop};

pub(crate) struct Bounded<'a, C> {
    gate: &'a Gate<C>,
    bytes: Vec<u8>,
    position: u64,
    output_limit: u64,
    processed_limit: u64,
    input: u64,
}

impl<'a, C: FnMut(Checkpoint) -> Result<(), Stop>> Bounded<'a, C> {
    pub(crate) fn new(
        gate: &'a Gate<C>,
        output_limit: u64,
        processed_limit: u64,
        input: u64,
    ) -> Self {
        Self {
            gate,
            bytes: Vec::new(),
            position: 0,
            output_limit,
            processed_limit,
            input,
        }
    }

    fn reject(&self, error: Failure) -> io::Error {
        io::Error::other(self.gate.fail(error))
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl<C: FnMut(Checkpoint) -> Result<(), Stop>> Write for Bounded<'_, C> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.gate
            .check(Checkpoint::Write {
                offset: self.position,
                len: data.len(),
                extent: self.bytes.len() as u64,
            })
            .map_err(io::Error::other)?;
        let end = self
            .position
            .checked_add(data.len() as u64)
            .ok_or_else(|| self.reject(Failure::Budget))?;
        let extent = end.max(self.bytes.len() as u64);
        let processed = self
            .input
            .checked_add(extent)
            .ok_or_else(|| self.reject(Failure::Budget))?;
        if extent > self.output_limit || processed > self.processed_limit {
            return Err(self.reject(Failure::Budget));
        }
        let end = usize::try_from(end).map_err(|_| self.reject(Failure::Budget))?;
        if end > self.bytes.len() {
            self.bytes
                .try_reserve_exact(end - self.bytes.len())
                .map_err(|_| self.reject(Failure::Allocation))?;
            self.bytes.resize(end, 0);
        }
        self.bytes[self.position as usize..end].copy_from_slice(data);
        self.position = end as u64;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.gate.check(Checkpoint::Flush).map_err(io::Error::other)
    }
}

impl<C: FnMut(Checkpoint) -> Result<(), Stop>> Seek for Bounded<'_, C> {
    fn seek(&mut self, seek: SeekFrom) -> io::Result<u64> {
        let to = match seek {
            SeekFrom::Start(to) => i128::from(to),
            SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
            SeekFrom::End(delta) => self.bytes.len() as i128 + i128::from(delta),
        };
        if to < 0 || to > self.bytes.len() as i128 {
            return Err(self.reject(Failure::Engine));
        }
        let to = to as u64;
        self.gate
            .check(Checkpoint::Seek {
                from: self.position,
                to,
            })
            .map_err(io::Error::other)?;
        self.position = to;
        Ok(to)
    }
}
