#[test]
fn browser_builds_virtual_folders_and_expands_only_the_selected_subtree() {
    use zipelo_ui_memory_app::archive_browser::*;
    let entries = [
        (0, "Install/cab1.cab", Kind::File),
        (1, "Install/nested/readme.txt", Kind::File),
        (2, "Install-old/file.txt", Kind::File),
        (3, "Install", Kind::Directory),
        (4, "Install/empty", Kind::Directory),
        (5, "Install.exe", Kind::File),
    ]
    .map(|(id, name, kind)| Entry {
        id,
        name: name.into(),
        kind,
        bytes: Some(10),
        packed: None,
        encrypted: false,
    });
    let top = rows(&entries, "", "");
    assert_eq!(
        top.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
        ["Install", "Install-old", "Install.exe"]
    );
    assert_eq!(top[0].ids, [0, 1, 3, 4].into_iter().collect());
    let child = rows(&entries, "Install", "");
    assert_eq!(
        child
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        ["empty", "nested", "cab1.cab"]
    );
    assert_eq!(rows(&entries, "Install", "CAB").len(), 1);
    assert_eq!(
        expand_selection(&entries, &[3].into_iter().collect()),
        top[0].ids
    );
    assert!(normalized("../unsafe").is_none());
    assert!(normalized("/absolute").is_none());
    assert!(normalized(r"C:\file").is_none());
    assert_eq!(
        normalized(r"Install\nested\file"),
        Some("Install/nested/file".into())
    );
}

#[test]
fn native_dialog_cancellation_never_starts_a_job() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    let dialogs = TestDialogs::default();
    dialogs.0.borrow_mut().cancel_save = true;
    app.set_dialogs(Box::new(dialogs.clone()));
    let ctx = egui::Context::default();
    policy_click(&ctx, &mut app, "열기");
    policy_click(&ctx, &mut app, "압축해제");
    assert_eq!(factory.state.borrow().starts.len(), 1);
    policy_click(&ctx, &mut app, "압축하기");
    policy_click(&ctx, &mut app, "파일 추가");
    policy_click(&ctx, &mut app, "폴더 추가");
    let shown = policy_render(&ctx, &mut app, vec![]);
    assert!(shown.iter().any(|(text, _)| text == "ZIP 만들기"));
    assert_eq!(factory.state.borrow().starts.len(), 1);
    let source = scratch.0.join("source.txt");
    fs::write(&source, b"source").unwrap();
    app.add_sources(vec![source]);
    policy_click(&ctx, &mut app, "압축하기");
    assert_eq!(factory.state.borrow().starts.len(), 1);
    assert!(!app.model.busy());
    let purposes = dialogs
        .0
        .borrow()
        .requests
        .iter()
        .map(|request| request.purpose)
        .collect::<Vec<_>>();
    assert_eq!(
        purposes,
        [
            Purpose::Open,
            Purpose::Destination,
            Purpose::Files,
            Purpose::Folder,
            Purpose::SaveZip
        ]
    );
}

#[test]
fn toolbar_and_modal_keyboard_do_not_submit_from_secondary_controls() {
    for label in ["취소", "파일 추가", "폴더 추가"] {
        let scratch = Folder::new();
        let factory = FakeFactory::default();
        let mut app = loaded(&scratch, &factory);
        let source = scratch.0.join("source.txt");
        fs::write(&source, b"source").unwrap();
        app.add_sources(vec![source]);
        let ctx = egui::Context::default();
        policy_focus(&ctx, &mut app, label);
        policy_render(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        assert_eq!(factory.state.borrow().starts.len(), 1, "{label}");
    }
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    let ctx = egui::Context::default();
    policy_render(&ctx, &mut app, vec![]);
    text(&ctx, &mut app, "archive-search", "읽어");
    policy_render(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(factory.state.borrow().starts.len(), 1);
}

#[test]
fn native_destination_expands_directory_ids_and_uses_archive_parent_default() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    app.model.entries = [
        (0, "docs", Kind::Directory),
        (1, "docs/file.txt", Kind::File),
        (2, "docs-old/other.txt", Kind::File),
    ]
    .map(|(id, name, kind)| Entry {
        id,
        name: name.into(),
        kind,
        bytes: Some(1),
        packed: None,
        encrypted: false,
    })
    .to_vec();
    app.model.selected.insert(0);
    let dialogs = TestDialogs::default();
    dialogs
        .0
        .borrow_mut()
        .answers
        .push_back(Some(vec![scratch.0.clone()]));
    app.set_dialogs(Box::new(dialogs.clone()));
    let ctx = egui::Context::default();
    policy_click(&ctx, &mut app, "압축해제");
    assert!(matches!(factory.state.borrow().starts.last(),
        Some(Start::Extract { selection: Selection::EntryIds(ids), destination, .. })
        if ids == &vec![0, 1] && destination == &scratch.0));
    assert_eq!(dialogs.0.borrow().requests[0].directory, scratch.0);
}

#[test]
fn toolbar_has_three_actions_and_zip_password_option_remains_available() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    let ctx = egui::Context::default();
    policy_render(&ctx, &mut app, vec![]);
    let shown = policy_render(&ctx, &mut app, vec![]);
    for label in ["열기", "압축하기", "압축해제"] {
        assert!(shown.iter().any(|(text, _)| text == label), "{label}");
    }
    assert!(!shown.iter().any(|(text, _)| text == "암호화"));
    policy_click(&ctx, &mut app, "압축하기");
    let shown = policy_render(&ctx, &mut app, vec![]);
    assert!(shown.iter().any(|(text, _)| text == "ZIP 만들기"));
    assert!(shown.iter().any(|(text, _)| text == "암호로 보호하기"));
    let source = scratch.0.join("protected.txt");
    fs::write(&source, b"protected").unwrap();
    app.add_sources(vec![source]);
    policy_click(&ctx, &mut app, "암호로 보호하기");
    policy_click(&ctx, &mut app, "압축하기");
    assert!(matches!(
        factory.state.borrow().starts.last(),
        Some(Start::CreateZip { aes256: true, .. })
    ));
}

