//! PDF 连续页预览（仿 docview PdfViewer：竖向一页接一页，可拖选文字）。

use std::path::Path;
use std::sync::Arc;

use egui::{
    pos2, Align, Color32, ColorImage, Context, CursorIcon, Frame, Layout, Margin, PointerButton,
    Pos2, Rect, RichText, Sense, Stroke, TextureOptions, Ui, UiBuilder, Vec2,
};

use crate::io::pdf::{PdfChar, PdfEngine, PdfEvent};
use crate::io::imgcache::Raster;

const PAGE_GAP: f32 = 12.0;
const PREFETCH: i32 = 2;
/// 视口外超过这么多页的纹理丢掉（对齐 Sumatra FreeNotVisible）。
const DROP_AWAY: i32 = 6;
const BG: Color32 = Color32::from_rgb(0xE5, 0xE7, 0xEB);
/// 100%：1 PDF 点 = 1/72 英寸，按 96 DPI 换成逻辑像素（与 Sumatra「实际大小」一致）。
const PDF_PT_TO_DIP: f32 = 96.0 / 72.0;

fn page_disp_size(pw: f32, ph: f32, zoom: f32) -> Vec2 {
    let z = PDF_PT_TO_DIP * zoom.clamp(crate::view::ZOOM_MIN, crate::view::ZOOM_MAX);
    Vec2::new((pw * z).max(1.0), (ph * z).max(1.0))
}

fn page_render_width(pw: f32, ppp: f32, zoom: f32) -> u32 {
    (pw * PDF_PT_TO_DIP * zoom.clamp(crate::view::ZOOM_MIN, crate::view::ZOOM_MAX) * ppp.max(1.0))
        .round()
        .clamp(160.0, 2400.0) as u32
}
/// SumatraPDF 选区：亮黄半透明叠在页面上（白纸变黄、黑字偏橄榄）。
const SEL: Color32 = Color32::from_rgba_premultiplied(0xC0, 0xC6, 0x0C, 0xC8);

enum Slot {
    Empty,
    /// 第一次光栅，还没有可拉伸的旧图。
    Loading {
        width: u32,
    },
    Ready {
        width: u32,
        raster: Raster,
        pending: Option<u32>,
    },
    Failed,
}

fn width_close(a: u32, b: u32) -> bool {
    a.abs_diff(b) <= 80
}

/// 已有图且宽度够近，或已经在渲目标宽度，就不再排队。
fn should_request(have: Option<u32>, pending: Option<u32>, want: u32) -> bool {
    if have.is_some_and(|w| width_close(w, want)) {
        return false;
    }
    if pending.is_some_and(|w| width_close(w, want)) {
        return false;
    }
    true
}

pub struct PdfSession {
    engine: Option<PdfEngine>,
    pub page_count: u32,
    sizes: Vec<(f32, f32)>,
    slots: Vec<Slot>,
    chars: Vec<Option<Vec<PdfChar>>>,
    text_pending: Vec<bool>,
    pub top_page: usize,
    pub err: Option<String>,
    page_tops: Vec<f32>,
    page_hs: Vec<f32>,
    content_h: f32,
    content_w: f32,
    layout_zoom: f32,
    layout_avail_w: f32,
    last_scroll_y: f32,
    bar_dragging: bool,
    pending_page: Option<usize>,
    sel_page: i32,
    sel_lo: i32,
    sel_hi: i32,
    dragging: bool,
    anchor_page: i32,
    anchor_char: i32,
    pub sel_chars: usize,
    pub zoom: f32,
}

impl PdfSession {
    pub fn open(path: &Path) -> Self {
        Self {
            engine: Some(PdfEngine::start(path)),
            page_count: 0,
            sizes: Vec::new(),
            slots: Vec::new(),
            chars: Vec::new(),
            text_pending: Vec::new(),
            top_page: 0,
            err: None,
            page_tops: Vec::new(),
            page_hs: Vec::new(),
            content_h: 1.0,
            content_w: 120.0,
            layout_zoom: -1.0,
            layout_avail_w: -1.0,
            last_scroll_y: 0.0,
            bar_dragging: false,
            pending_page: None,
            sel_page: -1,
            sel_lo: -1,
            sel_hi: -1,
            dragging: false,
            anchor_page: -1,
            anchor_char: -1,
            sel_chars: 0,
            zoom: 1.0,
        }
    }

