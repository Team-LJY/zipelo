use crate::{
    archive::{self, Member},
    filesystem::{self, Dir},
    protocol::*,
    wire::{Reader, Writer},
};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, ErrorCode>;
struct State<W> {
    writer: Writer<W>,
    cancelled: bool,
    phase: u8,
    fault: Option<ErrorCode>,
}
struct Job<W> {
    state: Arc<Mutex<State<W>>>,
    started: Instant,
    terminal: Terminal,
}
impl<W: Write> Job<W> {
    fn event(&mut self, event: Message) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| ErrorCode::Internal)?;
        state.writer.send(&event).map_err(|_| {
            state.fault = Some(ErrorCode::Io);
            ErrorCode::Io
        })
    }
    fn check(&mut self) -> Result<()> {
        if self.started.elapsed() > Duration::from_secs(300) {
            return Err(ErrorCode::Limit);
        }
        let state = self.state.lock().map_err(|_| ErrorCode::Internal)?;
        if let Some(error) = state.fault {
            return Err(error);
        }
        if state.cancelled && state.phase == 0 {
            return Err(ErrorCode::None);
        }
        Ok(())
    }
    fn cutoff(&mut self) -> Result<()> {
        self.check()?;
        let mut state = self.state.lock().map_err(|_| ErrorCode::Internal)?;
        if state.phase == 0 && state.cancelled {
            return Err(ErrorCode::None);
        }
        state.phase = 1;
        Ok(())
    }
    fn progress(
        &mut self,
        stage: Stage,
        bytes: u64,
        total: Option<u64>,
        entries: u32,
    ) -> Result<()> {
        self.event(Message::Progress {
            stage,
            processed_bytes: bytes,
            total_bytes: total,
            completed_entries: entries,
        })
    }
    fn output(
        &mut self,
        root: &Dir,
        name: &str,
        data: &[u8],
        id: Option<u32>,
        last: bool,
    ) -> Result<()> {
        self.check()?;
        self.progress(
            Stage::Write,
            0,
            Some(data.len() as u64),
            self.terminal.created_count as u32,
        )?;
        let p = filesystem::publish(root, name, data, &mut |commit| {
            if commit && last {
                self.cutoff()
            } else {
                self.check()
            }
        });
        self.terminal.retained_partial_count += p.retained;
        if p.cleanup != Cleanup::Complete {
            self.terminal.cleanup = p.cleanup;
        }
        if p.created {
            self.terminal.created_count += 1;
            self.terminal.publication = Publication::Partial;
            self.terminal.durability = match (self.terminal.durability, p.durability) {
                (Durability::Failed, _) | (_, Durability::Failed) => Durability::Failed,
                (Durability::Unconfirmed, _) | (_, Durability::Unconfirmed) => {
                    Durability::Unconfirmed
                }
                _ => p.durability,
            };
            self.event(Message::Output {
                entry_id: id,
                relative_name: name.to_owned(),
                disposition: Disposition::Created,
            })?;
        }
        if p.uncertain {
            self.terminal.publication = Publication::Uncertain;
            self.terminal.durability = Durability::Unconfirmed;
            self.event(Message::Output {
                entry_id: id,
                relative_name: name.to_owned(),
                disposition: Disposition::CommitUncertain,
            })?;
        }
        if p.retained != 0 {
            self.event(Message::Output {
                entry_id: id,
                relative_name: p.retained_name.clone().ok_or(ErrorCode::Internal)?,
                disposition: Disposition::PartialRetained,
            })?;
        }
        if let Some(error) = p.error {
            return Err(error);
        }
        Ok(())
    }
    fn run(&mut self, start: Start) -> Result<()> {
        if !start.limits.valid() {
            return Err(ErrorCode::InvalidRequest);
        }
        self.event(Message::Accepted {
            action: start.action.kind(),
            encryption: false,
        })?;
        self.check()?;
        match start.action {
            Action::List {
                archive: ref source,
            }
            | Action::Extract {
                archive: ref source,
                ..
            } => {
                let extract = matches!(start.action, Action::Extract { .. });
                let path = source.to_path()?;
                self.progress(Stage::Read, 0, None, 0)?;
                let snapshot =
                    filesystem::snapshot(&path, start.limits.input, &mut || self.check())?;
                if let Action::Extract {
                    list_id: Some(expected),
                    ..
                } = &start.action
                {
                    if expected.catalog_version != 1
                        || expected.source_object != snapshot.object
                        || expected.archive_bytes != snapshot.bytes.len() as u64
                        || expected.archive_sha256 != snapshot.hash
                    {
                        return Err(ErrorCode::SourceChanged);
                    }
                }
                self.progress(
                    Stage::Decode,
                    snapshot.bytes.len() as u64,
                    Some(snapshot.bytes.len() as u64),
                    0,
                )?;
                let mut catalog =
                    archive::parse(&snapshot.bytes, start.limits, extract, &mut || self.check())?;
                let list_id = ListId {
                    catalog_version: 1,
                    format: catalog.format,
                    source_object: snapshot.object,
                    archive_bytes: snapshot.bytes.len() as u64,
                    archive_sha256: snapshot.hash,
                };
                if !extract {
                    for member in &catalog.members {
                        self.check()?;
                        self.event(Message::Entry(member.entry.clone()))?;
                    }
                    self.event(Message::ListEnd {
                        entry_count: catalog.members.len() as u32,
                        list_id,
                    })?;
                } else if let Action::Extract {
                    destination,
                    selection,
                    list_id: expected,
                    gzip_output_name,
                    ..
                } = start.action
                {
                    if expected
                        .as_ref()
                        .is_some_and(|expected| *expected != list_id)
                    {
                        return Err(ErrorCode::SourceChanged);
                    }
                    let selected = match selection {
                        Selection::All => None,
                        Selection::EntryIds(ids) => {
                            let n = ids.len();
                            let set = ids.into_iter().collect::<BTreeSet<_>>();
                            if expected.is_none()
                                || set.is_empty()
                                || n != set.len()
                                || set.iter().any(|id| *id as usize >= catalog.members.len())
                            {
                                return Err(ErrorCode::InvalidRequest);
                            }
                            Some(set)
                        }
                    };
                    if catalog.members.iter().any(|m| m.entry.encrypted) {
                        return Err(ErrorCode::UnsupportedFeature);
                    }
                    if catalog
                        .members
                        .iter()
                        .any(|m| !matches!(m.entry.kind, EntryKind::File | EntryKind::Directory))
                    {
                        return Err(ErrorCode::UnsafePath);
                    }
                    if catalog.format == Format::Gzip {
                        let name = gzip_output_name.ok_or(ErrorCode::InvalidRequest)?;
                        let checked = archive::valid_name(&name, false)
                            .map_err(|_| ErrorCode::InvalidRequest)?;
                        if checked.contains('/') {
                            return Err(ErrorCode::InvalidRequest);
                        }
                        catalog.members[0].entry.name = checked;
                    } else if gzip_output_name.is_some() {
                        return Err(ErrorCode::InvalidRequest);
                    }
                    let root = Dir::root(&destination.to_path()?)?;
                    let members = catalog
                        .members
                        .into_iter()
                        .filter(|m| {
                            selected
                                .as_ref()
                                .is_none_or(|s| s.contains(&m.entry.entry_id))
                        })
                        .collect::<Vec<_>>();
                    let mut dirs = BTreeSet::new();
                    let files = members
                        .iter()
                        .filter(|m| m.entry.kind == EntryKind::File)
                        .collect::<Vec<_>>();
                    for m in &members {
                        if m.entry.kind == EntryKind::Directory {
                            dirs.insert(m.entry.name.clone());
                        }
                        let mut name = m.entry.name.as_str();
                        while let Some((parent, _)) = name.rsplit_once('/') {
                            dirs.insert(parent.to_owned());
                            name = parent;
                        }
                    }
                    let mut names = dirs
                        .iter()
                        .map(|n| (n.clone(), EntryKind::Directory))
                        .collect::<Vec<_>>();
                    names.extend(
                        files
                            .iter()
                            .map(|m| (m.entry.name.clone(), EntryKind::File)),
                    );
                    archive::validate_members(&members)?;
                    filesystem::preflight(&root, &names)?;
                    for name in dirs {
                        self.check()?;
                        let (created, durability) = filesystem::directory(&root, &name)?;
                        if created {
                            self.terminal.created_count += 1;
                            self.terminal.publication = Publication::Partial;
                            self.terminal.durability = durability;
                            self.event(Message::Output {
                                entry_id: None,
                                relative_name: name,
                                disposition: Disposition::Created,
                            })?;
                            if durability == Durability::Failed {
                                return Err(ErrorCode::Io);
                            }
                        }
                    }
                    for (index, m) in files.iter().enumerate() {
                        self.output(
                            &root,
                            &m.entry.name,
                            &m.data,
                            Some(m.entry.entry_id),
                            index + 1 == files.len(),
                        )?;
                    }
                    self.terminal.publication = Publication::Complete;
                }
            }
            Action::CreateZip {
                sources,
                output_file,
                method,
                encryption,
            } => {
                if encryption != Encryption::None {
                    return Err(ErrorCode::UnsupportedFeature);
                }
                if sources.len() > start.limits.entries as usize {
                    return Err(ErrorCode::Limit);
                }
                let mut members = Vec::new();
                let mut consumed = 0;
                for source in sources {
                    self.check()?;
                    let name = archive::valid_name(&source.archive_name, false)?;
                    let snapshot = filesystem::snapshot(
                        &source.path.to_path()?,
                        start.limits.input - consumed,
                        &mut || self.check(),
                    )?;
                    consumed += snapshot.bytes.len() as u64;
                    members.push(Member {
                        entry: Entry {
                            entry_id: members.len() as u32,
                            name,
                            kind: EntryKind::File,
                            size: snapshot.bytes.len() as u64,
                            compressed_size: 0,
                            encrypted: false,
                        },
                        data: snapshot.bytes,
                    });
                }
                let bytes =
                    archive::create_zip(&members, method, start.limits, &mut || self.check())?;
                let output = output_file.to_path()?;
                let leaf = output
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or(ErrorCode::InvalidRequest)?;
                let name = archive::valid_name(leaf, false)?;
                let root = Dir::root(output.parent().ok_or(ErrorCode::InvalidRequest)?)?;
                filesystem::preflight(&root, &[(name.clone(), EntryKind::File)])?;
                self.output(&root, &name, &bytes, None, true)?;
                self.terminal.publication = Publication::Complete;
            }
        }
        self.cutoff()?;
        Ok(())
    }
}
pub fn serve<R: Read + Send + 'static, W: Write + Send + 'static>(
    input: R,
    output: W,
) -> std::io::Result<()> {
    let mut reader = Reader::new(input);
    let (job, _, message) = reader
        .read()?
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::UnexpectedEof))?;
    let Message::Start(start) = message else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Start required",
        ));
    };
    let state = Arc::new(Mutex::new(State {
        writer: Writer::new(output, job),
        cancelled: false,
        phase: 0,
        fault: None,
    }));
    let control = Arc::clone(&state);
    std::thread::spawn(move || {
        loop {
            let command = reader.read();
            let Ok(Some((_, sequence, message))) = command else {
                if command.is_err() {
                    if let Ok(mut state) = control.lock() {
                        if state.phase < 2 {
                            state.fault = Some(ErrorCode::Protocol);
                        }
                    }
                }
                break;
            };
            let Ok(mut state) = control.lock() else {
                break;
            };
            if state.phase == 3 {
                break;
            }
            match message {
                Message::Cancel => {
                    let status = match state.phase {
                        2 => CancelStatus::Finished,
                        1 => CancelStatus::TooLate,
                        _ if state.cancelled => CancelStatus::AlreadyRequested,
                        _ => {
                            state.cancelled = true;
                            CancelStatus::Requested
                        }
                    };
                    if state
                        .writer
                        .send(&Message::CancelAck {
                            request_sequence: sequence,
                            status,
                        })
                        .is_err()
                    {
                        state.fault = Some(ErrorCode::Io);
                        break;
                    }
                }
                _ => {
                    state.fault = Some(ErrorCode::Protocol);
                    break;
                }
            }
        }
    });
    let mut context = Job {
        state: Arc::clone(&state),
        started: Instant::now(),
        terminal: Terminal {
            outcome: Outcome::Completed,
            error: ErrorCode::None,
            publication: Publication::None,
            created_count: 0,
            retained_partial_count: 0,
            durability: Durability::NotApplicable,
            cleanup: Cleanup::Complete,
            detail: String::new(),
        },
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| context.run(start)));
    let error = match result {
        Ok(r) => r.err(),
        Err(_) => {
            context.terminal.publication = Publication::Uncertain;
            context.terminal.cleanup = Cleanup::Unconfirmed;
            Some(ErrorCode::Internal)
        }
    };
    let mut state = state
        .lock()
        .map_err(|_| std::io::Error::other("worker state poisoned"))?;
    state.phase = 2;
    if let Some(error) = state.fault.or(error) {
        context.terminal.error = error;
        context.terminal.outcome = if error == ErrorCode::None {
            Outcome::Cancelled
        } else {
            Outcome::Failed
        };
        context.terminal.detail = match error {
            ErrorCode::None => "취소됨",
            ErrorCode::UnsupportedFeature => "암호 또는 해당 기능 미지원",
            ErrorCode::SourceChanged => "파일이 변경됨. 다시 열어 목록을 확인해야 함",
            _ => "작업 실패. 오류 코드와 게시 상태를 확인해야 함",
        }
        .to_owned();
    }
    state.writer.send(&Message::Terminal(context.terminal))?;
    state.phase = 3;
    Ok(())
}
