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
