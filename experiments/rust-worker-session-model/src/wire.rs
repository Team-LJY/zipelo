pub const HEADER_BYTES: usize = 64;
pub const CHUNK_BYTES: usize = 65_536;
pub const PROFILE: &str = "gzip-single-member-session-model-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestId([u8; 16]);

impl RequestId {
    // 호출자가 제공한 상관 ID임. 실행 identity 증명이 아님.
    pub fn new(bytes: [u8; 16]) -> Option<Self> {
        (bytes != [0; 16]).then_some(Self(bytes))
    }

    pub fn bytes(self) -> [u8; 16] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Kind {
    RequestBegin = 1,
    RequestData = 2,
    RequestEnd = 3,
    ResultBegin = 16,
    ResultData = 17,
    ResultEnd = 18,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    InvalidHeader,
    InvalidLength,
    IdMismatch,
    SequenceMismatch,
    UnexpectedFrame,
    FramesExceeded,
    WireExceeded,
    DataExceeded,
    Truncated,
    TrailingData,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WorkerStatus {
    Completed = 1,
    Failed = 2,
    Cancelled = 3,
}

impl WorkerStatus {
    pub(crate) fn parse(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Completed),
            2 => Ok(Self::Failed),
            3 => Ok(Self::Cancelled),
            _ => Err(ProtocolError::InvalidHeader),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    kind: Kind,
    id: RequestId,
    sequence: u64,
    length: u32,
}

impl Header {
    // 본문 할당 전에 고정 헤더와 종류별 상한을 검사함.
    pub fn inspect(bytes: &[u8; HEADER_BYTES]) -> Result<Self, ProtocolError> {
        if &bytes[..4] != b"ZWM1"
            || bytes[4..6] != 1_u16.to_le_bytes()
            || bytes[12..16] != [0; 4]
            || bytes[40..] != [0; 24]
        {
            return Err(ProtocolError::InvalidHeader);
        }
        let kind = match u16::from_le_bytes(bytes[6..8].try_into().expect("fixed kind width")) {
            1 => Kind::RequestBegin,
            2 => Kind::RequestData,
            3 => Kind::RequestEnd,
            16 => Kind::ResultBegin,
            17 => Kind::ResultData,
            18 => Kind::ResultEnd,
            _ => return Err(ProtocolError::InvalidHeader),
        };
        let length = u32::from_le_bytes(bytes[8..12].try_into().expect("fixed length width"));
        let valid = match kind {
            Kind::RequestBegin => length == 16,
            Kind::RequestData | Kind::ResultData => (1..=CHUNK_BYTES as u32).contains(&length),
            Kind::RequestEnd => length == 0,
            Kind::ResultBegin => length == 8,
            Kind::ResultEnd => length == 1,
        };
        if !valid {
            return Err(ProtocolError::InvalidLength);
        }
        let id = RequestId::new(bytes[16..32].try_into().expect("fixed id width"))
            .ok_or(ProtocolError::InvalidHeader)?;
        Ok(Self {
            kind,
            id,
            sequence: u64::from_le_bytes(bytes[32..40].try_into().expect("fixed sequence width")),
            length,
        })
    }

    pub fn kind(self) -> Kind {
        self.kind
    }

    pub fn id(self) -> RequestId {
        self.id
    }

    pub fn sequence(self) -> u64 {
        self.sequence
    }

    pub fn payload_length(self) -> usize {
        self.length as usize
    }
}

// 직렬화는 인증이나 상대 수신 확인을 제공하지 않음.
pub fn encode_header(kind: Kind, id: RequestId, sequence: u64, length: u32) -> [u8; HEADER_BYTES] {
    let mut bytes = [0; HEADER_BYTES];
    bytes[..4].copy_from_slice(b"ZWM1");
    bytes[4..6].copy_from_slice(&1_u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&(kind as u16).to_le_bytes());
    bytes[8..12].copy_from_slice(&length.to_le_bytes());
    bytes[16..32].copy_from_slice(&id.bytes());
    bytes[32..40].copy_from_slice(&sequence.to_le_bytes());
    bytes
}
