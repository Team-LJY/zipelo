//! GNOME Wayland는 창 테두리를 그려 주지 않으므로 제목 표시줄과 테두리를 직접 그림.
//! 앱 글꼴로 그려서 한글 제목도 그대로 표시됨.

use crate::archive_theme as theme;
use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Rect, ResizeDirection, Sense, Stroke, Vec2,
    ViewportCommand,
};

pub const TITLE_HEIGHT: f32 = 38.0;
const BUTTON_SIZE: Vec2 = egui::vec2(34.0, 26.0);
const RESIZE_EDGE: f32 = 6.0;
const DRAG_THRESHOLD: f32 = 4.0;
const TITLE_BG: Color32 = Color32::from_rgb(235, 238, 242);

/// 창 테두리를 직접 그려야 하는지 판단함.
/// winit은 Linux에서 Wayland 연결 정보가 있으면 Wayland로 창을 만듦.
pub fn custom_frame_needed() -> bool {
    cfg!(target_os = "linux")
        && ["WAYLAND_DISPLAY", "WAYLAND_SOCKET"]
            .iter()
            .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}

#[derive(Clone, Copy)]
pub struct Buttons {
    pub minimize: bool,
    pub maximize: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Glyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// 위쪽 패널로 제목 표시줄을 그림. 버튼과 끌기 동작은 현재 창에 명령으로 보냄.
/// 창 이동을 운영체제에 넘긴 프레임이면 true를 돌려줌.
pub fn title_bar(root: &mut egui::Ui, id: &str, title: &str, buttons: Buttons) -> bool {
    let mut moved = false;
    egui::Panel::top(egui::Id::unique(("window-title", id)))
        .exact_size(TITLE_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(TITLE_BG))
        .show(root, |ui| {
            let rect = ui.max_rect();
            let ctx = ui.ctx().clone();
            let bar = ui.interact(
                rect,
                egui::Id::unique(("window-title-drag", id)),
                Sense::click_and_drag(),
            );
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            if buttons.maximize && bar.double_clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            }

            let mut glyphs = vec![Glyph::Close];
            if buttons.maximize {
                glyphs.push(if maximized {
                    Glyph::Restore
                } else {
                    Glyph::Maximize
                });
            }
            if buttons.minimize {
                glyphs.push(Glyph::Minimize);
            }
            let button_count = glyphs.len() as f32;
            let buttons_left = rect.right() - 6.0 - button_count * (BUTTON_SIZE.x + 4.0);
            if start_move(&ctx, id, rect, buttons_left) {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                moved = true;
            }
            let mut right = rect.right() - 6.0;
            for glyph in glyphs {
                let button = Rect::from_center_size(
                    egui::pos2(right - BUTTON_SIZE.x / 2.0, rect.center().y),
                    BUTTON_SIZE,
                );
                right -= BUTTON_SIZE.x + 4.0;
                if window_button(ui, id, button, glyph).clicked() {
                    ctx.send_viewport_cmd(match glyph {
                        Glyph::Minimize => ViewportCommand::Minimized(true),
                        Glyph::Maximize => ViewportCommand::Maximized(true),
                        Glyph::Restore => ViewportCommand::Maximized(false),
                        Glyph::Close => ViewportCommand::Close,
                    });
                }
            }

            // 버튼 영역을 피해 가운데에 제목을 그림.
            let reserved = rect.right() - right;
            let width = (rect.width() - 2.0 * reserved).max(0.0);
            let font = FontId::proportional(theme::FONT_SMALL);
            let text = fit(ui, title, &font, width);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                text,
                font,
                theme::TEXT,
            );
            ui.painter().hline(
                rect.x_range(),
                rect.bottom() - 0.5,
                Stroke::new(1.0, theme::BORDER),
            );
        });
    moved
}

/// 제목 표시줄을 누른 채 움직였는지를 원시 입력으로 판단함.
/// 창 이동 중에는 운영체제가 마우스를 가져가서 버튼 놓기가 앱에 오지 않으므로,
/// egui의 끌기 상태에 기대면 다음 끌기가 시작되지 않음.
fn start_move(ctx: &egui::Context, id: &str, bar: Rect, buttons_left: f32) -> bool {
    let key = egui::Id::unique(("window-title-press", id));
    let mut origin: Option<egui::Pos2> = ctx.data(|d| d.get_temp(key)).flatten();
    let latest = ctx.input(|i| {
        for event in &i.raw.events {
            if let egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                ..
            } = event
            {
                origin = (*pressed && bar.contains(*pos) && pos.x < buttons_left).then_some(*pos);
            }
        }
        i.pointer.latest_pos()
    });
    let start = origin
        .zip(latest)
        .is_some_and(|(origin, latest)| (latest - origin).length() >= DRAG_THRESHOLD);
    if start {
        origin = None;
        // 운영체제가 이동을 맡으므로 egui 쪽 끌기는 끝낸 것으로 처리함.
        ctx.stop_dragging();
    }
    ctx.data_mut(|d| d.insert_temp(key, origin));
    start
}

