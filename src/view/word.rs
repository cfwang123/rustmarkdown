//! Word 连续页预览（对齐 docview DocxViewer：灰底白页、可选字、不经 Markdown）。

use std::sync::Arc;

use egui::{
    pos2, text::LayoutJob, vec2, Align, Color32, ColorImage, FontId, Frame, Label, Layout, Margin,
    Rect, RichText, Sense, Stroke, TextFormat, TextureOptions, Ui, UiBuilder, Vec2,
};

use crate::io::imgcache::Raster;
use crate::io::word::{WordBlock, WordDoc, WordImage, WordPara, WordRun, WordTable};
use crate::view::img_preview::{self, ThumbAction};
use crate::view::theme;

const PAGE_GAP: f32 = 12.0;
const BG: Color32 = Color32::from_rgb(0xE5, 0xE7, 0xEB);
const PAGE_STROKE: Color32 = Color32::from_rgb(0xD1, 0xD5, 0xDB);
const FIND_BG: Color32 = Color32::from_rgb(0xFE, 0xF0, 0x8A);
const LINK: Color32 = Color32::from_rgb(0x25, 0x63, 0xEB);
const HR: Color32 = Color32::from_rgb(0xD1, 0xD5, 0xDB);
const CELL_BORDER: Color32 = Color32::from_rgb(0xD1, 0xD5, 0xDB);
const HEADER_BG: Color32 = Color32::from_rgb(0xF3, 0xF4, 0xF6);

pub enum WordAction {
    None,
    OpenImage { raster: Raster, title: String },
    CopyImage(Raster),
    CopyAsFile(Raster),
    OpenHref(String),
}

pub struct WordSession {
    pub doc: WordDoc,
    pub zoom: f32,
    pub page: usize,
    pub pages: usize,
    pub top_block: usize,
    pending_page: Option<usize>,
    pending_block: Option<usize>,
    rasters: Vec<Option<Raster>>,
    tex_key: u64,
    layout_w: f32,
    layout_zoom: f32,
    block_hs: Vec<f32>,
    page_ranges: Vec<(usize, usize)>,
    page_frame_h: Vec<f32>,
}

impl WordSession {
    pub fn new(doc: WordDoc) -> Self {
        let n_img = doc.images.len();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        use std::hash::{Hash, Hasher};
        doc.plain.hash(&mut h);
        n_img.hash(&mut h);
        Self {
            doc,
            zoom: 1.0,
            page: 0,
            pages: 1,
            top_block: 0,
            pending_page: None,
            pending_block: None,
            rasters: vec![None; n_img],
            tex_key: h.finish(),
            layout_w: -1.0,
            layout_zoom: -1.0,
            block_hs: Vec::new(),
            page_ranges: Vec::new(),
            page_frame_h: Vec::new(),
        }
    }

    pub fn jump_to_page(&mut self, page0: usize) {
        self.pending_page = Some(page0);
    }

    pub fn jump_to_block(&mut self, block: usize) {
        self.pending_block = Some(block);
    }

    pub fn current_page(&self) -> usize {
        self.page
    }

    fn raster(&mut self, ctx: &egui::Context, id: usize) -> Option<Raster> {
        if let Some(Some(r)) = self.rasters.get(id) {
            return Some(r.clone());
        }
        let img = self.doc.images.get(id)?;
        let (w, h, px) = decode_rgba(&img.bytes)?;
        let key = format!("word-{}-{id}", self.tex_key);
        let tex = ctx.load_texture(
            key,
            ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px),
            TextureOptions::LINEAR,
        );
        let r = Raster {
            tex,
            size: Vec2::new(w as f32, h as f32),
            rgba: Arc::new(px),
            local_path: None,
        };
        if id < self.rasters.len() {
            self.rasters[id] = Some(r.clone());
        }
        Some(r)
    }
}

