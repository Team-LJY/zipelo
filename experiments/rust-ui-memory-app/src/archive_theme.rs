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
/// 파일 목록에서 선택한 행의 반투명 파란 배경 (BLUE 12%).
pub const ROW_SELECTED: Color32 = Color32::from_rgba_unmultiplied_const(37, 99, 235, 31);
/// 파일 목록에서 마우스를 올린 행의 배경. 선택한 행보다 조금 더 투명함 (BLUE 6%).
pub const ROW_HOVER: Color32 = Color32::from_rgba_unmultiplied_const(37, 99, 235, 15);
/// 선택한 행에 마우스를 올렸을 때의 배경. 선택 색보다 덜 투명함 (BLUE 18%).
pub const ROW_SELECTED_HOVER: Color32 = Color32::from_rgba_unmultiplied_const(37, 99, 235, 46);
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
/// 글자 크기 단계. 앱의 모든 글자는 이 세 값 중 하나를 씀.
/// 기본 16px을 기준으로 보조 글자는 작게, 제목과 강조 글자는 크게 씀.
pub const FONT_SMALL: f32 = 14.0;
pub const FONT_BODY: f32 = 16.0;
pub const FONT_LARGE: f32 = 20.0;
/// 경로와 검색 입력란의 높이. 위쪽 경로 줄은 이 높이만큼만 차지함.
pub const FIELD: f32 = 36.0;
/// 테두리 없는 버튼에 마우스를 올리거나 누를 때의 배경.
pub const HOVER: Color32 = Color32::from_rgb(229, 233, 239);
pub const PRESS: Color32 = Color32::from_rgb(214, 220, 229);

/// 앱에 포함한 기본 글꼴 (SIL OFL 1.1, assets/fonts/OFL.txt).
pub const PRETENDARD: &[u8] = include_bytes!("assets/fonts/Pretendard-Regular.ttf");

/// Pretendard를 기본 글꼴로 쓰고, 없는 글자는 운영체제 한글 글꼴로 대신 그림.
pub fn fonts(fallback: Option<(Vec<u8>, u32)>) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert(
        "pretendard".into(),
        egui::FontData::from_static(PRETENDARD).into(),
    );
    let mut family = vec!["pretendard".to_owned()];
    if let Some((bytes, index)) = fallback {
        let mut data = egui::FontData::from_owned(bytes);
        data.index = index;
        fonts.font_data.insert("system-kr".into(), data.into());
        family.push("system-kr".into());
    }
    fonts
        .families
        .insert(egui::FontFamily::Proportional, family.clone());
    fonts.families.insert(egui::FontFamily::Monospace, family);
    fonts
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    ctx.set_visuals(egui::Visuals::light());
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(FONT_BODY));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(FONT_BODY));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(FONT_SMALL));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(FONT_LARGE));
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
        // egui 기본 버튼에도 손 모양 커서를 씀.
        style.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
        // 클릭과 표시가 즉시 바뀌도록 서서히 나타남, 부드러운 스크롤, 커서 깜빡임을 모두 끔.
        style.animation_time = 0.0;
        style.scroll_animation = egui::style::ScrollAnimation::none();
        style.visuals.text_cursor.blink = false;
    });
}

/// 누를 수 있는 버튼이나 항목에 마우스를 올리면 손 모양 커서를 보여 줌.
/// 사용할 수 없거나 누를 수 없는 영역(끌기 전용 등)은 그대로 둠.
pub fn pointer(response: &egui::Response) {
    if response.enabled() && response.sense.senses_click() && response.hovered() {
        response.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
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
    let size = egui::vec2(
        if primary { 112.0 } else { 0.0 },
        if primary { BUTTON } else { 36.0 },
    );
    action_sized(ui, text, primary, enabled, size)
}

/// 정한 최소 크기로 버튼을 그림. 함께 놓이는 버튼끼리 크기를 맞출 때 씀.
pub fn action_sized(
    ui: &mut egui::Ui,
    text: &str,
    primary: bool,
    enabled: bool,
    size: egui::Vec2,
) -> egui::Response {
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
                .min_size(size)
                .stroke(Stroke::new(1.0, if primary { BLUE } else { BORDER })),
            )
        })
        .inner;
    pointer(&response);
    focus(&response);
    response
}

pub const STATUS_HEIGHT: f32 = 34.0;

/// 파일 목록 행 배경. 선택한 행에 마우스를 올리면 선택 색보다 진하게,
/// 선택하지 않은 행에 마우스를 올리면 선택 색보다 옅게 칠함.
pub fn row_background(ui: &egui::Ui, rect: egui::Rect, selected: bool, hovered: bool) {
    let fill = if selected && hovered {
        ROW_SELECTED_HOVER
    } else if selected {
        ROW_SELECTED
    } else if hovered {
        ROW_HOVER
    } else {
        return;
    };
    ui.painter().rect_filled(rect, 0, fill);
}

