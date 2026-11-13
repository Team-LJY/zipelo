fn conflict_fixture(scratch: &Scratch) -> (PathBuf, PathBuf) {
    use worker_protocol::{archive, protocol};
    let members = ["기록.txt", "사진.txt"]
        .into_iter()
        .enumerate()
        .map(|(id, name)| archive::Member {
            entry: protocol::Entry {
                entry_id: id as u32,
                name: name.into(),
                kind: protocol::EntryKind::File,
                size: 3,
                compressed_size: 3,
                encrypted: false,
            },
            data: b"new".to_vec(),
        })
        .collect::<Vec<_>>();
    let archive = scratch.path("충돌.zip");
    fs::write(
        &archive,
        archive::create_zip(
            &members,
            protocol::Method::Store,
            protocol::Limits::default(),
            &mut || Ok(()),
        )
        .unwrap(),
    )
    .unwrap();
    let output = make_directory(&scratch.path("output"));
    for name in ["기록.txt", "사진.txt"] {
        fs::write(output.join(name), b"original").unwrap();
    }
    (archive, output)
}

impl Screen {
    fn start_conflicting_extract(&mut self, output: &Path) {
        self.app.choose_destination(output.to_path_buf());
        self.dialogs
            .0
            .borrow_mut()
            .answers
            .push_back(Some(vec![output.to_path_buf()]));
        self.click("압축해제");
        assert!(self.app.model.busy());
    }

