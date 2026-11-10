use std::time::Duration;

pub const MAX_SNAPSHOT: usize = 16 * 1024 * 1024;
pub const MAX_REQUESTS: u64 = 65_536;
pub const MAX_CHUNK: usize = 65_536;
pub const MAX_SERVED: u64 = 48 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangeFault {
    Config,
    Header,
    Job,
    Sequence,
    Bounds,
    Budget,
    PartialHeader,
    OutstandingResponse,
    WriteProgress,
    Closed,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub job: [u8; 16],
    pub processed: u64,
    pub output: u64,
    pub entries: u64,
    pub duration: Duration,
}

impl Config {
    pub fn encode(self, length: usize) -> Result<[u8; 64], RangeFault> {
        let ticks = self.duration.as_nanos() / 100;
        if self.job == [0; 16]
            || !(22..=MAX_SNAPSHOT).contains(&length)
            || self.processed == 0
            || self.processed > 128 * 1024 * 1024
            || self.output == 0
            || self.output > MAX_SNAPSHOT as u64
            || self.entries == 0
            || self.entries > 1024
            || ticks == 0
            || self.duration > Duration::from_secs(300)
        {
            return Err(RangeFault::Config);
        }
        let mut bytes = [0; 64];
        bytes[..4].copy_from_slice(b"ZXE2");
        bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
        bytes[8..24].copy_from_slice(&self.job);
        bytes[24..32].copy_from_slice(&(length as u64).to_le_bytes());
        bytes[32..40].copy_from_slice(&self.processed.to_le_bytes());
        bytes[40..48].copy_from_slice(&self.output.to_le_bytes());
        bytes[48..56].copy_from_slice(&self.entries.to_le_bytes());
        bytes[56..64].copy_from_slice(&(ticks as u64).to_le_bytes());
        Ok(bytes)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RangeLimits {
    pub requests: u64,
    pub served: u64,
}

impl Default for RangeLimits {
    fn default() -> Self {
        Self {
            requests: MAX_REQUESTS,
            served: MAX_SERVED,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    pub requests: u64,
    pub reserved_bytes: u64,
    pub sent_bytes: u64,
}

struct Reply {
    offset: usize,
    count: usize,
    written: usize,
}

pub struct RangeMachine {
    snapshot: Box<[u8]>,
    job: [u8; 16],
    config: [u8; 64],
    config_sent: usize,
    header: [u8; 48],
    header_used: usize,
    reply: Option<Reply>,
    limits: RangeLimits,
    stats: Stats,
    eof: bool,
    aborted: bool,
    fault: Option<RangeFault>,
}

impl RangeMachine {
    pub fn new(
        snapshot: Box<[u8]>,
        config: Config,
        limits: RangeLimits,
    ) -> Result<Self, RangeFault> {
        let encoded = config.encode(snapshot.len())?;
        if limits.requests == 0
            || limits.requests > MAX_REQUESTS
            || limits.served == 0
            || limits.served > MAX_SERVED
        {
            return Err(RangeFault::Budget);
        }
        Ok(Self {
            snapshot,
            job: config.job,
            config: encoded,
            config_sent: 0,
            header: [0; 48],
            header_used: 0,
            reply: None,
            limits,
            stats: Stats::default(),
            eof: false,
            aborted: false,
            fault: None,
        })
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }
    pub fn fault(&self) -> Option<RangeFault> {
        self.fault
    }
    pub fn request_eof_seen(&self) -> bool {
        self.eof
    }

    fn fail(&mut self, fault: RangeFault) -> RangeFault {
        *self.fault.get_or_insert(fault)
    }

    // 호출자는 이 길이까지만 읽어 한 요청의 응답이 끝날 때까지 역압력을 유지함.
    pub fn request_capacity(&self) -> usize {
        if self.config_sent < 64
            || self.reply.is_some()
            || self.eof
            || self.aborted
            || self.fault.is_some()
        {
            0
        } else {
            48 - self.header_used
        }
    }

    pub fn feed_request(&mut self, bytes: &[u8]) -> Result<(), RangeFault> {
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        if self.eof || self.aborted {
            return Err(self.fail(RangeFault::Closed));
        }
        if bytes.is_empty() {
            return Ok(());
        }
        if bytes.len() > self.request_capacity() {
            return Err(self.fail(RangeFault::OutstandingResponse));
        }
        let end = self
            .header_used
            .checked_add(bytes.len())
            .ok_or(RangeFault::Bounds)?;
        self.header[self.header_used..end].copy_from_slice(bytes);
        self.header_used = end;
        if end == 48 {
            if let Err(fault) = self.accept_header() {
                return Err(self.fail(fault));
            }
        }
        Ok(())
    }

    fn accept_header(&mut self) -> Result<(), RangeFault> {
        let h = &self.header;
        if &h[..4] != b"ZXR2" || h[4..8] != 2u32.to_le_bytes() || h[44..48] != [0; 4] {
            return Err(RangeFault::Header);
        }
        if h[8..24] != self.job {
            return Err(RangeFault::Job);
        }
        let sequence = u64::from_le_bytes(h[24..32].try_into().unwrap());
        if sequence != self.stats.requests {
            return Err(RangeFault::Sequence);
        }
        let offset = u64::from_le_bytes(h[32..40].try_into().unwrap());
        let count = u32::from_le_bytes(h[40..44].try_into().unwrap()) as u64;
        let end = offset.checked_add(count).ok_or(RangeFault::Bounds)?;
        if count == 0 || count > MAX_CHUNK as u64 || end > self.snapshot.len() as u64 {
            return Err(RangeFault::Bounds);
        }
        let requests = self
            .stats
            .requests
            .checked_add(1)
            .ok_or(RangeFault::Budget)?;
        let reserved = self
            .stats
            .reserved_bytes
            .checked_add(count)
            .ok_or(RangeFault::Budget)?;
        if requests > self.limits.requests || reserved > self.limits.served {
            return Err(RangeFault::Budget);
        }
        self.reply = Some(Reply {
            offset: usize::try_from(offset).map_err(|_| RangeFault::Bounds)?,
            count: usize::try_from(count).map_err(|_| RangeFault::Bounds)?,
            written: 0,
        });
        self.stats.requests = requests;
        self.stats.reserved_bytes = reserved;
        Ok(())
    }

    // 설정 또는 응답의 남은 한 조각만 빌려줌. 추가 payload 복사는 하지 않음.
    pub fn outbound(&self) -> Option<&[u8]> {
        if self.aborted || self.fault.is_some() {
            return None;
        }
        if self.config_sent < 64 {
            return Some(&self.config[self.config_sent..]);
        }
        let reply = self.reply.as_ref()?;
        if reply.written < 48 {
            Some(&self.header[reply.written..])
        } else {
            let start = reply.offset + reply.written - 48;
            Some(&self.snapshot[start..reply.offset + reply.count])
        }
    }

    // 실제 write가 보고한 양만 전달함. EAGAIN/EINTR은 상태를 바꾸지 않음.
    pub fn written(&mut self, count: usize) -> Result<(), RangeFault> {
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        let available = self.outbound().map_or(0, <[u8]>::len);
        if count == 0 || count > available {
            return Err(self.fail(RangeFault::WriteProgress));
        }
        if self.config_sent < 64 {
            self.config_sent = self
                .config_sent
                .checked_add(count)
                .ok_or(RangeFault::WriteProgress)?;
        } else {
            let reply = self.reply.as_mut().ok_or(RangeFault::WriteProgress)?;
            if reply.written >= 48 {
                self.stats.sent_bytes = self
                    .stats
                    .sent_bytes
                    .checked_add(count as u64)
                    .ok_or(RangeFault::Budget)?;
            }
            reply.written = reply
                .written
                .checked_add(count)
                .ok_or(RangeFault::WriteProgress)?;
            if reply.written == 48 + reply.count {
                self.reply = None;
                self.header_used = 0;
            }
        }
        Ok(())
    }

    pub fn request_eof(&mut self) -> Result<(), RangeFault> {
        if self.eof {
            return Err(self.fail(RangeFault::Closed));
        }
        self.eof = true;
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        if self.header_used > 0 && self.header_used < 48 {
            return Err(self.fail(RangeFault::PartialHeader));
        }
        if self.config_sent < 64 || self.reply.is_some() {
            return Err(self.fail(RangeFault::OutstandingResponse));
        }
        Ok(())
    }

    // 응답 폐기는 쓰기 완료나 EOF 증거가 아님.
    pub fn abort(&mut self) {
        self.aborted = true;
        self.reply = None;
    }
}
