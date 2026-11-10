use std::{collections::HashSet, fs, io, path::Path};

use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Key, RichText, TextStyle,
};
use zipelo_rust_ui_probe::{Job, Navigation, Phase, ROW_COUNT, Selection, row_id, row_name};

const FONT: &str = "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc";

struct Probe {
    selection: Selection,
    job: Job,
    previous_job: Option<u64>,
    sample: String,
    notice: String,
    row_focus: HashSet<egui::Id>,
    focus_selected: bool,
    scroll_target: Option<usize>,
    rendered: std::ops::Range<usize>,
}

impl Probe {
    fn new(cc: &eframe::CreationContext<'_>, font: Vec<u8>) -> Self {
        let mut fonts = FontDefinitions::empty();
        let mut data = FontData::from_owned(font);
        data.index = 1;
        fonts
            .font_data
            .insert("installed-noto-kr".into(), data.into());
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .insert(family, vec!["installed-noto-kr".into()]);
        }
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        cc.egui_ctx.style_mut_of(egui::Theme::Light, |style| {
            for text in [TextStyle::Body, TextStyle::Button] {
                style.text_styles.insert(text, FontId::proportional(18.0));
            }
            style
                .text_styles
                .insert(TextStyle::Heading, FontId::proportional(26.0));
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 8.0);
            style.visuals.override_text_color = Some(Color32::from_rgb(24, 39, 57));
            style.visuals.panel_fill = Color32::WHITE;
            style.visuals.selection.bg_fill = Color32::from_rgb(211, 230, 246);
            style.visuals.selection.stroke = egui::Stroke::new(2.0, Color32::from_rgb(23, 76, 124));
        });
        Self {
            selection: Selection::new(ROW_COUNT),
            job: Job::default(),
            previous_job: None,
            sample: "한글 조합 입력을 확인하세요".into(),
            notice: "실제 파일과 연결되지 않은 합성 시험입니다.".into(),
            row_focus: HashSet::new(),
            focus_selected: false,
            scroll_target: None,
            rendered: 0..0,
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.job.busy(), egui::Button::new("모의 시작"))
                .clicked()
            {
                self.previous_job = (self.job.id() != 0).then_some(self.job.id());
                if self.job.start(100).is_ok() {
                    self.notice = "모의 작업을 시작했습니다. 진행 버튼으로 상태를 바꿉니다.".into();
                }
            }
            if ui
                .add_enabled(
                    self.job.phase() == Phase::Running,
                    egui::Button::new("모의 진행 +10"),
                )
                .clicked()
            {
                let id = self.job.id();
                let next = self.job.done().saturating_add(10).min(self.job.total());
                self.job.progress(id, next);
                if next == self.job.total() {
                    self.job.complete(id);
                }
            }
            if ui
                .add_enabled(
                    self.job.phase() == Phase::Running,
                    egui::Button::new("모의 취소 요청"),
                )
                .clicked()
            {
                self.job.request_cancel();
            }
            if ui
                .add_enabled(
                    self.job.phase() == Phase::CancelRequested,
                    egui::Button::new("모의 취소 확인"),
                )
                .clicked()
            {
                self.job.acknowledge_mock_cancel(self.job.id());
            }
            if ui
                .add_enabled(self.job.busy(), egui::Button::new("모의 오류"))
                .clicked()
            {
                self.job.fail(self.job.id());
            }
            if ui
                .add_enabled(
                    self.previous_job.is_some(),
                    egui::Button::new("이전 결과 주입"),
                )
                .clicked()
            {
                if let Some(id) = self.previous_job {
                    let accepted = self.job.progress(id, 100) | self.job.complete(id);
                    self.notice = if accepted {
                        "이전 결과가 수용됐습니다. 시험 실패입니다."
                    } else {
                        "이전 작업의 늦은 결과를 무시했습니다."
                    }
                    .into();
                }
            }
        });
    }

    fn list(&mut self, ui: &mut egui::Ui) {
        let enter = ui
            .button("합성 목록으로 이동")
            .accessible_name("합성 파일 100000개 목록으로 이동");
        if enter.clicked() {
            self.selection
                .select(self.selection.selected().unwrap_or(0));
            self.scroll_target = self.selection.selected();
            self.focus_selected = true;
        }
        let list_focused = enter.has_focus()
            || ui
                .ctx()
                .memory(|m| m.focused().is_some_and(|id| self.row_focus.contains(&id)));
        let row_height = ui.text_style_height(&TextStyle::Body) + 12.0;
        let stride = row_height + ui.spacing().item_spacing.y;
        let height = ui.available_height().max(1.0);
        let page = (height / stride).floor().max(1.0) as usize;
        if list_focused {
            let navigation = ui.input_mut(|input| {
                for (key, nav) in [
                    (Key::ArrowUp, Navigation::Previous),
                    (Key::ArrowDown, Navigation::Next),
                    (Key::Home, Navigation::First),
                    (Key::End, Navigation::Last),
                    (Key::PageUp, Navigation::PageUp(page)),
                    (Key::PageDown, Navigation::PageDown(page)),
                ] {
                    if input.consume_key(egui::Modifiers::NONE, key) {
                        return Some(nav);
                    }
                }
                None
            });
            if let Some(nav) = navigation {
                self.scroll_target = self.selection.navigate(nav);
                self.focus_selected = true;
            }
        }
        self.row_focus.clear();
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("synthetic-files")
            .max_height(height)
            .auto_shrink([false, false]);
        if let Some(index) = self.scroll_target.take() {
            scroll = scroll.vertical_scroll_offset(index as f32 * stride);
        }
        scroll.show_rows(ui, row_height, ROW_COUNT, |ui, range| {
            self.rendered = range.clone();
            for index in range {
                let id = row_id(index).unwrap();
                let name = row_name(index).unwrap();
                ui.push_id(("synthetic-row", id), |ui| {
                    let selected = self.selection.selected() == Some(index);
                    let response = ui.add_sized(
                        [ui.available_width(), row_height],
                        egui::Button::selectable(selected, &name),
                    );
                    let response = response.accessible_name(format!(
                        "합성 파일 {}, {} / {}, {}",
                        id,
                        index + 1,
                        ROW_COUNT,
                        name
                    ));
                    self.row_focus.insert(response.id);
                    if response.gained_focus() {
                        self.selection.select(index);
                    }
                    if response.clicked() {
                        self.selection.select(index);
                        response.request_focus();
                    }
                    if selected && self.focus_selected {
                        response.request_focus();
                        self.focus_selected = false;
                    }
                });
            }
        });
    }
}

