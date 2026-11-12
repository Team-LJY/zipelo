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
    screen.click("만들기");
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
        ("directory", Some(&["docs"]), &["docs"]),
        (
            "mixed",
            Some(&["docs", "docs/nested/one.txt"]),
            &["docs", "docs/nested", "docs/nested/one.txt"],
        ),
        ("empty", Some(&["docs/empty"]), &["docs", "docs/empty"]),
        (
            "nested-directory",
            Some(&["docs/nested"]),
            &["docs", "docs/nested"],
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
            screen.click("선택한 항목만 풀기");
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
                    assert_eq!(actual, &ids.iter().copied().collect::<Vec<_>>(), "{name}");
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
