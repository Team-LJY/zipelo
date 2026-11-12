use crate::{archive_present as present, archive_theme as theme};
use eframe::egui::{self, Align, Layout};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
pub enum Purpose {
    Open,
    Destination,
    Retry,
    Files,
    Folder,
    ZipFolder,
}

pub enum Choice {
    Cancel,
    Paths(Vec<PathBuf>),
}

pub struct Picker {
    pub purpose: Purpose,
    current: PathBuf,
    entries: Vec<present::BrowserEntry>,
    selected: Vec<PathBuf>,
    error: bool,
}

impl Picker {
    pub fn new(purpose: Purpose, start: &Path) -> Self {
        let current = if start.is_dir() {
            start.to_path_buf()
        } else {
            start
                .parent()
                .filter(|p| p.is_dir())
                .map(Path::to_path_buf)
                .unwrap_or_else(present::home_directory)
        };
        let mut picker = Self {
            purpose,
            current,
            entries: Vec::new(),
            selected: Vec::new(),
            error: false,
        };
        picker.reload();
        picker
    }

    fn reload(&mut self) {
        self.selected.clear();
        match present::directory_entries(&self.current) {
            Ok(entries) => {
                self.entries = entries;
                self.error = false;
            }
            Err(_) => {
                self.entries.clear();
                self.error = true;
            }
        }
    }

    fn directory_mode(&self) -> bool {
        matches!(
            self.purpose,
            Purpose::Destination | Purpose::Retry | Purpose::Folder | Purpose::ZipFolder
        )
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<Choice> {
        let mut choice = None;
        let mut navigate = None;
        let width = (ctx.content_rect().width() - 64.0).min(720.0);
        let height = (ctx.content_rect().height() - 280.0).clamp(160.0, 340.0);
        let title = match self.purpose {
            Purpose::Open => "파일 열기",
            Purpose::Destination | Purpose::Retry => "풀 위치 선택",
            Purpose::Files => "묶을 파일 선택",
            Purpose::Folder => "묶을 폴더 선택",
            Purpose::ZipFolder => "ZIP 저장 폴더 선택",
        };
        let title = if self.purpose == Purpose::Open {
            "파일 열기"
        } else {
            title
        };
        let modal = egui::Modal::new(egui::Id::unique("archive-picker")).show(ctx, |ui| {
            ui.set_width(width);
            ui.heading(title);
            egui::ScrollArea::horizontal()
                .id_salt("breadcrumbs")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let mut ancestors = self.current.ancestors().collect::<Vec<_>>();
                        ancestors.reverse();
                        for path in ancestors {
                            let label = path
                                .file_name()
                                .unwrap_or(path.as_os_str())
                                .to_string_lossy();
                            if ui
                                .button(label.as_ref())
                                .on_hover_text(path.display().to_string())
                                .clicked()
                            {
                                navigate = Some(path.to_path_buf());
                            }
                        }
                    });
                });
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(128.0, height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_min_size(egui::vec2(128.0, height));
                        for (label, path) in present::shortcuts() {
                            if theme::action(ui, &label, false, true).clicked() {
                                navigate = Some(path);
                            }
                        }
                        if let Some(parent) = self.current.parent() {
                            if theme::action(ui, "상위 폴더", false, true).clicked() {
                                navigate = Some(parent.to_path_buf());
                            }
                        }
                    },
                );
                ui.add_space(theme::SPACE);
                ui.allocate_ui_with_layout(
                    egui::vec2((width - 160.0).max(240.0), height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_min_size(egui::vec2((width - 160.0).max(240.0), height));
                        if self.error {
                            ui.colored_label(
                                theme::ERROR,
                                "이 폴더를 열 수 없습니다. 접근 권한을 확인하세요",
                            );
                        } else if self.entries.is_empty() {
                            ui.label("이 폴더는 비어 있습니다");
                        }
                        egui::ScrollArea::vertical()
                            .id_salt("picker-entries")
                            .max_height(height)
                            .show_rows(ui, theme::ROW, self.entries.len(), |ui, rows| {
                                for index in rows {
                                    let entry = &self.entries[index];
                                    let selected = self.selected.contains(&entry.path);
                                    let response = ui
                                        .horizontal(|ui| {
                                            theme::icon(
                                                ui,
                                                if entry.directory {
                                                    theme::Icon::Folder
                                                } else {
                                                    theme::Icon::File
                                                },
                                                20.0,
                                            );
                                            let name = if entry.link {
                                                format!("{} (링크)", entry.name)
                                            } else {
                                                entry.name.clone()
                                            };
                                            ui.add_sized(
                                                [ui.available_width(), theme::ROW],
                                                egui::Button::new("")
                                                    .left_text(present::middle_text(&name, 38))
                                                    .selected(selected),
                                            )
                                            .on_hover_text(entry.path.display().to_string())
                                        })
                                        .inner;
                                    theme::focus(&response);
                                    if response.clicked() {
                                        if self.purpose == Purpose::Files
                                            && ui.input(|i| i.modifiers.command || i.modifiers.ctrl)
                                        {
                                            if selected {
                                                self.selected.retain(|p| p != &entry.path);
                                            } else if !entry.directory {
                                                self.selected.push(entry.path.clone());
                                            }
                                        } else {
                                            self.selected = vec![entry.path.clone()];
                                        }
                                    }
                                    let keyboard_activation = response.has_focus()
                                        && ui.input(|i| {
                                            i.key_pressed(egui::Key::Enter)
                                                || i.key_pressed(egui::Key::Space)
                                        });
                                    if response.double_clicked() || keyboard_activation {
                                        if entry.directory && !entry.link {
                                            navigate = Some(entry.path.clone());
                                        } else if !self.directory_mode() {
                                            choice = Some(Choice::Paths(vec![entry.path.clone()]));
                                        }
                                    }
                                }
                            });
                    },
                );
            });
            ui.separator();
            ui.horizontal(|ui| {
                if self.purpose == Purpose::Files {
                    ui.label("Ctrl을 누르면 여러 파일을 고를 수 있습니다");
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let selected_directory = self.selected.first().filter(|path| {
                        self.entries
                            .iter()
                            .any(|e| &e.path == *path && (e.directory || e.link))
                    });
                    let paths = if self.directory_mode() {
                        vec![
                            selected_directory
                                .cloned()
                                .unwrap_or_else(|| self.current.clone()),
                        ]
                    } else {
                        self.selected
                            .iter()
                            .filter(|path| {
                                self.entries
                                    .iter()
                                    .any(|e| &e.path == *path && !e.directory)
                            })
                            .cloned()
                            .collect()
                    };
                    let label = if self.directory_mode() {
                        "이 폴더 선택"
                    } else if self.purpose == Purpose::Files {
                        "추가"
                    } else {
                        "열기"
                    };
                    let enabled = !paths.is_empty() && !self.error;
                    let (cancel, confirm) = theme::action_pair(ui, "취소", Some((label, enabled)));
                    if navigate.is_none()
                        && (confirm || (enabled && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                    {
                        choice = Some(Choice::Paths(paths));
                    }
                    if cancel {
                        choice = Some(Choice::Cancel);
                    }
                });
            });
        });
        if modal.should_close() {
            choice = Some(Choice::Cancel);
        }
        if let Some(path) = navigate {
            self.current = path;
            self.reload();
        }
        choice
    }
}