#[test]
fn invalid_source_batch_is_rejected_without_starting_or_adding_valid_members() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    let source = scratch.0.join("source.txt");
    fs::write(&source, b"source").unwrap();
    app.add_sources(vec![source, scratch.0.join("missing")]);
    app.show_create(false);
    let ctx = egui::Context::default();
    policy_click(&ctx, &mut app, "압축하기");
    assert_eq!(factory.state.borrow().starts.len(), 1);
    #[cfg(unix)]
    {
        let link = scratch.0.join("link");
        std::os::unix::fs::symlink(&scratch.0, &link).unwrap();
        app.add_sources(vec![link]);
        assert_eq!(factory.state.borrow().starts.len(), 1);
    }
}

#[test]
fn keyboard_opens_virtual_folder_and_searches_only_its_children() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    app.model.entries = ["Install/cab1.cab", "Install/cab2.cab", "Install.exe"]
        .iter()
        .enumerate()
        .map(|(id, name)| Entry {
            id: id as u32,
            name: name.to_string(),
            kind: Kind::File,
            bytes: Some(1),
            packed: None,
            encrypted: false,
        })
        .collect();
    let ctx = egui::Context::default();
    policy_focus(&ctx, &mut app, "Install");
    policy_render(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    let shown = policy_render(&ctx, &mut app, vec![]);
    assert!(shown.iter().any(|(text, _)| text == "cab1.cab"));
    assert!(!shown.iter().any(|(text, _)| text == "Install.exe"));
    text(&ctx, &mut app, "archive-search", "CAB2");
    let shown = policy_render(&ctx, &mut app, vec![]);
    assert!(shown.iter().any(|(text, _)| text == "cab2.cab"));
    assert!(!shown.iter().any(|(text, _)| text == "cab1.cab"));
    assert_eq!(factory.state.borrow().starts.len(), 1);
}

#[test]
fn focused_compress_button_opens_save_dialog_and_starts_exactly_once() {
    let scratch = Folder::new();
    let factory = FakeFactory::manual();
    let mut app = ArchiveApp::new(Box::new(factory.clone()), None, false);
    let dialogs = TestDialogs::default();
    app.set_dialogs(Box::new(dialogs.clone()));
    let source = scratch.0.join("source.txt");
    fs::write(&source, b"source").unwrap();
    app.add_sources(vec![source]);
    let ctx = egui::Context::default();
    policy_focus(&ctx, &mut app, "압축하기");
    policy_render(&ctx, &mut app, vec![key(egui::Key::Enter)]);
    assert_eq!(factory.state.borrow().starts.len(), 1);
    assert_eq!(dialogs.0.borrow().requests.len(), 1);
    assert_eq!(dialogs.0.borrow().requests[0].purpose, Purpose::SaveZip);
    assert_eq!(dialogs.0.borrow().requests[0].file_name, "source.zip");
}

