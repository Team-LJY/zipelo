use super::*;
use std::{
    collections::BTreeSet,
    fs,
    sync::atomic::{AtomicU64, Ordering},
};
use zipelo_ui_memory_app::archive_present::*;

fn terminal() -> Terminal {
    Terminal {
        outcome: Outcome::Failed,
        error: Some(ErrorCode::Io),
        publication: Publication::Partial,
        created_count: 1,
        retained_partial_count: 0,
        durability: Durability::Synced,
        cleanup: Cleanup::Complete,
    }
}
fn targets() -> BTreeSet<String> {
    ["one.txt".into(), "two.txt".into()].into_iter().collect()
}

#[test]
fn partial_extraction_requires_matching_file_reports() {
    let t = terminal();
    assert_eq!(
        warnings(
            Some(&t),
            true,
            Some(&targets()),
            &BTreeSet::new(),
            &[("one.txt".into(), Disposition::Created)]
        ),
        vec![PARTIAL]
    );
}
#[test]
fn all_saved_with_temporary_leftover_is_not_partial_extraction() {
    let mut t = terminal();
    t.retained_partial_count = 1;
    t.cleanup = Cleanup::Failed;
    let files = ["one.txt".into()].into_iter().collect();
    assert_eq!(
        warnings(
            Some(&t),
            true,
            Some(&files),
            &BTreeSet::new(),
            &[
                ("one.txt".into(), Disposition::Created),
                ("temp".into(), Disposition::PartialRetained)
            ]
        ),
        vec![LEFTOVERS]
    );
}
#[test]
fn leftover_without_file_reports_and_commit_uncertainty_are_unknown() {
    let mut t = terminal();
    t.created_count = 0;
    t.retained_partial_count = 1;
    for outputs in [
        vec![("temp".into(), Disposition::PartialRetained)],
        vec![("one.txt".into(), Disposition::CommitUncertain)],
    ] {
        assert!(
            warnings(Some(&t), true, Some(&targets()), &BTreeSet::new(), &outputs)
                .contains(&UNCERTAIN)
        );
        assert!(
            !warnings(Some(&t), true, Some(&targets()), &BTreeSet::new(), &outputs)
                .contains(&PARTIAL)
        );
    }
}
#[test]
fn synchronization_and_cleanup_warnings_are_both_visible() {
    let mut t = terminal();
    t.created_count = 0;
    t.durability = Durability::Unconfirmed;
    t.cleanup = Cleanup::Failed;
    let result = warnings(Some(&t), true, Some(&targets()), &BTreeSet::new(), &[]);
    assert!(result.contains(&SYNC_WARNING));
    assert!(result.contains(&CLEANUP_WARNING));
}
#[test]
fn missing_terminal_exit_or_contradictory_reports_never_claim_unsaved() {
    let mut t = terminal();
    t.publication = Publication::None;
    t.created_count = 0;
    assert!(definitely_unsaved(Some(&t), true, &[]));
    assert!(!definitely_unsaved(None, true, &[]));
    assert!(!definitely_unsaved(Some(&t), false, &[]));
    let reports = [("one.txt".into(), Disposition::Created)];
    assert!(!definitely_unsaved(Some(&t), true, &reports));
    assert!(
        warnings(Some(&t), true, Some(&targets()), &BTreeSet::new(), &reports).contains(&UNCERTAIN)
    );
}
#[test]
fn unsafe_path_and_conflict_preserve_result_classification() {
    for code in [ErrorCode::UnsafePath, ErrorCode::Conflict] {
        let mut t = terminal();
        t.error = Some(code);
        assert_eq!(
            warnings(
                Some(&t),
                true,
                Some(&targets()),
                &BTreeSet::new(),
                &[("one.txt".into(), Disposition::Created)]
            ),
            vec![PARTIAL]
        );
        t.publication = Publication::Uncertain;
        assert!(
            warnings(Some(&t), true, Some(&targets()), &BTreeSet::new(), &[]).contains(&UNCERTAIN)
        );
        t.publication = Publication::None;
        t.created_count = 0;
        assert!(definitely_unsaved(Some(&t), true, &[]));
    }
}
#[test]
fn every_error_and_stage_has_user_facing_text() {
    for code in [
        ErrorCode::Integrity,
        ErrorCode::UnsupportedFormat,
        ErrorCode::UnsupportedFeature,
        ErrorCode::WrongPassword,
        ErrorCode::UnsafePath,
        ErrorCode::Conflict,
        ErrorCode::Limit,
        ErrorCode::Io,
        ErrorCode::DiskFull,
        ErrorCode::Protocol,
        ErrorCode::Internal,
        ErrorCode::SourceChanged,
        ErrorCode::InvalidRequest,
    ] {
        assert!(!error_text(code).is_empty());
        assert!(!error_text(code).contains("작업자"));
    }
    assert_eq!(error_text(ErrorCode::Protocol), INTERNAL);
    assert_eq!(error_text(ErrorCode::Internal), INTERNAL);
    assert_eq!(progress_text(Stage::Decode, true), "압축하는 중");
    assert_eq!(progress_text(Stage::Decode, false), "압축을 푸는 중");
}
#[test]
fn file_targets_follow_requested_ids_without_expanding_directories() {
    let entries = vec![
        Entry {
            id: 0,
            name: "docs".into(),
            kind: Kind::Directory,
            bytes: None,
            packed: None,
            encrypted: false,
        },
        Entry {
            id: 1,
            name: "docs/one.txt".into(),
            kind: Kind::File,
            bytes: None,
            packed: None,
            encrypted: false,
        },
        Entry {
            id: 2,
            name: "docs-old/two.txt".into(),
            kind: Kind::File,
            bytes: None,
            packed: None,
            encrypted: false,
        },
    ];
    assert_eq!(
        target_files(&entries, &Selection::EntryIds(vec![0]), None),
        Some(BTreeSet::new())
    );
    assert_eq!(
        target_files(&entries, &Selection::EntryIds(vec![0, 2]), None),
        Some(["docs-old/two.txt".into()].into_iter().collect())
    );
    assert_eq!(
        target_files(&entries, &Selection::EntryIds(vec![1]), None),
        Some(["docs/one.txt".into()].into_iter().collect())
    );
    assert_eq!(
        target_files(&entries, &Selection::All, None),
        Some(
            ["docs/one.txt".into(), "docs-old/two.txt".into()]
                .into_iter()
                .collect()
        )
    );
}

