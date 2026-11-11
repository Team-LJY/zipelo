#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(feature = "ui-fixture")]
#[path = "../../../tests/rust-app-ui/fake.rs"]
mod fake;
#[path = "renderer.rs"]
mod renderer;
#[path = "system_font.rs"]
mod system_font;
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
fn main() -> eframe::Result {
    let result = run();
    #[cfg(target_os = "windows")]
    if let Err(error) = &result {
        let message = if matches!(error, eframe::Error::AppCreation(_)) {
            "앱을 시작하지 못했습니다. 설치 파일을 다시 설치한 뒤 시도해 주세요."
        } else {
            "이 PC에서 화면을 표시할 수 없습니다. 그래픽 드라이버를 업데이트해 주세요."
        };
        show_startup_error(message);
    }
    result
}

#[cfg(target_os = "windows")]
fn show_startup_error(message: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            window: *mut std::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let caption: Vec<u16> = "Zipelo".encode_utf16().chain(Some(0)).collect();
    // 종료까지 유효한 NUL 종료 문자열로 기본 오류 창을 표시함.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), 0x10);
    }
}

fn run() -> eframe::Result {
    let font = system_font::load().map_err(|e| eframe::Error::AppCreation(Box::new(e)))?;
    renderer::run_native(
        "Zipelo",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1060.0, 720.0])
                .with_min_inner_size([760.0, 540.0]),
            ..Default::default()
        },
        || {
            let bytes = font.bytes.clone();
            let face_index = font.face_index;
            Box::new(move |cc| {
                let mut fonts = FontDefinitions::empty();
                let mut data = FontData::from_owned(bytes);
                data.index = face_index;
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
                let (fixture, startup) = zipelo_ui_memory_app::archive_ui::launch_args(
                    std::env::args_os().skip(1).collect(),
                    std::env::current_dir()?,
                )
                .map_err(std::io::Error::other)?;
                let factory: Box<dyn zipelo_ui_memory_app::archive_ui::Factory> = if fixture {
                    #[cfg(feature = "ui-fixture")]
                    {
                        Box::new(fake::FakeFactory::default())
                    }
                    #[cfg(not(feature = "ui-fixture"))]
                    {
                        return Err(Box::new(std::io::Error::other(
                            "ui-fixture feature가 필요합니다",
                        )));
                    }
                } else {
                    Box::new(
                        zipelo_ui_memory_app::archive_process::ProcessFactory::sibling()
                            .map_err(std::io::Error::other)?,
                    )
                };
                Ok(Box::new(
                    zipelo_ui_memory_app::archive_screen::ArchiveApp::new(
                        factory, startup, fixture,
                    ),
                ))
            })
        },
    )
}
