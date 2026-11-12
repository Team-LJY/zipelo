struct Explorer {
    app: ArchiveApp,
    ctx: egui::Context,
    factory: FakeFactory,
    time: f64,
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
        let mut screen = Self {
            app,
            ctx: egui::Context::default(),
            factory,
            time: 0.0,
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
fn explorer_double_click_opens_only_folders_and_resets_selection() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let point = s.row("alpha.txt").center();
    for _ in 0..2 {
        s.pointer(point, true, egui::Modifiers::NONE);
        s.pointer(point, false, egui::Modifiers::NONE);
    }
    s.selected(&[1]);
    assert_eq!(s.factory.state.borrow().starts.len(), 1);
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