#[test]
fn directory_reports_allow_empty_and_mixed_archives_without_hiding_unknown_outputs() {
    let directories = ["parent".into(), "parent/empty".into()]
        .into_iter()
        .collect();
    let empty = BTreeSet::new();
    let mut t = terminal();
    t.outcome = Outcome::Completed;
    t.error = None;
    t.publication = Publication::Complete;
    t.created_count = 2;
    let reports = [
        ("parent".into(), Disposition::Created),
        ("parent/empty".into(), Disposition::Created),
    ];
    assert!(warnings(Some(&t), true, Some(&empty), &directories, &reports).is_empty());
    t.created_count = 0;
    assert!(warnings(Some(&t), true, Some(&empty), &directories, &[]).is_empty());
    let files = ["parent/file.txt".into()].into_iter().collect();
    t.created_count = 2;
    let reports = [
        ("parent".into(), Disposition::Created),
        ("parent/file.txt".into(), Disposition::Created),
    ];
    assert!(warnings(Some(&t), true, Some(&files), &directories, &reports).is_empty());
    t.created_count = 1;
    assert!(
        warnings(
            Some(&t),
            true,
            Some(&files),
            &directories,
            &[("parent".into(), Disposition::Created)]
        )
        .contains(&UNCERTAIN)
    );
    assert!(
        warnings(
            Some(&t),
            true,
            Some(&empty),
            &directories,
            &[("unknown".into(), Disposition::Created)]
        )
        .contains(&UNCERTAIN)
    );
    t.publication = Publication::Uncertain;
    assert!(
        warnings(
            Some(&t),
            true,
            Some(&empty),
            &directories,
            &[("parent".into(), Disposition::CommitUncertain)]
        )
        .contains(&UNCERTAIN)
    );
}

