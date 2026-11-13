use super::*;
use std::ffi::OsString;

#[path = "fake.rs"]
mod fake;
use fake::FakeFactory;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Scenario {
    pub name: &'static str,
    pub description: &'static str,
}

macro_rules! catalog {
    ($($name:literal => $description:literal),+ $(,)?) => {
        pub const SCENARIOS: &[Scenario] = &[$(Scenario {
            name: $name,
            description: $description,
        }),+];
    };
}

catalog! {
    "gzip-conflict-choice" => "단일 GZ 파일의 충돌 선택",
    "conflict-choice" => "같은 이름 파일의 세 가지 처리 선택",
    "conflict-apply" => "남은 충돌에 같은 선택 적용",
    "extract-skipped" => "모든 파일을 건너뛴 정상 완료",
    "extract-renamed" => "이름을 바꿔 저장한 완료 결과",
    "extract-rename-failed" => "새 이름을 정한 뒤 저장 전 I/O 실패",
    "empty" => "빈 첫 화면",
    "archive" => "폴더와 파일이 있는 압축파일",
    "tar-bz2" => "TAR.BZ2 목록과 자동 해제 폴더",
    "tar-xz" => "TAR.XZ 목록과 자동 해제 폴더",
    "folder" => "사진 하위 폴더",
    "many-items" => "500개 항목과 세로 스크롤",
    "selection" => "여러 파일과 폴더 선택",
    "marquee" => "반투명 사각형으로 끌어서 선택 중",
    "search-empty" => "검색 결과 없음",
    "password" => "압축 해제 암호 입력",
    "password-retry" => "암호가 틀린 뒤 다시 입력",
    "extract-progress" => "압축 해제 진행률 62%",
    "extract-unknown" => "전체 크기를 모르는 압축 해제",
    "extract-complete" => "압축 해제 완료",
    "extract-cancelled" => "압축 해제 취소 완료",
    "cancelling" => "취소 요청 후 작업 종료 대기",
    "conflict" => "같은 이름 파일 충돌",
    "unsupported" => "지원하지 않는 압축 형식",
    "damaged" => "손상된 압축파일",
    "create" => "암호 없는 ZIP 만들기 창",
    "create-encrypted" => "암호 설정이 켜진 ZIP 만들기 창",
    "create-progress" => "ZIP 만들기 진행률 62%",
    "create-complete" => "ZIP 만들기 완료",
    "folder-empty" => "항목이 없는 하위 폴더",
    "gzip-conflict" => "단일 GZ 파일 이름 충돌",
    "extract-partial" => "일부 파일만 풀린 결과",
    "extract-uncertain" => "저장 여부를 확인할 수 없는 결과",
    "extract-unsynced" => "완료 뒤 저장 확인 경고의 상세 정보",
    "source-changed" => "목록을 연 뒤 원본이 바뀐 오류",
    "dialog-error" => "운영체제 파일 선택 창 실행 실패",
    "create-error" => "ZIP 만들기 중 디스크 공간 부족",
    "password-create" => "암호 ZIP 생성의 작업자 암호 요청",
    "close-confirm" => "진행 중 창 닫기 확인",
    "uninstall-windows" => "Windows 제거 전 기본 앱 선택 안내",
    "uninstall-settings-error" => "Windows 기본 앱 설정 실행 실패",
    "uninstall-ubuntu-missing" => "Ubuntu 이전 기본 앱 복원 불가 안내",
    "uninstall-ubuntu-unknown-user" => "Ubuntu 제거 사용자 확인 불가 안내",
}

pub fn listing() -> String {
    SCENARIOS
        .iter()
        .map(|s| format!("{}\t{}\n", s.name, s.description))
        .collect()
}

pub fn parse(name: &str) -> Result<Scenario, String> {
    SCENARIOS
        .iter()
        .copied()
        .find(|s| s.name == name)
        .ok_or_else(|| {
            format!(
                "알 수 없는 UI 상황: {name}\n사용 가능한 상황:\n{}",
                listing()
            )
        })
}

#[derive(Debug, PartialEq)]
pub enum Launch {
    Normal(Vec<OsString>),
    Scenario(Scenario),
    List,
}