    pub fn jump_to(&mut self, page0: usize) {
        self.pending_page = Some(page0);
    }

    pub fn current_page(&self) -> usize {
        self.top_page
    }

    pub fn selected_text(&self) -> Option<String> {
        if self.sel_page < 0 || self.sel_lo < 0 {
            return None;
        }
        let i = self.sel_page as usize;
        let chars = self.chars.get(i).and_then(|c| c.as_ref())?;
        let t = selection_text(chars, self.sel_lo, self.sel_hi);
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    }

    fn clear_sel(&mut self) {
        self.sel_page = -1;
        self.sel_lo = -1;
        self.sel_hi = -1;
        self.sel_chars = 0;
        self.dragging = false;
    }

    fn poll(&mut self, ctx: &Context) {
        let Some(eng) = self.engine.as_ref() else {
            return;
        };
        let mut got = false;
        while let Ok(ev) = eng.rx.try_recv() {
            got = true;
            match ev {
                PdfEvent::Ready(meta) => {
                    self.page_count = meta.page_count;
                    self.sizes = meta.sizes;
                    self.slots = (0..self.page_count).map(|_| Slot::Empty).collect();
                    self.chars = vec![None; self.page_count as usize];
                    self.text_pending = vec![false; self.page_count as usize];
                    self.page_tops = vec![0.0; self.page_count as usize];
                    self.page_hs = vec![0.0; self.page_count as usize];
                    self.layout_zoom = -1.0;
                }
                PdfEvent::PageFailed(page) => {
                    let i = page as usize;
                    if i < self.slots.len() {
                        if matches!(&self.slots[i], Slot::Ready { .. }) {
                            if let Slot::Ready { pending, .. } = &mut self.slots[i] {
                                *pending = None;
                            }
                        } else {
                            self.slots[i] = Slot::Failed;
                        }
                    }
                }
                PdfEvent::Failed(e) => {
                    if self.page_count == 0 {
                        self.err = Some(e);
                    }
                }
                PdfEvent::Text { page, chars } => {
                    let i = page as usize;
                    if i < self.chars.len() {
                        self.chars[i] = Some(chars);
                        self.text_pending[i] = false;
                    }
                }
                PdfEvent::Page(px) => {
                    let i = px.page as usize;
                    if i >= self.slots.len() {
                        continue;
                    }
                    if let Slot::Ready { width: have, .. } = &self.slots[i] {
                        if width_close(*have, px.width) && *have >= px.width {
                            continue;
                        }
                    }
                    let img = ColorImage::from_rgba_unmultiplied(
                        [px.px_w as usize, px.px_h as usize],
                        &px.rgba,
                    );
                    let tex = ctx.load_texture(
                        format!("pdf-{}-{}", px.page, px.width),
                        img,
                        TextureOptions::LINEAR,
                    );
                    self.slots[i] = Slot::Ready {
                        width: px.width,
                        raster: Raster {
                            tex,
                            size: Vec2::new(px.px_w as f32, px.px_h as f32),
                            rgba: Arc::new(px.rgba),
                            local_path: None,
                        },
                        pending: None,
                    };
                }
            }
        }
        if got {
            ctx.request_repaint();
        }
    }

    fn request_visible(
        &mut self,
        vis_lo: usize,
        vis_hi: usize,
        ppp: f32,
        prefetch: i32,
        want_text: bool,
    ) {
        let Some(eng) = self.engine.as_ref() else {
            return;
        };
        if self.page_count == 0 {
            return;
        }
        let n = self.page_count as i32;
        let lo = (vis_lo as i32 - prefetch).max(0) as usize;
        let hi = ((vis_hi as i32) + prefetch).min(n - 1) as usize;
        eng.set_vis(lo as u32, hi as u32);
        for i in lo..=hi {
            let (pw, _) = self.sizes.get(i).copied().unwrap_or((612.0, 792.0));
            let width = page_render_width(pw, ppp, self.zoom);
            let (have, pending, failed) = match &self.slots[i] {
                Slot::Failed => (None, None, true),
                Slot::Empty => (None, None, false),
                Slot::Loading { width: w } => (None, Some(*w), false),
                Slot::Ready {
                    width: w, pending, ..
                } => (Some(*w), *pending, false),
            };
            if !failed && should_request(have, pending, width) {
                match &mut self.slots[i] {
                    Slot::Ready { pending, .. } => *pending = Some(width),
                    _ => self.slots[i] = Slot::Loading { width },
                }
                eng.request(i as u32, width);
            }
            if want_text && self.chars[i].is_none() && !self.text_pending[i] {
                self.text_pending[i] = true;
                eng.request_text(i as u32);
            }
        }
    }

