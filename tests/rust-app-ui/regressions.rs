mod fake;
use fake::*;
use std::path::PathBuf;
use zipelo_ui_memory_app::archive_ui::*;

fn path(name: &str) -> PathBuf {
    std::env::current_dir().unwrap().join(name)
}
fn pump(m: &mut Model) {
    for _ in 0..100 {
        m.poll();
        if !m.busy() {
            return;
        }
    }
    panic!("가짜 작업자 종료 누락: {}", m.status());
}
fn list(m: &mut Model, f: &mut FakeFactory) {
    assert!(m.begin(
        Start::List {
            archive: path("example.zip")
        },
        f
    ));
    pump(m);
    assert!(m.list_id.is_some());
}

#[test]
fn open_lists_only_after_terminal_eofs_and_wait() {
    let mut f = FakeFactory::default();
    let mut m = Model::default();
    assert!(m.begin(
        Start::List {
            archive: path("example.zip")
        },
        &mut f
    ));
    while m.terminal.is_none() {
        m.poll();
        assert!(m.list_id.is_none());
        assert!(!m.can_extract());
    }
    assert!(m.busy());
    assert!(!m.begin(
        Start::List {
            archive: path("replacement.zip")
        },
        &mut f
    ));
    pump(&mut m);
    assert_eq!(m.entries[0].name, "읽어주세요.txt");
    assert!(m.can_extract());
    assert!(m.confirmed);
}
#[test]
fn extract_uses_list_token_selection_and_destination() {
    let mut f = FakeFactory::default();
    let mut m = Model::default();
    list(&mut m, &mut f);
    let token = m.list_id.clone().unwrap();
    m.selected.insert(0);
    assert!(m.extract(path("output"), true, None, &mut f));
    pump(&mut m);
    assert!(
        matches!(f.state.borrow().starts.last(),Some(Start::Extract {selection:Selection::EntryIds(ids),list_id,..}) if ids==&vec![0]&&*list_id==token)
    );
    assert_eq!(m.terminal.unwrap().publication, Publication::Complete);
}
#[test]
fn gzip_requires_explicit_leaf() {
    let mut f = FakeFactory::default();
    let mut m = Model::default();
    assert!(m.begin(
        Start::List {
            archive: path("sample.gz")
        },
        &mut f
    ));
    pump(&mut m);
    for name in [None, Some("../escape".into()), Some("sub/file".into())] {
        assert!(!m.extract(path("output"), false, name, &mut f));
    }
    assert!(m.extract(path("output"), false, Some("plain.txt".into()), &mut f));
    pump(&mut m);
}
#[test]
fn creates_zip_and_reports_progress_outputs() {
    let mut f = FakeFactory::default();
    let mut m = Model::default();
    assert!(m.begin(
        Start::CreateZip {
            sources: vec![(path("one.txt"), "one.txt".into())],
            output_file: path("new.zip"),
            method: Method::Deflate,
            aes256: false
        },
        &mut f
    ));
    pump(&mut m);
    assert_eq!(m.progress.unwrap().1, 31);
    assert_eq!(m.outputs.len(), 1);
    assert_eq!(m.status(), "작업 완료");
}
#[test]
fn password_challenge_is_answered_without_retaining_secret() {
    let mut f = FakeFactory::default();
    let mut m = Model::default();
    assert!(m.begin(
        Start::List {
            archive: path("locked.zip")
        },
        &mut f
    ));
    for _ in 0..10 {
        m.poll();
        if m.challenge.is_some() {
            break;
        }
    }
    assert_eq!(m.challenge, Some((17, 1, false)));
    assert!(!m.password(String::new()));
    assert!(m.password("synthetic password".into()));
    pump(&mut m);
    assert!(f.state.borrow().password_seen);
    assert!(m.challenge.is_none());
    assert!(m.list_id.is_some());
}
#[test]
fn cancel_close_retract_preserves_request_and_waits_for_exit() {
    let mut f = FakeFactory::default();
    let mut m = Model::default();
    assert!(m.begin(
        Start::List {
            archive: path("one.zip")
        },
        &mut f
    ));
    m.poll();
    m.request_close();
    assert_eq!(m.close, Close::Confirm);
    m.confirm_close();
    m.keep_open();
    m.cancel();
    pump(&mut m);
    assert_eq!(m.close, Close::Open);
    assert_eq!(m.cancel, Some(CancelStatus::Requested));
    assert_eq!(m.terminal.unwrap().outcome, Outcome::Cancelled);
    assert_eq!(
        f.state
            .borrow()
            .requests
            .iter()
            .filter(|(_, k)| *k == "cancel")
            .count(),
        1
    );
}
#[test]
fn terminal_race_needs_no_cancel_ack() {
    let mut f = FakeFactory::manual();
    let mut m = Model::default();
    assert!(m.begin(
        Start::List {
            archive: path("one.zip")
        },
        &mut f
    ));
    {
        let mut s = f.state.borrow_mut();
        s.frame(Event::Accepted { encryption: false });
        s.finish(Outcome::Cancelled, Publication::None);
    }
    m.cancel();
    pump(&mut m);
    assert!(m.cancel.is_none());
    assert_eq!(m.status(), "작업 취소됨");
}
#[test]
fn eof_exit_without_terminal_never_means_success() {
    let mut f = FakeFactory::manual();
    let mut m = Model::default();
    assert!(m.begin(
        Start::List {
            archive: path("one.zip")
        },
        &mut f
    ));
    {
        let mut s = f.state.borrow_mut();
        s.queue.extend([
            HostEvent::StdoutEof,
            HostEvent::StderrEof,
            HostEvent::Exited { success: true },
        ]);
    }
    pump(&mut m);
    assert!(!m.confirmed);
    assert!(m.list_id.is_none());
    assert!(m.error.is_some());
}
#[test]
fn stale_sequence_and_job_are_protocol_errors() {
    for bad_job in [true, false] {
        let mut f = FakeFactory::manual();
        let mut m = Model::default();
        assert!(m.begin(
            Start::List {
                archive: path("one.zip")
            },
            &mut f
        ));
        let id = m.job_id().unwrap();
        f.state.borrow_mut().queue.push_back(HostEvent::Frame {
            job_id: if bad_job { id + 1 } else { id },
            sequence: if bad_job { 0 } else { 1 },
            event: Event::Accepted { encryption: false },
        });
        m.poll();
        assert!(m.error.is_some());
        assert!(m.busy());
    }
}
#[test]
fn source_changed_invalidates_selection_without_retry() {
    let mut f = FakeFactory::default();
    let mut m = Model::default();
    list(&mut m, &mut f);
    m.selected.insert(0);
    f.state.borrow_mut().manual = true;
    assert!(m.extract(path("out"), true, None, &mut f));
    {
        let mut s = f.state.borrow_mut();
        s.frame(Event::Accepted { encryption: false });
        s.frame(Event::Terminal(Terminal {
            outcome: Outcome::Failed,
            error: Some(ErrorCode::SourceChanged),
            publication: Publication::None,
            created_count: 0,
            retained_partial_count: 0,
            durability: Durability::NotApplicable,
            cleanup: Cleanup::Complete,
        }));
        s.queue.extend([
            HostEvent::StdoutEof,
            HostEvent::StderrEof,
            HostEvent::Exited { success: true },
        ]);
    }
    pump(&mut m);
    assert!(m.list_id.is_none());
    assert!(m.selected.is_empty());
    assert_eq!(f.state.borrow().starts.len(), 2);
}
#[test]
fn partial_and_uncertain_results_are_preserved() {
    let mut f = FakeFactory::manual();
    let mut m = Model::default();
    assert!(m.begin(
        Start::CreateZip {
            sources: vec![(path("one"), "one".into())],
            output_file: path("out.zip"),
            method: Method::Store,
            aes256: false
        },
        &mut f
    ));
    {
        let mut s = f.state.borrow_mut();
        s.frame(Event::Accepted { encryption: false });
        s.frame(Event::Output {
            entry_id: None,
            relative_name: "out.zip".into(),
            disposition: Disposition::CommitUncertain,
        });
        s.frame(Event::Terminal(Terminal {
            outcome: Outcome::Failed,
            error: Some(ErrorCode::Io),
            publication: Publication::Uncertain,
            created_count: 0,
            retained_partial_count: 1,
            durability: Durability::Failed,
            cleanup: Cleanup::Unconfirmed,
        }));
        s.queue.extend([
            HostEvent::StdoutEof,
            HostEvent::StderrEof,
            HostEvent::Exited { success: true },
        ]);
    }
    pump(&mut m);
    let t = m.terminal.as_ref().unwrap();
    assert_eq!(t.publication, Publication::Uncertain);
    assert_eq!(t.retained_partial_count, 1);
    assert_eq!(t.durability, Durability::Failed);
    assert_eq!(m.outputs[0].1, Disposition::CommitUncertain);
}
#[test]
fn close_requires_result_acknowledgement() {
    let mut f = FakeFactory::default();
    let mut m = Model::default();
    assert!(m.begin(
        Start::List {
            archive: path("one.zip")
        },
        &mut f
    ));
    m.request_close();
    m.confirm_close();
    m.acknowledge_close();
    assert_eq!(m.close, Close::Waiting);
    pump(&mut m);
    assert_eq!(m.close, Close::Review);
    m.acknowledge_close();
    assert_eq!(m.close, Close::Allowed);
}
#[test]
fn unsupported_worker_and_unsafe_inputs_do_not_start() {
    let mut m = Model::default();
    assert!(!m.begin(
        Start::List {
            archive: path("one.zip")
        },
        &mut Unavailable
    ));
    assert!(m.error.as_ref().unwrap().contains("준비"));
    let mut f = FakeFactory::default();
    assert!(!m.begin(
        Start::List {
            archive: "relative.zip".into()
        },
        &mut f
    ));
    assert!(!m.begin(
        Start::CreateZip {
            sources: vec![(path("one"), "../one".into())],
            output_file: path("out.zip"),
            method: Method::Store,
            aes256: false
        },
        &mut f
    ));
    assert!(f.state.borrow().starts.is_empty());
}
#[cfg(feature = "ui")]
#[test]
fn renders_every_flow_with_actual_egui_widgets() {
    use eframe::egui::{self, epaint::Shape};
    use zipelo_ui_memory_app::archive_screen::ArchiveApp;
    fn text(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Pos2)> {
        fn walk(shape: &Shape, result: &mut Vec<(String, egui::Pos2)>) {
            match shape {
                Shape::Text(t) => result.push((
                    t.galley.job.text.clone(),
                    t.pos + t.galley.rect.size() / 2.0,
                )),
                Shape::Vec(v) => {
                    for shape in v {
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
    let ctx = egui::Context::default();
    let f = FakeFactory::default();
    let mut app = ArchiveApp::new(Box::new(f.clone()), None, true);
    let render = |app: &mut ArchiveApp, events: Vec<egui::Event>, width: f32| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 720.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.show(ui),
        );
        output.textures_delta.clear();
        assert!(!output.shapes.is_empty());
        text(&output.shapes)
    };
    let initial = render(&mut app, vec![], 1060.0);
    let point = initial.iter().find(|(t, _)| t == "ZIP 만들기").unwrap().1;
    render(
        &mut app,
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
        ],
        1060.0,
    );
    render(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        }],
        1060.0,
    );
    let create = render(&mut app, vec![], 1060.0);
    assert!(create.iter().any(|(t, _)| t == "저장할 ZIP 절대 경로"));
    let mut app = ArchiveApp::new(Box::new(f.clone()), Some(path("locked.zip")), true);
    for _ in 0..8 {
        app.model.poll();
    }
    for width in [760.0, 1060.0] {
        render(&mut app, vec![], width);
        render(&mut app, vec![], width);
        let password = render(&mut app, vec![], width);
        assert!(password.iter().any(|(t, _)| t.contains("암호 전달")));
    }
    assert!(app.model.password("synthetic secret".into()));
    pump(&mut app.model);
    let listed = render(&mut app, vec![], 1060.0);
    assert!(listed.iter().any(|(t, _)| t == "읽어주세요.txt"));
    assert!(!listed.iter().any(|(t, _)| t.contains("synthetic secret")));
    assert!(app.model.extract(path("out"), false, None, &mut f.clone()));
    render(&mut app, vec![], 1060.0);
    app.model.request_close();
    render(&mut app, vec![], 1060.0);
    let confirm = render(&mut app, vec![], 1060.0);
    assert!(confirm.iter().any(|(t, _)| t == "작업 중 닫기"));
    app.model.confirm_close();
    pump(&mut app.model);
    render(&mut app, vec![], 1060.0);
    let result = render(&mut app, vec![], 1060.0);
    assert!(result.iter().any(|(t, _)| t == "결과 확인 후 닫기"));
}

