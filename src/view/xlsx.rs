//! XLS/XLSX 只读网格（对齐 docview VirtualSheetGrid：虚拟绘制、表头冻结、工作表页签）。

use egui::{
    pos2, vec2, Align, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, TextFormat, Ui,
    UiBuilder, Vec2,
};

use crate::io::xlsx::{col_name, CellAddr, Sheet, Workbook};

const HDR_H: f32 = 26.0;
const ROW_HDR_W: f32 = 48.0;
const TABS_H: f32 = 28.0;
const ZOOM_MIN: f32 = 0.7;
const ZOOM_MAX: f32 = 2.0;

const BG: Color32 = Color32::WHITE;
const HDR_BG: Color32 = Color32::from_rgb(0xF3, 0xF4, 0xF6);
const HDR_FG: Color32 = Color32::from_rgb(0x37, 0x41, 0x51);
const GRID: Color32 = Color32::from_rgb(0xD1, 0xD5, 0xDB);
const SEL: Color32 = Color32::from_rgb(0xDB, 0xEA, 0xFE);
const SEL_BD: Color32 = Color32::from_rgb(0x25, 0x63, 0xEB);
const FIND: Color32 = Color32::from_rgb(0xFE, 0xF0, 0x8A);
const TAB_BG: Color32 = Color32::from_rgb(0xE5, 0xE7, 0xEB);
const TEXT: Color32 = Color32::from_rgb(0x11, 0x18, 0x27);

#[derive(Clone, Copy, Debug)]
pub struct Sel {
    pub r0: i32,
    pub c0: i32,
    pub r1: i32,
    pub c1: i32,
}

impl Sel {
    fn none() -> Self {
        Self {
            r0: -1,
            c0: -1,
            r1: -1,
            c1: -1,
        }
    }

    fn is_none(self) -> bool {
        self.r0 < 0
    }

    fn norm(self) -> (usize, usize, usize, usize) {
        let r0 = self.r0.min(self.r1).max(0) as usize;
        let r1 = self.r0.max(self.r1).max(0) as usize;
        let c0 = self.c0.min(self.c1).max(0) as usize;
        let c1 = self.c0.max(self.c1).max(0) as usize;
        (r0, c0, r1, c1)
    }

    fn contains(self, r: usize, c: usize) -> bool {
        if self.is_none() {
            return false;
        }
        let (r0, c0, r1, c1) = self.norm();
        r >= r0 && r <= r1 && c >= c0 && c <= c1
    }
}

pub enum XlsxAction {
    None,
    CopyText(String),
}

pub struct XlsxSession {
    pub book: Workbook,
    pub sheet_i: usize,
    pub zoom: f32,
    sel: Sel,
    anchor: (i32, i32),
    dragging: bool,
    pending_cell: Option<(usize, usize)>,
}

impl XlsxSession {
    pub fn new(book: Workbook) -> Self {
        Self {
            book,
            sheet_i: 0,
            zoom: 1.0,
            sel: Sel::none(),
            anchor: (0, 0),
            dragging: false,
            pending_cell: None,
        }
    }

    pub fn current_sheet(&self) -> usize {
        self.sheet_i
    }

    pub fn sheet_count(&self) -> usize {
        self.book.sheets.len()
    }

    pub fn jump_to_sheet(&mut self, i: usize) {
        if self.book.sheets.is_empty() {
            return;
        }
        self.sheet_i = i.min(self.book.sheets.len() - 1);
        self.sel = Sel::none();
        self.dragging = false;
    }

    pub fn jump_to_plain_line(&mut self, line: usize) {
        let Some(addr) = self.book.hits.get(line).copied() else {
            return;
        };
        self.jump_to_addr(addr);
    }

    fn jump_to_addr(&mut self, addr: CellAddr) {
        self.jump_to_sheet(addr.sheet);
        self.sel = Sel {
            r0: addr.row as i32,
            c0: addr.col as i32,
            r1: addr.row as i32,
            c1: addr.col as i32,
        };
        self.anchor = (addr.row as i32, addr.col as i32);
        self.pending_cell = Some((addr.row, addr.col));
    }

    pub fn sel_chars(&self) -> usize {
        if self.sel.is_none() {
            return 0;
        }
        selection_text(self.sheet(), self.sel)
            .chars()
            .filter(|c| !c.is_control())
            .count()
    }

    fn sheet(&self) -> &Sheet {
        &self.book.sheets[self.sheet_i.min(self.book.sheets.len().saturating_sub(1))]
    }
}