#[test]
fn directory_targets_include_selected_members_and_implicit_parents() {
    let entries = [
        Entry {
            id: 0,
            name: "top".into(),
            kind: Kind::Directory,
            bytes: None,
            packed: None,
            encrypted: false,
        },
        Entry {
            id: 1,
            name: "top/empty".into(),
            kind: Kind::Directory,
            bytes: None,
            packed: None,
            encrypted: false,
        },
        Entry {
            id: 2,
            name: "top/nested/file.txt".into(),
            kind: Kind::File,
            bytes: None,
            packed: None,
            encrypted: false,
        },
        Entry {
            id: 3,
            name: "other/empty".into(),
            kind: Kind::Directory,
            bytes: None,
            packed: None,
            encrypted: false,
        },
    ];
    assert_eq!(
        target_directories(&entries, &Selection::EntryIds(vec![0])),
        ["top".into()].into_iter().collect()
    );
    assert_eq!(
        target_directories(&entries, &Selection::EntryIds(vec![2])),
        ["top".into(), "top/nested".into()].into_iter().collect()
    );
}

#[test]
fn duplicate_names_prevent_file_completion_assumptions() {
    let entry = Entry {
        id: 0,
        name: "same.txt".into(),
        kind: Kind::File,
        bytes: None,
        packed: None,
        encrypted: false,
    };
    assert!(target_files(&[entry.clone(), entry], &Selection::All, None).is_none());
    assert_eq!(
        warnings(Some(&terminal()), true, None, &BTreeSet::new(), &[]),
        vec![UNCERTAIN]
    );
}
#[test]
fn default_names_and_middle_truncation_preserve_unicode() {
    assert_eq!(default_destination(&path("자료.tar.gz")), path("자료"));
    assert_eq!(default_destination(&path("자료.zip")), path("자료"));
    assert_eq!(middle_text("가나다라마바사아자차", 7), "가나다…아자차");
    assert_eq!(bytes_text(21), "21 B");
}

static SCRATCH_ID: AtomicU64 = AtomicU64::new(0);
pub(super) struct Folder(pub(super) PathBuf);
impl Folder {
    pub(super) fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/rust-app-ui/runs");
        fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "presentation-{}-{}",
            std::process::id(),
            SCRATCH_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn automatic_destination_creation_and_failure_are_distinct() {
    let scratch = Folder::new();
    let output = scratch.0.join("output");
    assert!(prepare_destination(&output, true).unwrap());
    assert!(!prepare_destination(&output, true).unwrap());
    assert!(prepare_destination(&scratch.0.join("missing/child"), true).is_err());
    assert!(prepare_destination(&scratch.0.join("chosen-missing"), false).is_err());
    let file = scratch.0.join("file");
    fs::write(&file, "existing").unwrap();
    assert!(prepare_destination(&file, true).is_err());
    assert_eq!(fs::read_to_string(file).unwrap(), "existing");
}
#[test]
fn only_new_empty_output_is_removed_and_nonempty_is_preserved() {
    let scratch = Folder::new();
    let empty = scratch.0.join("empty");
    assert!(prepare_destination(&empty, true).unwrap());
    remove_owned_empty(&empty);
    assert!(!empty.exists());
    let nonempty = scratch.0.join("nonempty");
    assert!(prepare_destination(&nonempty, true).unwrap());
    fs::write(nonempty.join("existing"), "keep").unwrap();
    remove_owned_empty(&nonempty);
    assert_eq!(
        fs::read_to_string(nonempty.join("existing")).unwrap(),
        "keep"
    );
    assert!(!prepare_destination(&scratch.0, false).unwrap());
    assert!(scratch.0.is_dir());
}
#[cfg(unix)]
#[test]
fn browser_and_output_do_not_follow_symbolic_links() {
    let scratch = Folder::new();
    let link = scratch.0.join("link");
    std::os::unix::fs::symlink(&scratch.0, &link).unwrap();
    let rows = directory_entries(&scratch.0).unwrap();
    assert!(rows.iter().any(|entry| entry.link && entry.path == link));
    assert!(directory_entries(&link).is_err());
    assert!(prepare_destination(&link, true).is_err());
}

#[cfg(feature = "ui")]
mod widgets {
    use super::*;
    use crate::dialogs::TestDialogs;
    use eframe::egui::{self, epaint::Shape};
    use zipelo_ui_memory_app::{archive_dialog::Purpose, archive_screen::ArchiveApp};

