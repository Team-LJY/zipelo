use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    time::Duration,
};

use crate::{
    budget::{Budget, CHUNK, Limits, MAX_INPUT},
    error::{Failure, Result},
};

pub type Id = [u8; 16];

pub fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

pub fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("fixed-width field"),
    )
}

pub fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("fixed-width field"),
    )
}

pub fn parse_id(text: &str) -> Result<Id> {
    if text.len() != 36 {
        return Err(Failure::Protocol);
    }
    let mut network = [0; 16];
    let mut nibble = 0;
    for (position, value) in text.bytes().enumerate() {
        if matches!(position, 8 | 13 | 18 | 23) {
            if value != b'-' {
                return Err(Failure::Protocol);
            }
            continue;
        }
        let value = match value {
            b'0'..=b'9' => value - b'0',
            b'a'..=b'f' => value - b'a' + 10,
            b'A'..=b'F' => value - b'A' + 10,
            _ => return Err(Failure::Protocol),
        };
        network[nibble / 2] = (network[nibble / 2] << 4) | value;
        nibble += 1;
    }
    network[..4].reverse();
    network[4..6].reverse();
    network[6..8].reverse();
    if network == [0; 16] {
        return Err(Failure::Protocol);
    }
    Ok(network)
}

pub fn item_id(index: usize) -> Id {
    let mut id = [0; 16];
    id[..4].copy_from_slice(&((index + 1) as u32).to_le_bytes());
    id
}

pub struct Config {
    pub id: Id,
    pub length: u64,
    pub limits: Limits,
}

pub fn read_config(input: &mut impl Read) -> Result<Config> {
    let mut bytes = [0; 64];
    input.read_exact(&mut bytes)?;
    let id = bytes[8..24].try_into().map_err(|_| Failure::Protocol)?;
    let length = u64_at(&bytes, 24);
    let ticks = u64_at(&bytes, 56);
    if &bytes[..4] != b"ZXE2"
        || u32_at(&bytes, 4) != 2
        || id == [0; 16]
        || !(22..=MAX_INPUT).contains(&length)
        || ticks == 0
        || ticks > 3_000_000_000
    {
        return Err(Failure::Protocol);
    }
    let limits = Limits {
        entries: u64_at(&bytes, 48),
        processed: u64_at(&bytes, 32),
        output: u64_at(&bytes, 40),
        duration: Duration::from_nanos(ticks * 100),
    }
    .streaming()?;
    Ok(Config { id, length, limits })
}

pub struct Remote<R, W> {
    replies: R,
    requests: W,
    id: Id,
    length: u64,
    position: u64,
    sequence: u64,
    served: u64,
}

impl<R: Read, W: Write> Remote<R, W> {
    pub fn new(replies: R, requests: W, config: &Config) -> Self {
        Self {
            replies,
            requests,
            id: config.id,
            length: config.length,
            position: 0,
            sequence: 0,
            served: 0,
        }
    }
}

impl<R: Read, W: Write> Read for Remote<R, W> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let count = bytes
            .len()
            .min(CHUNK)
            .min((self.length - self.position) as usize);
        if count == 0 {
            return Ok(0);
        }
        if self.sequence >= 65_536 || count as u64 > 48 * 1024 * 1024 - self.served {
            return Err(Failure::Budget.into());
        }
        let mut header = [0; 48];
        header[..4].copy_from_slice(b"ZXR2");
        header[4..8].copy_from_slice(&2u32.to_le_bytes());
        header[8..24].copy_from_slice(&self.id);
        header[24..32].copy_from_slice(&self.sequence.to_le_bytes());
        header[32..40].copy_from_slice(&self.position.to_le_bytes());
        header[40..44].copy_from_slice(&(count as u32).to_le_bytes());
        self.requests.write_all(&header)?;
        self.requests.flush()?;
        let mut response = [0; 48];
        self.replies.read_exact(&mut response)?;
        if response != header {
            return Err(Failure::Protocol.into());
        }
        self.replies.read_exact(&mut bytes[..count])?;
        self.position += count as u64;
        self.sequence += 1;
        self.served += count as u64;
        Ok(count)
    }
}

