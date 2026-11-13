use crate::archive_ui as ui;
use std::{
    cell::RefCell,
    io::{self, Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use worker_protocol::{protocol as wire, wire as codec};

macro_rules! convert_enum {
    ($name:ident, $from:ty, $to:ty, $($variant:ident),+) => {
        fn $name(value: $from) -> $to {
            match value { $(<$from>::$variant => <$to>::$variant),+ }
        }
    };
}
convert_enum!(
    format_to_wire,
    ui::Format,
    wire::Format,
    Zip,
    Tar,
    Gzip,
    TarGzip,
    SevenZip
);
convert_enum!(
    format_to_ui,
    wire::Format,
    ui::Format,
    Zip,
    Tar,
    Gzip,
    TarGzip,
    SevenZip
);
convert_enum!(
    kind_to_ui,
    wire::EntryKind,
    ui::Kind,
    File,
    Directory,
    Link,
    Other
);
convert_enum!(
    stage_to_ui,
    wire::Stage,
    ui::Stage,
    Scan,
    Read,
    Decode,
    Write,
    Verify,
    Commit
);
convert_enum!(
    disposition_to_ui,
    wire::Disposition,
    ui::Disposition,
    Created,
    PartialRetained,
    Removed,
    CommitUncertain
);
convert_enum!(
    outcome_to_ui,
    wire::Outcome,
    ui::Outcome,
    Completed,
    Failed,
    Cancelled
);
convert_enum!(
    publication_to_ui,
    wire::Publication,
    ui::Publication,
    None,
    Partial,
    Complete,
    Uncertain
);
convert_enum!(
    durability_to_ui,
    wire::Durability,
    ui::Durability,
    NotApplicable,
    Synced,
    Unconfirmed,
    Failed
);
convert_enum!(
    cleanup_to_ui,
    wire::Cleanup,
    ui::Cleanup,
    Complete,
    Failed,
    Unconfirmed
);
convert_enum!(
    cancel_to_ui,
    wire::CancelStatus,
    ui::CancelStatus,
    Requested,
    AlreadyRequested,
    TooLate,
    Finished
);
convert_enum!(
    conflict_to_wire,
    ui::ConflictChoice,
    wire::ConflictChoice,
    Overwrite,
    Skip,
    Rename
);
convert_enum!(
    conflict_to_ui,
    wire::ConflictChoice,
    ui::ConflictChoice,
    Overwrite,
    Skip,
    Rename
);
fn list_to_wire(id: ui::ListId) -> wire::ListId {
    wire::ListId {
        catalog_version: id.catalog_version,
        format: format_to_wire(id.format),
        source_object: match id.source_object {
            ui::SourceObject::Unix { device, inode } => wire::SourceObject::Unix { device, inode },
            ui::SourceObject::Windows { volume, file_id } => wire::SourceObject::Windows {
                volume_serial: volume,
                file_id,
            },
        },
        archive_bytes: id.archive_bytes,
        archive_sha256: id.archive_sha256,
    }
}
fn list_to_ui(id: wire::ListId) -> ui::ListId {
    ui::ListId {
        catalog_version: id.catalog_version,
        format: format_to_ui(id.format),
        source_object: match id.source_object {
            wire::SourceObject::Unix { device, inode } => ui::SourceObject::Unix { device, inode },
            wire::SourceObject::Windows {
                volume_serial,
                file_id,
            } => ui::SourceObject::Windows {
                volume: volume_serial,
                file_id,
            },
        },
        archive_bytes: id.archive_bytes,
        archive_sha256: id.archive_sha256,
    }
}
fn start_to_wire(start: ui::Start) -> wire::Start {
    let action = match start {
        ui::Start::List { archive } => wire::Action::List {
            archive: wire::NativePath::from_path(&crate::archive_path::worker_path(&archive)),
        },
        ui::Start::Extract {
            archive,
            destination,
            selection,
            list_id,
            gzip_output_name,
        } => wire::Action::Extract {
            archive: wire::NativePath::from_path(&crate::archive_path::worker_path(&archive)),
            destination: wire::NativePath::from_path(&crate::archive_path::worker_path(
                &destination,
            )),
            selection: match selection {
                ui::Selection::All => wire::Selection::All,
                ui::Selection::EntryIds(ids) => wire::Selection::EntryIds(ids),
            },
            list_id: Some(list_to_wire(list_id)),
            gzip_output_name,
        },
        ui::Start::CreateZip {
            sources,
            output_file,
            method,
            aes256,
        } => wire::Action::CreateZip {
            sources: sources
                .into_iter()
                .map(|(path, archive_name)| wire::Source {
                    path: wire::NativePath::from_path(&crate::archive_path::worker_path(&path)),
                    archive_name,
                })
                .collect(),
            output_file: wire::NativePath::from_path(&crate::archive_path::worker_path(
                &output_file,
            )),
            method: match method {
                ui::Method::Store => wire::Method::Store,
                ui::Method::Deflate => wire::Method::Deflate,
            },
            encryption: if aes256 {
                wire::Encryption::Aes256
            } else {
                wire::Encryption::None
            },
        },
    };
    wire::Start {
        action,
        limits: wire::Limits::default(),
    }
}
fn error_to_ui(error: wire::ErrorCode) -> Option<ui::ErrorCode> {
    Some(match error {
        wire::ErrorCode::None => return None,
        wire::ErrorCode::Integrity => ui::ErrorCode::Integrity,
        wire::ErrorCode::UnsupportedFormat => ui::ErrorCode::UnsupportedFormat,
        wire::ErrorCode::UnsupportedFeature => ui::ErrorCode::UnsupportedFeature,
        wire::ErrorCode::WrongPassword => ui::ErrorCode::WrongPassword,
        wire::ErrorCode::UnsafePath => ui::ErrorCode::UnsafePath,
        wire::ErrorCode::Conflict => ui::ErrorCode::Conflict,
        wire::ErrorCode::Limit => ui::ErrorCode::Limit,
        wire::ErrorCode::Io => ui::ErrorCode::Io,
        wire::ErrorCode::DiskFull => ui::ErrorCode::DiskFull,
        wire::ErrorCode::Protocol => ui::ErrorCode::Protocol,
        wire::ErrorCode::Internal => ui::ErrorCode::Internal,
        wire::ErrorCode::SourceChanged => ui::ErrorCode::SourceChanged,
        wire::ErrorCode::InvalidRequest => ui::ErrorCode::InvalidRequest,
    })
}
fn event_to_ui(
    message: wire::Message,
    expected: Option<wire::ActionKind>,
) -> Result<ui::Event, ()> {
    Ok(match message {
        wire::Message::Accepted { action, encryption } if expected == Some(action) => {
            ui::Event::Accepted { encryption }
        }
        wire::Message::ResourceRequired {
            challenge_id,
            action,
            scope,
            usage,
            soft_limits,
            ceiling,
        } if expected == Some(action) => ui::Event::ResourceRequired(ui::ResourceChallenge {
            id: challenge_id,
            action,
            scope,
            usage,
            soft_limits,
            ceiling,
        }),
        wire::Message::Entry(entry) => ui::Event::Entry(ui::Entry {
            id: entry.entry_id,
            name: entry.name,
            kind: kind_to_ui(entry.kind),
            bytes: Some(entry.size),
            packed: Some(entry.compressed_size),
            encrypted: entry.encrypted,
        }),
        wire::Message::ListEnd {
            entry_count,
            list_id,
        } => ui::Event::ListEnd {
            entry_count,
            list_id: list_to_ui(list_id),
        },
        wire::Message::PasswordRequired {
            challenge_id,
            attempt,
            purpose,
        } => ui::Event::PasswordRequired {
            challenge_id,
            attempt,
            encrypt: purpose == wire::PasswordPurpose::Encrypt,
        },
        wire::Message::ConflictRequired {
            challenge_id,
            entry_id,
            relative_name,
            incoming_size,
            existing_size,
        } if expected == Some(wire::ActionKind::Extract) => {
            ui::Event::ConflictRequired(ui::Conflict {
                challenge_id,
                entry_id,
                relative_name,
                incoming_size,
                existing_size,
            })
        }
        wire::Message::ConflictResolved {
            entry_id,
            relative_name,
            choice,
        } if expected == Some(wire::ActionKind::Extract) => {
            ui::Event::ConflictResolved(ui::Resolution {
                entry_id,
                relative_name,
                choice: conflict_to_ui(choice),
            })
        }
        wire::Message::Progress {
            stage,
            processed_bytes,
            total_bytes,
            completed_entries,
        } => ui::Event::Progress {
            stage: stage_to_ui(stage),
            processed: processed_bytes,
            total: total_bytes,
            completed_entries: completed_entries.into(),
        },
        wire::Message::Output {
            entry_id,
            relative_name,
            disposition,
        } => ui::Event::Output {
            entry_id,
            relative_name,
            disposition: disposition_to_ui(disposition),
        },
        wire::Message::CancelAck {
            request_sequence,
            status,
        } => ui::Event::CancelAck {
            request_sequence,
            status: cancel_to_ui(status),
        },
        wire::Message::Terminal(terminal) => ui::Event::Terminal(ui::Terminal {
            outcome: outcome_to_ui(terminal.outcome),
            error: error_to_ui(terminal.error),
            publication: publication_to_ui(terminal.publication),
            created_count: terminal.created_count,
            retained_partial_count: terminal.retained_partial_count,
            durability: durability_to_ui(terminal.durability),
            cleanup: cleanup_to_ui(terminal.cleanup),
        }),
        _ => return Err(()),
    })
}

// 실제 파이프 쓰기는 백그라운드에서 수행하고 UI에서는 한 프레임만 직렬화함.
#[derive(Clone, Default)]
struct FrameBuffer(Rc<RefCell<Vec<u8>>>);
impl Write for FrameBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut buffer = self.0.borrow_mut();
        if buffer.len().saturating_add(bytes.len()) > codec::MAX_BODY + 32 {
            return Err(io::Error::other("요청 프레임 한도 초과"));
        }
        buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct Frame(Vec<u8>, bool);
impl Drop for Frame {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
fn notify(events: &SyncSender<ui::HostEvent>, event: ui::HostEvent) -> bool {
    events.send(event).is_ok()
}
fn fault(events: &SyncSender<ui::HostEvent>, stop: &AtomicBool, message: &str) {
    stop.store(true, Ordering::Release);
    notify(events, ui::HostEvent::Fault(message.into()));
}

#[derive(Default)]
struct ResourceDeadline {
    pending: Option<(u64, wire::ResourceScope)>,
    approved: bool,
}

pub struct ProcessFactory {
    worker: PathBuf,
}
impl ProcessFactory {
    pub fn new(worker: PathBuf) -> Self {
        Self { worker }
    }
    pub fn sibling() -> Result<Self, String> {
        let executable =
            std::env::current_exe().map_err(|_| "앱 실행 위치를 확인하지 못했습니다")?;
        let parent = executable.parent().ok_or("앱 실행 폴더가 없습니다")?;
        Ok(Self::new(parent.join(if cfg!(windows) {
            "zipelo-archive-worker.exe"
        } else {
            "zipelo-archive-worker"
        })))
    }
}
impl ui::Factory for ProcessFactory {
    fn connect(&mut self, job_id: u64) -> Result<Box<dyn ui::Connection>, String> {
        if !self.worker.is_absolute() || job_id == 0 {
            return Err("작업자 경로 또는 작업 ID가 올바르지 않습니다".into());
        }
        let (send, requests) = mpsc::sync_channel::<Frame>(8);
        let (events, receive) = mpsc::sync_channel(64);
        let (ready, started) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let expected = Arc::new(Mutex::new(None));
        let resources = Arc::new(Mutex::new(ResourceDeadline::default()));
        let worker = self.worker.clone();
        let worker_stop = stop.clone();
        let worker_done = done.clone();
        let worker_expected = expected.clone();
        let worker_resources = resources.clone();
        let supervisor = thread::Builder::new()
            .name("zipelo-worker-owner".into())
            .spawn(move || {
                let mut command = Command::new(worker);
                command
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x08000000);
                }
                let child = match command.spawn() {
                    Ok(child) => child,
                    Err(_) => {
                        let _ = ready.send(Err(
                            "작업자를 실행하지 못했습니다. 앱과 작업자를 함께 설치하세요"
                                .to_string(),
                        ));
                        return;
                    }
                };
                if ready.send(Ok(())).is_err() {
                    let mut child = child;
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                supervise(
                    child,
                    job_id,
                    requests,
                    events,
                    worker_stop,
                    worker_done,
                    worker_expected,
                    worker_resources,
                );
            })
            .map_err(|_| "작업자 관리 스레드를 시작하지 못했습니다")?;
        match started.recv() {
            Ok(Ok(())) => {
                let buffer = FrameBuffer::default();
                Ok(Box::new(ProcessConnection {
                    job_id,
                    next: 0,
                    writer: codec::Writer::new(buffer.clone(), job_id),
                    buffer,
                    send: Some(send),
                    receive: Some(receive),
                    stop,
                    supervisor: Some(supervisor),
                    expected,
                    resources,
                    done,
                    disconnected: false,
                    reaped: false,
                }))
            }
            result => {
                stop.store(true, Ordering::Release);
                drop(send);
                drop(receive);
                let _ = supervisor.join();
                Err(result
                    .ok()
                    .and_then(Result::err)
                    .unwrap_or_else(|| "작업자 시작 결과가 없습니다".into()))
            }
        }
    }
}
struct ProcessConnection {
    job_id: u64,
    next: u64,
    writer: codec::Writer<FrameBuffer>,
    buffer: FrameBuffer,
    send: Option<SyncSender<Frame>>,
    receive: Option<Receiver<ui::HostEvent>>,
    stop: Arc<AtomicBool>,
    supervisor: Option<JoinHandle<()>>,
    expected: Arc<Mutex<Option<wire::ActionKind>>>,
    resources: Arc<Mutex<ResourceDeadline>>,
    done: Arc<AtomicBool>,
    disconnected: bool,
    reaped: bool,
}
impl ui::Connection for ProcessConnection {
    fn send(&mut self, request: ui::Request) -> Result<(), String> {
        if self.stop.load(Ordering::Acquire)
            || request.job_id != self.job_id
            || request.sequence != self.next
            || self.next >= codec::MAX_FRAMES
        {
            self.stop.store(true, Ordering::Release);
            return Err("작업자 요청 채널이 닫혔거나 순서가 올바르지 않습니다".into());
        }
        let is_cancel = matches!(request.command, ui::Command::Cancel);
        let expanded = matches!(&request.command, ui::Command::StartExpanded(_));
        let message = match request.command {
            ui::Command::Start(start) | ui::Command::StartExpanded(start) => {
                let start = start_to_wire(start);
                *self.expected.lock().map_err(|_| {
                    self.stop.store(true, Ordering::Release);
                    "작업자 요청 상태 오류"
                })? = Some(start.action.kind());
                let ceiling = if expanded {
                    wire::Limits {
                        input: u64::MAX,
                        output: 64 * 1024 * 1024 * 1024,
                        entries: 200_000,
                    }
                } else {
                    wire::Limits::default()
                };
                let interactive = start.action.kind() == wire::ActionKind::Extract;
                wire::Message::StartGuarded(wire::GuardedStart {
                    start,
                    ceiling,
                    interactive,
                })
            }
            ui::Command::ResourceReply {
                challenge_id,
                scope,
                allow,
            } => wire::Message::ResourceReply {
                challenge_id,
                scope,
                allow,
            },
            ui::Command::ConflictReply {
                challenge_id,
                choice,
                apply_to_remaining,
            } => wire::Message::ConflictReply {
                challenge_id,
                choice: conflict_to_wire(choice),
                apply_to_remaining,
            },
            ui::Command::Cancel => wire::Message::Cancel,
            ui::Command::Password {
                challenge_id,
                secret,
            } => wire::Message::Password {
                challenge_id,
                secret: wire::Secret(secret.as_bytes().to_vec()),
            },
        };
        if self.writer.send(&message).is_err() {
            self.buffer.0.borrow_mut().fill(0);
            self.buffer.0.borrow_mut().clear();
            self.stop.store(true, Ordering::Release);
            return Err("작업자 요청을 직렬화하지 못했습니다".into());
        }
        let mut resources = self.resources.lock().map_err(|_| {
            self.stop.store(true, Ordering::Release);
            "작업자 승인 상태 오류"
        })?;
        let bytes = Frame(std::mem::take(&mut *self.buffer.0.borrow_mut()), is_cancel);
        if self.send.as_ref().unwrap().try_send(bytes).is_err() {
            if is_cancel && self.done.load(Ordering::Acquire) {
                self.next += 1;
                return Ok(());
            }
            self.stop.store(true, Ordering::Release);
            return Err("작업자 요청 대기열이 닫혔거나 가득 찼습니다".into());
        }
        if let wire::Message::ResourceReply {
            challenge_id,
            scope,
            allow,
        } = message
            && resources.pending == Some((challenge_id, scope))
        {
            resources.approved |= allow;
            resources.pending = None;
        }
        self.next += 1;
        Ok(())
    }
    fn poll(&mut self) -> Option<ui::HostEvent> {
        match self.receive.as_ref()?.try_recv() {
            Ok(event) => {
                if matches!(event, ui::HostEvent::Exited { .. }) {
                    self.reaped = true;
                }
                Some(event)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) if !self.reaped && !self.disconnected => {
                self.disconnected = true;
                Some(ui::HostEvent::Fault(
                    "작업자 통신 스레드가 종료됐습니다".into(),
                ))
            }
            Err(_) => None,
        }
    }
}
impl Drop for ProcessConnection {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.send.take();
        self.receive.take();
        if let Some(supervisor) = self.supervisor.take() {
            let _ = supervisor.join();
        }
    }
}

fn supervise(
    mut child: std::process::Child,
    job_id: u64,
    requests: Receiver<Frame>,
    events: SyncSender<ui::HostEvent>,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    expected: Arc<Mutex<Option<wire::ActionKind>>>,
    resources: Arc<Mutex<ResourceDeadline>>,
) {
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut errors = child.stderr.take().unwrap();
    let mut threads = Vec::new();
    let cancel_write_failed = Arc::new(AtomicBool::new(false));
    let terminal_received = Arc::new(AtomicBool::new(false));
    let failed_cancel = cancel_write_failed.clone();
    let tx = events.clone();
    let abort = stop.clone();
    let finished = done.clone();
    threads.push(
        thread::Builder::new()
            .name("zipelo-worker-input".into())
            .spawn(move || {
                while !finished.load(Ordering::Acquire) && !abort.load(Ordering::Acquire) {
                    match requests.recv_timeout(Duration::from_millis(10)) {
                        Ok(frame) => {
                            if input
                                .write_all(&frame.0)
                                .and_then(|_| input.flush())
                                .is_err()
                            {
                                if frame.1 {
                                    // 취소 쓰기 실패는 응답과 실제 종료 결과를 확인한 뒤 판단함.
                                    failed_cancel.store(true, Ordering::Release);
                                } else {
                                    fault(&tx, &abort, "작업자 요청 파이프 쓰기에 실패했습니다");
                                }
                                break;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(_) => break,
                    }
                }
            }),
    );
    let tx = events.clone();
    let abort = stop.clone();
    let received_terminal = terminal_received.clone();
    let pending_resource = resources.clone();
    threads.push(
        thread::Builder::new()
            .name("zipelo-worker-output".into())
            .spawn(move || {
                let eof;
                {
                    let mut reader = codec::Reader::new(&mut output);
                    eof = loop {
                        match reader.read() {
                            Ok(Some((job, sequence, message))) => {
                                let action = expected.lock().ok().and_then(|value| *value);
                                let event = event_to_ui(message, action);
                                if job != job_id || event.is_err() {
                                    fault(&tx, &abort, "작업자 응답이 요청과 일치하지 않습니다");
                                    break false;
                                }
                                if let Ok(ui::Event::ResourceRequired(question)) = &event {
                                    if let Ok(mut state) = pending_resource.lock() {
                                        state.pending = Some((question.id, question.scope));
                                    } else {
                                        fault(&tx, &abort, "작업자 승인 상태 오류");
                                        break false;
                                    }
                                }
                                if matches!(event, Ok(ui::Event::Terminal(_))) {
                                    received_terminal.store(true, Ordering::Release);
                                }
                                if !notify(
                                    &tx,
                                    ui::HostEvent::Frame {
                                        job_id: job,
                                        sequence,
                                        event: event.unwrap(),
                                    },
                                ) {
                                    return;
                                }
                            }
                            Ok(None) => break true,
                            Err(_) => {
                                fault(&tx, &abort, "작업자 응답 프레임을 읽지 못했습니다");
                                break false;
                            }
                        }
                    };
                }
                if eof || io::copy(&mut output, &mut io::sink()).is_ok() {
                    notify(&tx, ui::HostEvent::StdoutEof);
                }
            }),
    );
    let tx = events.clone();
    let abort = stop.clone();
    threads.push(
        thread::Builder::new()
            .name("zipelo-worker-errors".into())
            .spawn(move || {
                let mut bytes = [0u8; 4096];
                loop {
                    match errors.read(&mut bytes) {
                        Ok(0) => {
                            notify(&tx, ui::HostEvent::StderrEof);
                            break;
                        }
                        Ok(_) => {}
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(_) => {
                            fault(&tx, &abort, "작업자 오류 파이프를 읽지 못했습니다");
                            break;
                        }
                    }
                }
            }),
    );
    let mut handles = Vec::new();
    for result in threads {
        match result {
            Ok(handle) => handles.push(handle),
            Err(_) => fault(&events, &stop, "작업자 통신 스레드를 시작하지 못했습니다"),
        }
    }
    let started = Instant::now();
    let status = loop {
        // 승인 뒤 실제 진행과 질문 대기 시간은 작업자의 60초 정체 감시로 판단함.
        // 응답 프레임 간격을 실제 파일 처리 진행으로 간주하지 않음.
        let approved = resources
            .lock()
            .map(|state| state.approved)
            .unwrap_or(false);
        if !approved && started.elapsed() >= Duration::from_secs(310) {
            fault(&events, &stop, "작업자 처리 제한 시간을 초과했습니다");
        }
        if stop.load(Ordering::Acquire) {
            let _ = child.kill();
        }
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                fault(&events, &stop, "작업자 종료 상태를 확인하지 못했습니다");
                let _ = child.kill();
                break child.wait().ok();
            }
        }
    };
    done.store(true, Ordering::Release);
    for handle in handles {
        if handle.join().is_err() {
            fault(&events, &stop, "작업자 통신 스레드 회수에 실패했습니다");
        }
    }
    if cancel_write_failed.load(Ordering::Acquire)
        && !(terminal_received.load(Ordering::Acquire)
            && status.as_ref().is_some_and(|status| status.success()))
    {
        fault(&events, &stop, "작업자 요청 파이프 쓰기에 실패했습니다");
    }
    if let Some(status) = status {
        // 회수와 모든 파이프 핸들 정리가 끝난 뒤에만 닫기 허용 근거를 전달함.
        notify(&events, ui::HostEvent::CleanupComplete);
        notify(
            &events,
            ui::HostEvent::Exited {
                success: status.success(),
            },
        );
    }
}

#[cfg(test)]
#[path = "../../../tests/rust-app-ui/conflict_codec.rs"]
mod conflict_codec_tests;