pub fn show(ui: &mut Ui, st: &mut WordSession, jump: Option<usize>, find_q: &str) -> WordAction {
    let mut ui = crate::view::pane_ui(ui);
    let pane_clip = ui.clip_rect();
    ui.set_clip_rect(pane_clip);
    ui.painter().rect_filled(ui.max_rect(), 0.0, BG);
    if crate::view::ctrl_zoom(&mut ui, &mut st.zoom) {
        ui.ctx().request_repaint();
    }
    let max_h = ui.available_height();
    let avail_w = ui.available_width();
    let scale = st.zoom.clamp(crate::view::ZOOM_MIN, crate::view::ZOOM_MAX);
    let page_w = (st.doc.page_w * scale).max(80.0);
    let page_h = (st.doc.page_h * scale).max(80.0);
    let pad_l = (st.doc.pad_l * scale).max(16.0);
    let pad_t = (st.doc.pad_t * scale).max(16.0);
    let pad_r = (st.doc.pad_r * scale).max(16.0);
    let pad_b = (st.doc.pad_b * scale).max(16.0);
    let inner_w = (page_w - pad_l - pad_r).max(40.0);
    let inner_h = (page_h - pad_t - pad_b).max(40.0);

    ensure_layout(&mut ui, st, inner_w, inner_h, scale);
    st.pages = st.page_ranges.len().max(1);

    let nav = crate::view::consume_key_nav(&mut ui);
    match nav {
        crate::view::KeyNav::Page(dir) => {
            let last = st.pages.saturating_sub(1);
            let next = (st.page as i32 + dir).clamp(0, last as i32) as usize;
            if next != st.page {
                st.pending_page = Some(next);
            }
        }
        _ => {}
    }
    if let Some(b) = jump.or(st.pending_block.take()) {
        if let Some(pi) = page_of_block(&st.page_ranges, b) {
            st.pending_page = Some(pi);
        }
    }
    let page_jump = st.pending_page.take();
    let mut action = WordAction::None;
    let find_l = find_q.trim().to_lowercase();

    let sa = crate::view::content_scroll(false)
        .id_salt("word_native_scroll")
        .max_height(max_h)
        .show(&mut ui, |ui| {
            crate::view::wheel_while_dragging(ui);
            if let crate::view::KeyNav::Line(d) = nav {
                ui.scroll_with_delta(d);
            }
            ui.set_min_width((page_w + 32.0).max(avail_w));
            ui.add_space(10.0);
            let clip = ui.clip_rect();
            let pages = st.page_ranges.clone();
            let frames = st.page_frame_h.clone();
            let n_pages = pages.len();
            let mut jump_rect = None;
            for (pi, &(a, b)) in pages.iter().enumerate() {
                let frame_h = frames.get(pi).copied().unwrap_or(page_h).max(page_h);
                let top = ui.cursor().top();
                if top <= clip.top() + 24.0 {
                    st.page = pi;
                    st.top_block = a;
                }
                let row_w = page_w.max(avail_w);
                let (row_rect, _) = ui.allocate_exact_size(vec2(row_w, frame_h), Sense::hover());
                let page_rect = Rect::from_center_size(row_rect.center(), vec2(page_w, frame_h));
                if !page_rect.intersects(clip) {
                    ui.add_space(PAGE_GAP);
                    continue;
                }
                ui.scope_builder(
                    UiBuilder::new()
                        .id_salt(("word_np", pi))
                        .max_rect(page_rect),
                    |ui| {
                        ui.set_clip_rect(page_rect.intersect(pane_clip));
                        ui.set_min_size(vec2(page_w, frame_h));
                        ui.set_max_size(vec2(page_w, frame_h));
                        Frame::new()
                            .fill(Color32::WHITE)
                            .stroke(Stroke::new(1.0_f32, PAGE_STROKE))
                            .inner_margin(Margin::ZERO)
                            .show(ui, |ui| {
                                ui.set_min_size(vec2(page_w, frame_h));
                                ui.set_max_size(vec2(page_w, frame_h));
                                ui.set_clip_rect(
                                    ui.max_rect().intersect(pane_clip).intersect(page_rect),
                                );
                                Frame::new()
                                    .inner_margin(Margin {
                                        left: pad_l.min(120.0) as i8,
                                        right: pad_r.min(120.0) as i8,
                                        top: pad_t.min(120.0) as i8,
                                        bottom: pad_b.min(120.0) as i8,
                                    })
                                    .show(ui, |ui| {
                                        ui.set_min_width(inner_w);
                                        ui.set_max_width(inner_w);
                                        ui.spacing_mut().item_spacing.y = 0.0;
                                        if a < b {
                                            let act = paint_blocks(
                                                ui,
                                                st,
                                                a,
                                                b,
                                                inner_w,
                                                scale,
                                                &find_l,
                                            );
                                            if !matches!(act, WordAction::None) {
                                                action = act;
                                            }
                                        }
                                    });
                                let label = format!("{} / {}", pi + 1, n_pages.max(1));
                                let pr = ui.max_rect();
                                ui.painter().text(
                                    pos2(pr.right() - 10.0, pr.bottom() - 8.0),
                                    egui::Align2::RIGHT_BOTTOM,
                                    label,
                                    FontId::proportional(11.0),
                                    Color32::from_rgb(0x9C, 0xA3, 0xAF),
                                );
                            });
                    },
                );
                if page_jump == Some(pi) {
                    jump_rect = Some(page_rect);
                }
                ui.add_space(PAGE_GAP);
            }
            if let Some(r) = jump_rect {
                ui.scroll_to_rect(r, Some(Align::TOP));
            }
        });
    let _ = sa;
    action
}

