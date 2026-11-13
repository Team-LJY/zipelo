#[cfg(all(feature = "ui-fixture", debug_assertions))]
#[path = "../../../tests/rust-app-ui/scenarios.rs"]
pub mod scenarios;

use crate::{
    archive_browser as browser,
    archive_dialog::{
        DIALOG_UNAVAILABLE, DialogAnswer, DialogRequest, Dialogs, NativeDialogs, PendingDialog,
        Purpose,
    },
    archive_present as present,
    archive_theme::{self as theme, Icon},
    archive_ui::*,
    window_frame,
};
use eframe::egui::{self, Align, Layout, RichText};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const WORK_WIDTH: f32 = 480.0;
const CREATE_WIDTH: f32 = 560.0;
/// 처음 열 때의 높이. 그다음부터는 내용 높이에 맞춤.
const CREATE_MIN_HEIGHT: f32 = 300.0;
const CREATE_LIST_HEIGHT: f32 = 288.0;

/// ZIP 만들기 창 목록의 한 행. 디스크에서 읽은 파일이나 폴더임.
#[derive(Clone)]
struct SourceRow {
    name: String,
    directory: bool,
    bytes: Option<u64>,
}

fn source_row(path: &Path) -> SourceRow {
    let metadata = std::fs::symlink_metadata(path).ok();
    SourceRow {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        // 심볼릭 링크는 따라가지 않으므로 폴더로 보이지 않음.
        directory: metadata.as_ref().is_some_and(|m| m.is_dir()),
        bytes: metadata.filter(|m| m.is_file()).map(|m| m.len()),
    }
}

/// 폴더 안 항목을 메인 목록과 같은 순서(폴더 먼저, 이름순)로 읽음. 읽지 못하면 None.
fn read_source_dir(dir: &Path) -> Option<std::rc::Rc<Vec<SourceRow>>> {
    let mut rows = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| source_row(&entry.path()))
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.cmp(&b.name))
    });
    Some(std::rc::Rc::new(rows))
}

/// 덮어쓴 뒤 버려 메모리에 암호가 남지 않게 함.
fn wipe(value: &mut String) {
    let mut bytes = std::mem::take(value).into_bytes();
    bytes.fill(0);
}
const WORK_MIN_HEIGHT: f32 = 96.0;
const WORK_MAX_HEIGHT: f32 = 520.0;
const WORK_RESULT_MAX_HEIGHT: f32 = 220.0;

/// 작업 진행·결과를 보여주는 자식 창의 위치 상태.
/// 부모 창 기준 상대 위치를 기억해 두고, 부모 창이 움직이면 같은 상대 위치로 따라감.
struct WorkWindow {
    open: bool,
    initial: Option<egui::Pos2>,
    offset: Option<egui::Vec2>,
    parent: Option<egui::Pos2>,
    expected: Option<egui::Pos2>,
    settle_until: f64,
    height: f32,
}

impl Default for WorkWindow {
    fn default() -> Self {
        Self {
            open: false,
            initial: None,
            offset: None,
            parent: None,
            expected: None,
            settle_until: 0.0,
            height: 168.0,
        }
    }
}

impl WorkWindow {
    /// 부모 창 가운데에 열고, 사용자가 옮긴 상대 위치를 유지한 채 부모 창을 따라가게 함.
    /// 앱을 쓰는 동안에만 부모 위에 두고, 처음 위치를 담은 ViewportBuilder를 돌려줌.
    /// Wayland처럼 창 위치를 알 수 없으면 위치는 창 관리자에 맡김.
    fn builder(
        &mut self,
        ctx: &egui::Context,
        id: egui::ViewportId,
        size: egui::Vec2,
        title: &str,
        decorations: bool,
        native_dialog_open: bool,
    ) -> egui::ViewportBuilder {
        let (parent, parent_focused, now) = ctx.input(|i| {
            (
                i.viewport().outer_rect,
                i.viewport().focused.unwrap_or(false),
                i.time,
            )
        });
        let (child, child_focused) = ctx.input(|i| {
            i.raw.viewports.get(&id).map_or((None, false), |v| {
                (v.outer_rect, v.focused.unwrap_or(false))
            })
        });
        if !self.open {
            // 처음 열 때 부모 창 가운데에 둠. 닫혔다 다시 열 때는 기억한 위치에 둠.
            self.open = true;
            self.initial = parent.map(|p| match self.offset {
                Some(offset) => (p.min + offset).round(),
                None => (p.center() - size / 2.0).round(),
            });
            self.offset = self.initial.zip(parent).map(|(c, p)| c - p.min);
            self.parent = parent.map(|p| p.min);
            self.expected = self.initial;
            self.settle_until = now + 0.5;
        }
        if let (Some(parent), Some(child)) = (parent, child) {
            let parent_moved = self.parent.is_some_and(|p| (p - parent.min).length() > 0.5);
            let settled = self
                .expected
                .is_some_and(|e| (child.min - e).length() <= 2.0);
            if settled || now >= self.settle_until {
                self.settle_until = 0.0;
            }
            if parent_moved {
                if let Some(offset) = self.offset {
                    let target = (parent.min + offset).round();
                    ctx.send_viewport_cmd_to(id, egui::ViewportCommand::OuterPosition(target));
                    self.expected = Some(target);
                    self.settle_until = now + 0.5;
                }
            } else if self.settle_until == 0.0 && !settled {
                // 사용자가 자식 창을 옮긴 위치를 부모 기준으로 기억함.
                self.offset = Some(child.min - parent.min);
                self.expected = Some(child.min);
            }
            self.parent = Some(parent.min);
        }
        let mut builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_icon(crate::app_icon::load())
            .with_decorations(decorations)
            .with_inner_size(size)
            .with_resizable(false)
            .with_minimize_button(false)
            .with_maximize_button(false)
            .with_taskbar(false)
            .with_window_level(
                if !native_dialog_open && (parent_focused || child_focused) {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                },
            );
        if let Some(position) = self.initial {
            builder = builder.with_position(position);
        }
        builder
    }
}

struct Marquee {
    // 스크롤 중에도 시작점을 목록 좌표로 유지함.
    origin: egui::Pos2,
    base: BTreeSet<u32>,
    moved: bool,
}