    fn labels(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Pos2)> {
        fn walk(shape: &Shape, result: &mut Vec<(String, egui::Pos2)>) {
            match shape {
                Shape::Text(text) => result.push((
                    text.galley.job.text.clone(),
                    text.pos + text.galley.rect.size() / 2.0,
                )),
                Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, result);
                    }
                }
                _ => {}
            }
        }
        let mut result = Vec::new();
        for shape in shapes {
            walk(&shape.shape, &mut result);
        }
        result
    }

    fn render(
        ctx: &egui::Context,
        app: &mut ArchiveApp,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Pos2)> {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(760.0, 540.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.show(ui),
        );
        output.textures_delta.clear();
        labels(&output.shapes)
    }

    fn click(ctx: &egui::Context, app: &mut ArchiveApp, label: &str) {
        render(ctx, app, vec![]);
        let point = render(ctx, app, vec![])
            .into_iter()
            .filter(|(text, _)| text == label)
            .max_by(|a, b| a.1.y.total_cmp(&b.1.y))
            .unwrap_or_else(|| panic!("항목 없음: {label}"))
            .1;
        for pressed in [true, false] {
            render(
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

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn text(ctx: &egui::Context, app: &mut ArchiveApp, id: &str, value: &str) {
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::unique(id)));
        render(
            ctx,
            app,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                egui::Event::Text(value.into()),
            ],
        );
    }

    fn loaded(scratch: &Folder, factory: &FakeFactory) -> ArchiveApp {
        let mut app = ArchiveApp::new(
            Box::new(factory.clone()),
            Some(scratch.0.join("sample.zip")),
            false,
        );
        pump(&mut app.model);
        app.set_dialogs(Box::new(TestDialogs::default()));
        app
    }

    fn policy_render(
        ctx: &egui::Context,
        app: &mut ArchiveApp,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Pos2)> {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1060.0, 720.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.show(ui),
        );
        output.textures_delta.clear();
        labels(&output.shapes)
    }

    fn policy_focus(ctx: &egui::Context, app: &mut ArchiveApp, label: &str) {
        policy_render(ctx, app, vec![]);
        for _ in 0..100 {
            let shown = policy_render(ctx, app, vec![]);
            let point = shown
                .iter()
                .rev()
                .find(|(text, _)| text == label)
                .unwrap_or_else(|| panic!("표시 없음: {label}"))
                .1;
            if ctx
                .memory(|memory| memory.focused())
                .and_then(|id| ctx.read_response(id))
                .is_some_and(|response| response.rect.contains(point))
            {
                return;
            }
            policy_render(ctx, app, vec![key(egui::Key::Tab)]);
            let mut released = key(egui::Key::Tab);
            if let egui::Event::Key { pressed, .. } = &mut released {
                *pressed = false;
            }
            policy_render(ctx, app, vec![released]);
        }
        panic!("키보드 초점 없음: {label}");
    }

    fn policy_click(ctx: &egui::Context, app: &mut ArchiveApp, label: &str) {
        policy_render(ctx, app, vec![]);
        let point = policy_render(ctx, app, vec![])
            .iter()
            .rev()
            .find(|(text, _)| text == label)
            .unwrap()
            .1;
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

    #[test]
    fn destination_failure_sends_no_request_and_explains_folder_change() {
        let scratch = Folder::new();
        let factory = FakeFactory::default();
        let mut app = loaded(&scratch, &factory);
        let file = scratch.0.join("existing.txt");
        fs::write(&file, "preserve").unwrap();
        app.choose_destination(file.clone());
        assert!(!app.begin_extract());
        assert_eq!(factory.state.borrow().starts.len(), 1);
        let ctx = egui::Context::default();
        render(&ctx, &mut app, vec![]);
        assert!(
            render(&ctx, &mut app, vec![])
                .iter()
                .any(|(text, _)| text == FOLDER_ERROR)
        );
        assert_eq!(fs::read_to_string(file).unwrap(), "preserve");
    }

    #[test]
    fn cancelling_removes_only_automatically_created_empty_folder() {
        for case in ["automatic-empty", "automatic-nonempty", "chosen-empty"] {
            let scratch = Folder::new();
            let factory = FakeFactory::default();
            let mut app = loaded(&scratch, &factory);
            factory.state.borrow_mut().manual = true;
            let destination = if case == "chosen-empty" {
                let path = scratch.0.join("chosen");
                fs::create_dir(&path).unwrap();
                app.choose_destination(path.clone());
                path
            } else {
                scratch.0.join("sample")
            };
            assert!(app.begin_extract());
            if case == "automatic-nonempty" {
                fs::write(destination.join("preserve"), "keep").unwrap();
            }
            {
                let mut state = factory.state.borrow_mut();
                state.frame(Event::Accepted { encryption: true });
                state.finish(Outcome::Cancelled, Publication::None);
            }
            let ctx = egui::Context::default();
            for _ in 0..30 {
                render(&ctx, &mut app, vec![]);
            }
            assert!(!app.model.busy());
            assert_eq!(destination.exists(), case != "automatic-empty");
        }
    }

    #[test]
    fn password_secondary_enter_does_not_submit_valid_input() {
        for encrypt in [false, true] {
            for label in ["보기", "숨기기", "취소"] {
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
                        encrypt,
                    });
                }
                let ctx = egui::Context::default();
                for _ in 0..12 {
                    policy_render(&ctx, &mut app, vec![]);
                }
                text(&ctx, &mut app, "archive-password-input", "fixture-secret");
                if encrypt {
                    text(&ctx, &mut app, "archive-password-confirm", "fixture-secret");
                }
                if label == "숨기기" {
                    policy_click(&ctx, &mut app, "보기");
                }
                policy_focus(&ctx, &mut app, label);
                policy_render(&ctx, &mut app, vec![key(egui::Key::Enter)]);
                assert!(!factory.state.borrow().password_seen, "{label}");
                assert_eq!(factory.state.borrow().starts.len(), 1);
            }
        }
    }

    #[test]
    fn directory_only_extraction_displays_success() {
        let scratch = Folder::new();
        let factory = FakeFactory::default();
        let mut app = loaded(&scratch, &factory);
        app.model.entries = vec![Entry {
            id: 0,
            name: "empty".into(),
            kind: Kind::Directory,
            bytes: Some(0),
            packed: None,
            encrypted: false,
        }];
        factory.state.borrow_mut().manual = true;
        assert!(app.begin_extract());
        {
            let mut worker = factory.state.borrow_mut();
            worker.frame(Event::Accepted { encryption: false });
            worker.frame(Event::Output {
                entry_id: None,
                relative_name: "empty".into(),
                disposition: Disposition::Created,
            });
            worker.finish(Outcome::Completed, Publication::Complete);
        }
        let ctx = egui::Context::default();
        for _ in 0..30 {
            policy_render(&ctx, &mut app, vec![]);
        }
        let shown = policy_render(&ctx, &mut app, vec![]);
        assert!(shown.iter().any(|(text, _)| text == "압축을 풀었습니다"));
        assert!(!shown.iter().any(|(text, _)| text == UNCERTAIN));
    }

    #[test]
    fn completed_results_keep_folder_action_and_distinguish_durability() {
        for creating in [false, true] {
            for durability in [
                Durability::Synced,
                Durability::Unconfirmed,
                Durability::Failed,
            ] {
                let scratch = Folder::new();
                let factory = FakeFactory::default();
                let mut app = loaded(&scratch, &factory);
                factory.state.borrow_mut().manual = true;
                let ctx = egui::Context::default();
                let name = if creating {
                    let source = scratch.0.join("source.txt");
                    fs::write(&source, b"source").unwrap();
                    app.add_sources(vec![source]);
                    policy_render(&ctx, &mut app, vec![]);
                    policy_click(&ctx, &mut app, "압축하기");
                    match factory.state.borrow().starts.last().unwrap() {
                        Start::CreateZip { output_file, .. } => output_file
                            .file_name()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .to_owned(),
                        _ => panic!("생성 요청 누락"),
                    }
                } else {
                    assert!(app.begin_extract());
                    "읽어주세요.txt".to_owned()
                };
                {
                    let mut worker = factory.state.borrow_mut();
                    worker.frame(Event::Accepted { encryption: false });
                    worker.frame(Event::Output {
                        entry_id: None,
                        relative_name: name,
                        disposition: Disposition::Created,
                    });
                    worker.finish(Outcome::Completed, Publication::Complete);
                    for event in &mut worker.queue {
                        if let HostEvent::Frame {
                            event: Event::Terminal(terminal),
                            ..
                        } = event
                        {
                            terminal.durability = durability;
                        }
                    }
                }
                for _ in 0..30 {
                    policy_render(&ctx, &mut app, vec![]);
                }
                let shown = policy_render(&ctx, &mut app, vec![]);
                let title = if creating {
                    "ZIP 파일을 만들었습니다"
                } else {
                    "압축을 풀었습니다"
                };
                assert!(
                    shown.iter().any(|(text, _)| text == title),
                    "{durability:?}: {shown:?}"
                );
                assert!(shown.iter().any(|(text, _)| text == "폴더 열기"));
                assert!(!shown.iter().any(|(text, _)| text == "다시 시도"));
                assert!(!shown.iter().any(|(text, _)| text == SYNC_WARNING));
                let sync_failed = if creating {
                    "파일을 디스크에 완전히 저장하지 못했습니다. 저장 위치 폴더를 확인하세요."
                } else {
                    "파일을 디스크에 완전히 저장하지 못했습니다. 풀 위치 폴더를 확인하세요."
                };
                assert_eq!(
                    shown.iter().any(|(text, _)| text == sync_failed),
                    durability == Durability::Failed
                );
                policy_click(&ctx, &mut app, "자세히 보기");
                let details = policy_render(&ctx, &mut app, vec![]);
                assert_eq!(
                    details.iter().any(|(text, _)| text == SYNC_WARNING),
                    durability == Durability::Unconfirmed
                );
                policy_click(&ctx, &mut app, "자세히 닫기");
                let closed = policy_render(&ctx, &mut app, vec![]);
                assert!(!closed.iter().any(|(text, _)| text == SYNC_WARNING));
                assert_eq!(
                    closed.iter().any(|(text, _)| text == sync_failed),
                    durability == Durability::Failed
                );
                assert!(closed.iter().any(|(text, _)| text == "폴더 열기"));
            }
        }
    }

    #[test]
    fn durability_warning_visibility_matches_completed_failed_and_partial_results() {
        for (outcome, publication, saved, expected_files) in [
            (Outcome::Completed, Publication::Complete, 1, 1),
            (Outcome::Failed, Publication::None, 0, 1),
            (Outcome::Failed, Publication::Partial, 1, 2),
        ] {
            for durability in [
                Durability::Synced,
                Durability::Unconfirmed,
                Durability::Failed,
            ] {
                let scratch = Folder::new();
                let factory = FakeFactory::default();
                let mut app = loaded(&scratch, &factory);
                app.model.entries = (0..expected_files)
                    .map(|id| Entry {
                        id,
                        name: format!("{id}.txt"),
                        kind: Kind::File,
                        bytes: Some(1),
                        packed: None,
                        encrypted: false,
                    })
                    .collect();
                factory.state.borrow_mut().manual = true;
                assert!(app.begin_extract());
                {
                    let mut worker = factory.state.borrow_mut();
                    worker.frame(Event::Accepted { encryption: false });
                    if saved > 0 {
                        worker.frame(Event::Output {
                            entry_id: Some(0),
                            relative_name: "0.txt".into(),
                            disposition: Disposition::Created,
                        });
                    }
                    worker.finish(outcome, publication);
                    for event in &mut worker.queue {
                        if let HostEvent::Frame {
                            event: Event::Terminal(terminal),
                            ..
                        } = event
                        {
                            terminal.created_count = saved;
                            terminal.durability = durability;
                            terminal.error = (outcome == Outcome::Failed).then_some(ErrorCode::Io);
                        }
                    }
                }
                let ctx = egui::Context::default();
                for _ in 0..30 {
                    policy_render(&ctx, &mut app, vec![]);
                }
                let completed = outcome == Outcome::Completed;
                let shown = policy_render(&ctx, &mut app, vec![]);
                let warning = match durability {
                    Durability::Unconfirmed => Some(SYNC_WARNING),
                    Durability::Failed => Some(
                        "파일을 디스크에 완전히 저장하지 못했습니다. 풀 위치 폴더를 확인하세요.",
                    ),
                    _ => None,
                };
                let visible =
                    warning.is_some() && !(completed && durability == Durability::Unconfirmed);
                assert_eq!(
                    shown.iter().any(|(text, _)| Some(text.as_str()) == warning),
                    visible,
                    "{outcome:?}/{publication:?}: {shown:?}"
                );
                assert_eq!(shown.iter().any(|(text, _)| text == "폴더 열기"), completed);
                assert_eq!(
                    shown.iter().any(|(text, _)| text == "압축을 풀었습니다"),
                    completed
                );
                assert_eq!(
                    shown
                        .iter()
                        .any(|(text, _)| text == "압축을 풀지 못했습니다"),
                    !completed
                );
                assert_eq!(
                    shown.iter().any(|(text, _)| text == PARTIAL),
                    publication == Publication::Partial
                );
                policy_click(&ctx, &mut app, "자세히 보기");
                let details = policy_render(&ctx, &mut app, vec![]);
                assert_eq!(
                    details
                        .iter()
                        .filter(|(text, _)| Some(text.as_str()) == warning)
                        .count(),
                    usize::from(warning.is_some()),
                    "{outcome:?}/{publication:?}: {details:?}"
                );
                policy_click(&ctx, &mut app, "자세히 닫기");
                let closed = policy_render(&ctx, &mut app, vec![]);
                assert_eq!(
                    closed
                        .iter()
                        .any(|(text, _)| Some(text.as_str()) == warning),
                    visible
                );
            }
        }
    }
    include!("toolbar.rs");
    include!("long_layout.rs");
    include!("explorer_selection.rs");
    include!("window_frame.rs");
}