/// 경로 줄 높이를 꽉 채우는 입력란을 그림. 모서리는 각지게 하고,
/// 위 테두리는 상단 메뉴의 구분선이, 아래 테두리는 경로 줄의 아래 선이 대신해
/// 선이 겹쳐 보이지 않게 함. 초점이 있으면 파란 테두리.
pub fn field_frame(ui: &egui::Ui, rect: egui::Rect, focused: bool) {
    let painter = ui.painter();
    painter.rect_filled(rect, 0, PANEL);
    if focused {
        painter.rect_stroke(rect, 0, Stroke::new(1.0, BLUE), egui::StrokeKind::Inside);
        return;
    }
    let stroke = Stroke::new(1.0, BORDER);
    painter.vline(rect.left() + 0.5, rect.y_range(), stroke);
    painter.vline(rect.right() - 0.5, rect.y_range(), stroke);
}

/// 네 변 테두리가 모두 있는 입력란 모양 (주변에 구분선이 없는 곳에서 씀).
pub fn boxed_field(ui: &egui::Ui, rect: egui::Rect) {
    ui.painter().rect(
        rect,
        0,
        PANEL,
        Stroke::new(1.0, BORDER),
        egui::StrokeKind::Inside,
    );
}

/// 입력란 안에 경로처럼 한 줄 글자를 그림. 넘치면 줄이고 전체 글자는 툴팁으로 보여 줌.
/// 부모 배치에 영향을 주지 않는 하위 영역을 써서 옆 칸의 위치가 흔들리지 않게 함.
pub fn field_text(ui: &mut egui::Ui, rect: egui::Rect, text: &str) {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(10.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bounded_text(&mut child, text, (rect.width() - 20.0).max(0.0));
}

/// 상태 줄 글자를 칸의 세로 가운데에 그림. 넘치면 줄이고 전체 글자는 툴팁으로 보여 줌.
pub fn status_text(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    text: &str,
    width: f32,
    align: egui::Align,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.into(),
        FontId::proportional(FONT_BODY),
        TEXT,
    );
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(0.0),
        max_rows: 1,
        ..Default::default()
    };
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let size = galley.size();
    let x = if align == egui::Align::Max {
        rect.right() - size.x
    } else {
        rect.left()
    };
    let text_rect = egui::Rect::from_min_size(egui::pos2(x, rect.center().y - size.y / 2.0), size);
    ui.painter().galley(text_rect.min, galley, TEXT);
    let response = ui.interact(
        text_rect,
        ui.scope_id()
            .with(("status-text", align == egui::Align::Max)),
        egui::Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    response.on_hover_text(text);
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

pub fn bounded_text(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    bounded_text_hover(ui, text, text, width)
}

/// 폭에 맞춰 줄인 글자를 그리고, 마우스를 올리면 hover 글자를 보여 줌.
pub fn bounded_text_hover(
    ui: &mut egui::Ui,
    text: &str,
    hover: &str,
    width: f32,
) -> egui::Response {
    let width = width.max(0.0);
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.text_style_height(&TextStyle::Body)),
        ui.layout().clone(),
        |ui| {
            ui.set_min_width(width);
            ui.set_max_width(width);
            ui.add(egui::Label::new(text).truncate())
        },
    )
    .inner
    .on_hover_text(hover)
}

pub fn window_title(ui: &egui::Ui, text: &str, width: f32) -> String {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.into(),
        FontId::proportional(FONT_BODY),
        TEXT,
    );
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        ..Default::default()
    };
    ui.fonts_mut(|fonts| fonts.layout_job(job))
        .rows
        .iter()
        .flat_map(|row| row.glyphs.iter().map(|glyph| glyph.chr))
        .collect()
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
            .layout_no_wrap(text.into(), FontId::proportional(FONT_BODY), TEXT)
            .size()
            .x
    };
    // 두 버튼은 긴 쪽 글자에 맞춘 같은 크기로 그림.
    let button = egui::vec2(
        primary
            .map_or(0.0, |(label, _)| text_width(label))
            .max(text_width(secondary))
            + 32.0,
        BUTTON,
    )
    .max(egui::vec2(112.0, BUTTON));
    let count = if primary.is_some() { 2.0 } else { 1.0 };
    let width = button.x * count + SPACE * (count - 1.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add_space((ui.available_width() - width).max(0.0));
            let secondary = action_sized(ui, secondary, false, true, button).clicked();
            let primary = primary.is_some_and(|(label, enabled)| {
                action_sized(ui, label, true, enabled, button)
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
    // 움직이는 표시 없이 전체 막대를 옅은 파란색으로 채워 진행 중임을 나타냄.
    ui.painter()
        .rect_filled(rect, 8, Color32::from_rgb(191, 211, 248));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::ProgressIndicator,
            ui.is_enabled(),
            "전체 크기를 알 수 없는 작업 진행 중",
        )
    });
}

pub const PREVIEW: &str = include_str!("assets/icons/preview-file.svg");
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
        let width = rect.width() / 18.0;
        // 꼭짓점과 끝점에 원을 더해 둥근 이음과 둥근 끝을 만듦.
        for point in &points {
            painter.circle_filled(*point, width / 2.0, color);
        }
        painter.add(egui::Shape::line(points, Stroke::new(width, color)));
    }
}

