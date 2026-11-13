use super::*;
use std::collections::VecDeque;
struct FirstLimit {
    first: bool,
    process: ProcessFactory,
}
struct Failed {
    job: u64,
    events: VecDeque<HostEvent>,
}
impl Connection for Failed {
    fn send(&mut self, request: Request) -> Result<(), String> {
        assert!(matches!(request.command, Command::Start(_)));
        self.events.extend([
            HostEvent::Frame {
                job_id: self.job,
                sequence: 0,
                event: Event::Accepted { encryption: true },
            },
            HostEvent::Frame {
                job_id: self.job,
                sequence: 1,
                event: Event::Terminal(Terminal {
                    outcome: Outcome::Failed,
                    error: Some(ErrorCode::Limit),
                    publication: Publication::None,
                    created_count: 0,
                    retained_partial_count: 0,
                    durability: Durability::NotApplicable,
                    cleanup: Cleanup::Complete,
                }),
            },
            HostEvent::StdoutEof,
            HostEvent::StderrEof,
            HostEvent::Exited { success: true },
        ]);
        Ok(())
    }
    fn poll(&mut self) -> Option<HostEvent> {
        self.events.pop_front()
    }
}
impl Factory for FirstLimit {
    fn connect(&mut self, job: u64) -> Result<Box<dyn Connection>, String> {
        if self.first {
            self.first = false;
            Ok(Box::new(Failed {
                job,
                events: VecDeque::new(),
            }))
        } else {
            self.process.connect(job)
        }
    }
}
#[test]
fn model_retry_uses_real_guarded_codec_and_requires_a_new_job_approval() {
    for mode in 0..3 {
        let scratch = Scratch::new();
        let archive = scratch.path("sample.zip");
        fs::write(&archive, include_bytes!("fixtures/sample.zip")).unwrap();
        let mut factory = FirstLimit {
            first: true,
            process: ProcessFactory::new(binary("zipelo-archive-worker")),
        };
        let mut model = Model::default();
        assert!(model.begin(Start::List { archive }, &mut factory));
        let first_job = model.job_id().unwrap();
        wait(&mut model);
        assert!(model.can_retry_expanded());
        assert!(model.retry_expanded(&mut factory));
        let job = model.job_id().unwrap();
        assert_ne!(first_job, job);
        let deadline = Instant::now() + Duration::from_secs(10);
        while model.resource.is_none() {
            model.poll();
            assert!(model.error.is_none(), "{:?}", model.error);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let q = model.resource.clone().unwrap();
        assert_eq!(
            q.scope,
            ResourceScope {
                output: true,
                entries: true,
                ratio: false
            }
        );
        assert_eq!(q.usage.actual_bytes, None);
        assert!(q.usage.available_bytes.is_some());
        assert!(model.entries.is_empty());
        assert!(!model.resource_reply(first_job, q.id, true));
        if mode == 2 {
            model.cancel();
        } else {
            assert!(model.resource_reply(job, q.id, mode == 0));
        }
        wait(&mut model);
        assert!(model.confirmed, "{:?}", model.error);
        assert_eq!(
            model.terminal.as_ref().unwrap().outcome,
            if mode == 0 {
                Outcome::Completed
            } else {
                Outcome::Cancelled
            }
        );
        assert_eq!(model.list_id.is_some(), mode == 0);
        assert!(model.resource.is_none());
    }
}

#[test]
fn approved_extraction_reports_implicit_parents_separately_from_entries() {
    let scratch = Scratch::new();
    let archive = scratch.path("implicit.zip");
    let generated = std::process::Command::new("python3")
        .arg("-c")
        .arg("import sys, zipfile; z = zipfile.ZipFile(sys.argv[1], 'w'); [z.writestr(p, b'') for p in ['a/b/one', 'a/b/two', 'c/d/three']]; z.close()")
        .arg(&archive)
        .output()
        .unwrap();
    assert!(generated.status.success());
    let mut factory = ProcessFactory::new(binary("zipelo-archive-worker"));
    let mut model = Model::default();
    assert!(model.begin(Start::List { archive }, &mut factory));
    wait(&mut model);
    assert!(model.confirmed, "{:?}", model.error);
    assert_eq!(model.entries.len(), 3);
    model.entries[0].bytes = Some(Limits::default().output + 1);
    let destination = make_directory(&scratch.path("output"));
    assert!(model.extract(destination.clone(), false, None, &mut factory));
    let deadline = Instant::now() + Duration::from_secs(10);
    while model.resource.is_none() {
        model.poll();
        assert!(model.error.is_none(), "{:?}", model.error);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let question = model.resource.clone().unwrap();
    assert!(question.scope.output && question.scope.entries);
    assert!(model.outputs.is_empty());
    assert!(model.resource_reply(model.job_id().unwrap(), question.id, true));
    wait(&mut model);
    assert!(model.confirmed, "{:?}", model.error);
    assert_eq!(model.terminal.as_ref().unwrap().outcome, Outcome::Completed);
    assert_eq!(model.terminal.as_ref().unwrap().created_count, 7);
    assert_eq!(model.outputs.len(), 7);
    for name in ["a", "a/b", "c", "c/d"] {
        assert!(destination.join(name).is_dir());
        assert!(
            model
                .outputs
                .iter()
                .any(|(p, d)| p == name && *d == Disposition::Created)
        );
    }
    for name in ["a/b/one", "a/b/two", "c/d/three"] {
        assert_eq!(fs::read(destination.join(name)).unwrap(), b"");
        assert!(
            model
                .outputs
                .iter()
                .any(|(p, d)| p == name && *d == Disposition::Created)
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn approved_password_wait_is_not_killed_by_app_deadline() {
    const MODE: &str = "ZIPELO_SUPERVISOR_CLOCK_CASE";
    if let Ok(mode) = std::env::var(MODE) {
        let approved = mode != "unapproved";
        let scratch = Scratch::new();
        let output = scratch.path("output.zip");
        let mut factory = ProcessFactory::new(binary("zipelo-archive-worker"));
        let mut connection = factory.connect(1).unwrap();
        let start = Start::CreateZip {
            sources: vec![(scratch.path("source.txt"), "source.txt".into())],
            output_file: output.clone(),
            method: Method::Store,
            aes256: true,
        };
        fs::write(scratch.path("source.txt"), b"source").unwrap();
        connection
            .send(Request {
                job_id: 1,
                sequence: 0,
                command: if approved {
                    Command::StartExpanded(start)
                } else {
                    Command::Start(start)
                },
            })
            .unwrap();
        let started = Instant::now();
        let mut sequence = 1;
        let mut granted = false;
        let mut password = false;
        let mut cancelled = false;
        let mut terminal = None;
        let mut fault = None;
        loop {
            if let Some(event) = connection.poll() {
                match event {
                    HostEvent::Frame {
                        event: Event::ResourceRequired(q),
                        ..
                    } => {
                        assert!(approved);
                        connection
                            .send(Request {
                                job_id: 1,
                                sequence,
                                command: Command::ResourceReply {
                                    challenge_id: q.id,
                                    scope: q.scope,
                                    allow: true,
                                },
                            })
                            .unwrap();
                        sequence += 1;
                        granted = true;
                    }
                    HostEvent::Frame {
                        event: Event::PasswordRequired { .. },
                        ..
                    } => password = true,
                    HostEvent::Frame {
                        event: Event::Terminal(value),
                        ..
                    } => terminal = Some(value),
                    HostEvent::Fault(message) => fault = Some(message),
                    HostEvent::Exited { success } => {
                        assert!(password);
                        assert_eq!(granted, approved);
                        assert!(!output.exists());
                        if approved {
                            assert!(success);
                            assert!(
                                fault.is_none(),
                                "승인 뒤 암호 대기를 강제 종료함: {fault:?}"
                            );
                            assert_eq!(terminal.unwrap().outcome, Outcome::Cancelled);
                        } else {
                            assert!(!success);
                            assert!(fault.unwrap().contains("제한 시간"));
                        }
                        return;
                    }
                    _ => {}
                }
            }
            if approved && started.elapsed() > Duration::from_secs(4) && !cancelled {
                connection
                    .send(Request {
                        job_id: 1,
                        sequence,
                        command: Command::Cancel,
                    })
                    .unwrap();
                cancelled = true;
            }
            assert!(started.elapsed() < Duration::from_secs(8));
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let scratch = Scratch::new();
    let clock = scratch.path("supervisor-clock.so");
    assert!(
        std::process::Command::new("cc")
            .args(["-shared", "-fPIC", "-pthread"])
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/rust-app-real-worker/supervisor_clock.c")
            )
            .args(["-o"])
            .arg(&clock)
            .arg("-ldl")
            .status()
            .unwrap()
            .success()
    );
    for mode in ["control", "approved", "unapproved"] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "guarded_flows::approved_password_wait_is_not_killed_by_app_deadline",
                "--nocapture",
            ])
            .env(MODE, mode);
        if mode == "control" {
            command.env_remove("LD_PRELOAD");
        } else {
            command.env("LD_PRELOAD", &clock);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{mode}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
