use std::{
    cell::{Cell, RefCell},
    fmt,
    io::Write,
    time::{Duration, Instant},
};

use zip::{CompressionMethod, DateTime, ZipWriter, result::ZipError, write::SimpleFileOptions};

mod storage;

pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_INPUTS: usize = 1024;
pub const MAX_NAME_BYTES: usize = 1024;
pub const CHUNK: usize = 65_536;
pub const MAX_DURATION: Duration = Duration::from_secs(300);

#[derive(Debug)]
pub struct Entry {
    pub name: String,
    pub bytes: Box<[u8]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Method {
    Stored,
    #[default]
    Deflate,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub method: Method,
    pub encrypted: bool,
    pub split: bool,
    pub zip64: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub input: u64,
    pub output: u64,
    pub entries: usize,
    pub processed: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            input: MAX_BYTES,
            output: MAX_BYTES,
            entries: MAX_INPUTS + 1,
            processed: 2 * MAX_BYTES,
        }
    }
}

#[derive(Debug)]
pub struct Request {
    pub entries: Vec<Entry>,
    pub options: Options,
    pub limits: Limits,
    pub timeout: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Cancelled,
    Deadline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    InvalidRequest,
    Unsupported,
    UnsafeName,
    Conflict,
    Budget,
    Cancelled,
    Deadline,
    Allocation,
    Engine,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Failure {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checkpoint {
    Start,
    Validate {
        index: usize,
    },
    Entry {
        index: usize,
    },
    Input {
        index: usize,
        offset: usize,
        len: usize,
    },
    Write {
        offset: u64,
        len: usize,
        extent: u64,
    },
    Seek {
        from: u64,
        to: u64,
    },
    Flush,
    Finish,
    Return,
}

pub(crate) struct Gate<C> {
    control: RefCell<C>,
    deadline: Instant,
    first: Cell<Option<Failure>>,
}

impl<C: FnMut(Checkpoint) -> Result<(), Stop>> Gate<C> {
    pub(crate) fn fail(&self, failure: Failure) -> Failure {
        match self.first.get() {
            Some(first) => first,
            None => {
                self.first.set(Some(failure));
                failure
            }
        }
    }

    pub(crate) fn check(&self, point: Checkpoint) -> Result<(), Failure> {
        if let Some(first) = self.first.get() {
            return Err(first);
        }
        if Instant::now() >= self.deadline {
            return Err(self.fail(Failure::Deadline));
        }
        if let Err(stop) = (self.control.borrow_mut())(point) {
            return Err(self.fail(match stop {
                Stop::Cancelled => Failure::Cancelled,
                Stop::Deadline => Failure::Deadline,
            }));
        }
        if Instant::now() >= self.deadline {
            return Err(self.fail(Failure::Deadline));
        }
        Ok(())
    }
}

// 파일 접근 없이 이전 OS 정책의 공통 ASCII 범위만 허용함.
fn validate_name(name: &str) -> Result<(), Failure> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES || !name.is_ascii() {
        return Err(Failure::UnsafeName);
    }
    for segment in name.split('/') {
        if segment.is_empty()
            || segment.len() > 255
            || segment == "."
            || segment == ".."
            || segment.ends_with([' ', '.'])
            || segment.bytes().all(|b| b == b' ')
            || segment
                .bytes()
                .any(|b| b < 32 || b == 127 || b"\\<>:\"|?*".contains(&b))
        {
            return Err(Failure::UnsafeName);
        }
        let stem = segment
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        if matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && (b'1'..=b'9').contains(&stem.as_bytes()[3]))
        {
            return Err(Failure::UnsafeName);
        }
    }
    Ok(())
}

fn io_failure(error: std::io::Error) -> Failure {
    error
        .get_ref()
        .and_then(|e| e.downcast_ref::<Failure>())
        .copied()
        .unwrap_or(Failure::Engine)
}

