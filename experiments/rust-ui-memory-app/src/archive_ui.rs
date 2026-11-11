use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    Zip,
    Tar,
    Gzip,
    TarGzip,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceObject {
    Unix { device: u64, inode: u64 },
    Windows { volume: u64, file_id: [u8; 16] },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListId {
    pub catalog_version: u16,
    pub format: Format,
    pub source_object: SourceObject,
    pub archive_bytes: u64,
    pub archive_sha256: [u8; 32],
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
pub enum Command {
    Start(Start),
    Password { challenge_id: u64, secret: Secret },
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
#[derive(Clone, Debug)]
pub enum Event {
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
    running: Option<Running>,
    next_id: u64,
    pub archive: Option<PathBuf>,
    pub entries: Vec<Entry>,
    pub list_id: Option<ListId>,
    pub selected: BTreeSet<u32>,
    pub terminal: Option<Terminal>,
    pub challenge: Option<(u64, u32, bool)>,
    pub progress: Option<(Stage, u64, Option<u64>, u64)>,
    pub outputs: Vec<(String, Disposition)>,
    pub cancel: Option<CancelStatus>,
    pub error: Option<String>,
    pub encryption: bool,
    pub close: Close,
    pub confirmed: bool,
}
impl Model {
    pub fn busy(&self) -> bool {
        self.running.is_some()
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
    pub fn begin(&mut self, start: Start, factory: &mut dyn Factory) -> bool {
        if self.busy() || self.close != Close::Open {
            return false;
        }
        let validation = match &start {
            Start::List { archive } => archive.is_absolute(),
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
                                && ids.iter().all(|id| {
                                    self.entries.iter().any(|e| {
                                        e.id == *id
                                            && matches!(e.kind, Kind::File | Kind::Directory)
                                    })
                                })
                        }
                    }
            }
            Start::CreateZip {
                sources,
                output_file,
                ..
            } => {
                !sources.is_empty()
                    && output_file.is_absolute()
                    && sources
                        .iter()
                        .all(|(p, n)| p.is_absolute() && valid_relative(n))
            }
        };
        if !validation {
            self.error = Some("입력 경로, 출력명과 선택한 목록을 확인하세요".into());
            return false;
        }
        let Some(id) = self.next_id.checked_add(1) else {
            self.error = Some("작업 ID 한도에 도달했습니다".into());
            return false;
        };
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
                command: Command::Start(start.clone()),
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
        self.terminal = None;
        self.challenge = None;
        self.progress = None;
        self.outputs.clear();
        self.cancel = None;
        self.error = send_error;
        self.confirmed = false;
        self.running = Some(Running {
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
                self.error = Some(e.into());
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
            self.error = None;
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
        if self
            .running
            .as_ref()
            .is_some_and(|r| r.cancel_sequence.is_none())
        {
            if let Some(seq) = self.send(Command::Cancel) {
                self.running.as_mut().unwrap().cancel_sequence = Some(seq);
                self.challenge = None;
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
        for _ in 0..128 {
            let Some(event) = self.running.as_mut().and_then(|r| r.connection.poll()) else {
                break;
            };
            self.receive(event);
        }
        let complete = self.running.as_ref().is_some_and(|r| {
            r.exit.is_some()
                && ((r.stdout_eof && r.stderr_eof) || (r.transport_closed && self.error.is_some()))
        });
        if complete {
            let run = self.running.take().unwrap();
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
            if self
                .terminal
                .as_ref()
                .is_some_and(|t| t.error == Some(ErrorCode::SourceChanged))
            {
                self.list_id = None;
                self.selected.clear();
            }
            self.challenge = None;
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
                    Event::Accepted { encryption } => {
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
                            || run.entries.len() >= 100_000
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
                            || list_id.catalog_version != 1
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
                        if self.challenge.is_some() || challenge_id == 0 {
                            self.error = Some("암호 요청이 유효하지 않습니다".into());
                        } else {
                            self.challenge = Some((challenge_id, attempt, encrypt));
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
                        ..
                    } => {
                        if self.outputs.len() < 100_000 && relative_name.len() <= 4096 {
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
                        self.challenge = None;
                        self.terminal = Some(t);
                    }
                }
            }
        }
    }
    pub fn status(&self) -> String {
        if let Some(error) = &self.error {
            return error.clone();
        }
        if self.busy() {
            if self.terminal.is_some() {
                return "작업 결과 수신 / 프로세스 종료 확인 중".into();
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
                ErrorCode::Integrity => "압축파일이 손상됐습니다",
                ErrorCode::UnsupportedFormat => "지원하지 않는 압축 형식입니다",
                ErrorCode::UnsupportedFeature => "아직 지원하지 않는 기능입니다",
                ErrorCode::WrongPassword => "암호가 올바르지 않습니다",
                ErrorCode::UnsafePath => "안전하지 않은 경로가 있어 중단했습니다",
                ErrorCode::Conflict => "같은 이름의 파일이 있습니다. 다른 출력 폴더를 선택하세요",
                ErrorCode::Limit => "처리 한도를 초과했습니다",
                ErrorCode::Io => "파일을 읽거나 쓰지 못했습니다",
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
