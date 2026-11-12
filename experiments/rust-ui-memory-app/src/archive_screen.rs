use crate::{
    archive_picker::{Choice, Picker, Purpose},
    archive_present as present,
    archive_theme::{self as theme, Icon},
    archive_ui::*,
};
use eframe::egui::{self, Align, Layout, RichText};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub struct ArchiveApp {
    pub model: Model,
    factory: Box<dyn Factory>,
    destination: PathBuf,
    automatic_destination: bool,
    owned_destination: Option<PathBuf>,
    gzip_name: String,
    sources: Vec<PathBuf>,
    zip_folder: PathBuf,
    zip_name: String,
    password: String,
    password_confirm: String,
    password_visible: bool,
    password_challenge: Option<u64>,
    aes256: bool,
    create: bool,
    selected_only: bool,
    method: Method,
    fixture: bool,
    picker: Option<Picker>,
    last_request: Option<Start>,
    targets: Option<BTreeSet<String>>,
    directories: BTreeSet<String>,
    dismissed: bool,
    details: bool,
    checked_destination: bool,
    ui_error: Option<String>,
    initialized: bool,
}

impl ArchiveApp {
    pub fn new(factory: Box<dyn Factory>, startup: Option<PathBuf>, fixture: bool) -> Self {
        let mut app = Self {
            model: Model::default(),
            factory,
            destination: PathBuf::new(),
            automatic_destination: true,
            owned_destination: None,
            gzip_name: String::new(),
            sources: Vec::new(),
            zip_folder: present::home_directory(),
            zip_name: String::new(),
            password: String::new(),
            password_confirm: String::new(),
            password_visible: false,
            password_challenge: None,
            aes256: false,
            create: false,
            selected_only: false,
            method: Method::Deflate,
            fixture,
            picker: None,
            last_request: None,
            targets: None,
            directories: BTreeSet::new(),
            dismissed: false,
            details: false,
            checked_destination: false,
            ui_error: None,
            initialized: false,
        };
        if let Some(path) = startup {
            app.open_path(path);
        }
        app
    }

    fn clear_password(&mut self) {
        for value in [&mut self.password, &mut self.password_confirm] {
            let mut bytes = std::mem::take(value).into_bytes();
            bytes.fill(0);
        }
        self.password_visible = false;
    }

    pub fn open_path(&mut self, archive: PathBuf) {
        if self.model.busy() {
            return;
        }
        self.clear_password();
        self.destination = present::default_destination(&archive);
        self.automatic_destination = true;
        self.gzip_name = archive
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .strip_suffix(".gz")
            .unwrap_or("압축 해제 파일")
            .into();
        self.selected_only = false;
        self.targets = None;
        self.directories.clear();
        self.start(Start::List { archive });
    }

    fn start(&mut self, request: Start) -> bool {
        self.last_request = Some(request.clone());
        self.dismissed = false;
        self.details = false;
        self.checked_destination = false;
        self.ui_error = None;
        self.model.begin(request, self.factory.as_mut())
    }

    pub fn choose_destination(&mut self, path: PathBuf) {
        self.destination = path;
        self.automatic_destination = false;
        self.ui_error = None;
    }

    pub fn add_sources(&mut self, paths: Vec<PathBuf>) {
        if self.model.busy() {
            return;
        }
        if self.sources.is_empty() {
            if let Some(first) = paths.first() {
                self.zip_folder = first.parent().unwrap_or(Path::new("/")).to_path_buf();
                let name = if first.is_dir() {
                    first.file_name()
                } else {
                    first.file_stem()
                };
                self.zip_name = format!("{}.zip", name.unwrap_or_default().to_string_lossy());
            }
        }
        for path in paths {
            if !self.sources.contains(&path) {
                self.sources.push(path);
            }
        }
        self.create = true;
    }

    pub fn begin_extract(&mut self) -> bool {
        if !self.model.can_extract() || (self.selected_only && self.model.selected.is_empty()) {
            return false;
        }
        match present::prepare_destination(&self.destination, self.automatic_destination) {
            Ok(true) => self.owned_destination = Some(self.destination.clone()),
            Ok(false) => self.owned_destination = None,
            Err(_) => {
                self.ui_error = Some(present::FOLDER_ERROR.into());
                return false;
            }
        }
        let gzip = self
            .model
            .list_id
            .as_ref()
            .is_some_and(|id| id.format == Format::Gzip);
        let selection = if self.selected_only {
            Selection::EntryIds(self.model.selected.iter().copied().collect())
        } else {
            Selection::All
        };
        self.targets = present::target_files(
            &self.model.entries,
            &selection,
            gzip.then_some(self.gzip_name.as_str()),
        );
        self.directories = if gzip {
            BTreeSet::new()
        } else {
            present::target_directories(&self.model.entries, &selection)
        };
        let request = Start::Extract {
            archive: self.model.archive.clone().unwrap(),
            destination: self.destination.clone(),
            selection,
            list_id: self.model.list_id.clone().unwrap(),
            gzip_output_name: gzip.then(|| self.gzip_name.clone()),
        };
        if self.start(request) {
            true
        } else {
            if let Some(path) = self.owned_destination.take() {
                present::remove_owned_empty(&path);
            }
            false
        }
    }

