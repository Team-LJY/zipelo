#![forbid(unsafe_code)]
#![allow(dead_code)]

extern crate self as zipelo_ui_memory_app;
include!("../rust-ui-memory-races/assembly.rs");
mod gates;

use std::sync::atomic::Ordering;

mod original_app {
    include!("../../experiments/rust-ui-memory-app/src/app.rs");

    use crate::{controller::Status, gates};
    use std::{
        sync::atomic::Ordering,
        time::{Duration, Instant},
    };

    struct Harness {
        original: App,
        control: gates::Control,
        scenario: String,
        started: Instant,
        previous: Option<String>,
        frames: u64,
        operation: crate::core::OperationId,
    }

    pub fn create(
        controller: Controller,
        control: gates::Control,
        scenario: String,
    ) -> Box<dyn eframe::App> {
        let operation = controller.task.as_ref().unwrap().id;
        Box::new(Harness {
            original: App { controller },
            control,
            scenario,
            started: Instant::now(),
            previous: None,
            frames: 0,
            operation,
        })
    }

    impl eframe::App for Harness {
        fn ui(&mut self, root: &mut egui::Ui, frame: &mut eframe::Frame) {
            self.control.observe();
            egui::Panel::top("race-harness-only").show(root, |ui| {
                ui.heading("시험용 화면 / 배포 바이너리 검증 아님");
                ui.label(format!(
                    "시나리오: {} / 장벽: {:?}",
                    self.scenario, self.control.stage
                ));
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            self.control.checkpoint_enabled(),
                            egui::Button::new("checkpoint 해제"),
                        )
                        .clicked()
                    {
                        self.control.release_checkpoint();
                    }
                    if ui
                        .add_enabled(self.control.join_enabled(), egui::Button::new("join 해제"))
                        .clicked()
                    {
                        self.control.release_join();
                    }
                });
            });
            <App as eframe::App>::ui(&mut self.original, root, frame);
            self.frames += 1;
            let c = &mut self.original.controller;
            if c.task.as_ref().map(|t| t.id) != Some(self.operation) {
                self.control.evidence.fault.store(true, Ordering::SeqCst);
            }
            let joined = c.task.as_ref().is_some_and(|task| task.joined());
            self.control.evidence.joined.store(joined, Ordering::SeqCst);
            if joined {
                let result = match c.status {
                    Status::Completed => 1,
                    Status::Cancelled => 2,
                    _ => 0,
                };
                self.control
                    .evidence
                    .final_result
                    .store(result, Ordering::SeqCst);
            }
            let state = format!(
                "scenario={} stage={:?} status={:?} close={:?} terminal={} joined={} cancel={} rows={}",
                self.scenario,
                self.control.stage,
                c.status,
                c.close,
                c.terminal.is_some(),
                joined,
                c.task
                    .as_ref()
                    .is_some_and(|t| t.cancellation.is_requested()),
                c.rows().len()
            );
            if self.previous.as_ref() != Some(&state) {
                eprintln!("harness frame={} {state}", self.frames);
                self.previous = Some(state);
            }
            if c.status == Status::Panicked || self.started.elapsed() > Duration::from_secs(180) {
                self.control.evidence.fault.store(true, Ordering::SeqCst);
            }
            if self.control.evidence.fault.load(Ordering::SeqCst) {
                c.request_close();
                c.confirm_close();
            }
            root.ctx().request_repaint_after(Duration::from_millis(30));
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            let joined = self
                .original
                .controller
                .task
                .as_ref()
                .is_some_and(|t| t.joined());
            self.control.evidence.joined.store(joined, Ordering::SeqCst);
            if !joined {
                self.control.evidence.fault.store(true, Ordering::SeqCst);
            }
            eprintln!("harness drop joined={joined} frames={}", self.frames);
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3
        || args[0] != "--opt-in-xvfb"
        || args[1] != "--scenario"
        || !["cancel", "retract", "close"].contains(&args[2].as_str())
    {
        return Err("명시적 시험 실행 필요: --opt-in-xvfb --scenario cancel|retract|close".into());
    }
    let display = std::env::var("DISPLAY").map_err(|_| "시험 DISPLAY 없음")?;
    if display == ":0"
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("ZIPELO_XVFB_HARNESS").as_deref() != Ok("1")
    {
        return Err("분리 Xvfb 자식 환경에서만 실행할 것".into());
    }
    let font = std::fs::read("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc")
        .map_err(|e| e.to_string())?;
    let (controller, control) = gates::create();
    let evidence = control.evidence.clone();
    let scenario = args[2].clone();
    let expected = if scenario == "retract" { 1 } else { 2 };
    let result = eframe::run_native(
        "Zipelo 시험용 메모리 경합 화면",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Glow,
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size([1060.0, 840.0])
                .with_min_inner_size([760.0, 660.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            use eframe::egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
            let mut fonts = FontDefinitions::empty();
            let mut data = FontData::from_owned(font);
            data.index = 1;
            fonts.font_data.insert("noto-kr".into(), data.into());
            fonts
                .families
                .insert(FontFamily::Proportional, vec!["noto-kr".into()]);
            cc.egui_ctx.set_fonts(fonts);
            cc.egui_ctx.set_visuals(eframe::egui::Visuals::light());
            cc.egui_ctx
                .style_mut_of(eframe::egui::Theme::Light, |style| {
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
                    style.spacing.item_spacing = eframe::egui::vec2(12.0, 10.0);
                    style.spacing.button_padding = eframe::egui::vec2(14.0, 10.0);
                });
            Ok(original_app::create(controller, control, scenario))
        }),
    )
    .map_err(|e| e.to_string());
    result?;
    let joined = evidence.joined.load(Ordering::SeqCst);
    let actual = evidence.final_result.load(Ordering::SeqCst);
    if evidence.fault.load(Ordering::SeqCst) || !joined || actual != expected {
        return Err(format!(
            "시험 실패: joined={joined} expected={expected} actual={actual}"
        ));
    }
    eprintln!("harness scenario completed joined=true result={actual}");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("harness failure: {error}");
        std::process::exit(1);
    }
}
