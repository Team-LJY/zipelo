use crate::{
    archive,
    filesystem::{self, Dir},
    protocol::*,
    resources::{Budget, Monitor, Watchdog},
    wire::{Reader, Writer},
};
use std::{
    collections::BTreeSet,
    io::{Read, Seek, SeekFrom, Write},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, ErrorCode>;
#[derive(PartialEq, Eq)]
enum Challenge {
    Password(u64),
    Conflict(u64),
    Resource(u64, ResourceScope),
}
struct State<W> {
    writer: Writer<W>,
    cancelled: bool,
    phase: u8,
    fault: Option<ErrorCode>,
    challenge: Option<Challenge>,
    conflict_reply: Option<(ConflictChoice, bool)>,
    password: Option<Secret>,
    resource_reply: Option<bool>,
    input_closed: bool,
}
struct Job<W> {
    state: Arc<Mutex<State<W>>>,
    wake: Arc<Condvar>,
    next_challenge: u64,
    interactive: bool,
    action: ActionKind,
    budget: Option<Budget>,
    watchdog: Watchdog,
    remaining_choice: Option<ConflictChoice>,
    started: Instant,
    terminal: Terminal,
    parent: crate::parent::Parent,
}
impl<W: Write> Job<W> {
    fn request_resource(
        &mut self,
        scope: ResourceScope,
        mut usage: ResourceUsage,
        storage: Option<&std::fs::File>,
    ) -> Result<()> {
        let budget = self.budget.as_ref().ok_or(ErrorCode::Internal)?;
        let (soft_limits, ceiling) = (budget.soft, budget.ceiling);
        if let Some(file) = storage {
            usage.available_bytes =
                Some(crate::space::available(file)?.min(u64::MAX as u128) as u64);
        }
        self.next_challenge = self.next_challenge.checked_add(1).ok_or(ErrorCode::Limit)?;
        let id = self.next_challenge;
        let state_ref = Arc::clone(&self.state);
        let mut state = state_ref.lock().map_err(|_| ErrorCode::Internal)?;
        if state.challenge.is_some() {
            return Err(ErrorCode::Internal);
        }
        state.challenge = Some(Challenge::Resource(id, scope));
        state
            .writer
            .send(&Message::ResourceRequired {
                challenge_id: id,
                action: self.action,
                scope,
                usage,
                soft_limits,
                ceiling,
            })
            .map_err(|_| ErrorCode::Io)?;
        let waiting = Instant::now();
        loop {
            self.parent.check()?;
            if let Some(error) = state.fault {
                return Err(error);
            }
            if state.cancelled {
                return Err(ErrorCode::None);
            }
            if let Some(allow) = state.resource_reply.take() {
                if !allow {
                    return Err(ErrorCode::None);
                }
                self.budget
                    .as_mut()
                    .ok_or(ErrorCode::Internal)?
                    .grant(scope);
                self.watchdog.resume(self.started.elapsed(), true);
                return Ok(());
            }
            if state.input_closed {
                return Err(ErrorCode::Protocol);
            }
            if waiting.elapsed() >= Duration::from_secs(300) {
                return Err(ErrorCode::Limit);
            }
            state = self
                .wake
                .wait_timeout(state, Duration::from_millis(50))
                .map_err(|_| ErrorCode::Internal)?
                .0;
        }
    }
    fn password(&mut self, purpose: PasswordPurpose, attempt: u32) -> Result<Secret> {
        self.check()?;
        self.next_challenge = self.next_challenge.checked_add(1).ok_or(ErrorCode::Limit)?;
        let id = self.next_challenge;
        let state_ref = Arc::clone(&self.state);
        let mut state = state_ref.lock().map_err(|_| ErrorCode::Internal)?;
        state.challenge = Some(Challenge::Password(id));
        state
            .writer
            .send(&Message::PasswordRequired {
                challenge_id: id,
                attempt,
                purpose,
            })
            .map_err(|_| ErrorCode::Io)?;
        let waiting = Instant::now();
        loop {
            self.parent.check()?;
            if let Some(error) = state.fault {
                return Err(error);
            }
            if state.cancelled {
                return Err(ErrorCode::None);
            }
            if let Some(secret) = state.password.take() {
                self.watchdog.resume(self.started.elapsed(), false);
                return Ok(secret);
            }
            if state.input_closed {
                return Err(ErrorCode::Protocol);
            }
            if if self.budget.is_some() {
                waiting.elapsed() >= Duration::from_secs(300)
            } else {
                self.started.elapsed() > Duration::from_secs(300)
            } {
                return Err(ErrorCode::Limit);
            }
            state = self
                .wake
                .wait_timeout(state, Duration::from_millis(50))
                .map_err(|_| ErrorCode::Internal)?
                .0;
        }
    }

    fn conflict(&mut self, entry: &Entry, existing_size: u64) -> Result<ConflictChoice> {
        self.check()?;
        if let Some(choice) = self.remaining_choice {
            return Ok(choice);
        }
        self.next_challenge = self.next_challenge.checked_add(1).ok_or(ErrorCode::Limit)?;
        let id = self.next_challenge;
        let state_ref = Arc::clone(&self.state);
        let mut state = state_ref.lock().map_err(|_| ErrorCode::Internal)?;
        state.challenge = Some(Challenge::Conflict(id));
        state
            .writer
            .send(&Message::ConflictRequired {
                challenge_id: id,
                entry_id: entry.entry_id,
                relative_name: entry.name.clone(),
                incoming_size: entry.size,
                existing_size,
            })
            .map_err(|_| ErrorCode::Io)?;
        let waiting = Instant::now();
        loop {
            self.parent.check()?;
            if let Some(error) = state.fault {
                return Err(error);
            }
            if state.cancelled {
                return Err(ErrorCode::None);
            }
            if let Some((choice, apply)) = state.conflict_reply.take() {
                if apply {
                    self.remaining_choice = Some(choice);
                }
                self.watchdog.resume(self.started.elapsed(), false);
                return Ok(choice);
            }
            if state.input_closed {
                return Err(ErrorCode::Protocol);
            }
            if if self.budget.is_some() {
                waiting.elapsed() >= Duration::from_secs(300)
            } else {
                self.started.elapsed() > Duration::from_secs(300)
            } {
                return Err(ErrorCode::Limit);
            }
            state = self
                .wake
                .wait_timeout(state, Duration::from_millis(50))
                .map_err(|_| ErrorCode::Internal)?
                .0;
        }
    }
    fn rename(&mut self, root: &Dir, name: &str, reserved: &BTreeSet<String>) -> Result<String> {
        let (prefix, leaf) = name.rsplit_once('/').unwrap_or(("", name));
        let (stem, extension) = match leaf.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
            _ => (leaf, String::new()),
        };
        for index in 1..=10000 {
            self.check()?;
            fn clip(text: &str, bytes: usize) -> &str {
                let mut end = text.len().min(bytes);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                &text[..end]
            }
            let suffix = format!(" ({index})");
            let extension = clip(&extension, 255 - suffix.len() - 4).trim_end_matches(['.', ' ']);
            let stem = clip(stem, 255 - suffix.len() - extension.len());
            let stem = if stem.is_empty() { "file" } else { stem };
            let leaf = format!("{stem}{suffix}{extension}");
            let candidate = if prefix.is_empty() {
                leaf
            } else {
                format!("{prefix}/{leaf}")
            };
            archive::valid_name(&candidate, false)?;
            if !reserved.contains(&candidate.to_lowercase())
                && filesystem::unused(root, &candidate)?
            {
                return Ok(candidate);
            }
        }
        Err(ErrorCode::Conflict)
    }
    fn event(&mut self, event: Message) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| ErrorCode::Internal)?;
        state.writer.send(&event).map_err(|_| {
            state.fault = Some(ErrorCode::Io);
            ErrorCode::Io
        })
    }
    fn check(&mut self) -> Result<()> {
        self.parent.check()?;
        let state = self.state.lock().map_err(|_| ErrorCode::Internal)?;
        if let Some(error) = state.fault {
            return Err(error);
        }
        if state.cancelled && state.phase == 0 {
            return Err(ErrorCode::None);
        }
        self.watchdog.check(self.started.elapsed())
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
    fn output_reader(
        &mut self,
        root: &Dir,
        name: &str,
        data: &mut impl Read,
        length: u64,
        id: Option<u32>,
        last: bool,
        replace: Option<&filesystem::Existing>,
    ) -> Result<()> {
        self.check()?;
        self.progress(
            Stage::Write,
            0,
            Some(length),
            self.terminal.created_count as u32,
        )?;
        let mut first_chunk = true;
        let p = filesystem::publish_reader_mode(root, name, data, length, replace, &mut |commit| {
            if !first_chunk {
                self.activity()?;
            }
            first_chunk = false;
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
        if p.created {
            self.activity()?;
        }
        Ok(())
    }
    fn run(&mut self, start: Start) -> Result<()> {
        if !start.limits.valid() {
            return Err(ErrorCode::InvalidRequest);
        }
        let limits = self.budget.as_ref().map_or(start.limits, |b| b.ceiling);
        self.event(Message::Accepted {
            action: start.action.kind(),
            encryption: true,
        })?;
        self.check()?;
        if let Some(budget) = &self.budget {
            let scope = ResourceScope {
                output: budget.ceiling.output > budget.soft.output,
                entries: budget.ceiling.entries > budget.soft.entries,
                ratio: false,
            };
            if scope.any() {
                let temporary = crate::spool::temporary()?;
                self.request_resource(scope, ResourceUsage::default(), Some(&temporary))?;
            }
        }
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
                let mut snapshot =
                    crate::spool::Snapshot::open_reporting(&path, limits.input, &mut |step| {
                        match step {
                            crate::spool::Step::Check => self.check(),
                            crate::spool::Step::Read { bytes, total } => {
                                self.activity()?;
                                self.progress(Stage::Read, bytes, Some(total), 0)
                            }
                        }
                    })?;
                if let Action::Extract {
                    list_id: Some(expected),
                    ..
                } = &start.action
                {
                    if expected.catalog_version != 1
                        || expected.source_object != snapshot.object
                        || expected.archive_bytes != snapshot.len
                        || expected.archive_sha256 != snapshot.hash
                    {
                        return Err(ErrorCode::SourceChanged);
                    }
                }
                self.progress(Stage::Decode, snapshot.len, Some(snapshot.len), 0)?;
                let mut catalog = crate::streaming::Catalog::open(
                    &mut snapshot.file,
                    snapshot.len,
                    limits,
                    false,
                    None,
                    self,
                )?;
                let list_id = ListId {
                    catalog_version: 1,
                    format: catalog.format,
                    source_object: snapshot.object,
                    archive_bytes: snapshot.len,
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
                    let output_sizes = catalog
                        .members
                        .iter()
                        .filter(|m| {
                            m.entry.kind == EntryKind::File
                                && selected
                                    .as_ref()
                                    .is_none_or(|s| s.contains(&m.entry.entry_id))
                        })
                        .map(|m| (m.entry.name.clone(), m.entry.size))
                        .collect::<Vec<_>>();
                    let staged = if matches!(catalog.format, Format::Zip | Format::SevenZip) {
                        Some((
                            &catalog.data,
                            catalog.members.iter().try_fold(0u64, |sum, m| {
                                sum.checked_add(m.entry.size).ok_or(ErrorCode::Limit)
                            })?,
                        ))
                    } else {
                        None
                    };
                    // 선택 전에는 검증에 필요한 스테이징 공간만 계산함.
                    let initial_outputs = if self.interactive {
                        &[][..]
                    } else {
                        &output_sizes[..]
                    };
                    filesystem::preflight_space(&root, initial_outputs, staged)?;
                    let encrypted = catalog.members.iter().any(|m| m.entry.encrypted);
                    if matches!(catalog.format, Format::Zip | Format::SevenZip) {
                        for attempt in 1..=3 {
                            let secret = if encrypted {
                                Some(self.password(PasswordPurpose::Decrypt, attempt)?)
                            } else {
                                None
                            };
                            match crate::streaming::Catalog::open(
                                &mut snapshot.file,
                                snapshot.len,
                                limits,
                                true,
                                secret.as_ref().map(|s| s.0.as_slice()),
                                self,
                            ) {
                                Ok(decoded) => {
                                    catalog = decoded;
                                    break;
                                }
                                Err(ErrorCode::WrongPassword) if attempt < 3 => continue,
                                Err(error) => return Err(error),
                            }
                        }
                    }
                    let offsets = catalog.offsets;
                    let mut data = catalog.data;
                    let members = catalog
                        .members
                        .into_iter()
                        .filter(|m| {
                            selected
                                .as_ref()
                                .is_none_or(|s| s.contains(&m.entry.entry_id))
                        })
                        .collect::<Vec<_>>();
                    let dirs = archive::output_directories(&members)?;
                    let files = members
                        .iter()
                        .filter(|m| m.entry.kind == EntryKind::File)
                        .collect::<Vec<_>>();
                    let mut names = dirs
                        .iter()
                        .map(|n| (n.clone(), EntryKind::Directory))
                        .collect::<Vec<_>>();
                    names.extend(
                        files
                            .iter()
                            .map(|m| (m.entry.name.clone(), EntryKind::File)),
                    );
                    filesystem::preflight_mode(&root, &names, self.interactive)?;
                    if !self.interactive {
                        filesystem::preflight_space(&root, &output_sizes, None)?;
                    }
                    let mut reserved = names
                        .iter()
                        .map(|(name, _)| name.to_lowercase())
                        .collect::<BTreeSet<_>>();
                    for name in dirs {
                        self.check()?;
                        let (created, durability, sync_error) =
                            filesystem::directory(&root, &name)?;
                        if created {
                            self.terminal.created_count += 1;
                            self.terminal.publication = Publication::Partial;
                            self.terminal.durability = durability;
                            self.event(Message::Output {
                                entry_id: None,
                                relative_name: name,
                                disposition: Disposition::Created,
                            })?;
                            if let Some(error) = sync_error {
                                return Err(error);
                            }
                            self.activity()?;
                        }
                    }
                    for (index, m) in files.iter().enumerate() {
                        data.seek(SeekFrom::Start(offsets[m.entry.entry_id as usize]))
                            .map_err(|_| ErrorCode::Io)?;
                        self.check()?;
                        let existing = if self.interactive {
                            filesystem::existing(&root, &m.entry.name)?
                        } else {
                            None
                        };
                        let mut name = m.entry.name.clone();
                        let mut replace = None;
                        if let Some(ref existing) = existing {
                            let choice = self.conflict(&m.entry, existing.size)?;
                            self.check()?;
                            if choice == ConflictChoice::Rename {
                                name = self.rename(&root, &name, &reserved)?;
                                reserved.insert(name.to_lowercase());
                            } else if choice == ConflictChoice::Overwrite {
                                replace = Some(existing);
                            }
                            self.event(Message::ConflictResolved {
                                entry_id: m.entry.entry_id,
                                relative_name: name.clone(),
                                choice,
                            })?;
                            if choice == ConflictChoice::Skip {
                                continue;
                            }
                        }
                        if self.interactive {
                            // 건너뛴 파일을 제외하고 이번에 게시할 파일의 공간을 확인함.
                            filesystem::preflight_space(
                                &root,
                                &[(name.clone(), m.entry.size)],
                                None,
                            )?;
                        }
                        self.output_reader(
                            &root,
                            &name,
                            &mut (&mut data).take(m.entry.size),
                            m.entry.size,
                            Some(m.entry.entry_id),
                            index + 1 == files.len(),
                            replace,
                        )?;
                    }
                    if !self.interactive || self.terminal.created_count != 0 {
                        self.terminal.publication = Publication::Complete;
                    }
                }
            }
            Action::CreateZip {
                sources,
                output_file,
                method,
                encryption,
            } => {
                let output = output_file.to_path()?;
                let leaf = output
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or(ErrorCode::InvalidRequest)?;
                let name = archive::valid_name(leaf, false)?;
                let root = Dir::root(output.parent().ok_or(ErrorCode::InvalidRequest)?)?;
                filesystem::preflight(&root, &[(name.clone(), EntryKind::File)])?;
                let members = crate::create::plan(&sources, limits, self)?;
                let secret = if encryption == Encryption::Aes256 {
                    let secret = self.password(PasswordPurpose::Encrypt, 1)?;
                    if secret.0.is_empty() {
                        return Err(ErrorCode::InvalidRequest);
                    }
                    Some(secret)
                } else {
                    None
                };
                let (mut data, length) = crate::create::zip(
                    &members,
                    method,
                    limits,
                    secret.as_ref().map(|s| s.0.as_slice()),
                    self,
                )?;
                filesystem::preflight_space(&root, &[(name.clone(), length)], None)?;
                self.output_reader(&root, &name, &mut data, length, None, true, None)?;
                self.terminal.publication = Publication::Complete;
            }
        }
        self.cutoff()?;
        Ok(())
    }
}
impl<W: Write> Monitor for Job<W> {
    fn check(&mut self) -> Result<()> {
        Job::check(self)
    }
    fn activity(&mut self) -> Result<()> {
        self.check()?;
        self.watchdog.activity(self.started.elapsed())
    }
    fn observe(&mut self, usage: ResourceUsage, storage: Option<&std::fs::File>) -> Result<()> {
        self.check()?;
        let Some(budget) = &self.budget else {
            return Ok(());
        };
        let scope = budget.required(usage)?;
        if !scope.any() {
            return Ok(());
        }
        self.request_resource(scope, usage, storage)
    }
}

