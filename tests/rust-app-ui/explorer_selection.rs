struct Explorer {
    app: ArchiveApp,
    ctx: egui::Context,
    factory: FakeFactory,
    time: f64,
    opened: std::rc::Rc<std::cell::RefCell<Vec<PathBuf>>>,
}
impl Explorer {
    fn new(scratch: &Folder) -> Self {
        let factory = FakeFactory::default();
        let mut app = loaded(scratch, &factory);
        app.model.entries = [
            (0, "docs/child.txt", Kind::File),
            (1, "alpha.txt", Kind::File),
            (2, "beta.txt", Kind::File),
            (3, "gamma.txt", Kind::File),
            (4, "omega.txt", Kind::File),
            (5, "docs", Kind::Directory),
        ]
        .map(|(id, name, kind)| Entry {
            id,
            name: name.into(),
            kind,
            bytes: Some(7),
            packed: None,
            encrypted: false,
        })
        .into();
        let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let record = opened.clone();
        app.set_opener(Box::new(move |path| {
            record.borrow_mut().push(path.to_path_buf());
            Ok(())
        }));
        let mut screen = Self {
            app,
            ctx: egui::Context::default(),
            factory,
            time: 0.0,
            opened,
        };
        screen.frame(egui::Modifiers::NONE, vec![]);
        screen.frame(egui::Modifiers::NONE, vec![]);
        screen
    }
    fn frame(&mut self, modifiers: egui::Modifiers, events: Vec<egui::Event>) -> egui::FullOutput {
        self.time += 0.05;
        let mut events = events;
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(760.0, 540.0),
                )),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| self.app.show(ui),
        );
        output.textures_delta.clear();
        output
    }
    fn row(&mut self, path: &str) -> egui::Rect {
        self.frame(egui::Modifiers::NONE, vec![]);
        self.ctx
            .read_response(egui::Id::unique(("archive-row", path)))
            .unwrap()
            .rect
    }
    fn body(&self) -> egui::Rect {
        self.ctx
            .read_response(egui::Id::unique("archive-list"))
            .unwrap()
            .rect
    }
    fn pointer(
        &mut self,
        point: egui::Pos2,
        pressed: bool,
        modifiers: egui::Modifiers,
    ) -> egui::FullOutput {
        self.frame(
            modifiers,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers,
                },
            ],
        )
    }
    fn click(&mut self, path: &str, modifiers: egui::Modifiers) {
        self.time += 1.0;
        let point = self.row(path).center();
        self.pointer(point, true, modifiers);
        self.pointer(point, false, modifiers);
        self.frame(egui::Modifiers::NONE, vec![]);
    }
    fn key(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        self.frame(
            modifiers,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
        );
        self.frame(
            modifiers,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers,
            }],
        );
    }
    fn button(&mut self, point: egui::Pos2, button: egui::PointerButton) {
        self.time += 1.0;
        for pressed in [true, false] {
            self.frame(
                egui::Modifiers::NONE,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        self.frame(egui::Modifiers::NONE, vec![]);
    }
    fn double_click(&mut self, path: &str) {
        self.time += 1.0;
        let point = self.row(path).center();
        for _ in 0..2 {
            self.pointer(point, true, egui::Modifiers::NONE);
            self.pointer(point, false, egui::Modifiers::NONE);
        }
        for _ in 0..30 {
            self.frame(egui::Modifiers::NONE, vec![]);
        }
    }
    fn click_text(&mut self, label: &str) {
        let output = self.frame(egui::Modifiers::NONE, vec![]);
        let rect = layout_text(&output)
            .into_iter()
            .find(|(text, _, _)| text == label)
            .unwrap_or_else(|| panic!("{label} 없음"))
            .1;
        self.button(rect.center(), egui::PointerButton::Primary);
    }
    #[track_caller]
    fn selected(&self, ids: &[u32]) {
        assert_eq!(self.app.model.selected, ids.iter().copied().collect());
    }
}
fn explorer_ctrl() -> egui::Modifiers {
    egui::Modifiers {
        ctrl: true,
        ..egui::Modifiers::NONE
    }
}
fn explorer_shift() -> egui::Modifiers {
    egui::Modifiers {
        shift: true,
        ..egui::Modifiers::NONE
    }
}

#[test]
fn explorer_click_ctrl_shift_and_blank_match_visible_selection_count() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.click("alpha.txt", egui::Modifiers::NONE);
    s.selected(&[1]);
    s.click("beta.txt", explorer_ctrl());
    s.selected(&[1, 2]);
    s.click("alpha.txt", explorer_ctrl());
    s.selected(&[2]);
    s.click("alpha.txt", egui::Modifiers::NONE);
    s.click("gamma.txt", explorer_shift());
    s.selected(&[1, 2, 3]);
    let output = s.frame(egui::Modifiers::NONE, vec![]);
    assert!(
        layout_text(&output)
            .iter()
            .any(|(text, _, _)| text == "선택 3개")
    );
    s.click(
        "omega.txt",
        egui::Modifiers {
            ctrl: true,
            shift: true,
            ..egui::Modifiers::NONE
        },
    );
    s.selected(&[1, 2, 3, 4]);
    s.click("docs", egui::Modifiers::NONE);
    s.selected(&[0, 5]);
    let output = s.frame(egui::Modifiers::NONE, vec![]);
    assert!(
        layout_text(&output)
            .iter()
            .any(|(text, _, _)| text == "선택 1개")
    );
    let point = egui::pos2(100.0, s.body().bottom() - 4.0);
    s.pointer(point, true, explorer_ctrl());
    s.pointer(point, false, explorer_ctrl());
    s.selected(&[0, 5]);
    s.pointer(point, true, egui::Modifiers::NONE);
    s.pointer(point, false, egui::Modifiers::NONE);
    s.selected(&[]);
}

