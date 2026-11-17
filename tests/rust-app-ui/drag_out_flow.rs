// 목록 항목을 창 밖으로 끌어 놓는 흐름 시험. 운영체제 끌기는 가짜 구현으로 대신함.
// presentation.rs의 widgets 모듈 안에 explorer_selection.rs 다음으로 포함됨.

#[derive(Default)]
struct DragLog {
    starts: usize,
    accept: bool,
    provided: Vec<Vec<PathBuf>>,
    failed: usize,
    events: std::collections::VecDeque<zipelo_ui_memory_app::drag_out::DragEvent>,
}

struct FakeDrag(std::rc::Rc<std::cell::RefCell<DragLog>>);

impl zipelo_ui_memory_app::drag_out::DragOut for FakeDrag {
    fn start(&mut self) -> bool {
        let mut log = self.0.borrow_mut();
        log.starts += 1;
        log.accept
    }
    fn poll(&mut self) -> Option<zipelo_ui_memory_app::drag_out::DragEvent> {
        self.0.borrow_mut().events.pop_front()
    }
    fn provide(&mut self, paths: &[PathBuf]) {
        self.0.borrow_mut().provided.push(paths.to_vec());
    }
    fn fail(&mut self) {
        self.0.borrow_mut().failed += 1;
    }
}

fn with_drag(s: &mut Explorer) -> std::rc::Rc<std::cell::RefCell<DragLog>> {
    let log = std::rc::Rc::new(std::cell::RefCell::new(DragLog {
        accept: true,
        ..Default::default()
    }));
    s.app.set_drag_out(Box::new(FakeDrag(log.clone())));
    s.frame(egui::Modifiers::NONE, vec![]);
    log
}

/// 행을 누른 채 아래로 끌어 egui 끌기를 시작함.
fn drag_row(s: &mut Explorer, path: &str) {
    s.time += 1.0;
    let start = s.row(path).center();
    s.pointer(start, true, egui::Modifiers::NONE);
    for step in 1..=4 {
        s.frame(
            egui::Modifiers::NONE,
            vec![egui::Event::PointerMoved(start + egui::vec2(0.0, 12.0 * step as f32))],
        );
    }
}

fn finish_worker(s: &mut Explorer) {
    {
        let mut worker = s.factory.state.borrow_mut();
        worker.frame(Event::Accepted { encryption: false });
        worker.finish(Outcome::Completed, Publication::Complete);
    }
    for _ in 0..10 {
        s.frame(egui::Modifiers::NONE, vec![]);
    }
}

#[test]
fn dragging_selected_rows_out_extracts_them_and_hands_over_their_paths() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let log = with_drag(&mut s);
    s.click("alpha.txt", egui::Modifiers::NONE);
    s.click("docs", explorer_ctrl());
    s.selected(&[0, 1, 5]);
    s.factory.state.borrow_mut().manual = true;
    drag_row(&mut s, "alpha.txt");
    assert_eq!(log.borrow().starts, 1);
    // 선택이 유지되고(끌기는 범위 선택이 아님), 하위 항목을 포함해 임시 폴더에 풂.
    s.selected(&[0, 1, 5]);
    let folder = scratch.0.join("sample.zip (Zipelo 임시)").join("1");
    {
        let state = s.factory.state.borrow();
        let Some(Start::Extract {
            destination,
            selection: Selection::EntryIds(ids),
            ..
        }) = state.starts.last()
        else {
            panic!("{:?}", state.starts.last());
        };
        assert_eq!(destination, &folder);
        assert_eq!(ids, &vec![0, 1, 5]);
    }
    // 풀기가 끝나기 전에는 경로를 넘기지 않음.
    assert!(log.borrow().provided.is_empty());
    fs::create_dir_all(folder.join("docs")).unwrap();
    fs::write(folder.join("docs/child.txt"), "x").unwrap();
    fs::write(folder.join("alpha.txt"), "x").unwrap();
    finish_worker(&mut s);
    assert_eq!(
        log.borrow().provided,
        // 목록과 같은 순서(폴더 먼저).
        vec![vec![folder.join("docs"), folder.join("alpha.txt")]]
    );
    // 결과 창 없이 끝나고, 원래 풀 위치로 되돌아옴.
    assert!(!s.app.model.busy());
    s.factory.state.borrow_mut().manual = false;
    assert!(s.app.begin_extract());
    assert!(matches!(
        s.factory.state.borrow().starts.last(),
        Some(Start::Extract { destination, .. }) if !destination.starts_with(scratch.0.join("sample.zip (Zipelo 임시)"))
    ));
}

#[test]
fn dragging_an_unselected_row_selects_only_that_row() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let log = with_drag(&mut s);
    s.click("alpha.txt", egui::Modifiers::NONE);
    s.factory.state.borrow_mut().manual = true;
    drag_row(&mut s, "beta.txt");
    assert_eq!(log.borrow().starts, 1);
    s.selected(&[2]);
}

#[test]
fn cancelling_the_drag_cancels_extraction_without_a_result_window() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let log = with_drag(&mut s);
    s.click("alpha.txt", egui::Modifiers::NONE);
    s.factory.state.borrow_mut().manual = true;
    drag_row(&mut s, "alpha.txt");
    assert!(s.app.model.busy());
    log.borrow_mut()
        .events
        .push_back(zipelo_ui_memory_app::drag_out::DragEvent::Cancelled);
    s.frame(egui::Modifiers::NONE, vec![]);
    assert!(s.app.model.cancellation_requested());
    {
        let mut worker = s.factory.state.borrow_mut();
        worker.frame(Event::Accepted { encryption: false });
        worker.finish(Outcome::Cancelled, Publication::None);
    }
    let mut labels = Vec::new();
    for _ in 0..10 {
        labels = layout_text(&s.frame(egui::Modifiers::NONE, vec![]));
    }
    assert_eq!(log.borrow().failed, 1);
    assert!(log.borrow().provided.is_empty());
    assert!(
        !labels.iter().any(|(text, _, _)| text.contains("취소했습니다")),
        "{labels:?}"
    );
}

#[test]
fn rows_do_not_start_a_drag_when_the_platform_cannot() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let log = with_drag(&mut s);
    log.borrow_mut().accept = false;
    s.click("alpha.txt", egui::Modifiers::NONE);
    drag_row(&mut s, "alpha.txt");
    assert_eq!(log.borrow().starts, 1);
    // 끌기를 시작하지 못하면 아무 작업도 시작하지 않음.
    assert!(!s.app.model.busy());
    assert_eq!(s.factory.state.borrow().starts.len(), 1);
}
