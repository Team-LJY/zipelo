// ZIP 만들기 창: 폴더 선택으로 시작하고, 메인 목록과 같은 모양으로 폴더 안을 보여 주며,
// 암호는 창 안에서 받아 작업자 요청에 자동으로 넘기는지 확인함.

fn create_double_click(ctx: &egui::Context, app: &mut ArchiveApp, label: &str) {
    policy_render(ctx, app, vec![]);
    let point = policy_render(ctx, app, vec![])
        .iter()
        .rev()
        .find(|(text, _)| text == label)
        .unwrap_or_else(|| panic!("항목 없음: {label}"))
        .1;
    for _ in 0..2 {
        for pressed in [true, false] {
            policy_render(
                ctx,
                app,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
}

/// photos/a.txt, photos/trip/sea.jpg 폴더를 만들고 압축하기의 폴더 선택 응답으로 씀.
fn create_from_folder(scratch: &Folder, factory: &FakeFactory) -> (ArchiveApp, egui::Context) {
    let folder = scratch.0.join("photos");
    fs::create_dir_all(folder.join("trip")).unwrap();
    fs::write(folder.join("a.txt"), b"abc").unwrap();
    fs::write(folder.join("trip/sea.jpg"), b"sea").unwrap();
    let mut app = loaded(scratch, factory);
    let dialogs = TestDialogs::default();
    dialogs.0.borrow_mut().answers.push_back(Some(vec![folder]));
    app.set_dialogs(Box::new(dialogs));
    let ctx = egui::Context::default();
    policy_click(&ctx, &mut app, "압축하기");
    (app, ctx)
}

#[test]
fn create_window_lists_chosen_folder_and_browses_into_subfolders() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let (mut app, ctx) = create_from_folder(&scratch, &factory);
    let shown = policy_render(&ctx, &mut app, vec![]);
    for expected in [
        "ZIP 만들기",
        "/photos",
        "trip",
        "a.txt",
        "암호로 보호하기",
        "취소",
    ] {
        assert!(
            shown.iter().any(|(text, _)| text == expected),
            "{expected}: {shown:?}"
        );
    }
    // 끌어 놓기 영역, 파일/폴더 추가 버튼, 암호화 제목은 없음.
    for removed in [
        "파일이나 폴더를 여기에 끌어 놓으세요",
        "파일 추가",
        "폴더 추가",
        "빼기",
        "암호화",
    ] {
        assert!(!shown.iter().any(|(text, _)| text == removed), "{removed}");
    }
    // 폴더가 파일보다 먼저 나옴 (메인 목록과 같은 순서).
    let y = |label: &str| shown.iter().find(|(text, _)| text == label).unwrap().1.y;
    assert!(y("trip") < y("a.txt"));

    // 한 번 누르면 들어가지 않음.
    policy_click(&ctx, &mut app, "trip");
    for _ in 0..30 {
        policy_render(&ctx, &mut app, vec![]);
    }
    let shown = policy_render(&ctx, &mut app, vec![]);
    assert!(shown.iter().any(|(text, _)| text == "a.txt"));
    assert!(!shown.iter().any(|(text, _)| text == "/photos/trip"));

    create_double_click(&ctx, &mut app, "trip");
    let shown = policy_render(&ctx, &mut app, vec![]);
    assert!(shown.iter().any(|(text, _)| text == "/photos/trip"));
    assert!(shown.iter().any(|(text, _)| text == "sea.jpg"));
    assert!(!shown.iter().any(|(text, _)| text == "a.txt"));

    // ZIP 안에는 고른 폴더가 최상위 폴더로 들어감.
    policy_click(&ctx, &mut app, "압축하기");
    match factory.state.borrow().starts.last() {
        Some(Start::CreateZip {
            sources, aes256, ..
        }) => {
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].1, "photos");
            assert!(!aes256);
        }
        other => panic!("ZIP 만들기 요청이 필요함: {other:?}"),
    }
}