#[test]
fn file_association_arguments_preserve_native_paths() {
    let cwd = std::env::current_dir().unwrap();
    let (fixture, archive) =
        launch_args(vec!["--ui-fixture".into(), "한글.zip".into()], cwd.clone()).unwrap();
    assert!(fixture);
    assert_eq!(archive, Some(cwd.join("한글.zip")));
    assert!(launch_args(vec!["a".into(), "b".into()], cwd.clone()).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let name = std::ffi::OsString::from_vec(vec![b'/', 0xff, b'a']);
        assert_eq!(
            launch_args(vec![name.clone()], cwd).unwrap().1,
            Some(PathBuf::from(name))
        );
    }
}

#[cfg(feature = "ui")]
#[test]
fn dropped_native_file_opens_without_ui_reading_contents() {
    use eframe::egui;
    use zipelo_ui_memory_app::archive_screen::ArchiveApp;
    #[derive(Debug)]
    struct DropFile(PathBuf);
    impl egui::DroppedFile for DropFile {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            panic!("화면이 압축파일을 직접 읽으면 안 됨")
        }
    }
    let ctx = egui::Context::default();
    let f = FakeFactory::default();
    let mut app = ArchiveApp::new(Box::new(f.clone()), None, true);
    let file = path("dropped.zip");
    let mut output = ctx.run_ui(
        egui::RawInput {
            dropped_files: vec![std::sync::Arc::new(DropFile(file.clone()))],
            ..Default::default()
        },
        |ui| app.show(ui),
    );
    output.textures_delta.clear();
    assert_eq!(app.model.archive, Some(file));
    assert_eq!(f.state.borrow().starts.len(), 1);
    pump(&mut app.model);
    assert!(app.model.can_extract());
}