#[test]
fn explorer_keyboard_ranges_enter_backspace_and_escape_are_scoped_to_the_list() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.ctx
        .memory_mut(|m| m.request_focus(egui::Id::unique("archive-list")));
    s.frame(egui::Modifiers::NONE, vec![]);
    s.key(egui::Key::ArrowDown, egui::Modifiers::NONE);
    s.selected(&[0, 5]);
    s.key(egui::Key::ArrowDown, explorer_shift());
    s.selected(&[0, 1, 5]);
    s.key(egui::Key::ArrowUp, explorer_shift());
    s.selected(&[0, 5]);
    s.key(egui::Key::ArrowDown, explorer_shift());
    s.selected(&[0, 1, 5]);
    s.key(egui::Key::A, explorer_ctrl());
    s.selected(&[0, 1, 2, 3, 4, 5]);
    s.key(egui::Key::Escape, egui::Modifiers::NONE);
    s.selected(&[]);
    s.key(egui::Key::ArrowUp, egui::Modifiers::NONE);
    s.selected(&[0, 5]);
    s.key(egui::Key::Enter, egui::Modifiers::NONE);
    s.selected(&[]);
    assert!(
        layout_text(&s.frame(egui::Modifiers::NONE, vec![]))
            .iter()
            .any(|(text, _, _)| text == "child.txt")
    );
    s.key(egui::Key::Backspace, egui::Modifiers::NONE);
    s.selected(&[]);
    assert!(
        layout_text(&s.frame(egui::Modifiers::NONE, vec![]))
            .iter()
            .any(|(text, _, _)| text == "alpha.txt")
    );
    s.ctx
        .memory_mut(|m| m.request_focus(egui::Id::unique("archive-search")));
    s.key(egui::Key::A, explorer_ctrl());
    s.selected(&[]);
    s.key(egui::Key::Enter, egui::Modifiers::NONE);
    assert_eq!(s.factory.state.borrow().starts.len(), 1);
}