pub fn show(ui: &mut Ui, st: &mut XlsxSession, jump: Option<usize>, find_q: &str) -> XlsxAction {
    if let Some(i) = jump {
        st.jump_to_sheet(i);
    }
    let mut ui = crate::view::pane_ui(ui);
    if st.book.sheets.is_empty() {
        ui.add_space(24.0);
        ui.label(
            RichText::new(crate::i18n::t().xlsx_no_sheets).color(Color32::from_rgb(0x6B, 0x72, 0x80)),
        );
        return XlsxAction::None;
    }
    if crate::view::ctrl_zoom(&mut ui, &mut st.zoom) {
        st.zoom = st.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        ui.ctx().request_repaint();
    }
    st.zoom = st.zoom.clamp(ZOOM_MIN, ZOOM_MAX);

    let mut action = XlsxAction::None;
    let pane = ui.available_rect_before_wrap();
    let tabs_rect = Rect::from_min_max(pos2(pane.left(), pane.bottom() - TABS_H), pane.max);
    let grid_rect = Rect::from_min_max(pane.min, pos2(pane.right(), tabs_rect.top()));

    ui.scope_builder(UiBuilder::new().max_rect(grid_rect), |ui| {
        action = show_grid(ui, st, find_q);
    });
    ui.scope_builder(UiBuilder::new().max_rect(tabs_rect), |ui| {
        show_tabs(ui, st);
    });

    if matches!(action, XlsxAction::None) {
        if let Some(t) = selection_text_opt(st) {
            let copy = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
            if copy {
                action = XlsxAction::CopyText(t);
            }
        }
    }
    action
}

fn selection_text_opt(st: &XlsxSession) -> Option<String> {
    if st.sel.is_none() {
        return None;
    }
    let t = selection_text(st.sheet(), st.sel);
    if t.trim().is_empty() {
        None
    } else {
        Some(t)
    }
}

fn show_tabs(ui: &mut Ui, st: &mut XlsxSession) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, TAB_BG);
    ui.painter()
        .hline(rect.x_range(), rect.top(), Stroke::new(1.0_f32, GRID));
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.add_space(6.0);
            let names: Vec<String> = st.book.sheets.iter().map(|s| s.name.clone()).collect();
            for (i, name) in names.iter().enumerate() {
                let on = i == st.sheet_i;
                let txt = if on {
                    RichText::new(name).strong().color(Color32::from_rgb(0x1D, 0x4E, 0xD8))
                } else {
                    RichText::new(name).color(HDR_FG)
                };
                if ui.selectable_label(on, txt).clicked() {
                    st.jump_to_sheet(i);
                }
            }
        },
    );
}

fn show_grid(ui: &mut Ui, st: &mut XlsxSession, find_q: &str) -> XlsxAction {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, BG);
    let z = st.zoom;
    let hdr_h = HDR_H * z;
    let rh_w = ROW_HDR_W * z;
    let body = Rect::from_min_max(pos2(rect.left() + rh_w, rect.top() + hdr_h), rect.max);
    if body.width() < 8.0 || body.height() < 8.0 {
        return XlsxAction::None;
    }

    handle_keys(ui, st);

    let mut action = XlsxAction::None;
    let find_l = find_q.trim().to_lowercase();
    let mut offset = Vec2::ZERO;
    ui.scope_builder(UiBuilder::new().max_rect(body).id_salt("xlsx-body"), |ui| {
        let sheet_i = st.sheet_i;
        let sa = crate::view::content_scroll(false)
            .id_salt(("xlsx_scroll", sheet_i))
            .show_viewport(ui, |ui, vis| {
                crate::view::wheel_while_dragging(ui);
                let sh = st.sheet();
                let tw = sh.col_w.iter().sum::<f32>() * z;
                let th = sh.row_h.iter().sum::<f32>() * z;
                ui.set_min_size(vec2(tw.max(1.0), th.max(1.0)));
                paint_cells(ui, sh, vis, z, st.sel, &find_l);
                handle_pointer(ui, st, vis, body, z);
                if let Some((r, c)) = st.pending_cell.take() {
                    scroll_to_cell(ui, st.sheet(), r, c, z);
                }
            });
        offset = sa.state.offset;
        let _ = sa;
    });

    paint_headers(ui, rect, body, st, offset, z);

    if ui.input(|i| i.modifiers.ctrl || i.modifiers.command)
        && ui.input(|i| i.key_pressed(egui::Key::A))
        && ui.rect_contains_pointer(rect)
    {
        let sh = st.sheet();
        st.sel = Sel {
            r0: 0,
            c0: 0,
            r1: (sh.rows as i32 - 1).max(0),
            c1: (sh.cols as i32 - 1).max(0),
        };
    }

    let has_sel = !st.sel.is_none();
    let copy_t = selection_text_opt(st);
    let resp = ui.interact(rect, ui.id().with("xlsx-menu"), Sense::click());
    resp.context_menu(|ui| {
        if has_sel {
            if ui.button(crate::i18n::t().copy_text).clicked() {
                if let Some(t) = copy_t.clone() {
                    action = XlsxAction::CopyText(t);
                }
                ui.close();
            }
        }
    });
    action
}