#[test]
fn create_password_rows_follow_checkbox_and_answer_worker_without_prompt() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let (mut app, ctx) = create_from_folder(&scratch, &factory);
    let shown = policy_render(&ctx, &mut app, vec![]);
    assert!(!shown.iter().any(|(text, _)| text == "암호 확인"));

    policy_click(&ctx, &mut app, "암호로 보호하기");
    let shown = policy_render(&ctx, &mut app, vec![]);
    assert!(shown.iter().any(|(text, _)| text == "암호"));
    assert!(shown.iter().any(|(text, _)| text == "암호 확인"));

    // 다른 두 암호는 알리고 시작하지 않음.
    text(&ctx, &mut app, "zip-password-input", "first-secret");
    text(&ctx, &mut app, "zip-password-confirm", "other-secret");
    let shown = policy_render(&ctx, &mut app, vec![]);
    assert!(
        shown
            .iter()
            .any(|(text, _)| text == "두 암호가 같지 않습니다")
    );
    policy_click(&ctx, &mut app, "압축하기");
    assert_eq!(factory.state.borrow().starts.len(), 1);

    // 체크를 해제하면 암호 칸이 사라지고 입력한 암호도 남기지 않음.
    policy_click(&ctx, &mut app, "암호로 보호하기");
    let shown = policy_render(&ctx, &mut app, vec![]);
    assert!(!shown.iter().any(|(text, _)| text == "암호 확인"));
    policy_click(&ctx, &mut app, "암호로 보호하기");
    policy_click(&ctx, &mut app, "압축하기");
    assert_eq!(factory.state.borrow().starts.len(), 1, "지운 암호로 시작함");

    text(&ctx, &mut app, "zip-password-input", "fixture-secret");
    text(&ctx, &mut app, "zip-password-confirm", "fixture-secret");
    factory.state.borrow_mut().manual = true;
    policy_click(&ctx, &mut app, "압축하기");
    assert!(matches!(
        factory.state.borrow().starts.last(),
        Some(Start::CreateZip { aes256: true, .. })
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
    // 작업자가 암호를 요청하면 별도 암호 창 없이 ZIP 만들기 창의 암호를 넘김.
    for _ in 0..12 {
        let shown = policy_render(&ctx, &mut app, vec![]);
        assert!(!shown.iter().any(|(text, _)| text == "ZIP 암호 설정"));
    }
    assert!(app.model.challenge.is_none());
    assert!(factory.state.borrow().password_seen);
    assert_eq!(factory.state.borrow().password, b"fixture-secret");
}

#[test]
fn retry_of_encrypted_creation_reopens_create_window_for_password() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let (mut app, ctx) = create_from_folder(&scratch, &factory);
    policy_click(&ctx, &mut app, "암호로 보호하기");
    text(&ctx, &mut app, "zip-password-input", "fixture-secret");
    text(&ctx, &mut app, "zip-password-confirm", "fixture-secret");
    factory.state.borrow_mut().manual = true;
    policy_click(&ctx, &mut app, "압축하기");
    let starts = factory.state.borrow().starts.len();
    {
        let mut worker = factory.state.borrow_mut();
        worker.frame(Event::Accepted { encryption: true });
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
    for _ in 0..12 {
        policy_render(&ctx, &mut app, vec![]);
    }
    policy_click(&ctx, &mut app, "다시 시도");
    let shown = policy_render(&ctx, &mut app, vec![]);
    // 암호는 보관하지 않으므로 ZIP 만들기 창을 다시 열어 암호를 받음.
    assert!(shown.iter().any(|(text, _)| text == "ZIP 만들기"));
    assert!(shown.iter().any(|(text, _)| text == "암호 확인"));
    assert!(!shown.iter().any(|(text, _)| text == "ZIP 암호 설정"));
    assert_eq!(factory.state.borrow().starts.len(), starts);
}

/// 주어진 세로 위치를 덮는 목록 행 배경색(선택/마우스 올림 색 중)을 찾음.
fn row_fill_at(output: &egui::FullOutput, y: f32) -> Option<egui::Color32> {
    use zipelo_ui_memory_app::archive_theme::{ROW_HOVER, ROW_SELECTED, ROW_SELECTED_HOVER};
    fn walk(shape: &egui::epaint::Shape, y: f32, found: &mut Option<egui::Color32>) {
        match shape {
            egui::epaint::Shape::Rect(rect)
                if (rect.fill == ROW_SELECTED
                    || rect.fill == ROW_HOVER
                    || rect.fill == ROW_SELECTED_HOVER)
                    && rect.rect.top() <= y
                    && y <= rect.rect.bottom() =>
            {
                *found = Some(rect.fill);
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, y, found);
                }
            }
            _ => {}
        }
    }
    let mut found = None;
    for shape in &output.shapes {
        walk(&shape.shape, y, &mut found);
    }
    found
}