impl<R: Read, W: Write> Seek for Remote<R, W> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = match from {
            SeekFrom::Start(value) => value as i128,
            SeekFrom::Current(value) => self.position as i128 + value as i128,
            SeekFrom::End(value) => self.length as i128 + value as i128,
        };
        if position < 0 || position > self.length as i128 {
            return Err(Failure::Protocol.into());
        }
        self.position = position as u64;
        Ok(self.position)
    }
}

pub struct Frames<W> {
    output: W,
    id: Id,
    sequence: u64,
    wire_bytes: u64,
    items: usize,
    active: Option<(Id, u64, u64)>,
    begun: bool,
    terminal: bool,
    budget: Budget,
}

impl<W: Write> Frames<W> {
    pub fn new(output: W, id: Id, budget: Budget) -> Result<Self> {
        if id == [0; 16] {
            return Err(Failure::Protocol);
        }
        Ok(Self {
            output,
            id,
            sequence: 0,
            wire_bytes: 0,
            items: 0,
            active: None,
            begun: false,
            terminal: false,
            budget,
        })
    }

    fn send(&mut self, kind: u16, item: Id, bytes: &[u8]) -> Result<()> {
        self.budget.check()?;
        let length = 64 + bytes.len() as u64;
        if self.terminal
            || bytes.len() > CHUNK
            || self.sequence >= 65_536
            || length > 21 * 1024 * 1024 - self.wire_bytes
        {
            return Err(Failure::Budget);
        }
        let mut header = [0; 64];
        header[..4].copy_from_slice(b"ZIP2");
        header[4..6].copy_from_slice(&2u16.to_le_bytes());
        header[6..8].copy_from_slice(&kind.to_le_bytes());
        header[8..12].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        header[16..32].copy_from_slice(&self.id);
        header[32..48].copy_from_slice(&item);
        header[48..56].copy_from_slice(&self.sequence.to_le_bytes());
        self.output.write_all(&header)?;
        self.output.write_all(bytes)?;
        self.output.flush()?;
        self.sequence += 1;
        self.wire_bytes += length;
        Ok(())
    }

    pub fn begin(&mut self) -> Result<()> {
        if self.begun {
            return Err(Failure::Protocol);
        }
        self.send(1, [0; 16], &[])?;
        self.begun = true;
        Ok(())
    }

    pub fn begin_item(&mut self, index: usize, length: u64) -> Result<()> {
        if !self.begun || self.active.is_some() || index != self.items {
            return Err(Failure::Protocol);
        }
        self.budget.entries((index + 1) as u64)?;
        let id = item_id(index);
        self.send(2, id, &length.to_le_bytes())?;
        self.active = Some((id, length, 0));
        self.items += 1;
        Ok(())
    }

    pub fn data(&mut self, bytes: &[u8]) -> Result<()> {
        let (id, declared, actual) = self.active.ok_or(Failure::Protocol)?;
        if bytes.is_empty() || bytes.len() > CHUNK || bytes.len() as u64 > declared - actual {
            return Err(Failure::Integrity);
        }
        self.budget.charge(bytes.len() as u64, bytes.len() as u64)?;
        self.send(3, id, bytes)?;
        self.active = Some((id, declared, actual + bytes.len() as u64));
        Ok(())
    }

    pub fn end_item(&mut self) -> Result<()> {
        let (id, declared, actual) = self.active.ok_or(Failure::Protocol)?;
        if declared != actual {
            return Err(Failure::Integrity);
        }
        self.send(4, id, &actual.to_le_bytes())?;
        self.active = None;
        Ok(())
    }

    pub fn complete(&mut self) -> Result<()> {
        if !self.begun || self.active.is_some() {
            return Err(Failure::Protocol);
        }
        self.send(5, [0; 16], &[1])?;
        self.terminal = true;
        Ok(())
    }
}
