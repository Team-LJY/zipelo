const FIXTURE_PASSWORD: &str = "fixture-password";
fn encrypted_fixture(scratch: &Scratch, name: &str) -> PathBuf {
    let archive = scratch.path(name);
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/rust-archive-worker/fixtures")
            .join(name),
        &archive,
    )
    .unwrap();
    archive
}
impl Screen {
    fn password_request(&mut self, expected_attempt: u32, encrypt: bool) -> u64 {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.render(vec![]);
            assert!(self.app.model.busy(), "{}", self.app.model.status());
            if let Some((id, attempt, purpose)) = self.app.model.challenge {
                assert_eq!(attempt, expected_attempt);
                assert_eq!(purpose, encrypt);
                self.render(vec![]);
                return id;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn password_answer(&mut self, text: &str) {
        self.render(vec![]);
        let input = self
            .context
            .read_response(egui::Id::unique("archive-password-input"))
            .expect("암호 입력란이 없음")
            .rect
            .center();
        self.click_at(input);
        let rendered = self.render(vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            },
            egui::Event::Text(text.into()),
        ]);
        if !text.is_empty() {
            assert!(!rendered.iter().any(|(value, _)| value.contains(text)));
        }
        if self
            .app
            .model
            .challenge
            .is_some_and(|(_, _, encrypt)| encrypt)
        {
            self.set_text("archive-password-confirm", text);
        }
        self.click("확인");
    }
    fn start_password_extract(&mut self, output: &Path) {
        self.app.choose_destination(output.to_path_buf());
        self.dialogs
            .0
            .borrow_mut()
            .answers
            .push_back(Some(vec![output.to_path_buf()]));
        self.click("압축해제");
        assert!(self.app.model.busy());
    }
    fn finish_password_job(&mut self, expected: Outcome) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.app.model.busy() {
            self.render(vec![]);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(self.app.model.confirmed, "{}", self.app.model.status());
        assert_eq!(self.app.model.terminal.as_ref().unwrap().outcome, expected);
        assert!(self.app.model.challenge.is_none());
    }
    /// ZIP 만들기 창에서 암호로 보호하기를 켜고 두 칸에 암호를 넣은 뒤 압축하기를 누름.
    /// 작업이 시작됐는지 돌려줌.
    fn start_encrypted_zip(&mut self, source: &Path, archive: &Path, password: &str) -> bool {
        self.app.add_sources(vec![source.to_path_buf()]);
        self.render(vec![]);
        self.set_text("zip-name", archive.file_name().unwrap().to_str().unwrap());
        self.click("암호로 보호하기");
        self.set_text("zip-password-input", password);
        self.set_text("zip-password-confirm", password);
        if !password.is_empty() {
            // 입력한 암호는 화면에 그대로 보이지 않음.
            assert!(!self.render(vec![]).iter().any(|(value, _)| value.contains(password)));
        }
        self.click("압축하기");
        self.app.model.busy()
    }
    /// 별도 암호 입력 창 없이 작업이 끝나는지 확인함.
    fn finish_without_password_prompt(&mut self, expected: Outcome) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.app.model.busy() {
            let shown = self.render(vec![]);
            assert!(!shown.iter().any(|(value, _)| value == "ZIP 암호 설정"));
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(self.app.model.confirmed, "{}", self.app.model.status());
        assert_eq!(self.app.model.terminal.as_ref().unwrap().outcome, expected);
        assert!(self.app.model.challenge.is_none());
    }
}
#[test]
fn encrypted_zip_list_and_extract_with_actual_password_dialog() {
    for name in [
        "zipcrypto-store.zip",
        "zipcrypto-deflate.zip",
        "aes1-store.zip",
        "aes1-deflate.zip",
        "aes2-store.zip",
        "aes2-deflate.zip",
    ] {
        let scratch = Scratch::new();
        let archive = encrypted_fixture(&scratch, name);
        let original = fs::read(&archive).unwrap();
        let output = make_directory(&scratch.path("output"));
        let (factory, observed) = observed_factory();
        let mut screen = Screen::new(factory);
        screen.open(&archive);
        assert!(screen.app.model.entries.iter().all(|entry| entry.encrypted));
        assert!(screen.app.model.challenge.is_none());
        screen.start_password_extract(&output);
        screen.password_request(1, false);
        assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
        screen.password_answer(FIXTURE_PASSWORD);
        screen.finish_password_job(Outcome::Completed);
        assert_eq!(
            fs::read(output.join("payload.bin")).unwrap(),
            b"Zipelo encrypted interoperability\n".repeat(8)
        );
        assert_eq!(fs::read(archive).unwrap(), original);
        assert_reaped(&observed);
    }
}
#[test]
fn wrong_password_retry_clears_input_and_succeeds_without_partial_output() {
    let scratch = Scratch::new();
    let archive = encrypted_fixture(&scratch, "aes2-deflate.zip");
    let output = make_directory(&scratch.path("output"));
    let (factory, observed) = observed_factory();
    let mut screen = Screen::new(factory);
    screen.open(&archive);
    screen.start_password_extract(&output);
    let first = screen.password_request(1, false);
    screen.password_answer("wrong-password");
    let second = screen.password_request(2, false);
    assert_ne!(first, second);
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    let labels = screen.render(vec![]);
    assert!(
        labels
            .iter()
            .any(|(value, _)| value == "암호가 맞지 않습니다. 다시 입력하세요")
    );
    screen.click("확인");
    assert_eq!(screen.app.model.challenge, Some((second, 2, false)));
    assert!(screen.app.model.input_error.is_none());
    screen.render(vec![]);
    screen.password_answer(FIXTURE_PASSWORD);
    screen.finish_password_job(Outcome::Completed);
    assert_reaped(&observed);
}
#[test]
fn three_wrong_passwords_fail_without_writes_and_can_start_a_new_job() {
    let scratch = Scratch::new();
    let archive = encrypted_fixture(&scratch, "zipcrypto-deflate.zip");
    let output = make_directory(&scratch.path("output"));
    let (factory, observed) = observed_factory();
    let mut screen = Screen::new(factory);
    screen.open(&archive);
    screen.start_password_extract(&output);
    for attempt in 1..=3 {
        screen.password_request(attempt, false);
        assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
        screen.password_answer("wrong-password");
    }
    screen.finish_password_job(Outcome::Failed);
    assert_eq!(
        screen.app.model.terminal.as_ref().unwrap().error,
        Some(ErrorCode::WrongPassword)
    );
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    screen.open(&archive);
    assert_reaped(&observed);
}
#[test]
fn cancel_password_extraction_waits_for_cleanup_without_writes() {
    let scratch = Scratch::new();
    let archive = encrypted_fixture(&scratch, "aes1-store.zip");
    let output = make_directory(&scratch.path("output"));
    fs::write(output.join("existing.txt"), b"preserve").unwrap();
    let (factory, observed) = observed_factory();
    let mut screen = Screen::new(factory);
    screen.open(&archive);
    screen.start_password_extract(&output);
    screen.password_request(1, false);
    screen.escape();
    screen.app.model.request_close();
    screen.app.model.confirm_close();
    assert!(matches!(
        screen.app.model.close,
        Close::Waiting | Close::Review
    ));
    screen.finish_password_job(Outcome::Cancelled);
    // 정리가 끝나면 결과 확인 안내 없이 바로 닫힘.
    assert_eq!(screen.app.model.close, Close::Allowed);
    assert_eq!(fs::read(output.join("existing.txt")).unwrap(), b"preserve");
    assert_eq!(fs::read_dir(output).unwrap().count(), 1);
    assert_reaped(&observed);
}
#[test]
fn cold_start_creates_aes_zip_then_reopens_and_extracts_with_password() {
    let scratch = Scratch::new();
    let source = scratch.path("private.txt");
    let payload = b"Public synthetic password flow payload\n";
    fs::write(&source, payload).unwrap();
    let archive = scratch.path("encrypted.zip");
    let output = make_directory(&scratch.path("output"));
    let (factory, observed) = observed_factory();
    let mut screen = Screen::new(factory);
    assert!(!screen.app.model.encryption);
    // 암호는 ZIP 만들기 창에서 받고, 작업자가 요청하면 별도 창 없이 넘김.
    assert!(screen.start_encrypted_zip(&source, &archive, FIXTURE_PASSWORD));
    screen.finish_without_password_prompt(Outcome::Completed);
    let bytes = fs::read(&archive).unwrap();
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    assert_eq!(u16::from_le_bytes(bytes[6..8].try_into().unwrap()) & 1, 1);
    assert_eq!(u16::from_le_bytes(bytes[8..10].try_into().unwrap()), 99);
    screen.open(&archive);
    assert!(screen.app.model.entries.iter().all(|entry| entry.encrypted));
    screen.start_password_extract(&output);
    screen.password_request(1, false);
    screen.password_answer(FIXTURE_PASSWORD);
    screen.finish_password_job(Outcome::Completed);
    assert_eq!(fs::read(output.join("private.txt")).unwrap(), payload);
    assert_eq!(fs::read(source).unwrap(), payload);
    assert_reaped(&observed);
}
#[test]
fn cancel_encrypted_creation_window_starts_no_worker_or_archive() {
    let scratch = Scratch::new();
    let source = scratch.path("private.txt");
    fs::write(&source, b"preserve").unwrap();
    let archive = scratch.path("cancelled.zip");
    let (factory, observed) = observed_factory();
    let mut screen = Screen::new(factory);
    screen.app.add_sources(vec![source.clone()]);
    screen.render(vec![]);
    screen.click("암호로 보호하기");
    screen.set_text("zip-password-input", FIXTURE_PASSWORD);
    screen.set_text("zip-password-confirm", FIXTURE_PASSWORD);
    screen.click("취소");
    let shown = screen.render(vec![]);
    assert!(!shown.iter().any(|(value, _)| value == "ZIP 만들기"));
    assert!(!screen.app.model.busy());
    assert!(observed.borrow().jobs.is_empty());
    assert!(!archive.exists());
    assert_eq!(fs::read(source).unwrap(), b"preserve");
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
    assert_reaped(&observed);
}

