#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(feature = "ui-fixture")]
#[path = "../../../tests/rust-app-ui/fake.rs"]
mod fake;
#[path = "renderer.rs"]
mod renderer;
#[path = "system_font.rs"]
mod system_font;
use eframe::egui;
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
    let custom_frame = zipelo_ui_memory_app::window_frame::custom_frame_needed();
    // 운영체제 한글 글꼴은 Pretendard에 없는 글자를 위한 대체 글꼴로만 씀.
    let fallback = system_font::load()
        .ok()
        .map(|font| (font.bytes, font.face_index));
    renderer::run_native(
        "Zipelo",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1060.0, 720.0])
                .with_min_inner_size([760.0, 540.0])
                .with_decorations(!custom_frame),
            ..Default::default()
        },
        || {
            let fallback = fallback.clone();
            Box::new(move |cc| {
                cc.egui_ctx
                    .set_fonts(zipelo_ui_memory_app::archive_theme::fonts(fallback));
                zipelo_ui_memory_app::archive_theme::apply(&cc.egui_ctx);
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
                let mut app = zipelo_ui_memory_app::archive_screen::ArchiveApp::new(
                    factory, startup, fixture,
                );
                app.set_custom_frame(custom_frame);
                Ok(Box::new(app))
            })
        },
    )
}
