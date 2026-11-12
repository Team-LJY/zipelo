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
        assert!(warnings(Some(&t), true, Some(&targets()), &outputs).contains(&UNCERTAIN));
        assert!(!warnings(Some(&t), true, Some(&targets()), &outputs).contains(&PARTIAL));
    }
}
#[test]
fn synchronization_and_cleanup_warnings_are_both_visible() {
    let mut t = terminal();
    t.created_count = 0;
    t.durability = Durability::Unconfirmed;
    t.cleanup = Cleanup::Failed;
    let result = warnings(Some(&t), true, Some(&targets()), &[]);
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
    assert!(warnings(Some(&t), true, Some(&targets()), &reports).contains(&UNCERTAIN));
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
                &[("one.txt".into(), Disposition::Created)]
            ),
            vec![PARTIAL]
        );
        t.publication = Publication::Uncertain;
        assert!(warnings(Some(&t), true, Some(&targets()), &[]).contains(&UNCERTAIN));
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
fn folder_selection_expands_only_matching_descendants() {
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
        target_files(&entries, &[0].into_iter().collect(), true, None),
        Some(["docs/one.txt".into()].into_iter().collect())
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
    assert!(target_files(&[entry.clone(), entry], &BTreeSet::new(), false, None).is_none());
    assert_eq!(
        warnings(Some(&terminal()), true, None, &[]),
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
    use eframe::egui::{self, epaint::Shape};
    use zipelo_ui_memory_app::{
        archive_picker::{Choice, Picker, Purpose},
        archive_screen::ArchiveApp,
    };

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
        app
    }
    #[test]
    fn empty_view_has_no_internal_status_and_keyboard_focus_is_reachable() {
        let ctx = egui::Context::default();
        let mut app = ArchiveApp::new(Box::new(FakeFactory::default()), None, false);
        render(&ctx, &mut app, vec![]);
        let shown = render(&ctx, &mut app, vec![key(egui::Key::Tab)]);
        assert!(
            shown
                .iter()
                .any(|(text, _)| text == "압축파일을 여기에 끌어 놓으세요")
        );
        assert!(!shown.iter().any(|(text, _)| text.contains("게시")
            || text.contains("동기화")
            || text.contains("시험 모드")));
        assert!(ctx.memory(|memory| memory.focused().is_some()));
        app.model.request_close();
        let closing = render(&ctx, &mut app, vec![]);
        assert!(!closing.iter().any(|(text, _)| text == UNCERTAIN));
    }
    #[test]
    fn empty_selection_and_empty_zip_sources_do_not_start_work() {
        let scratch = Folder::new();
        let ctx = egui::Context::default();
        let factory = FakeFactory::default();
        let mut app = loaded(&scratch, &factory);
        render(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, "선택한 항목만 풀기");
        click(&ctx, &mut app, "풀기");
        assert_eq!(factory.state.borrow().starts.len(), 1);
        click(&ctx, &mut app, "ZIP 만들기");
        click(&ctx, &mut app, "만들기");
        assert_eq!(factory.state.borrow().starts.len(), 1);
        assert!(!app.model.busy());
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
    fn password_confirmation_requires_match_and_enter_sends_once() {
        let scratch = Folder::new();
        let factory = FakeFactory::manual();
        let mut app = ArchiveApp::new(Box::new(factory.clone()), None, false);
        assert!(app.model.begin(
            Start::CreateZip {
                sources: vec![(scratch.0.join("source"), "source".into())],
                output_file: scratch.0.join("new.zip"),
                method: Method::Deflate,
                aes256: true
            },
            &mut factory.clone()
        ));
        {
            let mut state = factory.state.borrow_mut();
            state.frame(Event::Accepted { encryption: true });
            state.frame(Event::PasswordRequired {
                challenge_id: 17,
                attempt: 1,
                encrypt: true,
            });
        }
        let ctx = egui::Context::default();
        for _ in 0..12 {
            render(&ctx, &mut app, vec![]);
        }
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(egui::Id::unique("archive-password-input"))
        );
        text(&ctx, &mut app, "archive-password-input", "fixture-secret");
        text(&ctx, &mut app, "archive-password-confirm", "different");
        click(&ctx, &mut app, "확인");
        assert!(!factory.state.borrow().password_seen);
        text(&ctx, &mut app, "archive-password-confirm", "fixture-secret");
        let shown = render(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        assert!(
            !shown
                .iter()
                .any(|(text, _)| text.contains("fixture-secret"))
        );
        assert!(factory.state.borrow().password_seen);
        assert!(app.model.challenge.is_none());
    }
    #[test]
    fn picker_fits_minimum_window_and_opens_selected_file_with_enter() {
        let scratch = Folder::new();
        let file = scratch.0.join("one.zip");
        fs::write(&file, "fixture").unwrap();
        let ctx = egui::Context::default();
        let mut picker = Picker::new(Purpose::Open, &scratch.0);
        let mut result = None;
        let mut draw = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(760.0, 540.0),
                    )),
                    events,
                    ..Default::default()
                },
                |_ui| {
                    result = picker.show(&ctx);
                },
            );
            output.textures_delta.clear();
            labels(&output.shapes)
        };
        draw(vec![]);
        let shown = draw(vec![]);
        for label in ["파일 열기", "열기", "홈"] {
            let (_, point) = shown.iter().find(|(text, _)| text == label).unwrap();
            assert!((0.0..540.0).contains(&point.y), "{label}: {point:?}");
        }
        let cancel = shown.iter().find(|(text, _)| text == "취소").unwrap().1;
        let confirm = shown.iter().find(|(text, _)| text == "열기").unwrap().1;
        assert!(cancel.x < confirm.x);
        let point = shown.iter().find(|(text, _)| text == "one.zip").unwrap().1;
        for pressed in [true, false] {
            draw(vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        draw(vec![key(egui::Key::Enter)]);
        assert!(matches!(result, Some(Choice::Paths(ref paths)) if paths == &vec![file]));
    }

    fn focus_zip_action_with_tab(ctx: &egui::Context, app: &mut ArchiveApp, label: &str) {
        render(ctx, app, vec![]);
        let shown = render(ctx, app, vec![]);
        let point = shown.iter().find(|(text, _)| text == label).unwrap().1;
        for _ in 0..80 {
            if ctx
                .memory(|memory| memory.focused())
                .and_then(|id| ctx.read_response(id))
                .is_some_and(|response| response.rect.contains(point))
            {
                return;
            }
            render(ctx, app, vec![key(egui::Key::Tab)]);
            let mut released = key(egui::Key::Tab);
            if let egui::Event::Key { pressed, .. } = &mut released {
                *pressed = false;
            }
            render(ctx, app, vec![released]);
        }
        panic!("키보드 초점 없음: {label}");
    }

    #[test]
    fn keyboard_cancel_and_escape_never_start_zip_creation() {
        for escape in [false, true] {
            let scratch = Folder::new();
            let factory = FakeFactory::manual();
            let mut app = ArchiveApp::new(Box::new(factory.clone()), None, false);
            let source = scratch.0.join("source.txt");
            fs::write(&source, "fixture").unwrap();
            app.add_sources(vec![source]);
            let ctx = egui::Context::default();
            focus_zip_action_with_tab(&ctx, &mut app, if escape { "만들기" } else { "취소" });
            let mut events = vec![key(egui::Key::Enter)];
            if escape {
                events.push(key(egui::Key::Escape));
            }
            render(&ctx, &mut app, events);
            let shown = render(&ctx, &mut app, vec![]);
            assert!(factory.state.borrow().starts.is_empty());
            assert!(!app.model.busy());
            assert!(!scratch.0.join("source.zip").exists());
            assert_eq!(
                shown
                    .iter()
                    .filter(|(text, _)| text == "ZIP 만들기")
                    .count(),
                1
            );
        }
    }

    #[test]
    fn zip_creation_enter_only_activates_focused_make_button() {
        for make_button in [false, true] {
            let scratch = Folder::new();
            let factory = FakeFactory::manual();
            let mut app = ArchiveApp::new(Box::new(factory.clone()), None, false);
            let source = scratch.0.join("source.txt");
            fs::write(&source, "fixture").unwrap();
            app.add_sources(vec![source.clone()]);
            let ctx = egui::Context::default();
            render(&ctx, &mut app, vec![]);
            if make_button {
                focus_zip_action_with_tab(&ctx, &mut app, "만들기");
            } else {
                ctx.memory_mut(|memory| memory.request_focus(egui::Id::unique("zip-name")));
                render(&ctx, &mut app, vec![]);
            }
            render(&ctx, &mut app, vec![key(egui::Key::Enter)]);
            render(&ctx, &mut app, vec![]);
            let state = factory.state.borrow();
            assert_eq!(state.starts.len(), usize::from(make_button));
            if make_button {
                assert!(
                    matches!(state.starts.last(), Some(Start::CreateZip {sources, output_file, ..})
                    if sources == &vec![(source, "source.txt".into())]
                        && output_file == &scratch.0.join("source.zip"))
                );
            }
        }
    }

    fn picker_frame(
        ctx: &egui::Context,
        picker: &mut Picker,
        events: Vec<egui::Event>,
    ) -> (Vec<(String, egui::Pos2)>, Option<Choice>) {
        let mut choice = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(760.0, 540.0),
                )),
                events,
                ..Default::default()
            },
            |_ui| choice = picker.show(ctx),
        );
        output.textures_delta.clear();
        (labels(&output.shapes), choice)
    }

    fn keyboard_activate_picker(
        ctx: &egui::Context,
        picker: &mut Picker,
        label: &str,
    ) -> Option<Choice> {
        picker_frame(ctx, picker, vec![]);
        let shown = picker_frame(ctx, picker, vec![]).0;
        let point = shown.iter().find(|(text, _)| text == label).unwrap().1;
        for _ in 0..80 {
            let focused = ctx.memory(|memory| memory.focused());
            if focused
                .and_then(|id| ctx.read_response(id))
                .is_some_and(|response| response.rect.contains(point))
            {
                let choice = picker_frame(ctx, picker, vec![key(egui::Key::Enter)]).1;
                let mut released = key(egui::Key::Enter);
                if let egui::Event::Key { pressed, .. } = &mut released {
                    *pressed = false;
                }
                picker_frame(ctx, picker, vec![released]);
                return choice;
            }
            picker_frame(ctx, picker, vec![key(egui::Key::Tab)]);
            let mut released = key(egui::Key::Tab);
            if let egui::Event::Key { pressed, .. } = &mut released {
                *pressed = false;
            }
            picker_frame(ctx, picker, vec![released]);
        }
        panic!("키보드 초점 없음: {label}");
    }

    #[test]
    fn keyboard_file_pickers_enter_child_before_confirming_file() {
        for purpose in [Purpose::Open, Purpose::Files] {
            let scratch = Folder::new();
            let child = scratch.0.join("child");
            fs::create_dir(&child).unwrap();
            let file = child.join("inside.zip");
            fs::write(&file, "fixture").unwrap();
            let ctx = egui::Context::default();
            let mut picker = Picker::new(purpose, &scratch.0);
            assert!(keyboard_activate_picker(&ctx, &mut picker, "child").is_none());
            assert!(
                picker_frame(&ctx, &mut picker, vec![])
                    .0
                    .iter()
                    .any(|(text, _)| text == "inside.zip")
            );
            let choice = keyboard_activate_picker(&ctx, &mut picker, "inside.zip");
            assert!(matches!(choice, Some(Choice::Paths(paths)) if paths == vec![file]));
        }
    }

    #[test]
    fn keyboard_folder_pickers_navigate_before_explicit_folder_confirmation() {
        for purpose in [
            Purpose::Destination,
            Purpose::Retry,
            Purpose::Folder,
            Purpose::ZipFolder,
        ] {
            let scratch = Folder::new();
            let child = scratch.0.join("child");
            fs::create_dir(&child).unwrap();
            fs::write(child.join("inside.zip"), "fixture").unwrap();
            let ctx = egui::Context::default();
            let mut picker = Picker::new(purpose, &scratch.0);
            assert!(keyboard_activate_picker(&ctx, &mut picker, "child").is_none());
            assert!(
                picker_frame(&ctx, &mut picker, vec![])
                    .0
                    .iter()
                    .any(|(text, _)| text == "inside.zip")
            );
            let choice = keyboard_activate_picker(&ctx, &mut picker, "이 폴더 선택");
            assert!(matches!(choice, Some(Choice::Paths(paths)) if paths == vec![child]));
        }
    }

    #[test]
    fn changed_zip_sources_retry_creation_with_or_without_open_archive() {
        for opened in [false, true] {
            let scratch = Folder::new();
            let factory = FakeFactory::default();
            let mut app = ArchiveApp::new(
                Box::new(factory.clone()),
                opened.then(|| scratch.0.join("unrelated.zip")),
                false,
            );
            if opened {
                pump(&mut app.model);
            }
            factory.state.borrow_mut().manual = true;
            let source = scratch.0.join("source.txt");
            fs::write(&source, "fixture").unwrap();
            app.add_sources(vec![source]);
            let ctx = egui::Context::default();
            click(&ctx, &mut app, "만들기");
            assert!(app.model.busy());
            let request = factory.state.borrow().starts.last().unwrap().clone();
            assert!(matches!(request, Start::CreateZip { .. }));
            {
                let mut state = factory.state.borrow_mut();
                state.frame(Event::Accepted { encryption: false });
                state.frame(Event::Terminal(Terminal {
                    outcome: Outcome::Failed,
                    error: Some(ErrorCode::SourceChanged),
                    publication: Publication::None,
                    created_count: 0,
                    retained_partial_count: 0,
                    durability: Durability::NotApplicable,
                    cleanup: Cleanup::Complete,
                }));
                state.queue.extend([
                    HostEvent::StdoutEof,
                    HostEvent::StderrEof,
                    HostEvent::Exited { success: true },
                ]);
            }
            for _ in 0..30 {
                render(&ctx, &mut app, vec![]);
            }
            assert!(!app.model.busy());
            let shown = render(&ctx, &mut app, vec![]);
            assert!(
                shown
                    .iter()
                    .any(|(text, _)| text.contains("묶을 파일이 바뀌었습니다"))
            );
            assert!(!shown.iter().any(|(text, _)| text == "압축파일 다시 열기"));
            let count = factory.state.borrow().starts.len();
            click(&ctx, &mut app, "다시 만들기");
            assert!(app.model.busy());
            let state = factory.state.borrow();
            assert_eq!(state.starts.len(), count + 1);
            assert_eq!(state.starts.last(), Some(&request));
        }
    }
}

