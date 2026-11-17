// 목록 항목을 창 밖으로 끌어 놓는 흐름 시험. 운영체제 끌기는 가짜 구현으로 대신함.
// presentation.rs의 widgets 모듈 안에 explorer_selection.rs 다음으로 포함됨.

#[derive(Default)]
struct DragLog {
    starts: usize,
    accept: bool,
    /// provide와 start가 불린 순서.
    calls: Vec<&'static str>,
    /// Windows처럼 놓을 때까지 start 안에 머물며, 받는 쪽 요청에 pump로 경로를 받음.
    /// 머무는 동안 실행할 일(가짜 작업자 끝내기 등)을 담음.
    blocking: Option<Box<dyn FnMut()>>,
    /// blocking일 때 pump로 받은 결과.
    pumped: Vec<Result<Vec<PathBuf>, ()>>,
    provided: Vec<Vec<PathBuf>>,
    failed: usize,
    events: std::collections::VecDeque<zipelo_ui_memory_app::drag_out::DragEvent>,
}

struct FakeDrag(std::rc::Rc<std::cell::RefCell<DragLog>>);

impl zipelo_ui_memory_app::drag_out::DragOut for FakeDrag {
    fn start(&mut self, pump: &mut zipelo_ui_memory_app::drag_out::Pump<'_>) -> bool {
        let mut hook = {
            let mut log = self.0.borrow_mut();
            log.starts += 1;
            log.calls.push("start");
            if !log.accept {
                return false;
            }
            log.blocking.take()
        };
        if let Some(work) = hook.as_mut() {
            work();
            // 받는 쪽이 놓은 뒤 파일을 요청한 것처럼 결과가 나올 때까지 pump를 부름.
            for _ in 0..1000 {
                if let Some(result) = pump() {
                    self.0.borrow_mut().pumped.push(result);
                    break;
                }
            }
        }
        true
    }
    fn poll(&mut self) -> Option<zipelo_ui_memory_app::drag_out::DragEvent> {
        self.0.borrow_mut().events.pop_front()
    }
    fn provide(&mut self, paths: &[PathBuf]) {
        let mut log = self.0.borrow_mut();
        log.calls.push("provide");
        log.provided.push(paths.to_vec());
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
    let folder = scratch.0.join(zipelo_ui_memory_app::archive_present::open_root_name("sample.zip")).join("1");
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
        Some(Start::Extract { destination, .. }) if !destination.starts_with(scratch.0.join(zipelo_ui_memory_app::archive_present::open_root_name("sample.zip")))
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
    s.factory.state.borrow_mut().manual = true;
    drag_row(&mut s, "alpha.txt");
    assert_eq!(log.borrow().starts, 1);
    // 풀기를 먼저 시작하므로, 끌기를 시작하지 못하면 바로 취소하고 결과 창을 띄우지 않음.
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
    assert!(!s.app.model.busy());
    assert!(log.borrow().provided.is_empty());
    assert!(
        !labels.iter().any(|(text, _, _)| text.contains("취소했습니다")),
        "{labels:?}"
    );
}

#[test]
fn dragging_the_same_items_again_reuses_the_extracted_files() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let log = with_drag(&mut s);
    s.click("alpha.txt", egui::Modifiers::NONE);
    s.factory.state.borrow_mut().manual = true;
    drag_row(&mut s, "alpha.txt");
    let folder = scratch
        .0
        .join(zipelo_ui_memory_app::archive_present::open_root_name("sample.zip"))
        .join("1");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("alpha.txt"), "x").unwrap();
    finish_worker(&mut s);
    let extracts = s.factory.state.borrow().starts.len();
    // 같은 항목을 다시 끌면 다시 풀지 않고 바로 시작해 풀어 둔 경로를 넘김.
    s.pointer(egui::pos2(5.0, 5.0), false, egui::Modifiers::NONE);
    drag_row(&mut s, "alpha.txt");
    assert_eq!(s.factory.state.borrow().starts.len(), extracts);
    assert_eq!(log.borrow().calls, ["start", "provide", "start", "provide"]);
    assert_eq!(
        log.borrow().provided,
        vec![vec![folder.join("alpha.txt")], vec![folder.join("alpha.txt")]]
    );
}

#[test]
fn a_different_selection_is_extracted_again() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let log = with_drag(&mut s);
    s.click("alpha.txt", egui::Modifiers::NONE);
    s.factory.state.borrow_mut().manual = true;
    drag_row(&mut s, "alpha.txt");
    let folder = scratch
        .0
        .join(zipelo_ui_memory_app::archive_present::open_root_name("sample.zip"))
        .join("1");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("alpha.txt"), "x").unwrap();
    finish_worker(&mut s);
    assert_eq!(log.borrow().provided.len(), 1);
    let extracts = s.factory.state.borrow().starts.len();
    s.pointer(egui::pos2(5.0, 5.0), false, egui::Modifiers::NONE);
    s.click("beta.txt", egui::Modifiers::NONE);
    drag_row(&mut s, "beta.txt");
    assert_eq!(s.factory.state.borrow().starts.len(), extracts + 1);
}