    fn begin_zip(&mut self) {
        if self.sources.is_empty() {
            return;
        }
        if !self.zip_name.to_ascii_lowercase().ends_with(".zip") {
            self.zip_name.push_str(".zip");
        }
        if !valid_leaf(&self.zip_name) {
            self.ui_error = Some("ZIP 파일 이름을 확인하세요".into());
            return;
        }
        let output_file = self.zip_folder.join(&self.zip_name);
        if std::fs::symlink_metadata(&output_file).is_ok() {
            self.ui_error = Some("같은 이름의 파일이 이미 있습니다. 다른 이름을 입력하세요".into());
            return;
        }
        let mut sources = Vec::new();
        for path in &self.sources {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                self.ui_error = Some(
                    "이름을 읽을 수 없는 파일이 있습니다. 파일 이름을 바꾼 뒤 다시 추가하세요"
                        .into(),
                );
                return;
            };
            sources.push((path.clone(), name.to_owned()));
        }
        self.targets = Some([self.zip_name.clone()].into_iter().collect());
        self.directories.clear();
        if self.start(Start::CreateZip {
            sources,
            output_file,
            method: self.method,
            aes256: self.aes256,
        }) {
            self.create = false;
        }
    }

    fn open_picker(&mut self, purpose: Purpose) {
        let start = match purpose {
            Purpose::Destination | Purpose::Retry => self.destination.clone(),
            Purpose::ZipFolder => self.zip_folder.clone(),
            Purpose::Open => self
                .model
                .archive
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .unwrap_or_else(present::home_directory),
            _ => self
                .sources
                .last()
                .and_then(|p| p.parent())
                .map(Path::to_path_buf)
                .unwrap_or_else(present::home_directory),
        };
        self.picker = Some(Picker::new(purpose, &start));
    }

    fn retry(&mut self) {
        if let Some(request) = self.last_request.clone() {
            match request {
                Start::List { archive } => self.open_path(archive),
                Start::Extract {
                    selection,
                    gzip_output_name,
                    ..
                } => {
                    self.selected_only = matches!(selection, Selection::EntryIds(_));
                    if let Selection::EntryIds(ids) = selection {
                        self.model.selected = ids.into_iter().collect();
                    }
                    if let Some(name) = gzip_output_name {
                        self.gzip_name = name;
                    }
                    self.begin_extract();
                }
                Start::CreateZip { .. } => {
                    self.create = true;
                    self.begin_zip();
                }
            }
        }
    }

    fn show_picker(&mut self, ctx: &egui::Context) {
        let answer = self.picker.as_mut().and_then(|picker| picker.show(ctx));
        if let Some(answer) = answer {
            let purpose = self.picker.take().unwrap().purpose;
            if let Choice::Paths(paths) = answer {
                if let Some(path) = paths.first() {
                    match purpose {
                        Purpose::Open => self.open_path(path.clone()),
                        Purpose::Destination | Purpose::Retry => {
                            self.choose_destination(path.clone());
                            if purpose == Purpose::Retry {
                                self.retry();
                            }
                        }
                        Purpose::ZipFolder => self.zip_folder = path.clone(),
                        Purpose::Files | Purpose::Folder => self.add_sources(paths),
                    }
                }
            }
        }
    }

    fn on_drop(&mut self, paths: Vec<PathBuf>) {
        if self.model.busy() || paths.is_empty() || self.picker.is_some() {
            return;
        }
        if paths.len() == 1 && !self.create && !paths[0].is_dir() {
            self.open_path(paths[0].clone());
        } else {
            self.add_sources(paths);
        }
    }

    fn poll(&mut self) {
        let busy = self.model.busy();
        self.model.poll();
        if busy && !self.model.busy() {
            if let Some(path) = self.owned_destination.take() {
                if !self.model.confirmed
                    || self
                        .model
                        .terminal
                        .as_ref()
                        .is_none_or(|t| t.outcome != Outcome::Completed)
                {
                    present::remove_owned_empty(&path);
                }
            }
        }
    }

    pub fn show(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        if !self.initialized {
            theme::apply(&ctx);
            self.initialized = true;
        }
        self.poll();
        let challenge = self.model.challenge.map(|(id, _, _)| id);
        let new_challenge = challenge != self.password_challenge;
        if new_challenge || challenge.is_none() {
            self.clear_password();
        }
        self.password_challenge = challenge;
        self.on_drop(ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        }));
        if ctx.input(|i| i.viewport().close_requested()) {
            self.model.request_close();
            if self.model.close != Close::Allowed {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if self.model.close == Close::Allowed {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        egui::Panel::top("archive-actions")
            .frame(theme::panel(theme::PANEL).corner_radius(0))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    theme::icon(ui, Icon::Archive, 24.0);
                    ui.heading("Zipelo");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::action(
                            ui,
                            "ZIP 만들기",
                            false,
                            !self.model.busy() && self.model.close == Close::Open,
                        )
                        .on_disabled_hover_text("작업이 끝나면 ZIP을 만들 수 있습니다")
                        .clicked()
                        {
                            self.create = true;
                            self.ui_error = None;
                        }
                    });
                });
                if self.fixture {
                    ui.colored_label(
                        theme::WARNING,
                        "화면 시험 모드 / 파일을 읽거나 저장하지 않습니다",
                    );
                }
            });
        if self.model.archive.is_some()
            || self.model.busy()
            || self.result_visible()
            || self.ui_error.is_some()
        {
            egui::Panel::bottom("archive-work")
                .frame(theme::panel(theme::PANEL).corner_radius(0))
                .show(root, |ui| self.work_area(ui));
        }
        egui::CentralPanel::default()
            .frame(theme::panel(theme::BACKGROUND))
            .show(root, |ui| {
                if self.model.archive.is_some() {
                    self.archive_list(ui);
                } else {
                    self.empty(ui);
                }
            });
        if self.create && !self.model.busy() {
            self.create_dialog(&ctx);
        }
        self.show_picker(&ctx);
        if self.model.challenge.is_some() {
            self.password_dialog(&ctx, new_challenge);
        }
        self.close_dialog(&ctx);
        if self.model.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }

    fn empty(&mut self, ui: &mut egui::Ui) {
        let hovered = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let width = (ui.available_width() * 0.60)
            .max(480.0)
            .min(ui.available_width());
        let y = ((ui.available_height() - theme::DROP_HEIGHT - theme::SPACE * 8.0) / 2.0).max(0.0);
        ui.add_space(y);
        ui.vertical_centered(|ui| {
            let drop_frame = egui::Frame::new()
                .fill(if hovered { theme::DROP } else { theme::PANEL })
                .corner_radius(theme::DROP_RADIUS)
                .inner_margin(egui::Margin::same(theme::DROP_PAD))
                .show(ui, |ui| {
                    ui.set_width(width - 2.0 * theme::DROP_PAD as f32);
                    ui.set_min_height(theme::DROP_HEIGHT - 2.0 * theme::DROP_PAD as f32);
                    ui.vertical_centered(|ui| {
                        ui.add_space(theme::PAD as f32);
                        theme::icon(ui, Icon::Archive, 48.0);
                        ui.add_space(theme::PAD as f32);
                        ui.label(
                            RichText::new(if hovered {
                                "놓으면 엽니다"
                            } else {
                                "압축파일을 여기에 끌어 놓으세요"
                            })
                            .size(20.0)
                            .strong(),
                        );
                        ui.label("또는");
                        if theme::action(ui, "파일 열기", true, !self.model.busy()).clicked() {
                            self.open_picker(Purpose::Open);
                        }
                    });
                });
            let rect = drop_frame.response.rect;
            if hovered {
                ui.painter().rect_stroke(
                    rect,
                    12,
                    egui::Stroke::new(2.0, theme::BLUE),
                    egui::StrokeKind::Inside,
                );
            } else {
                let stroke = egui::Stroke::new(2.0, theme::DASH);
                for (a, b) in [
                    (rect.left_top(), rect.right_top()),
                    (rect.right_top(), rect.right_bottom()),
                    (rect.right_bottom(), rect.left_bottom()),
                    (rect.left_bottom(), rect.left_top()),
                ] {
                    let direction = (b - a).normalized();
                    let mut distance = theme::DROP_RADIUS as f32;
                    while distance < a.distance(b) - theme::DROP_RADIUS as f32 {
                        ui.painter().line_segment(
                            [
                                a + direction * distance,
                                a + direction
                                    * (distance + theme::SPACE)
                                        .min(a.distance(b) - theme::DROP_RADIUS as f32),
                            ],
                            stroke,
                        );
                        distance += theme::PAD as f32;
                    }
                }
            }
            ui.add_space(theme::PAD as f32);
            ui.colored_label(
                theme::MUTED,
                "ZIP, 7z, TAR, GZ, TAR.GZ 파일을 열 수 있습니다",
            );
        });
    }

    fn archive_list(&mut self, ui: &mut egui::Ui) {
        let archive = self.model.archive.as_ref().unwrap().clone();
        ui.horizontal(|ui| {
            theme::icon(ui, Icon::Archive, 32.0);
            let name = archive.file_name().unwrap_or_default().to_string_lossy();
            let limit = ((ui.available_width() - 144.0) / theme::TITLE).max(10.0) as usize;
            ui.heading(present::middle_text(&name, limit))
                .on_hover_text(archive.display().to_string());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::action(ui, "다른 파일", false, !self.model.busy()).clicked() {
                    self.open_picker(Purpose::Open);
                }
            });
        });
        let bytes = self
            .model
            .entries
            .iter()
            .filter_map(|entry| entry.bytes)
            .sum();
        ui.colored_label(
            theme::MUTED,
            format!(
                "항목 {}개, {}",
                self.model.entries.len(),
                present::bytes_text(bytes)
            ),
        );
        ui.add_space(theme::SPACE);
        let selectable = self
            .model
            .entries
            .iter()
            .filter(|e| matches!(e.kind, Kind::File | Kind::Directory))
            .map(|e| e.id)
            .collect::<BTreeSet<_>>();
        let mut all = !selectable.is_empty() && selectable.is_subset(&self.model.selected);
        let widths =
            (ui.available_width() - 32.0 - 96.0 - 64.0 - theme::SPACE * 4.0 - 32.0).max(160.0);
        theme::panel(theme::PANEL)
            .inner_margin(egui::Margin::symmetric(theme::SPACE as i8, 0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let response = ui
                        .add_enabled(
                            !self.model.busy() && !selectable.is_empty(),
                            egui::Checkbox::without_text(&mut all),
                        )
                        .on_hover_text("전체 선택");
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::Role::CheckBox,
                            !self.model.busy(),
                            all,
                            "전체 선택",
                        )
                    });
                    theme::focus(&response);
                    if response.changed() {
                        self.model.selected = if all {
                            selectable.clone()
                        } else {
                            BTreeSet::new()
                        };
                    }
                    theme::column_label(ui, RichText::new("이름").strong(), widths, Align::Min);
                    theme::column_label(ui, "크기", 96.0, Align::Max);
                    theme::column_label(ui, "암호", 64.0, Align::Min);
                });
                ui.separator();
                if self.model.entries.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(theme::SPACE * 8.0);
                        ui.label(if self.model.list_id.is_some() {
                            "이 압축파일은 비어 있습니다"
                        } else {
                            "압축파일을 읽는 중"
                        });
                    });
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("archive-rows")
                        .auto_shrink([false, false])
                        .show_rows(ui, theme::ROW, self.model.entries.len(), |ui, rows| {
                            for index in rows {
                                let entry = &self.model.entries[index];
                                let enabled = !self.model.busy()
                                    && matches!(entry.kind, Kind::File | Kind::Directory);
                                let mut selected = self.model.selected.contains(&entry.id);
                                let width = ui.available_width();
                                let (rect, row) = ui.allocate_exact_size(
                                    egui::vec2(width, theme::ROW),
                                    if enabled {
                                        egui::Sense::click()
                                    } else {
                                        egui::Sense::hover()
                                    },
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    0,
                                    if selected {
                                        theme::SELECTED
                                    } else if index % 2 == 0 {
                                        theme::PANEL
                                    } else {
                                        theme::STRIPE
                                    },
                                );
                                let mut changed = false;
                                ui.scope_builder(
                                    egui::UiBuilder::new()
                                        .max_rect(rect)
                                        .layout(Layout::left_to_right(Align::Center)),
                                    |ui| {
                                        let check = ui.add_enabled(
                                            enabled,
                                            egui::Checkbox::without_text(&mut selected),
                                        );
                                        check.widget_info(|| {
                                            egui::WidgetInfo::selected(
                                                egui::Role::CheckBox,
                                                enabled,
                                                selected,
                                                &entry.name,
                                            )
                                        });
                                        theme::focus(&check);
                                        changed = check.changed();
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(widths, theme::ROW),
                                            Layout::left_to_right(Align::Center),
                                            |ui| {
                                                ui.set_min_width(widths);
                                                theme::icon(
                                                    ui,
                                                    if entry.kind == Kind::Directory {
                                                        Icon::Folder
                                                    } else {
                                                        Icon::File
                                                    },
                                                    20.0,
                                                );
                                                let name = if matches!(
                                                    entry.kind,
                                                    Kind::Link | Kind::Other
                                                ) {
                                                    format!("{} (링크)", entry.name)
                                                } else {
                                                    entry.name.clone()
                                                };
                                                let limit = ((widths - 32.0) / theme::BODY).max(8.0)
                                                    as usize;
                                                ui.label(present::middle_text(&name, limit));
                                            },
                                        );
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(96.0, theme::ROW),
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.set_min_width(96.0);
                                                ui.label(entry.bytes.map_or_else(
                                                    || "알 수 없음".into(),
                                                    present::bytes_text,
                                                ));
                                            },
                                        );
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(64.0, theme::ROW),
                                            Layout::left_to_right(Align::Center),
                                            |ui| {
                                                ui.set_min_width(64.0);
                                                if entry.encrypted {
                                                    theme::icon(ui, Icon::Lock, 16.0);
                                                    ui.label("있음");
                                                }
                                            },
                                        );
                                    },
                                );
                                row.widget_info(|| {
                                    egui::WidgetInfo::selected(
                                        egui::Role::Button,
                                        enabled,
                                        selected,
                                        &entry.name,
                                    )
                                });
                                row.clone().on_hover_text(&entry.name);
                                theme::focus(&row);
                                let content = ui.interact(
                                    egui::Rect::from_min_max(
                                        rect.min + egui::vec2(32.0, 0.0),
                                        rect.max,
                                    ),
                                    row.id.with("content"),
                                    if enabled {
                                        egui::Sense::CLICK
                                    } else {
                                        egui::Sense::hover()
                                    },
                                );
                                if enabled && !changed && (row.clicked() || content.clicked()) {
                                    selected = !selected;
                                    changed = true;
                                }
                                if changed {
                                    if selected {
                                        self.model.selected.insert(entry.id);
                                    } else {
                                        self.model.selected.remove(&entry.id);
                                    }
                                }
                            }
                        });
                }
            });
    }

    fn result_visible(&self) -> bool {
        !self.dismissed
            && !self.model.busy()
            && self.last_request.as_ref().is_some_and(|request| {
                !matches!(request, Start::List { .. })
                    || self.model.error.is_some()
                    || self
                        .model
                        .terminal
                        .as_ref()
                        .is_some_and(|t| t.outcome != Outcome::Completed)
            })
    }

    fn work_area(&mut self, ui: &mut egui::Ui) {
        if self.model.busy() {
            self.progress(ui);
        } else if self.result_visible() {
            self.result(ui);
        } else if self.model.list_id.is_some() {
            ui.horizontal(|ui| {
                ui.strong("풀 위치");
                let width = ui.available_width() - 136.0;
                theme::column_label(
                    ui,
                    present::middle_text(
                        &self.destination.display().to_string(),
                        (width / theme::BODY).max(10.0) as usize,
                    ),
                    width,
                    Align::Min,
                )
                .on_hover_text(self.destination.display().to_string());
                if theme::action(ui, "폴더 바꾸기", false, true).clicked() {
                    self.open_picker(Purpose::Destination);
                }
            });
            if self
                .model
                .list_id
                .as_ref()
                .is_some_and(|id| id.format == Format::Gzip)
            {
                ui.horizontal(|ui| {
                    ui.label("저장할 파일 이름");
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.gzip_name)
                            .id(egui::Id::unique("gzip-name")),
                    );
                    theme::focus(&edit);
                });
            }
            ui.horizontal(|ui| {
                let response = ui.checkbox(&mut self.selected_only, "선택한 항목만 풀기");
                theme::focus(&response);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let enabled = self.model.can_extract()
                        && (!self.selected_only || !self.model.selected.is_empty());
                    if theme::action(ui, "풀기", true, enabled)
                        .on_disabled_hover_text("풀 항목을 선택하세요")
                        .clicked()
                    {
                        self.begin_extract();
                    }
                });
            });
        }
        if let Some(error) = &self.ui_error {
            ui.colored_label(theme::ERROR, error);
            if error == present::FOLDER_ERROR
                && theme::action(ui, "폴더 바꾸기", false, true).clicked()
            {
                self.open_picker(Purpose::Destination);
            }
        }
        if let Some(error) = &self.model.input_error {
            ui.colored_label(
                theme::ERROR,
                if error.starts_with("암호는") {
                    error.as_str()
                } else {
                    "풀 위치와 선택한 항목을 확인하세요"
                },
            );
        }
    }

    fn progress(&mut self, ui: &mut egui::Ui) {
        let creating = matches!(self.last_request, Some(Start::CreateZip { .. }));
        let cancelling = self.model.cancellation_requested() || self.model.cancel.is_some();
        let title = present::activity_text(&self.model, creating);
        ui.strong(title);
        if let Some((_, done, total, count)) = self.model.progress {
            if let Some(total) = total.filter(|n| *n > 0) {
                ui.scope(|ui| {
                    ui.visuals_mut().extreme_bg_color = theme::BORDER;
                    ui.add(
                        egui::ProgressBar::new((done as f64 / total as f64).clamp(0.0, 1.0) as f32)
                            .fill(theme::BLUE)
                            .desired_height(theme::SPACE * 2.0),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{:.0}% / {count}개 처리함, {} / {}",
                        (done as f64 / total as f64).clamp(0.0, 1.0) * 100.0,
                        present::bytes_text(done),
                        present::bytes_text(total)
                    ));
                    self.cancel_action(ui, cancelling);
                });
            } else {
                theme::indeterminate(ui);
                ui.horizontal(|ui| {
                    ui.label(format!("{count}개 처리함, {}", present::bytes_text(done)));
                    self.cancel_action(ui, cancelling);
                });
            }
        } else {
            theme::indeterminate(ui);
            ui.horizontal(|ui| {
                ui.label("작업을 진행하고 있습니다");
                self.cancel_action(ui, cancelling);
            });
        }
    }

    fn cancel_action(&mut self, ui: &mut egui::Ui, cancelling: bool) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::action(ui, "취소", false, !cancelling).clicked() {
                self.clear_password();
                self.model.cancel();
            }
        });
    }

    fn result(&mut self, ui: &mut egui::Ui) {
        let t = self.model.terminal.clone();
        let listing = matches!(self.last_request, Some(Start::List { .. }));
        let creating = matches!(self.last_request, Some(Start::CreateZip { .. }));
        let warnings = if listing {
            Vec::new()
        } else {
            present::warnings(
                t.as_ref(),
                self.model.confirmed,
                self.targets.as_ref(),
                &self.directories,
                &self.model.outputs,
            )
        };
        let success = self.model.confirmed
            && self.model.error.is_none()
            && t.as_ref().is_some_and(|t| t.outcome == Outcome::Completed)
            && warnings
                .iter()
                .all(|warning| *warning == present::SYNC_WARNING);
        let cancelled =
            self.model.confirmed && t.as_ref().is_some_and(|t| t.outcome == Outcome::Cancelled);
        let title = if success {
            if creating {
                "ZIP 파일을 만들었습니다"
            } else {
                "압축을 풀었습니다"
            }
        } else if cancelled {
            "작업을 취소했습니다"
        } else if listing {
            "압축파일을 열지 못했습니다"
        } else if creating {
            "ZIP 파일을 만들지 못했습니다"
        } else {
            "압축을 풀지 못했습니다"
        };
        let fill = if success {
            theme::SUCCESS_BG
        } else if cancelled {
            theme::STRIPE
        } else {
            theme::ERROR_BG
        };
        let mut choose = false;
        let mut retry = false;
        let mut reopen = false;
        let mut folder = false;
        theme::panel(fill).show(ui, |ui| {
            ui.horizontal(|ui| {
                theme::icon(
                    ui,
                    if success {
                        Icon::Success
                    } else if cancelled {
                        Icon::Warning
                    } else {
                        Icon::Error
                    },
                    24.0,
                );
                ui.strong(title);
            });
            let max_height = (ui.ctx().content_rect().height() * 0.24).max(80.0);
            egui::ScrollArea::vertical()
                .id_salt("result-message")
                .max_height(max_height)
                .show(ui, |ui| {
                    if success {
                        ui.label(match self.last_request.as_ref() {
                            Some(Start::CreateZip { output_file, .. }) => {
                                output_file.display().to_string()
                            }
                            _ => format!("{} 폴더에 저장했습니다.", self.destination.display()),
                        });
                    } else if let Some(code) = t.as_ref().and_then(|t| t.error) {
                        ui.label(if creating && code == ErrorCode::SourceChanged {
                            "묶을 파일이 바뀌었습니다. 파일을 확인한 뒤 다시 만드세요"
                        } else {
                            present::error_text(code)
                        });
                    } else if let Some(error) = &self.model.error {
                        ui.label(present::client_error_text(error));
                    }
                    for warning in warnings
                        .iter()
                        .filter(|warning| !success || **warning != present::SYNC_WARNING)
                    {
                        ui.horizontal_wrapped(|ui| {
                            theme::icon(ui, Icon::Warning, 20.0);
                            ui.colored_label(theme::WARNING, *warning);
                        });
                    }
                    if !success
                        && !listing
                        && present::definitely_unsaved(
                            t.as_ref(),
                            self.model.confirmed,
                            &self.model.outputs,
                        )
                    {
                        ui.label("아무 파일도 풀지 않았습니다");
                    }
                    if self.details {
                        theme::panel(theme::STRIPE).show(ui, |ui| {
                            if success && warnings.contains(&present::SYNC_WARNING) {
                                ui.label(present::SYNC_WARNING);
                            }
                            if let Some(t) = &t {
                                for line in terminal_lines(t) {
                                    ui.label(line);
                                }
                            }
                            if let Some(error) = &self.model.error {
                                ui.label(error);
                            }
                            for (name, state) in &self.model.outputs {
                                ui.label(format!("{name}: {}", disposition_label(*state)));
                            }
                        });
                    }
                });
            let conflict = t
                .as_ref()
                .is_some_and(|t| t.error == Some(ErrorCode::Conflict))
                && !creating;
            let uncertain = warnings.contains(&present::UNCERTAIN);
            if conflict && uncertain {
                let response = ui.checkbox(
                    &mut self.checked_destination,
                    "기존 풀 위치 폴더를 확인했습니다",
                );
                theme::focus(&response);
            }
            ui.horizontal(|ui| {
                if theme::action(
                    ui,
                    if self.details {
                        "자세히 닫기"
                    } else {
                        "자세히 보기"
                    },
                    false,
                    true,
                )
                .clicked()
                {
                    self.details = !self.details;
                }
                let action = if success {
                    Some(("폴더 열기", true))
                } else if conflict {
                    Some(("다른 폴더에 풀기", !uncertain || self.checked_destination))
                } else if t
                    .as_ref()
                    .is_some_and(|t| t.error == Some(ErrorCode::SourceChanged))
                {
                    Some((
                        if creating {
                            "다시 만들기"
                        } else {
                            "압축파일 다시 열기"
                        },
                        true,
                    ))
                } else if !cancelled && (!uncertain || listing) {
                    Some(("다시 시도", true))
                } else {
                    None
                };
                let (confirm, primary) = theme::action_pair(ui, "확인", action);
                if confirm {
                    self.dismissed = true;
                    self.ui_error = None;
                    self.model.input_error = None;
                }
                if primary {
                    match action.map(|(label, _)| label) {
                        Some("폴더 열기") => folder = true,
                        Some("다른 폴더에 풀기") => choose = true,
                        Some("압축파일 다시 열기") => reopen = true,
                        Some("다시 시도" | "다시 만들기") => retry = true,
                        _ => {}
                    }
                }
            });
        });
        if choose {
            self.open_picker(Purpose::Retry);
        }
        if retry {
            self.retry();
        }
        if reopen {
            if let Some(path) = self.model.archive.clone() {
                self.open_path(path);
            }
        }
        if folder {
            let path = match self.last_request.as_ref() {
                Some(Start::CreateZip { output_file, .. }) => {
                    output_file.parent().unwrap_or(Path::new("/"))
                }
                _ => self.destination.as_path(),
            };
            match present::folder_command(path).spawn() {
                Ok(mut child) => {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                Err(_) => {
                    self.ui_error = Some(
                        "폴더를 열지 못했습니다. 파일 탐색기에서 저장 위치를 확인하세요".into(),
                    )
                }
            }
        }
    }

    fn create_dialog(&mut self, ctx: &egui::Context) {
        let mut create = false;
        let mut pick = None;
        let modal = egui::Modal::new(egui::Id::unique("create-zip")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).min(560.0));
            ui.heading("ZIP 만들기");
            egui::ScrollArea::vertical()
                .id_salt("create-fields")
                .scroll_bar_visibility(if ctx.content_rect().height() < 600.0 {
                    egui::scroll_area::ScrollBarVisibility::AlwaysVisible
                } else {
                    egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                })
                .max_height((ctx.content_rect().height() - 200.0).max(240.0))
                .show(ui, |ui| {
                    ui.strong("1. 묶을 파일과 폴더");
                    let mut remove = None;
                    theme::panel(theme::STRIPE).show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("zip-sources")
                            .max_height(128.0)
                            .show(ui, |ui| {
                                if self.sources.is_empty() {
                                    ui.label("여기에 파일이나 폴더를 끌어 놓으세요");
                                }
                                for (index, path) in self.sources.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        theme::icon(
                                            ui,
                                            if path.is_dir() {
                                                Icon::Folder
                                            } else {
                                                Icon::File
                                            },
                                            20.0,
                                        );
                                        theme::column_label(
                                            ui,
                                            present::middle_text(
                                                &path
                                                    .file_name()
                                                    .unwrap_or_default()
                                                    .to_string_lossy(),
                                                28,
                                            ),
                                            ui.available_width() - 80.0,
                                            Align::Min,
                                        )
                                        .on_hover_text(path.display().to_string());
                                        if theme::action(ui, "빼기", false, true).clicked() {
                                            remove = Some(index);
                                        }
                                    });
                                }
                            });
                    });
                    if let Some(index) = remove {
                        self.sources.remove(index);
                    }
                    ui.horizontal(|ui| {
                        if theme::action(ui, "파일 추가", false, true).clicked() {
                            pick = Some(Purpose::Files);
                        }
                        if theme::action(ui, "폴더 추가", false, true).clicked() {
                            pick = Some(Purpose::Folder);
                        }
                    });
                    ui.add_space(theme::SPACE);
                    ui.strong("2. 저장 위치");
                    ui.horizontal(|ui| {
                        ui.label("이름");
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut self.zip_name)
                                .id(egui::Id::unique("zip-name"))
                                .desired_width(ui.available_width()),
                        );
                        theme::focus(&edit);
                    });
                    if let Some(error) = &self.ui_error {
                        ui.colored_label(theme::ERROR, error);
                    }
                    ui.horizontal(|ui| {
                        ui.label("폴더");
                        theme::column_label(
                            ui,
                            present::middle_text(&self.zip_folder.display().to_string(), 30),
                            ui.available_width() - 136.0,
                            Align::Min,
                        )
                        .on_hover_text(self.zip_folder.display().to_string());
                        if theme::action(ui, "폴더 바꾸기", false, true).clicked() {
                            pick = Some(Purpose::ZipFolder);
                        }
                    });
                    ui.add_space(theme::SPACE);
                    ui.strong("3. 보호");
                    let response = ui.checkbox(&mut self.aes256, "암호로 보호하기");
                    theme::focus(&response);
                    ui.collapsing("자세한 설정", |ui| {
                        ui.radio_value(&mut self.method, Method::Deflate, "작게 압축(기본)");
                        ui.radio_value(&mut self.method, Method::Store, "빠르게 묶기");
                    });
                });
            ui.separator();
            let can_create = !self.sources.is_empty() && !self.zip_name.is_empty();
            let (cancel, confirm) = theme::action_pair(ui, "취소", Some(("만들기", can_create)));
            if cancel {
                self.create = false;
                self.ui_error = None;
                return;
            }
            create = confirm;
        });
        if !self.create || modal.should_close() {
            self.create = false;
            return;
        }
        if let Some(purpose) = pick {
            self.open_picker(purpose);
        }
        if create {
            self.begin_zip();
        }
    }

    fn password_dialog(&mut self, ctx: &egui::Context, new_challenge: bool) {
        let (_, attempt, encrypt) = self.model.challenge.unwrap();
        let mut submit = false;
        let mut cancel = false;
        let password_enter = ctx.memory(|memory| memory.focused()).is_some_and(|id| {
            id == egui::Id::unique("archive-password-input")
                || id == egui::Id::unique("archive-password-confirm")
        }) && ctx.input(|input| input.key_pressed(egui::Key::Enter));
        let modal = egui::Modal::new(egui::Id::unique("archive-password")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).min(420.0));
            ui.heading(if encrypt {
                "ZIP 암호 설정"
            } else {
                "암호가 필요합니다"
            });
            if encrypt {
                ui.label("암호를 잊으면 파일을 열 수 없습니다.");
            } else if let Some(path) = &self.model.archive {
                ui.label(format!(
                    "{}은 암호로 보호되어 있습니다.",
                    present::middle_text(
                        &path.file_name().unwrap_or_default().to_string_lossy(),
                        24
                    )
                ));
            }
            ui.horizontal(|ui| {
                ui.label("암호");
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.password)
                        .password(!self.password_visible)
                        .id(egui::Id::unique("archive-password-input"))
                        .char_limit(1024)
                        .desired_width(232.0),
                );
                theme::focus(&edit);
                if new_challenge {
                    edit.request_focus();
                }
                if theme::action(
                    ui,
                    if self.password_visible {
                        "숨기기"
                    } else {
                        "보기"
                    },
                    false,
                    true,
                )
                .clicked()
                {
                    self.password_visible = !self.password_visible;
                }
            });
            if encrypt {
                ui.horizontal(|ui| {
                    ui.label("암호 확인");
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.password_confirm)
                            .password(!self.password_visible)
                            .id(egui::Id::unique("archive-password-confirm"))
                            .char_limit(1024)
                            .desired_width(232.0),
                    );
                    theme::focus(&edit);
                });
                if self.password != self.password_confirm {
                    ui.colored_label(theme::ERROR, "두 암호가 같지 않습니다");
                }
            } else if attempt > 1 {
                ui.colored_label(theme::ERROR, "암호가 맞지 않습니다. 다시 입력하세요");
            }
            if self.password.len() > 1024 {
                ui.colored_label(theme::ERROR, "암호는 UTF-8 1~1024바이트로 입력하세요");
            }
            let valid = !self.password.is_empty()
                && self.password.len() <= 1024
                && (!encrypt || self.password == self.password_confirm);
            let actions = theme::action_pair(ui, "취소", Some(("확인", valid)));
            cancel = actions.0;
            submit = actions.1 || (valid && password_enter);
        });
        if modal.should_close() || cancel {
            self.clear_password();
            self.model.cancel();
        } else if submit {
            let password = std::mem::take(&mut self.password);
            self.clear_password();
            self.model.password(password);
        }
    }

    fn close_dialog(&mut self, ctx: &egui::Context) {
        if self.model.close == Close::Open || self.model.close == Close::Allowed {
            return;
        }
        egui::Modal::new(egui::Id::unique("archive-close")).show(ctx, |ui| {
            ui.set_width(420.0);
            match self.model.close {
                Close::Confirm => {
                    ui.heading("작업 중 닫기");
                    ui.label("작업을 취소하고 닫을까요?");
                    ui.horizontal(|ui| {
                        if theme::action(ui, "계속 작업", false, true).clicked() {
                            self.model.keep_open();
                        }
                        if theme::action(ui, "취소하고 닫기", true, true).clicked() {
                            self.clear_password();
                            self.model.confirm_close();
                        }
                    });
                }
                Close::Waiting => {
                    ui.heading("종료 확인 중");
                    ui.label("마무리하는 중입니다. 잠시만 기다려 주세요");
                    if theme::action(ui, "닫기 취소", false, true).clicked() {
                        self.model.keep_open();
                    }
                }
                Close::Review => {
                    ui.heading("결과 확인 후 닫기");
                    let wrote = self
                        .last_request
                        .as_ref()
                        .is_some_and(|request| !matches!(request, Start::List { .. }));
                    ui.label(if !wrote {
                        "프로그램을 닫을까요?"
                    } else if self.model.confirmed {
                        "작업 결과를 확인한 뒤 닫으세요"
                    } else {
                        present::UNCERTAIN
                    });
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .show(ui, |ui| {
                            if let Some(code) = self.model.terminal.as_ref().and_then(|t| t.error) {
                                ui.colored_label(theme::ERROR, present::error_text(code));
                            }
                            if wrote {
                                for warning in present::warnings(
                                    self.model.terminal.as_ref(),
                                    self.model.confirmed,
                                    self.targets.as_ref(),
                                    &self.directories,
                                    &self.model.outputs,
                                ) {
                                    ui.colored_label(theme::WARNING, warning);
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        if theme::action(ui, "계속 작업", false, true).clicked() {
                            self.model.keep_open();
                        }
                        if theme::action(ui, "닫기", true, true).clicked() {
                            self.model.acknowledge_close();
                        }
                    });
                }
                _ => {}
            }
        });
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
