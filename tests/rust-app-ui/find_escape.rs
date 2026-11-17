// 찾기 창(Ctrl+F, Ctrl+Shift+F), 폴더 크기와 Esc 동작 시험.
// presentation.rs의 widgets 모듈 안에 explorer_selection.rs 다음으로 포함됨.

fn find_ctrl(shift: bool) -> egui::Modifiers {
    // Linux와 Windows의 실제 입력처럼 ctrl과 command를 함께 켬.
    egui::Modifiers {
        ctrl: true,
        command: true,
        shift,
        ..egui::Modifiers::NONE
    }
}

fn press(s: &mut Explorer, key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::FullOutput> {
    let mut outputs = Vec::new();
    for pressed in [true, false] {
        outputs.push(s.frame(
            modifiers,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            }],
        ));
    }
    outputs
}

fn close_requested(outputs: &[egui::FullOutput]) -> bool {
    outputs.iter().any(|output| {
        output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .is_some_and(|viewport| {
                viewport
                    .commands
                    .iter()
                    .any(|command| *command == egui::ViewportCommand::Close)
            })
    })
}

fn find_entries() -> Vec<Entry> {
    [
        (0, "docs/child.txt", Kind::File, Some(7)),
        (1, "docs/deep/report.TXT", Kind::File, Some(1000)),
        (2, "alpha.txt", Kind::File, Some(3)),
        (3, "docs", Kind::Directory, Some(0)),
        (4, "music/child-song.mp3", Kind::File, None),
    ]
    .map(|(id, name, kind, bytes)| Entry {
        id,
        name: name.into(),
        kind,
        bytes,
        packed: None,
        encrypted: false,
    })
    .into()
}

#[test]
fn folder_rows_show_the_sum_of_contained_file_sizes() {
    use zipelo_ui_memory_app::archive_browser::rows;
    let entries = find_entries();
    let root = rows(&entries, "", "");
    let size = |name: &str| root.iter().find(|row| row.name == name).unwrap().bytes;
    // 명시된 폴더와 하위 폴더의 파일을 모두 더함.
    assert_eq!(size("docs"), Some(1007));
    // 압축파일에 따로 없는 폴더도 크기를 알 수 없는 파일은 0으로 보고 더함.
    assert_eq!(size("music"), Some(0));
    assert_eq!(size("alpha.txt"), Some(3));
    let docs = rows(&entries, "docs", "");
    assert_eq!(
        docs.iter().find(|row| row.name == "deep").unwrap().bytes,
        Some(1000)
    );
    // 빈 폴더는 0 B.
    let empty = [Entry {
        id: 0,
        name: "empty/".into(),
        kind: Kind::Directory,
        bytes: None,
        packed: None,
        encrypted: false,
    }];
    assert_eq!(rows(&empty, "", "")[0].bytes, Some(0));
}

#[test]
fn find_searches_subfolders_by_name_or_whole_archive_by_path() {
    use zipelo_ui_memory_app::archive_browser::find;
    let entries = find_entries();
    let paths = |base: &str, query: &str, whole: bool| {
        find(&entries, base, query, whole, 100)
            .0
            .into_iter()
            .map(|found| (found.path, found.directory))
            .collect::<Vec<_>>()
    };
    // Ctrl+F: 연 폴더 아래 모든 하위 폴더에서 이름으로 찾음. 대소문자는 구분하지 않음.
    assert_eq!(
        paths("", "child", false),
        [
            ("docs/child.txt".into(), false),
            ("music/child-song.mp3".into(), false)
        ]
    );
    assert_eq!(paths("docs", "child", false), [("docs/child.txt".into(), false)]);
    assert_eq!(
        paths("docs", "txt", false),
        [
            ("docs/child.txt".into(), false),
            ("docs/deep/report.TXT".into(), false)
        ]
    );
    // 압축파일에 따로 없는 하위 폴더도 찾음. 연 폴더 자신은 결과에 넣지 않음.
    assert_eq!(paths("docs", "deep", false), [("docs/deep".into(), true)]);
    assert_eq!(paths("docs", "docs", false), []);
    // 이름만 보므로 상위 폴더 이름으로는 찾지 않음.
    assert_eq!(paths("", "deep", false), [("docs/deep".into(), true)]);
    // Ctrl+Shift+F: 연 폴더와 관계없이 전체 경로에서 찾음.
    assert_eq!(
        paths("music", "deep", true),
        [
            ("docs/deep".into(), true),
            ("docs/deep/report.TXT".into(), false)
        ]
    );
    assert_eq!(paths("", "  ", true), []);
    let (limited, truncated) = find(&entries, "", "t", true, 2);
    assert_eq!(limited.len(), 2);
    assert!(truncated);
}

#[test]
fn ctrl_f_finds_below_current_folder_and_enter_moves_to_the_file() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.app.model.entries = find_entries();
    s.frame(egui::Modifiers::NONE, vec![]);
    press(&mut s, egui::Key::F, find_ctrl(false));
    assert!(s.app.find_open());
    let labels = layout_text(&s.frame(egui::Modifiers::NONE, vec![]));
    assert!(
        labels
            .iter()
            .any(|(text, _, _)| text == "/ 아래 모든 폴더에서 이름으로 찾습니다"),
        "{labels:?}"
    );
    // 입력란에 초점이 있으므로 그대로 입력함.
    s.frame(egui::Modifiers::NONE, vec![egui::Event::Text("report".into())]);
    s.frame(egui::Modifiers::NONE, vec![]);
    assert_eq!(s.app.find_results().len(), 1);
    press(&mut s, egui::Key::Enter, egui::Modifiers::NONE);
    s.frame(egui::Modifiers::NONE, vec![]);
    assert!(!s.app.find_open());
    // 파일이 든 폴더를 열고 그 파일을 선택함.
    s.selected(&[1]);
    let labels = layout_text(&s.frame(egui::Modifiers::NONE, vec![]));
    assert!(labels.iter().any(|(text, _, _)| text == "/docs/deep"), "{labels:?}");
}