impl eframe::App for Probe {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("probe-controls").show(root, |ui| {
            ui.heading("Zipelo UI 시험");
            ui.label("합성 파일 100,000개 / 실제 압축 작업 없음");
            ui.add(
                egui::TextEdit::singleline(&mut self.sample)
                    .char_limit(256)
                    .desired_width(f32::INFINITY),
            )
            .accessible_name("한글 IME 조합 입력 시험");
            self.controls(ui);
        });
        egui::Panel::bottom("probe-status").show(root, |ui| {
            let status = match self.job.phase() {
                Phase::Idle => "모의 작업 대기",
                Phase::Running => "모의 진행 중",
                Phase::CancelRequested => "모의 취소 요청됨 / 확인 대기",
                Phase::MockCancelled => "모의 취소 확인됨 / 실제 작업 중단 증거 아님",
                Phase::MockCompleted => "모의 완료 / 파일 생성 없음",
                Phase::MockFailed => "모의 오류 / 모의 시작으로 다시 시도",
            };
            ui.label(
                RichText::new(format!(
                    "{status}  {} / {}",
                    self.job.done(),
                    self.job.total()
                ))
                .size(20.0),
            );
            ui.label(&self.notice);
            ui.label(format!(
                "선택: {} / 이번 화면 생성 행: {}..{}",
                self.selection
                    .selected()
                    .map_or("없음".into(), |i| (i + 1).to_string()),
                self.rendered.start,
                self.rendered.end
            ));
        });
        egui::CentralPanel::default_margins().show(root, |ui| self.list(ui));
    }
}

fn main() -> eframe::Result {
    let path = Path::new(FONT);
    let length = fs::metadata(path)
        .map_err(|e| eframe::Error::AppCreation(Box::new(e)))?
        .len();
    if length != 19_484_784 {
        return Err(eframe::Error::AppCreation(Box::new(io::Error::other(
            "중앙 시험 계약의 설치 폰트 크기와 다릅니다.",
        ))));
    }
    let font = fs::read(path).map_err(|e| eframe::Error::AppCreation(Box::new(e)))?;
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 720.0])
            .with_min_inner_size([640.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Zipelo 합성 UI 시험",
        options,
        Box::new(move |cc| Ok(Box::new(Probe::new(cc, font)))),
    )
}
