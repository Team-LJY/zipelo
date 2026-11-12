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
fn encryption_tool_opens_protected_zip_modal_without_starting_work() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    let ctx = egui::Context::default();
    policy_click(&ctx, &mut app, "암호화");
    let shown = policy_render(&ctx, &mut app, vec![]);
    assert!(shown.iter().any(|(text, _)| text == "ZIP 만들기"));
    assert!(shown.iter().any(|(text, _)| text == "암호로 보호하기"));
    policy_click(&ctx, &mut app, "취소");
    assert_eq!(factory.state.borrow().starts.len(), 1);
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