#[test]
fn conflict_native_dialog_cancel_and_retry_preserve_selection_and_existing_files() {
    let scratch = Folder::new();
    let chosen = scratch.0.join("chosen");
    fs::create_dir(&chosen).unwrap();
    fs::write(scratch.0.join("existing"), b"preserve").unwrap();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    app.model.selected.insert(0);
    factory.state.borrow_mut().manual = true;
    let dialogs = TestDialogs::default();
    dialogs
        .0
        .borrow_mut()
        .answers
        .push_back(Some(vec![scratch.0.clone()]));
    app.set_dialogs(Box::new(dialogs.clone()));
    let ctx = egui::Context::default();
    policy_click(&ctx, &mut app, "압축해제");
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
                t.error = Some(ErrorCode::Conflict);
            }
        }
    }
    for _ in 0..30 {
        policy_render(&ctx, &mut app, vec![]);
    }
    policy_click(&ctx, &mut app, "다른 폴더에 풀기");
    assert_eq!(factory.state.borrow().starts.len(), 2);
    dialogs
        .0
        .borrow_mut()
        .answers
        .push_back(Some(vec![chosen.clone()]));
    policy_click(&ctx, &mut app, "다른 폴더에 풀기");
    assert!(matches!(factory.state.borrow().starts.last(),
        Some(Start::Extract { destination, selection: Selection::EntryIds(ids), .. })
        if destination == &chosen && ids == &vec![0]));
    assert_eq!(fs::read(scratch.0.join("existing")).unwrap(), b"preserve");
}

#[test]
fn folder_navigation_clears_parent_selection_and_extracts_only_new_selection() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    app.model.entries = [
        (0, "docs", Kind::Directory),
        (1, "docs/excluded.txt", Kind::File),
        (2, "docs/retained.txt", Kind::File),
    ]
    .map(|(id, name, kind)| Entry {
        id,
        name: name.into(),
        kind,
        bytes: Some(1),
        packed: None,
        encrypted: false,
    })
    .to_vec();
    app.model.selected = [0, 1, 2].into_iter().collect();
    app.browse_folder("docs");
    assert!(app.model.selected.is_empty());
    let dialogs = TestDialogs::default();
    dialogs
        .0
        .borrow_mut()
        .answers
        .push_back(Some(vec![scratch.0.clone()]));
    app.set_dialogs(Box::new(dialogs));
    let ctx = egui::Context::default();
    policy_click(&ctx, &mut app, "retained.txt");
    assert_eq!(app.model.selected, [2].into_iter().collect());
    policy_click(&ctx, &mut app, "압축해제");
    assert!(matches!(
        factory.state.borrow().starts.last(),
        Some(Start::Extract { selection: Selection::EntryIds(ids), .. }) if ids == &vec![2]
    ));
}

#[test]
fn gzip_save_cancel_and_invalid_output_do_not_start_an_extraction() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    factory.state.borrow_mut().list_format = Some(Format::Gzip);
    let mut app = loaded(&scratch, &factory);
    let dialogs = TestDialogs::default();
    app.set_dialogs(Box::new(dialogs.clone()));
    let ctx = egui::Context::default();
    policy_click(&ctx, &mut app, "압축해제");
    assert_eq!(factory.state.borrow().starts.len(), 1);
    assert_eq!(dialogs.0.borrow().requests[0].purpose, Purpose::SaveGzip);
    dialogs
        .0
        .borrow_mut()
        .answers
        .push_back(Some(vec![scratch.0.join("invalid:name")]));
    policy_click(&ctx, &mut app, "압축해제");
    assert_eq!(factory.state.borrow().starts.len(), 1);
    assert!(!app.model.busy());
}

