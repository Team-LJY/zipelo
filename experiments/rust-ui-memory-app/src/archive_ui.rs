use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    Zip,
    Tar,
    Gzip,
    TarGzip,
    SevenZip,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceObject {
    Unix { device: u64, inode: u64 },
    Windows { volume: u64, file_id: [u8; 16] },
    SplitSevenZip { manifest_sha256: [u8; 32] },
}
#[derive(Clone, Debug, Eq, PartialEq)]
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
    pub fn is_split(&self) -> bool {
        matches!(self.source_object, SourceObject::SplitSevenZip { .. })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolumeProblem {
    pub reason: VolumeProblemReason,
    pub volume: NativePath,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selection {
    All,
    EntryIds(Vec<u32>),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    Store,
    Deflate,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Start {
    List {
        archive: PathBuf,
    },
    Extract {
        archive: PathBuf,
        destination: PathBuf,
        selection: Selection,
        list_id: ListId,
        gzip_output_name: Option<String>,
    },
    Preview {
        archive: PathBuf,
        entry_id: u32,
        list_id: ListId,
    },
    RecoverZip {
        archive: PathBuf,
        output_file: PathBuf,
    },
    CreateSevenZip {
        sources: Vec<(PathBuf, String)>,
        output_file: PathBuf,
    },
    CreateZip {
        sources: Vec<(PathBuf, String)>,
        output_file: PathBuf,
        method: Method,
        aes256: bool,
    },
}
// 암호를 Debug와 로그에서 제외하고 소유 버퍼를 해제 전에 지움.
pub struct Secret(Vec<u8>);
impl Secret {
    pub fn new(text: String) -> Result<Self, &'static str> {
        let mut bytes = text.into_bytes();
        if bytes.is_empty() || bytes.len() > 1024 {
            bytes.fill(0);
            return Err("암호는 UTF-8 1~1024바이트로 입력하세요");
        }
        Ok(Self(bytes))
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictChoice {
    Overwrite,
    Skip,
    Rename,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Conflict {
    pub challenge_id: u64,
    pub entry_id: u32,
    pub relative_name: String,
    pub incoming_size: u64,
    pub existing_size: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resolution {
    pub entry_id: u32,
    pub relative_name: String,
    pub choice: ConflictChoice,
}
pub enum Command {
    Start(Start),
    StartExpanded(Start),
    ResourceReply {
        challenge_id: u64,
        scope: ResourceScope,
        allow: bool,
    },
    Password {
        challenge_id: u64,
        secret: Secret,
    },
    ConflictReply {
        challenge_id: u64,
        choice: ConflictChoice,
        apply_to_remaining: bool,
    },
    Cancel,
}
pub struct Request {
    pub job_id: u64,
    pub sequence: u64,
    pub command: Command,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    File,
    Directory,
    Link,
    Other,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub id: u32,
    pub name: String,
    pub kind: Kind,
    pub bytes: Option<u64>,
    pub packed: Option<u64>,
    pub encrypted: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancelStatus {
    Requested,
    AlreadyRequested,
    TooLate,
    Finished,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    Scan,
    Read,
    Decode,
    Write,
    Verify,
    Commit,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Completed,
    Failed,
    Cancelled,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    RecoveryIncomplete,
    RecoveryImpossible,
    MissingVolume,
    Integrity,
    UnsupportedFormat,
    UnsupportedFeature,
    WrongPassword,
    UnsafePath,
    Conflict,
    Limit,
    Io,
    DiskFull,
    Protocol,
    Internal,
    SourceChanged,
    InvalidRequest,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Publication {
    None,
    Partial,
    Complete,
    Uncertain,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Durability {
    NotApplicable,
    Synced,
    Unconfirmed,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cleanup {
    Complete,
    Failed,
    Unconfirmed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Disposition {
    Created,
    PartialRetained,
    Removed,
    CommitUncertain,
}
#[derive(Clone, Debug)]
pub struct Terminal {
    pub outcome: Outcome,
    pub error: Option<ErrorCode>,
    pub publication: Publication,
    pub created_count: u64,
    pub retained_partial_count: u64,
    pub durability: Durability,
    pub cleanup: Cleanup,
}
pub use worker_protocol::protocol::{
    ActionKind, Limits, NativePath, PreviewFormat, PreviewInfo, RecoveryEnd, RecoveryEntry,
    RecoveryMode, RecoveryReason, RecoveryStatus, ResourceScope, ResourceUsage,
    VolumeProblemReason,
};

pub struct Preview {
    pub info: PreviewInfo,
    pub data: Vec<u8>,
}

struct PreviewBuffer {
    info: PreviewInfo,
    data: Vec<u8>,
    ended: bool,
}

impl PreviewBuffer {
    fn chunk(&mut self, entry_id: u32, offset: u64, data: Vec<u8>) -> bool {
        if self.ended
            || entry_id != self.info.entry_id
            || offset != self.data.len() as u64
            || data.is_empty()
            || data.len() > 32 * 1024
            || (data.len() as u64) > self.info.total_bytes.saturating_sub(offset)
        {
            return false;
        }
        self.data.extend_from_slice(&data);
        true
    }
    fn end(&mut self, entry_id: u32, total_bytes: u64, sha256: [u8; 32]) -> bool {
        use sha2::{Digest, Sha256};
        if self.ended
            || entry_id != self.info.entry_id
            || total_bytes != self.info.total_bytes
            || total_bytes != self.data.len() as u64
            || Sha256::digest(&self.data)[..] != sha256
            || (self.info.format == PreviewFormat::Utf8Text
                && std::str::from_utf8(&self.data).is_err())
        {
            return false;
        }
        self.ended = true;
        true
    }
}

#[derive(Clone, Debug)]
pub struct ResourceChallenge {
    pub id: u64,
    pub action: ActionKind,
    pub scope: ResourceScope,
    pub usage: ResourceUsage,
    pub soft_limits: Limits,
    pub ceiling: Limits,
}

#[derive(Clone, Debug)]
pub enum Event {
    RecoveryEntry(RecoveryEntry),
    RecoveryEnd(RecoveryEnd),
    VolumeProblem(VolumeProblem),
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
    ResourceRequired(ResourceChallenge),
    Accepted {
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
        encrypt: bool,
    },
    ConflictRequired(Conflict),
    ConflictResolved(Resolution),
    Progress {
        stage: Stage,
        processed: u64,
        total: Option<u64>,
        completed_entries: u64,
    },
    Output {
        entry_id: Option<u32>,
        relative_name: String,
        disposition: Disposition,
    },
    CancelAck {
        request_sequence: u64,
        status: CancelStatus,
    },
    Terminal(Terminal),
}
pub enum HostEvent {
    Fault(String),
    CleanupComplete,
    Frame {
        job_id: u64,
        sequence: u64,
        event: Event,
    },
    StdoutEof,
    StderrEof,
    Exited {
        success: bool,
    },
}
// 바이트 codec과 프로세스 소유는 작업자 어댑터에서 담당함.
// 전송 실패 후에도 poll로 양쪽 EOF와 자식 회수 결과를 전달함.
pub trait Connection {
    fn send(&mut self, request: Request) -> Result<(), String>;
    fn poll(&mut self) -> Option<HostEvent>;
}
pub trait Factory {
    fn connect(&mut self, job_id: u64) -> Result<Box<dyn Connection>, String>;
}
pub struct Unavailable;
impl Factory for Unavailable {
    fn connect(&mut self, _: u64) -> Result<Box<dyn Connection>, String> {
        Err("작업자 연결이 아직 준비되지 않았습니다".into())
    }
}
struct Running {
    recovery_metadata: usize,
    volume_problem: Option<VolumeProblem>,
    preview: Option<PreviewBuffer>,
    preview_discarded: bool,
    id: u64,
    outgoing: u64,
    incoming: u64,
    accepted: bool,
    start_failed: bool,
    start: Start,
    connection: Box<dyn Connection>,
    entries: Vec<Entry>,
    list_end: Option<(u32, ListId)>,
    stdout_eof: bool,
    transport_closed: bool,
    stderr_eof: bool,
    exit: Option<bool>,
    cancel_sequence: Option<u64>,
    expanded: bool,
    granted: ResourceScope,
    last_challenge: u64,
    estimated: ResourceUsage,
    conflict_ids: BTreeSet<u64>,
    resolved_ids: BTreeSet<u32>,
    file_ids: BTreeSet<u32>,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Close {
    #[default]
    Open,
    Confirm,
    Waiting,
    Review,
    Allowed,
}
#[derive(Default)]
pub struct Model {
    pub recovery_entries: Vec<RecoveryEntry>,
    pub recovery_end: Option<RecoveryEnd>,
    pub volume_problem: Option<VolumeProblem>,
    running: Option<Running>,
    pub preview: Option<Preview>,
    next_id: u64,
    pub archive: Option<PathBuf>,
    pub entries: Vec<Entry>,
    pub list_id: Option<ListId>,
    pub selected: BTreeSet<u32>,
    pub terminal: Option<Terminal>,
    pub challenge: Option<(u64, u32, bool)>,
    pub resource: Option<ResourceChallenge>,
    retry_start: Option<Start>,
    pub conflict: Option<Conflict>,
    pub resolutions: Vec<Resolution>,
    pub progress: Option<(Stage, u64, Option<u64>, u64)>,
    pub outputs: Vec<(String, Disposition)>,
    pub cancel: Option<CancelStatus>,
    pub error: Option<String>,
    pub input_error: Option<String>,
    pub encryption: bool,
    pub close: Close,
    pub confirmed: bool,
}
impl Model {
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }
    pub fn cancellation_requested(&self) -> bool {
        self.running
            .as_ref()
            .is_some_and(|run| run.cancel_sequence.is_some())
    }
    pub fn job_id(&self) -> Option<u64> {
        self.running.as_ref().map(|r| r.id)
    }
    pub fn can_extract(&self) -> bool {
        !self.busy()
            && self.list_id.is_some()
            && self.archive.is_some()
            && self.close == Close::Open
    }
    pub fn can_preview(&self) -> bool {
        self.can_extract()
            && self.list_id.as_ref().is_some_and(|id| !id.is_split())
            && self.selected.len() == 1
            && self
                .entries
                .iter()
                .any(|e| self.selected.contains(&e.id) && e.kind == Kind::File)
    }
    pub fn begin_preview(&mut self, factory: &mut dyn Factory) -> bool {
        if !self.can_preview() {
            return false;
        }
        self.begin(
            Start::Preview {
                archive: self.archive.clone().unwrap(),
                entry_id: *self.selected.first().unwrap(),
                list_id: self.list_id.clone().unwrap(),
            },
            factory,
        )
    }
    pub fn dismiss_preview(&mut self) {
        self.preview = None;
        if let Some(run) = self.running.as_mut()
            && matches!(run.start, Start::Preview { .. })
        {
            run.preview = None;
            run.preview_discarded = true;
            self.cancel();
        }
    }
    pub fn begin(&mut self, start: Start, factory: &mut dyn Factory) -> bool {
        let expanded =
            matches!(start, Start::CreateSevenZip { .. }) || self.needs_expansion(&start);
        self.begin_with_limits(start, factory, expanded)
    }
    pub fn estimated_usage(&self) -> ResourceUsage {
        self.running
            .as_ref()
            .map(|run| run.estimated)
            .unwrap_or_default()
    }
    fn estimate(&self, start: &Start) -> ResourceUsage {
        let mut usage = ResourceUsage::default();
        match start {
            Start::Extract { selection, .. } => {
                let selected_ids: BTreeSet<_> = match selection {
                    Selection::All => BTreeSet::new(),
                    Selection::EntryIds(ids) => ids.iter().copied().collect(),
                };
                let directories: BTreeSet<_> = self
                    .entries
                    .iter()
                    .filter(|e| selected_ids.contains(&e.id) && e.kind == Kind::Directory)
                    .map(|e| e.name.trim_end_matches('/'))
                    .collect();
                let selected = |entry: &&Entry| {
                    matches!(selection, Selection::All)
                        || selected_ids.contains(&entry.id)
                        || entry
                            .name
                            .match_indices('/')
                            .any(|(n, _)| directories.contains(&entry.name[..n]))
                };
                let entries: Vec<_> = self.entries.iter().filter(selected).collect();
                usage.estimated_entries = u32::try_from(entries.len()).ok();
                usage.estimated_bytes = entries
                    .iter()
                    .try_fold(0u64, |sum, e| sum.checked_add(e.bytes?));
            }
            Start::CreateZip { sources, .. } | Start::CreateSevenZip { sources, .. } => {
                usage.estimated_bytes = sources.iter().try_fold(0u64, |sum, (path, _)| {
                    let m = std::fs::symlink_metadata(path).ok()?;
                    if !m.is_file() {
                        return None;
                    }
                    sum.checked_add(m.len())
                });
                usage.estimated_entries = sources
                    .iter()
                    .all(|(path, _)| std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file()))
                    .then(|| u32::try_from(sources.len()).ok())
                    .flatten();
            }
            Start::Preview { .. } => {
                usage.estimated_entries = u32::try_from(self.entries.len()).ok();
                usage.estimated_bytes = self
                    .entries
                    .iter()
                    .try_fold(0u64, |sum, e| sum.checked_add(e.bytes?));
            }
            Start::List { .. } | Start::RecoverZip { .. } => {}
        }
        usage
    }
    fn needs_expansion(&self, start: &Start) -> bool {
        let usage = self.estimate(start);
        usage
            .estimated_bytes
            .is_some_and(|n| n > Limits::default().output)
            || usage
                .estimated_entries
                .is_some_and(|n| n > Limits::default().entries)
    }
    pub fn can_retry_expanded(&self) -> bool {
        !self.busy() && self.retry_start.is_some() && self.close == Close::Open
    }
    pub fn retry_expanded(&mut self, factory: &mut dyn Factory) -> bool {
        if !self.can_retry_expanded() {
            return false;
        }
        let start = self.retry_start.clone().unwrap();
        self.begin_with_limits(start, factory, true)
    }
    pub fn resource_reply(&mut self, job_id: u64, challenge_id: u64, allow: bool) -> bool {
        if self.job_id() != Some(job_id) || self.cancellation_requested() {
            return false;
        }
        let Some(question) = self.resource.clone().filter(|q| q.id == challenge_id) else {
            return false;
        };
        if self
            .send(Command::ResourceReply {
                challenge_id,
                scope: question.scope,
                allow,
            })
            .is_none()
        {
            return false;
        }
        if allow {
            let granted = &mut self.running.as_mut().unwrap().granted;
            granted.output |= question.scope.output;
            granted.entries |= question.scope.entries;
            granted.ratio |= question.scope.ratio;
        }
        self.resource = None;
        true
    }
    fn begin_with_limits(
        &mut self,
        start: Start,
        factory: &mut dyn Factory,
        expanded: bool,
    ) -> bool {
        if self.busy() || self.close != Close::Open {
            return false;
        }
        let selectable: BTreeSet<_> = self
            .entries
            .iter()
            .filter(|e| matches!(e.kind, Kind::File | Kind::Directory))
            .map(|e| e.id)
            .collect();
        let validation = match &start {
            Start::RecoverZip {
                archive,
                output_file,
            } => archive.is_absolute() && output_file.is_absolute() && archive != output_file,
            Start::List { archive } => archive.is_absolute(),
            Start::Preview {
                archive,
                entry_id,
                list_id,
            } => {
                archive.is_absolute()
                    && list_id.valid()
                    && !list_id.is_split()
                    && self.archive.as_ref() == Some(archive)
                    && self.list_id.as_ref() == Some(list_id)
                    && self
                        .entries
                        .iter()
                        .any(|e| e.id == *entry_id && e.kind == Kind::File)
            }
            Start::Extract {
                archive,
                destination,
                selection,
                gzip_output_name,
                list_id,
            } => {
                archive.is_absolute()
                    && destination.is_absolute()
                    && self.archive.as_ref() == Some(archive)
                    && self.list_id.as_ref() == Some(list_id)
                    && match list_id.format {
                        Format::Gzip => gzip_output_name.as_ref().is_some_and(|s| valid_leaf(s)),
                        _ => gzip_output_name.is_none(),
                    }
                    && match selection {
                        Selection::All => true,
                        Selection::EntryIds(ids) => {
                            !ids.is_empty()
                                && ids.iter().collect::<BTreeSet<_>>().len() == ids.len()
                                && ids.iter().all(|id| selectable.contains(id))
                        }
                    }
            }
            Start::CreateZip {
                sources,
                output_file,
                ..
            }
            | Start::CreateSevenZip {
                sources,
                output_file,
            } => {
                !sources.is_empty()
                    && output_file.is_absolute()
                    && sources
                        .iter()
                        .all(|(p, n)| p.is_absolute() && valid_relative(n))
            }
        };
        if !validation {
            self.input_error = Some("입력 경로, 출력명과 선택한 목록을 확인하세요".into());
            return false;
        }
        let Some(id) = self.next_id.checked_add(1) else {
            self.error = Some("작업 ID 한도에 도달했습니다".into());
            return false;
        };
        self.preview = None;
        let estimated = self.estimate(&start);
        self.volume_problem = None;
        let mut connection = match factory.connect(id) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(e);
                return false;
            }
        };
        let send_error = connection
            .send(Request {
                job_id: id,
                sequence: 0,
                command: if expanded {
                    Command::StartExpanded(start.clone())
                } else {
                    Command::Start(start.clone())
                },
            })
            .err();
        let started = send_error.is_none();
        if let Start::List { archive } = &start
            && started
        {
            self.archive = Some(archive.clone());
            self.list_id = None;
            self.entries.clear();
            self.selected.clear();
        }
        self.next_id = id;
        self.recovery_entries.clear();
        self.recovery_end = None;
        self.terminal = None;
        self.volume_problem = None;
        self.retry_start = None;
        self.resource = None;
        self.challenge = None;
        self.conflict = None;
        self.resolutions.clear();
        self.progress = None;
        self.outputs.clear();
        self.cancel = None;
        self.input_error = None;
        self.error = send_error;
        self.confirmed = false;
        self.running = Some(Running {
            recovery_metadata: 0,
            volume_problem: None,
            preview: None,
            preview_discarded: false,
            id,
            outgoing: 1,
            incoming: 0,
            accepted: false,
            start_failed: !started,
            start,
            connection,
            entries: Vec::new(),
            list_end: None,
            stdout_eof: false,
            transport_closed: false,
            stderr_eof: false,
            exit: None,
            cancel_sequence: None,
            expanded,
            granted: ResourceScope::default(),
            last_challenge: 0,
            estimated,
            conflict_ids: BTreeSet::new(),
            resolved_ids: BTreeSet::new(),
            file_ids: self
                .entries
                .iter()
                .filter(|e| e.kind == Kind::File)
                .map(|e| e.id)
                .collect(),
        });
        started
    }
    pub fn extract(
        &mut self,
        destination: PathBuf,
        selected_only: bool,
        gzip_output_name: Option<String>,
        factory: &mut dyn Factory,
    ) -> bool {
        if !self.can_extract() {
            return false;
        }
        self.begin(
            Start::Extract {
                archive: self.archive.clone().unwrap(),
                destination,
                selection: if selected_only {
                    Selection::EntryIds(self.selected.iter().copied().collect())
                } else {
                    Selection::All
                },
                list_id: self.list_id.clone().unwrap(),
                gzip_output_name,
            },
            factory,
        )
    }
    pub fn password(&mut self, text: String) -> bool {
        let Some((challenge_id, _, _)) = self.challenge else {
            return false;
        };
        let secret = match Secret::new(text) {
            Ok(s) => s,
            Err(e) => {
                self.input_error = Some(e.into());
                return false;
            }
        };
        if self
            .send(Command::Password {
                challenge_id,
                secret,
            })
            .is_some()
        {
            self.challenge = None;
            self.resource = None;
            self.conflict = None;
            self.input_error = None;
            true
        } else {
            false
        }
    }
    pub fn resolve_conflict(&mut self, choice: ConflictChoice, apply_to_remaining: bool) -> bool {
        let Some(conflict) = &self.conflict else {
            return false;
        };
        if self.cancellation_requested() || self.error.is_some() || self.close != Close::Open {
            return false;
        }
        if self
            .send(Command::ConflictReply {
                challenge_id: conflict.challenge_id,
                choice,
                apply_to_remaining,
            })
            .is_some()
        {
            self.conflict = None;
            self.input_error = None;
            true
        } else {
            false
        }
    }
    fn send(&mut self, command: Command) -> Option<u64> {
        let run = self.running.as_mut()?;
        if run.start_failed || self.terminal.is_some() || run.exit.is_some() {
            return None;
        }
        let seq = run.outgoing;
        let Some(next) = seq.checked_add(1) else {
            self.error = Some("요청 순서 한도 초과".into());
            return None;
        };
        match run.connection.send(Request {
            job_id: run.id,
            sequence: seq,
            command,
        }) {
            Ok(()) => {
                run.outgoing = next;
                Some(seq)
            }
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }
    pub fn cancel(&mut self) {
        self.preview = None;
        if let Some(run) = self.running.as_mut()
            && matches!(run.start, Start::Preview { .. })
        {
            run.preview = None;
            run.preview_discarded = true;
        }
        if self
            .running
            .as_ref()
            .is_some_and(|r| r.cancel_sequence.is_none())
        {
            if let Some(seq) = self.send(Command::Cancel) {
                self.running.as_mut().unwrap().cancel_sequence = Some(seq);
                self.challenge = None;
                self.resource = None;
                self.conflict = None;
                self.input_error = None;
            }
        }
    }
    pub fn request_close(&mut self) {
        if self.close == Close::Open {
            self.close = if self.busy() {
                Close::Confirm
            } else {
                Close::Review
            };
        }
    }
    pub fn confirm_close(&mut self) {
        if self.close == Close::Confirm {
            self.cancel();
            self.close = if self.busy() {
                Close::Waiting
            } else {
                Close::Review
            };
        }
    }
    pub fn keep_open(&mut self) {
        if self.close != Close::Allowed {
            self.close = Close::Open;
        }
    }
    pub fn acknowledge_close(&mut self) {
        if !self.busy() && self.close == Close::Review {
            self.close = Close::Allowed;
        }
    }
    pub fn poll(&mut self) {
        let changed = self.running.as_ref().is_some_and(|run| {
            matches!(&run.start, Start::Preview { entry_id, .. }
                if self.selected.len() != 1 || !self.selected.contains(entry_id))
        });
        if changed {
            self.dismiss_preview();
        }
        for _ in 0..128 {
            let Some(event) = self.running.as_mut().and_then(|r| r.connection.poll()) else {
                break;
            };
            self.receive(event);
            if self.error.is_some()
                && self
                    .running
                    .as_ref()
                    .is_some_and(|run| matches!(run.start, Start::Preview { .. }))
            {
                self.cancel();
            }
        }
        let complete = self.running.as_ref().is_some_and(|r| {
            r.exit.is_some()
                && ((r.stdout_eof && r.stderr_eof) || (r.transport_closed && self.error.is_some()))
        });
        if complete {
            let mut run = self.running.take().unwrap();
            self.confirmed =
                self.terminal.is_some() && run.exit == Some(true) && self.error.is_none();
            if !self.confirmed {
                self.error.get_or_insert(
                    "작업자 종료 결과를 확인할 수 없습니다. 출력 상태를 확인하세요".into(),
                );
            }
            if self.confirmed
                && self
                    .terminal
                    .as_ref()
                    .is_some_and(|t| t.outcome == Outcome::Completed)
            {
                if matches!(run.start, Start::List { .. }) {
                    if let Some((count, id)) = run
                        .list_end
                        .filter(|(n, _)| *n as usize == run.entries.len())
                    {
                        let _ = count;
                        self.entries = run.entries;
                        self.list_id = Some(id);
                    } else {
                        self.error = Some("목록 완료 정보가 일치하지 않습니다. 다시 여세요".into());
                        self.confirmed = false;
                    }
                }
            }
            if matches!(run.start, Start::Preview { .. }) {
                if self.confirmed
                    && self
                        .terminal
                        .as_ref()
                        .is_some_and(|t| t.outcome == Outcome::Completed)
                    && !run.preview_discarded
                {
                    if let Some(buffer) = run.preview.take().filter(|b| b.ended) {
                        self.preview = Some(Preview {
                            info: buffer.info,
                            data: buffer.data,
                        });
                    } else {
                        self.error = Some("미리보기 완료 정보가 일치하지 않습니다".into());
                        self.confirmed = false;
                    }
                }
            }
            if self.terminal.as_ref().is_some_and(|t| {
                matches!(
                    t.error,
                    Some(ErrorCode::SourceChanged | ErrorCode::MissingVolume)
                )
            }) {
                self.list_id = None;
                self.selected.clear();
            }
            if self.confirmed && run.cancel_sequence.is_none() {
                self.volume_problem = run.volume_problem.take();
            }
            self.retry_start = if !matches!(run.start, Start::Preview { .. })
                && !run.expanded
                && run.cancel_sequence.is_none()
                && self.confirmed
                && self.terminal.as_ref().is_some_and(|t| {
                    t.outcome == Outcome::Failed
                        && t.error == Some(ErrorCode::Limit)
                        && t.publication == Publication::None
                        && t.created_count == 0
                        && t.retained_partial_count == 0
                        && t.cleanup == Cleanup::Complete
                })
                && self.outputs.iter().all(|(_, d)| *d == Disposition::Removed)
            {
                Some(run.start.clone())
            } else {
                None
            };
            self.challenge = None;
            self.resource = None;
            self.conflict = None;
            self.input_error = None;
            if matches!(self.close, Close::Waiting | Close::Confirm) {
                self.close = Close::Review;
            }
        }
    }
    fn receive(&mut self, host: HostEvent) {
        let run = self.running.as_mut().unwrap();
        match host {
            HostEvent::Fault(message) => self.error = Some(message),
            HostEvent::CleanupComplete => run.transport_closed = true,
            HostEvent::StdoutEof => run.stdout_eof = true,
            HostEvent::StderrEof => run.stderr_eof = true,
            HostEvent::Exited { success } => run.exit = Some(success),
            HostEvent::Frame {
                job_id,
                sequence,
                event,
            } => {
                if job_id != run.id
                    || sequence != run.incoming
                    || self.terminal.is_some()
                    || run.stdout_eof
                    || sequence >= 1_000_000
                {
                    self.error = Some("작업자 응답 순서나 작업 ID가 일치하지 않습니다".into());
                    return;
                }
                run.incoming += 1;
                if !run.accepted && !matches!(event, Event::Accepted { .. }) {
                    self.error = Some("작업 승인 응답이 없습니다".into());
                    return;
                }
                match event {
                    Event::RecoveryEntry(entry) => {
                        let budget = if run.granted.any() {
                            64 * 1024 * 1024
                        } else {
                            8 * 1024 * 1024
                        };
                        let used = run.recovery_metadata;
                        if !matches!(run.start, Start::RecoverZip { .. })
                            || self.recovery_end.is_some()
                            || !entry.valid()
                            || entry.candidate_id as usize != self.recovery_entries.len()
                            || entry.name.as_ref().is_some_and(|n| {
                                n.len() > 4096 || !valid_relative(n.trim_end_matches('/'))
                            })
                            || self
                                .recovery_entries
                                .last()
                                .is_some_and(|e| e.source_offset >= entry.source_offset)
                            || self.recovery_entries.len()
                                >= if run.granted.entries { 200_000 } else { 10_000 }
                            || used + 128 + entry.name.as_ref().map_or(0, String::len) > budget
                        {
                            self.error = Some("복구 항목 응답이 유효하지 않습니다".into());
                        } else {
                            run.recovery_metadata +=
                                128 + entry.name.as_ref().map_or(0, String::len);
                            self.recovery_entries.push(entry);
                        }
                    }
                    Event::RecoveryEnd(summary) => {
                        let count = |status| {
                            self.recovery_entries
                                .iter()
                                .filter(|e| e.status == status)
                                .count() as u32
                        };
                        if !matches!(run.start, Start::RecoverZip { .. })
                            || self.recovery_end.is_some()
                            || !summary.valid()
                            || summary.source_bytes > 4 * 1024 * 1024 * 1024
                            || self
                                .recovery_entries
                                .iter()
                                .any(|e| e.source_offset >= summary.source_bytes)
                            || summary.verified_entries != count(RecoveryStatus::Verified)
                            || summary.lost_entries != count(RecoveryStatus::Lost)
                            || summary.unverified_entries != count(RecoveryStatus::Unverified)
                        {
                            self.error = Some("복구 요약이 항목과 일치하지 않습니다".into());
                        } else {
                            self.recovery_end = Some(summary);
                        }
                    }
                    Event::VolumeProblem(problem) => {
                        if run.cancel_sequence.is_some() {
                            return;
                        }
                        if run.volume_problem.is_some()
                            || !matches!(run.start, Start::List { .. } | Start::Extract { .. })
                            || !problem.volume.is_basename()
                        {
                            self.error = Some("분할 조각 오류 응답이 유효하지 않습니다".into());
                        } else {
                            run.volume_problem = Some(problem);
                        }
                    }
                    Event::PreviewBegin(info) => {
                        if run.preview_discarded {
                            return;
                        }
                        if !matches!(run.start, Start::Preview { entry_id, .. } if entry_id == info.entry_id)
                            || run.preview.is_some()
                            || !info.valid()
                            || self.error.is_some()
                        {
                            self.error = Some("미리보기 메타정보가 유효하지 않습니다".into());
                        } else {
                            let mut data = Vec::new();
                            if data.try_reserve_exact(info.total_bytes as usize).is_err() {
                                self.error = Some("미리보기 메모리를 확보하지 못했습니다".into());
                            } else {
                                run.preview = Some(PreviewBuffer {
                                    info,
                                    data,
                                    ended: false,
                                });
                            }
                        }
                    }
                    Event::PreviewChunk {
                        entry_id,
                        offset,
                        data,
                    } => {
                        if run.preview_discarded {
                            return;
                        }
                        if self.error.is_some()
                            || !run
                                .preview
                                .as_mut()
                                .is_some_and(|b| b.chunk(entry_id, offset, data))
                        {
                            run.preview = None;
                            self.error =
                                Some("미리보기 데이터 순서나 크기가 일치하지 않습니다".into());
                        }
                    }
                    Event::PreviewEnd {
                        entry_id,
                        total_bytes,
                        sha256,
                    } => {
                        if run.preview_discarded {
                            return;
                        }
                        if !run
                            .preview
                            .as_mut()
                            .is_some_and(|b| b.end(entry_id, total_bytes, sha256))
                        {
                            run.preview = None;
                            self.error = Some("미리보기 내용 검증에 실패했습니다".into());
                        }
                    }
                    Event::Accepted { encryption } => {
                        if matches!(
                            run.start,
                            Start::CreateSevenZip { .. } | Start::RecoverZip { .. }
                        ) && encryption
                        {
                            self.error = Some("7z 생성의 암호 응답이 유효하지 않습니다".into());
                        }
                        if run.accepted {
                            self.error = Some("작업 승인 중복".into());
                        }
                        run.accepted = true;
                        self.encryption = encryption;
                    }
                    Event::Entry(entry) => {
                        if !matches!(run.start, Start::List { .. })
                            || run.list_end.is_some()
                            || entry.id as usize != run.entries.len()
                            || entry.name.len() > 4096
                            || run.entries.len()
                                >= if run.granted.entries {
                                    200_000
                                } else {
                                    100_000
                                }
                        {
                            self.error = Some("목록 응답이 유효하지 않습니다".into());
                        } else {
                            run.entries.push(entry);
                        }
                    }
                    Event::ListEnd {
                        entry_count,
                        list_id,
                    } => {
                        if !matches!(run.start, Start::List { .. })
                            || run.list_end.is_some()
                            || !list_id.valid()
                        {
                            self.error = Some("목록 완료 응답이 유효하지 않습니다".into());
                        } else {
                            run.list_end = Some((entry_count, list_id));
                        }
                    }
                    Event::PasswordRequired {
                        challenge_id,
                        attempt,
                        encrypt,
                    } => {
                        if run.cancel_sequence.is_some() {
                            return;
                        }
                        if matches!(
                            run.start,
                            Start::CreateSevenZip { .. } | Start::RecoverZip { .. }
                        ) || self.challenge.is_some()
                            || self.resource.is_some()
                            || challenge_id <= run.last_challenge
                            || self.conflict.is_some()
                        {
                            self.error = Some("암호 요청이 유효하지 않습니다".into());
                        } else {
                            run.last_challenge = challenge_id;
                            self.challenge = Some((challenge_id, attempt, encrypt));
                        }
                    }
                    Event::ResourceRequired(question) => {
                        if run.cancel_sequence.is_some() {
                            return;
                        }
                        let expected = match run.start {
                            Start::List { .. } => ActionKind::List,
                            Start::Extract { .. } => ActionKind::Extract,
                            Start::CreateZip { .. } => ActionKind::CreateZip,
                            Start::CreateSevenZip { .. } => ActionKind::CreateSevenZip,
                            Start::Preview { .. } => ActionKind::Preview,
                            Start::RecoverZip { .. } => ActionKind::RecoverZip,
                        };
                        let hard = Limits {
                            input: u64::MAX,
                            output: 64 * 1024 * 1024 * 1024,
                            entries: 200_000,
                        };
                        let ceiling = if run.expanded {
                            hard
                        } else {
                            Limits::default()
                        };
                        if self.challenge.is_some()
                            || self.resource.is_some()
                            || self.conflict.is_some()
                            || question.id <= run.last_challenge
                            || question.action != expected
                            || !question.scope.any()
                            || question.soft_limits != Limits::default()
                            || question.ceiling != ceiling
                            || (question.scope.output && (!run.expanded || run.granted.output))
                            || (question.scope.entries && (!run.expanded || run.granted.entries))
                            || (question.scope.ratio && run.granted.ratio)
                        {
                            self.error = Some("자원 승인 요청이 유효하지 않습니다".into());
                        } else {
                            run.last_challenge = question.id;
                            self.resource = Some(question);
                        }
                    }
                    Event::ConflictRequired(conflict) => {
                        let valid_entry = self
                            .entries
                            .iter()
                            .any(|entry| entry.id == conflict.entry_id && entry.kind == Kind::File);
                        if !matches!(run.start, Start::Extract { .. })
                            || !valid_entry
                            || self.resource.is_some()
                            || conflict.challenge_id <= run.last_challenge
                            || self.challenge.is_some()
                            || self.conflict.is_some()
                            || run.conflict_ids.len()
                                >= if run.granted.entries {
                                    200_000
                                } else {
                                    100_000
                                }
                            || !valid_relative(&conflict.relative_name)
                            || conflict.relative_name.len() > 4096
                            || !run.conflict_ids.insert(conflict.challenge_id)
                        {
                            self.error = Some("파일 충돌 요청이 유효하지 않습니다".into());
                        } else if run.cancel_sequence.is_none() {
                            run.last_challenge = conflict.challenge_id;
                            self.conflict = Some(conflict);
                        }
                    }
                    Event::ConflictResolved(resolution) => {
                        if !matches!(run.start, Start::Extract { .. })
                            || !run.file_ids.contains(&resolution.entry_id)
                            || self.resolutions.len()
                                >= if run.granted.entries {
                                    200_000
                                } else {
                                    100_000
                                }
                            || !valid_relative(&resolution.relative_name)
                            || resolution.relative_name.len() > 4096
                            || !run.resolved_ids.insert(resolution.entry_id)
                        {
                            self.error = Some("파일 충돌 결과가 유효하지 않습니다".into());
                        } else {
                            self.resolutions.push(resolution);
                        }
                    }
                    Event::Progress {
                        stage,
                        processed,
                        total,
                        completed_entries,
                    } => self.progress = Some((stage, processed, total, completed_entries)),
                    Event::Output {
                        relative_name,
                        disposition,
                        entry_id,
                    } => {
                        if let Start::RecoverZip { output_file, .. } = &run.start {
                            if entry_id.is_some()
                                || self.recovery_end.is_none()
                                || !self.outputs.is_empty()
                                || output_file.file_name().and_then(|n| n.to_str())
                                    != Some(relative_name.as_str())
                            {
                                self.error = Some("복구본 출력 응답이 유효하지 않습니다".into());
                                return;
                            }
                        }
                        if matches!(run.start, Start::Preview { .. }) {
                            self.error = Some("미리보기 중 파일 출력이 보고됐습니다".into());
                            return;
                        }
                        if self.outputs.len()
                            < if run.granted.any() {
                                // 암묵적 상위 폴더도 출력되므로 작업자 노드 예산과 맞춤
                                262_144
                            } else {
                                100_000
                            }
                            && relative_name.len() <= 4096
                        {
                            self.outputs.push((relative_name, disposition));
                        } else {
                            self.error = Some("출력 보고 한도 초과".into());
                        }
                    }
                    Event::CancelAck {
                        request_sequence,
                        status,
                    } => {
                        if run.cancel_sequence == Some(request_sequence) && self.cancel.is_none() {
                            self.cancel = Some(status);
                        } else {
                            self.error = Some("취소 응답이 요청과 일치하지 않습니다".into());
                        }
                    }
                    Event::Terminal(t) => {
                        if matches!(run.start, Start::RecoverZip { .. }) {
                            let business = t.outcome == Outcome::Completed
                                || matches!(
                                    t.error,
                                    Some(
                                        ErrorCode::RecoveryIncomplete
                                            | ErrorCode::RecoveryImpossible
                                    )
                                );
                            if business
                                && !self.recovery_end.as_ref().is_some_and(|s| {
                                    if s.verified_entries == 0 {
                                        t.outcome == Outcome::Failed
                                            && t.error == Some(ErrorCode::RecoveryImpossible)
                                            && t.publication == Publication::None
                                            && t.created_count == 0
                                            && self.outputs.is_empty()
                                    } else {
                                        t.publication == Publication::Complete
                                            && t.created_count == 1
                                            && self.outputs.len() == 1
                                            && self.outputs[0].1 == Disposition::Created
                                            && if s.complete() {
                                                t.outcome == Outcome::Completed && t.error.is_none()
                                            } else {
                                                t.outcome == Outcome::Failed
                                                    && t.error
                                                        == Some(ErrorCode::RecoveryIncomplete)
                                            }
                                    }
                                })
                            {
                                self.error =
                                    Some("복구 결과와 저장 상태가 일치하지 않습니다".into());
                            }
                        }
                        if run.cancel_sequence.is_none()
                            && run.volume_problem.as_ref().is_some_and(|problem| {
                                t.outcome != Outcome::Failed
                                    || t.error
                                        != match problem.reason {
                                            VolumeProblemReason::FirstVolumeRequired => {
                                                Some(ErrorCode::InvalidRequest)
                                            }
                                            VolumeProblemReason::MissingVolume => {
                                                Some(ErrorCode::MissingVolume)
                                            }
                                            VolumeProblemReason::IncompleteSet => {
                                                Some(ErrorCode::Integrity)
                                            }
                                        }
                            })
                        {
                            self.error =
                                Some("분할 조각 오류와 종료 상태가 일치하지 않습니다".into());
                        }
                        if matches!(run.start, Start::Preview { .. })
                            && (t.publication != Publication::None
                                || t.created_count != 0
                                || t.retained_partial_count != 0
                                || t.durability != Durability::NotApplicable)
                        {
                            self.error = Some("미리보기 종료 상태가 유효하지 않습니다".into());
                        }
                        if t.outcome != Outcome::Completed {
                            run.preview = None;
                        }
                        self.challenge = None;
                        self.conflict = None;
                        self.terminal = Some(t);
                    }
                }
            }
        }
    }
    pub fn status(&self) -> String {
        if let Some(error) = self.error.as_ref().or(self.input_error.as_ref()) {
            return error.clone();
        }
        if self.busy() {
            if self.terminal.is_some() {
                return "작업 결과 수신 / 프로세스 종료 확인 중".into();
            }
            if self.resource.is_some() {
                return "사용자 허용 대기".into();
            }
            if self.conflict.is_some() {
                return "같은 이름의 파일을 처리할 방법을 선택하세요".into();
            }
            if self.challenge.is_some() {
                return "암호를 입력하세요".into();
            }
            if let Some(c) = self.cancel {
                return match c {
                    CancelStatus::Requested | CancelStatus::AlreadyRequested => {
                        "취소 요청 접수 / 최종 결과 대기"
                    }
                    CancelStatus::TooLate => "취소할 수 없는 단계 / 최종 결과 대기",
                    CancelStatus::Finished => "처리 종료 응답 / 프로세스 종료 확인 중",
                }
                .into();
            }
            if self
                .running
                .as_ref()
                .is_some_and(|r| r.cancel_sequence.is_some())
            {
                return "취소 요청 전송 / 최종 결과 대기".into();
            }
            return "작업 중".into();
        }
        match self
            .terminal
            .as_ref()
            .filter(|_| self.confirmed)
            .map(|t| t.outcome)
        {
            Some(Outcome::Completed) => "작업 완료",
            Some(Outcome::Cancelled) => "작업 취소됨",
            Some(Outcome::Failed) => "작업 실패",
            None => "압축파일을 열거나 ZIP 파일을 만드세요",
        }
        .into()
    }
}
pub fn valid_leaf(name: &str) -> bool {
    valid_relative(name) && !name.contains('/') && !name.contains('\\')
}
pub fn valid_relative(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\0')
        && !name.contains(':')
        && !name.starts_with('/')
        && !name.starts_with('\\')
        && name
            .split(['/', '\\'])
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

// 파일 연결로 전달된 원형 경로를 손실 변환 없이 보존함.
pub fn launch_args(
    args: Vec<std::ffi::OsString>,
    cwd: PathBuf,
) -> Result<(bool, Option<PathBuf>), &'static str> {
    let fixture = args.iter().any(|arg| arg == "--ui-fixture");
    let paths = args
        .into_iter()
        .filter(|arg| arg != "--ui-fixture")
        .collect::<Vec<_>>();
    if paths.len() > 1 {
        return Err("압축파일 하나를 지정하세요");
    }
    let path = paths
        .into_iter()
        .next()
        .map(PathBuf::from)
        .map(|p| if p.is_absolute() { p } else { cwd.join(p) });
    Ok((fixture, path))
}

pub fn stage_label(stage: Stage) -> &'static str {
    match stage {
        Stage::Scan => "목록 읽는 중",
        Stage::Read => "파일 읽는 중",
        Stage::Decode => "압축 푸는 중",
        Stage::Write => "파일 쓰는 중",
        Stage::Verify => "검증 중",
        Stage::Commit => "저장 마무리 중",
    }
}
pub fn disposition_label(disposition: Disposition) -> &'static str {
    match disposition {
        Disposition::Created => "생성됨",
        Disposition::PartialRetained => "일부 파일 남음",
        Disposition::Removed => "작업 임시 파일 정리됨",
        Disposition::CommitUncertain => "저장 여부 불확실",
    }
}
pub fn terminal_lines(t: &Terminal) -> Vec<String> {
    let publication = match t.publication {
        Publication::None => "게시된 파일 없음",
        Publication::Partial => "일부 파일 게시됨",
        Publication::Complete => "파일 게시 완료",
        Publication::Uncertain => "파일 게시 여부 불확실 / 출력 폴더를 확인하세요",
    };
    let durability = match t.durability {
        Durability::NotApplicable => "동기화 해당 없음",
        Durability::Synced => "디스크 동기화 완료",
        Durability::Unconfirmed => "디스크 동기화 미확인",
        Durability::Failed => "디스크 동기화 실패",
    };
    let cleanup = match t.cleanup {
        Cleanup::Complete => "작업 임시 파일 정리 완료",
        Cleanup::Failed => "작업 임시 파일 정리 실패",
        Cleanup::Unconfirmed => "작업 임시 파일 정리 미확인",
    };
    let mut lines = vec![
        publication.into(),
        format!(
            "생성 {}개 / 남은 임시 파일 {}개",
            t.created_count, t.retained_partial_count
        ),
        format!("{durability} / {cleanup}"),
    ];
    if let Some(error) = t.error {
        lines.push(
            match error {
                ErrorCode::RecoveryIncomplete => "일부 항목만 복구했습니다",
                ErrorCode::RecoveryImpossible => "복구 가능한 항목이 없습니다",
                ErrorCode::MissingVolume => "분할 압축파일의 조각이 없습니다. 모든 조각을 같은 폴더에 모은 뒤 첫 조각을 다시 여세요",
                ErrorCode::Integrity => "압축파일이 손상됐습니다",
                ErrorCode::UnsupportedFormat => "지원하지 않는 압축 형식입니다",
                ErrorCode::UnsupportedFeature => "아직 지원하지 않는 기능입니다",
                ErrorCode::WrongPassword => "암호가 올바르지 않습니다",
                ErrorCode::UnsafePath => "안전하지 않은 경로가 있어 중단했습니다",
                ErrorCode::Conflict => "같은 이름의 파일이 있습니다. 다른 출력 폴더를 선택하세요",
                ErrorCode::Limit => "처리 한도를 초과했습니다",
                ErrorCode::Io => {
                    "파일을 읽거나 쓰지 못했습니다. 파일과 폴더의 접근 권한을 확인하세요"
                }
                ErrorCode::DiskFull => {
                    "디스크 여유 공간이 부족합니다. 공간을 확보한 뒤 다시 시도하세요"
                }
                ErrorCode::Protocol => "작업자 응답이 올바르지 않습니다",
                ErrorCode::Internal => "작업자 처리 오류입니다",
                ErrorCode::SourceChanged => "원본 파일이 바뀌었습니다. 압축파일을 다시 여세요",
                ErrorCode::InvalidRequest => "입력과 선택한 항목을 확인하세요",
            }
            .into(),
        );
    }
    lines
}
