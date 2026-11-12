// 별도 진행 창(실제 즉시 viewport 콜백)에서 파일 선택 대기 중에 작업이 시작되지 않는지 확인함.

#[derive(Default)]
struct WorkChild {
    events: Vec<egui::Event>,
    labels: Vec<(String, egui::Pos2)>,
    rendered: usize,
}

std::thread_local! {
    static WORK_CHILD: std::cell::RefCell<WorkChild> = Default::default();
}

/// eframe처럼 별도 viewport 콜백을 같은 Context에서 실행하고, 그 viewport에만 입력을 넣음.
fn install_work_child_renderer() {
    // 같은 스레드에서 이어 도는 시험끼리 기록이 섞이지 않게 비움.
    WORK_CHILD.with(|child| *child.borrow_mut() = WorkChild::default());
    egui::Context::set_immediate_viewport_renderer(|ctx, mut viewport| {
        let events = WORK_CHILD.with(|child| std::mem::take(&mut child.borrow_mut().events));
        let mut raw = egui::RawInput {
            viewport_id: viewport.ids.this,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(480.0, 240.0),
            )),
            events,
            ..Default::default()
        };
        raw.viewports.insert(
            viewport.ids.this,
            egui::ViewportInfo {
                parent: Some(viewport.ids.parent),
                focused: Some(true),
                ..Default::default()
            },
        );
        let mut output = ctx.run_ui(
            raw,
            |ui| (viewport.viewport_ui_cb)(ui),
        );
        output.textures_delta.clear();
        WORK_CHILD.with(|child| {
            let mut child = child.borrow_mut();
            child.labels = labels(&output.shapes);
            child.rendered += 1;
        });
    });
}

fn work_child_label(label: &str) -> Option<egui::Pos2> {
    WORK_CHILD.with(|child| {
        child
            .borrow()
            .labels
            .iter()
            .rev()
            .find(|(text, _)| text == label)
            .map(|(_, pos)| *pos)
    })
}

fn work_child_click(ctx: &egui::Context, app: &mut ArchiveApp, point: egui::Pos2) {
    for pressed in [true, false] {
        WORK_CHILD.with(|child| {
            child.borrow_mut().events = vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
        });
        policy_render(ctx, app, vec![]);
    }
}

/// 열기 선택 창의 응답을 시험이 보낼 때까지 보류함.
#[derive(Clone, Default)]
struct HeldDialogs(
    std::rc::Rc<
        std::cell::RefCell<
            Vec<std::sync::mpsc::Sender<zipelo_ui_memory_app::archive_dialog::DialogAnswer>>,
        >,
    >,
);

impl zipelo_ui_memory_app::archive_dialog::Dialogs for HeldDialogs {
    fn choose(
        &mut self,
        _: zipelo_ui_memory_app::archive_dialog::DialogRequest,
    ) -> zipelo_ui_memory_app::archive_dialog::DialogAnswer {
        Ok(None)
    }

    fn start(
        &mut self,
        _: zipelo_ui_memory_app::archive_dialog::DialogRequest,
    ) -> zipelo_ui_memory_app::archive_dialog::PendingDialog {
        let (sender, receiver) = std::sync::mpsc::channel();
        self.0.borrow_mut().push(sender);
        zipelo_ui_memory_app::archive_dialog::PendingDialog::Waiting(receiver)
    }
}

#[test]
fn picker_wait_blocks_retry_in_separate_work_window() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    factory.state.borrow_mut().manual = true;
    assert!(app.begin_extract());
    {
        let mut worker = factory.state.borrow_mut();
        worker.frame(Event::Accepted { encryption: false });
        worker.finish(Outcome::Failed, Publication::None);
        for event in &mut worker.queue {
            if let HostEvent::Frame {
                event: Event::Terminal(t),
                ..
            } = event
            {
                t.error = Some(ErrorCode::Io);
            }
        }
    }
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    install_work_child_renderer();
    for _ in 0..10 {
        policy_render(&ctx, &mut app, vec![]);
    }
    assert!(
        WORK_CHILD.with(|child| child.borrow().rendered) > 0,
        "결과가 별도 viewport에 그려져야 함"
    );
    let retry = work_child_label("다시 시도").expect("별도 결과 창에 다시 시도 버튼이 있어야 함");

    let dialogs = HeldDialogs::default();
    app.set_dialogs(Box::new(dialogs.clone()));
    policy_click(&ctx, &mut app, "열기");
    assert_eq!(dialogs.0.borrow().len(), 1, "열기 선택 창이 응답을 기다려야 함");
    let before = factory.state.borrow().starts.len();
    work_child_click(&ctx, &mut app, retry);
    assert_eq!(
        factory.state.borrow().starts.len(),
        before,
        "파일 선택 창이 열린 동안 별도 결과 창에서 작업이 시작됨"
    );

    // 대조: 선택 창을 닫으면 같은 클릭으로 다시 시도가 시작됨.
    dialogs.0.borrow()[0].send(Ok(None)).unwrap();
    for _ in 0..3 {
        policy_render(&ctx, &mut app, vec![]);
    }
    let retry = work_child_label("다시 시도").expect("선택 창을 닫은 뒤에도 결과가 남아야 함");
    work_child_click(&ctx, &mut app, retry);
    assert_eq!(factory.state.borrow().starts.len(), before + 1);
}

