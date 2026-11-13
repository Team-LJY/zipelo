use std::sync::{Arc, OnceLock};

pub fn load() -> Arc<eframe::egui::IconData> {
    static ICON: OnceLock<Arc<eframe::egui::IconData>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(
            eframe::icon_data::from_png_bytes(include_bytes!("assets/icons/zipelo-256.png"))
                .expect("공통 앱 아이콘 PNG가 유효해야 함"),
        )
    })
    .clone()
}
