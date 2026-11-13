use std::{cell::RefCell, collections::VecDeque, rc::Rc};
use zipelo_ui_memory_app::archive_ui::*;

#[derive(Default)]
pub struct State {
    pub requests: Vec<(u64, &'static str)>,
    pub starts: Vec<Start>,
    pub password_seen: bool,
    pub expanded: bool,
    pub resources: Vec<(u64, ResourceScope, bool)>,
    /// 마지막으로 받은 암호 원문. 화면에서 입력한 암호가 그대로 전달됐는지 확인할 때 씀.
    pub password: Vec<u8>,
    pub job: u64,
    sequence: u64,
    pub queue: VecDeque<HostEvent>,
    pub manual: bool,
    pub list_format: Option<Format>,
    pub fail_start: bool,
    pub dropped_connections: usize,
    pause: bool,
}
#[derive(Default, Clone)]
pub struct FakeFactory {
    pub state: Rc<RefCell<State>>,
}
impl FakeFactory {
    pub fn manual() -> Self {
        let f = Self::default();
        f.state.borrow_mut().manual = true;
        f
    }
}
impl State {
    pub fn frame(&mut self, event: Event) {
        let sequence = self.sequence;
        self.sequence += 1;
        self.queue.push_back(HostEvent::Frame {
            job_id: self.job,
            sequence,
            event,
        });
    }
    pub fn finish(&mut self, outcome: Outcome, publication: Publication) {
        self.frame(Event::Terminal(Terminal {
            outcome,
            error: None,
            publication,
            created_count: if publication == Publication::Complete {
                1
            } else {
                0
            },
            retained_partial_count: 0,
            durability: Durability::Synced,
            cleanup: Cleanup::Complete,
        }));
        self.queue.push_back(HostEvent::StdoutEof);
        self.queue.push_back(HostEvent::StderrEof);
        self.queue.push_back(HostEvent::Exited { success: true });
    }
    fn complete_action(&mut self) {
        match self.starts.last().cloned().unwrap() {
            Start::List { archive } => {
                self.frame(Event::Entry(Entry {
                    id: 0,
                    name: "읽어주세요.txt".into(),
                    kind: Kind::File,
                    bytes: Some(31),
                    packed: Some(54),
                    encrypted: false,
                }));
                let format = self.list_format.unwrap_or_else(|| {
                    if archive.extension().is_some_and(|x| x == "gz") {
                        Format::Gzip
                    } else {
                        Format::Zip
                    }
                });
                self.frame(Event::ListEnd {
                    entry_count: 1,
                    list_id: ListId {
                        catalog_version: 1,
                        format,
                        source_object: SourceObject::Unix {
                            device: 1,
                            inode: 1,
                        },
                        archive_bytes: 54,
                        archive_sha256: [7; 32],
                    },
                });
                self.finish(Outcome::Completed, Publication::None);
            }
            _ => {
                self.frame(Event::Progress {
                    stage: Stage::Write,
                    processed: 31,
                    total: Some(31),
                    completed_entries: 1,
                });
                self.frame(Event::Output {
                    entry_id: Some(0),
                    relative_name: "읽어주세요.txt".into(),
                    disposition: Disposition::Created,
                });
                self.finish(Outcome::Completed, Publication::Complete);
            }
        }
    }
}
struct FakeConnection {
    state: Rc<RefCell<State>>,
}
impl Drop for FakeConnection {
    fn drop(&mut self) {
        self.state.borrow_mut().dropped_connections += 1;
    }
}
impl Factory for FakeFactory {
    fn connect(&mut self, job: u64) -> Result<Box<dyn Connection>, String> {
        let mut s = self.state.borrow_mut();
        s.job = job;
        s.sequence = 0;
        s.queue.clear();
        s.pause = false;
        Ok(Box::new(FakeConnection {
            state: self.state.clone(),
        }))
    }
}
impl Connection for FakeConnection {
    fn send(&mut self, request: Request) -> Result<(), String> {
        let mut s = self.state.borrow_mut();
        assert_eq!(s.job, request.job_id);
        if matches!(&request.command, Command::StartExpanded(_)) {
            s.expanded = true;
        }
        match request.command {
            Command::Start(start) | Command::StartExpanded(start) => {
                if let Start::Extract {
                    list_id,
                    gzip_output_name,
                    ..
                } = &start
                {
                    let valid = match list_id.format {
                        Format::Gzip => gzip_output_name.as_deref().is_some_and(valid_leaf),
                        _ => gzip_output_name.is_none(),
                    };
                    if !valid {
                        return Err("형식에 맞지 않는 GZIP 출력명".into());
                    }
                }
                s.requests.push((request.sequence, "start"));
                s.starts.push(start.clone());
                if s.fail_start {
                    return Err("Start 전송 실패".into());
                }
                if !s.manual {
                    s.frame(Event::Accepted { encryption: false });
                    s.frame(Event::Progress {
                        stage: Stage::Scan,
                        processed: 0,
                        total: None,
                        completed_entries: 0,
                    });
                    if matches!(&start,Start::List {archive} if archive.file_name().is_some_and(|n|n.to_string_lossy().contains("locked")))
                    {
                        s.frame(Event::PasswordRequired {
                            challenge_id: 17,
                            attempt: 1,
                            encrypt: false,
                        });
                    } else {
                        s.complete_action();
                    }
                }
            }
            Command::Password {
                challenge_id,
                secret,
            } => {
                assert_eq!(challenge_id, 17);
                assert!(!secret.as_bytes().is_empty());
                s.password_seen = true;
                s.password = secret.as_bytes().to_vec();
                s.requests.push((request.sequence, "password"));
                if !s.manual {
                    s.complete_action();
                }
            }
            Command::ResourceReply {
                challenge_id,
                scope,
                allow,
            } => {
                s.resources.push((challenge_id, scope, allow));
            }
            Command::Cancel => {
                s.requests.push((request.sequence, "cancel"));
                if !s.manual {
                    let consumed = s
                        .queue
                        .iter()
                        .filter(|e| matches!(e, HostEvent::Frame { .. }))
                        .count() as u64;
                    s.sequence -= consumed;
                    s.queue.clear();
                    s.frame(Event::CancelAck {
                        request_sequence: request.sequence,
                        status: CancelStatus::Requested,
                    });
                    s.finish(Outcome::Cancelled, Publication::None);
                }
            }
        }
        Ok(())
    }
    fn poll(&mut self) -> Option<HostEvent> {
        let mut s = self.state.borrow_mut();
        if s.pause {
            s.pause = false;
            return None;
        }
        s.pause = true;
        s.queue.pop_front()
    }
}