fn handle_keys(ui: &mut Ui, st: &mut XlsxSession) {
    if ui.ctx().wants_keyboard_input() {
        return;
    }
    if ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.alt) {
        return;
    }
    let mut sheet_delta = 0i32;
    let mut d = (0i32, 0i32);
    let shift = ui.input(|i| i.modifiers.shift);
    ui.input_mut(|i| {
        if i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown)
            || i.consume_key(egui::Modifiers::SHIFT, egui::Key::PageDown)
        {
            sheet_delta = 1;
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp)
            || i.consume_key(egui::Modifiers::SHIFT, egui::Key::PageUp)
        {
            sheet_delta = -1;
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
            || i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowDown)
        {
            d.0 = 1;
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
            || i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowUp)
        {
            d.0 = -1;
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)
            || i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowRight)
            || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
        {
            d.1 = 1;
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)
            || i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowLeft)
        {
            d.1 = -1;
        }
    });
    if sheet_delta != 0 {
        let n = st.book.sheets.len();
        if n > 0 {
            let next = st.sheet_i as i32 + sheet_delta;
            st.jump_to_sheet(next.clamp(0, n as i32 - 1) as usize);
        }
        return;
    }
    if d == (0, 0) {
        return;
    }
    let rows = st.sheet().rows as i32;
    let cols = st.sheet().cols as i32;
    if rows <= 0 || cols <= 0 {
        return;
    }
    if st.sel.is_none() {
        st.sel = Sel {
            r0: 0,
            c0: 0,
            r1: 0,
            c1: 0,
        };
        st.anchor = (0, 0);
    }
    let mut r = st.sel.r1 + d.0;
    let mut c = st.sel.c1 + d.1;
    r = r.clamp(0, rows - 1);
    c = c.clamp(0, cols - 1);
    let (rr, cc) = st.sheet().resolve_origin(r as usize, c as usize);
    r = rr as i32;
    c = cc as i32;
    if shift {
        st.sel.r1 = r;
        st.sel.c1 = c;
    } else {
        st.sel = Sel {
            r0: r,
            c0: c,
            r1: r,
            c1: c,
        };
        st.anchor = (r, c);
    }
    st.pending_cell = Some((r as usize, c as usize));
}

fn scroll_to_cell(ui: &mut Ui, sh: &Sheet, r: usize, c: usize, z: f32) {
    let x = sh.col_w.iter().take(c).sum::<f32>() * z;
    let y = sh.row_h.iter().take(r).sum::<f32>() * z;
    let w = sh.col_w.get(c).copied().unwrap_or(64.0) * z;
    let h = sh.row_h.get(r).copied().unwrap_or(20.0) * z;
    ui.scroll_to_rect(Rect::from_min_size(pos2(x, y), vec2(w.max(1.0), h.max(1.0))), None);
}

fn handle_pointer(ui: &mut Ui, st: &mut XlsxSession, vis: Rect, body: Rect, z: f32) {
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let Some(pos) = pointer else {
        if ui.input(|i| i.pointer.primary_released()) {
            st.dragging = false;
        }
        return;
    };
    if !body.contains(pos) {
        if ui.input(|i| i.pointer.primary_released()) {
            st.dragging = false;
        }
        return;
    }
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    let down = ui.input(|i| i.pointer.primary_down());
    let released = ui.input(|i| i.pointer.primary_released());
    let cx = (pos.x - body.left()) + vis.min.x;
    let cy = (pos.y - body.top()) + vis.min.y;
    let Some((r, c)) = ({
        let sh = st.sheet();
        hit_cell(sh, cx, cy, z).map(|(r, c)| sh.resolve_origin(r, c))
    }) else {
        return;
    };
    if pressed {
        st.dragging = true;
        st.anchor = (r as i32, c as i32);
        st.sel = Sel {
            r0: r as i32,
            c0: c as i32,
            r1: r as i32,
            c1: c as i32,
        };
    } else if down && st.dragging {
        st.sel.r1 = r as i32;
        st.sel.c1 = c as i32;
        st.sel.r0 = st.anchor.0;
        st.sel.c0 = st.anchor.1;
    }
    if released {
        st.dragging = false;
    }
}