#[test]
fn explorer_double_click_opens_files_with_default_app_and_folders_in_place() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let destination = s.app.model.archive.clone().unwrap().with_file_name("sample");
    // 실제 작업자처럼 새로 만든 상위 폴더도 출력으로 알림.
    s.factory.state.borrow_mut().outputs = Some(vec!["docs".into(), "docs/child.txt".into()]);
    s.app.browse_folder("docs");
    s.double_click("docs/child.txt");
    s.selected(&[0]);
    // 파일은 압축파일 옆 숨김 임시 폴더에 그 파일만 풀고 기본 프로그램으로 엶.
    let root = scratch.0.join("sample.zip (Zipelo 임시)");
    {
        let state = s.factory.state.borrow();
        assert_eq!(state.starts.len(), 2);
        let Start::Extract {
            destination: folder,
            selection,
            ..
        } = state.starts.last().unwrap()
        else {
            panic!("{:?}", state.starts.last());
        };
        assert_eq!(selection, &Selection::EntryIds(vec![0]));
        assert_eq!(folder, &root.join("1"));
    }
    assert_eq!(
        *s.opened.borrow(),
        vec![root.join("1").join("docs").join("child.txt")]
    );
    let output = s.frame(egui::Modifiers::NONE, vec![]);
    assert!(
        !layout_text(&output)
            .iter()
            .any(|(text, _, _)| text == "작업 결과")
    );
    // 같은 파일을 다시 열면 새 하위 폴더를 씀.
    s.double_click("docs/child.txt");
    assert!(matches!(
        s.factory.state.borrow().starts.last(),
        Some(Start::Extract { destination, .. }) if destination == &root.join("2")
    ));
    assert_eq!(s.opened.borrow().len(), 2);
    // 열 파일이 출력에 없으면 조용히 넘어가지 않고 알림.
    s.factory.state.borrow_mut().outputs = Some(vec!["docs".into()]);
    s.double_click("docs/child.txt");
    assert_eq!(s.opened.borrow().len(), 2);
    assert!(
        layout_text(&s.frame(egui::Modifiers::NONE, vec![]))
            .iter()
            .any(|(text, _, _)| text == "풀린 파일을 찾지 못해 열지 못했습니다")
    );
    // 열기용 폴더는 압축파일 이름 폴더와 섞이지 않음.
    assert!(!destination.exists());
    // 앱을 닫으면 열기용 폴더를 지움.
    assert!(root.is_dir());
    drop(s);
    assert!(!root.exists());
}

/// 열기용 풀기가 한도 초과로 실패한 상태까지 진행함. 결과 창을 그리는 문맥도 함께 돌려줌.
fn double_click_open_hits_limit(s: &mut Explorer) -> egui::Context {
    s.factory.state.borrow_mut().manual = true;
    s.double_click("alpha.txt");
    assert_eq!(s.factory.state.borrow().starts.len(), 2);
    finish_limit(s);
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    for _ in 0..12 {
        policy_render(&ctx, &mut s.app, vec![]);
    }
    assert!(s.app.model.can_retry_expanded());
    assert!(s.opened.borrow().is_empty());
    ctx
}

fn finish_limit(s: &mut Explorer) {
    {
        let mut worker = s.factory.state.borrow_mut();
        worker.frame(Event::Accepted { encryption: false });
        worker.frame(Event::Terminal(Terminal {
            outcome: Outcome::Failed,
            error: Some(ErrorCode::Limit),
            publication: Publication::None,
            created_count: 0,
            retained_partial_count: 0,
            durability: Durability::NotApplicable,
            cleanup: Cleanup::Complete,
        }));
        worker.queue.extend([
            HostEvent::StdoutEof,
            HostEvent::StderrEof,
            HostEvent::Exited { success: true },
        ]);
    }
    for _ in 0..30 {
        s.frame(egui::Modifiers::NONE, vec![]);
    }
}

