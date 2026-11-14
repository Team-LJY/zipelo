use eframe::egui::{self, FontId, TextStyle};
use std::{collections::VecDeque, path::PathBuf};
use zipelo_ui_memory_app::{archive_screen::ArchiveApp, archive_ui::*};
#[path = "../../experiments/rust-ui-memory-app/src/renderer.rs"]
mod renderer;
#[path = "../../experiments/rust-ui-memory-app/src/system_font.rs"]
mod system_font;

#[derive(Clone)]
struct CaptureFactory {
    mode: String,
}
struct CaptureConnection {
    job: u64,
    sequence: u64,
    mode: String,
    queue: VecDeque<HostEvent>,
    create_name: Option<String>,
}
impl CaptureConnection {
    fn event(&mut self, event: Event) {
        self.queue.push_back(HostEvent::Frame {
            job_id: self.job,
            sequence: self.sequence,
            event,
        });
        self.sequence += 1;
    }
    fn terminal(&mut self, terminal: Terminal) {
        self.event(Event::Terminal(terminal));
        self.queue.extend([
            HostEvent::StdoutEof,
            HostEvent::StderrEof,
            HostEvent::Exited { success: true },
        ]);
    }
    fn finish(&mut self, listing: bool) {
        let error = !listing && self.mode == "error";
        let cancelled = !listing && self.mode == "cancelled";
        let mut count = 0;
        if !listing && !error && !cancelled {
            let names = self
                .create_name
                .clone()
                .map(|name| vec![name])
                .unwrap_or_else(|| {
                    vec![
                        "읽어주세요.txt".into(),
                        "여행 일정.pdf".into(),
                        "사진".into(),
                    ]
                });
            for name in names {
                self.event(Event::Output {
                    entry_id: None,
                    relative_name: name,
                    disposition: Disposition::Created,
                });
                count += 1;
            }
        }
        self.terminal(Terminal {
            outcome: if error {
                Outcome::Failed
            } else if cancelled {
                Outcome::Cancelled
            } else {
                Outcome::Completed
            },
            error: error.then_some(ErrorCode::DiskFull),
            publication: if listing || error || cancelled {
                Publication::None
            } else {
                Publication::Complete
            },
            created_count: count,
            retained_partial_count: 0,
            durability: if listing {
                Durability::NotApplicable
            } else {
                Durability::Synced
            },
            cleanup: Cleanup::Complete,
        });
    }
}
fn entries() -> Vec<Entry> {
    vec![
        Entry {
            id: 0,
            name: "읽어주세요.txt".into(),
            kind: Kind::File,
            bytes: Some(4096),
            packed: Some(1800),
            encrypted: false,
        },
        Entry {
            id: 1,
            name: "여행 일정.pdf".into(),
            kind: Kind::File,
            bytes: Some(1_287_952),
            packed: Some(900_000),
            encrypted: true,
        },
        Entry {
            id: 2,
            name: "사진".into(),
            kind: Kind::Directory,
            bytes: Some(0),
            packed: None,
            encrypted: false,
        },
    ]
}
fn toolbar_entries() -> Vec<Entry> {
    let mut entries = vec![
        Entry {
            id: 0,
            name: "Install".into(),
            kind: Kind::Directory,
            bytes: Some(0),
            packed: None,
            encrypted: false,
        },
        Entry {
            id: 1,
            name: "Install.exe".into(),
            kind: Kind::File,
            bytes: Some(234_906),
            packed: None,
            encrypted: false,
        },
    ];
    for (index, name) in [
        "cab1.cab",
        "cab10.cab",
        "cab11.cab",
        "cab12.cab",
        "cab2.cab",
        "cab3.cab",
        "cab4.cab",
        "cab5.cab",
    ]
    .iter()
    .enumerate()
    {
        entries.push(Entry {
            id: index as u32 + 2,
            name: format!("Install/{name}"),
            kind: Kind::File,
            bytes: Some(
                [
                    99_510_272,
                    77_909_197,
                    102_236_160,
                    67_842_867,
                    103_389_594,
                    59_454_669,
                    78_014_054,
                    81_159_782,
                ][index],
            ),
            packed: None,
            encrypted: false,
        });
    }
    entries
}