fn hit_cell(sh: &Sheet, x: f32, y: f32, z: f32) -> Option<(usize, usize)> {
    if sh.rows == 0 || sh.cols == 0 {
        return None;
    }
    let c = index_at(&sh.col_w, x / z.max(0.01));
    let r = index_at(&sh.row_h, y / z.max(0.01));
    Some((r.min(sh.rows - 1), c.min(sh.cols - 1)))
}

fn index_at(sizes: &[f32], v: f32) -> usize {
    let mut acc = 0.0;
    for (i, s) in sizes.iter().enumerate() {
        acc += *s;
        if v < acc {
            return i;
        }
    }
    sizes.len().saturating_sub(1)
}

fn paint_cells(ui: &mut Ui, sh: &Sheet, vis: Rect, z: f32, sel: Sel, find_l: &str) {
    let (c0, c1) = vis_span(&sh.col_w, vis.min.x / z, vis.max.x / z);
    let (r0, r1) = vis_span(&sh.row_h, vis.min.y / z, vis.max.y / z);
    let origin = ui.cursor().min;
    let fs = (11.0 * z).clamp(8.0, 22.0);
    let font = FontId::proportional(fs);
    for r in r0..=r1 {
        if r >= sh.rows {
            break;
        }
        for c in c0..=c1 {
            if c >= sh.cols {
                break;
            }
            if let Some(m) = sh.merge_at(r, c) {
                if !m.is_origin(r, c) {
                    continue;
                }
            }
            let cell = cell_rect(sh, origin, r, c, z);
            if !cell.intersects(vis.translate(origin.to_vec2())) && !cell.intersects(vis) {
                // vis is content coords; origin is screen of content 0
            }
            let screen = Rect::from_min_size(
                pos2(origin.x + col_x(sh, c) * z, origin.y + row_y(sh, r) * z),
                cell_size(sh, r, c, z),
            );
            let bg = if !find_l.is_empty() && sh.cell(r, c).to_lowercase().contains(find_l) {
                FIND
            } else if sel.contains(r, c) {
                SEL
            } else {
                BG
            };
            ui.painter().rect_filled(screen, 0.0, bg);
            ui.painter().rect_stroke(screen, 0.0, Stroke::new(1.0_f32, GRID), egui::StrokeKind::Inside);
            if sel.contains(r, c) {
                ui.painter().rect_stroke(
                    screen.shrink(0.5),
                    0.0,
                    Stroke::new(1.5_f32, SEL_BD),
                    egui::StrokeKind::Inside,
                );
            }
            let t = sh.cell(r, c);
            if t.is_empty() {
                continue;
            }
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = (screen.width() - 6.0).max(8.0);
            job.wrap.max_rows = 2;
            job.wrap.break_anywhere = true;
            job.append(
                t,
                0.0,
                TextFormat {
                    font_id: font.clone(),
                    color: TEXT,
                    ..Default::default()
                },
            );
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            ui.painter().with_clip_rect(screen).galley(
                pos2(screen.left() + 4.0, screen.top() + 2.0),
                galley,
                Color32::PLACEHOLDER,
            );
        }
    }
}