#[test]
fn password_prompt_is_not_covered_by_separate_work_window() {
    let scratch = Folder::new();
    let factory = FakeFactory::manual();
    let mut app = ArchiveApp::new(Box::new(factory.clone()), None, false);
    assert!(app.model.begin(
        Start::CreateZip {
            sources: vec![(scratch.0.join("source"), "source".into())],
            output_file: scratch.0.join("output.zip"),
            method: Method::Deflate,
            aes256: true
        },
        &mut factory.clone()
    ));
    {
        let mut worker = factory.state.borrow_mut();
        worker.frame(Event::Accepted { encryption: true });
        worker.frame(Event::PasswordRequired {
            challenge_id: 17,
            attempt: 1,
            encrypt: true,
        });
    }
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    install_work_child_renderer();
    for _ in 0..12 {
        policy_render(&ctx, &mut app, vec![]);
    }
    assert!(app.model.challenge.is_some(), "암호 입력을 기다려야 함");
    // 작업자 사건을 처리하기 전 몇 프레임은 진행 창이 정상으로 그려짐.
    // 암호 입력 창이 뜬 뒤부터 진행 창이 그려지는지만 셈.
    WORK_CHILD.with(|child| child.borrow_mut().rendered = 0);
    for _ in 0..5 {
        policy_render(&ctx, &mut app, vec![]);
    }
    assert!(
        ctx.read_response(egui::Id::unique("archive-password-input"))
            .is_some(),
        "메인 창에 암호 입력란이 있어야 함"
    );
    // 항상 위에 뜨는 진행 창이 메인 창 가운데의 암호 입력란을 가리지 않도록
    // 암호를 기다리는 동안에는 진행 창을 그리지 않음.
    assert_eq!(
        WORK_CHILD.with(|child| child.borrow().rendered),
        0,
        "암호 입력 중에 별도 진행 창이 떠 있음"
    );

    // 암호 입력이 끝나 작업이 이어지면 진행 창이 다시 나타남.
    app.model.challenge = None;
    for _ in 0..3 {
        policy_render(&ctx, &mut app, vec![]);
    }
    assert!(app.model.busy());
    assert!(
        WORK_CHILD.with(|child| child.borrow().rendered) > 0,
        "암호 입력 뒤 진행 창이 다시 나타나야 함"
    );
}

#[test]
fn close_confirmation_is_not_covered_by_separate_work_window() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    factory.state.borrow_mut().manual = true;
    assert!(app.begin_extract());
    factory
        .state
        .borrow_mut()
        .frame(Event::Accepted { encryption: false });
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    install_work_child_renderer();
    for _ in 0..6 {
        policy_render(&ctx, &mut app, vec![]);
    }
    assert!(app.model.busy());
    assert!(
        WORK_CHILD.with(|child| child.borrow().rendered) > 0,
        "작업 중에는 진행 창이 떠 있어야 함"
    );

    // 작업 중 메인 창을 닫으면 메인 창 가운데에 종료 확인 창이 뜸.
    app.model.request_close();
    assert_eq!(app.model.close, Close::Confirm);
    policy_render(&ctx, &mut app, vec![]);
    WORK_CHILD.with(|child| child.borrow_mut().rendered = 0);
    let mut shown = Vec::new();
    for _ in 0..5 {
        shown = policy_render(&ctx, &mut app, vec![]);
    }
    for label in ["작업 중 닫기", "계속 작업", "취소하고 닫기"] {
        assert!(shown.iter().any(|(text, _)| text == label), "{label}");
    }
    // 항상 위에 뜨는 진행 창이 확인 창을 가리지 않도록 그동안 진행 창을 그리지 않음.
    assert_eq!(
        WORK_CHILD.with(|child| child.borrow().rendered),
        0,
        "종료 확인 중에 별도 진행 창이 떠 있음"
    );

    // 계속 작업을 고르면 진행 창이 다시 나타남.
    app.model.keep_open();
    for _ in 0..3 {
        policy_render(&ctx, &mut app, vec![]);
    }
    assert!(app.model.busy());
    assert!(
        WORK_CHILD.with(|child| child.borrow().rendered) > 0,
        "계속 작업을 고르면 진행 창이 다시 나타나야 함"
    );

    // 취소하고 닫기 뒤 정리를 기다리는 동안("종료 확인 중")도 가리지 않음.
    app.model.request_close();
    app.model.confirm_close();
    assert_eq!(app.model.close, Close::Waiting);
    policy_render(&ctx, &mut app, vec![]);
    WORK_CHILD.with(|child| child.borrow_mut().rendered = 0);
    for _ in 0..5 {
        shown = policy_render(&ctx, &mut app, vec![]);
    }
    assert!(shown.iter().any(|(text, _)| text == "종료 확인 중"));
    assert_eq!(
        WORK_CHILD.with(|child| child.borrow().rendered),
        0,
        "종료 확인 중에 별도 진행 창이 떠 있음"
    );

    // 정리가 끝나 결과 확인 후 닫기가 뜰 때도 결과 창이 가리지 않음.
    factory
        .state
        .borrow_mut()
        .finish(Outcome::Cancelled, Publication::None);
    for _ in 0..12 {
        shown = policy_render(&ctx, &mut app, vec![]);
    }
    assert_eq!(app.model.close, Close::Review);
    assert!(shown.iter().any(|(text, _)| text == "결과 확인 후 닫기"));
    assert_eq!(
        WORK_CHILD.with(|child| child.borrow().rendered),
        0,
        "결과 확인 후 닫기에 별도 결과 창이 떠 있음"
    );
}