fn window_button(ui: &mut egui::Ui, id: &str, rect: Rect, glyph: Glyph) -> egui::Response {
    let label = match glyph {
        Glyph::Minimize => "최소화",
        Glyph::Maximize => "최대화",
        Glyph::Restore => "이전 크기로",
        Glyph::Close => "닫기",
    };
    let response = ui
        .interact(
            rect,
            egui::Id::unique(("window-button", id, label)),
            Sense::click(),
        )
        .on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let painter = ui.painter();
    if response.hovered() || response.is_pointer_button_down_on() {
        let fill = if glyph == Glyph::Close {
            Color32::from_rgb(232, 17, 35)
        } else {
            theme::BORDER
        };
        painter.rect_filled(rect, 6.0, fill);
    }
    let color = if glyph == Glyph::Close && response.hovered() {
        Color32::WHITE
    } else {
        theme::TEXT
    };
    let stroke = Stroke::new(1.4, color);
    let c = rect.center();
    let r = 5.0;
    match glyph {
        Glyph::Minimize => {
            painter.hline(c.x - r..=c.x + r, c.y + r * 0.6, stroke);
        }
        Glyph::Maximize => {
            painter.rect_stroke(
                Rect::from_center_size(c, egui::vec2(2.0 * r, 2.0 * r)),
                1.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Glyph::Restore => {
            let back = Rect::from_center_size(c + egui::vec2(1.5, -1.5), egui::vec2(8.0, 8.0));
            let front = Rect::from_center_size(c + egui::vec2(-1.5, 1.5), egui::vec2(8.0, 8.0));
            painter.rect_stroke(back, 1.0, stroke, egui::StrokeKind::Middle);
            painter.rect_filled(front, 1.0, TITLE_BG);
            painter.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Middle);
        }
        Glyph::Close => {
            painter.line_segment([c + egui::vec2(-r, -r), c + egui::vec2(r, r)], stroke);
            painter.line_segment([c + egui::vec2(-r, r), c + egui::vec2(r, -r)], stroke);
        }
    }
    response
}

/// 폭을 넘는 제목은 앞부분을 줄이고 끝(파일 이름과 앱 이름)을 남김.
fn fit(ui: &egui::Ui, title: &str, font: &FontId, width: f32) -> String {
    let measure = |text: &str| {
        ui.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(text.to_owned(), font.clone(), theme::TEXT)
                .size()
                .x
        })
    };
    if measure(title) <= width {
        return title.to_owned();
    }
    let chars: Vec<char> = title.chars().collect();
    for start in 1..chars.len() {
        let candidate = format!("…{}", chars[start..].iter().collect::<String>());
        if measure(&candidate) <= width {
            return candidate;
        }
    }
    String::new()
}

/// 창 가장자리에서 크기 조절을 시작하고 얇은 테두리를 그림.
pub fn edges(ctx: &egui::Context, resizable: bool) {
    let rect = ctx.content_rect();
    let maximized = ctx.input(|i| {
        i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
    });
    if !maximized {
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("window-border"),
        ))
        .rect_stroke(
            rect.shrink(0.5),
            0.0,
            Stroke::new(1.0, theme::BORDER),
            egui::StrokeKind::Middle,
        );
    }
    if !resizable || maximized {
        return;
    }
    let Some(pointer) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let left = pointer.x - rect.left() <= RESIZE_EDGE;
    let right = rect.right() - pointer.x <= RESIZE_EDGE;
    let top = pointer.y - rect.top() <= RESIZE_EDGE;
    let bottom = rect.bottom() - pointer.y <= RESIZE_EDGE;
    let direction = match (left, right, top, bottom) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (_, true, true, _) => ResizeDirection::NorthEast,
        (true, _, _, true) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, ..) => ResizeDirection::West,
        (_, true, ..) => ResizeDirection::East,
        (_, _, true, _) => ResizeDirection::North,
        (_, _, _, true) => ResizeDirection::South,
        _ => return,
    };
    ctx.set_cursor_icon(match direction {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    });
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
    }
}
