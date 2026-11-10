use std::{fmt, io};
use zip::result::ZipError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    Unsupported,
    UnsupportedEntry,
    Integrity,
    Budget,
    Protocol,
    Truncated,
    Engine,
    DuplicateName,
    MetadataMismatch,
}

pub type Result<T> = std::result::Result<T, Failure>;

impl Failure {
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Unsupported => 10,
            Self::UnsupportedEntry => 11,
            Self::Integrity => 12,
            Self::Budget => 13,
            Self::Protocol => 14,
            Self::Truncated => 15,
            Self::Engine => 16,
            Self::DuplicateName => 17,
            Self::MetadataMismatch => 18,
        }
    }

    pub fn archive_code(self) -> u8 {
        match self {
            Self::Unsupported => 2,
            Self::UnsupportedEntry => 4,
            Self::Budget => 7,
            Self::Integrity | Self::Truncated | Self::MetadataMismatch => 10,
            Self::Protocol | Self::Engine | Self::DuplicateName => 12,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unsupported => "UnsupportedZipSubset",
            Self::UnsupportedEntry => "UnsupportedEntry",
            Self::Integrity => "IntegrityFailure",
            Self::Budget => "BudgetExceeded",
            Self::Protocol => "ProtocolFailure",
            Self::Truncated => "ObservedUnexpectedEof",
            Self::Engine => "EngineFailure",
            Self::DuplicateName => "DuplicateNameWouldBeCollapsed",
            Self::MetadataMismatch => "LocalCentralOrAdapterMismatch",
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

impl std::error::Error for Failure {}

impl From<Failure> for io::Error {
    fn from(value: Failure) -> Self {
        io::Error::other(value)
    }
}

impl From<io::Error> for Failure {
    fn from(value: io::Error) -> Self {
        if let Some(failure) = value.get_ref().and_then(|e| e.downcast_ref::<Failure>()) {
            return *failure;
        }
        if let Some(failure) = value.get_ref().and_then(|e| e.downcast_ref::<ZipError>()) {
            return match failure {
                ZipError::UnsupportedArchive(_) | ZipError::CompressionMethodNotSupported(_) => {
                    Self::Unsupported
                }
                _ => Self::Integrity,
            };
        }
        match value.kind() {
            io::ErrorKind::UnexpectedEof => Self::Truncated,
            io::ErrorKind::InvalidData => Self::Integrity,
            _ => Self::Engine,
        }
    }
}

impl From<ZipError> for Failure {
    fn from(value: ZipError) -> Self {
        match value {
            ZipError::Io(error) => Self::from(error),
            ZipError::UnsupportedArchive(_)
            | ZipError::CompressionMethodNotSupported(_)
            | ZipError::InvalidPassword => Self::Unsupported,
            ZipError::DecompressedSizeLimitExceeded { .. } => Self::Budget,
            _ => Self::Integrity,
        }
    }
}
