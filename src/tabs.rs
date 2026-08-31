use egui::{Color32, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};

use crate::doc::Tab;

pub enum TabBarEvent {
    Select(usize),
    Close(usize),
    CloseOthers(usize),
    CloseAll,
    Reorder { from: usize, to: usize },
    TearOff(usize),
    OpenAsWorkspace(usize),
    DragStart { idx: usize, grab: Vec2 },
}

pub struct TabBarGeom {
    pub bar_rect: Rect,
    pub chips: Vec<Rect>,
}

/// 按芯片中点算插入下标；`exclude` 为正在拖的芯片（仍占位）。
pub fn insert_index(chip_mids: &[f32], exclude: Option<usize>, x: f32) -> usize {
    for (i, &mid) in chip_mids.iter().enumerate() {
        if Some(i) == exclude {
            continue;
        }
        if x < mid {
            return i;
        }
    }
    chip_mids.len()
}

/// 插入下标（含自身）→ 移除自身后的目标下标。
pub fn adj_insert(old: usize, insert: usize) -> usize {
    if insert > old {
        insert - 1
    } else {
        insert
    }
}

/// 标签栏：点击切换、中键关闭、拖动排序、关闭按钮。
/// `dragging_id` 为正在拖的标签；`ghost_x` 为视口坐标下幽灵标签左缘（仅条内跟手）。
pub fn show(
    ui: &mut egui::Ui,
    tabs: &[Tab],
    active: usize,
    dragging_id: Option<u64>,
    ghost_x: Option<f32>,
) -> (Option<TabBarEvent>, TabBarGeom) {
    let mut event = None;
    let bar_rect = ui.max_rect();
    let mut chips = Vec::with_capacity(tabs.len());
    let drag_from = dragging_id.and_then(|id| tabs.iter().position(|t| t.id == id));
    egui::ScrollArea::horizontal()
        .id_salt("tabbar_scroll")
        .auto_shrink([false, true])
        .scroll_source(egui::containers::scroll_area::ScrollSource {
            scroll_bar: true,
            drag: false,
            mouse_wheel: true,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, tab) in tabs.iter().enumerate() {
                    let dim = Some(tab.id) == dragging_id;
                    let r = tab_chip(ui, tab, i == active, dim);
                    chips.push(r.rect);
                    if r.clicked() {
                        event = Some(TabBarEvent::Select(i));
                    }
                    if r.middle_clicked() {
                        event = Some(TabBarEvent::Close(i));
                    }
                    if close_x(ui, tab.id, r.rect) {
                        event = Some(TabBarEvent::Close(i));
                    }
                    if r.drag_started() && event.is_none() {
                        let grab = r
                            .interact_pointer_pos()
                            .map(|p| p - r.rect.min)
                            .unwrap_or(Vec2::new(r.rect.width() * 0.5, r.rect.height() * 0.5));
                        event = Some(TabBarEvent::DragStart { idx: i, grab });
                    }
                    r.context_menu(|ui| {
                        let has_dir = tab
                            .doc
                            .path
                            .as_ref()
                            .and_then(|p| p.parent())
                            .is_some_and(|d| !d.as_os_str().is_empty());
                        if ui.button(crate::i18n::t().close).clicked() {
                            event = Some(TabBarEvent::Close(i));
                            ui.close();
                        }
                        if ui.button(crate::i18n::t().close_others).clicked() {
                            event = Some(TabBarEvent::CloseOthers(i));
                            ui.close();
                        }
                        if ui.button(crate::i18n::t().close_all).clicked() {
                            event = Some(TabBarEvent::CloseAll);
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(has_dir, egui::Button::new(crate::i18n::t().open_as_workspace))
                            .clicked()
                        {
                            event = Some(TabBarEvent::OpenAsWorkspace(i));
                            ui.close();
                        }
                        if ui.button(crate::i18n::t().move_to_new_window).clicked() {
                            event = Some(TabBarEvent::TearOff(i));
                            ui.close();
                        }
                    });
                }
                if let Some(from) = drag_from {
                    if let Some(ptr) = ui.ctx().pointer_latest_pos() {
                        let mids: Vec<f32> = chips.iter().map(|c| c.center().x).collect();
                        let insert = insert_index(&mids, Some(from), ptr.x);
                        let to = adj_insert(from, insert);
                        if to != from && event.is_none() {
                            event = Some(TabBarEvent::Reorder { from, to });
                        }
                    }
                }
            });
        });
    if let (Some(from), Some(gx)) = (drag_from, ghost_x) {
        if let Some(chip) = chips.get(from) {
            paint_ghost(ui, *chip, &tabs[from].title(), gx);
        }
    }
    (event, TabBarGeom { bar_rect, chips })
}