fn ensure_layout(ui: &mut Ui, st: &mut WordSession, inner_w: f32, inner_h: f32, scale: f32) {
    if (st.layout_w - inner_w).abs() < 0.5 && (st.layout_zoom - scale).abs() < 0.001
        && st.block_hs.len() == st.doc.blocks.len()
        && !st.page_ranges.is_empty()
    {
        return;
    }
    st.layout_w = inner_w;
    st.layout_zoom = scale;
    st.block_hs.clear();
    for b in &st.doc.blocks {
        let h = measure_block(ui, b, &st.doc, inner_w, scale);
        st.block_hs.push(h);
    }
    let (ranges, frames) = pack_pages(&st.doc.blocks, &st.block_hs, inner_h, inner_h + st.doc.pad_t * scale + st.doc.pad_b * scale);
    st.page_ranges = ranges;
    st.page_frame_h = frames;
    if st.page_ranges.is_empty() {
        st.page_ranges.push((0, st.doc.blocks.len()));
        st.page_frame_h.push(inner_h + st.doc.pad_t * scale + st.doc.pad_b * scale);
    }
}

fn pack_pages(
    blocks: &[WordBlock],
    heights: &[f32],
    inner_h: f32,
    page_min_h: f32,
) -> (Vec<(usize, usize)>, Vec<f32>) {
    let n = blocks.len();
    if n == 0 {
        return (Vec::new(), Vec::new());
    }
    let limit = inner_h.max(40.0);
    let mut pages = Vec::new();
    let mut frames = Vec::new();
    let mut start = 0usize;
    let mut used = 0.0;
    let mut i = 0usize;
    while i < n {
        if matches!(blocks[i], WordBlock::PageBreak) {
            if i > start {
                pages.push((start, i));
                frames.push(page_min_h.max(used + (page_min_h - inner_h)));
            } else if i == start && i > 0 {
                // 连续分页符：空页
                pages.push((start, i));
                frames.push(page_min_h);
            }
            start = i + 1;
            used = 0.0;
            i += 1;
            continue;
        }
        let h = heights.get(i).copied().unwrap_or(20.0).max(4.0);
        if i > start && used + h > limit {
            pages.push((start, i));
            frames.push(page_min_h.max(used + (page_min_h - inner_h)));
            start = i;
            used = 0.0;
        }
        used += h;
        i += 1;
    }
    if start < n {
        pages.push((start, n));
        frames.push(page_min_h.max(used + (page_min_h - inner_h)));
    }
    if pages.is_empty() {
        pages.push((0, n));
        frames.push(page_min_h);
    }
    (pages, frames)
}

fn page_of_block(pages: &[(usize, usize)], block: usize) -> Option<usize> {
    pages.iter().position(|&(a, b)| a <= block && block < b)
}

fn measure_block(ui: &mut Ui, b: &WordBlock, doc: &WordDoc, inner_w: f32, scale: f32) -> f32 {
    match b {
        WordBlock::Para(p) => measure_para(ui, p, inner_w, scale),
        WordBlock::Hr => 16.0 * scale,
        WordBlock::PageBreak => 0.0,
        WordBlock::Image { id, .. } => {
            let Some(im) = doc.images.get(*id) else {
                return 8.0;
            };
            let max_w = inner_w.max(8.0);
            let (_w, h) = fit_image(im, max_w, scale);
            h + 8.0 * scale
        }
        WordBlock::Table(t) => measure_table(ui, t, doc, inner_w, scale),
    }
}