#[cfg(feature = "ui")]
#[test]
fn conflict_retry_waits_for_folder_choice_and_preserves_selection() {
    use eframe::egui;
    use zipelo_ui_memory_app::archive_screen::ArchiveApp;
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = ArchiveApp::new(
        Box::new(factory.clone()),
        Some(scratch.0.join("sample.zip")),
        false,
    );
    pump(&mut app.model);
    app.model.selected.insert(0);
    factory.state.borrow_mut().manual = true;
    let ctx = egui::Context::default();
    let render = |app: &mut ArchiveApp, events| {
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
        fn walk(shape: &egui::epaint::Shape, labels: &mut Vec<(String, egui::Pos2)>) {
            match shape {
                egui::epaint::Shape::Text(text) => labels.push((
                    text.galley.job.text.clone(),
                    text.pos + text.galley.rect.size() / 2.0,
                )),
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, labels);
                    }
                }
                _ => {}
            }
        }
        let mut labels = Vec::new();
        for shape in &output.shapes {
            walk(&shape.shape, &mut labels);
        }
        labels
    };
    let click = |app: &mut ArchiveApp, text: &str| {
        render(app, vec![]);
        let point = render(app, vec![])
            .into_iter()
            .filter(|(value, _)| value == text)
            .max_by(|a, b| a.1.y.total_cmp(&b.1.y))
            .unwrap_or_else(|| panic!("항목 없음: {text}"))
            .1;
        for pressed in [true, false] {
            render(
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
    };
    render(&mut app, vec![]);
    click(&mut app, "선택한 항목만 풀기");
    assert!(app.begin_extract());
    let old = scratch.0.join("sample");
    fs::write(old.join("existing"), "preserve").unwrap();
    let chosen = scratch.0.join("chosen");
    fs::create_dir(&chosen).unwrap();
    {
        let mut state = factory.state.borrow_mut();
        state.frame(Event::Accepted { encryption: true });
        state.frame(Event::Terminal(Terminal {
            outcome: Outcome::Failed,
            error: Some(ErrorCode::Conflict),
            publication: Publication::None,
            created_count: 0,
            retained_partial_count: 0,
            durability: Durability::NotApplicable,
            cleanup: Cleanup::Complete,
        }));
        state.queue.extend([
            HostEvent::StdoutEof,
            HostEvent::StderrEof,
            HostEvent::Exited { success: true },
        ]);
    }
    for _ in 0..30 {
        render(&mut app, vec![]);
    }
    assert_eq!(factory.state.borrow().starts.len(), 2);
    click(&mut app, "다른 폴더에 풀기");
    assert_eq!(factory.state.borrow().starts.len(), 2);
    click(&mut app, "상위 폴더");
    click(&mut app, "chosen");
    click(&mut app, "이 폴더 선택");
    assert!(matches!(factory.state.borrow().starts.last(),
        Some(Start::Extract { destination, selection: Selection::EntryIds(ids), .. })
        if destination == &chosen && ids == &vec![0]));
    assert_eq!(
        fs::read_to_string(old.join("existing")).unwrap(),
        "preserve"
    );
}

#[cfg(feature = "ui")]
#[test]
fn create_dialog_keeps_title_and_actions_visible_after_resize() {
    use eframe::egui::{self, epaint::Shape};
    use zipelo_ui_memory_app::archive_screen::ArchiveApp;
    let scratch = Folder::new();
    let mut app = ArchiveApp::new(Box::new(FakeFactory::default()), None, false);
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
        for label in ["ZIP 만들기", "만들기", "취소"] {
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