#[test]
fn explorer_open_keeps_pending_open_when_limit_retry_succeeds() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let root = scratch.0.join("sample.zip (Zipelo 임시)");
    let ctx = double_click_open_hits_limit(&mut s);
    policy_click(&ctx, &mut s.app, "한도를 늘려 다시 시도");
    assert!(s.app.model.busy());
    assert!(s.factory.state.borrow().expanded);
    assert!(matches!(
        s.factory.state.borrow().starts.last(),
        Some(Start::Extract { destination, .. }) if destination == &root.join("1")
    ));
    {
        let mut worker = s.factory.state.borrow_mut();
        worker.frame(Event::Accepted { encryption: false });
        worker.frame(Event::Output {
            entry_id: None,
            relative_name: "alpha.txt".into(),
            disposition: Disposition::Created,
        });
        worker.finish(Outcome::Completed, Publication::Complete);
    }
    for _ in 0..30 {
        s.frame(egui::Modifiers::NONE, vec![]);
    }
    // 다시 시도가 성공하면 기본 프로그램이 열림.
    assert_eq!(*s.opened.borrow(), vec![root.join("1").join("alpha.txt")]);
    // 열기가 끝나면 풀 위치는 원래대로 돌아옴.
    s.factory.state.borrow_mut().manual = false;
    assert!(s.app.begin_extract());
    assert!(matches!(
        s.factory.state.borrow().starts.last(),
        Some(Start::Extract { destination, .. }) if !destination.starts_with(&root)
    ));
}

#[test]
fn explorer_open_restores_destination_when_limit_result_is_dismissed() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let root = scratch.0.join("sample.zip (Zipelo 임시)");
    let ctx = double_click_open_hits_limit(&mut s);
    policy_click(&ctx, &mut s.app, "확인");
    for _ in 0..3 {
        policy_render(&ctx, &mut s.app, vec![]);
    }
    assert!(s.opened.borrow().is_empty());
    s.factory.state.borrow_mut().manual = false;
    assert!(s.app.begin_extract());
    let state = s.factory.state.borrow();
    let Some(Start::Extract { destination, .. }) = state.starts.last() else {
        panic!("{:?}", state.starts.last());
    };
    assert_eq!(destination, &scratch.0.join("sample"));
    assert!(!destination.starts_with(&root));
}

#[test]
fn explorer_open_restores_destination_when_expanded_retry_fails_again() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let ctx = double_click_open_hits_limit(&mut s);
    policy_click(&ctx, &mut s.app, "한도를 늘려 다시 시도");
    assert!(s.app.model.busy());
    finish_limit(&mut s);
    assert!(s.opened.borrow().is_empty());
    assert!(!s.app.model.can_retry_expanded());
    s.factory.state.borrow_mut().manual = false;
    assert!(s.app.begin_extract());
    assert!(matches!(
        s.factory.state.borrow().starts.last(),
        Some(Start::Extract { destination, .. }) if destination == &scratch.0.join("sample")
    ));
}

#[test]
fn explorer_open_pending_after_limit_is_dropped_when_zip_creation_starts() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let root = scratch.0.join("sample.zip (Zipelo 임시)");
    let _limit_ctx = double_click_open_hits_limit(&mut s);
    // 한도 초과 결과를 닫지 않고 폴더를 끌어 놓아 ZIP 만들기를 시작함.
    let folder = scratch.0.join("photos");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.txt"), b"abc").unwrap();
    s.app.add_sources(vec![folder]);
    let ctx = egui::Context::default();
    policy_click(&ctx, &mut s.app, "압축하기");
    assert!(s.app.model.busy());
    assert!(matches!(
        s.factory.state.borrow().starts.last(),
        Some(Start::CreateZip { .. })
    ));
    {
        let mut worker = s.factory.state.borrow_mut();
        worker.frame(Event::Accepted { encryption: false });
        worker.frame(Event::Output {
            entry_id: None,
            relative_name: "photos.zip".into(),
            disposition: Disposition::Created,
        });
        worker.finish(Outcome::Completed, Publication::Complete);
    }
    for _ in 0..30 {
        s.frame(egui::Modifiers::NONE, vec![]);
    }
    // 만들기가 끝나도 이전 열기가 적용되지 않아 오류가 뜨지 않음.
    for _ in 0..3 {
        policy_render(&ctx, &mut s.app, vec![]);
    }
    let shown = policy_render(&ctx, &mut s.app, vec![]);
    assert!(
        !shown.iter().any(|(text, _)| text.contains("열지 못했습니다")),
        "{shown:?}"
    );
    assert!(s.opened.borrow().is_empty());
    // 풀 위치도 원래대로 돌아옴.
    s.factory.state.borrow_mut().manual = false;
    assert!(s.app.begin_extract());
    let state = s.factory.state.borrow();
    let Some(Start::Extract { destination, .. }) = state.starts.last() else {
        panic!("{:?}", state.starts.last());
    };
    assert_eq!(destination, &scratch.0.join("sample"));
    assert!(!destination.starts_with(&root));
}

