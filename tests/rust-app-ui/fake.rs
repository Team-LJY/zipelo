use std::{cell::RefCell, collections::VecDeque, rc::Rc};
use zipelo_ui_memory_app::archive_ui::*;

#[derive(Default)]
pub struct State {
    pub requests: Vec<(u64, &'static str)>,
    pub starts: Vec<Start>,
    pub password_seen: bool,
    pub job: u64,
    sequence: u64,
    pub queue: VecDeque<HostEvent>,
    pub manual: bool,
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
                let format = if archive.extension().is_some_and(|x| x == "gz") {
                    Format::Gzip
                } else {
                    Format::Zip
                };
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
        match request.command {
            Command::Start(start) => {
                s.requests.push((request.sequence, "start"));
                s.starts.push(start.clone());
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
                s.requests.push((request.sequence, "password"));
                if !s.manual {
                    s.complete_action();
                }
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
