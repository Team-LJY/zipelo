#[cfg(feature = "ui-fixture")]
#[path = "../../../tests/rust-app-ui/fake.rs"]
mod fake;
#[path = "system_font.rs"]
mod system_font;
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
fn main() -> eframe::Result {
    let font = system_font::load().map_err(|e| eframe::Error::AppCreation(Box::new(e)))?;
    eframe::run_native(
        "Zipelo",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Glow,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1060.0, 720.0])
                .with_min_inner_size([760.0, 540.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut fonts = FontDefinitions::empty();
            let mut data = FontData::from_owned(font.bytes);
            data.index = font.face_index;
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
                zipelo_ui_memory_app::archive_screen::ArchiveApp::new(factory, startup, fixture),
            ))
        }),
    )
}