fn label_y(ctx: &egui::Context, app: &mut ArchiveApp, label: &str) -> f32 {
    policy_render(ctx, app, vec![])
        .iter()
        .rev()
        .find(|(text, _)| text == label)
        .unwrap_or_else(|| panic!("항목 없음: {label}"))
        .1
        .y
}

fn hover(ctx: &egui::Context, app: &mut ArchiveApp, label: &str) -> egui::FullOutput {
    let point = policy_render(ctx, app, vec![])
        .iter()
        .rev()
        .find(|(text, _)| text == label)
        .unwrap()
        .1;
    frame_output(ctx, app, vec![egui::Event::PointerMoved(point)]);
    frame_output(ctx, app, vec![egui::Event::PointerMoved(point)])
}

#[test]
fn create_list_click_selects_and_hover_uses_lighter_blue() {
    use zipelo_ui_memory_app::archive_theme::{ROW_HOVER, ROW_SELECTED, ROW_SELECTED_HOVER};
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let (mut app, ctx) = create_from_folder(&scratch, &factory);
    let a = label_y(&ctx, &mut app, "a.txt");
    let trip = label_y(&ctx, &mut app, "trip");
    // 한 번 누르면 반투명 파란 배경으로 선택함.
    policy_click(&ctx, &mut app, "a.txt");
    // 다른 행에 마우스를 올리면 선택 색보다 옅은 파란색.
    let output = hover(&ctx, &mut app, "trip");
    assert_eq!(row_fill_at(&output, a), Some(ROW_SELECTED));
    assert_eq!(row_fill_at(&output, trip), Some(ROW_HOVER));
    // 선택한 행에 마우스를 올리면 선택 색보다 진해짐 (덜 투명함).
    let output = hover(&ctx, &mut app, "a.txt");
    assert_eq!(row_fill_at(&output, a), Some(ROW_SELECTED_HOVER));
    assert_eq!(row_fill_at(&output, trip), None);
    assert!(ROW_HOVER.a() < ROW_SELECTED.a());
    assert!(ROW_SELECTED.a() < ROW_SELECTED_HOVER.a());
}

#[test]
fn main_list_hover_uses_lighter_blue_and_selected_row_gets_stronger() {
    use zipelo_ui_memory_app::archive_theme::{ROW_HOVER, ROW_SELECTED, ROW_SELECTED_HOVER};
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    let ctx = egui::Context::default();
    let name = "읽어주세요.txt";
    let y = label_y(&ctx, &mut app, name);
    let output = hover(&ctx, &mut app, name);
    assert_eq!(row_fill_at(&output, y), Some(ROW_HOVER));
    policy_click(&ctx, &mut app, name);
    let output = hover(&ctx, &mut app, name);
    assert_eq!(row_fill_at(&output, y), Some(ROW_SELECTED_HOVER));
    // 마우스를 치우면 선택 색만 남음 (마우스 상태는 다음 프레임에 반영됨).
    frame_output(&ctx, &mut app, vec![egui::Event::PointerGone]);
    let output = frame_output(&ctx, &mut app, vec![]);
    assert_eq!(row_fill_at(&output, y), Some(ROW_SELECTED));
    // 선택 배경은 오른쪽 여백 없이 목록 너비(창 1060px)를 꽉 채움.
    let right = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect)
                if rect.fill == ROW_SELECTED && rect.rect.top() <= y && y <= rect.rect.bottom() =>
            {
                Some(rect.rect.right())
            }
            _ => None,
        })
        .fold(0.0f32, f32::max);
    assert!(right >= 1059.0, "선택 배경 오른쪽 끝: {right}");
}