    fn drop_far_pages(&mut self, vis_lo: usize, vis_hi: usize) {
        if self.slots.is_empty() {
            return;
        }
        let n = self.slots.len() as i32;
        let lo = (vis_lo as i32 - DROP_AWAY).max(0);
        let hi = (vis_hi as i32 + DROP_AWAY).min(n - 1);
        for i in 0..self.slots.len() {
            let ii = i as i32;
            if ii >= lo && ii <= hi {
                continue;
            }
            if !matches!(&self.slots[i], Slot::Failed) {
                self.slots[i] = Slot::Empty;
            }
        }
    }

    fn ensure_layout(&mut self, avail_w: f32) {
        let n = self.page_count as usize;
        if n == 0 {
            return;
        }
        if self.page_hs.len() == n
            && (self.layout_zoom - self.zoom).abs() < 1e-4
            && (self.layout_avail_w - avail_w).abs() < 0.5
        {
            return;
        }
        self.page_tops.resize(n, 0.0);
        self.page_hs.resize(n, 0.0);
        let mut y = 0.0f32;
        let mut max_w = 120.0f32;
        for i in 0..n {
            let (pw, ph) = self.sizes.get(i).copied().unwrap_or((612.0, 792.0));
            let disp = page_disp_size(pw, ph, self.zoom);
            self.page_tops[i] = y;
            self.page_hs[i] = disp.y;
            max_w = max_w.max(disp.x);
            y += disp.y + PAGE_GAP;
        }
        self.content_h = y.max(1.0);
        self.content_w = (max_w + 32.0).max(avail_w);
        self.layout_zoom = self.zoom;
        self.layout_avail_w = avail_w;
    }
}

