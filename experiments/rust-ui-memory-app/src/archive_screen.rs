use crate::{
    archive_browser as browser,
    archive_dialog::{DialogRequest, Dialogs, NativeDialogs, Purpose},
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
    dialogs: Box<dyn Dialogs>,
    folder: String,
    search: String,
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
    pub fn new(factory: Box<dyn Factory>, startup: Option<PathBuf>, _fixture: bool) -> Self {
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
            dialogs: Box::new(NativeDialogs),
            folder: String::new(),
            search: String::new(),
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

    pub fn set_dialogs(&mut self, dialogs: Box<dyn Dialogs>) {
        self.dialogs = dialogs;
    }

    pub fn browse_folder(&mut self, folder: &str) {
        self.folder = folder.trim_matches('/').to_owned();
        self.search.clear();
    }

    pub fn show_create(&mut self, encrypted: bool) {
        if !self.model.busy() {
            self.create = true;
            self.aes256 = encrypted;
            self.ui_error = None;
        }
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
        self.gzip_name = present::gzip_output_name(&archive);
        self.selected_only = false;
        self.folder.clear();
        self.search.clear();
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
        let mut validated = Vec::new();
        for path in paths {
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                self.ui_error = Some("파일이나 폴더를 찾지 못했습니다. 다시 선택하세요".into());
                return;
            };
            if metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
                self.ui_error = Some("일반 파일과 폴더만 추가할 수 있습니다".into());
                return;
            }
            let Ok(path) = path.canonicalize() else {
                return;
            };
            if path.to_str().is_none()
                || !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(valid_leaf)
            {
                self.ui_error = Some("파일이나 폴더의 이름을 확인하세요".into());
                return;
            }
            validated.push(path);
        }
        let paths = validated;
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
            Selection::EntryIds(
                browser::expand_selection(&self.model.entries, &self.model.selected)
                    .into_iter()
                    .collect(),
            )
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
        if self.model.busy() {
            return;
        }
        let gzip = self
            .model
            .list_id
            .as_ref()
            .is_some_and(|id| id.format == Format::Gzip);
        let purpose = match (purpose, gzip) {
            (Purpose::Destination, true) => Purpose::SaveGzip,
            (Purpose::Retry, true) => Purpose::RetryGzip,
            _ => purpose,
        };
        let directory = match purpose {
            Purpose::SaveZip => self.zip_folder.clone(),
            Purpose::Destination | Purpose::SaveGzip => self
                .model
                .archive
                .as_ref()
                .and_then(|path| path.parent())
                .map(Path::to_path_buf)
                .unwrap_or_else(present::home_directory),
            Purpose::Retry | Purpose::RetryGzip => self.destination.clone(),
            _ => self
                .sources
                .last()
                .and_then(|path| path.parent())
                .map(Path::to_path_buf)
                .unwrap_or_else(present::home_directory),
        };
        let request = DialogRequest {
            purpose,
            directory,
            file_name: if matches!(purpose, Purpose::SaveGzip | Purpose::RetryGzip) {
                self.gzip_name.clone()
            } else {
                self.zip_name.clone()
            },
        };
        let answer = self.dialogs.choose(request);
        if answer.is_ok()
            && self.ui_error.as_deref() == Some(crate::archive_dialog::DIALOG_UNAVAILABLE)
        {
            self.ui_error = None;
        }
        match answer {
            Ok(Some(paths)) if !paths.is_empty() => {
                let path = paths[0].clone();
                match purpose {
                    Purpose::Open => {
                        if std::fs::symlink_metadata(&path)
                            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
                        {
                            self.open_path(path);
                        } else {
                            self.ui_error = Some("일반 압축파일을 선택하세요".into());
                        }
                    }
                    Purpose::Files | Purpose::Folder => self.add_sources(paths),
                    Purpose::Destination | Purpose::Retry => {
                        self.choose_destination(path);
                        self.selected_only = !self.model.selected.is_empty();
                        if purpose == Purpose::Retry {
                            self.retry();
                        } else {
                            self.begin_extract();
                        }
                    }
                    Purpose::SaveGzip | Purpose::RetryGzip => {
                        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                            self.ui_error = Some("저장할 파일 이름을 확인하세요".into());
                            return;
                        };
                        if !present::valid_output_leaf(name) {
                            self.ui_error = Some("저장할 파일 이름을 확인하세요".into());
                            return;
                        }
                        let name = name.to_owned();
                        self.choose_destination(
                            path.parent().unwrap_or(Path::new("/")).to_path_buf(),
                        );
                        self.gzip_name = name.clone();
                        self.selected_only = !self.model.selected.is_empty();
                        if purpose == Purpose::RetryGzip {
                            if let Some(Start::Extract {
                                gzip_output_name, ..
                            }) = &mut self.last_request
                            {
                                *gzip_output_name = Some(name);
                            }
                            self.retry();
                        } else {
                            self.begin_extract();
                        }
                    }
                    Purpose::SaveZip => {
                        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                            self.zip_name = name.to_owned();
                            self.zip_folder = path.parent().unwrap_or(Path::new("/")).to_path_buf();
                            self.begin_zip();
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(error) => self.ui_error = Some(error),
        }
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

    fn on_drop(&mut self, paths: Vec<PathBuf>) {
        if self.model.busy() || paths.is_empty() {
            return;
        }
        if paths.len() == 1 && !self.create && !paths[0].is_dir() {
            if std::fs::symlink_metadata(&paths[0])
                .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
            {
                self.open_path(paths[0].clone());
            } else {
                self.ui_error = Some("일반 압축파일을 선택하세요".into());
            }
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
        let title = self
            .model
            .archive
            .as_ref()
            .map(|path| {
                let full = if path.is_absolute() {
                    path.clone()
                } else {
                    std::env::current_dir().unwrap_or_default().join(path)
                };
                format!("{} — Zipelo", full.display())
            })
            .unwrap_or_else(|| "Zipelo".into());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        egui::Panel::top("archive-actions")
            .frame(
                theme::panel(theme::PANEL)
                    .corner_radius(0)
                    .inner_margin(egui::Margin::symmetric(16, 4)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    let idle = !self.model.busy() && self.model.close == Close::Open;
                    if theme::tool(ui, "열기", theme::OPEN, idle, false).clicked() {
                        self.open_picker(Purpose::Open);
                    }
                    if theme::tool(ui, "압축하기", theme::CREATE, idle, false).clicked() {
                        self.show_create(false);
                    }
                    if theme::tool(
                        ui,
                        "압축해제",
                        theme::EXTRACT,
                        self.model.can_extract(),
                        true,
                    )
                    .clicked()
                    {
                        self.open_picker(Purpose::Destination);
                    }
                });
            });
        egui::Panel::bottom("archive-status")
            .frame(
                theme::panel(theme::STRIPE)
                    .corner_radius(0)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{}개 항목",
                        browser::rows(&self.model.entries, &self.folder, &self.search).len()
                    ));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(format!("선택 {}개", self.model.selected.len()));
                    });
                });
            });
        if self.model.busy() || self.result_visible() || (self.ui_error.is_some() && !self.create) {
            egui::Panel::bottom("archive-work")
                .frame(theme::panel(theme::PANEL).corner_radius(0))
                .show(root, |ui| self.work_area(ui));
        }
        egui::CentralPanel::default()
            .frame(
                theme::panel(theme::PANEL)
                    .corner_radius(0)
                    .inner_margin(egui::Margin::ZERO),
            )
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
        theme::panel(theme::STRIPE).corner_radius(0).show(ui, |ui| {
            ui.horizontal(|ui| {
                let up = theme::icon_button(
                    ui,
                    "위로",
                    theme::UP,
                    !self.folder.is_empty() && !self.model.busy(),
                );
                if up.clicked() {
                    let parent = self
                        .folder
                        .rsplit_once('/')
                        .map_or("", |(parent, _)| parent)
                        .to_owned();
                    self.browse_folder(&parent);
                }
                ui.label("현재 경로");
                let mut path = format!("/{}", self.folder);
                ui.add_sized(
                    [ui.available_width() - 270.0, 36.0],
                    egui::TextEdit::singleline(&mut path).interactive(false),
                );
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                theme::svg_icon(ui.painter(), rect, theme::SEARCH, theme::MUTED);
                let search = ui.add_sized(
                    [220.0, 36.0],
                    egui::TextEdit::singleline(&mut self.search)
                        .id(egui::Id::unique("archive-search"))
                        .hint_text("파일 찾기"),
                );
                theme::focus(&search);
            });
        });
        let rows = browser::rows(&self.model.entries, &self.folder, &self.search);
        let visible = rows
            .iter()
            .flat_map(|row| row.ids.iter().copied())
            .collect::<BTreeSet<_>>();
        let mut all = !visible.is_empty() && visible.is_subset(&self.model.selected);
        let name_width = (ui.available_width() - 32.0 - 96.0 - 64.0 - 64.0).max(160.0);
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            let check = ui
                .add_enabled(
                    !self.model.busy() && !visible.is_empty(),
                    egui::Checkbox::without_text(&mut all),
                )
                .on_hover_text("전체 선택");
            if check.changed() {
                if all {
                    self.model.selected.extend(&visible);
                } else {
                    browser::remove_selection(
                        &self.model.entries,
                        &mut self.model.selected,
                        &visible,
                    );
                }
            }
            theme::focus(&check);
            theme::column_label(ui, RichText::new("이름").strong(), name_width, Align::Min);
            theme::column_label(ui, "크기", 96.0, Align::Max);
            theme::column_label(ui, "암호", 64.0, Align::Min);
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("archive-rows")
            .auto_shrink([false, false])
            .show_rows(ui, 38.0, rows.len(), |ui, range| {
                for index in range {
                    let row = &rows[index];
                    let mut selected =
                        !row.ids.is_empty() && row.ids.is_subset(&self.model.selected);
                    let response = ui
                        .horizontal(|ui| {
                            ui.set_min_height(36.0);
                            ui.add_space(16.0);
                            let check = ui.add_enabled(
                                !self.model.busy() && !row.ids.is_empty(),
                                egui::Checkbox::without_text(&mut selected),
                            );
                            check.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::Role::CheckBox,
                                    !self.model.busy(),
                                    selected,
                                    &row.name,
                                )
                            });
                            theme::focus(&check);
                            if check.changed() {
                                if selected {
                                    self.model.selected.extend(&row.ids);
                                } else {
                                    browser::remove_selection(
                                        &self.model.entries,
                                        &mut self.model.selected,
                                        &row.ids,
                                    );
                                }
                            }
                            theme::icon(
                                ui,
                                if row.directory {
                                    Icon::Folder
                                } else {
                                    Icon::File
                                },
                                20.0,
                            );
                            let label = ui
                                .allocate_ui_with_layout(
                                    egui::vec2(name_width - 28.0, 34.0),
                                    Layout::left_to_right(Align::Center),
                                    |ui| {
                                        ui.set_min_width(name_width - 28.0);
                                        ui.add_enabled(
                                            !self.model.busy(),
                                            egui::Button::new(&row.name)
                                                .selected(selected)
                                                .frame(false)
                                                .min_size(egui::vec2(0.0, 34.0)),
                                        )
                                    },
                                )
                                .inner;
                            theme::focus(&label);
                            let enter =
                                label.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if row.directory && (label.double_clicked() || enter) {
                                self.browse_folder(&row.path);
                            } else if !row.directory && label.clicked() {
                                if selected {
                                    browser::remove_selection(
                                        &self.model.entries,
                                        &mut self.model.selected,
                                        &row.ids,
                                    );
                                } else {
                                    self.model.selected.extend(&row.ids);
                                }
                            }
                            theme::column_label(
                                ui,
                                if row.directory {
                                    "0 B".into()
                                } else {
                                    row.bytes
                                        .map(present::bytes_text)
                                        .unwrap_or_else(|| "—".into())
                                },
                                96.0,
                                Align::Max,
                            );
                            if row.encrypted {
                                theme::icon(ui, Icon::Lock, 16.0);
                            }
                        })
                        .response;
                    ui.painter().hline(
                        response.rect.x_range(),
                        response.rect.bottom(),
                        egui::Stroke::new(1.0, theme::BORDER),
                    );
                }
            });
        if rows.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(if self.search.is_empty() {
                    "이 폴더는 비어 있습니다"
                } else {
                    "일치하는 항목이 없습니다"
                });
            });
        }
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

    fn error_notice(ui: &mut egui::Ui, error: &str) {
        let response = theme::panel(theme::ERROR_BG)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    theme::icon(ui, Icon::Error, 24.0);
                    ui.colored_label(theme::ERROR, error);
                });
            })
            .response;
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Alert, true, error));
    }

    fn work_area(&mut self, ui: &mut egui::Ui) {
        if self.model.busy() {
            self.progress(ui);
        } else if self.result_visible() {
            self.result(ui);
        }
        if !self.create
            && let Some(error) = &self.ui_error
        {
            Self::error_notice(ui, error);
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
            present::publication_warnings(
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
            && warnings.is_empty();
        let durability_warning = t
            .as_ref()
            .filter(|_| self.model.confirmed && !listing)
            .and_then(|t| present::durability_warning(t.durability));
        let durability_in_details = success
            && t.as_ref()
                .is_some_and(|t| t.durability == Durability::Unconfirmed);
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
                        .copied()
                        .chain(durability_warning.filter(|_| !durability_in_details))
                    {
                        ui.horizontal_wrapped(|ui| {
                            theme::icon(ui, Icon::Warning, 20.0);
                            ui.colored_label(theme::WARNING, warning);
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
                            if durability_in_details {
                                if let Some(warning) = durability_warning {
                                    ui.label(warning);
                                }
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
                    Some((
                        if self
                            .model
                            .list_id
                            .as_ref()
                            .is_some_and(|id| id.format == Format::Gzip)
                        {
                            "다른 이름으로 저장"
                        } else {
                            "다른 폴더에 풀기"
                        },
                        !uncertain || self.checked_destination,
                    ))
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
                        Some("다른 폴더에 풀기" | "다른 이름으로 저장") => {
                            choose = true
                        }
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
        let mut compress = false;
        let mut pick = None;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::unique("create-zip")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).min(600.0));
            ui.horizontal(|ui| {
                ui.heading("ZIP 만들기");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let close = theme::icon_button(ui, "닫기", theme::CLOSE, true);
                    theme::focus(&close);
                    cancel = close.clicked();
                });
            });
            if let Some(error) = &self.ui_error {
                Self::error_notice(ui, error);
            }
            egui::ScrollArea::vertical()
                .id_salt("create-fields")
                .max_height(
                    (ctx.content_rect().height()
                        - 180.0
                        - if self.ui_error.is_some() { 96.0 } else { 0.0 })
                    .max(120.0),
                )
                .show(ui, |ui| {
                    ui.add_space(16.0);
                    let frame = theme::panel(theme::STRIPE)
                        .stroke(egui::Stroke::new(1.0, theme::DASH))
                        .inner_margin(egui::Margin::same(24));
                    let area = frame
                        .show(ui, |ui| {
                            ui.set_min_size(egui::vec2(ui.available_width(), 144.0));
                            ui.vertical_centered(|ui| {
                                theme::icon(ui, Icon::File, 44.0);
                                ui.add_space(16.0);
                                ui.label(
                                    RichText::new("파일이나 폴더를 여기에 끌어 놓으세요")
                                        .size(18.0),
                                );
                            });
                        })
                        .response;
                    let drop = ui.interact(
                        area.rect,
                        egui::Id::unique("zip-source-drop"),
                        egui::Sense::click(),
                    );
                    theme::focus(&drop);
                    drop.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, "파일 추가")
                    });
                    if drop.clicked() {
                        pick = Some(Purpose::Files);
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("zip-sources")
                        .max_height(86.0)
                        .show(ui, |ui| {
                            let mut remove = None;
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
                                    ui.label(present::middle_text(
                                        &path.file_name().unwrap_or_default().to_string_lossy(),
                                        36,
                                    ))
                                    .on_hover_text(path.display().to_string());
                                    if ui.button("빼기").clicked() {
                                        remove = Some(index);
                                    }
                                });
                            }
                            if let Some(index) = remove {
                                self.sources.remove(index);
                            }
                        });
                    ui.horizontal(|ui| {
                        if theme::action(ui, "파일 추가", false, true).clicked() {
                            pick = Some(Purpose::Files);
                        }
                        if theme::action(ui, "폴더 추가", false, true).clicked() {
                            pick = Some(Purpose::Folder);
                        }
                    });
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        theme::icon(ui, Icon::Lock, 24.0);
                        ui.strong("암호화");
                    });
                    let check = ui.checkbox(&mut self.aes256, "암호로 보호하기");
                    theme::focus(&check);
                    ui.add_space(16.0);
                });
            let (dismiss, confirm) =
                theme::action_pair(ui, "취소", Some(("압축하기", !self.sources.is_empty())));
            cancel |= dismiss;
            compress = confirm;
        });
        if cancel || modal.should_close() {
            self.create = false;
            self.ui_error = None;
            return;
        }
        if let Some(purpose) = pick {
            self.open_picker(purpose);
        }
        if compress {
            self.open_picker(Purpose::SaveZip);
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