    fn await_conflict(&mut self) -> Conflict {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.render(vec![]);
            if let Some(conflict) = &self.app.model.conflict {
                return conflict.clone();
            }
            assert!(
                self.app.model.busy(),
                "충돌 질문 전에 종료됨: {:?}",
                self.app.model.terminal
            );
            assert!(Instant::now() < deadline, "충돌 질문 시간 초과");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn settle_conflict_result(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.app.model.busy() {
            self.render(vec![]);
            assert!(Instant::now() < deadline, "충돌 결과 시간 초과");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(self.app.model.confirmed, "{}", self.app.model.status());
        assert!(self.app.model.conflict.is_none());
    }
}

#[test]
fn real_conflicts_follow_widget_choices_and_apply_remaining() {
    for (label, choice) in [
        ("덮어쓰기", ConflictChoice::Overwrite),
        ("건너뛰기", ConflictChoice::Skip),
        ("이름 바꿔 저장", ConflictChoice::Rename),
    ] {
        for apply in [false, true] {
            let scratch = Scratch::new();
            let (archive, output) = conflict_fixture(&scratch);
            let (factory, observed) = observed_factory();
            let mut screen = Screen::new(factory);
            screen.open(&archive);
            screen.start_conflicting_extract(&output);
            let first = screen.await_conflict();
            assert_eq!(first.incoming_size, 3);
            assert_eq!(first.existing_size, 8);
            if apply {
                screen.click("나머지 파일에도 같게 적용");
            }
            screen.click(label);
            if !apply {
                let second = screen.await_conflict();
                assert_ne!(first.challenge_id, second.challenge_id);
                assert_ne!(first.entry_id, second.entry_id);
                screen.click(label);
            }
            screen.settle_conflict_result();
            completed(&screen.app.model);
            assert_eq!(screen.app.model.resolutions.len(), 2);
            assert!(
                screen
                    .app
                    .model
                    .resolutions
                    .iter()
                    .all(|r| r.choice == choice)
            );
            let terminal = screen.app.model.terminal.as_ref().unwrap();
            assert_eq!(
                terminal.created_count,
                if choice == ConflictChoice::Skip { 0 } else { 2 }
            );
            assert_eq!(
                terminal.publication,
                if choice == ConflictChoice::Skip {
                    Publication::None
                } else {
                    Publication::Complete
                }
            );
            for name in ["기록.txt", "사진.txt"] {
                assert_eq!(
                    fs::read(output.join(name)).unwrap(),
                    if choice == ConflictChoice::Overwrite {
                        b"new".as_slice()
                    } else {
                        b"original".as_slice()
                    }
                );
            }
            for resolution in &screen.app.model.resolutions {
                if choice == ConflictChoice::Rename {
                    assert_eq!(
                        fs::read(output.join(&resolution.relative_name)).unwrap(),
                        b"new"
                    );
                    assert!(resolution.relative_name.contains(" (1)"));
                }
            }
            let shown = screen
                .render(vec![])
                .into_iter()
                .map(|(t, _)| t)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                shown.contains(if choice == ConflictChoice::Skip {
                    "모든 파일을 건너뛰었습니다"
                } else {
                    "압축을 풀었습니다"
                }),
                "{shown}"
            );
            assert!(!shown.contains("저장이 끝났는지"), "{shown}");
            assert_reaped(&observed);
        }
    }
}

#[test]
fn real_conflict_cancel_button_and_escape_preserve_originals() {
    for escape in [false, true] {
        let scratch = Scratch::new();
        let (archive, output) = conflict_fixture(&scratch);
        let (factory, observed) = observed_factory();
        let mut screen = Screen::new(factory);
        screen.open(&archive);
        screen.start_conflicting_extract(&output);
        screen.await_conflict();
        if escape {
            screen.escape();
        } else {
            screen.click("작업 취소");
        }
        screen.settle_conflict_result();
        assert_eq!(
            screen.app.model.terminal.as_ref().unwrap().outcome,
            Outcome::Cancelled
        );
        assert!(screen.app.model.outputs.is_empty());
        for name in ["기록.txt", "사진.txt"] {
            assert_eq!(fs::read(output.join(name)).unwrap(), b"original");
        }
        assert_reaped(&observed);
    }
}

#[test]
fn target_changed_during_real_question_fails_without_saved_name_notice() {
    let scratch = Scratch::new();
    let (archive, output) = conflict_fixture(&scratch);
    let (factory, observed) = observed_factory();
    let mut screen = Screen::new(factory);
    screen.open(&archive);
    screen.start_conflicting_extract(&output);
    let conflict = screen.await_conflict();
    fs::write(
        output.join(&conflict.relative_name),
        b"changed after question",
    )
    .unwrap();
    screen.click("덮어쓰기");
    screen.settle_conflict_result();
    assert_eq!(
        screen.app.model.terminal.as_ref().unwrap().outcome,
        Outcome::Failed
    );
    assert_eq!(screen.app.model.terminal.as_ref().unwrap().created_count, 0);
    assert!(screen.app.model.outputs.is_empty());
    assert_eq!(
        fs::read(output.join(&conflict.relative_name)).unwrap(),
        b"changed after question"
    );
    let shown = screen
        .render(vec![])
        .into_iter()
        .map(|(t, _)| t)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(shown.contains("압축을 풀지 못했습니다"), "{shown}");
    assert!(!shown.contains("이름 바꿔 저장:"), "{shown}");
    assert_reaped(&observed);
}

#[test]
fn approved_extract_continues_to_conflict_choices() {
    let scratch = Scratch::new();
    let (archive, output) = conflict_fixture(&scratch);
    let (factory, observed) = observed_factory();
    let mut screen = Screen::new(factory);
    screen.open(&archive);
    screen.app.model.entries[0].bytes = Some(17 * 1024 * 1024 * 1024);
    screen.start_conflicting_extract(&output);
    let deadline = Instant::now() + Duration::from_secs(10);
    while screen.app.model.resource.is_none() {
        screen.render(vec![]);
        assert!(screen.app.model.busy(), "{:?}", screen.app.model.error);
        assert!(Instant::now() < deadline, "승인 질문 시간 초과");
        std::thread::sleep(Duration::from_millis(2));
    }
    let approval = screen.app.model.resource.clone().unwrap();
    assert!(screen.app.model.conflict.is_none());
    screen.click("이 작업 허용");
    let conflict = screen.await_conflict();
    assert!(conflict.challenge_id > approval.id);
    assert!(screen.app.model.resource.is_none());
    screen.click("나머지 파일에도 같게 적용");
    screen.click("덮어쓰기");
    screen.settle_conflict_result();
    completed(&screen.app.model);
    assert_eq!(screen.app.model.resolutions.len(), 2);
    for name in ["기록.txt", "사진.txt"] {
        assert_eq!(fs::read(output.join(name)).unwrap(), b"new");
    }
    assert_reaped(&observed);
}