/// 按页顶/高二分可见区间（Sumatra DisplayModel：不扫全部页）。
fn visible_page_range(tops: &[f32], hs: &[f32], clip_top: f32, clip_bot: f32) -> (usize, usize) {
    let n = tops.len();
    if n == 0 || n != hs.len() {
        return (0, 0);
    }
    let mut lo = 0usize;
    let mut hi = n;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if tops[mid] + hs[mid] < clip_top - 8.0 {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let vis_lo = lo.min(n - 1);
    let mut lo = 0usize;
    let mut hi = n;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if tops[mid] <= clip_bot + 8.0 {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let vis_hi = lo.saturating_sub(1).min(n - 1).max(vis_lo);
    (vis_lo, vis_hi)
}

pub enum PdfAction {
    None,
    Open(Raster),
    Copy(Raster),
    CopyFile(Raster),
    CopyText(String),
}

pub fn show(ui: &mut Ui, st: &mut PdfSession, jump: Option<usize>) -> PdfAction {
    st.poll(ui.ctx());
    if let Some(p) = jump {
        st.jump_to(p);
    }
    let mut ui = crate::view::pane_ui(ui);
    ui.painter().rect_filled(ui.max_rect(), 0.0, BG);
    if let Some(err) = &st.err {
        ui.add_space(24.0);
        ui.label(RichText::new(err).color(Color32::from_rgb(0xB9, 0x1C, 0x1C)));
        return PdfAction::None;
    }
    if st.page_count == 0 {
        ui.add_space(24.0);
        ui.label(RichText::new(crate::i18n::t().opening_pdf).color(Color32::from_rgb(0x6B, 0x72, 0x80)));
        ui.ctx().request_repaint();
        return PdfAction::None;
    }

    if crate::view::ctrl_zoom(&mut ui, &mut st.zoom) {
        ui.ctx().request_repaint();
    }

    let max_h = ui.available_height();
    let avail_w = ui.available_width();
    let ppp = ui.ctx().pixels_per_point();
    let mut action = PdfAction::None;
    let nav = crate::view::consume_key_nav(&mut ui);
    match nav {
        crate::view::KeyNav::Page(dir) => {
            let n = st.page_count as i32;
            if n > 0 {
                let next = (st.top_page as i32 + dir).clamp(0, n - 1) as usize;
                if next != st.top_page {
                    st.jump_to(next);
                }
            }
        }
        _ => {}
    }
    let jump_page = st.pending_page.take();
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let primary_pressed = ui.input(|i| i.pointer.primary_pressed());
    let primary_down = ui.input(|i| i.pointer.primary_down());
    let primary_released = ui.input(|i| i.pointer.primary_released());
    st.ensure_layout(avail_w);

    let sa = crate::view::content_scroll(false)
        .id_salt("pdf_scroll")
        .max_height(max_h)
        .show(&mut ui, |ui| {
            crate::view::wheel_while_dragging(ui);
            if let crate::view::KeyNav::Line(d) = nav {
                ui.scroll_with_delta(d);
            }
            let clip = ui.clip_rect();
            let content_w = st.content_w.max(avail_w);
            ui.set_min_width(content_w);
            let origin = ui.cursor().min;
            ui.allocate_exact_size(Vec2::new(content_w, st.content_h), Sense::hover());
            if let Some(p) = jump_page {
                if p < st.page_tops.len() {
                    let top = origin.y + st.page_tops[p];
                    let h = st.page_hs.get(p).copied().unwrap_or(1.0);
                    ui.scroll_to_rect(
                        Rect::from_min_size(pos2(origin.x, top), Vec2::new(content_w, h)),
                        Some(Align::TOP),
                    );
                }
            }
            let (vis_lo, vis_hi) = visible_page_range(
                &st.page_tops,
                &st.page_hs,
                clip.top() - origin.y,
                clip.bottom() - origin.y,
            );
            let mut top_page = vis_lo;
            for i in vis_lo..=vis_hi {
                if origin.y + st.page_tops[i] <= clip.top() + 24.0 {
                    top_page = i;
                }
            }
            st.top_page = top_page;
            let mut hover_text = false;
            let n = st.page_count as usize;
            for i in vis_lo..=vis_hi {
                if i >= n {
                    break;
                }
                let (pw, ph) = st.sizes.get(i).copied().unwrap_or((612.0, 792.0));
                let disp = page_disp_size(pw, ph, st.zoom);
                let top = origin.y + st.page_tops[i];
                let row_w = disp.x.max(avail_w);
                let row_rect = Rect::from_min_size(pos2(origin.x, top), Vec2::new(row_w, disp.y));
                ui.scope_builder(
                    UiBuilder::new().id_salt(("pdf-row", i)).max_rect(row_rect),
                    |ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(row_w, disp.y),
                    Layout::left_to_right(Align::Min),
                    |ui| {
                        ui.add_space(((row_w - disp.x) * 0.5).max(0.0));
                        Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.0, Color32::from_rgb(0xD1, 0xD5, 0xDB)))
                    .inner_margin(Margin::ZERO)
                    .show(ui, |ui| {
                        ui.set_min_size(disp);
                        ui.set_max_size(disp);
                        let page_rect = ui.max_rect();
                        enum PageView {
                            Ready(Raster),
                            Failed,
                            Loading,
                        }
                        let view = match st.slots.get(i) {
                            Some(Slot::Ready { raster, .. }) => PageView::Ready(raster.clone()),
                            Some(Slot::Failed) => PageView::Failed,
                            _ => PageView::Loading,
                        };
                        match view {
                            PageView::Ready(raster) => {
                                ui.put(
                                    page_rect,
                                    egui::Image::new((raster.tex.id(), disp)),
                                );
                                draw_sel(ui, st, i, page_rect, pw, ph);
                                let resp = ui.interact(
                                    page_rect,
                                    ui.id().with(("pdf-page", i)),
                                    Sense::click_and_drag(),
                                );
                                if pointer_on_char(st, i, page_rect, pw, ph, pointer) {
                                    hover_text = true;
                                    resp.clone().on_hover_cursor(CursorIcon::Text);
                                } else {
                                    resp.clone().on_hover_cursor(CursorIcon::PointingHand);
                                }
                                resp.clone().on_hover_text(crate::i18n::t().pdf_drag_hint);
                                if resp.dragged_by(PointerButton::Primary)
                                    || (primary_down && resp.contains_pointer())
                                {
                                    if let Some(pos) = pointer {
                                        handle_drag(st, i, page_rect, pw, ph, pos, primary_pressed);
                                    }
                                }
                                if resp.double_clicked() {
                                    action = PdfAction::Open(raster.clone());
                                }
                                let has_text = st.sel_page >= 0 && st.sel_lo >= 0;
                                let rast = raster.clone();
                                let copy_text = st.selected_text();
                                resp.context_menu(|ui| {
                                    if has_text {
                                        if ui.button(crate::i18n::t().copy_text).clicked() {
                                            if let Some(t) = copy_text.clone() {
                                                ui.ctx().copy_text(t.clone());
                                                action = PdfAction::CopyText(t);
                                            }
                                            ui.close();
                                        }
                                    }
                                    if ui.button(crate::i18n::t().copy_image).clicked() {
                                        action = PdfAction::Copy(rast.clone());
                                        ui.close();
                                    }
                                    if ui.button(crate::i18n::t().copy_as_file).clicked() {
                                        action = PdfAction::CopyFile(rast.clone());
                                        ui.close();
                                    }
                                });
                            }
                            PageView::Failed => {
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        RichText::new(crate::i18n::pdf_page_fail(i + 1))
                                            .color(Color32::GRAY),
                                    );
                                });
                            }
                            PageView::Loading => {
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        RichText::new(crate::i18n::pdf_page_loading(i + 1))
                                            .color(Color32::GRAY),
                                    );
                                });
                            }
                        }
                    });
                    },
                );
                    },
                );
            }
            if hover_text {
                ui.ctx().set_cursor_icon(CursorIcon::Text);
            }
            (vis_lo, vis_hi)
        });

    if primary_released {
        st.dragging = false;
        recount_sel(st);
    }

    if matches!(action, PdfAction::None) {
        if let Some(t) = st.selected_text() {
            let copy = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
            if copy {
                action = PdfAction::CopyText(t);
            }
        }
    }

    let (vis_lo, vis_hi) = sa.inner;
    let vis_hi = vis_hi.max(vis_lo);
    let offset_y = sa.state.offset.y;
    if primary_released {
        st.bar_dragging = false;
    } else if primary_down && (offset_y - st.last_scroll_y).abs() > 1.0 {
        st.bar_dragging = true;
    }
    st.last_scroll_y = offset_y;
    let prefetch = if st.bar_dragging { 0 } else { PREFETCH };
    st.request_visible(vis_lo, vis_hi, ppp, prefetch, !st.bar_dragging);
    st.drop_far_pages(vis_lo, vis_hi);
    let busy = (vis_lo..=vis_hi).any(|i| match st.slots.get(i) {
        Some(Slot::Empty | Slot::Loading { .. }) => true,
        Some(Slot::Ready { pending: Some(_), .. }) => true,
        _ => false,
    });
    if busy {
        ui.ctx().request_repaint();
    }
    action
}

