#[test]
fn extraction_completion_matches_actual_worker_selection_matrix() {
    use std::collections::BTreeSet;

    fn contents(root: &Path) -> BTreeSet<String> {
        fn visit(root: &Path, current: &Path, found: &mut BTreeSet<String>) {
            for entry in fs::read_dir(current).unwrap() {
                let path = entry.unwrap().path();
                found.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .replace('\\', "/"),
                );
                if path.is_dir() {
                    visit(root, &path, found);
                }
            }
        }
        let mut found = BTreeSet::new();
        visit(root, root, &mut found);
        found
    }

    let scratch = Scratch::new();
    let docs = make_directory(&scratch.path("docs"));
    fs::create_dir(docs.join("empty")).unwrap();
    fs::create_dir(docs.join("nested")).unwrap();
    fs::write(docs.join("nested/one.txt"), b"nested content").unwrap();
    let root_file = scratch.path("root.txt");
    fs::write(&root_file, b"root content").unwrap();
    let archive = scratch.path("selection.zip");
    let (factory, observed) = observed_factory();
    let mut screen = Screen::new(factory);
    screen.app.add_sources(vec![docs, root_file]);
    screen.render(vec![]);
    screen.set_text("zip-name", "selection.zip");
    screen.click("압축하기");
    screen.settle();

    let cases: [(&str, Option<&[&str]>, &[&str]); 6] = [
        (
            "all",
            None,
            &[
                "docs",
                "docs/empty",
                "docs/nested",
                "docs/nested/one.txt",
                "root.txt",
            ],
        ),
        (
            "files",
            Some(&["docs/nested/one.txt", "root.txt"]),
            &["docs", "docs/nested", "docs/nested/one.txt", "root.txt"],
        ),
        (
            "directory",
            Some(&["docs"]),
            &["docs", "docs/empty", "docs/nested", "docs/nested/one.txt"],
        ),
        (
            "mixed",
            Some(&["docs", "docs/nested/one.txt"]),
            &["docs", "docs/empty", "docs/nested", "docs/nested/one.txt"],
        ),
        ("empty", Some(&["docs/empty"]), &["docs", "docs/empty"]),
        (
            "nested-directory",
            Some(&["docs/nested"]),
            &["docs", "docs/nested", "docs/nested/one.txt"],
        ),
    ];
    for (name, selected, expected) in cases {
        screen.open(&archive);
        let ids = selected.map(|names| {
            names
                .iter()
                .map(|name| {
                    screen
                        .app
                        .model
                        .entries
                        .iter()
                        .find(|entry| entry.name.trim_end_matches('/') == *name)
                        .unwrap_or_else(|| panic!("목록 항목 누락: {name}"))
                        .id
                })
                .collect::<BTreeSet<_>>()
        });
        if let Some(ids) = &ids {
            screen.app.model.selected = ids.clone();
        }
        let output = make_directory(&scratch.path(name));
        screen.extract(&output);
        let terminal = screen.app.model.terminal.as_ref().unwrap();
        assert_eq!(terminal.outcome, Outcome::Completed, "{name}");
        assert!(screen.app.model.confirmed, "{name}");
        assert!(terminal.error.is_none(), "{name}");
        let expected = expected
            .iter()
            .map(|name| name.to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(contents(&output), expected, "{name}");
        let reports = screen
            .app
            .model
            .outputs
            .iter()
            .map(|(name, disposition)| {
                assert_eq!(*disposition, Disposition::Created);
                name.clone()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(reports, expected, "{name}");
        assert_eq!(terminal.created_count as usize, expected.len(), "{name}");
        for (path, bytes) in [
            ("docs/nested/one.txt", b"nested content".as_slice()),
            ("root.txt", b"root content".as_slice()),
        ] {
            if expected.contains(path) {
                assert_eq!(fs::read(output.join(path)).unwrap(), bytes, "{name}");
            }
        }
        match observed.borrow().starts.last().unwrap() {
            Start::Extract { selection, .. } => match (selection, &ids) {
                (Selection::All, None) => {}
                (Selection::EntryIds(actual), Some(ids)) => {
                    assert_eq!(
                        actual,
                        &zipelo_ui_memory_app::archive_browser::expand_selection(
                            &screen.app.model.entries,
                            ids
                        )
                        .into_iter()
                        .collect::<Vec<_>>(),
                        "{name}"
                    );
                }
                _ => panic!("요청 선택 범위 불일치: {name}"),
            },
            _ => panic!("추출 요청 누락: {name}"),
        }
        let shown = screen.render(vec![]);
        assert!(
            shown.iter().any(|(text, _)| text == "압축을 풀었습니다"),
            "{name}: {shown:?}"
        );
        assert!(
            !shown
                .iter()
                .any(|(text, _)| text.contains("저장이 끝났는지") || text.contains("일부 파일만")),
            "{name}"
        );
        screen.click("확인");
    }
    assert_reaped(&observed);
}

#[cfg(unix)]
#[test]
fn auto_destination_is_private_under_desktop_umask() {
    use std::{os::unix::fs::PermissionsExt, process::Command};

    const CHILD: &str = "ZIPELO_PRIVATE_DESTINATION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = Command::new("sh")
            .args([
                "-c",
                "umask 0002; exec \"$@\"",
                "zipelo-private-destination",
            ])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "screen::auto_destination_is_private_under_desktop_umask",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "umask 0002 추출 회귀 실패: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let mask = Command::new("sh").args(["-c", "umask"]).output().unwrap();
    assert_eq!(String::from_utf8(mask.stdout).unwrap().trim(), "0002");
    let scratch = Scratch::new();
    let archive = scratch.path("sample.zip");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/rust-app-real-worker/fixtures/sample.zip"),
        &archive,
    )
    .unwrap();
    let output = scratch.path("sample");
    assert!(!output.exists());
    let (factory, observed) = observed_factory();
    let mut screen = Screen::new(factory);
    screen.open(&archive);
    assert!(screen.app.begin_extract());
    assert!(screen.app.model.busy());
    screen.settle();
    assert_eq!(
        fs::metadata(&output).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::read(output.join("content.txt")).unwrap(),
        b"Zipelo real worker flow\n"
    );
    assert_eq!(
        screen.app.model.terminal.as_ref().unwrap().publication,
        Publication::Complete
    );
    assert!(
        screen
            .render(vec![])
            .iter()
            .any(|(text, _)| text == "압축을 풀었습니다")
    );
    assert_reaped(&observed);
}

#[test]
fn explorer_folder_click_and_escape_control_actual_extraction_scope() {
    let scratch = Scratch::new();
    let folder = make_directory(&scratch.path("docs"));
    fs::write(folder.join("child.txt"), b"selected subtree").unwrap();
    let outside = scratch.path("outside.txt");
    fs::write(&outside, b"outside subtree").unwrap();
    let archive = scratch.path("picked.zip");
    let selected_output = make_directory(&scratch.path("selected-output"));
    let all_output = make_directory(&scratch.path("all-output"));
    let (factory, observed) = observed_factory();
    let mut screen = Screen::new(factory);
    screen.app.add_sources(vec![folder, outside]);
    screen.render(vec![]);
    screen.set_text("zip-name", "picked.zip");
    screen.click("압축하기");
    screen.settle();
    screen.open(&archive);
    screen.click("docs");
    assert!(!screen.app.model.selected.is_empty());
    screen.extract(&selected_output);
    assert_eq!(
        fs::read(selected_output.join("docs/child.txt")).unwrap(),
        b"selected subtree"
    );
    assert!(!selected_output.join("outside.txt").exists());
    assert!(matches!(
        observed.borrow().starts.last(),
        Some(Start::Extract {
            selection: Selection::EntryIds(_),
            ..
        })
    ));
    screen.click("확인");
    screen
        .context
        .memory_mut(|memory| memory.request_focus(egui::Id::unique("archive-list")));
    screen.render(vec![]);
    screen.escape();
    assert!(screen.app.model.selected.is_empty());
    screen.extract(&all_output);
    assert_eq!(
        fs::read(all_output.join("outside.txt")).unwrap(),
        b"outside subtree"
    );
    assert_eq!(
        fs::read(all_output.join("docs/child.txt")).unwrap(),
        b"selected subtree"
    );
    assert!(matches!(
        observed.borrow().starts.last(),
        Some(Start::Extract {
            selection: Selection::All,
            ..
        })
    ));
    assert_reaped(&observed);
}