/// 파일 목록 한 행의 높이. 행 안의 크기 칸(theme::ROW)이 위아래 2px 여백으로 들어감.
const LIST_ROW: f32 = 36.0;

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
    conflict_challenge: Option<(u64, u64)>,
    conflict_apply: bool,
    aes256: bool,
    create: bool,
    selected_only: bool,
    method: Method,
    dialogs: Box<dyn Dialogs>,
    picker: Option<(Purpose, std::sync::mpsc::Receiver<DialogAnswer>)>,
    work: WorkWindow,
    create_win: WorkWindow,
    /// ZIP 만들기 창에서 보고 있는 폴더 (고른 폴더 기준 상대 경로, 비어 있으면 최상위).
    create_folder: String,
    create_listing: Option<(PathBuf, Option<std::rc::Rc<Vec<SourceRow>>>)>,
    /// 마지막으로 누른 폴더 행 (이름, 프레임 번호, 시각). 두 번 누름을 직접 판단할 때 씀.
    create_last_click: Option<(String, u64, f64)>,
    /// ZIP 만들기 목록에서 한 번 눌러 선택한 항목 이름. 보고 있는 폴더가 바뀌면 지움.
    create_selected: Option<String>,
    zip_password: String,
    zip_password_confirm: String,
    zip_password_visible: bool,
    /// 암호를 지웠으면 입력창의 실행 취소 기록도 다음 그리기 전에 지움.
    forget_password_history: Vec<&'static str>,
    /// 작업을 시작한 뒤 작업자가 암호를 요청하면 넘길 암호. 넘기거나 작업이 끝나면 지움.
    pending_zip_password: Option<String>,
    custom_frame: bool,
    release_after_move: bool,
    folder: String,
    search: String,
    list_anchor: Option<usize>,
    list_cursor: Option<usize>,
    list_offset: f32,
    list_focused: bool,
    marquee: Option<Marquee>,
    last_request: Option<Start>,
    targets: Option<BTreeSet<String>>,
    directories: BTreeSet<String>,
    dismissed: bool,
    details: bool,
    checked_destination: bool,
    ui_error: Option<String>,
    initialized: bool,
    #[cfg(all(feature = "ui-fixture", debug_assertions))]
    scenario_marquee: Option<egui::Rect>,
    #[cfg(all(feature = "ui-fixture", debug_assertions))]
    scenario_sources: Option<std::rc::Rc<Vec<SourceRow>>>,
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
            conflict_challenge: None,
            conflict_apply: false,
            aes256: false,
            create: false,
            selected_only: false,
            method: Method::Deflate,
            dialogs: Box::new(NativeDialogs),
            picker: None,
            work: WorkWindow::default(),
            create_win: WorkWindow::default(),
            create_folder: String::new(),
            create_listing: None,
            create_last_click: None,
            create_selected: None,
            zip_password: String::new(),
            zip_password_confirm: String::new(),
            zip_password_visible: false,
            forget_password_history: Vec::new(),
            pending_zip_password: None,
            custom_frame: false,
            release_after_move: false,
            folder: String::new(),
            search: String::new(),
            list_anchor: None,
            list_cursor: None,
            list_offset: 0.0,
            list_focused: false,
            marquee: None,
            last_request: None,
            targets: None,
            directories: BTreeSet::new(),
            dismissed: false,
            details: false,
            checked_destination: false,
            ui_error: None,
            initialized: false,
            #[cfg(all(feature = "ui-fixture", debug_assertions))]
            scenario_marquee: None,
            #[cfg(all(feature = "ui-fixture", debug_assertions))]
            scenario_sources: None,
        };
        if let Some(path) = startup {
            app.open_path(path);
        }
        app
    }

    pub fn set_dialogs(&mut self, dialogs: Box<dyn Dialogs>) {
        self.dialogs = dialogs;
    }

    /// 운영체제가 창 테두리를 그려 주지 않을 때(GNOME Wayland) 제목 표시줄을 직접 그림.
    pub fn set_custom_frame(&mut self, custom: bool) {
        self.custom_frame = custom;
    }

    pub fn browse_folder(&mut self, folder: &str) {
        self.folder = folder.trim_matches('/').to_owned();
        self.search.clear();
        self.clear_list_selection();
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
        // 이 함수는 암호 요청이 없는 동안 매 프레임 불리므로, 암호 창을 쓴 뒤에만 기록을 지움.
        if self.password_challenge.is_some() {
            self.forget_password_history
                .extend(["archive-password-input", "archive-password-confirm"]);
        }
    }

    /// 지운 암호가 입력창의 실행 취소(Ctrl+Z)로 되살아나지 않게 egui가 보관하는
    /// 입력창 상태(실행 취소 기록 포함)를 지움.
    fn forget_password_history(&mut self, ctx: &egui::Context) {
        if self.forget_password_history.is_empty() {
            return;
        }
        let names = std::mem::take(&mut self.forget_password_history);
        ctx.data_mut(|data| {
            for name in names {
                data.remove::<egui::text_edit::TextEditState>(egui::Id::unique(name));
            }
        });
    }

    fn clear_zip_password(&mut self) {
        wipe(&mut self.zip_password);
        wipe(&mut self.zip_password_confirm);
        if let Some(mut pending) = self.pending_zip_password.take() {
            wipe(&mut pending);
        }
        self.zip_password_visible = false;
        self.forget_password_history
            .extend(["zip-password-input", "zip-password-confirm"]);
    }

    /// 암호로 보호하지 않거나, 두 암호가 같고 1~1024바이트이면 만들 수 있음.
    fn zip_password_ready(&self) -> bool {
        !self.aes256
            || (!self.zip_password.is_empty()
                && self.zip_password.len() <= 1024
                && self.zip_password == self.zip_password_confirm)
    }

    fn close_create(&mut self) {
        self.create = false;
        self.ui_error = None;
        self.clear_zip_password();
        self.create_win = WorkWindow::default();
    }

    /// 압축하기: 묶을 폴더를 운영체제 폴더 선택 창에서 고른 뒤 ZIP 만들기 창을 엶.
    fn choose_zip_folder(&mut self) {
        if self.model.busy() || self.picker.is_some() {
            return;
        }
        // 이미 열린 ZIP 만들기 창의 목록과 암호 설정은 새 폴더를 실제로 고른 뒤에만 바꿈.
        // 선택을 취소하면 그대로 남음(apply_picker의 Purpose::Folder).
        self.open_picker(Purpose::Folder);
    }

    /// 폴더 하나를 골랐으면 그 폴더 안을 목록으로 보여 줌. 여러 항목이면 그 항목들을 보여 줌.
    fn create_root(&self) -> Option<PathBuf> {
        (self.sources.len() == 1 && self.sources[0].is_dir()).then(|| self.sources[0].clone())
    }

    fn create_rows(&mut self) -> Option<std::rc::Rc<Vec<SourceRow>>> {
        #[cfg(all(feature = "ui-fixture", debug_assertions))]
        if let Some(rows) = &self.scenario_sources {
            return Some(rows.clone());
        }
        let Some(root) = self.create_root() else {
            return Some(std::rc::Rc::new(
                self.sources.iter().map(|path| source_row(path)).collect(),
            ));
        };
        let dir = if self.create_folder.is_empty() {
            root
        } else {
            root.join(&self.create_folder)
        };
        // 프레임마다 디스크를 읽지 않도록 보고 있는 폴더가 바뀔 때만 다시 읽음.
        if self
            .create_listing
            .as_ref()
            .is_none_or(|(path, _)| path != &dir)
        {
            let rows = read_source_dir(&dir);
            self.create_listing = Some((dir, rows));
        }
        self.create_listing
            .as_ref()
            .and_then(|(_, rows)| rows.clone())
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
        self.clear_list_selection();
        self.targets = None;
        self.directories.clear();
        self.start(Start::List { archive });
    }

    fn start(&mut self, request: Start) -> bool {
        // 파일 선택 창이 응답을 기다리는 동안에는 어느 창에서도 새 작업을 시작하지 않음.
        if self.picker.is_some() {
            return false;
        }
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
            validated.push(crate::archive_path::worker_path(&path));
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
        self.create_folder.clear();
        self.create_listing = None;
        self.create_selected = None;
        self.create = true;
    }

    pub fn begin_extract(&mut self) -> bool {
        self.selected_only = !self.model.selected.is_empty();
        let selection = if self.selected_only {
            Selection::EntryIds(
                browser::expand_selection(&self.model.entries, &self.model.selected)
                    .into_iter()
                    .collect(),
            )
        } else {
            Selection::All
        };
        self.extract_selection(selection)
    }

    fn extract_selection(&mut self, selection: Selection) -> bool {
        if !self.model.can_extract()
            || matches!(&selection, Selection::EntryIds(ids) if ids.is_empty())
        {
            return false;
        }
        self.selected_only = matches!(&selection, Selection::EntryIds(_));
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
        if !self.zip_password_ready() {
            self.ui_error = Some("두 암호가 같은지 확인하세요".into());
            return;
        }
        self.targets = Some([self.zip_name.clone()].into_iter().collect());
        self.directories.clear();
        // 암호는 ZIP 만들기 창에서 받아 두었다가 작업자가 요청하면 넘김.
        let password = self.aes256.then(|| std::mem::take(&mut self.zip_password));
        if self.start(Start::CreateZip {
            sources,
            output_file,
            method: self.method,
            aes256: self.aes256,
        }) {
            self.create = false;
            self.create_win = WorkWindow::default();
            self.clear_zip_password();
            self.pending_zip_password = password;
        } else if let Some(password) = password {
            self.zip_password = password;
        }
    }

    fn open_picker(&mut self, purpose: Purpose) {
        if self.model.busy() || self.picker.is_some() {
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
        match self.dialogs.start(request) {
            PendingDialog::Ready(answer) => self.apply_picker(purpose, answer),
            PendingDialog::Waiting(receiver) => self.picker = Some((purpose, receiver)),
        }
    }

    fn poll_picker(&mut self) {
        let Some((purpose, receiver)) = &self.picker else {
            return;
        };
        let purpose = *purpose;
        let answer = match receiver.try_recv() {
            Ok(answer) => answer,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(DIALOG_UNAVAILABLE.into()),
        };
        self.picker = None;
        self.apply_picker(purpose, answer);
    }

    fn apply_picker(&mut self, purpose: Purpose, answer: DialogAnswer) {
        if answer.is_ok() && self.ui_error.as_deref() == Some(DIALOG_UNAVAILABLE) {
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
                    Purpose::Files => self.add_sources(paths),
                    Purpose::Folder => {
                        // 새 폴더로 처음부터 시작함. 고른 폴더가 쓸 수 없으면 기존 내용을 되돌림.
                        let previous = std::mem::take(&mut self.sources);
                        self.add_sources(paths);
                        if self.sources.is_empty() {
                            self.sources = previous;
                        } else {
                            self.aes256 = false;
                            self.clear_zip_password();
                            self.ui_error = None;
                        }
                    }
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
                    // 저장 위치를 고르는 사이 ZIP 만들기 창을 닫았으면 아무 작업도 시작하지 않음.
                    Purpose::SaveZip if !self.create => {}
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
                    if let Some(name) = gzip_output_name {
                        self.gzip_name = name;
                    }
                    self.extract_selection(selection);
                }
                Start::CreateZip { aes256, .. } => {
                    self.create = true;
                    if aes256 {
                        // 암호는 보관하지 않으므로 ZIP 만들기 창에서 다시 받음.
                        self.aes256 = true;
                    } else {
                        self.begin_zip();
                    }
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
        let close_before = self.model.close;
        self.model.poll();
        if close_before == Close::Confirm && self.model.close == Close::Review {
            // 종료 확인 창에 답하기 전에 작업이 끝났음. 사용자가 닫기를 승인하지 않았으므로
            // 바로 닫지 않고 계속 물음(close_dialog가 작업이 끝난 문구로 바꿔 보여 줌).
            self.model.close = Close::Confirm;
        }
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
        self.forget_password_history(&ctx);
        if !self.initialized {
            theme::apply(&ctx);
            self.initialized = true;
        }
        self.poll();
        self.poll_picker();
        if self.model.challenge.is_some_and(|(_, _, encrypt)| encrypt)
            && let Some(password) = self.pending_zip_password.take()
        {
            // ZIP 만들기 창에서 받은 암호를 작업자의 암호 요청에 바로 넘김.
            self.model.password(password);
        }
        if !self.model.busy()
            && let Some(mut pending) = self.pending_zip_password.take()
        {
            wipe(&mut pending);
        }
        let challenge = self.model.challenge.map(|(id, _, _)| id);
        let new_challenge = challenge != self.password_challenge;
        if new_challenge || challenge.is_none() {
            self.clear_password();
        }
        self.password_challenge = challenge;
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        if self.picker.is_none() {
            self.on_drop(dropped);
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.model.request_close();
            // 실행 중인 작업이 없으면 결과 확인 안내 없이 바로 닫음.
            self.model.acknowledge_close();
            if self.model.close != Close::Allowed {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if self.model.close == Close::Review {
            // 작업을 취소하고 닫기로 한 뒤 정리가 끝난 경우도 안내 없이 닫음.
            self.model.acknowledge_close();
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
        let short_title = if let Some(path) = title.strip_suffix(" — Zipelo") {
            format!(
                "{} — Zipelo",
                theme::window_title(root, path, ctx.content_rect().width() - 300.0)
            )
        } else {
            title.clone()
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(short_title));
        if self.custom_frame
            && window_frame::title_bar(
                root,
                "main",
                &title,
                window_frame::Buttons {
                    minimize: true,
                    maximize: true,
                },
            )
        {
            self.release_after_move = true;
        }
        egui::Panel::top("archive-actions")
            .frame(
                theme::panel(theme::PANEL)
                    .corner_radius(0)
                    .inner_margin(egui::Margin {
                        left: 12,
                        right: 12,
                        // 버튼 안 내용은 보이는 모양 기준으로 가운데에 두므로 위아래를 같게 둠.
                        top: 4,
                        bottom: 4,
                    }),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let idle = !self.model.busy()
                        && self.model.close == Close::Open
                        && self.picker.is_none();
                    if theme::tool(ui, "열기", theme::OPEN, idle, false)
                        .on_hover_text(&title)
                        .clicked()
                    {
                        self.open_picker(Purpose::Open);
                    }
                    if theme::tool(ui, "압축하기", theme::CREATE, idle, false).clicked() {
                        self.choose_zip_folder();
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
            .exact_size(theme::STATUS_HEIGHT)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::STRIPE)
                    .inner_margin(egui::Margin::symmetric(16, 0)),
            )
            .show(root, |ui| {
                let rows = browser::rows(&self.model.entries, &self.folder, &self.search);
                let selected = rows
                    .iter()
                    .filter(|row| !row.ids.is_empty() && row.ids.is_subset(&self.model.selected))
                    .count();
                let rect = ui.max_rect();
                let half = (rect.width() - theme::SPACE) / 2.0;
                theme::status_text(
                    ui,
                    rect,
                    &format!("{}개 항목", rows.len()),
                    half,
                    Align::Min,
                );
                theme::status_text(ui, rect, &format!("선택 {selected}개"), half, Align::Max);
            });
        if ctx.embed_viewports() && self.work_visible() && !self.limit_result_visible() {
            // 별도 창을 만들 수 없을 때만 쓰는 대체 배치. 목록을 가리지 않도록 하단에 둠.
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
        self.forget_password_history(&ctx);
        self.work_window(&ctx);
        self.limit_result_dialog(&ctx);
        self.create_window(&ctx);
        if self.model.challenge.is_some() {
            self.password_dialog(&ctx, new_challenge);
        }
        self.resource_dialog(&ctx);
        if self.model.conflict.is_some() && self.model.close == Close::Open {
            self.conflict_dialog(&ctx);
        }
        self.close_dialog(&ctx);
        self.picker_notice(&ctx);
        if self.custom_frame {
            window_frame::edges(&ctx, true);
        }
        if self.model.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }

    // 선택 창이 열려 있는 동안 다른 조작을 막고, 응답을 받도록 화면을 계속 갱신함.
    fn picker_notice(&mut self, ctx: &egui::Context) {
        if self.picker.is_none() {
            return;
        }
        egui::Modal::new(egui::Id::unique("archive-picker")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label("파일 선택 창에서 선택을 마쳐 주세요");
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
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
                            .size(theme::FONT_LARGE)
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
                "ZIP, 7z, TAR, GZ, TAR.GZ, TAR.BZ2, TAR.XZ 파일을 열 수 있습니다",
            );
        });
    }

    fn archive_list(&mut self, ui: &mut egui::Ui) {
        // 경로 줄은 입력란 높이만 차지하고 창 너비 전체를 회색으로 채움.
        let path_row = egui::Frame::new()
            .fill(theme::STRIPE)
            .inner_margin(egui::Margin {
                left: 8,
                right: 12,
                top: 0,
                bottom: 0,
            })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.set_height(theme::FIELD);
                    // 간격은 아래에서 직접 넣어 위치가 흔들리지 않게 함.
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let up = theme::flat_icon_button(
                        ui,
                        "위로",
                        theme::UP,
                        !self.folder.is_empty() && !self.model.busy(),
                        egui::vec2(theme::FIELD, theme::FIELD),
                    );
                    if up.clicked() {
                        let parent = self
                            .folder
                            .rsplit_once('/')
                            .map_or("", |(parent, _)| parent)
                            .to_owned();
                        self.browse_folder(&parent);
                    }
                    ui.add_space(6.0);
                    let path = format!("/{}", self.folder);
                    let search_width = 240.0;
                    let gap = 8.0;
                    let path_width = (ui.available_width() - search_width - gap).max(0.0);
                    // 두 입력란은 같은 높이의 칸을 받아 같은 세로 위치에 그림.
                    let (path_rect, _) = ui.allocate_exact_size(
                        egui::vec2(path_width, theme::FIELD),
                        egui::Sense::hover(),
                    );
                    theme::field_frame(ui, path_rect, false);
                    // 부모 배치에 영향을 주지 않는 하위 영역에 경로 글자를 그림.
                    let mut text_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(path_rect.shrink2(egui::vec2(10.0, 0.0)))
                            .layout(Layout::left_to_right(Align::Center)),
                    );
                    theme::bounded_text(&mut text_ui, &path, (path_width - 20.0).max(0.0));
                    ui.add_space(gap);
                    let (search_rect, _) = ui.allocate_exact_size(
                        egui::vec2(search_width, theme::FIELD),
                        egui::Sense::hover(),
                    );
                    let search_id = egui::Id::unique("archive-search");
                    let focused = ui.memory(|m| m.has_focus(search_id));
                    theme::field_frame(ui, search_rect, focused);
                    let icon = egui::Rect::from_center_size(
                        egui::pos2(search_rect.left() + 20.0, search_rect.center().y),
                        egui::vec2(16.0, 16.0),
                    );
                    theme::svg_icon(ui.painter(), icon, theme::SEARCH, theme::MUTED);
                    let search = ui.put(
                        egui::Rect::from_min_max(
                            egui::pos2(icon.right() + 8.0, search_rect.top() + 1.0),
                            egui::pos2(search_rect.right() - 8.0, search_rect.bottom() - 1.0),
                        ),
                        egui::TextEdit::singleline(&mut self.search)
                            .id(search_id)
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::ZERO)
                            .align(egui::Align2::LEFT_CENTER)
                            .hint_text("파일 검색"),
                    );
                    if search.changed() {
                        self.clear_list_selection();
                    }
                });
            })
            .response
            .rect;
        // 경로 줄 아래 한 줄이 목록과의 경계이자 입력란의 아래 테두리 역할을 함.
        ui.painter().hline(
            path_row.x_range(),
            path_row.bottom() - 0.5,
            egui::Stroke::new(1.0, theme::BORDER),
        );
        let rows = browser::rows(&self.model.entries, &self.folder, &self.search);

        // 열 제목 줄 없이 바로 목록을 보여 줌. 이름과 크기는 내용으로 구분됨.
        let name_width = (ui.available_width() - 16.0 - 28.0 - 96.0 - 40.0).max(120.0);
        let active = !self.model.busy()
            && !self.create
            && self.model.challenge.is_none()
            && self.model.close == Close::Open;
        let list_id = egui::Id::unique("archive-list");
        let background = ui.interact(
            ui.available_rect_before_wrap(),
            list_id,
            if active {
                egui::Sense::click_and_drag()
            } else {
                egui::Sense::hover()
            },
        );
        background
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListBox, active, "파일 목록"));
        if background.has_focus() {
            ui.ctx().memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    list_id,
                    egui::EventFilter {
                        vertical_arrows: true,
                        escape: true,
                        ..Default::default()
                    },
                )
            });
        }
        let mut focused = None;
        let mut open = None;
        let mut next_offset = None;
        let output = egui::ScrollArea::vertical()
            .id_salt("archive-rows")
            .auto_shrink([false, false])
            .vertical_scroll_offset(self.list_offset)
            .show_viewport(ui, |ui, viewport| {
                let origin = ui.max_rect().min;
                let clip = ui.clip_rect();
                // 행을 누르는 영역은 오른쪽 24px을 비워 둠. 목록이 길어도 그 빈 곳에서
                // 끌어서 선택을 시작할 수 있음. 행 배경(선택, 마우스 올림)은 너비를 꽉 채움.
                let full_width = ui.available_width();
                let width = (full_width - 24.0).max(0.0);
                ui.set_min_height((rows.len() as f32 * LIST_ROW).max(viewport.height()));
                let point = ui.input(|i| i.pointer.interact_pos());
                if active && let Some(point) = point {
                    let press = ui.input(|i| i.pointer.press_origin()).unwrap_or(point);
                    let relative = press - origin;
                    let in_row = relative.x >= 0.0
                        && relative.x <= width
                        && relative.y >= 0.0
                        && relative.y < rows.len() as f32 * LIST_ROW;
                    if clip.contains(press) && !in_row && ui.input(|i| i.pointer.primary_pressed())
                    {
                        background.request_focus();
                        let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
                        self.marquee = Some(Marquee {
                            origin: relative.to_pos2(),
                            base: if ctrl {
                                self.model.selected.clone()
                            } else {
                                BTreeSet::new()
                            },
                            moved: false,
                        });
                        if !ctrl {
                            self.model.selected.clear();
                            self.list_anchor = None;
                            self.list_cursor = None;
                        }
                    }
                    if let Some(drag) = &mut self.marquee {
                        let end = (point.clamp(clip.min, clip.max) - origin).to_pos2();
                        drag.moved |= drag.origin.distance(end) > 3.0;
                        if drag.moved {
                            let rect = egui::Rect::from_two_pos(drag.origin, end);
                            self.model.selected = drag.base.clone();
                            if rect.left() < width && rect.right() > 0.0 {
                                let first = (rect.top().max(0.0) / LIST_ROW).floor() as usize;
                                let last = ((rect.bottom().max(0.0) / LIST_ROW).ceil() as usize)
                                    .min(rows.len());
                                for row in rows.iter().take(last).skip(first) {
                                    self.model.selected.extend(&row.ids);
                                }
                                if first < last {
                                    self.list_anchor = Some(first);
                                    self.list_cursor = Some(last - 1);
                                }
                            }
                            let speed = if point.y > clip.bottom() - 24.0 {
                                ((point.y - clip.bottom() + 24.0) / 24.0).clamp(0.0, 3.0)
                            } else if point.y < clip.top() + 24.0 {
                                -((clip.top() + 24.0 - point.y) / 24.0).clamp(0.0, 3.0)
                            } else {
                                0.0
                            };
                            if ui.input(|i| i.pointer.primary_down()) && speed != 0.0 {
                                let dt = ui.input(|i| i.stable_dt).clamp(0.001, 0.05);
                                next_offset = Some((viewport.min.y + speed * 500.0 * dt).clamp(
                                    0.0,
                                    (rows.len() as f32 * LIST_ROW - viewport.height()).max(0.0),
                                ));
                                ui.ctx().request_repaint();
                            }
                        }
                    }
                }
                let first = (viewport.min.y / LIST_ROW).floor() as usize;
                let last = ((viewport.max.y / LIST_ROW).ceil() as usize + 1).min(rows.len());
                for index in first.min(rows.len())..last {
                    let row = &rows[index];
                    let rect = egui::Rect::from_min_size(
                        origin + egui::vec2(0.0, index as f32 * LIST_ROW),
                        egui::vec2(width, LIST_ROW),
                    );
                    let selected = !row.ids.is_empty() && row.ids.is_subset(&self.model.selected);
                    let response = ui
                        .interact(
                            rect,
                            egui::Id::unique(("archive-row", &row.path)),
                            if active {
                                egui::Sense::click()
                            } else {
                                egui::Sense::hover()
                            },
                        )
                        .on_hover_text(&row.name);
                    theme::pointer(&response);
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::Role::ListBoxOption,
                            active,
                            selected,
                            &row.name,
                        )
                    });
                    if response.has_focus() {
                        focused = Some(index);
                        ui.ctx().memory_mut(|memory| {
                            memory.set_focus_lock_filter(
                                response.id,
                                egui::EventFilter {
                                    vertical_arrows: true,
                                    escape: true,
                                    ..Default::default()
                                },
                            )
                        });
                    }
                    theme::row_background(
                        ui,
                        egui::Rect::from_min_size(rect.min, egui::vec2(full_width, LIST_ROW)),
                        selected,
                        response.hovered(),
                    );
                    if response.clicked() {
                        background.request_focus();
                        let modifiers = ui.input(|i| i.modifiers);
                        self.select_row(
                            &rows,
                            index,
                            modifiers.ctrl || modifiers.command,
                            modifiers.shift,
                        );
                    }
                    if row.directory && response.double_clicked() {
                        open = Some(row.path.clone());
                    }
                    // 암호가 걸린 항목은 이름 앞에 *를 붙이고, 툴팁은 실제 이름을 보여 줌.
                    let name = if row.encrypted {
                        format!("*{}", row.name)
                    } else {
                        row.name.clone()
                    };
                    Self::entry_cells(
                        ui,
                        rect,
                        row.directory,
                        &name,
                        &row.name,
                        if row.directory {
                            "0 B".into()
                        } else {
                            row.bytes
                                .map(present::bytes_text)
                                .unwrap_or_else(|| "—".into())
                        },
                        name_width,
                    );
                    if active
                        && self.list_cursor == Some(index)
                        && (response.has_focus() || background.has_focus())
                    {
                        ui.painter().rect_stroke(
                            egui::Rect::from_min_size(rect.min, egui::vec2(full_width, LIST_ROW))
                                .shrink(1.0),
                            0,
                            egui::Stroke::new(1.0, theme::BLUE),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
                if rows.is_empty() {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(clip), |ui| {
                        ui.centered_and_justified(|ui| {
                            ui.label(if self.search.is_empty() {
                                "이 폴더는 비어 있습니다"
                            } else {
                                "일치하는 항목이 없습니다"
                            })
                        });
                    });
                }
                #[cfg(all(feature = "ui-fixture", debug_assertions))]
                if let Some(rect) = self.scenario_marquee {
                    ui.painter().with_clip_rect(clip).rect(
                        rect.translate(origin.to_vec2()),
                        0,
                        theme::BLUE.gamma_multiply(0.16),
                        egui::Stroke::new(1.0, theme::BLUE),
                        egui::StrokeKind::Inside,
                    );
                }
                if let Some(drag) = &self.marquee
                    && drag.moved
                    && let Some(point) = point
                {
                    let rect = egui::Rect::from_two_pos(
                        origin + drag.origin.to_vec2(),
                        point.clamp(clip.min, clip.max),
                    );
                    ui.painter().with_clip_rect(clip).rect(
                        rect,
                        0,
                        theme::BLUE.gamma_multiply(0.16),
                        egui::Stroke::new(1.0, theme::BLUE),
                        egui::StrokeKind::Inside,
                    );
                }
            });
        self.list_offset = next_offset.unwrap_or(output.state.clamped_offset().y);
        if ui.input(|i| i.pointer.primary_released() || !i.pointer.primary_down()) {
            self.marquee = None;
        }
        let escape_from_list = self.list_focused
            && ui.ctx().memory(|memory| memory.focused()).is_none()
            && ui.input(|i| i.key_pressed(egui::Key::Escape));
        self.list_focused =
            active && (background.has_focus() || focused.is_some() || escape_from_list);
        if self.list_focused {
            if let Some(index) = focused {
                self.list_cursor = Some(index);
            }
            let modifiers = ui.input(|i| i.modifiers);
            let ctrl = modifiers.ctrl || modifiers.command;
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.model.selected.clear();
                self.marquee = None;
                background.request_focus();
            }
            if ctrl && ui.input(|i| i.key_pressed(egui::Key::A)) {
                self.model.selected = rows
                    .iter()
                    .flat_map(|row| row.ids.iter().copied())
                    .collect();
                self.list_cursor = (!rows.is_empty()).then_some(0);
                self.list_anchor = self.list_cursor;
            }
            if !rows.is_empty() {
                let direction = if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    Some(1isize)
                } else if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    Some(-1)
                } else {
                    None
                };
                if let Some(direction) = direction {
                    let index = self.list_cursor.map_or(0, |index| {
                        index.saturating_add_signed(direction).min(rows.len() - 1)
                    });
                    if ctrl && !modifiers.shift {
                        self.list_cursor = Some(index);
                    } else {
                        self.select_row(&rows, index, ctrl, modifiers.shift);
                    }
                    background.request_focus();
                    let top = index as f32 * LIST_ROW;
                    if top < self.list_offset {
                        self.list_offset = top;
                    } else if top + LIST_ROW > self.list_offset + output.inner_rect.height() {
                        self.list_offset = top + LIST_ROW - output.inner_rect.height();
                    }
                    ui.ctx().request_repaint();
                }
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let index = self
                        .list_cursor
                        .or(focused)
                        .unwrap_or(0)
                        .min(rows.len() - 1);
                    if rows[index].directory {
                        open = Some(rows[index].path.clone());
                    }
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Backspace)) && !self.folder.is_empty() {
                open = Some(
                    self.folder
                        .rsplit_once('/')
                        .map_or("", |(parent, _)| parent)
                        .to_owned(),
                );
            }
        }
        if let Some(folder) = open {
            self.browse_folder(&folder);
            background.request_focus();
        }
    }

    fn clear_list_selection(&mut self) {
        self.model.selected.clear();
        self.list_anchor = None;
        self.list_cursor = None;
        self.list_offset = 0.0;
        self.marquee = None;
    }

    fn select_row(&mut self, rows: &[browser::Row], index: usize, ctrl: bool, shift: bool) {
        if shift {
            let anchor = self
                .list_anchor
                .unwrap_or(self.list_cursor.unwrap_or(index))
                .min(rows.len() - 1);
            if !ctrl {
                self.model.selected.clear();
            }
            for row in &rows[anchor.min(index)..=anchor.max(index)] {
                self.model.selected.extend(&row.ids);
            }
            self.list_anchor = Some(anchor);
        } else {
            if !ctrl {
                self.model.selected.clear();
            }
            if ctrl && rows[index].ids.is_subset(&self.model.selected) {
                // 폴더 이동 시 선택을 지우므로 현재 행의 ID만 제거함.
                self.model
                    .selected
                    .retain(|id| !rows[index].ids.contains(id));
            } else {
                self.model.selected.extend(&rows[index].ids);
            }
            self.list_anchor = Some(index);
        }
        self.list_cursor = Some(index);
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

    fn limit_result_visible(&self) -> bool {
        self.result_visible()
            && self
                .model
                .terminal
                .as_ref()
                .is_some_and(|terminal| terminal.error == Some(ErrorCode::Limit))
    }

    fn limit_result_dialog(&mut self, ctx: &egui::Context) {
        if !self.limit_result_visible() || self.main_modal_open() {
            return;
        }
        egui::Modal::new(egui::Id::new("archive-limit-result")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).min(WORK_WIDTH));
            ui.heading("작업 결과");
            self.work_area(ui);
        });
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

    fn work_visible(&self) -> bool {
        self.model.busy() || self.result_visible() || (self.ui_error.is_some() && !self.create)
    }

    /// 메인 창 가운데에 모달(암호 입력, 종료 확인, 파일 선택 안내)이나 같은 자리에 뜨는
    /// ZIP 만들기 창이 떠 있는지. 닫기가 정해진 뒤(Allowed)에도 결과 창을 다시 띄우지 않음.
    /// 모달이 늘어도 진행·결과 창 숨김 규칙은 이 한 곳만 보면 되도록 모음.
    fn main_modal_open(&self) -> bool {
        (self.create && !self.model.busy())
            || self.picker.is_some()
            || self.model.challenge.is_some()
            || self.model.resource.is_some()
            || self.model.conflict.is_some()
            || self.model.close != Close::Open
    }

    fn work_window(&mut self, ctx: &egui::Context) {
        if !self.work_visible() {
            self.work = WorkWindow::default();
            return;
        }
        if self.limit_result_visible() {
            self.work.open = false;
            return;
        }
        if ctx.embed_viewports() {
            // 별도 창을 만들 수 없는 환경(시험 등)은 show()에서 하단에 그림.
            return;
        }
        if self.main_modal_open() {
            // 메인 창 가운데에 뜨는 모달을 항상 위에 뜨는 진행·결과 창이 가리지 않게
            // 그동안 창을 닫음. 부모 기준 위치는 기억해 두고 다시 엶.
            self.work.open = false;
            return;
        }
        let id = egui::ViewportId::from_hash_of("archive-work");
        let title = if self.model.busy() {
            present::activity_text(
                &self.model,
                matches!(self.last_request, Some(Start::CreateZip { .. })),
            )
            .to_owned()
        } else {
            "작업 결과".to_owned()
        };
        let window_title = format!("{title} — Zipelo");
        let custom_frame = self.custom_frame;
        let size = egui::vec2(WORK_WIDTH, self.work.height);
        let builder = self.work.builder(
            ctx,
            id,
            size,
            &window_title,
            !custom_frame,
            self.picker.is_some(),
        );
        let height = ctx.show_viewport_immediate(id, builder, |ui, class| {
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                if self.model.busy() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                } else {
                    self.dismissed = true;
                    self.ui_error = None;
                    self.model.input_error = None;
                }
            }
            if class == egui::ViewportClass::EmbeddedWindow {
                ui.set_width(WORK_WIDTH);
                self.work_area(ui);
                return None;
            }
            let mut chrome = 0.0;
            if custom_frame {
                window_frame::title_bar(
                    ui,
                    "work",
                    &window_title,
                    window_frame::Buttons {
                        minimize: false,
                        maximize: false,
                    },
                );
                window_frame::edges(ui.ctx(), false);
                chrome = window_frame::TITLE_HEIGHT;
            }
            let margin = theme::PAD as f32;
            egui::CentralPanel::default()
                .frame(theme::panel(theme::PANEL).corner_radius(0))
                .show(ui, |ui| {
                    let content = ui.vertical(|ui| self.work_area(ui)).response.rect;
                    Some(content.height() + 2.0 * margin + chrome)
                })
                .inner
        });
        if let Some(height) = height {
            let height = height.clamp(WORK_MIN_HEIGHT, WORK_MAX_HEIGHT).ceil();
            if (height - self.work.height).abs() > 1.0 {
                self.work.height = height;
            }
        }
    }

    fn work_area(&mut self, ui: &mut egui::Ui) {
        // 선택 창 안내 모달은 메인 창만 막으므로, 별도 진행 창의 버튼은 여기서 막음.
        let enabled = self.picker.is_none();
        ui.add_enabled_ui(enabled, |ui| self.work_contents(ui));
    }

    fn work_contents(&mut self, ui: &mut egui::Ui) {
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
                    if matches!(self.last_request, Some(Start::CreateZip { .. })) {
                        "저장 위치와 추가한 항목을 확인하세요"
                    } else {
                        "풀 위치와 선택한 항목을 확인하세요"
                    }
                },
            );
        }
    }

    fn progress(&mut self, ui: &mut egui::Ui) {
        if self.model.resource.is_some() {
            ui.horizontal(|ui| {
                ui.label("사용자 허용을 기다리고 있습니다");
                self.cancel_action(ui, false);
            });
            return;
        }
        let creating = matches!(self.last_request, Some(Start::CreateZip { .. }));
        let cancelling = self.model.cancellation_requested() || self.model.cancel.is_some();
        let title = present::activity_text(&self.model, creating);
        ui.strong(title);
        if let Some((stage, done, total, count)) = self.model.progress {
            // 원본을 읽는 단계에서는 아직 처리한 항목이 없으므로 개수를 표시하지 않음.
            let processed = if stage == Stage::Read {
                String::new()
            } else {
                format!("{count}개 처리함, ")
            };
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
                        "{:.0}% / {processed}{} / {}",
                        (done as f64 / total as f64).clamp(0.0, 1.0) * 100.0,
                        present::bytes_text(done),
                        present::bytes_text(total)
                    ));
                    self.cancel_action(ui, cancelling);
                });
            } else {
                theme::indeterminate(ui);
                ui.horizontal(|ui| {
                    ui.label(format!("{processed}{}", present::bytes_text(done)));
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
        let resolved_targets = present::resolved_targets(
            self.targets.as_ref(),
            &self.model.entries,
            matches!(
                self.last_request,
                Some(Start::Extract {
                    gzip_output_name: Some(_),
                    ..
                })
            )
            .then_some(self.gzip_name.as_str()),
            &self.model.resolutions,
        );
        let skipped = self
            .model
            .resolutions
            .iter()
            .filter(|r| r.choice == ConflictChoice::Skip)
            .count();
        let created_names = self
            .model
            .outputs
            .iter()
            .filter(|(_, disposition)| *disposition == Disposition::Created)
            .map(|(name, _)| name.replace('\\', "/"))
            .collect::<BTreeSet<_>>();
        let warnings = if listing {
            Vec::new()
        } else {
            present::publication_warnings(
                t.as_ref(),
                self.model.confirmed,
                resolved_targets.as_ref(),
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
            } else if skipped > 0 && t.as_ref().is_some_and(|t| t.created_count == 0) {
                "모든 파일을 건너뛰었습니다"
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
        let mut expand = false;
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
            let max_height = WORK_RESULT_MAX_HEIGHT;
            egui::ScrollArea::vertical()
                .id_salt("result-message")
                .max_height(max_height)
                .show(ui, |ui| {
                    if success && t.as_ref().is_some_and(|t| t.created_count > 0) {
                        let path = match self.last_request.as_ref() {
                            Some(Start::CreateZip { output_file, .. }) => {
                                output_file.display().to_string()
                            }
                            _ => format!("{} 폴더에 저장했습니다.", self.destination.display()),
                        };
                        theme::bounded_text(ui, &path, ui.available_width());
                    } else if let Some(code) = t.as_ref().and_then(|t| t.error) {
                        ui.label(if creating && code == ErrorCode::SourceChanged {
                            "묶을 파일이 바뀌었습니다. 파일을 확인한 뒤 다시 만드세요".to_owned()
                        } else {
                            present::operation_text(present::error_text(code), creating)
                        });
                    } else if let Some(error) = &self.model.error {
                        ui.label(present::operation_text(
                            present::client_error_text(error),
                            creating,
                        ));
                    }
                    if skipped > 0 {
                        ui.label(format!("건너뛴 파일 {skipped}개"));
                    }
                    for resolution in self.model.resolutions.iter().filter(|r| {
                        r.choice == ConflictChoice::Rename
                            && created_names.contains(&r.relative_name.replace('\\', "/"))
                    }) {
                        theme::bounded_text(
                            ui,
                            &format!("이름 바꿔 저장: {}", resolution.relative_name),
                            ui.available_width(),
                        );
                    }
                    for warning in warnings
                        .iter()
                        .copied()
                        .chain(durability_warning.filter(|_| !durability_in_details))
                    {
                        ui.horizontal_wrapped(|ui| {
                            theme::icon(ui, Icon::Warning, 20.0);
                            ui.colored_label(
                                theme::WARNING,
                                present::operation_text(warning, creating),
                            );
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
                        ui.label(if creating {
                            "ZIP 파일을 만들지 않았습니다"
                        } else {
                            "아무 파일도 풀지 않았습니다"
                        });
                    }
                    if t.as_ref().is_some_and(|t| {
                        t.error == Some(ErrorCode::Limit)
                            && (t.created_count > 0
                                || t.retained_partial_count > 0
                                || t.publication != Publication::None)
                    }) {
                        ui.label(format!(
                            "{}개 파일이 저장되었거나 일부 남아 있습니다. 저장 위치를 확인하세요.",
                            t.as_ref().unwrap().created_count
                        ));
                        theme::bounded_text(
                            ui,
                            &self.destination.display().to_string(),
                            ui.available_width(),
                        );
                    }
                    if self.details {
                        theme::panel(theme::STRIPE).show(ui, |ui| {
                            if durability_in_details {
                                if let Some(warning) = durability_warning {
                                    ui.label(present::operation_text(warning, creating));
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
                                theme::bounded_text(
                                    ui,
                                    &format!("{name}: {}", disposition_label(*state)),
                                    ui.available_width(),
                                );
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
                theme::pointer(&response);
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
                } else if self.model.can_retry_expanded() {
                    None
                } else if t
                    .as_ref()
                    .is_some_and(|t| t.error == Some(ErrorCode::Limit))
                {
                    None
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
            if self.model.can_retry_expanded() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    expand = theme::action(ui, "한도를 늘려 다시 시도", true, true).clicked();
                });
            }
        });
        if choose {
            self.open_picker(Purpose::Retry);
        }
        if retry {
            self.retry();
        }
        if expand {
            let ready = if matches!(self.last_request, Some(Start::Extract { .. })) {
                match present::prepare_destination(&self.destination, self.automatic_destination) {
                    Ok(created) => {
                        self.owned_destination = created.then(|| self.destination.clone());
                        true
                    }
                    Err(_) => {
                        self.ui_error = Some(present::FOLDER_ERROR.into());
                        false
                    }
                }
            } else {
                true
            };
            if ready && self.model.retry_expanded(self.factory.as_mut()) {
                self.dismissed = false;
                self.details = false;
                self.ui_error = None;
            }
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

    /// 파일 목록 한 행의 아이콘, 이름, 크기 칸. 메인 목록과 ZIP 만들기 창이 같이 씀.
    fn entry_cells(
        ui: &mut egui::Ui,
        rect: egui::Rect,
        directory: bool,
        name: &str,
        hover: &str,
        size: String,
        name_width: f32,
    ) {
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(0.0, 2.0)))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                ui.add_space(16.0);
                theme::icon(ui, if directory { Icon::Folder } else { Icon::File }, 20.0);
                theme::bounded_text_hover(ui, name, hover, name_width);
                theme::column_label(ui, size, 96.0, Align::Max);
            },
        );
    }

    /// ZIP 만들기를 진행 창처럼 부모 창을 따라가는 별도 창으로 띄움.
    /// 별도 창을 만들 수 없는 환경(시험 등)에서는 메인 창 안의 모달로 띄움.
    fn create_window(&mut self, ctx: &egui::Context) {
        if !self.create || self.model.busy() {
            self.create_win = WorkWindow::default();
            return;
        }
        if ctx.embed_viewports() {
            let mut close = false;
            let modal = egui::Modal::new(egui::Id::unique("create-zip")).show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 64.0).min(CREATE_WIDTH));
                ui.horizontal(|ui| {
                    ui.heading("ZIP 만들기");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let button = theme::icon_button(ui, "닫기", theme::CLOSE, true);
                        close = button.clicked();
                    });
                });
                close |= self.create_area(ui);
                ui.add_space(theme::SPACE);
                close |= self.create_actions(ui);
            });
            if close || modal.should_close() {
                self.close_create();
            }
            return;
        }
        let id = egui::ViewportId::from_hash_of("archive-create");
        let title = "ZIP 만들기 — Zipelo";
        let custom_frame = self.custom_frame;
        if !self.create_win.open {
            self.create_win.height = self.create_win.height.max(CREATE_MIN_HEIGHT);
        }
        let size = egui::vec2(CREATE_WIDTH, self.create_win.height);
        let builder =
            self.create_win
                .builder(ctx, id, size, title, !custom_frame, self.picker.is_some());
        let mut close = false;
        let height = ctx.show_viewport_immediate(id, builder, |ui, class| {
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                close = true;
            }
            if class == egui::ViewportClass::EmbeddedWindow {
                ui.set_width(CREATE_WIDTH);
                close |= self.create_area(ui);
                close |= self.create_actions(ui);
                return None;
            }
            let mut chrome = 0.0;
            if custom_frame {
                window_frame::title_bar(
                    ui,
                    "create",
                    title,
                    window_frame::Buttons {
                        minimize: false,
                        maximize: false,
                    },
                );
                window_frame::edges(ui.ctx(), false);
                chrome = window_frame::TITLE_HEIGHT;
            }
            // 버튼은 창 오른쪽 아래에 여백을 두고 고정함. 위 내용이 바뀌어도 자리가 같음.
            let actions = egui::Panel::bottom(egui::Id::unique("create-actions"))
                .resizable(false)
                .show_separator_line(false)
                .frame(
                    egui::Frame::new()
                        .fill(theme::PANEL)
                        .inner_margin(egui::Margin {
                            left: theme::PAD,
                            right: theme::PAD,
                            top: 0,
                            bottom: theme::PAD,
                        }),
                )
                .show(ui, |ui| close |= self.create_actions(ui))
                .response
                .rect
                .height();
            let margin = theme::PAD as f32;
            egui::CentralPanel::default()
                .frame(theme::panel(theme::PANEL).corner_radius(0))
                .show(ui, |ui| {
                    let content = ui
                        .vertical(|ui| close |= self.create_area(ui))
                        .response
                        .rect;
                    Some(content.height() + 2.0 * margin + chrome + actions)
                })
                .inner
        });
        if let Some(height) = height {
            let height = height.clamp(CREATE_MIN_HEIGHT, 900.0).ceil();
            if (height - self.create_win.height).abs() > 1.0 {
                self.create_win.height = height;
            }
        }
        if close {
            self.close_create();
        }
    }

    /// ZIP 만들기 내용. 닫기를 눌렀으면 true를 돌려줌.
    fn create_area(&mut self, ui: &mut egui::Ui) -> bool {
        // 저장 위치 선택 창이 응답을 기다리는 동안에는 버튼을 막음.
        let enabled = self.picker.is_none();
        ui.add_enabled_ui(enabled, |ui| self.create_contents(ui))
            .inner
    }

    fn create_contents(&mut self, ui: &mut egui::Ui) -> bool {
        let rows = self.create_rows();
        let root = self.create_root();

        // 경로 줄: 메인 화면처럼 위로 버튼과 현재 폴더 경로.
        ui.horizontal(|ui| {
            ui.set_height(theme::FIELD);
            ui.spacing_mut().item_spacing.x = 0.0;
            let up = theme::flat_icon_button(
                ui,
                "위로",
                theme::UP,
                !self.create_folder.is_empty(),
                egui::vec2(theme::FIELD, theme::FIELD),
            );
            if up.clicked() {
                self.create_folder = self
                    .create_folder
                    .rsplit_once('/')
                    .map_or("", |(parent, _)| parent)
                    .to_owned();
                self.create_selected = None;
            }
            ui.add_space(6.0);
            let path = match &root {
                Some(root) => {
                    let name = root.file_name().unwrap_or_default().to_string_lossy();
                    if self.create_folder.is_empty() {
                        format!("/{name}")
                    } else {
                        format!("/{name}/{}", self.create_folder)
                    }
                }
                None => "/".to_owned(),
            };
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), theme::FIELD),
                egui::Sense::hover(),
            );
            theme::boxed_field(ui, rect);
            theme::field_text(ui, rect, &path);
        });
        ui.add_space(theme::SPACE);

        // 파일 목록: 메인 화면 목록과 같은 행 모양. 폴더를 두 번 누르면 들어감.
        let mut enter = None;
        egui::Frame::new()
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .show(ui, |ui| {
                ui.set_min_size(egui::vec2(ui.available_width(), CREATE_LIST_HEIGHT));
                ui.set_max_height(CREATE_LIST_HEIGHT);
                let message = match &rows {
                    None => Some("폴더를 읽지 못했습니다"),
                    Some(rows) if rows.is_empty() => Some("이 폴더는 비어 있습니다"),
                    Some(_) => None,
                };
                if let Some(message) = message {
                    ui.centered_and_justified(|ui| ui.label(message));
                    return;
                }
                let rows = rows.unwrap();
                let name_width = (ui.available_width() - 16.0 - 28.0 - 96.0 - 24.0).max(80.0);
                egui::ScrollArea::vertical()
                    .id_salt("create-rows")
                    .auto_shrink([false, false])
                    .max_height(CREATE_LIST_HEIGHT)
                    .show_rows(ui, LIST_ROW, rows.len(), |ui, range| {
                        for row in &rows[range] {
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), LIST_ROW),
                                egui::Sense::click(),
                            );
                            let response = response.on_hover_text(&row.name);
                            theme::pointer(&response);
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::Role::ListBoxOption,
                                    true,
                                    &row.name,
                                )
                            });
                            if response.clicked() {
                                self.create_selected = Some(row.name.clone());
                            }
                            theme::row_background(
                                ui,
                                rect,
                                self.create_selected.as_deref() == Some(row.name.as_str()),
                                response.hovered(),
                            );
                            if row.directory && root.is_some() && response.clicked() {
                                // egui가 한 프레임을 다시 그리면 두 번 누름 정보가 사라질 수 있어
                                // 같은 폴더를 다른 프레임에서 0.4초 안에 다시 누르면 두 번 누름으로 봄.
                                let frame = ui.ctx().cumulative_frame_nr();
                                let time = ui.input(|i| i.time);
                                match &self.create_last_click {
                                    Some((name, last_frame, last_time))
                                        if name == &row.name
                                            && *last_frame != frame
                                            && time - last_time < 0.4 =>
                                    {
                                        enter = Some(row.name.clone());
                                        self.create_last_click = None;
                                    }
                                    Some((name, last_frame, _))
                                        if name == &row.name && *last_frame == frame => {}
                                    _ => {
                                        self.create_last_click =
                                            Some((row.name.clone(), frame, time));
                                    }
                                }
                            }
                            Self::entry_cells(
                                ui,
                                rect,
                                row.directory,
                                &row.name,
                                &row.name,
                                row.bytes.map(present::bytes_text).unwrap_or_default(),
                                name_width,
                            );
                        }
                    });
            });
        if let Some(name) = enter {
            self.create_folder = if self.create_folder.is_empty() {
                name
            } else {
                format!("{}/{name}", self.create_folder)
            };
            self.create_selected = None;
        }
        ui.add_space(theme::SPACE);

        if let Some(error) = &self.ui_error {
            Self::error_notice(ui, error);
        }

        // 암호: 체크하면 바로 아래에 암호와 암호 확인 칸을 보여 줌.
        let check = ui.checkbox(&mut self.aes256, "암호로 보호하기");
        theme::pointer(&check);
        theme::focus(&check);
        if self.aes256 {
            let label_width = 72.0;
            ui.horizontal(|ui| {
                ui.add_sized([label_width, theme::FIELD], egui::Label::new("암호"));
                let toggle = if self.zip_password_visible {
                    "숨기기"
                } else {
                    "보기"
                };
                let toggle_width = 72.0;
                let edit = ui.add_sized(
                    [
                        (ui.available_width() - toggle_width - theme::SPACE).max(80.0),
                        theme::FIELD,
                    ],
                    egui::TextEdit::singleline(&mut self.zip_password)
                        .password(!self.zip_password_visible)
                        .id(egui::Id::unique("zip-password-input"))
                        .char_limit(1024)
                        .align(egui::Align2::LEFT_CENTER),
                );
                theme::focus(&edit);
                if theme::action(ui, toggle, false, true).clicked() {
                    self.zip_password_visible = !self.zip_password_visible;
                }
            });
            ui.horizontal(|ui| {
                ui.add_sized([label_width, theme::FIELD], egui::Label::new("암호 확인"));
                let edit = ui.add_sized(
                    [
                        (ui.available_width() - 72.0 - theme::SPACE).max(80.0),
                        theme::FIELD,
                    ],
                    egui::TextEdit::singleline(&mut self.zip_password_confirm)
                        .password(!self.zip_password_visible)
                        .id(egui::Id::unique("zip-password-confirm"))
                        .char_limit(1024)
                        .align(egui::Align2::LEFT_CENTER),
                );
                theme::focus(&edit);
            });
            if self.zip_password.len() > 1024 {
                ui.colored_label(theme::ERROR, "암호는 UTF-8 1~1024바이트로 입력하세요");
            } else if !self.zip_password_confirm.is_empty()
                && self.zip_password != self.zip_password_confirm
            {
                ui.colored_label(theme::ERROR, "두 암호가 같지 않습니다");
            }
            ui.label(RichText::new("암호를 잊으면 파일을 열 수 없습니다.").color(theme::MUTED));
        } else if check.changed()
            || !self.zip_password.is_empty()
            || !self.zip_password_confirm.is_empty()
        {
            // 체크를 해제하면 입력한 암호와 그 실행 취소 기록을 남겨 두지 않음.
            // 입력했다가 모두 지운 경우도 기록에는 남아 있으므로 해제한 순간 함께 지움.
            self.clear_zip_password();
        }
        false
    }

    /// 취소와 압축하기 버튼. 취소를 눌렀으면 true.
    fn create_actions(&mut self, ui: &mut egui::Ui) -> bool {
        let ready = !self.sources.is_empty() && self.zip_password_ready();
        let enabled = self.picker.is_none();
        let (cancel, compress) = ui
            .add_enabled_ui(enabled, |ui| {
                theme::action_pair(ui, "취소", Some(("압축하기", ready)))
            })
            .inner;
        if compress {
            self.open_picker(Purpose::SaveZip);
        }
        cancel
    }

    fn resource_dialog(&mut self, ctx: &egui::Context) {
        let Some(question) = self.model.resource.clone() else {
            return;
        };
        let Some(job) = self.model.job_id() else {
            return;
        };
        let mut allow = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::unique(("archive-resource", job, question.id)))
            .show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 64.0).min(460.0));
                ui.heading(if question.scope.ratio {
                    "압축률이 매우 높습니다"
                } else {
                    "큰 작업을 허용할까요?"
                });
                ui.label(match question.action {
                    ActionKind::List => "압축파일 목록 열기",
                    ActionKind::Extract => "압축 해제",
                    ActionKind::CreateZip => "ZIP 만들기",
                });
                if question.scope.ratio {
                    ui.label("작은 압축파일도 많은 저장 공간을 사용할 수 있습니다.");
                }
                if question.scope.output {
                    ui.label(format!(
                        "총량 한도: {}에서 {}로",
                        present::bytes_text(question.soft_limits.output),
                        present::bytes_text(question.ceiling.output)
                    ));
                }
                if question.scope.entries {
                    ui.label(format!(
                        "항목 한도: {}개에서 {}개로",
                        question.soft_limits.entries, question.ceiling.entries
                    ));
                }
                let mut u = question.usage;
                let estimated = self.model.estimated_usage();
                u.estimated_bytes = u.estimated_bytes.or(estimated.estimated_bytes);
                u.estimated_entries = u.estimated_entries.or(estimated.estimated_entries);
                let bytes = |value: Option<u64>| {
                    value
                        .map(present::bytes_text)
                        .unwrap_or_else(|| "미확인".into())
                };
                let entries = |value: Option<u32>| {
                    value
                        .map(|n| format!("{n}개"))
                        .unwrap_or_else(|| "미확인".into())
                };
                ui.label(format!(
                    "예상 총량: {} / 실제 총량: {}",
                    bytes(u.estimated_bytes),
                    bytes(u.actual_bytes)
                ));
                ui.label(format!(
                    "예상 항목: {} / 실제 항목: {}",
                    entries(u.estimated_entries),
                    entries(u.actual_entries)
                ));
                if question.scope.ratio {
                    ui.label(format!(
                        "압축 크기: {} / 풀린 크기: {}",
                        bytes(u.compressed_bytes),
                        bytes(u.ratio_output_bytes)
                    ));
                }
                ui.label(format!(
                    "확인한 저장 볼륨의 여유 공간: {}",
                    bytes(u.available_bytes)
                ));
                ui.label(format!(
                    "이 작업의 상한: {} / {}개",
                    present::bytes_text(question.ceiling.output),
                    question.ceiling.entries
                ));
                ui.label("이 작업에만 적용합니다. 공간 부족이나 자원 상한에 도달하면 중단합니다.");
                ui.horizontal(|ui| {
                    cancel = theme::action(ui, "취소", false, true).clicked();
                    allow = theme::action(ui, "이 작업 허용", true, true).clicked();
                });
            });
        if cancel || modal.should_close() {
            self.clear_password();
            self.model.cancel();
        } else if allow {
            self.model.resource_reply(job, question.id, true);
        }
    }

    fn conflict_dialog(&mut self, ctx: &egui::Context) {
        let conflict = self.model.conflict.clone().unwrap();
        let question = (self.model.job_id().unwrap_or(0), conflict.challenge_id);
        if self.conflict_challenge != Some(question) {
            self.conflict_challenge = Some(question);
            self.conflict_apply = false;
        }
        let mut choice = None;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::unique("archive-conflict")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).min(460.0));
            ui.heading("같은 이름의 파일이 있습니다");
            theme::bounded_text(ui, &conflict.relative_name, ui.available_width());
            ui.add_space(theme::SPACE);
            egui::Grid::new("conflict-sizes")
                .spacing([24.0, 8.0])
                .show(ui, |ui| {
                    ui.label("기존 파일");
                    ui.label(present::bytes_text(conflict.existing_size));
                    ui.end_row();
                    ui.label("압축파일 안의 파일");
                    ui.label(present::bytes_text(conflict.incoming_size));
                    ui.end_row();
                });
            ui.add_space(theme::SPACE);
            ui.checkbox(&mut self.conflict_apply, "나머지 파일에도 같게 적용");
            ui.add_space(theme::SPACE);
            ui.horizontal_wrapped(|ui| {
                for (label, value) in [
                    ("덮어쓰기", ConflictChoice::Overwrite),
                    ("건너뛰기", ConflictChoice::Skip),
                    ("이름 바꿔 저장", ConflictChoice::Rename),
                ] {
                    if theme::action(ui, label, false, self.model.error.is_none()).clicked() {
                        choice = Some(value);
                    }
                }
            });
            ui.label("이름 바꿔 저장하면 같은 폴더에 번호를 붙입니다.");
            if let Some(error) = &self.model.error {
                Self::error_notice(ui, present::client_error_text(error));
            }
            cancel = theme::action(ui, "작업 취소", false, true).clicked();
        });
        if modal.should_close() || cancel {
            self.model.cancel();
        } else if let Some(choice) = choice {
            self.model.resolve_conflict(choice, self.conflict_apply);
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
        // Review(결과 확인 후 닫기)는 show()에서 바로 확인 처리하므로 그리지 않음.
        if matches!(
            self.model.close,
            Close::Open | Close::Allowed | Close::Review
        ) {
            return;
        }
        egui::Modal::new(egui::Id::unique("archive-close")).show(ctx, |ui| {
            ui.set_width(420.0);
            match self.model.close {
                Close::Confirm => {
                    // 답하기 전에 작업이 끝났으면 취소할 작업이 없으므로 닫을지만 물음.
                    let busy = self.model.busy();
                    let (heading, text, keep, close) = if busy {
                        (
                            "작업 중 닫기",
                            "작업을 취소하고 닫을까요?",
                            "계속 작업",
                            "취소하고 닫기",
                        )
                    } else {
                        (
                            "닫기",
                            "작업이 끝났습니다. 창을 닫을까요?",
                            "열어 두기",
                            "닫기",
                        )
                    };
                    ui.heading(heading);
                    ui.label(text);
                    ui.horizontal(|ui| {
                        if theme::action(ui, keep, false, true).clicked() {
                            self.model.keep_open();
                        }
                        if theme::action(ui, close, true, true).clicked() {
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
                _ => {}
            }
        });
    }
}

impl eframe::App for ArchiveApp {
    fn ui(&mut self, root: &mut egui::Ui, _: &mut eframe::Frame) {
        self.show(root);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // 운영체제가 창 이동을 맡으면 버튼 놓기가 오지 않으므로 대신 넣어 눌림 상태를 풀어 줌.
        if std::mem::take(&mut self.release_after_move) {
            let pos = ctx.input(|i| i.pointer.latest_pos()).unwrap_or_default();
            raw_input.events.insert(
                0,
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: ctx.input(|i| i.modifiers),
                },
            );
        }
    }
}
impl Drop for ArchiveApp {
    fn drop(&mut self) {
        self.clear_password();
    }
}
