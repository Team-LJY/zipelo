use eframe::egui::{self, Color32, FontId, Stroke, TextStyle};

pub const BACKGROUND: Color32 = Color32::from_rgb(247, 248, 250);
pub const PANEL: Color32 = Color32::WHITE;
pub const STRIPE: Color32 = Color32::from_rgb(243, 245, 248);
pub const BORDER: Color32 = Color32::from_rgb(217, 222, 229);
pub const TEXT: Color32 = Color32::from_rgb(31, 41, 51);
pub const MUTED: Color32 = Color32::from_rgb(91, 102, 115);
pub const BLUE: Color32 = Color32::from_rgb(37, 99, 235);
pub const BLUE_HOVER: Color32 = Color32::from_rgb(29, 78, 216);
pub const BLUE_ACTIVE: Color32 = Color32::from_rgb(30, 64, 175);
pub const SELECTED: Color32 = Color32::from_rgb(232, 240, 254);
pub const DROP: Color32 = Color32::from_rgb(238, 244, 255);
pub const DASH: Color32 = Color32::from_rgb(170, 180, 192);
pub const SUCCESS: Color32 = Color32::from_rgb(21, 128, 61);
pub const SUCCESS_BG: Color32 = Color32::from_rgb(232, 245, 236);
pub const WARNING: Color32 = Color32::from_rgb(180, 83, 9);
pub const WARNING_BG: Color32 = Color32::from_rgb(255, 247, 232);
pub const ERROR: Color32 = Color32::from_rgb(185, 28, 28);
pub const ERROR_BG: Color32 = Color32::from_rgb(253, 237, 237);
pub const FOLDER: Color32 = Color32::from_rgb(240, 160, 0);
pub const FILE: Color32 = Color32::from_rgb(123, 135, 148);
pub const SPACE: f32 = 8.0;
pub const PAD: i8 = 16;
pub const DROP_PAD: i8 = 24;
pub const DROP_HEIGHT: f32 = 280.0;
pub const DROP_RADIUS: u8 = 12;
pub const ROW: f32 = 32.0;
pub const BUTTON: f32 = 40.0;
pub const BODY: f32 = 17.0;
pub const TITLE: f32 = 22.0;

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    ctx.set_visuals(egui::Visuals::light());
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(BODY));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(BODY));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(BODY));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(TITLE));
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.panel_fill = BACKGROUND;
        style.visuals.window_fill = PANEL;
        style.visuals.extreme_bg_color = PANEL;
        style.visuals.faint_bg_color = STRIPE;
        style.visuals.selection.bg_fill = SELECTED;
        style.visuals.selection.stroke = Stroke::new(2.0, BLUE);
        style.visuals.widgets.inactive.bg_fill = PANEL;
        style.visuals.widgets.inactive.weak_bg_fill = PANEL;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        style.visuals.widgets.hovered.bg_fill = DROP;
        style.visuals.widgets.hovered.weak_bg_fill = DROP;
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BLUE);
        style.visuals.widgets.active.bg_stroke = Stroke::new(2.0, BLUE);
        for widget in [
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.noninteractive,
        ] {
            widget.corner_radius = egui::CornerRadius::same(6);
            widget.fg_stroke = Stroke::new(1.5, TEXT);
        }
        style.visuals.window_corner_radius = egui::CornerRadius::same(8);
        style.spacing.item_spacing = egui::vec2(SPACE, SPACE);
        style.spacing.button_padding = egui::vec2(16.0, SPACE);
        style.spacing.interact_size.y = ROW;
        style.spacing.window_margin = egui::Margin::same(PAD);
        style.visuals.disabled_alpha = 0.4;
    });
}

pub fn focus(response: &egui::Response) {
    if response.has_focus() {
        response.ctx.layer_painter(response.layer_id).rect_stroke(
            response.rect.expand(2.0),
            6,
            Stroke::new(2.0, BLUE),
            egui::StrokeKind::Outside,
        );
    }
}

