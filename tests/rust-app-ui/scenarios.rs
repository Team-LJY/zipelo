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
    "conflict-choice" => "같은 이름 파일의 세 가지 처리 선택",
    "conflict-apply" => "남은 충돌에 같은 선택 적용",
    "extract-skipped" => "모든 파일을 건너뛴 정상 완료",
    "extract-renamed" => "이름을 바꿔 저장한 완료 결과",
    "empty" => "빈 첫 화면",
    "archive" => "폴더와 파일이 있는 압축파일",
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
        if self.name == "empty" {
            return app;
        }
        let archive = base.join(if self.name == "gzip-conflict" {
            "여행 자료.gz"
        } else {
            "여행 자료.zip"
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
        if self.name == "gzip-conflict" {
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
                gzip_output_name: (self.name == "gzip-conflict").then(|| app.gzip_name.clone()),
            }
        };
        app.targets = Some(if creating {
            BTreeSet::from(["여행 자료.zip".into()])
        } else if self.name == "gzip-conflict" {
            BTreeSet::from(["여행 자료".into()])
        } else {
            BTreeSet::from([
                "읽어주세요.txt".into(),
                "여행 일정.pdf".into(),
                "사진/바다.jpg".into(),
                "사진/풍경.jpg".into(),
            ])
        });
        if matches!(self.name, "extract-skipped" | "extract-renamed") {
            app.targets = Some(["읽어주세요.txt".into()].into_iter().collect());
            app.directories.clear();
        }
        assert!(app.start(request));
        {
            let mut state = state.borrow_mut();
            state.frame(Event::Accepted { encryption: true });
            match self.name {
                "conflict-choice" | "conflict-apply" => {
                    state.frame(Event::ConflictRequired(Conflict {
                        challenge_id: 29,
                        entry_id: 1,
                        relative_name: "읽어주세요.txt".into(),
                        incoming_size: 1_287_952,
                        existing_size: 2048,
                    }));
                }
                "extract-skipped" | "extract-renamed" => {
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
                    if !skip {
                        state.frame(Event::Output {
                            entry_id: Some(1),
                            relative_name: name.into(),
                            disposition: Disposition::Created,
                        });
                    }
                    state.frame(Event::Terminal(Terminal {
                        outcome: Outcome::Completed,
                        error: None,
                        publication: if skip {
                            Publication::None
                        } else {
                            Publication::Complete
                        },
                        created_count: u64::from(!skip),
                        retained_partial_count: 0,
                        durability: if skip {
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
pub struct Preview(pub ArchiveApp);
impl Preview {
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
        Self(app)
    }
}
impl eframe::App for Preview {
    fn ui(&mut self, root: &mut egui::Ui, _: &mut eframe::Frame) {
        if root.ctx().input(|i| i.viewport().close_requested()) {
            root.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.0.show(root);
    }
}