#[test]
fn explorer_double_click_does_not_open_executables() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.app.model.entries[2].name = "setup.EXE".into();
    s.double_click("setup.EXE");
    assert_eq!(s.factory.state.borrow().starts.len(), 1);
    assert!(s.opened.borrow().is_empty());
    let output = s.frame(egui::Modifiers::NONE, vec![]);
    assert!(
        layout_text(&output)
            .iter()
            .any(|(text, _, _)| text == "실행 파일(.EXE)은 안전을 위해 바로 열 수 없습니다")
    );
    // 실행 파일은 압축을 풀지 않으므로 임시 폴더도 만들지 않음.
    assert!(!scratch.0.join("sample.zip (Zipelo 임시)").exists());
}

#[test]
fn explorer_dragging_in_parent_work_window_does_not_select_rows() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.app.set_work_window_in_parent(true);
    s.app.model.entries[2].name = "setup.EXE".into();
    s.double_click("setup.EXE");
    s.selected(&[2]);
    let close = s
        .ctx
        .read_response(egui::Id::unique(("window-button", "work-in-parent", "닫기")))
        .expect("부모 안 작업 창")
        .rect;
    // 행이 하나뿐인 폴더로 옮겨 작업 창 제목 줄 아래가 목록의 빈 곳이 되게 함.
    s.app.browse_folder("docs");
    let row = s.row("docs/child.txt");
    s.selected(&[]);
    // 작업 창 제목 줄을 누르고 목록 행 위로 끌어도 목록 선택 사각형이 생기지 않아야 함.
    let start = egui::pos2(close.left() - 120.0, close.center().y);
    assert!(start.y > row.bottom(), "{start:?} {row:?}");
    s.pointer(start, true, egui::Modifiers::NONE);
    let end = egui::pos2(row.center().x, row.center().y);
    for step in 1..=6 {
        let t = step as f32 / 6.0;
        s.frame(
            egui::Modifiers::NONE,
            vec![egui::Event::PointerMoved(start + (end - start) * t)],
        );
    }
    let output = s.frame(egui::Modifiers::NONE, vec![]);
    s.selected(&[]);
    assert!(!output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::epaint::Shape::Rect(rect) if rect.fill == theme_blue_fill()
    )));
    s.pointer(end, false, egui::Modifiers::NONE);
    s.selected(&[]);
}

fn theme_blue_fill() -> egui::Color32 {
    zipelo_ui_memory_app::archive_theme::BLUE.gamma_multiply(0.16)
}

#[test]
fn explorer_context_menu_extracts_selection_into_numbered_archive_folder() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    fs::create_dir(scratch.0.join("sample")).unwrap();
    s.click("alpha.txt", egui::Modifiers::NONE);
    // 선택하지 않은 행을 오른쪽 클릭하면 그 행만 선택하고 메뉴를 엶.
    let point = s.row("docs").center();
    s.button(point, egui::PointerButton::Secondary);
    s.selected(&[0, 5]);
    s.click_text("압축 파일명으로 새 폴더를 만들어 압축 풀기");
    for _ in 0..30 {
        s.frame(egui::Modifiers::NONE, vec![]);
    }
    let state = s.factory.state.borrow();
    let Some(Start::Extract {
        destination,
        selection,
        ..
    }) = state.starts.last()
    else {
        panic!("{:?}", state.starts.last());
    };
    assert_eq!(destination, &scratch.0.join("sample (1)"));
    assert_eq!(selection, &Selection::EntryIds(vec![0, 5]));
    assert!(scratch.0.join("sample (1)").is_dir());
    assert!(s.opened.borrow().is_empty());
}

