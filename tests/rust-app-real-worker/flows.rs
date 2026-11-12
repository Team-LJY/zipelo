use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use zipelo_ui_memory_app::{archive_process::ProcessFactory, archive_ui::*};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/rust-app-real-worker/runs");
        fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        Self(make_directory(&path))
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn make_directory(path: &Path) -> PathBuf {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).unwrap();
    path.canonicalize().unwrap()
}
fn binary(name: &str) -> PathBuf {
    let current = std::env::current_exe().unwrap();
    let path = current
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "실제 작업자와 시험 probe를 먼저 빌드해야 함: {}",
        path.display()
    );
    path
}
#[derive(Default)]
struct Observation {
    jobs: Vec<Vec<&'static str>>,
    starts: Vec<Start>,
}
struct ObservedFactory {
    process: ProcessFactory,
    observed: Rc<RefCell<Observation>>,
}
struct ObservedConnection {
    connection: Box<dyn Connection>,
    index: usize,
    observed: Rc<RefCell<Observation>>,
}
impl Factory for ObservedFactory {
    fn connect(&mut self, job: u64) -> Result<Box<dyn Connection>, String> {
        let connection = self.process.connect(job)?;
        let index = self.observed.borrow().jobs.len();
        self.observed.borrow_mut().jobs.push(Vec::new());
        Ok(Box::new(ObservedConnection {
            connection,
            index,
            observed: self.observed.clone(),
        }))
    }
}
impl Connection for ObservedConnection {
    fn send(&mut self, request: Request) -> Result<(), String> {
        if let Command::Start(start) = &request.command {
            self.observed.borrow_mut().starts.push(start.clone());
        }
        self.connection.send(request)
    }
    fn poll(&mut self) -> Option<HostEvent> {
        let event = self.connection.poll()?;
        let label = match &event {
            HostEvent::StdoutEof => "stdout",
            HostEvent::StderrEof => "stderr",
            HostEvent::Exited { .. } => "wait",
            HostEvent::CleanupComplete => "cleanup",
            HostEvent::Fault(_) => "fault",
            HostEvent::Frame {
                event: Event::Terminal(_),
                ..
            } => "terminal",
            _ => "frame",
        };
        self.observed.borrow_mut().jobs[self.index].push(label);
        Some(event)
    }
}
impl Drop for ObservedConnection {
    fn drop(&mut self) {
        self.observed.borrow_mut().jobs[self.index].push("drop");
    }
}
fn observed_factory() -> (ObservedFactory, Rc<RefCell<Observation>>) {
    let observed = Rc::new(RefCell::new(Observation::default()));
    (
        ObservedFactory {
            process: ProcessFactory::new(binary("zipelo-archive-worker")),
            observed: observed.clone(),
        },
        observed,
    )
}
fn assert_reaped(observed: &Rc<RefCell<Observation>>) {
    for events in &observed.borrow().jobs {
        for label in ["terminal", "stdout", "stderr", "cleanup", "wait", "drop"] {
            assert_eq!(
                events.iter().filter(|value| **value == label).count(),
                1,
                "{events:?}"
            );
        }
        assert!(!events.contains(&"fault"));
        assert_eq!(&events[events.len() - 2..], &["wait", "drop"]);
    }
}
fn wait(model: &mut Model) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while model.busy() {
        model.poll();
        assert!(
            Instant::now() < deadline,
            "작업자 종료 시간 초과: {}",
            model.status()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn completed(model: &Model) {
    assert!(model.confirmed, "{}", model.status());
    assert_eq!(
        model.terminal.as_ref().unwrap().outcome,
        Outcome::Completed,
        "terminal: {:?}",
        model.terminal
    );
}
#[cfg(feature = "ui")]
#[path = "../rust-app-ui/dialogs.rs"]
mod dialogs;
#[cfg(feature = "ui")]
mod screen {
    use super::dialogs::TestDialogs;
    use super::*;
    use eframe::egui::{self, epaint::Shape};
    use zipelo_ui_memory_app::archive_screen::ArchiveApp;
    struct Screen {
        context: egui::Context,
        app: ArchiveApp,
        dialogs: TestDialogs,
    }
    impl Screen {
        fn new(factory: ObservedFactory) -> Self {
            let dialogs = TestDialogs::default();
            let mut app = ArchiveApp::new(Box::new(factory), None, false);
            app.set_dialogs(Box::new(dialogs.clone()));
            Self {
                context: egui::Context::default(),
                app,
                dialogs,
            }
        }
        fn render(&mut self, events: Vec<egui::Event>) -> Vec<(String, egui::Pos2)> {
            fn text(shape: &Shape, output: &mut Vec<(String, egui::Pos2)>) {
                match shape {
                    Shape::Text(value) => output.push((
                        value.galley.job.text.clone(),
                        value.pos + value.galley.rect.size() / 2.0,
                    )),
                    Shape::Vec(values) => {
                        for value in values {
                            text(value, output);
                        }
                    }
                    _ => {}
                }
            }
            let mut frame = self.context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1060.0, 720.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| self.app.show(ui),
            );
            frame.textures_delta.clear();
            let mut output = Vec::new();
            for shape in frame.shapes {
                text(&shape.shape, &mut output);
            }
            output
        }
        fn locate(&mut self, label: &str) -> egui::Pos2 {
            self.render(vec![]);
            self.render(vec![])
                .into_iter()
                .filter(|(text, _)| text == label)
                .max_by(|a, b| a.1.y.total_cmp(&b.1.y))
                .unwrap_or_else(|| panic!("화면 항목 누락: {label}"))
                .1
        }
        fn click_at(&mut self, point: egui::Pos2) {
            self.render(vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ]);
            self.render(vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }]);
        }
        fn click(&mut self, label: &str) {
            let point = self.locate(label);
            self.click_at(point);
        }
        fn set_text(&mut self, id: &str, value: &str) {
            if id == "zip-name" {
                self.dialogs.0.borrow_mut().save_name = Some(value.to_owned());
                return;
            }
            self.render(vec![]);
            let point = self
                .context
                .read_response(egui::Id::unique(id))
                .unwrap()
                .rect
                .center();
            self.click_at(point);
            self.context
                .memory_mut(|memory| memory.request_focus(egui::Id::unique(id)));
            self.render(vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                egui::Event::Text(value.into()),
            ]);
        }
        fn escape(&mut self) {
            self.render(vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
        }
        fn settle(&mut self) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while self.app.model.busy() {
                self.render(vec![]);
                assert!(Instant::now() < deadline, "{}", self.app.model.status());
                std::thread::sleep(Duration::from_millis(2));
            }
            completed(&self.app.model);
        }
        fn open(&mut self, archive: &Path) {
            self.app.open_path(archive.to_path_buf());
            assert!(self.app.model.busy());
            self.settle();
        }
        fn extract(&mut self, destination: &Path) {
            self.app.choose_destination(destination.to_path_buf());
            self.dialogs
                .0
                .borrow_mut()
                .answers
                .push_back(Some(vec![destination.to_path_buf()]));
            self.click("압축해제");
            assert!(self.app.model.busy());
            self.settle();
            assert_eq!(
                self.app.model.terminal.as_ref().unwrap().publication,
                Publication::Complete
            );
        }
    }
    include!("password_flows.rs");
    include!("selection_flows.rs");
    include!("native_dialog_flows.rs");

    fn archive_flow(name: &str, format: Format) {
        let scratch = Scratch::new();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/rust-app-real-worker/fixtures")
            .join(name);
        let archive = scratch.path(name);
        fs::copy(fixture, &archive).unwrap();
        let original = fs::read(&archive).unwrap();
        let output = make_directory(&scratch.path("output"));
        let (factory, observed) = observed_factory();
        let mut screen = Screen::new(factory);
        screen.open(&archive);
        assert_eq!(screen.app.model.list_id.as_ref().unwrap().format, format);
        assert_eq!(screen.app.model.entries[0].name, "content.txt");
        screen.extract(&output);
        assert_eq!(
            fs::read(output.join("content.txt")).unwrap(),
            b"Zipelo real worker flow\n"
        );
        assert_eq!(fs::read(archive).unwrap(), original);
        assert_eq!(observed.borrow().jobs.len(), 2);
        assert_reaped(&observed);
    }
    #[test]
    fn seven_zip_open_and_extract_via_actual_widgets_and_worker() {
        for name in ["lzma.7z", "lzma2.7z"] {
            let scratch = Scratch::new();
            let archive = scratch.path(name);
            fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/rust-archive-worker/fixtures/7z")
                    .join(name),
                &archive,
            )
            .unwrap();
            let original = fs::read(&archive).unwrap();
            let output = make_directory(&scratch.path("output"));
            let (factory, observed) = observed_factory();
            let mut screen = Screen::new(factory);
            screen.open(&archive);
            assert_eq!(
                screen.app.model.list_id.as_ref().unwrap().format,
                Format::SevenZip
            );
            assert!(screen.app.model.can_extract());
            screen.extract(&output);
            assert_eq!(
                fs::read(output.join("folder/hello.txt")).unwrap(),
                b"Zipelo 7z interoperability\n".repeat(100)
            );
            assert_eq!(fs::read(archive).unwrap(), original);
            assert_eq!(observed.borrow().jobs.len(), 2);
            assert_reaped(&observed);
        }
    }
    #[test]
    fn zip_open_and_extract_via_actual_widgets_and_worker() {
        archive_flow("sample.zip", Format::Zip);
    }
    #[test]
    fn targz_open_and_extract_via_actual_widgets_and_worker() {
        archive_flow("sample.tar.gz", Format::TarGzip);
    }

    #[test]
    fn empty_directory_zip_extracts_successfully_with_existing_directory_reuse() {
        let scratch = Scratch::new();
        let source = make_directory(&scratch.path("empty-source"));
        fs::create_dir(source.join("child")).unwrap();
        let archive = scratch.path("empty-source.zip");
        let output = make_directory(&scratch.path("output"));
        let (factory, observed) = observed_factory();
        let mut screen = Screen::new(factory);
        screen.app.add_sources(vec![source]);
        screen.render(vec![]);
        screen.click("압축하기");
        screen.settle();
        assert!(archive.is_file());
        screen.open(&archive);
        assert!(
            screen
                .app
                .model
                .entries
                .iter()
                .all(|entry| entry.kind == Kind::Directory)
        );
        for expected in [2, 0] {
            screen.extract(&output);
            assert!(output.join("empty-source/child").is_dir());
            assert_eq!(
                screen.app.model.terminal.as_ref().unwrap().created_count,
                expected
            );
            let shown = screen.render(vec![]);
            assert!(shown.iter().any(|(text, _)| text == "압축을 풀었습니다"));
            assert!(
                !shown
                    .iter()
                    .any(|(text, _)| text.contains("저장이 끝났는지"))
            );
            screen.click("확인");
        }
        assert_reaped(&observed);
    }

    #[test]
    fn create_zip_reopen_and_extract_via_actual_widgets_and_worker() {
        let scratch = Scratch::new();
        let source = scratch.path("one.txt");
        fs::write(&source, b"ZIP creation through real worker\n").unwrap();
        let archive = scratch.path("new.zip");
        let output = make_directory(&scratch.path("output"));
        let (factory, observed) = observed_factory();
        let mut screen = Screen::new(factory);
        screen.app.add_sources(vec![source.clone()]);
        screen.render(vec![]);
        screen.set_text("zip-name", archive.file_name().unwrap().to_str().unwrap());
        screen.click("압축하기");
        assert!(screen.app.model.busy());
        screen.settle();
        assert!(archive.is_file());
        screen.open(&archive);
        assert_eq!(screen.app.model.entries[0].name, "one.txt");
        screen.extract(&output);
        assert_eq!(
            fs::read(output.join("one.txt")).unwrap(),
            fs::read(source).unwrap()
        );
        assert_eq!(observed.borrow().jobs.len(), 3);
        assert_reaped(&observed);
    }
}
#[test]
fn missing_worker_is_an_error_without_a_fake_result() {
    let scratch = Scratch::new();
    let mut factory = ProcessFactory::new(scratch.path("not-installed"));
    let mut model = Model::default();
    assert!(!model.begin(
        Start::List {
            archive: scratch.path("sample.zip")
        },
        &mut factory
    ));
    assert!(!model.busy());
    assert!(model.error.is_some());
    assert!(!model.confirmed);
}
#[test]
fn broken_or_incomplete_worker_output_is_not_success() {
    for name in ["truncated", "wrong-action", "early-exit"] {
        let scratch = Scratch::new();
        let probe = scratch.path(&format!("{name}{}", std::env::consts::EXE_SUFFIX));
        fs::copy(binary("zipelo-real-worker-probe"), &probe).unwrap();
        let mut factory = ProcessFactory::new(probe);
        let mut model = Model::default();
        assert!(
            model.begin(
                Start::List {
                    archive: scratch.path("sample.zip")
                },
                &mut factory
            ),
            "{:?}",
            model.error
        );
        model.request_close();
        model.confirm_close();
        assert_eq!(model.close, Close::Waiting);
        wait(&mut model);
        assert!(!model.confirmed);
        assert!(model.error.is_some());
        assert_eq!(model.close, Close::Review);
        model.acknowledge_close();
        assert_eq!(model.close, Close::Allowed);
    }
}
#[cfg(target_os = "linux")]
#[test]
fn dropping_live_connection_kills_and_reaps_owned_child() {
    let scratch = Scratch::new();
    let probe = scratch.path("hang");
    fs::copy(binary("zipelo-real-worker-probe"), &probe).unwrap();
    let mut factory = ProcessFactory::new(probe);
    let mut model = Model::default();
    assert!(
        model.begin(
            Start::List {
                archive: scratch.path("sample.zip")
            },
            &mut factory
        ),
        "{:?}",
        model.error
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !scratch.path("probe.pid").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let pid = fs::read_to_string(scratch.path("probe.pid")).unwrap();
    drop(model);
    assert!(!Path::new("/proc").join(pid).exists());
}

#[test]
fn failed_start_serialization_reaps_the_real_worker_without_terminal() {
    let scratch = Scratch::new();
    let mut factory = ProcessFactory::new(binary("zipelo-archive-worker"));
    let mut model = Model::default();
    let archive = scratch.path(&"a".repeat(40000));
    assert!(!model.begin(Start::List { archive }, &mut factory));
    assert!(model.busy());
    model.request_close();
    model.confirm_close();
    assert_eq!(model.close, Close::Waiting);
    wait(&mut model);
    assert!(!model.confirmed);
    assert!(model.terminal.is_none());
    assert!(model.error.is_some());
    assert_eq!(model.close, Close::Review);
}

#[test]
fn late_cancel_after_child_reap_keeps_the_original_terminal_result() {
    let scratch = Scratch::new();
    let archive = scratch.path("sample.zip");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/rust-app-real-worker/fixtures/sample.zip"),
        &archive,
    )
    .unwrap();
    let mut factory = ProcessFactory::new(binary("zipelo-archive-worker"));
    let mut connection = factory.connect(1).unwrap();
    connection
        .send(Request {
            job_id: 1,
            sequence: 0,
            command: Command::Start(Start::List { archive }),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut terminal = false;
    loop {
        assert!(Instant::now() < deadline);
        match connection.poll() {
            Some(HostEvent::CleanupComplete) => break,
            Some(HostEvent::Frame {
                event: Event::Terminal(value),
                ..
            }) => {
                assert_eq!(value.outcome, Outcome::Completed);
                terminal = true;
            }
            Some(HostEvent::Fault(message)) => panic!("{message}"),
            _ => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    assert!(terminal);
    assert!(
        connection
            .send(Request {
                job_id: 1,
                sequence: 1,
                command: Command::Cancel
            })
            .is_ok()
    );
    loop {
        assert!(Instant::now() < deadline);
        match connection.poll() {
            Some(HostEvent::Exited { success }) => {
                assert!(success);
                break;
            }
            Some(HostEvent::Fault(message)) => panic!("{message}"),
            _ => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    assert!(connection.poll().is_none());
}

#[test]
fn cancel_racing_child_exit_preserves_completed_result_before_poll() {
    struct CompletedFactory(ProcessFactory);
    struct BufferedConnection {
        inner: Box<dyn Connection>,
        pending: std::collections::VecDeque<HostEvent>,
    }
    impl Factory for CompletedFactory {
        fn connect(&mut self, job_id: u64) -> Result<Box<dyn Connection>, String> {
            Ok(Box::new(BufferedConnection {
                inner: self.0.connect(job_id)?,
                pending: Default::default(),
            }))
        }
    }
    impl Connection for BufferedConnection {
        fn send(&mut self, request: Request) -> Result<(), String> {
            let starting = matches!(&request.command, Command::Start(_));
            self.inner.send(request)?;
            if starting {
                let deadline = Instant::now() + Duration::from_secs(20);
                loop {
                    assert!(Instant::now() < deadline, "작업자 종료 대기 시간 초과");
                    if let Some(event) = self.inner.poll() {
                        let exited = matches!(&event, HostEvent::Exited { .. });
                        self.pending.push_back(event);
                        if exited {
                            break;
                        }
                    } else {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            }
            Ok(())
        }
        fn poll(&mut self) -> Option<HostEvent> {
            self.pending.pop_front().or_else(|| self.inner.poll())
        }
    }
    let archive = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/rust-app-real-worker/fixtures/sample.zip")
        .canonicalize()
        .unwrap();
    for iteration in 0..100 {
        let mut factory = CompletedFactory(ProcessFactory::new(binary("zipelo-archive-worker")));
        let mut model = Model::default();
        assert!(
            model.begin(
                Start::List {
                    archive: archive.clone()
                },
                &mut factory
            ),
            "{:?}",
            model.error
        );
        assert!(model.terminal.is_none());
        model.cancel();
        wait(&mut model);
        assert!(model.confirmed, "iteration={iteration}: {:?}", model.error);
        assert_eq!(model.terminal.as_ref().unwrap().outcome, Outcome::Completed);
        assert!(model.error.is_none());
    }
}

#[test]
fn cancel_racing_exit_without_terminal_remains_unconfirmed() {
    let scratch = Scratch::new();
    let probe = scratch.path("early-exit");
    fs::copy(binary("zipelo-real-worker-probe"), &probe).unwrap();
    let mut factory = ProcessFactory::new(probe);
    let mut model = Model::default();
    assert!(
        model.begin(
            Start::List {
                archive: scratch.path("sample.zip")
            },
            &mut factory
        ),
        "{:?}",
        model.error
    );
    std::thread::sleep(Duration::from_millis(2));
    model.cancel();
    model.request_close();
    model.confirm_close();
    assert_eq!(model.close, Close::Waiting);
    wait(&mut model);
    assert!(!model.confirmed);
    assert!(model.terminal.is_none());
    assert!(model.error.is_some());
    assert_eq!(model.close, Close::Review);
}