fn fit_image(im: &WordImage, max_w: f32, scale: f32) -> (f32, f32) {
    let mut w = (im.w_dip * scale).max(8.0);
    let mut h = (im.h_dip * scale).max(8.0);
    if w > max_w {
        let s = max_w / w;
        w = max_w;
        h *= s;
    }
    (w, h)
}

fn measure_para(ui: &mut Ui, p: &WordPara, inner_w: f32, scale: f32) -> f32 {
    let hang = p.hanging * scale;
    let indent = p.indent * scale;
    let wrap = (inner_w - indent).max(20.0);
    let body_w = if hang > 1.0 { (wrap - hang).max(20.0) } else { wrap };
    let job = para_job(p, body_w, scale, "");
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    p.space_before * scale + galley.size().y.max(p_runs_line(p, scale)) + p.space_after * scale
}

fn p_runs_line(p: &WordPara, scale: f32) -> f32 {
    let sz = p
        .runs
        .iter()
        .map(|r| r.size * scale)
        .fold(14.0 * scale, f32::max);
    sz * 1.15
}

fn measure_table(ui: &mut Ui, t: &WordTable, doc: &WordDoc, inner_w: f32, scale: f32) -> f32 {
    if t.rows.is_empty() {
        return 8.0;
    }
    let mut h = 8.0 * scale;
    let n_cols = t
        .rows
        .iter()
        .map(|r| r.cells.iter().map(|c| c.col_span.max(1)).sum::<u32>())
        .max()
        .unwrap_or(1)
        .max(1);
    let col_w = ((inner_w - 2.0) / n_cols as f32).max(16.0);
    for row in &t.rows {
        let mut rh = 0.0f32;
        for cell in &row.cells {
            let cw = col_w * cell.col_span.max(1) as f32 - 12.0 * scale;
            let mut ch = 8.0 * scale;
            for b in &cell.blocks {
                ch += measure_block(ui, b, doc, cw.max(20.0), scale);
            }
            rh = rh.max(ch);
        }
        h += rh.max(20.0 * scale);
    }
    h
}

fn para_job(p: &WordPara, wrap_w: f32, scale: f32, find_l: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_w.max(8.0);
    job.wrap.break_anywhere = true;
    job.halign = p.align;
    for r in &p.runs {
        append_run(&mut job, r, scale, find_l);
    }
    if job.sections.is_empty() {
        let dummy = WordRun {
            text: "\u{00A0}".into(),
            bold: p.heading > 0,
            italic: false,
            strike: false,
            underline: false,
            size: 10.5 * 96.0 / 72.0,
            color: Color32::from_rgb(0x1F, 0x29, 0x37),
            href: None,
        };
        append_run(&mut job, &dummy, scale, find_l);
    }
    job
}

fn append_run(job: &mut LayoutJob, r: &WordRun, scale: f32, find_l: &str) {
    let size = (r.size * scale).clamp(8.0, 96.0);
    let family = if r.bold {
        theme::bold_family()
    } else {
        theme::preview_family()
    };
    let color = if r.href.is_some() { LINK } else { r.color };
    let fmt = TextFormat {
        font_id: FontId::new(size, family),
        color,
        italics: r.italic,
        underline: if r.underline || r.href.is_some() {
            Stroke::new(1.0_f32, color)
        } else {
            Stroke::NONE
        },
        strikethrough: if r.strike {
            Stroke::new(1.0_f32, color)
        } else {
            Stroke::NONE
        },
        line_height: Some(size * 1.15),
        extra_letter_spacing: 0.0,
        background: Color32::TRANSPARENT,
        valign: Align::BOTTOM,
        ..Default::default()
    };
    if find_l.is_empty() || r.text.is_empty() {
        job.append(&r.text, 0.0, fmt);
        return;
    }
    let lower = r.text.to_lowercase();
    let mut pos = 0usize;
    while pos < r.text.len() {
        let rest_l = &lower[pos..];
        if let Some(rel) = rest_l.find(find_l) {
            let hit = pos + rel;
            if hit > pos {
                job.append(&r.text[pos..hit], 0.0, fmt.clone());
            }
            let end = (hit + find_l.len()).min(r.text.len());
            let mut hl = fmt.clone();
            hl.background = FIND_BG;
            job.append(&r.text[hit..end], 0.0, hl);
            pos = end;
        } else {
            job.append(&r.text[pos..], 0.0, fmt.clone());
            break;
        }
    }
}

