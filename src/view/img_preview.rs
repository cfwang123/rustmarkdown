use egui::{
    pos2, vec2, Align, Color32, Context, Key, Layout, Modifiers, Order, Rect, RichText, Sense,
    Stroke, Vec2,
};

use crate::io::imgcache::Raster;

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 32.0;
const WHEEL: f32 = 1.12;

/// 文档区图片弹层（对齐 docview ImageOverlay：半透明底、适合区域、滚轮缩放、拖拽平移）。
pub struct ImgPreview {
    pub title: String,
    pub raster: Raster,
    zoom: f32,
    pan: Vec2,
    fitted: bool,
}

pub enum OverlayAction {
    Close,
    CopyImage,
    CopyAsFile,
}

impl ImgPreview {
    pub fn new(title: String, raster: Raster) -> Self {
        Self {
            title,
            raster,
            zoom: 1.0,
            pan: Vec2::ZERO,
            fitted: false,
        }
    }
}

pub fn show(ctx: &Context, st: &mut ImgPreview) -> Option<OverlayAction> {
    let mut action = None;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("img_preview_overlay"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_min_size(screen.size());
            ui.set_max_size(screen.size());
            let dim = Color32::from_rgba_unmultiplied(15, 15, 18, 0xE0);
            ui.painter().rect_filled(screen, 0.0, dim);

            let mut close = false;
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                close = true;
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::C)) {
                action = Some(OverlayAction::CopyImage);
            }

            let bot_h = 44.0;
            let bot = Rect::from_min_max(pos2(screen.min.x, screen.max.y - bot_h), screen.max);
            let stage = Rect::from_min_max(screen.min, pos2(screen.max.x, screen.max.y - bot_h));
            let close_r = Rect::from_center_size(
                pos2(screen.max.x - 22.0, screen.min.y + 20.0),
                vec2(28.0, 22.0),
            );

            let pad = 12.0;
            let avail = Vec2::new(
                (stage.width() - pad * 2.0).max(40.0),
                (stage.height() - pad * 2.0).max(40.0),
            );
            if !st.fitted {
                let px = st.raster.size;
                if px.x > 1.0 && px.y > 1.0 {
                    st.zoom = (avail.x / px.x)
                        .min(avail.y / px.y)
                        .clamp(MIN_ZOOM, MAX_ZOOM);
                }
                st.pan = Vec2::ZERO;
                st.fitted = true;
            }

            let disp = st.raster.size * st.zoom;
            let center = stage.center() + st.pan;
            let img_rect = Rect::from_center_size(center, disp);
            let hit = img_rect.intersect(stage);

            ui.painter().with_clip_rect(stage).image(
                st.raster.tex.id(),
                img_rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );

            let bg_id = ui.allocate_rect(stage, Sense::click_and_drag());
            let img_id = if hit.width() > 1.0 && hit.height() > 1.0 {
                Some(ui.allocate_rect(hit, Sense::click_and_drag()))
            } else {
                None
            };

            let img_dragged = img_id.as_ref().map(|r| r.dragged()).unwrap_or(false);
            if img_dragged {
                st.pan += img_id.as_ref().unwrap().drag_delta();
            } else if bg_id.dragged() {
                st.pan += bg_id.drag_delta();
            }

            let hovered = bg_id.hovered() || img_id.as_ref().map(|r| r.hovered()).unwrap_or(false);
            let scroll_y = ui.input(|i| i.raw_scroll_delta.y);
            if hovered && scroll_y.abs() > 0.1 {
                let f = if scroll_y > 0.0 { WHEEL } else { 1.0 / WHEEL };
                st.zoom = (st.zoom * f).clamp(MIN_ZOOM, MAX_ZOOM);
            }
            ui.input_mut(|i| {
                i.raw_scroll_delta = Vec2::ZERO;
                i.smooth_scroll_delta = Vec2::ZERO;
            });

            if let Some(img_id) = &img_id {
                img_id.context_menu(|ui| {
                    if ui.button("复制图片").clicked() {
                        action = Some(OverlayAction::CopyImage);
                        ui.close();
                    }
                    if ui.button("复制为文件").clicked() {
                        action = Some(OverlayAction::CopyAsFile);
                        ui.close();
                    }
                });
            }

            let chrome = Color32::from_rgba_unmultiplied(0, 0, 0, 0xB0);
            ui.painter().rect_filled(bot, 0.0, chrome);

            ui.scope_builder(egui::UiBuilder::new().max_rect(close_r), |ui| {
                let tip = format!("关闭 (Esc) · {}", st.title);
                if close_btn(ui, &tip).clicked() {
                    close = true;
                }
            });

            ui.scope_builder(egui::UiBuilder::new().max_rect(bot), |ui| {
                ui.allocate_ui_with_layout(
                    bot.size(),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.add_space(8.0);
                        if ui.button("－").on_hover_text("缩小").clicked() {
                            st.zoom = (st.zoom / WHEEL).clamp(MIN_ZOOM, MAX_ZOOM);
                        }
                        ui.label(
                            RichText::new(format!("{:.0}%", st.zoom * 100.0))
                                .color(Color32::from_rgb(0xD1, 0xD5, 0xDB))
                                .size(12.0),
                        );
                        if ui.button("＋").on_hover_text("放大").clicked() {
                            st.zoom = (st.zoom * WHEEL).clamp(MIN_ZOOM, MAX_ZOOM);
                        }
                        if ui.button("适合").on_hover_text("适合区域").clicked() {
                            st.fitted = false;
                        }
                        if ui.button("1:1").on_hover_text("原始大小").clicked() {
                            st.zoom = 1.0;
                            st.pan = Vec2::ZERO;
                        }
                        if ui.button("复制图片").on_hover_text("Ctrl+C").clicked() {
                            action = Some(OverlayAction::CopyImage);
                        }
                        if ui.button("复制为文件").clicked() {
                            action = Some(OverlayAction::CopyAsFile);
                        }
                    },
                );
            });

            if bg_id.clicked()
                && !img_id.as_ref().map(|r| r.hovered()).unwrap_or(false)
                && !bg_id.dragged()
                && ui
                    .input(|i| i.pointer.interact_pos())
                    .map_or(true, |p| !close_r.contains(p))
            {
                close = true;
            }

            if close {
                action = Some(OverlayAction::Close);
            }
        });
    action
}