fn zip_failure(error: ZipError) -> Failure {
    match error {
        ZipError::Io(error) => io_failure(error),
        _ => Failure::Engine,
    }
}

// 소유권을 받은 메모리만 읽음. 성공 전 ZIP bytes를 외부에 노출하지 않음.
pub fn create<C>(request: Request, control: C) -> Result<Vec<u8>, Failure>
where
    C: FnMut(Checkpoint) -> Result<(), Stop>,
{
    if request.timeout.is_zero() || request.timeout > MAX_DURATION {
        return Err(Failure::InvalidRequest);
    }
    let deadline = Instant::now()
        .checked_add(request.timeout)
        .ok_or(Failure::InvalidRequest)?;
    let gate = Gate {
        control: RefCell::new(control),
        deadline,
        first: Cell::new(None),
    };
    gate.check(Checkpoint::Start)?;
    if request.options.encrypted || request.options.split || request.options.zip64 {
        return Err(gate.fail(Failure::Unsupported));
    }
    if request.entries.is_empty() {
        return Err(gate.fail(Failure::InvalidRequest));
    }
    let count = request.entries.len();
    let limits = Limits {
        input: request.limits.input.min(MAX_BYTES),
        output: request.limits.output.min(MAX_BYTES),
        entries: request.limits.entries.min(MAX_INPUTS + 1),
        processed: request.limits.processed.min(2 * MAX_BYTES),
    };
    if count > MAX_INPUTS || count.checked_add(1).ok_or(Failure::Budget)? > limits.entries {
        return Err(gate.fail(Failure::Budget));
    }
    let mut input = 0u64;
    for (index, entry) in request.entries.iter().enumerate() {
        gate.check(Checkpoint::Validate { index })?;
        validate_name(&entry.name).map_err(|e| gate.fail(e))?;
        input = input
            .checked_add(entry.bytes.len() as u64)
            .ok_or(Failure::Budget)?;
        if input > limits.input || input > limits.processed {
            return Err(gate.fail(Failure::Budget));
        }
        for prior in &request.entries[..index] {
            let a = prior.name.as_bytes();
            let b = entry.name.as_bytes();
            let n = a.len().min(b.len());
            if a[..n].eq_ignore_ascii_case(&b[..n])
                && (a.len() == b.len() || a.get(n) == Some(&b'/') || b.get(n) == Some(&b'/'))
            {
                return Err(gate.fail(Failure::Conflict));
            }
        }
    }
    let mut output = storage::Bounded::new(&gate, limits.output, limits.processed, input);
    let mut writer = ZipWriter::new(&mut output);
    let method = match request.options.method {
        Method::Stored => CompressionMethod::Stored,
        Method::Deflate => CompressionMethod::Deflated,
    };
    let options = SimpleFileOptions::default()
        .compression_method(method)
        .compression_level(if request.options.method == Method::Deflate {
            Some(6)
        } else {
            None
        })
        .large_file(false)
        .last_modified_time(DateTime::DEFAULT)
        .unix_permissions(0o644);
    let writing = (|| -> Result<(), Failure> {
        for (index, entry) in request.entries.iter().enumerate() {
            gate.check(Checkpoint::Entry { index })?;
            writer
                .start_file(&entry.name, options)
                .map_err(zip_failure)?;
            for (part, bytes) in entry.bytes.chunks(CHUNK).enumerate() {
                gate.check(Checkpoint::Input {
                    index,
                    offset: part * CHUNK,
                    len: bytes.len(),
                })?;
                writer.write_all(bytes).map_err(io_failure)?;
            }
        }
        gate.check(Checkpoint::Finish)
    })();
    if let Err(error) = writing {
        let first = gate.fail(error);
        drop(writer);
        return Err(first);
    }
    // finish 내부 Drop의 뒤늦은 정리 오류 대신 원래 반환 오류를 보존함.
    if let Err(error) = writer.finish() {
        return Err(zip_failure(error));
    }
    gate.check(Checkpoint::Return)?;
    Ok(output.into_bytes())
}
