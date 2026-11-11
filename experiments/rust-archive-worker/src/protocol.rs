use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativePath {
    Unix(Vec<u8>),
    Windows(Vec<u16>),
}
impl NativePath {
    pub fn from_path(path: &Path) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self::Unix(path.as_os_str().as_bytes().to_vec())
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            Self::Windows(path.as_os_str().encode_wide().collect())
        }
    }
    pub fn to_path(&self) -> Result<PathBuf, ErrorCode> {
        #[cfg(unix)]
        if let Self::Unix(bytes) = self {
            use std::os::unix::ffi::OsStringExt;
            if bytes.contains(&0) {
                return Err(ErrorCode::InvalidRequest);
            }
            let path = PathBuf::from(std::ffi::OsString::from_vec(bytes.clone()));
            return if path.is_absolute() {
                Ok(path)
            } else {
                Err(ErrorCode::InvalidRequest)
            };
        }
        #[cfg(windows)]
        if let Self::Windows(units) = self {
            use std::os::windows::ffi::OsStringExt;
            if units.contains(&0) {
                return Err(ErrorCode::InvalidRequest);
            }
            let path = PathBuf::from(std::ffi::OsString::from_wide(units));
            let text = path.to_string_lossy();
            if path.is_absolute() && !text.starts_with("\\\\") && !text.starts_with("//") {
                return Ok(path);
            }
            return Err(ErrorCode::InvalidRequest);
        }
        Err(ErrorCode::UnsupportedFeature)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub input: u64,
    pub output: u64,
    pub entries: u32,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            input: 64 * 1024 * 1024,
            output: 128 * 1024 * 1024,
            entries: 10_000,
        }
    }
}
impl Limits {
    pub fn valid(self) -> bool {
        let hard = Self::default();
        self.input > 0
            && self.input <= hard.input
            && self.output > 0
            && self.output <= hard.output
            && self.entries > 0
            && self.entries <= hard.entries
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Zip,
    Tar,
    Gzip,
    TarGzip,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceObject {
    Unix {
        device: u64,
        inode: u64,
    },
    Windows {
        volume_serial: u64,
        file_id: [u8; 16],
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListId {
    pub catalog_version: u16,
    pub format: Format,
    pub source_object: SourceObject,
    pub archive_bytes: u64,
    pub archive_sha256: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    All,
    EntryIds(Vec<u32>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Store,
    Deflate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encryption {
    None,
    Aes256,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub path: NativePath,
    pub archive_name: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    List {
        archive: NativePath,
    },
    Extract {
        archive: NativePath,
        destination: NativePath,
        selection: Selection,
        list_id: Option<ListId>,
        gzip_output_name: Option<String>,
    },
    CreateZip {
        sources: Vec<Source>,
        output_file: NativePath,
        method: Method,
        encryption: Encryption,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    List,
    Extract,
    CreateZip,
}
impl Action {
    pub fn kind(&self) -> ActionKind {
        match self {
            Self::List { .. } => ActionKind::List,
            Self::Extract { .. } => ActionKind::Extract,
            Self::CreateZip { .. } => ActionKind::CreateZip,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Start {
    pub action: Action,
    pub limits: Limits,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Link,
    Other,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub entry_id: u32,
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub compressed_size: u64,
    pub encrypted: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Scan,
    Read,
    Decode,
    Write,
    Verify,
    Commit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    Created,
    PartialRetained,
    Removed,
    CommitUncertain,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Failed,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    None,
    Integrity,
    UnsupportedFormat,
    UnsupportedFeature,
    WrongPassword,
    UnsafePath,
    Conflict,
    Limit,
    Io,
    Protocol,
    Internal,
    SourceChanged,
    InvalidRequest,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Publication {
    None,
    Partial,
    Complete,
    Uncertain,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    NotApplicable,
    Synced,
    Unconfirmed,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cleanup {
    Complete,
    Failed,
    Unconfirmed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelStatus {
    Requested,
    AlreadyRequested,
    TooLate,
    Finished,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordPurpose {
    Decrypt,
    Encrypt,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Terminal {
    pub outcome: Outcome,
    pub error: ErrorCode,
    pub publication: Publication,
    pub created_count: u64,
    pub retained_partial_count: u64,
    pub durability: Durability,
    pub cleanup: Cleanup,
    pub detail: String,
}
pub struct Secret(pub Vec<u8>);
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}
impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
#[derive(Debug)]
pub enum Message {
    Start(Start),
    Password {
        challenge_id: u64,
        secret: Secret,
    },
    Cancel,
    Accepted {
        action: ActionKind,
        encryption: bool,
    },
    Entry(Entry),
    ListEnd {
        entry_count: u32,
        list_id: ListId,
    },
    PasswordRequired {
        challenge_id: u64,
        attempt: u32,
        purpose: PasswordPurpose,
    },
    Progress {
        stage: Stage,
        processed_bytes: u64,
        total_bytes: Option<u64>,
        completed_entries: u32,
    },
    Output {
        entry_id: Option<u32>,
        relative_name: String,
        disposition: Disposition,
    },
    Terminal(Terminal),
    CancelAck {
        request_sequence: u64,
        status: CancelStatus,
    },
}