pub fn action(ui: &mut egui::Ui, text: &str, primary: bool, enabled: bool) -> egui::Response {
    let response = ui
        .scope(|ui| {
            if primary {
                let style = ui.style_mut();
                style.visuals.widgets.inactive.bg_fill = BLUE;
                style.visuals.widgets.inactive.weak_bg_fill = BLUE;
                style.visuals.widgets.hovered.bg_fill = BLUE_HOVER;
                style.visuals.widgets.hovered.weak_bg_fill = BLUE_HOVER;
                style.visuals.widgets.active.bg_fill = BLUE_ACTIVE;
                style.visuals.widgets.active.weak_bg_fill = BLUE_ACTIVE;
            }
            ui.add_enabled(
                enabled,
                egui::Button::new(egui::RichText::new(text).strong().color(if primary {
                    PANEL
                } else if text == "취소" {
                    ERROR
                } else {
                    TEXT
                }))
                .min_size(egui::vec2(
                    if primary { 112.0 } else { 0.0 },
                    if primary { BUTTON } else { 36.0 },
                ))
                .stroke(Stroke::new(1.0, if primary { BLUE } else { BORDER })),
            )
        })
        .inner;
    focus(&response);
    response
}

pub fn panel(fill: Color32) -> egui::Frame {
    egui::Frame::new()
        .fill(fill)
        .inner_margin(egui::Margin::same(PAD))
        .corner_radius(8)
}

#[derive(Clone, Copy)]
pub enum Icon {
    File,
    Folder,
    Archive,
    Lock,
    Success,
    Warning,
    Error,
}

pub fn icon(ui: &mut egui::Ui, kind: Icon, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let p = ui.painter();
    let color = match kind {
        Icon::Folder => FOLDER,
        Icon::Archive => BLUE,
        Icon::Lock | Icon::Warning => WARNING,
        Icon::Success => SUCCESS,
        Icon::Error => ERROR,
        Icon::File => FILE,
    };
    let s = Stroke::new(if size > 32.0 { 2.5 } else { 1.5 }, color);
    let point = |x, y| rect.min + egui::vec2(x * size, y * size);
    let box_at = |x, y, w, h| egui::Rect::from_min_max(point(x, y), point(x + w, y + h));
    match kind {
        Icon::Folder => {
            p.rect_filled(box_at(0.08, 0.22, 0.42, 0.22), 2, color);
            p.rect_filled(box_at(0.08, 0.34, 0.84, 0.50), 2, color);
        }
        Icon::File | Icon::Archive => {
            p.rect_stroke(
                box_at(0.19, 0.08, 0.62, 0.84),
                2,
                s,
                egui::StrokeKind::Inside,
            );
            p.line_segment([point(0.60, 0.08), point(0.60, 0.30)], s);
            p.line_segment([point(0.60, 0.30), point(0.81, 0.30)], s);
            if matches!(kind, Icon::Archive) {
                for y in [0.42, 0.57, 0.72] {
                    p.line_segment([point(0.39, y), point(0.59, y)], s);
                }
            }
        }
        Icon::Lock => {
            p.circle_stroke(point(0.50, 0.35), size * 0.23, s);
            p.rect_filled(box_at(0.18, 0.39, 0.64, 0.47), 2, color);
        }
        Icon::Success | Icon::Error => {
            p.circle_stroke(rect.center(), size * 0.40, s);
            if matches!(kind, Icon::Success) {
                p.line_segment([point(0.28, 0.50), point(0.44, 0.66)], s);
                p.line_segment([point(0.44, 0.66), point(0.74, 0.33)], s);
            } else {
                p.line_segment([point(0.34, 0.34), point(0.66, 0.66)], s);
                p.line_segment([point(0.66, 0.34), point(0.34, 0.66)], s);
            }
        }
        Icon::Warning => {
            p.add(egui::Shape::closed_line(
                vec![point(0.50, 0.08), point(0.94, 0.86), point(0.06, 0.86)],
                s,
            ));
            p.line_segment([point(0.50, 0.32), point(0.50, 0.57)], s);
            p.circle_filled(point(0.50, 0.73), size * 0.035, color);
        }
    }
}