#[cfg(feature = "ui")]
#[test]
fn create_dialog_keeps_title_and_actions_visible_after_resize() {
    use eframe::egui::{self, epaint::Shape};
    use zipelo_ui_memory_app::archive_screen::ArchiveApp;
    let scratch = Folder::new();
    let mut app = ArchiveApp::new(Box::new(FakeFactory::default()), None, false);
    fs::write(scratch.0.join("one.txt"), b"fixture").unwrap();
    fs::create_dir(scratch.0.join("photos")).unwrap();
    app.add_sources(vec![scratch.0.join("one.txt"), scratch.0.join("photos")]);
    let ctx = egui::Context::default();
    for size in [egui::vec2(1060.0, 720.0), egui::vec2(760.0, 540.0)] {
        let mut texts = Vec::new();
        for _ in 0..8 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ui| app.show(ui),
            );
            output.textures_delta.clear();
            fn walk(shape: &Shape, output: &mut Vec<(String, egui::Pos2)>) {
                match shape {
                    Shape::Text(text) => output.push((
                        text.galley.job.text.clone(),
                        text.pos + text.galley.rect.size() / 2.0,
                    )),
                    Shape::Vec(shapes) => {
                        for shape in shapes {
                            walk(shape, output);
                        }
                    }
                    _ => {}
                }
            }
            texts.clear();
            for shape in &output.shapes {
                walk(&shape.shape, &mut texts);
            }
        }
        for label in ["ZIP 만들기", "압축하기", "취소"] {
            let positions = texts
                .iter()
                .filter(|(text, _)| text == label)
                .map(|(_, p)| p)
                .collect::<Vec<_>>();
            assert!(!positions.is_empty(), "{label}");
            assert!(
                positions.iter().all(|p| p.y >= 0.0 && p.y <= size.y),
                "{label}: {positions:?}"
            );
        }
    }
}
#[test]
fn folder_open_command_preserves_shell_characters_as_one_argument() {
    let path = path("한글 폴더 ; $(touch unexpected)");
    let command = folder_command(&path);
    assert_eq!(
        command.get_program(),
        if cfg!(windows) {
            "explorer"
        } else {
            "xdg-open"
        }
    );
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        vec![path.as_os_str()]
    );
}