fn handle_drag(
    st: &mut PdfSession,
    page: usize,
    page_rect: Rect,
    pw: f32,
    ph: f32,
    pos: Pos2,
    just_pressed: bool,
) {
    if just_pressed {
        if let Some(idx) = hit_char(st, page, page_rect, pw, ph, pos, true) {
            st.dragging = true;
            st.anchor_page = page as i32;
            st.anchor_char = idx;
            st.sel_page = page as i32;
            st.sel_lo = idx;
            st.sel_hi = idx;
        } else {
            st.clear_sel();
        }
        recount_sel(st);
        return;
    }
    if !st.dragging {
        return;
    }
    let Some(idx) = hit_char(st, page, page_rect, pw, ph, pos, false) else {
        return;
    };
    if page as i32 != st.anchor_page {
        let Some(chars) = st
            .chars
            .get(st.anchor_page as usize)
            .and_then(|c| c.as_ref())
        else {
            return;
        };
        if chars.is_empty() {
            return;
        }
        if (page as i32) > st.anchor_page {
            let last = chars.last().map(|c| c.index).unwrap_or(st.anchor_char);
            st.sel_page = st.anchor_page;
            st.sel_lo = st.anchor_char.min(last);
            st.sel_hi = st.anchor_char.max(last);
        } else {
            let first = chars.first().map(|c| c.index).unwrap_or(st.anchor_char);
            st.sel_page = st.anchor_page;
            st.sel_lo = st.anchor_char.min(first);
            st.sel_hi = st.anchor_char.max(first);
        }
        recount_sel(st);
        return;
    }
    st.sel_page = page as i32;
    st.sel_lo = st.anchor_char.min(idx);
    st.sel_hi = st.anchor_char.max(idx);
    recount_sel(st);
}

