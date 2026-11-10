use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use zipelo_ui_memory_app::{
    controller::{CloseState, Controller},
    runtime::Action,
};

#[derive(Default)]
struct App {
    controller: Controller,
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.controller.poll();
        let ctx = root.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) {
            self.controller.request_close();
            if self.controller.close != CloseState::Allowed {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if self.controller.close == CloseState::Allowed {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        egui::Panel::top("actions").show(root, |ui| {
            ui.heading("Zipelo 메모리 작업실");
            ui.label("합성 입력 전용 / 파일 저장 미지원");
            ui.horizontal_wrapped(|ui| {
                for (label, action) in [
                    ("ZIP 생성", Action::Zip),
                    ("USTAR 목록과 내용", Action::Ustar),
                    ("GZip 검증과 내용", Action::Gzip),
                    ("합성 오류 확인", Action::InvalidGzip),
                ] {
                    if ui
                        .add_enabled(
                            !self.controller.busy() && self.controller.close == CloseState::Open,
                            egui::Button::new(label),
                        )
                        .clicked()
                    {
                        self.controller.start(action);
                    }
                }
                if ui
                    .add_enabled(self.controller.busy(), egui::Button::new("취소 요청"))
                    .clicked()
                {
                    self.controller.cancel();
                }
                if ui.button("닫기").clicked() {
                    self.controller.request_close();
                }
            });
        });
        egui::Panel::bottom("status").show(root, |ui| {
            ui.label(self.controller.status_text());
            if let Some(progress) = self.controller.progress {
                ui.label(format!("{:?}: {}", progress.metric, progress.value));
            }
            ui.label("USTAR 내용 무결성 검증 정보는 제공되지 않음");
            if self.controller.busy() {
                ui.label("작업 스레드 종료 확인 전에는 다시 시작하거나 닫지 않습니다");
            }
            if let Some(error) = &self.controller.spawn_error {
                ui.label(format!(
                    "작업 시작 실패: {}",
                    format!("{error:?}").chars().take(256).collect::<String>()
                ));
            }
            if let Some(completion) = &self.controller.terminal {
                if !completion.outcome.is_ok() || completion.delivery.is_some() {
                    let detail = match &completion.outcome {
                        zipelo_ui_memory_app::runtime::Outcome::Zip(t) => {
                            format!("{:?}", t.result().err())
                        }
                        zipelo_ui_memory_app::runtime::Outcome::Ustar(t) => {
                            format!("{:?}", t.result().err())
                        }
                        zipelo_ui_memory_app::runtime::Outcome::Gzip(t) => {
                            format!("{:?}", t.result().err())
                        }
                    };
                    ui.label(format!(
                        "반환 원인: {}",
                        detail.chars().take(512).collect::<String>()
                    ));
                    if let Some(fault) = completion.delivery {
                        ui.label(format!(
                            "UI 전달 원인: {:?}, 최초 누락 {}",
                            fault.kind, fault.first_missing_sequence
                        ));
                    }
                }
            }
        });
        egui::CentralPanel::default_margins().show(root, |ui| {
            ui.columns(2, |columns| {
                columns[0].heading("메모리 결과 목록");
                let rows: Vec<String> = self
                    .controller
                    .rows()
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
                for (index, name) in rows.iter().enumerate() {
                    if columns[0]
                        .selectable_label(
                            self.controller.selected.is_some_and(|(_, _, i)| i == index),
                            name,
                        )
                        .clicked()
                    {
                        if let Some(task) = &self.controller.task {
                            self.controller
                                .select(task.id, self.controller.generation, index);
                        }
                    }
                }
                columns[1].heading("선택한 내용");
                if let Some(bytes) = self.controller.selected_bytes() {
                    columns[1].label(format!("원본 {} bytes / 텍스트 최대 2048자", bytes.len()));
                }
                columns[1].label(self.controller.preview());
            });
        });
        if self.controller.close == CloseState::Confirming {
            egui::Window::new("작업 중 닫기")
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label("취소를 요청하고 작업 스레드 종료를 기다릴까요?");
                    if ui.button("계속 열기").clicked() {
                        self.controller.keep_open();
                    }
                    if ui.button("취소 후 닫기").clicked() {
                        self.controller.confirm_close();
                    }
                });
        }
        if self.controller.close == CloseState::Waiting {
            egui::Window::new("종료 대기")
                .collapsible(false)
                .show(&ctx, |ui| {
                    ui.label("취소 요청은 전송됐습니다. 작업 종료를 기다립니다.");
                    if ui.button("닫기 철회").clicked() {
                        self.controller.keep_open();
                    }
                });
        }
        if self.controller.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
}

fn main() -> eframe::Result {
    let font = std::fs::read("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc")
        .map_err(|e| eframe::Error::AppCreation(Box::new(e)))?;
    eframe::run_native(
        "Zipelo 합성 메모리 작업",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Glow,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1060.0, 720.0])
                .with_min_inner_size([760.0, 540.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut fonts = FontDefinitions::empty();
            let mut data = FontData::from_owned(font);
            data.index = 1;
            fonts.font_data.insert("noto-kr".into(), data.into());
            fonts
                .families
                .insert(FontFamily::Proportional, vec!["noto-kr".into()]);
            cc.egui_ctx.set_fonts(fonts);
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
                style.visuals.panel_fill = Color32::WHITE;
                style.visuals.override_text_color = Some(Color32::from_rgb(24, 39, 57));
                style.visuals.selection.bg_fill = Color32::from_rgb(211, 230, 246);
                style.spacing.item_spacing = egui::vec2(12.0, 10.0);
                style.spacing.button_padding = egui::vec2(14.0, 10.0);
            });
            Ok(Box::<App>::default())
        }),
    )
}