#[test]
fn ctrl_shift_f_searches_whole_paths_and_double_click_opens_folder() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.app.model.entries = find_entries();
    s.app.browse_folder("music");
    s.frame(egui::Modifiers::NONE, vec![]);
    press(&mut s, egui::Key::F, find_ctrl(true));
    assert!(s.app.find_open());
    s.app.set_find_query("deep");
    let labels = layout_text(&s.frame(egui::Modifiers::NONE, vec![]));
    assert!(
        labels
            .iter()
            .any(|(text, _, _)| text == "압축파일 전체 경로에서 찾습니다"),
        "{labels:?}"
    );
    // 첫 "deep"은 검색어 입력란이므로 마지막(결과 행)을 고름.
    let row = labels
        .iter()
        .rev()
        .find(|(text, _, _)| text == "deep")
        .map(|(_, rect, _)| rect.center())
        .expect("찾은 폴더 행");
    s.time += 1.0;
    for _ in 0..2 {
        s.pointer(row, true, egui::Modifiers::NONE);
        s.pointer(row, false, egui::Modifiers::NONE);
    }
    let after = layout_text(&s.frame(egui::Modifiers::NONE, vec![]));
    assert!(!s.app.find_open(), "{row:?} {after:?}");
    let labels = layout_text(&s.frame(egui::Modifiers::NONE, vec![]));
    assert!(labels.iter().any(|(text, _, _)| text == "/docs/deep"), "{labels:?}");
    assert!(labels.iter().any(|(text, _, _)| text == "report.TXT"), "{labels:?}");
}

#[test]
fn escape_closes_child_windows_first_and_then_quits() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    // 찾기 창이 떠 있으면 찾기 창만 닫음.
    press(&mut s, egui::Key::F, find_ctrl(false));
    assert!(s.app.find_open());
    let outputs = press(&mut s, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!s.app.find_open());
    assert!(!close_requested(&outputs));
    // 선택한 항목이 있으면 먼저 선택만 해제함.
    s.click("alpha.txt", egui::Modifiers::NONE);
    s.selected(&[1]);
    let outputs = press(&mut s, egui::Key::Escape, egui::Modifiers::NONE);
    s.selected(&[]);
    assert!(!close_requested(&outputs));
    // 만들기 창이 떠 있으면 그 창을 닫음.
    s.app.show_create(false);
    s.frame(egui::Modifiers::NONE, vec![]);
    let shown = |s: &mut Explorer| {
        layout_text(&s.frame(egui::Modifiers::NONE, vec![]))
            .iter()
            .any(|(text, _, _)| text == "ZIP 만들기")
    };
    assert!(
        shown(&mut s),
        "{:?}",
        layout_text(&s.frame(egui::Modifiers::NONE, vec![]))
    );
    let outputs = press(&mut s, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!close_requested(&outputs));
    assert!(!shown(&mut s));
    // 아무 자식 창도 없으면 프로그램을 종료함.
    let outputs = press(&mut s, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(close_requested(&outputs));
}

#[test]
fn escape_in_the_search_box_only_leaves_the_text_field() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.ctx
        .memory_mut(|m| m.request_focus(egui::Id::unique("archive-search")));
    s.frame(egui::Modifiers::NONE, vec![]);
    assert!(s.ctx.text_edit_focused());
    let outputs = press(&mut s, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!close_requested(&outputs));
}

#[test]
fn enter_after_clicking_a_result_in_the_parent_find_window_moves_there() {
    let scratch = Folder::new();
    let mut s = Explorer::new(&scratch);
    s.app.model.entries = find_entries();
    s.frame(egui::Modifiers::NONE, vec![]);
    press(&mut s, egui::Key::F, find_ctrl(false));
    s.app.set_find_query("report");
    let labels = layout_text(&s.frame(egui::Modifiers::NONE, vec![]));
    let row = labels
        .iter()
        .rev()
        .find(|(text, _, _)| text == "report.TXT")
        .map(|(_, rect, _)| rect.center())
        .expect("찾은 파일 행");
    // 결과를 한 번 누르면 입력란은 초점을 잃음.
    s.time += 1.0;
    s.pointer(row, true, egui::Modifiers::NONE);
    s.pointer(row, false, egui::Modifiers::NONE);
    s.frame(egui::Modifiers::NONE, vec![]);
    assert!(s.app.find_open());
    assert!(!s.ctx.text_edit_focused());
    press(&mut s, egui::Key::Enter, egui::Modifiers::NONE);
    s.frame(egui::Modifiers::NONE, vec![]);
    assert!(!s.app.find_open());
    s.selected(&[1]);
    // 화살표로 고른 뒤 Enter도 같음.
    press(&mut s, egui::Key::F, find_ctrl(true));
    s.app.set_find_query("txt");
    let labels = layout_text(&s.frame(egui::Modifiers::NONE, vec![]));
    let first = labels
        .iter()
        .rev()
        .find(|(text, _, _)| text == "child.txt")
        .map(|(_, rect, _)| rect.center())
        .expect("첫 결과 행");
    s.time += 1.0;
    s.pointer(first, true, egui::Modifiers::NONE);
    s.pointer(first, false, egui::Modifiers::NONE);
    press(&mut s, egui::Key::ArrowDown, egui::Modifiers::NONE);
    press(&mut s, egui::Key::Enter, egui::Modifiers::NONE);
    s.frame(egui::Modifiers::NONE, vec![]);
    assert!(!s.app.find_open());
    s.selected(&[1]);
}