pub fn column_label(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    width: f32,
    align: egui::Align,
) -> egui::Response {
    let layout = if align == egui::Align::Max {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    ui.allocate_ui_with_layout(egui::vec2(width, ROW), layout, |ui| {
        ui.set_min_width(width);
        ui.set_max_width(width);
        ui.add(egui::Label::new(text).truncate())
    })
    .inner
}

pub fn action_pair(
    ui: &mut egui::Ui,
    secondary: &str,
    primary: Option<(&str, bool)>,
) -> (bool, bool) {
    let text_width = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.into(), FontId::proportional(BODY), TEXT)
            .size()
            .x
    };
    let width = text_width(secondary)
        + 32.0
        + primary.map_or(0.0, |(label, _)| {
            SPACE + (text_width(label) + 32.0).max(112.0)
        });
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add_space((ui.available_width() - width).max(0.0));
            let secondary = action(ui, secondary, false, true).clicked();
            let primary = primary.is_some_and(|(label, enabled)| {
                action(ui, label, true, enabled)
                    .on_disabled_hover_text(match label {
                        "만들기" => "묶을 파일이나 폴더를 추가하세요",
                        "다른 폴더에 풀기" => "기존 풀 위치 폴더를 먼저 확인하세요",
                        _ => "입력한 내용을 확인하세요",
                    })
                    .clicked()
            });
            (secondary, primary)
        })
        .inner
    })
    .inner
}

pub fn indeterminate(ui: &mut egui::Ui) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), SPACE * 2.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 8, BORDER);
    let phase = if ui.style().animation_time > 0.0 {
        ui.input(|i| (i.time * 0.6).fract()) as f32
    } else {
        0.5
    };
    let width = (SPACE * 8.0).min(rect.width());
    let pulse = egui::Rect::from_min_size(
        rect.min + egui::vec2((rect.width() - width) * phase, 0.0),
        egui::vec2(width, rect.height()),
    );
    ui.painter().rect_filled(pulse, 8, BLUE);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::ProgressIndicator,
            ui.is_enabled(),
            "전체 크기를 알 수 없는 작업 진행 중",
        )
    });
}

pub const OPEN: &str = include_str!("assets/icons/open-archive.svg");
pub const CREATE: &str = include_str!("assets/icons/create-zip.svg");
pub const EXTRACT: &str = include_str!("assets/icons/extract-archive.svg");
pub const UP: &str = include_str!("assets/icons/navigate-up.svg");
pub const CLOSE: &str = include_str!("assets/icons/close-dialog.svg");

pub fn svg_icon(painter: &egui::Painter, rect: egui::Rect, asset: &str, color: Color32) {
    for element in asset.split("points=\"").skip(1) {
        let points = element
            .split('"')
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|pair| {
                let (x, y) = pair.split_once(',')?;
                Some(
                    rect.min
                        + egui::vec2(x.parse::<f32>().ok()?, y.parse::<f32>().ok()?)
                            * (rect.width() / 24.0),
                )
            })
            .collect::<Vec<_>>();
        painter.add(egui::Shape::line(
            points,
            Stroke::new(rect.width() / 18.0, color),
        ));
    }
}

pub fn tool(
    ui: &mut egui::Ui,
    label: &str,
    asset: &str,
    enabled: bool,
    blue: bool,
) -> egui::Response {
    let response = ui.add_enabled(
        enabled,
        egui::Button::new(
            egui::RichText::new(label)
                .size(18.0)
                .color(Color32::TRANSPARENT),
        )
        .frame(false)
        .min_size(egui::vec2(108.0, 86.0)),
    );
    let rect = response.rect;
    let color = if enabled {
        if blue { BLUE } else { TEXT }
    } else {
        MUTED
    };
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 27.0),
        egui::vec2(36.0, 36.0),
    );
    ui.painter()
        .rect_filled(rect.shrink2(egui::vec2(1.0, 27.0)), 0, PANEL);
    svg_icon(ui.painter(), icon_rect, asset, color);
    ui.painter().text(
        egui::pos2(rect.center().x, rect.bottom() - 15.0),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(18.0),
        color,
    );
    focus(&response);
    response
}

pub const SEARCH: &str = include_str!("assets/icons/search-files.svg");

pub fn icon_button(ui: &mut egui::Ui, name: &str, asset: &str, enabled: bool) -> egui::Response {
    let response = ui
        .add_enabled(
            enabled,
            egui::Button::new("").min_size(egui::vec2(38.0, 36.0)),
        )
        .on_hover_text(name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    svg_icon(
        ui.painter(),
        egui::Rect::from_center_size(response.rect.center(), egui::vec2(22.0, 22.0)),
        asset,
        if enabled { TEXT } else { MUTED },
    );
    focus(&response);
    response
}