#[test]
fn dialog_failures_show_alerts_while_cancelled_dialogs_remain_silent() {
    use zipelo_ui_memory_app::archive_dialog::DIALOG_UNAVAILABLE;
    for failed in [false, true] {
        for purpose in [
            Purpose::Open,
            Purpose::Files,
            Purpose::Folder,
            Purpose::Destination,
            Purpose::SaveZip,
            Purpose::SaveGzip,
        ] {
            let scratch = Folder::new();
            let factory = FakeFactory::default();
            if purpose == Purpose::SaveGzip {
                factory.state.borrow_mut().list_format = Some(Format::Gzip);
            }
            let mut app = loaded(&scratch, &factory);
            let dialogs = TestDialogs::default();
            dialogs.0.borrow_mut().cancel_save = true;
            if failed {
                dialogs.0.borrow_mut().error = Some(DIALOG_UNAVAILABLE.into());
            }
            app.set_dialogs(Box::new(dialogs.clone()));
            let ctx = egui::Context::default();
            let label = match purpose {
                Purpose::Open => "열기",
                Purpose::Files | Purpose::Folder | Purpose::SaveZip => {
                    policy_click(&ctx, &mut app, "압축하기");
                    if purpose == Purpose::SaveZip {
                        let source = scratch.0.join("source.txt");
                        fs::write(&source, b"preserve").unwrap();
                        app.add_sources(vec![source]);
                    }
                    match purpose {
                        Purpose::Files => "파일 추가",
                        Purpose::Folder => "폴더 추가",
                        _ => "압축하기",
                    }
                }
                _ => "압축해제",
            };
            policy_click(&ctx, &mut app, label);
            let shown = policy_render(&ctx, &mut app, vec![]);
            assert_eq!(
                shown.iter().any(|(text, _)| text == DIALOG_UNAVAILABLE),
                failed,
                "{purpose:?}"
            );
            assert_eq!(factory.state.borrow().starts.len(), 1, "{purpose:?}");
            assert!(!app.model.busy());
            assert_eq!(dialogs.0.borrow().requests.last().unwrap().purpose, purpose);
            assert_eq!(
                shown
                    .iter()
                    .filter(|(text, _)| text == DIALOG_UNAVAILABLE)
                    .count(),
                usize::from(failed)
            );
            if failed {
                dialogs.0.borrow_mut().error = None;
                policy_click(&ctx, &mut app, label);
                let recovered = policy_render(&ctx, &mut app, vec![]);
                assert!(!recovered.iter().any(|(text, _)| text == DIALOG_UNAVAILABLE));
                assert_eq!(factory.state.borrow().starts.len(), 1);
            }
        }
    }
}

#[test]
fn empty_list_notices_remain_visible_in_the_archive_content_area() {
    for case in ["archive", "folder", "search"] {
        let scratch = Folder::new();
        let factory = FakeFactory::default();
        let mut app = loaded(&scratch, &factory);
        if case == "archive" {
            app.model.entries.clear();
        }
        if case == "folder" {
            app.model.entries[0].name = "empty".into();
            app.model.entries[0].kind = Kind::Directory;
            app.browse_folder("empty");
        }
        let ctx = egui::Context::default();
        narrow_frame(&ctx, &mut app, 0.0, vec![]);
        if case == "search" {
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::unique("archive-search")));
            narrow_frame(
                &ctx,
                &mut app,
                0.1,
                vec![egui::Event::Text("no-match".into())],
            );
        }
        for frame in 2..8 {
            narrow_frame(&ctx, &mut app, frame as f64 / 10.0, vec![]);
        }
        let output = narrow_frame(&ctx, &mut app, 1.0, vec![]);
        let expected = if case == "search" {
            "일치하는 항목이 없습니다"
        } else {
            "이 폴더는 비어 있습니다"
        };
        let shown = layout_text(&output);
        let notice = shown
            .iter()
            .find(|(text, _, _)| text == expected)
            .unwrap_or_else(|| panic!("{case}: 안내 없음"));
        assert!(
            notice.1.top() > 200.0 && notice.1.bottom() < 490.0,
            "{case}: {:?}",
            notice.1
        );
        assert_narrow_text(&output);
        assert!(shown.iter().any(|(text, _, _)| text == "0개 항목"));
        assert_eq!(factory.state.borrow().starts.len(), 1);
    }
}

#[test]
fn failed_zip_creation_shows_save_location_and_no_extraction_copy() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = ArchiveApp::new(Box::new(factory.clone()), None, false);
    app.set_dialogs(Box::new(TestDialogs::default()));
    let source = scratch.0.join("source.txt");
    fs::write(&source, b"source").unwrap();
    app.add_sources(vec![source]);
    let ctx = egui::Context::default();
    policy_click(&ctx, &mut app, "압축하기");
    for _ in 0..30 {
        policy_render(&ctx, &mut app, vec![]);
    }
    assert!(!app.model.busy());
    app.model.outputs.clear();
    app.model.terminal = Some(Terminal {
        outcome: Outcome::Failed,
        error: Some(ErrorCode::InvalidRequest),
        publication: Publication::None,
        created_count: 0,
        retained_partial_count: 0,
        durability: Durability::NotApplicable,
        cleanup: Cleanup::Complete,
    });
    let shown = policy_render(&ctx, &mut app, vec![]);
    for expected in [
        "ZIP 파일을 만들지 못했습니다",
        "저장 위치와 추가한 항목을 확인하세요",
        "ZIP 파일을 만들지 않았습니다",
    ] {
        assert!(shown.iter().any(|(text, _)| text == expected), "{shown:?}");
    }
    assert!(
        !shown
            .iter()
            .any(|(text, _)| text.contains("풀 위치") || text.contains("풀지 않았습니다"))
    );
}
