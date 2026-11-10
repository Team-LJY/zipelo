pub type Id = [u8; 16];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fault {
    Header,
    Job,
    Sequence,
    Item,
    State,
    Length,
    Budget,
    PartialHeader,
    PartialPayload,
    TrailingData,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub frames: u64,
    pub wire_bytes: u64,
    pub output_bytes: u64,
    pub items: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            frames: 65_536,
            wire_bytes: 21 * 1024 * 1024,
            output_bytes: 16 * 1024 * 1024,
            items: 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamEnd {
    Open,
    Boundary,
    PartialHeader,
    PartialPayload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamSummary {
    pub frames: u64,
    pub wire_bytes: u64,
    pub output_bytes: u64,
    pub items: u32,
    pub completed: bool,
    pub end: StreamEnd,
    pub fault: Option<Fault>,
}

pub struct Stream {
    job: Id,
    limits: Limits,
    header: [u8; 64],
    header_used: usize,
    payload: [u8; 8],
    payload_used: usize,
    payload_len: usize,
    kind: u16,
    item: Id,
    begun: bool,
    active: Option<(Id, u64, u64)>,
    summary: StreamSummary,
}

fn u64_at(bytes: &[u8], start: usize) -> u64 {
    u64::from_le_bytes(bytes[start..start + 8].try_into().unwrap())
}

impl Stream {
    pub fn new(job: Id, limits: Limits) -> Result<Self, Fault> {
        let cap = Limits::default();
        if job == [0; 16] {
            return Err(Fault::Job);
        }
        if limits.frames == 0
            || limits.frames > cap.frames
            || limits.wire_bytes == 0
            || limits.wire_bytes > cap.wire_bytes
            || limits.output_bytes > cap.output_bytes
            || limits.items > cap.items
        {
            return Err(Fault::Budget);
        }
        Ok(Self {
            job,
            limits,
            header: [0; 64],
            header_used: 0,
            payload: [0; 8],
            payload_used: 0,
            payload_len: 0,
            kind: 0,
            item: [0; 16],
            begun: false,
            active: None,
            summary: StreamSummary {
                frames: 0,
                wire_bytes: 0,
                output_bytes: 0,
                items: 0,
                completed: false,
                end: StreamEnd::Open,
                fault: None,
            },
        })
    }

    pub fn summary(&self) -> StreamSummary {
        self.summary
    }

    fn fail(&mut self, fault: Fault) -> Fault {
        *self.summary.fault.get_or_insert(fault)
    }

    pub fn feed(&mut self, mut bytes: &[u8]) -> Result<(), Fault> {
        if let Some(fault) = self.summary.fault {
            return Err(fault);
        }
        if !bytes.is_empty() && (self.summary.end != StreamEnd::Open || self.summary.completed) {
            return Err(self.fail(Fault::TrailingData));
        }
        while !bytes.is_empty() {
            if self.summary.completed {
                return Err(self.fail(Fault::TrailingData));
            }
            if self.header_used < 64 {
                let count = bytes.len().min(64 - self.header_used);
                if self.summary.wire_bytes + count as u64 > self.limits.wire_bytes {
                    return Err(self.fail(Fault::Budget));
                }
                self.header[self.header_used..self.header_used + count]
                    .copy_from_slice(&bytes[..count]);
                self.header_used += count;
                self.summary.wire_bytes += count as u64;
                bytes = &bytes[count..];
                if self.header_used < 64 {
                    continue;
                }
                if let Err(fault) = self.start_frame() {
                    return Err(self.fail(fault));
                }
            }
            let count = bytes.len().min(self.payload_len - self.payload_used);
            // Data 본문은 보관하지 않고 길이만 집계함.
            if self.kind != 3 {
                self.payload[self.payload_used..self.payload_used + count]
                    .copy_from_slice(&bytes[..count]);
            }
            self.payload_used += count;
            self.summary.wire_bytes += count as u64;
            bytes = &bytes[count..];
            if self.payload_used == self.payload_len {
                if let Err(fault) = self.finish_frame() {
                    return Err(self.fail(fault));
                }
                self.header_used = 0;
                self.payload_used = 0;
            }
        }
        Ok(())
    }

    fn start_frame(&mut self) -> Result<(), Fault> {
        let h = &self.header;
        if &h[..4] != b"ZIP2"
            || h[4..6] != 2u16.to_le_bytes()
            || h[12..16] != [0; 4]
            || h[56..64] != [0; 8]
        {
            return Err(Fault::Header);
        }
        if h[16..32] != self.job {
            return Err(Fault::Job);
        }
        if u64_at(h, 48) != self.summary.frames {
            return Err(Fault::Sequence);
        }
        if self.summary.frames >= self.limits.frames {
            return Err(Fault::Budget);
        }
        self.kind = u16::from_le_bytes(h[6..8].try_into().unwrap());
        self.payload_len = u32::from_le_bytes(h[8..12].try_into().unwrap()) as usize;
        self.item.copy_from_slice(&h[32..48]);
        if self.payload_len > 65_536 {
            return Err(Fault::Length);
        }
        if self.summary.wire_bytes + self.payload_len as u64 > self.limits.wire_bytes {
            return Err(Fault::Budget);
        }
        let length_ok = match self.kind {
            1 => self.payload_len == 0,
            2 | 4 => self.payload_len == 8,
            3 => self.payload_len > 0,
            5 => self.payload_len == 1,
            _ => return Err(Fault::Header),
        };
        if !length_ok {
            return Err(Fault::Length);
        }
        match self.kind {
            1 if self.begun || self.item != [0; 16] => return Err(Fault::State),
            2 => {
                if !self.begun || self.active.is_some() {
                    return Err(Fault::State);
                }
                if self.summary.items >= self.limits.items {
                    return Err(Fault::Budget);
                }
                let mut expected = [0; 16];
                expected[..4].copy_from_slice(&(self.summary.items + 1).to_le_bytes());
                if self.item != expected {
                    return Err(Fault::Item);
                }
            }
            3 | 4 => {
                let (id, declared, actual) = self.active.ok_or(Fault::State)?;
                if self.item != id {
                    return Err(Fault::Item);
                }
                if self.kind == 3 {
                    let next = actual + self.payload_len as u64;
                    if next > declared {
                        return Err(Fault::Length);
                    }
                    if self.summary.output_bytes + self.payload_len as u64
                        > self.limits.output_bytes
                    {
                        return Err(Fault::Budget);
                    }
                }
            }
            5 if !self.begun || self.active.is_some() || self.item != [0; 16] => {
                return Err(Fault::State);
            }
            _ => {}
        }
        Ok(())
    }

    fn finish_frame(&mut self) -> Result<(), Fault> {
        match self.kind {
            1 => self.begun = true,
            2 => {
                let declared = u64_at(&self.payload, 0);
                if declared > self.limits.output_bytes - self.summary.output_bytes {
                    return Err(Fault::Budget);
                }
                self.active = Some((self.item, declared, 0));
                self.summary.items += 1;
            }
            3 => {
                let active = self.active.as_mut().ok_or(Fault::State)?;
                active.2 += self.payload_len as u64;
                self.summary.output_bytes += self.payload_len as u64;
            }
            4 => {
                let (_, declared, actual) = self.active.ok_or(Fault::State)?;
                if actual != declared || u64_at(&self.payload, 0) != actual {
                    return Err(Fault::Length);
                }
                self.active = None;
            }
            5 => {
                // 고정 Rust 프로필은 Completed만 출력함.
                if self.payload[0] != 1 {
                    return Err(Fault::State);
                }
                self.summary.completed = true;
            }
            _ => return Err(Fault::Header),
        }
        self.summary.frames += 1;
        Ok(())
    }

    // 입력 끝의 위치만 판정하며 프로세스와 파이프 종료는 입증하지 않음.
    pub fn finish_input(&mut self) -> StreamSummary {
        if self.summary.end == StreamEnd::Open {
            self.summary.end = if self.header_used == 0 {
                StreamEnd::Boundary
            } else if self.header_used < 64 {
                self.fail(Fault::PartialHeader);
                StreamEnd::PartialHeader
            } else {
                self.fail(Fault::PartialPayload);
                StreamEnd::PartialPayload
            };
        }
        self.summary
    }
}
