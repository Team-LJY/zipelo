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
    for expected in ["ZIP 만들기", "/photos", "trip", "a.txt", "암호로 보호하기", "취소"] {
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
    assert!(shown.iter().any(|(text, _)| text == "두 암호가 같지 않습니다"));
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
    use zipelo_ui_memory_app::archive_theme::{ROW_HOVER, ROW_SELECTED};
    fn walk(shape: &egui::epaint::Shape, y: f32, found: &mut Option<egui::Color32>) {
        match shape {
            egui::epaint::Shape::Rect(rect)
                if (rect.fill == ROW_SELECTED || rect.fill == ROW_HOVER)
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
    use zipelo_ui_memory_app::archive_theme::{ROW_HOVER, ROW_SELECTED};
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
    // 선택한 행에 마우스를 올려도 더 옅어지지 않음.
    let output = hover(&ctx, &mut app, "a.txt");
    assert_eq!(row_fill_at(&output, a), Some(ROW_SELECTED));
    assert_eq!(row_fill_at(&output, trip), None);
    assert!(ROW_HOVER.a() < ROW_SELECTED.a());
}

#[test]
fn main_list_hover_uses_lighter_blue_and_selected_row_keeps_its_color() {
    use zipelo_ui_memory_app::archive_theme::{ROW_HOVER, ROW_SELECTED};
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
    assert_eq!(row_fill_at(&output, y), Some(ROW_SELECTED));
}