/// 가장 긴 이름(압축해제, FONT_SMALL에서 약 48px) 양옆에 12px씩 남는 폭.
pub const TOOL_SIZE: egui::Vec2 = egui::vec2(72.0, 60.0);
const TOOL_ICON: f32 = 30.0;
const TOOL_LABEL: f32 = FONT_SMALL;
/// 아이콘 그림 아래와 이름 글자 위 사이의 보이는 간격.
const TOOL_GAP: f32 = 6.0;

/// SVG 폴리라인 점들의 세로 범위(24 단위 좌표).
fn svg_y_range(asset: &str) -> Option<(f32, f32)> {
    asset
        .split("points=\"")
        .skip(1)
        .flat_map(|element| {
            element
                .split('"')
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .filter_map(|pair| pair.split_once(',')?.1.parse::<f32>().ok())
                .collect::<Vec<_>>()
        })
        .fold(None, |range: Option<(f32, f32)>, y| {
            Some(range.map_or((y, y), |(lo, hi)| (lo.min(y), hi.max(y))))
        })
}

/// 상단 메뉴 아이콘 세 개를 합친 그림 범위. 모든 버튼이 같은 기준을 써서 이름 높이가 맞음.
fn tool_icon_ink() -> (f32, f32) {
    let scale = TOOL_ICON / 24.0;
    let half_stroke = TOOL_ICON / 18.0 / 2.0;
    let (lo, hi) = [OPEN, CREATE, EXTRACT]
        .iter()
        .filter_map(|asset| svg_y_range(asset))
        .fold((24.0f32, 0.0f32), |(lo, hi), (a, b)| (lo.min(a), hi.max(b)));
    (lo * scale - half_stroke, hi * scale + half_stroke)
}

/// 글자 줄 상자가 아니라 실제로 그려지는 글자 모양의 위아래 범위.
fn galley_ink(galley: &egui::Galley) -> (f32, f32) {
    galley
        .rows
        .iter()
        .flat_map(|row| {
            row.glyphs
                .iter()
                .filter(|g| g.uv_rect.size.y > 0.0)
                .map(move |g| {
                    let top = row.pos.y + g.pos.y + g.uv_rect.offset.y;
                    (top, top + g.uv_rect.size.y)
                })
        })
        .fold(None, |range: Option<(f32, f32)>, (top, bottom)| {
            Some(range.map_or((top, bottom), |(a, b)| (a.min(top), b.max(bottom))))
        })
        .unwrap_or((0.0, galley.size().y))
}

pub fn tool(
    ui: &mut egui::Ui,
    label: &str,
    asset: &str,
    enabled: bool,
    blue: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        TOOL_SIZE,
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let color = if enabled {
        if blue { BLUE } else { TEXT }
    } else {
        MUTED
    };
    if enabled && (response.hovered() || response.has_focus()) {
        let fill = if response.is_pointer_button_down_on() {
            PRESS
        } else {
            HOVER
        };
        ui.painter().rect_filled(rect, 8, fill);
    }
    // 아이콘 그림 위쪽부터 글자 모양 아래쪽까지를 하나로 보고 버튼 가운데에 둠.
    // 그래서 마우스를 올려 배경이 보일 때 위아래 여백이 같음.
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), FontId::proportional(TOOL_LABEL), color);
    let (icon_top, icon_bottom) = tool_icon_ink();
    let (text_top, text_bottom) = galley_ink(&galley);
    let block = (icon_bottom - icon_top) + TOOL_GAP + (text_bottom - text_top);
    let start = rect.top() + (rect.height() - block) / 2.0;
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.center().x - TOOL_ICON / 2.0, start - icon_top),
        egui::vec2(TOOL_ICON, TOOL_ICON),
    );
    svg_icon(ui.painter(), icon_rect, asset, color);
    let text_y = start + (icon_bottom - icon_top) + TOOL_GAP - text_top;
    ui.painter().galley(
        egui::pos2(rect.center().x - galley.size().x / 2.0, text_y).round(),
        galley,
        color,
    );
    pointer(&response);
    focus(&response);
    response
}

/// 테두리와 배경 없이 아이콘만 그리는 버튼. 사용할 수 있을 때만 마우스를 올리면 배경이 어두워짐.
pub fn flat_icon_button(
    ui: &mut egui::Ui,
    name: &str,
    asset: &str,
    enabled: bool,
    size: egui::Vec2,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let response = response.on_hover_text(name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    if enabled && response.hovered() {
        let fill = if response.is_pointer_button_down_on() {
            PRESS
        } else {
            HOVER
        };
        ui.painter().rect_filled(rect, 6, fill);
    }
    svg_icon(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), egui::vec2(20.0, 20.0)),
        asset,
        if enabled { TEXT } else { MUTED },
    );
    pointer(&response);
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
    pointer(&response);
    focus(&response);
    response
}