fn cursor_over(ctx: &egui::Context, app: &mut ArchiveApp, point: egui::Pos2) -> egui::CursorIcon {
    frame_output(ctx, app, vec![egui::Event::PointerMoved(point)]);
    frame_output(ctx, app, vec![egui::Event::PointerMoved(point)])
        .platform_output
        .cursor_icon
}

#[test]
fn clickable_buttons_and_rows_show_pointing_hand_cursor() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    let ctx = egui::Context::default();
    let at = |ctx: &egui::Context, app: &mut ArchiveApp, label: &str| {
        policy_render(ctx, app, vec![])
            .iter()
            .rev()
            .find(|(text, _)| text == label)
            .unwrap_or_else(|| panic!("항목 없음: {label}"))
            .1
    };
    for label in ["열기", "압축하기", "읽어주세요.txt"] {
        let point = at(&ctx, &mut app, label);
        assert_eq!(
            cursor_over(&ctx, &mut app, point),
            egui::CursorIcon::PointingHand,
            "{label}"
        );
    }
    // 목록의 빈 곳(끌어서 선택하는 영역)과 검색 입력란은 손 모양이 아님.
    let row = at(&ctx, &mut app, "읽어주세요.txt");
    let empty = row + egui::vec2(0.0, 200.0);
    assert_eq!(
        cursor_over(&ctx, &mut app, empty),
        egui::CursorIcon::Default
    );
    let search = at(&ctx, &mut app, "파일 검색");
    assert_ne!(
        cursor_over(&ctx, &mut app, search),
        egui::CursorIcon::PointingHand
    );

    // ZIP 만들기 창의 체크박스와 항목도 손 모양.
    let create_scratch = Folder::new();
    let (mut app, ctx) = create_from_folder(&create_scratch, &FakeFactory::default());
    for label in ["암호로 보호하기", "a.txt", "취소"] {
        let point = at(&ctx, &mut app, label);
        assert_eq!(
            cursor_over(&ctx, &mut app, point),
            egui::CursorIcon::PointingHand,
            "{label}"
        );
    }
}

/// 별도 창으로 뜨는 ZIP 만들기 창을 photos 폴더로 엶.
fn create_window_from_folder(
    scratch: &Folder,
    factory: &FakeFactory,
) -> (ArchiveApp, egui::Context) {
    let folder = scratch.0.join("photos");
    fs::create_dir_all(folder.join("trip")).unwrap();
    fs::write(folder.join("a.txt"), b"abc").unwrap();
    let mut app = loaded(scratch, factory);
    let dialogs = TestDialogs::default();
    dialogs.0.borrow_mut().answers.push_back(Some(vec![folder]));
    app.set_dialogs(Box::new(dialogs));
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    install_work_child_renderer();
    policy_click(&ctx, &mut app, "압축하기");
    for _ in 0..3 {
        policy_render(&ctx, &mut app, vec![]);
    }
    assert!(
        work_child_label("암호로 보호하기").is_some(),
        "ZIP 만들기 창이 별도 창으로 떠야 함"
    );
    (app, ctx)
}

fn create_child_render(ctx: &egui::Context, app: &mut ArchiveApp) {
    WORK_CHILD.with(|child| child.borrow_mut().labels.clear());
    for _ in 0..3 {
        policy_render(ctx, app, vec![]);
    }
}

