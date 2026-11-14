use std::path::{Path, PathBuf};

pub const MAX_PREVIEW_TEXT_BYTES: u64 = 1024 * 1024;
pub const MAX_PREVIEW_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_PREVIEW_RGBA_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_PREVIEW_DIMENSION: u32 = 8192;
pub const MAX_PREVIEW_PIXELS: u64 = 16_000_000;
pub const MAX_PREVIEW_CHUNK_BYTES: usize = 32 * 1024;

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
    pub fn is_basename(&self) -> bool {
        match self {
            Self::Unix(bytes) => {
                !bytes.is_empty()
                    && bytes != b"."
                    && bytes != b".."
                    && !bytes.iter().any(|byte| matches!(*byte, 0 | b'/' | b'\\'))
            }
            Self::Windows(units) => {
                !units.is_empty()
                    && units != &[b'.' as u16]
                    && units != &[b'.' as u16, b'.' as u16]
                    && !units.iter().any(|unit| matches!(*unit, 0 | 47 | 92 | 58))
            }
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
            input: u64::MAX,
            output: 16 * 1024 * 1024 * 1024,
            entries: 10_000,
        }
    }
}
impl Limits {
    pub(crate) fn legacy(self) -> Self {
        Self {
            input: self.input.min(64 * 1024 * 1024),
            output: self.output.min(128 * 1024 * 1024),
            entries: self.entries,
        }
    }

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardedStart {
    pub start: Start,
    pub ceiling: Limits,
    pub interactive: bool,
}
impl GuardedStart {
    pub fn valid(&self) -> bool {
        self.start.limits.valid()
            && self.ceiling.input == self.start.limits.input
            && self.ceiling.output >= self.start.limits.output
            && self.ceiling.output <= 64 * 1024 * 1024 * 1024
            && self.ceiling.entries >= self.start.limits.entries
            && self.ceiling.entries <= 200_000
            && (!self.interactive
                || matches!(
                    self.start.action,
                    Action::Extract { .. } | Action::Preview { .. }
                ))
            && (!matches!(self.start.action, Action::Preview { .. }) || self.interactive)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceScope {
    pub output: bool,
    pub entries: bool,
    pub ratio: bool,
}
impl ResourceScope {
    pub fn any(self) -> bool {
        self.output || self.entries || self.ratio
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceUsage {
    pub estimated_bytes: Option<u64>,
    pub actual_bytes: Option<u64>,
    pub estimated_entries: Option<u32>,
    pub actual_entries: Option<u32>,
    pub compressed_bytes: Option<u64>,
    pub ratio_output_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Zip,
    Tar,
    Gzip,
    TarGzip,
    SevenZip,
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
    SplitSevenZip {
        manifest_sha256: [u8; 32],
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
impl ListId {
    pub fn valid(&self) -> bool {
        matches!(
            (self.catalog_version, self.format, &self.source_object),
            (
                1,
                _,
                SourceObject::Unix { .. } | SourceObject::Windows { .. }
            ) | (2, Format::SevenZip, SourceObject::SplitSevenZip { .. })
        )
    }
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
pub enum SevenZipMethod {
    Lzma2,
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
    Preview {
        archive: NativePath,
        entry_id: u32,
        list_id: ListId,
    },
    RecoverZip {
        archive: NativePath,
        output_file: NativePath,
    },
    CreateSevenZip {
        sources: Vec<Source>,
        output_file: NativePath,
        method: SevenZipMethod,
        solid: bool,
        encryption: Encryption,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    List,
    Extract,
    CreateZip,
    Preview,
    CreateSevenZip,
    RecoverZip,
}
impl Action {
    pub fn kind(&self) -> ActionKind {
        match self {
            Self::List { .. } => ActionKind::List,
            Self::Extract { .. } => ActionKind::Extract,
            Self::CreateZip { .. } => ActionKind::CreateZip,
            Self::Preview { .. } => ActionKind::Preview,
            Self::CreateSevenZip { .. } => ActionKind::CreateSevenZip,
            Self::RecoverZip { .. } => ActionKind::RecoverZip,
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
pub enum PreviewFormat {
    Utf8Text,
    Rgba8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewInfo {
    pub entry_id: u32,
    pub format: PreviewFormat,
    pub total_bytes: u64,
    pub width: u32,
    pub height: u32,
}
impl PreviewInfo {
    pub fn valid(&self) -> bool {
        match self.format {
            PreviewFormat::Utf8Text => {
                self.width == 0 && self.height == 0 && self.total_bytes <= MAX_PREVIEW_TEXT_BYTES
            }
            PreviewFormat::Rgba8 => {
                if !(1..=MAX_PREVIEW_DIMENSION).contains(&self.width)
                    || !(1..=MAX_PREVIEW_DIMENSION).contains(&self.height)
                {
                    return false;
                }
                let pixels = u64::from(self.width) * u64::from(self.height);
                pixels <= MAX_PREVIEW_PIXELS
                    && pixels.checked_mul(4) == Some(self.total_bytes)
                    && self.total_bytes <= MAX_PREVIEW_RGBA_BYTES
            }
        }
    }
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
    DiskFull,
    MissingVolume,
    RecoveryIncomplete,
    RecoveryImpossible,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeProblemReason {
    FirstVolumeRequired,
    MissingVolume,
    IncompleteSet,
}
impl VolumeProblemReason {
    pub fn terminal_error(self) -> ErrorCode {
        match self {
            Self::FirstVolumeRequired => ErrorCode::InvalidRequest,
            Self::MissingVolume => ErrorCode::MissingVolume,
            Self::IncompleteSet => ErrorCode::Integrity,
        }
    }
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictChoice {
    Overwrite,
    Skip,
    Rename,
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
        zeroize::Zeroize::zeroize(&mut self.0);
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
    StartInteractive(Start),
    StartGuarded(GuardedStart),
    ResourceReply {
        challenge_id: u64,
        scope: ResourceScope,
        allow: bool,
    },
    ResourceRequired {
        challenge_id: u64,
        action: ActionKind,
        scope: ResourceScope,
        usage: ResourceUsage,
        soft_limits: Limits,
        ceiling: Limits,
    },
    ConflictReply {
        challenge_id: u64,
        choice: ConflictChoice,
        apply_to_remaining: bool,
    },
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
    PreviewBegin(PreviewInfo),
    PreviewChunk {
        entry_id: u32,
        offset: u64,
        data: Vec<u8>,
    },
    PreviewEnd {
        entry_id: u32,
        total_bytes: u64,
        sha256: [u8; 32],
    },
    VolumeProblem {
        reason: VolumeProblemReason,
        volume: NativePath,
    },
    RecoveryEntry(RecoveryEntry),
    RecoveryEnd(RecoveryEnd),
    Terminal(Terminal),
    CancelAck {
        request_sequence: u64,
        status: CancelStatus,
    },
    ConflictRequired {
        challenge_id: u64,
        entry_id: u32,
        relative_name: String,
        incoming_size: u64,
        existing_size: u64,
    },
    ConflictResolved {
        entry_id: u32,
        relative_name: String,
        choice: ConflictChoice,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryMode {
    CentralDirectory,
    LocalChain,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryStatus {
    Verified,
    Lost,
    Unverified,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryReason {
    None,
    HeaderRebuilt,
    Truncated,
    CrcMismatch,
    DecodeFailure,
    BoundaryUnknown,
    UnsupportedFeature,
    MetadataMissing,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryEntry {
    pub candidate_id: u32,
    pub source_offset: u64,
    pub name: Option<String>,
    pub kind: Option<EntryKind>,
    pub status: RecoveryStatus,
    pub reason: RecoveryReason,
    pub declared_size: Option<u64>,
    pub verified_size: Option<u64>,
    pub sha256: Option<[u8; 32]>,
}
impl RecoveryEntry {
    pub fn valid(&self) -> bool {
        if self.name.as_ref().is_some_and(|name| name.is_empty()) {
            return false;
        }
        if !matches!(
            self.kind,
            None | Some(EntryKind::File | EntryKind::Directory)
        ) {
            return false;
        }
        match self.status {
            RecoveryStatus::Verified => {
                if self.name.as_ref().is_none_or(|name| name.is_empty())
                    || !matches!(
                        self.reason,
                        RecoveryReason::None
                            | RecoveryReason::HeaderRebuilt
                            | RecoveryReason::MetadataMissing
                    )
                    || self.declared_size.is_none()
                    || self.declared_size != self.verified_size
                {
                    return false;
                }
                match self.kind {
                    Some(EntryKind::File) => self.sha256.is_some(),
                    Some(EntryKind::Directory) => {
                        self.declared_size == Some(0) && self.sha256.is_none()
                    }
                    _ => false,
                }
            }
            RecoveryStatus::Lost => {
                self.verified_size.is_none()
                    && self.sha256.is_none()
                    && matches!(
                        self.reason,
                        RecoveryReason::Truncated
                            | RecoveryReason::CrcMismatch
                            | RecoveryReason::DecodeFailure
                    )
            }
            RecoveryStatus::Unverified => {
                self.verified_size.is_none()
                    && self.sha256.is_none()
                    && matches!(
                        self.reason,
                        RecoveryReason::BoundaryUnknown
                            | RecoveryReason::UnsupportedFeature
                            | RecoveryReason::MetadataMissing
                    )
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryEnd {
    pub mode: RecoveryMode,
    pub source_bytes: u64,
    pub source_sha256: [u8; 32],
    pub known_entries: Option<u32>,
    pub verified_entries: u32,
    pub lost_entries: u32,
    pub unverified_entries: u32,
    pub unknown_loss: bool,
}
impl RecoveryEnd {
    pub fn valid(&self) -> bool {
        let total = self
            .verified_entries
            .checked_add(self.lost_entries)
            .and_then(|n| n.checked_add(self.unverified_entries));
        let Some(total) = total else {
            return false;
        };
        match (self.mode, self.known_entries, self.unknown_loss) {
            (RecoveryMode::CentralDirectory, Some(known), false) => known == total,
            (_, None, true) => true,
            _ => false,
        }
    }
    pub fn complete(&self) -> bool {
        self.valid() && !self.unknown_loss && self.lost_entries == 0 && self.unverified_entries == 0
    }
}
