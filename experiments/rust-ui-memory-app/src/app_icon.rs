pub fn load() -> eframe::egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("assets/icons/zipelo-256.png"))
        .expect("공통 앱 아이콘 PNG가 유효해야 함")
}