#[test]
fn closing_create_window_drops_pending_save_answer() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let (mut app, ctx) = create_window_from_folder(&scratch, &factory);
    let before = factory.state.borrow().starts.len();

    // ZIP 만들기 창의 압축하기로 저장 위치 선택 창을 띄워 둠.
    let dialogs = HeldDialogs::default();
    app.set_dialogs(Box::new(dialogs.clone()));
    let compress = work_child_label("압축하기").expect("ZIP 만들기 창의 압축하기");
    work_child_click(&ctx, &mut app, compress);
    assert_eq!(
        dialogs.0.borrow().len(),
        1,
        "저장 위치 선택 창이 응답을 기다려야 함"
    );

    // 저장 위치 창을 둔 채 ZIP 만들기 창을 X로 닫음.
    WORK_CHILD.with(|child| child.borrow_mut().close = true);
    // 닫기 요청을 받은 프레임은 내용을 그린 뒤 닫으므로 그다음 프레임부터 봄.
    policy_render(&ctx, &mut app, vec![]);
    create_child_render(&ctx, &mut app);
    assert!(
        work_child_label("암호로 보호하기").is_none(),
        "ZIP 만들기 창이 닫혀야 함"
    );

    // 나중에 저장 위치 창에서 저장을 눌러도 닫힌 창의 압축을 시작하지 않음.
    dialogs.0.borrow()[0]
        .send(Ok(Some(vec![scratch.0.join("photos.zip")])))
        .unwrap();
    for _ in 0..5 {
        policy_render(&ctx, &mut app, vec![]);
    }
    assert_eq!(
        factory.state.borrow().starts.len(),
        before,
        "닫은 ZIP 만들기 창의 저장 응답으로 압축이 시작됨"
    );
}

#[test]
fn cancelling_second_folder_choice_keeps_create_window_contents() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let (mut app, ctx) = create_window_from_folder(&scratch, &factory);
    let check = work_child_label("암호로 보호하기").unwrap();
    work_child_click(&ctx, &mut app, check);
    create_child_render(&ctx, &mut app);
    assert!(
        work_child_label("암호 확인").is_some(),
        "암호 칸이 나와야 함"
    );

    // ZIP 만들기 창을 둔 채 툴바의 압축하기를 다시 누르고 폴더 선택을 취소함.
    let dialogs = TestDialogs::default();
    app.set_dialogs(Box::new(dialogs.clone()));
    policy_click(&ctx, &mut app, "압축하기");
    assert_eq!(
        dialogs.0.borrow().requests.len(),
        1,
        "폴더 선택 창이 열려야 함"
    );
    create_child_render(&ctx, &mut app);
    for label in ["/photos", "a.txt", "암호 확인"] {
        assert!(
            work_child_label(label).is_some(),
            "폴더 선택을 취소했는데 {label}이(가) 사라짐"
        );
    }

    // 대조: 다른 폴더를 실제로 고르면 그 폴더로 새로 시작하고 암호 설정도 비움.
    let other = scratch.0.join("music");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("song.mp3"), b"la").unwrap();
    dialogs.0.borrow_mut().answers.push_back(Some(vec![other]));
    policy_click(&ctx, &mut app, "압축하기");
    create_child_render(&ctx, &mut app);
    assert!(work_child_label("/music").is_some());
    assert!(work_child_label("song.mp3").is_some());
    assert!(work_child_label("a.txt").is_none());
    assert!(work_child_label("암호 확인").is_none());
}

#[test]
fn unchecking_password_also_forgets_undo_history() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let (mut app, ctx) = create_from_folder(&scratch, &factory);
    policy_click(&ctx, &mut app, "암호로 보호하기");
    text(&ctx, &mut app, "zip-password-input", "undo-secret");
    // egui는 입력이 1초쯤 멈춰야 실행 취소 지점을 남기므로 그만큼 프레임을 진행함.
    for _ in 0..90 {
        policy_render(&ctx, &mut app, vec![]);
    }

    // 체크를 풀었다가 다시 켜면 암호 칸은 비어 있음.
    policy_click(&ctx, &mut app, "암호로 보호하기");
    policy_click(&ctx, &mut app, "암호로 보호하기");

    // 실행 취소(Ctrl+Z)를 눌러도 지운 암호가 되살아나지 않아야 함.
    // 한 번만 누름(두 번째 실행 취소는 다시 처음 상태로 돌아감).
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::unique("zip-password-input")));
    policy_render(&ctx, &mut app, vec![]);
    {
        policy_render(
            &ctx,
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
    }
    policy_click(&ctx, &mut app, "보기");
    let shown = policy_render(&ctx, &mut app, vec![]);
    assert!(
        shown.iter().any(|(text, _)| text == "숨기기"),
        "암호가 보여야 함"
    );
    assert!(
        !shown.iter().any(|(text, _)| text.contains("undo-secret")),
        "지운 암호가 실행 취소로 되살아남"
    );
}