fn paint_headers(ui: &mut Ui, rect: Rect, body: Rect, st: &XlsxSession, offset: Vec2, z: f32) {
    let sh = st.sheet();
    let hdr_h = HDR_H * z;
    let rh_w = ROW_HDR_W * z;
    let corner = Rect::from_min_size(rect.min, vec2(rh_w, hdr_h));
    ui.painter().rect_filled(corner, 0.0, HDR_BG);
    ui.painter().rect_stroke(corner, 0.0, Stroke::new(1.0_f32, GRID), egui::StrokeKind::Inside);

    let col_band = Rect::from_min_max(pos2(body.left(), rect.top()), pos2(rect.right(), body.top()));
    ui.painter().rect_filled(col_band, 0.0, HDR_BG);
    let (c0, c1) = vis_span(&sh.col_w, offset.x / z, (offset.x + body.width()) / z);
    let fs = (10.0 * z).clamp(8.0, 16.0);
    for c in c0..=c1 {
        if c >= sh.cols {
            break;
        }
        let x = body.left() + col_x(sh, c) * z - offset.x;
        let w = sh.col_w[c] * z;
        let r = Rect::from_min_size(pos2(x, rect.top()), vec2(w, hdr_h));
        if r.right() < body.left() || r.left() > rect.right() {
            continue;
        }
        let clip = r.intersect(col_band);
        ui.painter().rect_stroke(clip, 0.0, Stroke::new(1.0_f32, GRID), egui::StrokeKind::Inside);
        let name = col_name(c);
        ui.painter().text(
            clip.center(),
            egui::Align2::CENTER_CENTER,
            name,
            FontId::proportional(fs),
            HDR_FG,
        );
    }

    let row_band = Rect::from_min_max(pos2(rect.left(), body.top()), pos2(body.left(), rect.bottom()));
    ui.painter().rect_filled(row_band, 0.0, HDR_BG);
    let (r0, r1) = vis_span(&sh.row_h, offset.y / z, (offset.y + body.height()) / z);
    for r in r0..=r1 {
        if r >= sh.rows {
            break;
        }
        let y = body.top() + row_y(sh, r) * z - offset.y;
        let h = sh.row_h[r] * z;
        let cell = Rect::from_min_size(pos2(rect.left(), y), vec2(rh_w, h));
        if cell.bottom() < body.top() || cell.top() > rect.bottom() {
            continue;
        }
        let clip = cell.intersect(row_band);
        ui.painter().rect_stroke(clip, 0.0, Stroke::new(1.0_f32, GRID), egui::StrokeKind::Inside);
        ui.painter().text(
            clip.center(),
            egui::Align2::CENTER_CENTER,
            format!("{}", r + 1),
            FontId::proportional(fs),
            HDR_FG,
        );
    }
    ui.painter()
        .vline(body.left(), rect.y_range(), Stroke::new(1.0_f32, GRID));
    ui.painter()
        .hline(rect.x_range(), body.top(), Stroke::new(1.0_f32, GRID));
}

fn vis_span(sizes: &[f32], a: f32, b: f32) -> (usize, usize) {
    if sizes.is_empty() {
        return (0, 0);
    }
    let lo = index_at(sizes, a.max(0.0));
    let hi = index_at(sizes, b.max(0.0));
    (lo, hi.max(lo))
}

fn col_x(sh: &Sheet, c: usize) -> f32 {
    sh.col_w.iter().take(c).sum()
}

fn row_y(sh: &Sheet, r: usize) -> f32 {
    sh.row_h.iter().take(r).sum()
}

fn cell_size(sh: &Sheet, r: usize, c: usize, z: f32) -> Vec2 {
    if let Some(m) = sh.merge_at(r, c) {
        if m.is_origin(r, c) {
            let w: f32 = sh.col_w[m.c0..=m.c1.min(sh.cols.saturating_sub(1))].iter().sum();
            let h: f32 = sh.row_h[m.r0..=m.r1.min(sh.rows.saturating_sub(1))].iter().sum();
            return vec2(w * z, h * z);
        }
    }
    vec2(
        sh.col_w.get(c).copied().unwrap_or(64.0) * z,
        sh.row_h.get(r).copied().unwrap_or(20.0) * z,
    )
}

fn cell_rect(sh: &Sheet, origin: Pos2, r: usize, c: usize, z: f32) -> Rect {
    Rect::from_min_size(
        pos2(origin.x + col_x(sh, c) * z, origin.y + row_y(sh, r) * z),
        cell_size(sh, r, c, z),
    )
}

fn selection_text(sh: &Sheet, sel: Sel) -> String {
    if sel.is_none() {
        return String::new();
    }
    let (r0, c0, r1, c1) = sel.norm();
    let r1 = r1.min(sh.rows.saturating_sub(1));
    let c1 = c1.min(sh.cols.saturating_sub(1));
    let mut lines = Vec::new();
    for r in r0..=r1 {
        let mut cols = Vec::new();
        for c in c0..=c1 {
            if let Some(m) = sh.merge_at(r, c) {
                if !m.is_origin(r, c) {
                    cols.push(String::new());
                    continue;
                }
            }
            cols.push(sh.cell(r, c).to_string());
        }
        lines.push(cols.join("\t"));
    }
    let out = lines.join("\n");
    if cfg!(windows) {
        out.replace('\n', "\r\n")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::xlsx::col_name;

    #[test]
    fn sel_norm_swaps() {
        let s = Sel {
            r0: 5,
            c0: 3,
            r1: 1,
            c1: 0,
        };
        assert_eq!(s.norm(), (1, 0, 5, 3));
        assert!(s.contains(2, 1));
        assert!(!s.contains(0, 0));
    }

    #[test]
    fn col_letters_match_loader() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(26), "AA");
    }
}