#[test]
fn oversized_utf8_password_stays_in_dialog_then_accepts_valid_input() {
    let scratch = Scratch::new();
    let archive = encrypted_fixture(&scratch, "aes2-store.zip");
    let output = make_directory(&scratch.path("output"));
    let (factory, observed) = observed_factory();
    let mut screen = Screen::new(factory);
    screen.open(&archive);
    screen.start_password_extract(&output);
    let id = screen.password_request(1, false);
    screen.password_answer(&"가".repeat(342));
    assert_eq!(screen.app.model.challenge, Some((id, 1, false)));
    assert!(
        screen
            .render(vec![])
            .iter()
            .any(|(text, _)| text.contains("1~1024바이트"))
    );
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    screen.password_answer(FIXTURE_PASSWORD);
    screen.finish_password_job(Outcome::Completed);
    assert_reaped(&observed);
}

fn invalid_zip_password_never_starts_a_worker(invalid: &str, close: bool) {
    let scratch = Scratch::new();
    let source = scratch.path("private.txt");
    fs::write(&source, b"preserve").unwrap();
    let archive = scratch.path("cancelled.zip");
    let (factory, observed) = observed_factory();
    let mut screen = Screen::new(factory);
    // 빈 암호나 1024바이트를 넘는 암호로는 압축하기를 누를 수 없음.
    assert!(!screen.start_encrypted_zip(&source, &archive, invalid));
    assert!(observed.borrow().jobs.is_empty());
    if !invalid.is_empty() {
        assert!(
            screen
                .render(vec![])
                .iter()
                .any(|(text, _)| text.contains("1~1024바이트"))
        );
    }
    if close {
        // 작업이 없으므로 확인만 거치면 닫을 수 있음.
        screen.app.model.request_close();
        assert_eq!(screen.app.model.close, Close::Review);
        screen.app.model.acknowledge_close();
        assert_eq!(screen.app.model.close, Close::Allowed);
    } else {
        screen.click("취소");
        assert!(
            !screen
                .render(vec![])
                .iter()
                .any(|(value, _)| value == "ZIP 만들기")
        );
    }
    assert!(!archive.exists());
    assert_eq!(fs::read(&source).unwrap(), b"preserve");
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
    assert_reaped(&observed);
    if !close {
        screen.open(&encrypted_fixture(&scratch, "aes2-store.zip"));
        assert_reaped(&observed);
    }
}
#[test]
fn empty_zip_password_never_starts_a_worker() {
    invalid_zip_password_never_starts_a_worker("", false);
}
#[test]
fn oversized_zip_password_never_starts_a_worker() {
    invalid_zip_password_never_starts_a_worker(&"가".repeat(342), false);
}
#[test]
fn invalid_zip_password_then_close_allows_closing() {
    invalid_zip_password_never_starts_a_worker("", true);
}