#[test]
fn explorer_context_menu_asks_for_folder_and_extracts_only_selection() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let dialogs = crate::dialogs::TestDialogs::default();
    let target = scratch.0.join("target");
    fs::create_dir(&target).unwrap();
    dialogs
        .0
        .borrow_mut()
        .answers
        .push_back(Some(vec![target.clone()]));
    s.app.set_dialogs(Box::new(dialogs.clone()));
    s.click("alpha.txt", egui::Modifiers::NONE);
    s.click("beta.txt", explorer_ctrl());
    let point = s.row("beta.txt").center();
    s.button(point, egui::PointerButton::Secondary);
    s.selected(&[1, 2]);
    s.click_text("특정 폴더에 압축 풀기");
    for _ in 0..30 {
        s.frame(egui::Modifiers::NONE, vec![]);
    }
    assert_eq!(
        dialogs.0.borrow().requests.last().unwrap().purpose,
        zipelo_ui_memory_app::archive_dialog::Purpose::Destination
    );
    assert!(matches!(
        s.factory.state.borrow().starts.last(),
        Some(Start::Extract { destination, selection: Selection::EntryIds(ids), .. })
            if destination == &target && ids == &vec![1, 2]
    ));
}

#[test]
fn explorer_mouse_back_button_goes_to_parent_folder() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.app.browse_folder("docs");
    let point = s.row("docs/child.txt").center();
    s.button(point, egui::PointerButton::Extra2);
    assert!(s.ctx.read_response(egui::Id::unique(("archive-row", "docs/child.txt"))).is_some());
    s.button(point, egui::PointerButton::Extra1);
    assert!(
        layout_text(&s.frame(egui::Modifiers::NONE, vec![]))
            .iter()
            .any(|(text, _, _)| text == "alpha.txt")
    );
    // 최상위에서는 아무 일도 하지 않음.
    s.button(point, egui::PointerButton::Extra1);
    assert!(
        layout_text(&s.frame(egui::Modifiers::NONE, vec![]))
            .iter()
            .any(|(text, _, _)| text == "alpha.txt")
    );
}

#[test]
fn explorer_double_click_opens_only_folders_and_resets_selection() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.time += 1.0;
    let point = s.row("docs").center();
    for _ in 0..2 {
        s.pointer(point, true, egui::Modifiers::NONE);
        s.pointer(point, false, egui::Modifiers::NONE);
    }
    s.selected(&[]);
    assert!(
        layout_text(&s.frame(egui::Modifiers::NONE, vec![]))
            .iter()
            .any(|(text, _, _)| text == "child.txt")
    );
}

#[test]
fn explorer_marquee_selects_intersections_and_ctrl_adds_with_a_translucent_blue_rectangle() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.click("docs", egui::Modifiers::NONE);
    let row = s.row("alpha.txt");
    let start = egui::pos2(row.right() + 8.0, s.body().bottom() - 5.0);
    let end = egui::pos2(150.0, row.top() + 2.0);
    s.pointer(start, true, explorer_ctrl());
    let output = s.frame(explorer_ctrl(), vec![egui::Event::PointerMoved(end)]);
    s.selected(&[0, 1, 2, 3, 4, 5]);
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        Shape::Rect(rect)
            if rect.fill == zipelo_ui_memory_app::archive_theme::BLUE.gamma_multiply(0.16)
                && rect.rect.height() > 100.0
                && rect.fill.a() > 0
                && rect.fill.a() < 255
                && rect.stroke == egui::Stroke::new(1.0, zipelo_ui_memory_app::archive_theme::BLUE)
                && rect.stroke_kind == egui::StrokeKind::Inside
    )));
    s.pointer(end, false, explorer_ctrl());
    s.selected(&[0, 1, 2, 3, 4, 5]);
    let start = egui::pos2(row.right() + 8.0, s.body().bottom() - 5.0);
    s.pointer(start, true, egui::Modifiers::NONE);
    s.frame(egui::Modifiers::NONE, vec![egui::Event::PointerMoved(end)]);
    s.pointer(end, false, egui::Modifiers::NONE);
    s.selected(&[1, 2, 3, 4]);
}