fn paint_blocks(
    ui: &mut Ui,
    st: &mut WordSession,
    a: usize,
    b: usize,
    inner_w: f32,
    scale: f32,
    find_l: &str,
) -> WordAction {
    let mut action = WordAction::None;
    let blocks: Vec<WordBlock> = st.doc.blocks[a..b].to_vec();
    for (k, blk) in blocks.iter().enumerate() {
        let act = paint_block(ui, st, blk, a + k, inner_w, scale, find_l);
        if !matches!(act, WordAction::None) {
            action = act;
        }
    }
    action
}

fn paint_block(
    ui: &mut Ui,
    st: &mut WordSession,
    blk: &WordBlock,
    _idx: usize,
    inner_w: f32,
    scale: f32,
    find_l: &str,
) -> WordAction {
    match blk {
        WordBlock::PageBreak => WordAction::None,
        WordBlock::Hr => {
            ui.add_space(6.0 * scale);
            let r = ui.available_rect_before_wrap();
            let y = ui.cursor().top();
            ui.painter()
                .hline(r.x_range(), y, Stroke::new(1.0_f32, HR));
            ui.add_space(10.0 * scale);
            WordAction::None
        }
        WordBlock::Para(p) => paint_para(ui, p, inner_w, scale, find_l),
        WordBlock::Image { id, center } => paint_image(ui, st, *id, *center, inner_w, scale),
        WordBlock::Table(t) => paint_table(ui, st, t, inner_w, scale, find_l),
    }
}

fn paint_para(ui: &mut Ui, p: &WordPara, inner_w: f32, scale: f32, find_l: &str) -> WordAction {
    ui.add_space(p.space_before * scale);
    let indent = p.indent * scale;
    let hang = p.hanging * scale;
    let wrap = (inner_w - indent).max(20.0);
    let href = p.runs.iter().find_map(|r| r.href.clone());
    let mut clicked = false;
    ui.horizontal(|ui| {
        if indent > 0.5 {
            ui.add_space(indent);
        }
        if let Some(m) = &p.marker {
            let mark_run = WordRun {
                text: m.clone(),
                bold: p.heading > 0 || p.runs.iter().any(|r| r.bold),
                italic: false,
                strike: false,
                underline: false,
                size: p.runs.first().map(|r| r.size).unwrap_or(14.0),
                color: Color32::from_rgb(0x1F, 0x29, 0x37),
                href: None,
            };
            let mw = hang.max(14.0 * scale);
            ui.set_max_width(wrap);
            let mut j = LayoutJob::default();
            j.wrap.max_width = mw;
            append_run(&mut j, &mark_run, scale, "");
            ui.add(Label::new(j).selectable(true));
        }
        let body_w = if p.marker.is_some() {
            (wrap - hang.max(14.0 * scale)).max(20.0)
        } else {
            wrap
        };
        ui.vertical(|ui| {
            ui.set_max_width(body_w);
            ui.set_min_width(body_w.min(ui.available_width()));
            let job = para_job(p, body_w, scale, find_l);
            let resp = ui.add(Label::new(job).selectable(true));
            if href.is_some() {
                resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    clicked = true;
                }
            }
        });
    });
    ui.add_space(p.space_after * scale);
    if clicked {
        if let Some(h) = href {
            return WordAction::OpenHref(h);
        }
    }
    WordAction::None
}

fn paint_image(
    ui: &mut Ui,
    st: &mut WordSession,
    id: usize,
    center: bool,
    inner_w: f32,
    scale: f32,
) -> WordAction {
    let Some(im) = st.doc.images.get(id).cloned() else {
        return WordAction::None;
    };
    let (w, h) = fit_image(&im, inner_w.max(8.0), scale);
    let raster = st.raster(ui.ctx(), id);
    let mut action = WordAction::None;
    ui.add_space(4.0 * scale);
    if center {
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            action = image_widget(ui, raster.as_ref(), &im, w, h);
        });
    } else {
        action = image_widget(ui, raster.as_ref(), &im, w, h);
    }
    ui.add_space(4.0 * scale);
    action
}