pub fn launch(args: Vec<OsString>) -> Result<Launch, String> {
    let requested = args
        .iter()
        .any(|a| a == "--ui-scenario" || a == "--list-ui-scenarios");
    if !requested {
        return Ok(Launch::Normal(args));
    }
    if args.len() == 1 && args[0] == "--list-ui-scenarios" {
        return Ok(Launch::List);
    }
    if args.len() == 2 && args[0] == "--ui-scenario" {
        return args[1]
            .to_str()
            .ok_or_else(|| format!("상황 이름은 UTF-8이어야 합니다\n{}", listing()))
            .and_then(parse)
            .map(Launch::Scenario);
    }
    Err(format!(
        "사용법: --ui-scenario <이름> 또는 --list-ui-scenarios\n{}",
        listing()
    ))
}

fn sample_root() -> PathBuf {
    PathBuf::from(if cfg!(target_os = "windows") {
        "C:\\Samples"
    } else {
        "/Samples"
    })
}

fn entries(many: bool) -> Vec<Entry> {
    let mut entries = vec![
        ("사진", Kind::Directory),
        ("읽어주세요.txt", Kind::File),
        ("여행 일정.pdf", Kind::File),
        ("사진/바다.jpg", Kind::File),
        ("사진/풍경.jpg", Kind::File),
    ]
    .into_iter()
    .enumerate()
    .map(|(id, (name, kind))| Entry {
        id: id as u32,
        name: name.into(),
        kind,
        bytes: Some(if kind == Kind::Directory {
            0
        } else {
            1_287_952
        }),
        packed: Some(900_000),
        encrypted: id == 2,
    })
    .collect::<Vec<_>>();
    if many {
        entries.extend((5..505).map(|id| Entry {
            id,
            name: format!("자료-{id:03}.txt"),
            kind: Kind::File,
            bytes: Some(4096),
            packed: Some(1800),
            encrypted: false,
        }));
    }
    entries
}