#[test]
fn explorer_marquee_autoscrolls_thousands_of_rows_without_rendering_them_all() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.app.model.entries = (0..5000)
        .map(|id| Entry {
            id,
            name: format!("item-{id:05}.txt"),
            kind: Kind::File,
            bytes: Some(1),
            packed: None,
            encrypted: false,
        })
        .collect();
    s.frame(egui::Modifiers::NONE, vec![]);
    let row = s.row("item-00000.txt");
    let start = egui::pos2(row.right() + 8.0, row.center().y);
    s.pointer(start, true, egui::Modifiers::NONE);
    let end = egui::pos2(100.0, s.body().bottom() + 45.0);
    s.frame(egui::Modifiers::NONE, vec![egui::Event::PointerMoved(end)]);
    let started = std::time::Instant::now();
    let mut output = s.frame(egui::Modifiers::NONE, vec![]);
    for _ in 0..40 {
        output = s.frame(egui::Modifiers::NONE, vec![]);
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(
        s.app.model.selected.len() > 20,
        "{:?}",
        s.app.model.selected
    );
    assert!(
        layout_text(&output)
            .iter()
            .filter(|(text, _, _)| text.starts_with("item-"))
            .count()
            < 30
    );
    assert!(
        layout_text(&output)
            .iter()
            .filter(|(text, _, _)| text.starts_with("item-"))
            .all(|(text, _, _)| text != "item-00000.txt")
    );
    let first_visible = |output: &egui::FullOutput| {
        layout_text(output)
            .into_iter()
            .filter_map(|(text, _, _)| {
                text.strip_prefix("item-")
                    .and_then(|name| name.strip_suffix(".txt"))
                    .and_then(|number| number.parse::<u32>().ok())
            })
            .min()
            .unwrap()
    };
    let down = first_visible(&output);
    let up = egui::pos2(100.0, s.body().top() - 45.0);
    s.frame(egui::Modifiers::NONE, vec![egui::Event::PointerMoved(up)]);
    for _ in 0..12 {
        output = s.frame(egui::Modifiers::NONE, vec![]);
    }
    assert!(first_visible(&output) < down);
    s.pointer(up, false, egui::Modifiers::NONE);
    let selected = s.app.model.selected.clone();
    s.frame(egui::Modifiers::NONE, vec![egui::Event::PointerMoved(end)]);
    assert_eq!(s.app.model.selected, selected);
}

#[test]
fn explorer_selected_folder_and_empty_selection_reach_the_exact_extraction_request() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.click("docs", egui::Modifiers::NONE);
    s.app.choose_destination(scratch.0.clone());
    assert!(s.app.begin_extract());
    assert!(
        matches!(s.factory.state.borrow().starts.last(), Some(Start::Extract { selection: Selection::EntryIds(ids), .. }) if ids.iter().copied().collect::<BTreeSet<_>>() == [0,5].into_iter().collect())
    );
    for _ in 0..30 {
        s.frame(egui::Modifiers::NONE, vec![]);
    }
    s.app.model.selected.clear();
    assert!(s.app.begin_extract());
    assert!(matches!(
        s.factory.state.borrow().starts.last(),
        Some(Start::Extract {
            selection: Selection::All,
            ..
        })
    ));
}

#[test]
fn explorer_fast_forward_marquee_uses_the_original_press_position() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let first = s.row("docs");
    let last = s.row("beta.txt");
    let start = egui::pos2(first.right() + 8.0, first.top() + 2.0);
    let end = egui::pos2(100.0, last.bottom() - 1.0);
    s.frame(
        egui::Modifiers::NONE,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerMoved(end),
        ],
    );
    s.selected(&[0, 1, 2, 5]);
    s.pointer(end, false, egui::Modifiers::NONE);
    s.selected(&[0, 1, 2, 5]);
}

