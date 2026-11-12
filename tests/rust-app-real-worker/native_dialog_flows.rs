fn native_fixture(scratch: &Scratch, fixture: &str, name: &str) -> PathBuf {
    let target = scratch.path(name);
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/rust-app-real-worker/fixtures")
            .join(fixture),
        &target,
    ).unwrap();
    target
}

#[test]
fn raw_gzip_save_dialog_corrects_names_and_preserves_cancel_and_conflict_retry() {
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
        assert_eq!(screen.app.model.list_id.as_ref().unwrap().format, Format::Gzip);
        screen.click("압축해제");
        assert!(!screen.app.model.busy());
        assert_eq!(observed.borrow().jobs.len(), 1);
        let request = screen.dialogs.0.borrow().requests.last().unwrap().clone();
        assert_eq!(request.purpose, Purpose::SaveGzip);
        assert_eq!(request.directory, scratch.0);
        assert_eq!(request.file_name, if name == ".gz" || name.contains(':') { "압축 해제 파일" } else { "report" });
        let existing = output.join("기존 파일.txt");
        fs::write(&existing, b"preserve").unwrap();
        screen.dialogs.0.borrow_mut().answers.push_back(Some(vec![existing.clone()]));
        screen.click("압축해제");
        let deadline = Instant::now() + Duration::from_secs(10);
        while screen.app.model.busy() {
            screen.render(vec![]);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(screen.app.model.terminal.as_ref().unwrap().error, Some(ErrorCode::Conflict));
        assert_eq!(fs::read(&existing).unwrap(), b"preserve");
        screen.click("다른 이름으로 저장");
        assert!(!screen.app.model.busy());
        assert_eq!(observed.borrow().jobs.len(), 2);
        assert_eq!(screen.dialogs.0.borrow().requests.last().unwrap().purpose, Purpose::RetryGzip);
        let saved = output.join("사용자 이름.txt");
        screen.dialogs.0.borrow_mut().answers.push_back(Some(vec![saved.clone()]));
        screen.click("다른 이름으로 저장");
        screen.settle();
        assert_eq!(fs::read(saved).unwrap(), b"Zipelo real worker flow\n");
        assert_eq!(fs::read(existing).unwrap(), b"preserve");
        assert_eq!(fs::read(&archive).unwrap(), original);
        assert!(matches!(
            observed.borrow().starts.last(),
            Some(Start::Extract { destination, gzip_output_name: Some(name), .. })
            if destination == &output && name == "사용자 이름.txt"
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
        screen.dialogs.0.borrow_mut().answers.push_back(Some(vec![archive.clone()]));
        screen.click("열기");
        screen.settle();
        assert_eq!(screen.dialogs.0.borrow().requests[0].purpose, Purpose::Open);
        assert_eq!(screen.app.model.list_id.as_ref().unwrap().format, Format::Zip);
        screen.extract(&output);
        assert_eq!(fs::read(output.join("content.txt")).unwrap(), b"Zipelo real worker flow\n");
        assert_eq!(fs::read(archive).unwrap(), original);
        assert_reaped(&observed);
    }
}