fn pointer_on_char(
    st: &PdfSession,
    page: usize,
    page_rect: Rect,
    pw: f32,
    ph: f32,
    pointer: Option<Pos2>,
) -> bool {
    let Some(pos) = pointer else {
        return false;
    };
    hit_char(st, page, page_rect, pw, ph, pos, true).is_some()
}

fn hit_char(
    st: &PdfSession,
    page: usize,
    page_rect: Rect,
    pw: f32,
    ph: f32,
    pos: Pos2,
    strict: bool,
) -> Option<i32> {
    if !page_rect.expand(8.0).contains(pos) {
        return None;
    }
    let chars = st.chars.get(page)?.as_ref()?;
    if chars.is_empty() {
        return None;
    }
    let sx = pw / page_rect.width().max(1.0);
    let sy = ph / page_rect.height().max(1.0);
    let px = (pos.x - page_rect.left()) * sx;
    let py = (pos.y - page_rect.top()) * sy;
    if strict {
        let pad = 2.5;
        let mut best = None;
        let mut best_d = f32::MAX;
        for c in chars {
            if c.ch.is_whitespace() && c.ch != '\t' {
                continue;
            }
            if px >= c.left - pad && px <= c.right + pad && py >= c.top - pad && py <= c.bottom + pad
            {
                let mx = (c.left + c.right) * 0.5;
                let my = (c.top + c.bottom) * 0.5;
                let d = (mx - px) * (mx - px) + (my - py) * (my - py);
                if d < best_d {
                    best_d = d;
                    best = Some(c.index);
                }
            }
        }
        return best;
    }
    let mut best_i = chars[0].index;
    let mut best_d = f32::MAX;
    for c in chars {
        let my = (c.top + c.bottom) * 0.5;
        let dy = (my - py).abs();
        let dx = if px < c.left {
            c.left - px
        } else if px > c.right {
            px - c.right
        } else {
            0.0
        };
        let d = dy * 4.0 + dx;
        if d < best_d {
            best_d = d;
            best_i = c.index;
        }
    }
    Some(best_i)
}

fn draw_sel(ui: &Ui, st: &PdfSession, page: usize, page_rect: Rect, pw: f32, ph: f32) {
    if st.sel_page != page as i32 || st.sel_lo < 0 {
        return;
    }
    let Some(chars) = st.chars.get(page).and_then(|c| c.as_ref()) else {
        return;
    };
    let sx = page_rect.width() / pw.max(1.0);
    let sy = page_rect.height() / ph.max(1.0);
    for line in group_sel_lines(chars, st.sel_lo, st.sel_hi) {
        let mut r = Rect::from_min_max(
            egui::pos2(page_rect.left() + line.0 * sx, page_rect.top() + line.1 * sy),
            egui::pos2(page_rect.left() + line.2 * sx, page_rect.top() + line.3 * sy),
        );
        r = r.expand2(egui::vec2(1.5, 1.0));
        ui.painter().rect_filled(r, 0.0, SEL);
    }
}