#[cfg(windows)]
#[test]
fn windows_temp_folder_is_hidden() {
    use std::os::windows::fs::MetadataExt;
    let scratch = Folder::new();
    let root = zipelo_ui_memory_app::archive_present::create_open_root(&scratch.0.join("sample.zip"))
        .unwrap();
    let attributes = fs::metadata(root.path()).unwrap().file_attributes();
    assert_ne!(attributes & 0x2, 0, "FILE_ATTRIBUTE_HIDDEN");
    zipelo_ui_memory_app::archive_present::remove_open_root(root);
}

#[test]
fn long_press_without_moving_does_not_extract_and_one_drag_extracts_once() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let log = with_drag(&mut s);
    s.click("docs", egui::Modifiers::NONE);
    let before = s.factory.state.borrow().starts.len();
    s.time += 1.0;
    let start = s.row("docs").center();
    s.pointer(start, true, egui::Modifiers::NONE);
    // 움직이지 않고 1.5초 동안 누르고 있어도 끌기와 풀기를 시작하지 않음.
    for _ in 0..30 {
        s.frame(egui::Modifiers::NONE, vec![]);
    }
    assert_eq!(log.borrow().starts, 0);
    assert_eq!(s.factory.state.borrow().starts.len(), before);
    // 움직이면 한 번만 시작하고, 계속 끌고 놓아도 다시 풀지 않음.
    for step in 1..=20 {
        s.frame(
            egui::Modifiers::NONE,
            vec![egui::Event::PointerMoved(start + egui::vec2(0.0, 6.0 * step as f32))],
        );
    }
    s.pointer(start + egui::vec2(0.0, 120.0), false, egui::Modifiers::NONE);
    for _ in 0..10 {
        s.frame(egui::Modifiers::NONE, vec![]);
    }
    assert_eq!(log.borrow().starts, 1);
    assert_eq!(s.factory.state.borrow().starts.len(), before + 1);
}

#[cfg(unix)]
#[test]
fn drag_temp_folder_is_hidden_next_to_the_archive() {
    assert_eq!(
        zipelo_ui_memory_app::archive_present::open_root_name("자료.zip"),
        ".자료.zip (Zipelo 임시)"
    );
    // snap 앱은 홈 폴더 바로 아래의 숨김 항목을 읽지 못하므로 그 자리에서는 숨기지 않음.
    let home = zipelo_ui_memory_app::archive_present::home_directory();
    assert_eq!(
        zipelo_ui_memory_app::archive_present::open_root_name_in(&home, "자료.zip"),
        "자료.zip (Zipelo 임시)"
    );
    assert_eq!(
        zipelo_ui_memory_app::archive_present::open_root_name_in(&home.join("다운로드"), "자료.zip"),
        ".자료.zip (Zipelo 임시)"
    );
}

#[test]
fn blocking_drag_waits_for_extraction_inside_the_drag_and_hands_over_paths() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let log = with_drag(&mut s);
    s.click("alpha.txt", egui::Modifiers::NONE);
    s.factory.state.borrow_mut().manual = true;
    let folder = scratch
        .0
        .join(zipelo_ui_memory_app::archive_present::open_root_name("sample.zip"))
        .join("1");
    // 끌기가 진행되는 동안(화면이 그려지지 않는 동안) 작업자가 풀기를 마침.
    let worker = s.factory.state.clone();
    let target = folder.clone();
    log.borrow_mut().blocking = Some(Box::new(move || {
        assert!(!worker.borrow().starts.is_empty(), "끌기 전에 풀기를 시작해야 함");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("alpha.txt"), "x").unwrap();
        let mut worker = worker.borrow_mut();
        worker.frame(Event::Accepted { encryption: false });
        worker.finish(Outcome::Completed, Publication::Complete);
    }));
    let before = s.factory.state.borrow().starts.len();
    drag_row(&mut s, "alpha.txt");
    assert_eq!(s.factory.state.borrow().starts.len(), before + 1);
    assert_eq!(log.borrow().pumped, vec![Ok(vec![folder.join("alpha.txt")])]);
    // 끌기에서 돌아오면 이미 끝난 작업을 마무리하고 원래 풀 위치로 되돌림.
    assert!(!s.app.model.busy());
    for _ in 0..5 {
        s.frame(egui::Modifiers::NONE, vec![]);
    }
    s.factory.state.borrow_mut().manual = false;
    assert!(s.app.begin_extract());
    assert!(matches!(
        s.factory.state.borrow().starts.last(),
        Some(Start::Extract { destination, .. }) if !destination.starts_with(&folder)
    ));
}

#[test]
fn blocking_drag_does_not_wait_for_a_password_prompt() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    let log = with_drag(&mut s);
    s.click("alpha.txt", egui::Modifiers::NONE);
    s.factory.state.borrow_mut().manual = true;
    let worker = s.factory.state.clone();
    log.borrow_mut().blocking = Some(Box::new(move || {
        let mut worker = worker.borrow_mut();
        worker.frame(Event::Accepted { encryption: false });
        worker.frame(Event::PasswordRequired {
            challenge_id: 1,
            attempt: 1,
            encrypt: false,
        });
    }));
    drag_row(&mut s, "alpha.txt");
    // 끌기 중에는 암호를 물을 수 없으므로 실패로 넘기고, 끌기가 끝나면 암호 창을 보여 줌.
    assert_eq!(log.borrow().pumped, vec![Err(())]);
    assert!(s.app.model.busy());
    assert!(s.app.model.challenge.is_some());
}