fn image_widget(ui: &mut Ui, raster: Option<&Raster>, im: &WordImage, w: f32, h: f32) -> WordAction {
    let Some(raster) = raster else {
        ui.label(
            RichText::new(if im.alt.is_empty() {
                "[img]".into()
            } else {
                format!("[img] {}", im.alt)
            })
            .color(Color32::from_rgb(0x6B, 0x72, 0x80)),
        );
        return WordAction::None;
    };
    let title = if im.alt.trim().is_empty() {
        crate::i18n::t().image.to_string()
    } else {
        im.alt.clone()
    };
    let resp = ui
        .add(egui::Image::new((raster.tex.id(), Vec2::new(w, h))).sense(Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(crate::i18n::t().dblclick_preview_copy);
    match img_preview::interact_thumb(&resp) {
        ThumbAction::Preview => WordAction::OpenImage {
            raster: raster.clone(),
            title,
        },
        ThumbAction::CopyImage => WordAction::CopyImage(raster.clone()),
        ThumbAction::CopyFile => WordAction::CopyAsFile(raster.clone()),
        ThumbAction::None => WordAction::None,
    }
}

fn paint_table(
    ui: &mut Ui,
    st: &mut WordSession,
    t: &WordTable,
    inner_w: f32,
    scale: f32,
    find_l: &str,
) -> WordAction {
    if t.rows.is_empty() {
        return WordAction::None;
    }
    let n_cols = t
        .rows
        .iter()
        .map(|r| r.cells.iter().map(|c| c.col_span.max(1)).sum::<u32>())
        .max()
        .unwrap_or(1)
        .max(1) as usize;
    let col_w = ((inner_w - 1.0) / n_cols as f32).max(16.0);
    let mut action = WordAction::None;
    ui.add_space(8.0 * scale);
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    for (ri, row) in t.rows.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for (ci, cell) in row.cells.iter().enumerate() {
                let cw = col_w * cell.col_span.max(1) as f32;
                let fill = if row.header {
                    HEADER_BG
                } else {
                    Color32::WHITE
                };
                ui.allocate_ui(vec2(cw, 1.0), |ui| {
                    Frame::new()
                        .fill(fill)
                        .stroke(Stroke::new(0.5_f32, CELL_BORDER))
                        .inner_margin(Margin::symmetric((6.0 * scale) as i8, (4.0 * scale) as i8))
                        .show(ui, |ui| {
                            ui.set_min_width((cw - 12.0 * scale).max(8.0));
                            ui.set_max_width((cw - 12.0 * scale).max(8.0));
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.push_id(("wcell", ri, ci), |ui| {
                                for b in &cell.blocks {
                                    let act = paint_block(
                                        ui,
                                        st,
                                        b,
                                        0,
                                        (cw - 12.0 * scale).max(20.0),
                                        scale,
                                        find_l,
                                    );
                                    if !matches!(act, WordAction::None) {
                                        action = act;
                                    }
                                }
                            });
                        });
                });
            }
        });
    }
    ui.add_space(8.0 * scale);
    action
}

fn decode_rgba(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let w = rgba.width();
    let h = rgba.height();
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h, rgba.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::word::{WordBlock, WordPara, WordRun};

    fn dummy_para() -> WordBlock {
        WordBlock::Para(WordPara {
            runs: vec![WordRun {
                text: "x".into(),
                bold: false,
                italic: false,
                strike: false,
                underline: false,
                size: 14.0,
                color: Color32::BLACK,
                href: None,
            }],
            align: Align::LEFT,
            indent: 0.0,
            hanging: 0.0,
            space_before: 0.0,
            space_after: 0.0,
            heading: 0,
            marker: None,
        })
    }

    #[test]
    fn pack_splits_when_full() {
        let blocks = vec![dummy_para(), dummy_para(), dummy_para()];
        let hs = vec![40.0, 40.0, 40.0];
        let (pages, _) = pack_pages(&blocks, &hs, 50.0, 80.0);
        assert!(pages.len() >= 2);
        assert_eq!(pages[0].0, 0);
        assert_eq!(pages.last().unwrap().1, 3);
    }

    #[test]
    fn pack_page_break() {
        let blocks = vec![dummy_para(), WordBlock::PageBreak, dummy_para()];
        let hs = vec![10.0, 0.0, 10.0];
        let (pages, _) = pack_pages(&blocks, &hs, 400.0, 400.0);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0], (0, 1));
        assert_eq!(pages[1], (2, 3));
    }
}