fn group_sel_lines(chars: &[PdfChar], lo: i32, hi: i32) -> Vec<(f32, f32, f32, f32)> {
    let mut picked: Vec<&PdfChar> = chars
        .iter()
        .filter(|c| c.index >= lo && c.index <= hi)
        .collect();
    picked.sort_by(|a, b| a.top.partial_cmp(&b.top).unwrap_or(std::cmp::Ordering::Equal));
    if picked.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut l = picked[0].left;
    let mut t = picked[0].top;
    let mut r = picked[0].right;
    let mut b = picked[0].bottom;
    let mut mid = (t + b) * 0.5;
    for c in picked.iter().skip(1) {
        let m = (c.top + c.bottom) * 0.5;
        let tol = ((b - t).max(c.bottom - c.top) * 0.6).max(3.0);
        if (m - mid).abs() <= tol {
            l = l.min(c.left);
            t = t.min(c.top);
            r = r.max(c.right);
            b = b.max(c.bottom);
            mid = (t + b) * 0.5;
        } else {
            lines.push((l, t, r, b));
            l = c.left;
            t = c.top;
            r = c.right;
            b = c.bottom;
            mid = m;
        }
    }
    lines.push((l, t, r, b));
    lines
}

fn selection_text(chars: &[PdfChar], lo: i32, hi: i32) -> String {
    let mut picked: Vec<&PdfChar> = chars
        .iter()
        .filter(|c| c.index >= lo && c.index <= hi)
        .collect();
    picked.sort_by_key(|c| c.index);
    let mut out = String::new();
    let mut prev_mid: Option<f32> = None;
    for c in picked {
        if c.ch == '\n' || c.ch == '\r' {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            prev_mid = Some((c.top + c.bottom) * 0.5);
            continue;
        }
        let mid = (c.top + c.bottom) * 0.5;
        if let Some(pm) = prev_mid {
            let tol = ((c.bottom - c.top) * 0.7).max(4.0);
            if (mid - pm).abs() > tol && !out.ends_with('\n') {
                out.push('\n');
            }
        }
        out.push(c.ch);
        prev_mid = Some(mid);
    }
    if cfg!(windows) {
        out.replace('\n', "\r\n")
    } else {
        out
    }
}

fn recount_sel(st: &mut PdfSession) {
    if st.sel_page < 0 {
        st.sel_chars = 0;
        return;
    }
    let Some(chars) = st.chars.get(st.sel_page as usize).and_then(|c| c.as_ref()) else {
        st.sel_chars = 0;
        return;
    };
    st.sel_chars = chars
        .iter()
        .filter(|c| c.index >= st.sel_lo && c.index <= st.sel_hi && !c.ch.is_control())
        .count();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_rerender_when_width_close() {
        assert!(!should_request(Some(1200), None, 1240));
        assert!(!should_request(Some(800), Some(1600), 1600));
        assert!(should_request(None, None, 800));
        assert!(should_request(Some(800), None, 1600));
        assert!(!should_request(None, Some(800), 820));
    }

    #[test]
    fn page_size_100pct_matches_96dpi() {
        let s = page_disp_size(612.0, 792.0, 1.0);
        assert!((s.x - 816.0).abs() < 0.5);
        assert!((s.y - 1056.0).abs() < 0.5);
    }

    #[test]
    fn render_width_clamped() {
        let w = page_render_width(612.0, 1.0, 1.0);
        assert_eq!(w, 816);
        let hi = page_render_width(612.0, 4.0, 8.0);
        assert_eq!(hi, 2400);
    }

    #[test]
    fn visible_range_matches_scan_on_1000_pages() {
        let n = 1000usize;
        let mut tops = vec![0.0f32; n];
        let hs = vec![100.0f32; n];
        for i in 0..n {
            tops[i] = i as f32 * 112.0;
        }
        let clip_top = 50_000.0;
        let clip_bot = 51_200.0;
        let (lo, hi) = visible_page_range(&tops, &hs, clip_top, clip_bot);
        let mut brute_lo = None;
        let mut brute_hi = 0usize;
        for i in 0..n {
            let t = tops[i];
            let b = t + hs[i];
            if b >= clip_top - 8.0 && t <= clip_bot + 8.0 {
                if brute_lo.is_none() {
                    brute_lo = Some(i);
                }
                brute_hi = i;
            }
        }
        assert_eq!((lo, hi), (brute_lo.unwrap(), brute_hi));
        assert!(hi - lo < 20, "可见页应很少, {lo}..{hi}");
    }
}