impl Scenario {
    /// 제거 안내는 외부 설치 화면의 문구만 재현하며 OS 동작은 실행하지 않음.
    pub fn show(self, app: &mut ArchiveApp, ui: &mut egui::Ui) {
        let (title, messages, button): (&str, Vec<&str>, Option<&str>) = match self.name {
            "uninstall-windows" => (
                "제거 전 기본 앱 선택",
                vec![
                    "Zipelo를 기본 앱으로 사용했다면 제거 전에 다른 앱을 선택하세요. [기본 앱 설정 열기]에서 .zip, .7z, .tar, .gz, .tgz를 검색해 사용할 앱을 고를 수 있습니다. 다른 앱이 없으면 먼저 설치하세요.",
                    "직접 고른 기본 앱은 자동으로 바꾸지 않습니다. 제거 후 파일이 열리지 않으면 파일을 오른쪽 클릭하고 [연결 프로그램]에서 다른 앱을 선택하세요.",
                ],
                Some("기본 앱 설정 열기"),
            ),
            "uninstall-settings-error" => (
                "기본 앱 설정 실행 실패",
                vec![
                    "Windows 설정을 열지 못했습니다. 설정 > 앱 > 기본 앱에서 확장자를 검색해 다른 앱을 선택하세요.",
                ],
                Some("확인"),
            ),
            "uninstall-ubuntu-missing" => (
                "Ubuntu 제거 안내",
                vec![
                    "Zipelo를 제거합니다. 파일이 열리지 않으면 파일을 오른쪽 클릭하고 [다른 프로그램으로 열기]에서 다른 앱을 선택하세요.",
                    "이전 앱을 복원할 수 없습니다: application/zip. 파일을 오른쪽 클릭해 [다른 프로그램으로 열기]에서 앱을 선택하세요.",
                ],
                None,
            ),
            "uninstall-ubuntu-unknown-user" => (
                "Ubuntu 제거 안내",
                vec![
                    "Zipelo를 제거합니다. 파일이 열리지 않으면 파일을 오른쪽 클릭하고 [다른 프로그램으로 열기]에서 다른 앱을 선택하세요.",
                    "제거를 요청한 사용자를 확인할 수 없어 사용자 기본 앱 설정은 변경하지 않습니다.",
                ],
                None,
            ),
            _ => {
                app.show(ui);
                return;
            }
        };
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.add_space(16.0);
            ui.heading(title);
            ui.add_space(16.0);
            for message in messages {
                ui.label(message);
                ui.add_space(16.0);
            }
            if let Some(button) = button {
                // 미리보기에서는 설정 열기와 제거를 실행하지 않음.
                let _ = ui.button(button);
            }
        });
    }

    /// 실제 파일과 작업자를 사용하지 않고 기존 화면 상태를 준비함.
    pub fn build(self) -> ArchiveApp {
        let factory = FakeFactory::manual();
        let state = factory.state.clone();
        let mut app = ArchiveApp::new(Box::new(factory), None, true);
        let base = sample_root();
        app.destination = base.join("풀린 자료");
        app.automatic_destination = false;
        app.zip_folder = base.clone();
        app.zip_name = "여행 자료.zip".into();
        if self.name == "empty" || self.name.starts_with("uninstall-") {
            return app;
        }
        let archive = base.join(match self.name {
            "gzip-conflict" | "gzip-conflict-choice" => "여행 자료.gz",
            "tar-bz2" => "여행 자료.TAR.BZ2",
            "tar-xz" => "여행 자료.TaR.Xz",
            _ => "여행 자료.zip",
        });
        app.model.archive = Some(archive.clone());
        app.model.entries = entries(self.name == "many-items");
        app.model.list_id = Some(ListId {
            catalog_version: 1,
            format: Format::Zip,
            source_object: SourceObject::Unix {
                device: 1,
                inode: 1,
            },
            archive_bytes: 2_700_000,
            archive_sha256: [7; 32],
        });
        app.model.confirmed = true;
        match self.name {
            "tar-bz2" | "tar-xz" => {
                app.model.list_id.as_mut().unwrap().format = Format::Tar;
                for entry in &mut app.model.entries {
                    entry.encrypted = false;
                }
                app.destination = present::default_destination(&archive);
                app.automatic_destination = true;
                return app;
            }
            "archive" | "many-items" => return app,
            "folder-empty" => {
                app.browse_folder("빈 폴더");
                return app;
            }
            "dialog-error" => {
                app.ui_error = Some(DIALOG_UNAVAILABLE.into());
                return app;
            }
            "folder" => {
                app.browse_folder("사진");
                return app;
            }
            "search-empty" => {
                app.search = "없는 파일".into();
                return app;
            }
            "selection" | "marquee" => {
                app.model
                    .selected
                    .extend([0, 3, 4, if self.name == "marquee" { 2 } else { 1 }]);
                if self.name == "marquee" {
                    app.scenario_marquee = Some(egui::Rect::from_min_size(
                        egui::pos2(22.0, 8.0),
                        egui::vec2(360.0, 62.0),
                    ));
                }
                return app;
            }
            "create" | "create-encrypted" => {
                app.sources = vec![base.join("읽어주세요.txt"), base.join("사진")];
                app.scenario_sources = Some(std::rc::Rc::new(vec![
                    SourceRow {
                        name: "사진".into(),
                        directory: true,
                        bytes: Some(0),
                    },
                    SourceRow {
                        name: "읽어주세요.txt".into(),
                        directory: false,
                        bytes: Some(4096),
                    },
                ]));
                app.show_create(self.name == "create-encrypted");
                return app;
            }
            _ => {}
        }
        let creating = self.name.starts_with("create-") || self.name == "password-create";
        let listing_error = matches!(self.name, "unsupported" | "damaged");
        if matches!(self.name, "gzip-conflict" | "gzip-conflict-choice") {
            app.model.list_id.as_mut().unwrap().format = Format::Gzip;
            app.model.entries = vec![Entry {
                id: 0,
                name: "여행 자료".into(),
                kind: Kind::File,
                bytes: Some(1_287_952),
                packed: Some(900_000),
                encrypted: false,
            }];
            app.gzip_name = "여행 자료".into();
        }
        let request = if creating {
            Start::CreateZip {
                sources: vec![(base.join("사진"), "사진".into())],
                output_file: base.join("여행 자료.zip"),
                method: Method::Deflate,
                aes256: self.name == "password-create",
            }
        } else if listing_error {
            Start::List { archive }
        } else {
            Start::Extract {
                archive,
                destination: app.destination.clone(),
                selection: Selection::All,
                list_id: app.model.list_id.clone().unwrap(),
                gzip_output_name: (matches!(self.name, "gzip-conflict" | "gzip-conflict-choice"))
                    .then(|| app.gzip_name.clone()),
            }
        };
        app.targets = Some(if creating {
            BTreeSet::from(["여행 자료.zip".into()])
        } else if matches!(self.name, "gzip-conflict" | "gzip-conflict-choice") {
            BTreeSet::from(["여행 자료".into()])
        } else {
            BTreeSet::from([
                "읽어주세요.txt".into(),
                "여행 일정.pdf".into(),
                "사진/바다.jpg".into(),
                "사진/풍경.jpg".into(),
            ])
        });
        if matches!(
            self.name,
            "extract-skipped" | "extract-renamed" | "extract-rename-failed"
        ) {
            app.targets = Some(["읽어주세요.txt".into()].into_iter().collect());
            app.directories.clear();
        }
        assert!(app.start(request));
        {
            let mut state = state.borrow_mut();
            state.frame(Event::Accepted { encryption: true });
            match self.name {
                "conflict-choice" | "conflict-apply" | "gzip-conflict-choice" => {
                    state.frame(Event::ConflictRequired(Conflict {
                        challenge_id: 29,
                        entry_id: if self.name == "gzip-conflict-choice" {
                            0
                        } else {
                            1
                        },
                        relative_name: if self.name == "gzip-conflict-choice" {
                            "여행 자료"
                        } else {
                            "읽어주세요.txt"
                        }
                        .into(),
                        incoming_size: 1_287_952,
                        existing_size: 2048,
                    }));
                }
                "extract-skipped" | "extract-renamed" | "extract-rename-failed" => {
                    let failed = self.name == "extract-rename-failed";
                    let skip = self.name == "extract-skipped";
                    let name = if skip {
                        "읽어주세요.txt"
                    } else {
                        "읽어주세요 (1).txt"
                    };
                    state.frame(Event::ConflictResolved(Resolution {
                        entry_id: 1,
                        relative_name: name.into(),
                        choice: if skip {
                            ConflictChoice::Skip
                        } else {
                            ConflictChoice::Rename
                        },
                    }));
                    if !skip && !failed {
                        state.frame(Event::Output {
                            entry_id: Some(1),
                            relative_name: name.into(),
                            disposition: Disposition::Created,
                        });
                    }
                    state.frame(Event::Terminal(Terminal {
                        outcome: if failed {
                            Outcome::Failed
                        } else {
                            Outcome::Completed
                        },
                        error: failed.then_some(ErrorCode::Io),
                        publication: if skip || failed {
                            Publication::None
                        } else {
                            Publication::Complete
                        },
                        created_count: u64::from(!skip && !failed),
                        retained_partial_count: 0,
                        durability: if skip || failed {
                            Durability::NotApplicable
                        } else {
                            Durability::Synced
                        },
                        cleanup: Cleanup::Complete,
                    }));
                    state.queue.extend([
                        HostEvent::StdoutEof,
                        HostEvent::StderrEof,
                        HostEvent::Exited { success: true },
                    ]);
                }
                "password" | "password-retry" | "password-create" => {
                    state.frame(Event::PasswordRequired {
                        challenge_id: 17,
                        attempt: if self.name == "password-retry" { 2 } else { 1 },
                        encrypt: self.name == "password-create",
                    })
                }
                "extract-complete" | "create-complete" | "extract-unsynced" => {
                    let names = app.targets.as_ref().unwrap();
                    for name in names {
                        state.frame(Event::Output {
                            entry_id: None,
                            relative_name: name.clone(),
                            disposition: Disposition::Created,
                        });
                    }
                    state.frame(Event::Terminal(Terminal {
                        outcome: Outcome::Completed,
                        error: None,
                        publication: Publication::Complete,
                        created_count: names.len() as u64,
                        retained_partial_count: 0,
                        durability: if self.name == "extract-unsynced" {
                            Durability::Unconfirmed
                        } else {
                            Durability::Synced
                        },
                        cleanup: Cleanup::Complete,
                    }));
                    state.queue.extend([
                        HostEvent::StdoutEof,
                        HostEvent::StderrEof,
                        HostEvent::Exited { success: true },
                    ]);
                }
                "extract-cancelled" => state.finish(Outcome::Cancelled, Publication::None),
                "extract-partial" | "extract-uncertain" => {
                    let partial = self.name == "extract-partial";
                    if partial {
                        state.frame(Event::Output {
                            entry_id: Some(1),
                            relative_name: "읽어주세요.txt".into(),
                            disposition: Disposition::Created,
                        });
                    }
                    state.frame(Event::Terminal(Terminal {
                        outcome: Outcome::Failed,
                        error: Some(ErrorCode::Io),
                        publication: if partial {
                            Publication::Partial
                        } else {
                            Publication::Uncertain
                        },
                        created_count: u64::from(partial),
                        retained_partial_count: 0,
                        durability: Durability::Synced,
                        cleanup: Cleanup::Complete,
                    }));
                    state.queue.extend([
                        HostEvent::StdoutEof,
                        HostEvent::StderrEof,
                        HostEvent::Exited { success: true },
                    ]);
                }
                "conflict" | "gzip-conflict" | "unsupported" | "damaged" | "source-changed"
                | "create-error" => {
                    state.frame(Event::Terminal(Terminal {
                        outcome: Outcome::Failed,
                        error: Some(match self.name {
                            "conflict" | "gzip-conflict" => ErrorCode::Conflict,
                            "source-changed" => ErrorCode::SourceChanged,
                            "create-error" => ErrorCode::DiskFull,
                            "unsupported" => ErrorCode::UnsupportedFormat,
                            _ => ErrorCode::Integrity,
                        }),
                        publication: Publication::None,
                        created_count: 0,
                        retained_partial_count: 0,
                        durability: Durability::NotApplicable,
                        cleanup: Cleanup::Complete,
                    }));
                    state.queue.extend([
                        HostEvent::StdoutEof,
                        HostEvent::StderrEof,
                        HostEvent::Exited { success: true },
                    ]);
                }
                _ => state.frame(Event::Progress {
                    stage: if creating {
                        Stage::Write
                    } else {
                        Stage::Decode
                    },
                    processed: 38 << 20,
                    total: (self.name != "extract-unknown").then_some(61 << 20),
                    completed_entries: 12,
                }),
            }
        }
        for _ in 0..64 {
            app.model.poll();
        }
        if self.name == "conflict-apply" {
            app.conflict_challenge = Some((app.model.job_id().unwrap(), 29));
            app.conflict_apply = true;
        }
        if self.name == "extract-unsynced" {
            app.details = true;
        }
        if self.name == "cancelling" {
            app.model.cancel();
        }
        if self.name == "close-confirm" {
            app.model.request_close();
        }
        app
    }
}

/// 화면을 고정하고 실제 파일 조작을 막는 미리보기임.
pub struct Preview(pub ArchiveApp, Option<Scenario>);
impl Preview {
    pub fn with_scenario(mut self, scenario: Scenario) -> Self {
        self.1 = Some(scenario);
        self
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        if let Some(scenario) = self.1 {
            scenario.show(&mut self.0, ui);
        } else {
            self.0.show(ui);
        }
    }

    pub fn new(app: ArchiveApp, ctx: &egui::Context) -> Self {
        ctx.on_begin_pass(
            "ui-scenario-readonly",
            std::sync::Arc::new(|ctx| {
                ctx.input_mut(|i| {
                    i.events.clear();
                    i.pointer = Default::default();
                    i.keys_down.clear();
                    i.modifiers = Default::default();
                    i.raw.dropped_files.clear();
                    i.raw.hovered_files.clear();
                });
            }),
        );
        Self(app, None)
    }
}
impl eframe::App for Preview {
    fn ui(&mut self, root: &mut egui::Ui, _: &mut eframe::Frame) {
        if root.ctx().input(|i| i.viewport().close_requested()) {
            root.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.show(root);
    }
}