fn native_picker_render(ctx: &egui::Context, app: &mut ArchiveApp) {
    for _ in 0..3 {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1060.0, 720.0),
            )),
            ..Default::default()
        };
        raw.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                outer_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(100.0, 100.0),
                    egui::vec2(1060.0, 720.0),
                )),
                focused: Some(true),
                ..Default::default()
            },
        );
        let _ = ctx.run_ui(raw, |ui| app.show(ui));
    }
}

#[test]
fn native_picker_lowers_child_windows_and_restores_their_position_and_level() {
    for save in [false, true] {
        for encrypted in [false, true] {
            for failed in [false, true] {
                let scratch = Folder::new();
                let factory = FakeFactory::default();
                let mut app = loaded(&scratch, &factory);
                let source = scratch.0.join("photos");
                fs::create_dir(&source).unwrap();
                fs::write(source.join("a.txt"), b"source").unwrap();
                app.add_sources(vec![source]);
                let ctx = egui::Context::default();
                ctx.set_embed_viewports(false);
                install_work_child_renderer();
                native_picker_render(&ctx, &mut app);
                if encrypted {
                    let check = work_child_label("암호로 보호하기").unwrap();
                    work_child_click(&ctx, &mut app, check);
                    for name in ["zip-password-input", "zip-password-confirm"] {
                        WORK_CHILD.with(|child| {
                            let mut child = child.borrow_mut();
                            child.focus = Some(egui::Id::unique(name));
                            child.events = vec![egui::Event::Text("secret".into())];
                        });
                        native_picker_render(&ctx, &mut app);
                    }
                    native_picker_render(&ctx, &mut app);
                }
                let id = egui::ViewportId::from_hash_of("archive-create");
                let builder = || {
                    WORK_CHILD.with(|child| {
                        child
                            .borrow()
                            .builders
                            .iter()
                            .rev()
                            .find(|(window, _)| *window == id)
                            .unwrap()
                            .1
                            .clone()
                    })
                };
                let before = builder();
                assert!(before.position.is_some());
                assert_eq!(before.window_level, Some(egui::WindowLevel::AlwaysOnTop));
                let dialogs = HeldDialogs::default();
                app.set_dialogs(Box::new(dialogs.clone()));
                let starts = factory.state.borrow().starts.len();
                if save {
                    let compress = work_child_label("압축하기").unwrap();
                    work_child_click(&ctx, &mut app, compress);
                } else {
                    policy_click(&ctx, &mut app, "압축하기");
                }
                assert_eq!(
                    dialogs.0.borrow().len(),
                    1,
                    "save={save}, encrypted={encrypted}, failed={failed}"
                );
                native_picker_render(&ctx, &mut app);
                let during = builder();
                assert_eq!(during.window_level, Some(egui::WindowLevel::Normal));
                assert_eq!(during.position, before.position);
                assert_eq!(during.inner_size, before.inner_size);
                let rendered = work_rendered();
                native_picker_render(&ctx, &mut app);
                assert_eq!(work_rendered(), rendered);
                assert_eq!(factory.state.borrow().starts.len(), starts);
                dialogs.0.borrow()[0]
                    .send(if failed {
                        Err(zipelo_ui_memory_app::archive_dialog::DIALOG_UNAVAILABLE.into())
                    } else {
                        Ok(None)
                    })
                    .unwrap();
                native_picker_render(&ctx, &mut app);
                let after = builder();
                assert_eq!(after.window_level, before.window_level);
                assert_eq!(after.position, before.position);
                assert_eq!(factory.state.borrow().starts.len(), starts);
                assert!(work_child_label("암호로 보호하기").is_some());
            }
        }
    }
}
