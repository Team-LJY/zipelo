#[test]
fn compressed_tar_opens_with_native_dialog_and_extracts_to_compound_stem() {
    use zipelo_ui_memory_app::archive_dialog::Purpose;
    for (kind, name) in [("bz2", "여행 자료.TaR.Bz2"), ("xz", "여행 자료.tAr.XZ")] {
        let scratch = Scratch::new();
        let generated = std::process::Command::new("python3")
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/rust-archive-worker/compressed_tar_cases.py"),
            )
            .arg(&scratch.0)
            .arg(kind)
            .arg("normal")
            .output()
            .unwrap();
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let archive = scratch.path(name);
        fs::rename(scratch.path("input"), &archive).unwrap();
        let original = fs::read(&archive).unwrap();
        let (factory, observed) = observed_factory();
        let mut screen = Screen::new(factory);
        screen
            .dialogs
            .0
            .borrow_mut()
            .answers
            .push_back(Some(vec![archive.clone()]));
        screen.click("열기");
        screen.settle();
        assert_eq!(screen.dialogs.0.borrow().requests[0].purpose, Purpose::Open);
        assert_eq!(
            screen.app.model.list_id.as_ref().unwrap().format,
            Format::Tar
        );
        assert!(
            screen
                .app
                .model
                .entries
                .iter()
                .any(|e| e.name == "한글.txt")
        );
        assert!(screen.app.begin_extract());
        screen.settle();
        let output = scratch.path("여행 자료");
        assert_eq!(
            fs::read(output.join("folder/payload.bin")).unwrap(),
            fs::read(scratch.path("expected")).unwrap()
        );
        assert_eq!(fs::read(output.join("한글.txt")).unwrap(), b"end");
        assert!(output.join("empty").is_dir());
        assert_eq!(fs::read(&archive).unwrap(), original);
        assert!(
            matches!(observed.borrow().starts.last(), Some(Start::Extract {
            destination, gzip_output_name: None, ..
        }) if destination == &output)
        );
        let labels = screen
            .render(vec![])
            .into_iter()
            .map(|(s, _)| s)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(labels.contains("압축을 풀었습니다"), "{labels}");
        assert_reaped(&observed);
    }
}