#[test]
fn late_cancel_and_terminal_wait_use_finishing_text() {
    let mut model = Model::default();
    model.progress = Some((Stage::Decode, 1, Some(2), 0));
    assert_eq!(activity_text(&model, false), "압축을 푸는 중");
    model.cancel = Some(CancelStatus::Requested);
    assert_eq!(activity_text(&model, false), "취소하는 중...");
    for state in [CancelStatus::TooLate, CancelStatus::Finished] {
        model.cancel = Some(state);
        assert_eq!(activity_text(&model, false), "마무리하는 중");
    }
    model.cancel = Some(CancelStatus::Requested);
    model.terminal = Some(terminal());
    assert_eq!(activity_text(&model, false), "마무리하는 중");
}
#[test]
fn client_protocol_errors_hide_internal_terms_but_preserve_exit_uncertainty() {
    for error in [
        "작업 ID 한도에 도달했습니다",
        "작업자 응답 순서나 작업 ID가 일치하지 않습니다",
        "작업 승인 응답이 없습니다",
        "출력 보고 한도 초과",
    ] {
        assert_eq!(client_error_text(error), INTERNAL);
    }
    assert_eq!(
        client_error_text("작업자 종료 결과를 확인할 수 없습니다. 출력 상태를 확인하세요"),
        UNCONFIRMED_FINISH
    );
}
