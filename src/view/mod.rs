pub mod editor;
pub mod find;
pub mod highlight;
pub mod icons;
pub mod img_preview;
pub mod img_view;
pub mod incr;
pub mod md_hl;
pub mod outline;
pub mod pdf;
pub mod preview;
pub mod word;
pub mod sync;
pub mod text_sel;
pub mod theme;

use eframe::egui::{self, Key, Modifiers};

/// 预览 / 编辑区滚轮倍率。
pub const WHEEL_SCROLL_MULT: f32 = 2.5;
pub const ZOOM_MIN: f32 = 0.25;
pub const ZOOM_MAX: f32 = 4.0;
const ZOOM_STEP: f32 = 1.1;

/// Ctrl+滚轮 / Ctrl++ / Ctrl+- / Ctrl+0。指针在区域内才生效，并吃掉滚轮以免带动滚动。
pub fn ctrl_zoom(ui: &mut egui::Ui, zoom: &mut f32) -> bool {
    if !ui.rect_contains_pointer(ui.max_rect()) {
        return false;
    }
    let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
    if !ctrl {
        return false;
    }
    let mut z = *zoom;
    let mut changed = false;
    ui.input_mut(|i| {
        let dy = i.raw_scroll_delta.y;
        if dy.abs() > 0.1 {
            if dy > 0.0 {
                z *= ZOOM_STEP;
            } else {
                z /= ZOOM_STEP;
            }
            i.raw_scroll_delta = egui::Vec2::ZERO;
            i.smooth_scroll_delta = egui::Vec2::ZERO;
            changed = true;
        }
        let plus = i.consume_key(Modifiers::CTRL, Key::Equals)
            || i.consume_key(Modifiers::COMMAND, Key::Equals)
            || i.consume_key(Modifiers::CTRL, Key::Plus)
            || i.consume_key(Modifiers::COMMAND, Key::Plus);
        let minus = i.consume_key(Modifiers::CTRL, Key::Minus)
            || i.consume_key(Modifiers::COMMAND, Key::Minus);
        let reset = i.consume_key(Modifiers::CTRL, Key::Num0)
            || i.consume_key(Modifiers::COMMAND, Key::Num0);
        if plus {
            z *= ZOOM_STEP;
            changed = true;
        }
        if minus {
            z /= ZOOM_STEP;
            changed = true;
        }
        if reset {
            z = 1.0;
            changed = true;
        }
    });
    z = z.clamp(ZOOM_MIN, ZOOM_MAX);
    if changed {
        *zoom = z;
    }
    changed
}

pub fn content_scroll(vertical_only: bool) -> egui::ScrollArea {
    let sa = if vertical_only {
        egui::ScrollArea::vertical()
    } else {
        egui::ScrollArea::both()
    };
    sa.auto_shrink([false, false])
        // Ctrl+A / 跳转 / 后退前进：立刻到位，不要滑过去。
        .animated(false)
        .scroll_source(egui::containers::scroll_area::ScrollSource {
            scroll_bar: true,
            drag: false,
            mouse_wheel: true,
        })
        .wheel_scroll_multiplier(egui::vec2(1.0, WHEEL_SCROLL_MULT))
}

/// 方向键按行滚动；PgUp/PgDn 翻页（对齐 docview）。编辑框有焦点时不抢键。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyNav {
    None,
    /// `scroll_with_delta`：正 Y 把内容往下推（箭头向上）。
    Line(egui::Vec2),
    /// -1 上一页 / +1 下一页。
    Page(i32),
}

const KEY_LINE: f32 = 36.0;

pub fn consume_key_nav(ui: &mut egui::Ui) -> KeyNav {
    if ui.ctx().wants_keyboard_input() {
        return KeyNav::None;
    }
    if ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.alt) {
        return KeyNav::None;
    }
    ui.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::PageDown) {
            return KeyNav::Page(1);
        }
        if i.consume_key(Modifiers::NONE, Key::PageUp) {
            return KeyNav::Page(-1);
        }
        let mut d = egui::Vec2::ZERO;
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            d.y -= KEY_LINE;
        }
        if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            d.y += KEY_LINE;
        }
        if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
            d.x += KEY_LINE;
        }
        if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
            d.x -= KEY_LINE;
        }
        if d == egui::Vec2::ZERO {
            KeyNav::None
        } else {
            KeyNav::Line(d)
        }
    })
}

pub fn apply_key_nav_scroll(ui: &mut egui::Ui, nav: KeyNav, page_h: Option<f32>) {
    match nav {
        KeyNav::None => {}
        KeyNav::Line(d) => ui.scroll_with_delta_animation(d, egui::style::ScrollAnimation::none()),
        KeyNav::Page(dir) => {
            let h = page_h.unwrap_or_else(|| (ui.clip_rect().height() * 0.9).max(120.0));
            ui.scroll_with_delta_animation(
                egui::vec2(0.0, -(dir as f32) * h),
                egui::style::ScrollAnimation::none(),
            );
        }
    }
}

/// 拖选文字时 egui 因 `dragged_id` 忽略滚轮，在 ScrollArea 内容里补一次。
pub fn wheel_while_dragging(ui: &mut egui::Ui) {
    if ui.ctx().dragged_id().is_none() {
        return;
    }
    if !ui.rect_contains_pointer(ui.clip_rect()) {
        return;
    }
    let delta = ui.input(|i| i.smooth_scroll_delta);
    if delta == egui::Vec2::ZERO {
        return;
    }
    let delta = egui::vec2(delta.x, delta.y * WHEEL_SCROLL_MULT);
    ui.scroll_with_delta_animation(delta, egui::style::ScrollAnimation::none());
}

/// 占满父级剩余区域，滚动条才能贴到容器边缘。
pub fn pane_ui(ui: &mut egui::Ui) -> egui::Ui {
    let rect = ui.available_rect_before_wrap();
    ui.advance_cursor_after_rect(rect);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("pane")
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child
}