pub fn serve<R: Read + Send + 'static, W: Write + Send + 'static>(
    input: R,
    output: W,
) -> std::io::Result<()> {
    let parent = crate::parent::Parent::capture()
        .map_err(|_| std::io::Error::other("worker parent unavailable"))?;
    let mut reader = Reader::new(input);
    let (job, _, message) = reader
        .read()?
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::UnexpectedEof))?;
    let (start, interactive, budget) = match message {
        Message::Start(start) => (start, false, None),
        Message::StartInteractive(start) => (start, true, None),
        Message::StartGuarded(guarded) if guarded.valid() => {
            let budget = Budget {
                soft: guarded.start.limits,
                ceiling: guarded.ceiling,
                granted: ResourceScope::default(),
            };
            (guarded.start, guarded.interactive, Some(budget))
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Start required",
            ));
        }
    };
    let state = Arc::new(Mutex::new(State {
        writer: Writer::new(output, job),
        cancelled: false,
        phase: 0,
        fault: None,
        challenge: None,
        conflict_reply: None,
        password: None,
        resource_reply: None,
        input_closed: false,
    }));
    let wake = Arc::new(Condvar::new());
    let control_wake = Arc::clone(&wake);
    let control = Arc::clone(&state);
    std::thread::spawn(move || {
        loop {
            let command = reader.read();
            let Ok(Some((_, sequence, message))) = command else {
                if let Ok(mut state) = control.lock() {
                    state.input_closed = true;
                    if command.is_err() && state.phase < 2 {
                        state.fault = Some(ErrorCode::Protocol);
                    }
                    control_wake.notify_all();
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
                Message::Password {
                    challenge_id,
                    secret,
                } => {
                    if state.phase != 0
                        || state.challenge != Some(Challenge::Password(challenge_id))
                        || state.password.is_some()
                    {
                        state.fault = Some(ErrorCode::Protocol);
                    } else {
                        state.challenge = None;
                        if !state.cancelled {
                            state.password = Some(secret);
                        }
                    }
                    control_wake.notify_all();
                }
                Message::ConflictReply {
                    challenge_id,
                    choice,
                    apply_to_remaining,
                } => {
                    if state.phase != 0
                        || state.challenge != Some(Challenge::Conflict(challenge_id))
                        || state.conflict_reply.is_some()
                    {
                        state.fault = Some(ErrorCode::Protocol);
                    } else {
                        state.challenge = None;
                        if !state.cancelled {
                            state.conflict_reply = Some((choice, apply_to_remaining));
                        }
                    }
                    control_wake.notify_all();
                }
                Message::ResourceReply {
                    challenge_id,
                    scope,
                    allow,
                } => {
                    if state.phase != 0
                        || state.challenge != Some(Challenge::Resource(challenge_id, scope))
                        || state.resource_reply.is_some()
                    {
                        state.fault = Some(ErrorCode::Protocol);
                    } else {
                        state.challenge = None;
                        if !state.cancelled {
                            state.resource_reply = Some(allow);
                        }
                    }
                    control_wake.notify_all();
                }
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
                    control_wake.notify_all();
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
        wake,
        next_challenge: 0,
        interactive,
        action: start.action.kind(),
        budget,
        watchdog: Watchdog::new(),
        remaining_choice: None,
        parent,
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
    state.challenge = None;
    state.password = None;
    state.conflict_reply = None;
    state.resource_reply = None;
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
            ErrorCode::DiskFull => {
                "디스크 여유 공간이 부족합니다. 공간을 확보한 뒤 다시 시도하세요"
            }
            ErrorCode::Io => {
                "파일을 읽거나 쓰지 못했습니다. 디스크 여유 공간과 접근 권한을 확인하세요"
            }
            _ => "작업 실패. 오류 코드와 게시 상태를 확인해야 함",
        }
        .to_owned();
    }
    state.writer.send(&Message::Terminal(context.terminal))?;
    state.phase = 3;
    Ok(())
}