#[test]
fn explorer_ctrl_removes_a_large_directory_subtree_without_ancestor_rescans() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.app.model.entries = (0..5001)
        .map(|id| Entry {
            id,
            name: if id == 0 {
                "docs".into()
            } else {
                format!("docs/child-{id:05}")
            },
            kind: Kind::Directory,
            bytes: None,
            packed: None,
            encrypted: false,
        })
        .collect();
    s.frame(egui::Modifiers::NONE, vec![]);
    s.click("docs", egui::Modifiers::NONE);
    assert_eq!(s.app.model.selected.len(), 5001);
    let started = std::time::Instant::now();
    s.click("docs", explorer_ctrl());
    assert!(s.app.model.selected.is_empty());
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn explorer_mouse_click_gives_list_focus_for_keyboard_selection_and_navigation() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    assert!(s.ctx.memory(|memory| memory.focused()).is_none());
    s.click("alpha.txt", egui::Modifiers::NONE);
    assert!(s.ctx.memory(|memory| memory.focused()).is_some());
    s.key(egui::Key::ArrowDown, egui::Modifiers::NONE);
    s.selected(&[2]);
    s.key(egui::Key::ArrowDown, explorer_shift());
    s.selected(&[2, 3]);
    s.key(egui::Key::A, explorer_ctrl());
    s.selected(&[0, 1, 2, 3, 4, 5]);
    s.click("docs", egui::Modifiers::NONE);
    s.key(egui::Key::Enter, egui::Modifiers::NONE);
    assert!(
        layout_text(&s.frame(egui::Modifiers::NONE, vec![]))
            .iter()
            .any(|(text, _, _)| text == "child.txt")
    );
    s.key(egui::Key::Backspace, egui::Modifiers::NONE);
    assert!(
        layout_text(&s.frame(egui::Modifiers::NONE, vec![]))
            .iter()
            .any(|(text, _, _)| text == "alpha.txt")
    );
    s.click("alpha.txt", egui::Modifiers::NONE);
    s.key(egui::Key::Escape, egui::Modifiers::NONE);
    s.selected(&[]);
}

#[test]
fn explorer_retry_preserves_original_scope_after_selection_changes_and_conflicts() {
    for selected in [false, true] {
        for conflict in [false, true] {
            let scratch = Folder::new();
            let mut s = Explorer::new(&scratch);
            s.factory.state.borrow_mut().manual = true;
            if selected {
                s.click("docs", egui::Modifiers::NONE);
            }
            assert!(s.app.begin_extract());
            let original = match s.factory.state.borrow().starts.last().unwrap() {
                Start::Extract { selection, .. } => selection.clone(),
                _ => panic!("추출 요청이 필요함"),
            };
            {
                let mut worker = s.factory.state.borrow_mut();
                worker.frame(Event::Accepted { encryption: false });
                worker.finish(Outcome::Failed, Publication::None);
                for event in &mut worker.queue {
                    if let HostEvent::Frame {
                        event: Event::Terminal(t),
                        ..
                    } = event
                    {
                        t.error = Some(if conflict {
                            ErrorCode::Conflict
                        } else {
                            ErrorCode::Io
                        });
                    }
                }
            }
            for _ in 0..30 {
                s.frame(egui::Modifiers::NONE, vec![]);
            }
            s.click("alpha.txt", egui::Modifiers::NONE);
            s.selected(&[1]);
            let chosen = scratch.0.join("retry-output");
            if conflict {
                fs::create_dir(&chosen).unwrap();
                let dialogs = TestDialogs::default();
                dialogs
                    .0
                    .borrow_mut()
                    .answers
                    .push_back(Some(vec![chosen.clone()]));
                s.app.set_dialogs(Box::new(dialogs));
            }
            policy_click(
                &s.ctx,
                &mut s.app,
                if conflict {
                    "다른 폴더에 풀기"
                } else {
                    "다시 시도"
                },
            );
            assert!(matches!(s.factory.state.borrow().starts.last(),
                Some(Start::Extract { selection, destination, .. })
                    if selection == &original && (!conflict || destination == &chosen)));
        }
    }
}
