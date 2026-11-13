fn native_fixture(scratch: &Scratch, fixture: &str, name: &str) -> PathBuf {
    let target = scratch.path(name);
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/rust-app-real-worker/fixtures")
            .join(fixture),
        &target,
    )
    .unwrap();
    target
}

#[test]
fn raw_gzip_save_dialog_corrects_names_and_preserves_cancel_and_conflict_choice() {
    use zipelo_ui_memory_app::archive_dialog::Purpose;
    let mut names = vec![".gz", "report.GZ", "report.Gz", "report.gz"];
    #[cfg(unix)]
    names.push("report:2026.gz");
    for name in names {
        let scratch = Scratch::new();
        let archive = native_fixture(&scratch, "sample.gz", name);
        let original = fs::read(&archive).unwrap();
        let output = make_directory(&scratch.path("output"));
        let (factory, observed) = observed_factory();
        let mut screen = Screen::new(factory);
        screen.open(&archive);
        assert_eq!(
            screen.app.model.list_id.as_ref().unwrap().format,
            Format::Gzip
        );
        screen.click("압축해제");
        assert!(!screen.app.model.busy());
        assert_eq!(observed.borrow().jobs.len(), 1);
        let request = screen.dialogs.0.borrow().requests.last().unwrap().clone();
        assert_eq!(request.purpose, Purpose::SaveGzip);
        assert_eq!(request.directory, scratch.0);
        assert_eq!(
            request.file_name,
            if name == ".gz" || name.contains(':') {
                "압축 해제 파일"
            } else {
                "report"
            }
        );
        let existing = output.join("기존 파일.txt");
        fs::write(&existing, b"preserve").unwrap();
        screen
            .dialogs
            .0
            .borrow_mut()
            .answers
            .push_back(Some(vec![existing.clone()]));
        screen.click("압축해제");
        let conflict = screen.await_conflict();
        assert_eq!(conflict.relative_name, "기존 파일.txt");
        screen.click("이름 바꿔 저장");
        screen.settle_conflict_result();
        completed(&screen.app.model);
        let saved = output.join("기존 파일 (1).txt");
        assert_eq!(fs::read(saved).unwrap(), b"Zipelo real worker flow\n");
        assert_eq!(fs::read(existing).unwrap(), b"preserve");
        assert_eq!(fs::read(&archive).unwrap(), original);
        assert!(matches!(
            observed.borrow().starts.last(),
            Some(Start::Extract { destination, gzip_output_name: Some(name), .. })
            if destination == &output && name == "기존 파일.txt"
        ));
        assert_reaped(&observed);
    }
}

#[test]
fn mixed_case_zip_names_open_and_extract_through_native_dialog_responses() {
    use zipelo_ui_memory_app::archive_dialog::Purpose;
    for name in ["sample.ZIP", "sample.Zip", "sample.zIp"] {
        let scratch = Scratch::new();
        let archive = native_fixture(&scratch, "sample.zip", name);
        let original = fs::read(&archive).unwrap();
        let output = make_directory(&scratch.path("output"));
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
            Format::Zip
        );
        screen.extract(&output);
        assert_eq!(
            fs::read(output.join("content.txt")).unwrap(),
            b"Zipelo real worker flow\n"
        );
        assert_eq!(fs::read(archive).unwrap(), original);
        assert_reaped(&observed);
    }
}
