use crate::archive_ui::*;
use eframe::egui;
use std::path::PathBuf;

pub struct ArchiveApp {
    pub model: Model,
    factory: Box<dyn Factory>,
    open_text: String,
    destination: String,
    gzip_name: String,
    sources: String,
    zip_output: String,
    password: String,
    password_challenge: Option<u64>,
    aes256: bool,
    create: bool,
    open: bool,
    selected_only: bool,
    method: Method,
    fixture: bool,
}
impl ArchiveApp {
    pub fn new(factory: Box<dyn Factory>, startup: Option<PathBuf>, fixture: bool) -> Self {
        let mut app = Self {
            model: Model::default(),
            factory,
            open_text: String::new(),
            destination: String::new(),
            gzip_name: String::new(),
            sources: String::new(),
            zip_output: String::new(),
            password: String::new(),
            password_challenge: None,
            aes256: false,
            create: false,
            open: false,
            selected_only: false,
            method: Method::Deflate,
            fixture,
        };
        if let Some(path) = startup {
            app.open_path(path);
        }
        app
    }
    fn clear_password(&mut self) {
        let mut bytes = std::mem::take(&mut self.password).into_bytes();
        bytes.fill(0);
    }
    pub fn open_path(&mut self, archive: PathBuf) {
        self.clear_password();
        self.model
            .begin(Start::List { archive }, self.factory.as_mut());
    }
    fn on_drop(&mut self, paths: Vec<PathBuf>) {
        if self.model.busy() || paths.is_empty() {
            return;
        }
        if paths.len() == 1 && !self.create {
            self.open_path(paths[0].clone());
        } else if paths.iter().all(|p| p.to_str().is_some()) {
            self.sources = paths
                .iter()
                .map(|p| p.to_str().unwrap())
                .collect::<Vec<_>>()
                .join("\n");
            self.create = true;
        } else {
            self.model.error = Some("ZIP 생성 경로는 현재 UTF-8 입력만 지원합니다".into());
        }
    }
}
impl ArchiveApp {
    pub fn show(&mut self, root: &mut egui::Ui) {
        self.model.poll();
        let challenge = self.model.challenge.map(|(id, _, _)| id);
        let new_challenge = challenge != self.password_challenge;
        if new_challenge || challenge.is_none() {
            self.clear_password();
        }
        self.password_challenge = challenge;
        let ctx = root.ctx().clone();
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect::<Vec<_>>()
        });
        self.on_drop(dropped);
        if ctx.input(|i| i.viewport().close_requested()) {
            self.model.request_close();
            if self.model.close != Close::Allowed {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if self.model.close == Close::Allowed {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        egui::Panel::top("archive-actions").show(root, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Zipelo");
                ui.add_enabled_ui(
                    !self.model.busy() && self.model.close == Close::Open,
                    |ui| {
                        if ui.button("파일 열기").clicked() {
                            self.open = true;
                        }
                        if ui.button("ZIP 만들기").clicked() {
                            self.create = true;
                        }
                    },
                );
                if ui
                    .add_enabled(self.model.busy(), egui::Button::new("취소 요청"))
                    .clicked()
                {
                    self.clear_password();
                    self.model.cancel();
                }
            });
            if self.fixture {
                ui.colored_label(
                    egui::Color32::from_rgb(143, 69, 0),
                    "화면 시험 모드 / 파일을 읽거나 저장하지 않습니다",
                );
            }
        });
        egui::Panel::bottom("archive-status").show(root, |ui| {
            ui.label(self.model.status());
            if let Some((stage, done, total, entries)) =
                self.model.progress.filter(|_| self.model.busy())
            {
                ui.label(format!(
                    "{} / {done}바이트 / {entries}개 처리",
                    stage_label(stage)
                ));
                if let Some(total) = total.filter(|n| *n > 0) {
                    ui.add(
                        egui::ProgressBar::new((done as f64 / total as f64).clamp(0.0, 1.0) as f32)
                            .show_percentage(),
                    );
                } else if self.model.busy() {
                    ui.spinner();
                }
            }
            if !self.model.busy() {
                if let Some(t) = &self.model.terminal {
                    for line in terminal_lines(t) {
                        ui.label(line);
                    }
                }
            }
        });
        egui::CentralPanel::default_margins().show(root, |ui| {
            if let Some(path) = &self.model.archive {
                ui.heading(
                    path.file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy(),
                );
                ui.label(path.display().to_string());
            } else {
                ui.heading("압축파일을 여기로 끌어 놓으세요");
            }
            if self.model.list_id.is_some() {
                ui.add_enabled_ui(self.model.can_extract(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("출력 폴더");
                        ui.text_edit_singleline(&mut self.destination);
                    });
                    if self
                        .model
                        .list_id
                        .as_ref()
                        .is_some_and(|id| id.format == Format::Gzip)
                    {
                        ui.horizontal(|ui| {
                            ui.label("출력 파일명");
                            ui.text_edit_singleline(&mut self.gzip_name);
                        });
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut self.selected_only, "선택한 항목만 풀기");
                        if ui.button("풀기").clicked() {
                            self.model.extract(
                                PathBuf::from(&self.destination),
                                self.selected_only,
                                self.model
                                    .list_id
                                    .as_ref()
                                    .filter(|id| id.format == Format::Gzip)
                                    .map(|_| self.gzip_name.clone()),
                                self.factory.as_mut(),
                            );
                        }
                    });
                });
            }
            ui.separator();
            egui::ScrollArea::both().show(ui, |ui| {
                egui::Grid::new("archive-list")
                    .striped(true)
                    .min_col_width(90.0)
                    .show(ui, |ui| {
                        ui.label("선택");
                        ui.label("이름");
                        ui.label("크기");
                        ui.label("형식");
                        ui.label("암호");
                        ui.end_row();
                        for entry in &self.model.entries {
                            let mut selected = self.model.selected.contains(&entry.id);
                            if ui
                                .add_enabled(
                                    !self.model.busy()
                                        && matches!(entry.kind, Kind::File | Kind::Directory),
                                    egui::Checkbox::without_text(&mut selected),
                                )
                                .on_hover_text(format!("{} 선택", entry.name))
                                .changed()
                            {
                                if selected {
                                    self.model.selected.insert(entry.id);
                                } else {
                                    self.model.selected.remove(&entry.id);
                                }
                            }
                            ui.label(&entry.name);
                            ui.label(
                                entry
                                    .bytes
                                    .map_or("알 수 없음".into(), |n| format!("{n} B")),
                            );
                            ui.label(match entry.kind {
                                Kind::File => "파일",
                                Kind::Directory => "폴더",
                                Kind::Link => "링크",
                                Kind::Other => "기타",
                            });
                            ui.label(if entry.encrypted { "있음" } else { "없음" });
                            ui.end_row();
                        }
                    });
                if self.model.entries.is_empty() && self.model.list_id.is_some() {
                    ui.label("압축파일에 항목이 없습니다");
                }
                for (name, state) in &self.model.outputs {
                    ui.label(format!("{name}: {}", disposition_label(*state)));
                }
            });
        });
        if self.open {
            egui::Window::new("파일 열기")
                .collapsible(false)
                .show(&ctx, |ui| {
                    ui.label("압축파일 절대 경로");
                    ui.text_edit_singleline(&mut self.open_text);
                    if ui.button("열기").clicked() {
                        self.open_path(self.open_text.clone().into());
                        self.open = false;
                    }
                    if ui.button("닫기").clicked() {
                        self.open = false;
                    }
                });
        }
        if self.create {
            egui::Window::new("ZIP 만들기")
                .collapsible(false)
                .show(&ctx, |ui| {
                    ui.add_enabled_ui(!self.model.busy(), |ui| {
                        ui.label("원본 파일 절대 경로 / 한 줄에 하나");
                        ui.add(egui::TextEdit::multiline(&mut self.sources).desired_rows(4));
                        ui.label("저장할 ZIP 절대 경로");
                        ui.text_edit_singleline(&mut self.zip_output);
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.method, Method::Deflate, "압축");
                            ui.selectable_value(&mut self.method, Method::Store, "묶기");
                        });
                        ui.checkbox(&mut self.aes256, "암호로 보호");
                        if ui.button("ZIP 만들기").clicked() {
                            let sources = self
                                .sources
                                .lines()
                                .map(|s| {
                                    let path = PathBuf::from(s);
                                    let name = path
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("")
                                        .to_owned();
                                    (path, name)
                                })
                                .collect();
                            if self.model.begin(
                                Start::CreateZip {
                                    sources,
                                    output_file: self.zip_output.clone().into(),
                                    method: self.method,
                                    aes256: self.aes256,
                                },
                                self.factory.as_mut(),
                            ) {
                                self.create = false;
                            }
                        }
                    });
                    if ui.button("닫기").clicked() {
                        self.create = false;
                    }
                });
        }
        if let Some((_, attempt, encrypt)) = self.model.challenge {
            egui::Window::new(if encrypt {
                "ZIP 암호 설정"
            } else {
                "압축파일 암호"
            })
            .collapsible(false)
            .show(&ctx, |ui| {
                ui.label(if encrypt {
                    "사용할 암호를 입력하세요"
                } else if attempt > 1 {
                    "암호가 맞지 않습니다. 다시 입력하세요"
                } else {
                    "암호를 입력하세요"
                });
                ui.label("암호");
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.password)
                        .password(true)
                        .id(egui::Id::new("archive-password-input"))
                        .char_limit(1024),
                );
                if new_challenge {
                    edit.request_focus();
                }
                if ui.button("암호 전달").clicked()
                    || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    let text = std::mem::take(&mut self.password);
                    self.model.password(text);
                }
                if ui.button("작업 취소").clicked() {
                    self.clear_password();
                    self.model.cancel();
                }
            });
        }
        match self.model.close {
            Close::Confirm => {
                egui::Window::new("작업 중 닫기")
                    .collapsible(false)
                    .show(&ctx, |ui| {
                        ui.label("취소를 요청하고 작업자 종료를 기다릴까요?");
                        if ui.button("계속 열기").clicked() {
                            self.model.keep_open();
                        }
                        if ui.button("취소 후 기다리기").clicked() {
                            self.model.confirm_close();
                        }
                    });
            }
            Close::Waiting => {
                egui::Window::new("종료 확인 중")
                    .collapsible(false)
                    .show(&ctx, |ui| {
                        ui.label("취소 결과와 프로세스 종료를 기다립니다");
                        if ui.button("닫기 철회").clicked() {
                            self.model.keep_open();
                        }
                    });
            }
            Close::Review => {
                egui::Window::new("결과 확인 후 닫기")
                    .collapsible(false)
                    .show(&ctx, |ui| {
                        ui.label(self.model.status());
                        if let Some(t) = &self.model.terminal {
                            for line in terminal_lines(t) {
                                ui.label(line);
                            }
                        }
                        if ui.button("계속 열기").clicked() {
                            self.model.keep_open();
                        }
                        if ui.button("닫기").clicked() {
                            self.model.acknowledge_close();
                        }
                    });
            }
            _ => {}
        }
        if self.model.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
}

impl eframe::App for ArchiveApp {
    fn ui(&mut self, root: &mut egui::Ui, _: &mut eframe::Frame) {
        self.show(root);
    }
}

impl Drop for ArchiveApp {
    fn drop(&mut self) {
        self.clear_password();
    }
}