fn close_btn(ui: &mut egui::Ui, tip: &str) -> egui::Response {
    let size = vec2(28.0, 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let vis = ui.style().interact(&resp);
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect.shrink(1.0), 3.0, vis.weak_bg_fill);
    } else {
        ui.painter().rect_filled(
            rect.shrink(1.0),
            3.0,
            Color32::from_rgba_unmultiplied(0, 0, 0, 0x90),
        );
    }
    let s = Stroke::new(1.6_f32, Color32::from_rgb(0xE5, 0xE7, 0xEB));
    let r = rect.shrink(7.0);
    ui.painter()
        .line_segment([r.left_top(), r.right_bottom()], s);
    ui.painter()
        .line_segment([r.right_top(), r.left_bottom()], s);
    resp.on_hover_text(tip)
}

/// 预览区缩略图：双击打开弹层，右键复制。
pub fn interact_thumb(resp: &egui::Response) -> ThumbAction {
    let mut act = ThumbAction::None;
    if resp.double_clicked() {
        act = ThumbAction::Preview;
    }
    resp.context_menu(|ui| {
        if ui.button("复制图片").clicked() {
            act = ThumbAction::CopyImage;
            ui.close();
        }
        if ui.button("复制为文件").clicked() {
            act = ThumbAction::CopyFile;
            ui.close();
        }
    });
    act
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThumbAction {
    None,
    Preview,
    CopyImage,
    CopyFile,
}