impl Factory for CaptureFactory {
    fn connect(&mut self, job: u64) -> Result<Box<dyn Connection>, String> {
        Ok(Box::new(CaptureConnection {
            job,
            sequence: 0,
            mode: self.mode.clone(),
            queue: VecDeque::new(),
            create_name: None,
        }))
    }
}
impl Connection for CaptureConnection {
    fn send(&mut self, request: Request) -> Result<(), String> {
        match request.command {
            Command::Start(start) | Command::StartExpanded(start) => {
                self.event(Event::Accepted { encryption: true });
                match start {
                    Start::List { .. } => {
                        let entries = if self.mode.starts_with("toolbar-") {
                            toolbar_entries()
                        } else {
                            entries()
                        };
                        let count = entries.len() as u32;
                        for entry in entries {
                            self.event(Event::Entry(entry));
                        }
                        self.event(Event::ListEnd {
                            entry_count: count,
                            list_id: ListId {
                                catalog_version: 1,
                                format: Format::Zip,
                                source_object: SourceObject::Unix {
                                    device: 1,
                                    inode: 1,
                                },
                                archive_bytes: 902000,
                                archive_sha256: [7; 32],
                            },
                        });
                        self.finish(true);
                    }
                    Start::Extract { .. } => {
                        if self.mode.starts_with("password") {
                            self.event(Event::PasswordRequired {
                                challenge_id: 17,
                                attempt: if self.mode == "password-retry" { 2 } else { 1 },
                                encrypt: false,
                            });
                        } else if self.mode.starts_with("progress") || self.mode == "cancelling" {
                            self.event(Event::Progress {
                                stage: Stage::Decode,
                                processed: 38 << 20,
                                total: (self.mode != "progress-unknown").then_some(61 << 20),
                                completed_entries: 12,
                            });
                        } else {
                            self.finish(false);
                        }
                    }
                    Start::Preview { .. } => self.terminal(Terminal {
                        outcome: Outcome::Failed,
                        error: Some(ErrorCode::UnsupportedFeature),
                        publication: Publication::None,
                        created_count: 0,
                        retained_partial_count: 0,
                        durability: Durability::NotApplicable,
                        cleanup: Cleanup::Complete,
                    }),
                    Start::CreateZip {
                        output_file,
                        aes256,
                        ..
                    } => {
                        self.create_name =
                            Some(output_file.file_name().unwrap().to_string_lossy().into());
                        if aes256 {
                            self.event(Event::PasswordRequired {
                                challenge_id: 17,
                                attempt: 1,
                                encrypt: true,
                            });
                        } else {
                            self.finish(false);
                        }
                    }
                }
            }
            Command::ConflictReply { .. } => return Err("충돌 선택은 상황 명령으로 확인함".into()),
            Command::Password { .. } => self.finish(false),
            Command::ResourceReply { .. } => {}
            Command::Cancel => {
                self.event(Event::CancelAck {
                    request_sequence: request.sequence,
                    status: CancelStatus::Requested,
                });
                if self.mode != "cancelling" {
                    self.terminal(Terminal {
                        outcome: Outcome::Cancelled,
                        error: None,
                        publication: Publication::None,
                        created_count: 0,
                        retained_partial_count: 0,
                        durability: Durability::NotApplicable,
                        cleanup: Cleanup::Complete,
                    });
                }
            }
        }
        Ok(())
    }
    fn poll(&mut self) -> Option<HostEvent> {
        self.queue.pop_front()
    }
}
struct Capture {
    app: ArchiveApp,
    mode: String,
    factory: CaptureFactory,
    triggered: bool,
}
impl eframe::App for Capture {
    fn ui(&mut self, root: &mut egui::Ui, _: &mut eframe::Frame) {
        if self.mode == "hover" {
            root.ctx().input_mut(|i| {
                i.raw.hovered_files = vec![egui::HoveredFile {
                    path: Some(PathBuf::from("여행.zip")),
                    ..Default::default()
                }]
            });
        }
        if !self.triggered && self.app.model.list_id.is_some() {
            self.triggered = true;
            if self.mode.starts_with("long-") {
                let name = format!("{}.txt", "W".repeat(220));
                let folder = "deep-folder/".repeat(30);
                let base = std::env::current_dir()
                    .unwrap()
                    .join("tests/ui-redesign/runs/fixtures");
                self.app.model.archive = Some(base.join(&folder).join(format!("{name}.zip")));
                if self.mode == "long-list" || self.mode == "long-folder" {
                    self.app.model.entries = (0..5000)
                        .map(|id| Entry {
                            id,
                            name: format!(
                                "{}{id:04}-{name}",
                                if self.mode == "long-folder" {
                                    folder.as_str()
                                } else {
                                    ""
                                }
                            ),
                            kind: Kind::File,
                            bytes: Some(31),
                            packed: None,
                            encrypted: true,
                        })
                        .collect();
                    if self.mode == "long-folder" {
                        self.app.browse_folder(folder.trim_end_matches('/'));
                    }
                }
                if self.mode == "long-create" {
                    let source = base.join(&name);
                    std::fs::write(&source, b"capture source").unwrap();
                    self.app.add_sources(vec![source]);
                    self.app.show_create(false);
                }
                if self.mode == "long-result" {
                    let destination = base.join(&folder);
                    std::fs::create_dir_all(&destination).unwrap();
                    self.app.choose_destination(destination);
                    self.app.begin_extract();
                }
            }
            if self.mode == "toolbar-folder" {
                self.app.browse_folder("Install");
            }
            if self.mode == "toolbar-create" {
                self.app.show_create(false);
            }
            if [
                "password",
                "password-retry",
                "password-create",
                "progress",
                "progress-unknown",
                "cancelling",
                "success",
                "error",
                "cancelled",
            ]
            .contains(&self.mode.as_str())
            {
                let archive = self.app.model.archive.clone().unwrap();
                let start = if self.mode == "password-create" {
                    Start::CreateZip {
                        sources: vec![(archive.clone(), "자료".into())],
                        output_file: archive.with_file_name("보호한 자료.zip"),
                        method: Method::Deflate,
                        aes256: true,
                    }
                } else {
                    Start::Extract {
                        archive,
                        destination: std::env::current_dir().unwrap(),
                        selection: Selection::All,
                        list_id: self.app.model.list_id.clone().unwrap(),
                        gzip_output_name: None,
                    }
                };
                // 이전 화면과 같은 공개 상태 API로 암호 및 진행 상태를 재현함.
                if self.mode == "password-create" {
                    self.app.model.begin(start, &mut self.factory);
                } else {
                    self.app.begin_extract();
                }
            }
        }
        self.app.show(root);
        root.ctx()
            .request_repaint_after(std::time::Duration::from_millis(30));
    }
}
fn main() -> eframe::Result {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "empty".into());
    let base = std::env::current_dir()
        .unwrap()
        .join("tests/ui-redesign/runs/fixtures");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("읽어주세요.txt"), "화면 검증 입력").unwrap();
    std::fs::create_dir_all(base.join("사진")).unwrap();
    let fallback = system_font::load()
        .ok()
        .map(|font| (font.bytes, font.face_index));
    let title = format!("Zipelo UI capture {mode}");
    renderer::run_native(
        &title,
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1060.0, 720.0])
                .with_min_inner_size([760.0, 540.0]),
            ..Default::default()
        },
        || {
            let mode = mode.clone();
            let base = base.clone();
            let fallback = fallback.clone();
            Box::new(move |cc| {
                cc.egui_ctx
                    .set_fonts(zipelo_ui_memory_app::archive_theme::fonts(fallback.clone()));
                cc.egui_ctx.set_visuals(egui::Visuals::light());
                cc.egui_ctx.style_mut_of(egui::Theme::Light, |style| {
                    style
                        .text_styles
                        .insert(TextStyle::Body, FontId::proportional(18.0));
                    style
                        .text_styles
                        .insert(TextStyle::Button, FontId::proportional(18.0));
                    style
                        .text_styles
                        .insert(TextStyle::Heading, FontId::proportional(26.0));
                });
                let factory = CaptureFactory { mode: mode.clone() };
                let startup = (!["empty", "hover", "create", "create-filled"]
                    .contains(&mode.as_str()))
                .then(|| base.join("여행.zip"));
                let mut app = ArchiveApp::new(Box::new(factory.clone()), startup, true);
                if mode == "create-filled" {
                    app.add_sources(vec![base.join("읽어주세요.txt"), base.join("사진")]);
                }
                Ok(Box::new(Capture {
                    app,
                    mode,
                    factory,
                    triggered: false,
                }))
            })
        },
    )
}