fn tab_chip(ui: &mut egui::Ui, tab: &Tab, selected: bool, dim: bool) -> egui::Response {
    let title = tab.title();
    let galley = ui.painter().layout_no_wrap(
        title,
        egui::FontId::proportional(13.0),
        ui.visuals().text_color(),
    );
    let text_w = galley.size().x.min(160.0).max(48.0);
    let size = Vec2::new(text_w + 28.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    if ui.is_rect_visible(rect) {
        paint_chip_visual(ui, rect, &galley, selected, dim);
    }
    response.on_hover_text(match &tab.doc.path {
        Some(p) => p.display().to_string(),
        None => crate::i18n::t().unsaved.to_string(),
    })
}

fn paint_chip_visual(
    ui: &mut egui::Ui,
    rect: Rect,
    galley: &std::sync::Arc<egui::Galley>,
    selected: bool,
    dim: bool,
) {
    let mut fill = if selected {
        ui.visuals().extreme_bg_color
    } else {
        ui.visuals().faint_bg_color
    };
    let mut stroke_color = if selected {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().widgets.inactive.bg_stroke.color
    };
    let mut text_color = ui.visuals().text_color();
    if dim {
        fill = fill.gamma_multiply(0.35);
        stroke_color = stroke_color.gamma_multiply(0.35);
        text_color = text_color.gamma_multiply(0.35);
    }
    ui.painter().rect(
        rect,
        3.0,
        fill,
        Stroke::new(1.0_f32, stroke_color),
        egui::StrokeKind::Inside,
    );
    let text_pos = Pos2::new(rect.left() + 8.0, rect.center().y - galley.size().y * 0.5);
    let text_clip = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, rect.top()),
        Pos2::new(rect.right() - 18.0, rect.bottom()),
    );
    ui.painter()
        .with_clip_rect(text_clip)
        .galley(text_pos, galley.clone(), text_color);
    if selected && !dim {
        ui.painter().line_segment(
            [
                Pos2::new(rect.left() + 1.0, rect.bottom() - 1.0),
                Pos2::new(rect.right() - 1.0, rect.bottom() - 1.0),
            ],
            Stroke::new(2.0_f32, stroke_color),
        );
    }
}

fn paint_ghost(ui: &mut egui::Ui, slot: Rect, title: &str, ghost_x: f32) {
    let size = slot.size();
    let pos = Pos2::new(ghost_x, slot.min.y - 2.0);
    egui::Area::new(egui::Id::new("tab_drag_ghost"))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .interactable(false)
        .constrain(false)
        .show(ui.ctx(), |ui| {
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let text_color = ui.visuals().text_color();
            let galley =
                ui.painter()
                    .layout_no_wrap(title.to_string(), egui::FontId::proportional(13.0), text_color);
            ui.painter().rect(
                rect.translate(Vec2::new(2.0, 3.0)),
                3.0,
                Color32::from_black_alpha(40),
                Stroke::NONE,
                egui::StrokeKind::Inside,
            );
            paint_chip_visual(ui, rect, &galley, true, false);
        });
}

fn close_x(ui: &mut egui::Ui, tab_id: u64, tab_rect: Rect) -> bool {
    let x_rect = Rect::from_center_size(
        Pos2::new(tab_rect.right() - 10.0, tab_rect.center().y),
        Vec2::splat(14.0),
    );
    let id = ui.id().with("tab_x").with(tab_id);
    let r = ui.interact(x_rect, id, Sense::click());
    let color = if r.hovered() {
        Color32::from_rgb(220, 80, 80)
    } else {
        ui.visuals().weak_text_color()
    };
    let p = ui.painter();
    let c = x_rect.center();
    p.line_segment(
        [c + Vec2::new(-3.5, -3.5), c + Vec2::new(3.5, 3.5)],
        Stroke::new(1.2_f32, color),
    );
    p.line_segment(
        [c + Vec2::new(-3.5, 3.5), c + Vec2::new(3.5, -3.5)],
        Stroke::new(1.2_f32, color),
    );
    r.clicked_by(PointerButton::Primary)
}

#[cfg(test)]
mod tests {
    use super::{adj_insert, insert_index};

    #[test]
    fn insert_skips_dragged_and_stays() {
        let mids = [50.0, 150.0, 250.0];
        let ins = insert_index(&mids, Some(1), 80.0);
        assert_eq!(ins, 2);
        assert_eq!(adj_insert(1, ins), 1);
    }

    #[test]
    fn insert_left_of_first() {
        let mids = [50.0, 150.0, 250.0];
        let ins = insert_index(&mids, Some(1), 40.0);
        assert_eq!(ins, 0);
        assert_eq!(adj_insert(1, ins), 0);
    }

    #[test]
    fn insert_past_last() {
        let mids = [50.0, 150.0, 250.0];
        let ins = insert_index(&mids, Some(1), 300.0);
        assert_eq!(ins, 3);
        assert_eq!(adj_insert(1, ins), 2);
    }
}
