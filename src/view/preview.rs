use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;

use egui::{
    pos2, text_selection::LabelSelectionState, vec2, Align, Align2, Color32, Frame, Label, Layout,
    Margin, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};

use crate::io::imgcache::{ImgCache, Raster};
use crate::io::mermaid::{self, MermaidCache, MermaidReady};
use crate::parser::table as tbl;
use crate::parser::{HeadingNumber, MdBlock, MdBlockKind, MdDoc, MdSpan, MdSpanKind, TableAlign};
use crate::view::highlight;
use crate::view::img_preview::{self, ThumbAction};
use crate::view::theme;

pub enum PreviewEvent {
    OpenHref(String),
    OpenImage { raster: Raster, title: String },
    CopyImage(Raster),
    CopyAsFile(Raster),
}

pub struct PreviewState {
    pub block_tops: Vec<f32>,
    pub block_heights: Vec<f32>,
    /// 折叠的标题（键为源码行号 `line0`）。
    pub collapsed_heads: HashSet<usize>,
    /// 超过 10 行仍展开的代码块（键为 `line0`）。默认折叠。
    pub code_open: HashSet<usize>,
    /// 预览视口顶部的屏幕 Y，用于大纲高亮。
    pub viewport_top: f32,
    /// 视口顶部对应的源行（0-based）。
    pub top_line: usize,
    pub offset_y: f32,
    pub hovered: bool,
    pub hint_line0: Option<usize>,
    pub hint_line1: Option<usize>,
    pub hint_text: String,
    pub pick_lines: Option<(usize, usize)>,
    pick_anchor: Option<usize>,
    hl_block: bool,
    /// Word 分页：当前视口所在页（0-based）与总页数。
    pub word_page: usize,
    pub word_pages: usize,
    /// Word 页缩放，1.0 = 100%（A4 96 DPI）。
    pub word_zoom: f32,
    /// PgUp/PgDn 目标页（0-based）。
    pending_word_page: Option<usize>,
    /// 顶层块指纹（不含源行号，插入上方不污染后续段）。
    fingerprints: Vec<u64>,
    last_page_w: f32,
}

impl PreviewState {
    pub fn request_word_page(&mut self, page0: usize) {
        self.pending_word_page = Some(page0);
    }

    pub fn clear_pick(&mut self) {
        self.pick_lines = None;
        self.pick_anchor = None;
    }
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            block_tops: Vec::new(),
            block_heights: Vec::new(),
            collapsed_heads: HashSet::new(),
            code_open: HashSet::new(),
            viewport_top: 0.0,
            top_line: 0,
            offset_y: 0.0,
            hovered: false,
            hint_line0: None,
            hint_line1: None,
            hint_text: String::new(),
            pick_lines: None,
            pick_anchor: None,
            hl_block: false,
            word_page: 0,
            word_pages: 0,
            word_zoom: 1.0,
            pending_word_page: None,
            fingerprints: Vec::new(),
            last_page_w: 0.0,
        }
    }
}

const BASE_FS: f32 = 14.0;
/// 源码空行在预览中占的高度（对齐 mdview 空行 / docview 列表 line-height 1.45）。
const BLANK_H: f32 = BASE_FS * 1.45;
const CODE_PAD_X: f32 = 4.0;
const CODE_ROUND: f32 = 3.0;
/// 等宽字形墨水在 metrics 上半，按字号下移，让字落在灰底中间。
const CODE_INK_DOWN: f32 = 0.24;
/// 灰底相对行再略下移，对齐汉字字身。
const CODE_CHIP_DOWN: f32 = 0.10;
const CODE_CHIP: Color32 = Color32::from_rgb(0xF3, 0xF4, 0xF6);
const CODE_FG: Color32 = Color32::from_rgb(0x1F, 0x29, 0x37);
const HEAD_SIZES: [f32; 6] = [28.0, 21.7, 17.5, 15.4, 14.0, 14.0];
const LIST_INDENT_STEP: f32 = 25.0;
const CODE_FOLD_LINES: usize = 10;
const HEAD_FOLD_W: f32 = 16.0;
const SEL_BG: Color32 = Color32::from_rgb(0xBF, 0xDB, 0xFE);

#[derive(Clone, Copy)]
pub struct PreviewOpts {
    pub heading_auto_number: bool,
    pub tab_size: i32,
    pub img_max_width: i32,
}

impl Default for PreviewOpts {
    fn default() -> Self {
        Self {
            heading_auto_number: true,
            tab_size: 3,
            img_max_width: 0,
        }
    }
}

fn c(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn block_fp(b: &MdBlock) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    hash_block(b, &mut h);
    h.finish()
}

fn hash_block(b: &MdBlock, h: &mut impl Hasher) {
    std::mem::discriminant(&b.kind).hash(h);
    b.level.hash(h);
    b.ordered.hash(h);
    b.task.hash(h);
    b.lang.hash(h);
    b.text.hash(h);
    b.summary.hash(h);
    b.details_open.hash(h);
    for s in &b.spans {
        std::mem::discriminant(&s.kind).hash(h);
        s.text.hash(h);
        s.href.hash(h);
    }
    b.table_rows.hash(h);
    for a in &b.table_align {
        std::mem::discriminant(a).hash(h);
    }
    b.img_w.map(|v| v.to_bits()).hash(h);
    b.img_h.map(|v| v.to_bits()).hash(h);
    for c in &b.children {
        hash_block(c, h);
    }
}

/// 对齐 mdview：指纹前后缀对齐，迁移未变块高度。
/// 返回脏区 `[lo, hi)`（新块下标）以及折行宽是否变了（变了则视口外也不能跳过）。
/// 可见块每帧仍会画（链接/折叠/自动序号）；只跳过视口外且未脏的块。
fn sync_preview_incr(
    st: &mut PreviewState,
    blocks: &[MdBlock],
    page_w: f32,
) -> (usize, usize, bool) {
    let fps: Vec<u64> = blocks.iter().map(block_fp).collect();
    let n = blocks.len();
    let wrap_changed = st.last_page_w > 1.0 && (st.last_page_w - page_w).abs() > 1.0;
    let (lo, hi_old, hi_new) = if wrap_changed || st.fingerprints.is_empty() {
        (0, st.fingerprints.len(), n)
    } else {
        crate::view::incr::diff_fps(&st.fingerprints, &fps)
    };
    let mut new_h = vec![28.0f32; n];
    if !wrap_changed && st.block_heights.len() == st.fingerprints.len() {
        for i in 0..lo.min(n) {
            new_h[i] = st.block_heights[i];
        }
        let n_suf = n.saturating_sub(hi_new);
        for k in 0..n_suf {
            let oi = hi_old + k;
            let ni = hi_new + k;
            if oi < st.block_heights.len() && ni < n {
                new_h[ni] = st.block_heights[oi];
            }
        }
    }
    st.block_heights = new_h;
    st.block_tops.clear();
    st.block_tops.resize(n, 0.0);
    st.fingerprints = fps;
    st.last_page_w = page_w;
    (lo, hi_new, wrap_changed)
}

/// 跳过视口外块时仍推进列表序号 / 标题自动编号，避免后面可见块编号错。
fn bump_skip_state(
    b: &MdBlock,
    ol: &mut HashMap<u32, i32>,
    head_num: &mut HeadingNumber,
    opts: PreviewOpts,
) {
    if b.kind == MdBlockKind::ListItem {
        ol.retain(|&k, _| k <= b.level);
        if b.ordered {
            *ol.entry(b.level).or_insert(0) += 1;
        }
    }
    if b.kind == MdBlockKind::Heading && opts.heading_auto_number && !b.text.trim().is_empty() {
        let lv = b.level.clamp(1, 6) as i32;
        let _ = head_num.next(lv);
    }
}

/// Markdown 块级预览（对齐 MdFlowBuilder 色值/间距）。
pub fn show(
    ui: &mut Ui,
    doc: &MdDoc,
    st: &mut PreviewState,
    img: &mut ImgCache,
    mermaid: &mut MermaidCache,
    base: Option<&Path>,
    events: &mut Vec<PreviewEvent>,
    opts: PreviewOpts,
    jump_line: Option<usize>,
    caret_line: Option<usize>,
) {
    let mut ui = crate::view::pane_ui(ui);
    ui.painter().rect_filled(ui.max_rect(), 0.0, Color32::WHITE);
    arm_preview_copy_fix(&ui);
    if ui.input(|i| i.pointer.primary_pressed()) {
        st.pick_lines = None;
        st.pick_anchor = None;
        ui.ctx()
            .data_mut(|d| d.remove::<PreviewLinePick>(egui::Id::new("preview_line_pick")));
    }
    let max_h = ui.available_height();
    let nav = crate::view::consume_key_nav(&mut ui);
    let sa = crate::view::content_scroll(true)
        .id_salt("preview_scroll")
        .max_height(max_h)
        .show(&mut ui, |ui| {
            crate::view::wheel_while_dragging(ui);
            crate::view::apply_key_nav_scroll(ui, nav, None);
            st.viewport_top = ui.clip_rect().top();
            ui.set_min_width(ui.available_width());
            let h = ui.available_height();
            if h.is_finite() {
                ui.set_min_height(h);
            }
            Frame::new()
                .inner_margin(Margin::symmetric(28, 20))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let page_w = ui.available_width();
                    if doc.blocks.is_empty()
                        || doc.blocks.iter().all(|b| b.kind == MdBlockKind::Blank)
                    {
                        ui.label(
                            RichText::new(crate::i18n::t().empty_doc).color(c(0x4B, 0x55, 0x63)),
                        );
                        return;
                    }
                    let (dirty_lo, dirty_hi, wrap_changed) =
                        sync_preview_incr(st, &doc.blocks, page_w);
                    let mut ol: HashMap<u32, i32> = HashMap::new();
                    let mut head_num = HeadingNumber::default();
                    render_blocks(
                        ui,
                        &doc.blocks,
                        st,
                        img,
                        mermaid,
                        base,
                        events,
                        page_w,
                        true,
                        opts,
                        caret_line,
                        0,
                        &mut ol,
                        &mut head_num,
                        dirty_lo,
                        dirty_hi,
                        !wrap_changed,
                    );
                    st.top_line = 0;
                    for (i, b) in doc.blocks.iter().enumerate() {
                        if i < st.block_tops.len() && st.block_tops[i] <= st.viewport_top + 12.0 {
                            st.top_line = b.line0;
                        }
                    }
                    if let Some(line) = jump_line {
                        let bi = crate::parser::block_index_for_line(doc, line);
                        if let Some(&y) = st.block_tops.get(bi) {
                            let r = Rect::from_min_size(
                                pos2(ui.max_rect().left(), y),
                                vec2(ui.max_rect().width().max(8.0), 24.0),
                            );
                            ui.scroll_to_rect(r, Some(Align::TOP));
                        }
                    }
                });
        });
    st.offset_y = sa.state.offset.y;
    let pane = ui.max_rect();
    st.hovered = ui.rect_contains_pointer(pane);
}

fn blank_takes_space(_blocks: &[MdBlock], _i: usize) -> bool {
    true
}

/// egui 跨 Label 复制会在相邻控件之间插空格：汉字之间、中英/数字与汉字之间挤掉；英文词之间保留。
fn squeeze_cjk_spaces(s: &str) -> String {
    let chs: Vec<char> = s.chars().filter(|&c| c != '\u{200B}').collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chs.len() {
        if chs[i] == ' ' {
            let mut j = i;
            while j < chs.len() && chs[j] == ' ' {
                j += 1;
            }
            let prev = out.chars().last();
            let next = chs.get(j).copied();
            let drop = match (prev, next) {
                (Some(a), Some(b)) => {
                    let ac = is_cjk(a);
                    let bc = is_cjk(b);
                    (ac && bc) || (ac && b.is_ascii()) || (a.is_ascii() && bc)
                }
                _ => false,
            };
            if !drop {
                for _ in i..j {
                    out.push(' ');
                }
            }
            i = j;
        } else {
            out.push(chs[i]);
            i += 1;
        }
    }
    out
}

#[derive(Default)]
struct PreviewCopyFix {
    squeeze: bool,
}

impl egui::Plugin for PreviewCopyFix {
    fn debug_name(&self) -> &'static str {
        "preview_copy_fix"
    }

    fn on_begin_pass(&mut self, _ctx: &egui::Context) {
        self.squeeze = false;
    }

    fn output_hook(&mut self, output: &mut egui::FullOutput) {
        if !self.squeeze {
            return;
        }
        for cmd in &mut output.platform_output.commands {
            if let egui::OutputCommand::CopyText(s) = cmd {
                *s = squeeze_cjk_spaces(s);
            }
        }
    }
}

fn arm_preview_copy_fix(ui: &Ui) {
    if !ui.rect_contains_pointer(ui.max_rect()) {
        return;
    }
    ui.ctx().plugin_or_default::<PreviewCopyFix>().lock().squeeze = true;
}

/// 与 Label 相同：可拖选，但不抢 Tab 焦点。
fn select_sense() -> Sense {
    let mut s = Sense::click_and_drag();
    s -= Sense::FOCUSABLE;
    s
}

/// 块间空隙做成可选中 galley，从空白处按下也能拖选中间正文。
fn sel_gap(ui: &mut Ui, h: f32) {
    let rem = ui.available_size_before_wrap().x;
    if !rem.is_finite() || rem <= 0.5 {
        ui.add_space(h);
        return;
    }
    sel_gap_size(ui, rem, h);
}

fn sel_gap_size(ui: &mut Ui, w: f32, h: f32) {
    let h = h.max(1.0);
    let w = w.max(1.0);
    let (rect, response) = ui.allocate_exact_size(vec2(w, h), select_sense());
    let multi = ui.input(|i| {
        i.pointer.button_double_clicked(egui::PointerButton::Primary)
            || i.pointer.button_triple_clicked(egui::PointerButton::Primary)
    });
    if !multi {
        // 空 galley：可起选，但复制时不会塞进空格。双击帧不要交给 egui 分词（会崩）。
        let galley = ui.painter().layout_no_wrap(
            String::new(),
            egui::FontId::proportional(1.0),
            Color32::TRANSPARENT,
        );
        LabelSelectionState::label_text_selection(
            ui,
            &response,
            rect.left_top(),
            galley,
            Color32::TRANSPARENT,
            Stroke::NONE,
        );
    }
    // gap 可起选/触发双击，但不进双击高亮 rect（避免短行右侧空白铺蓝）。
    span_group_note(ui, &response, false);
    // 单击空白不留选区；按住拖过才选中中间正文。
    if response.clicked() && !response.double_clicked() && !response.triple_clicked() {
        ui.ctx()
            .plugin::<LabelSelectionState>()
            .lock()
            .clear_selection();
    }
}

/// 吃掉当前行剩余宽度，短行右侧空白也能起选。已在行首则不动，避免多垫一行。
fn eat_line_rest(ui: &mut Ui, size: f32) {
    let rem = ui.available_size_before_wrap().x;
    let line_w = ui.max_rect().width();
    if !rem.is_finite() || rem <= 1.0 {
        return;
    }
    if rem >= line_w - 2.0 {
        return;
    }
    sel_gap_size(ui, rem, size * 1.45);
}

fn show_blank(ui: &mut Ui, blocks: &[MdBlock], i: usize) {
    if blank_takes_space(blocks, i) {
        sel_gap(ui, BLANK_H);
    }
}

fn hint_covers(st: &PreviewState, b: &MdBlock) -> bool {
    let (Some(a), Some(z)) = (st.hint_line0, st.hint_line1) else {
        return false;
    };
    b.kind != MdBlockKind::Blank && b.line1 >= a && b.line0 <= z
}

fn note_pick(ui: &Ui, st: &mut PreviewState, b: &MdBlock, rect: Rect) {
    if !ui.input(|i| i.pointer.primary_down()) || !ui.rect_contains_pointer(rect) {
        return;
    }
    if st.pick_anchor.is_none() {
        st.pick_anchor = Some(b.line0);
        st.pick_lines = Some((b.line0, b.line1));
        return;
    }
    let a = st.pick_anchor.unwrap_or(b.line0);
    let l0 = a.min(b.line0).min(b.line1);
    let l1 = a.max(b.line0).max(b.line1);
    st.pick_lines = Some((l0, l1));
}

fn piece_in_sel(hint: &str, piece: &str) -> bool {
    let p = piece.trim();
    let h = hint.trim();
    if p.is_empty() || h.is_empty() {
        return false;
    }
    if h.contains(p) {
        return true;
    }
    p.contains(h) && p.chars().count() <= h.chars().count().saturating_add(8)
}

fn rt_sel(rt: RichText, hl: bool) -> RichText {
    if hl {
        rt.background_color(SEL_BG)
    } else {
        rt
    }
}

fn job_sel(job: &mut egui::text::LayoutJob, hl: bool) {
    if !hl {
        return;
    }
    for sec in &mut job.sections {
        sec.format.background = SEL_BG;
    }
}

fn render_blocks(
    ui: &mut Ui,
    blocks: &[MdBlock],
    st: &mut PreviewState,
    img: &mut ImgCache,
    mermaid: &mut MermaidCache,
    base: Option<&Path>,
    events: &mut Vec<PreviewEvent>,
    page_w: f32,
    record_tops: bool,
    opts: PreviewOpts,
    caret_line: Option<usize>,
    index_base: usize,
    ol: &mut HashMap<u32, i32>,
    head_num: &mut HeadingNumber,
    dirty_lo: usize,
    dirty_hi: usize,
    skip_offscreen: bool,
) {
    let page = page_w.max(120.0);
    let img_max = if opts.img_max_width > 0 {
        page.min(opts.img_max_width as f32)
    } else {
        page
    };
    let clip = ui.clip_rect();
    let pad = clip.height().max(80.0);
    let vis_top = clip.top() - pad;
    let vis_bot = clip.bottom() + pad;
    let mut i = 0;
    while i < blocks.len() {
        let b = &blocks[i];
        if b.kind != MdBlockKind::ListItem {
            ol.clear();
        }
        let gi = index_base + i;
        if record_tops && gi < st.block_tops.len() {
            st.block_tops[gi] = ui.cursor().top();
        }
        let y0 = ui.cursor().top();
        let h_cached = st.block_heights.get(gi).copied().unwrap_or(28.0);
        let dirty = gi >= dirty_lo && gi < dirty_hi;
        let caret_here = caret_line
            .is_some_and(|cl| b.kind != MdBlockKind::Blank && b.line0 <= cl && cl <= b.line1);
        let hint_here = record_tops && hint_covers(st, b);
        let offscreen = y0 + h_cached < vis_top || y0 > vis_bot;
        let foldable = b.kind == MdBlockKind::Heading && heading_has_body(blocks, i);
        let collapsed = foldable && st.collapsed_heads.contains(&b.line0);

        if skip_offscreen
            && record_tops
            && !dirty
            && !caret_here
            && !hint_here
            && offscreen
            && h_cached > 1.0
        {
            bump_skip_state(b, ol, head_num, opts);
            ui.add_space(h_cached);
            if collapsed {
                let end = skip_heading_section(blocks, i);
                let y = ui.cursor().top();
                for k in (i + 1)..end {
                    let gk = index_base + k;
                    if gk < st.block_tops.len() {
                        st.block_tops[gk] = y;
                    }
                    if gk < st.block_heights.len() {
                        st.block_heights[gk] = 0.0;
                    }
                }
                i = end;
                continue;
            }
            i += 1;
            continue;
        }

        let mut collapsed_end = None;
        let hint = if hint_here {
            st.hint_text.clone()
        } else {
            String::new()
        };
        st.hl_block = !hint.is_empty();
        ui.push_id((b.line0, gi), |ui| match b.kind {
            MdBlockKind::Blank => show_blank(ui, blocks, i),
            MdBlockKind::Heading => {
                show_heading(
                    ui, b, head_num, img, base, events, img_max, opts, foldable, collapsed, st,
                    &hint,
                );
                if collapsed {
                    collapsed_end = Some(skip_heading_section(blocks, i));
                }
            }
            MdBlockKind::Paragraph => show_paragraph(ui, b, img, base, events, img_max, &hint),
            MdBlockKind::Quote => show_quote(ui, b, img, base, events, img_max, &hint),
            MdBlockKind::Code => show_code(ui, b, mermaid, img_max, events, st, &hint),
            MdBlockKind::Hr => {
                sel_gap(ui, BASE_FS * 1.25);
                let rect = ui.available_rect_before_wrap();
                let y = ui.cursor().top();
                ui.painter()
                    .hline(rect.x_range(), y, Stroke::new(2.0_f32, c(0xD1, 0xD5, 0xDB)));
                sel_gap(ui, BASE_FS * 1.25 + 2.0);
            }
            MdBlockKind::ListItem => show_list(ui, b, ol, img, base, events, img_max, opts, &hint),
            MdBlockKind::Table => show_table(ui, b, img, base, events, img_max, page_w, &hint),
            MdBlockKind::Html => {
                sel_gap(ui, BASE_FS * 0.5);
                Frame::new()
                    .fill(c(0xF9, 0xFA, 0xFB))
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        add_sel_label(
                            ui,
                            Label::new(rt_sel(
                                RichText::new(&b.text)
                                    .monospace()
                                    .size(12.0)
                                    .color(c(0x4B, 0x55, 0x63)),
                                piece_in_sel(&hint, &b.text),
                            )),
                        );
                    });
                sel_gap(ui, BASE_FS * 0.9);
            }
            MdBlockKind::HtmlImg => {
                sel_gap(ui, BASE_FS * 0.5);
                let href = if b.text.is_empty() {
                    b.spans.first().map(|s| s.href.as_str()).unwrap_or("")
                } else {
                    b.text.as_str()
                };
                let mut max_w = img_max;
                if let Some(w) = b.img_w {
                    if w > 0.0 {
                        max_w = img_max.min(w);
                    }
                }
                show_image(ui, href, "", img, base, max_w, b.img_w, b.img_h, events);
                sel_gap(ui, BASE_FS * 0.9);
            }
            MdBlockKind::Details => {
                sel_gap(ui, BASE_FS * 0.5);
                let title = if b.summary.is_empty() {
                    "Details"
                } else {
                    b.summary.as_str()
                };
                egui::CollapsingHeader::new(title)
                    .default_open(b.details_open)
                    .show(ui, |ui| {
                        let mut nested_ol: HashMap<u32, i32> = HashMap::new();
                        let mut nested_hn = HeadingNumber::default();
                        render_blocks(
                            ui,
                            &b.children,
                            st,
                            img,
                            mermaid,
                            base,
                            events,
                            page_w,
                            false,
                            opts,
                            caret_line,
                            0,
                            &mut nested_ol,
                            &mut nested_hn,
                            0,
                            usize::MAX,
                            false,
                        );
                    });
                sel_gap(ui, BASE_FS * 0.9);
            }
        });
        if record_tops {
            let y1 = ui.cursor().top().max(y0 + 6.0);
            let rect = Rect::from_min_max(
                pos2(ui.max_rect().left(), y0),
                pos2(ui.max_rect().right(), y1),
            );
            note_pick(ui, st, b, rect);
            if gi < st.block_heights.len() {
                st.block_heights[gi] = (y1 - y0).max(4.0);
            }
            if let Some(cl) = caret_line {
                if st.hint_line0.is_none()
                    && b.kind != MdBlockKind::Blank
                    && b.line0 <= cl
                    && cl <= b.line1
                {
                    let x = ui.max_rect().left();
                    ui.painter().rect_filled(
                        Rect::from_min_max(pos2(x, y0), pos2(x + 3.0, y1)),
                        0.0,
                        c(0x3B, 0x82, 0xF6),
                    );
                }
            }
        }
        if let Some(end) = collapsed_end {
            let y = ui.cursor().top();
            if record_tops {
                for k in (i + 1)..end {
                    let gk = index_base + k;
                    if gk < st.block_tops.len() {
                        st.block_tops[gk] = y;
                    }
                    if gk < st.block_heights.len() {
                        st.block_heights[gk] = 0.0;
                    }
                }
            }
            i = end;
            continue;
        }
        i += 1;
    }
}

/// 标题到下一同级/更高级标题之间是否有非空块。
fn heading_has_body(blocks: &[MdBlock], i: usize) -> bool {
    let lv = blocks[i].level;
    for b in &blocks[i + 1..] {
        if b.kind == MdBlockKind::Heading && b.level <= lv {
            break;
        }
        if b.kind != MdBlockKind::Blank {
            return true;
        }
    }
    false
}

fn skip_heading_section(blocks: &[MdBlock], i: usize) -> usize {
    let lv = blocks[i].level;
    let mut j = i + 1;
    while j < blocks.len() {
        if blocks[j].kind == MdBlockKind::Heading && blocks[j].level <= lv {
            break;
        }
        j += 1;
    }
    j
}

fn first_n_lines(s: &str, n: usize) -> String {
    let mut out = String::new();
    for (i, line) in s.lines().enumerate() {
        if i >= n {
            break;
        }
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

fn show_heading(
    ui: &mut Ui,
    b: &MdBlock,
    num: &mut HeadingNumber,
    img: &mut ImgCache,
    base: Option<&Path>,
    events: &mut Vec<PreviewEvent>,
    img_max: f32,
    opts: PreviewOpts,
    foldable: bool,
    collapsed: bool,
    st: &mut PreviewState,
    hint: &str,
) {
    let lv = b.level.clamp(1, 6) as usize;
    let size = HEAD_SIZES[lv - 1];
    sel_gap(ui, BASE_FS * 1.1);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.allocate_ui_with_layout(
            Vec2::new(HEAD_FOLD_W, size),
            Layout::left_to_right(Align::Center),
            |ui| {
                if foldable {
                    let tri = if collapsed { "▶" } else { "▼" };
                    let r = ui.add(
                        Label::new(RichText::new(tri).size(11.0).color(c(0x9C, 0xA3, 0xAF)))
                            .sense(Sense::click()),
                    );
                    if r.clicked() {
                        if collapsed {
                            st.collapsed_heads.remove(&b.line0);
                        } else {
                            st.collapsed_heads.insert(b.line0);
                        }
                    }
                    r.on_hover_text(if collapsed {
                        crate::i18n::t().expand
                    } else {
                        crate::i18n::t().collapse
                    });
                }
            },
        );
        text_flow(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if opts.heading_auto_number && !b.text.trim().is_empty() {
                ui.label(
                    RichText::new(format!("{} ", num.next(lv as i32)))
                        .size(size)
                        .family(theme::bold_family())
                        .color(c(0x4B, 0x55, 0x63)),
                );
            }
            let color = if lv >= 5 {
                c(0x37, 0x41, 0x51)
            } else {
                c(0x11, 0x18, 0x27)
            };
            show_spans(
                ui, &b.spans, img, base, events, img_max, size, color, true, hint,
            );
        });
    });
    if lv <= 2 {
        let y = ui.cursor().top() + BASE_FS * 0.3;
        let rect = ui.available_rect_before_wrap();
        let col = if lv == 1 {
            c(0xD1, 0xD5, 0xDB)
        } else {
            c(0xE5, 0xE7, 0xEB)
        };
        ui.painter()
            .hline(rect.x_range(), y, Stroke::new(1.0_f32, col));
        sel_gap(ui, BASE_FS * 0.5 + 2.0);
    } else {
        sel_gap(ui, BASE_FS * 0.5);
    }
}

fn text_flow<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let initial = vec2(
        ui.available_size_before_wrap().x,
        ui.spacing().interact_size.y,
    );
    ui.allocate_ui_with_layout(
        initial,
        Layout::left_to_right(Align::BOTTOM).with_main_wrap(true),
        add,
    )
}

fn show_paragraph(
    ui: &mut Ui,
    b: &MdBlock,
    img: &mut ImgCache,
    base: Option<&Path>,
    events: &mut Vec<PreviewEvent>,
    img_max: f32,
    hint: &str,
) {
    if b.spans.len() == 1 && b.spans[0].kind == MdSpanKind::Image {
        sel_gap(ui, BASE_FS * 0.6);
        show_image(
            ui,
            &b.spans[0].href,
            &b.spans[0].text,
            img,
            base,
            img_max,
            None,
            None,
            events,
        );
        sel_gap(ui, BASE_FS * 0.9);
        return;
    }
    show_spans(
        ui,
        &b.spans,
        img,
        base,
        events,
        img_max,
        BASE_FS,
        c(0x11, 0x18, 0x27),
        false,
        hint,
    );
    sel_gap(ui, BASE_FS * 0.75);
}

fn show_quote(
    ui: &mut Ui,
    b: &MdBlock,
    img: &mut ImgCache,
    base: Option<&Path>,
    events: &mut Vec<PreviewEvent>,
    img_max: f32,
    hint: &str,
) {
    sel_gap(ui, BASE_FS * 0.75);
    let out = Frame::new()
        .fill(c(0xF3, 0xF4, 0xF6))
        .inner_margin(Margin::symmetric(14, 8))
        .show(ui, |ui| {
            show_spans(
                ui,
                &b.spans,
                img,
                base,
                events,
                img_max,
                BASE_FS,
                c(0x4B, 0x55, 0x63),
                false,
                hint,
            );
        });
    // 竖线必须用布局后的实际高度。max_rect 是剩余可视区，会一直画到后面的标题/代码块。
    let r = out.response.rect;
    ui.painter().rect_filled(
        Rect::from_min_max(r.left_top(), egui::pos2(r.left() + 4.0, r.bottom())),
        0.0,
        c(0x9C, 0xA3, 0xAF),
    );
    sel_gap(ui, BASE_FS);
}

fn show_code(
    ui: &mut Ui,
    b: &MdBlock,
    mermaid: &mut MermaidCache,
    img_max: f32,
    events: &mut Vec<PreviewEvent>,
    st: &mut PreviewState,
    hint: &str,
) {
    if mermaid::is_mermaid_lang(&b.lang) {
        show_mermaid(ui, b, mermaid, img_max, events);
        return;
    }
    let n_lines = if b.text.is_empty() {
        0
    } else {
        b.text.lines().count()
    };
    let foldable = n_lines > CODE_FOLD_LINES;
    let expanded = st.code_open.contains(&b.line0);
    let folded = foldable && !expanded;
    sel_gap(ui, BASE_FS * 0.5);
    let frame_w = ui.available_width();
    Frame::new()
        .fill(c(0xF3, 0xF4, 0xF6))
        .stroke(Stroke::new(1.0_f32, c(0xD1, 0xD5, 0xDB)))
        .corner_radius(4.0)
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_min_width((frame_w - 22.0).max(1.0));
            ui.horizontal(|ui| {
                let lang = if b.lang.is_empty() {
                    "code"
                } else {
                    b.lang.as_str()
                };
                ui.label(
                    RichText::new(lang)
                        .italics()
                        .size(11.0)
                        .color(c(0x4B, 0x55, 0x63)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(crate::i18n::t().copy).clicked() {
                        ui.ctx().copy_text(b.text.clone());
                    }
                });
            });
            let src = if folded {
                first_n_lines(&b.text, CODE_FOLD_LINES)
            } else {
                b.text.clone()
            };
            let mut job = highlight::code_job(&src, &b.lang);
            job_sel(&mut job, piece_in_sel(hint, &src));
            add_sel_label(ui, egui::Label::new(job).wrap());
            if foldable {
                let more = n_lines.saturating_sub(CODE_FOLD_LINES);
                let txt = if expanded {
                    crate::i18n::t().code_fold_collapse.to_string()
                } else {
                    crate::i18n::code_fold_more(more)
                };
                let (rect, resp) = ui
                    .allocate_exact_size(vec2(ui.available_width().max(8.0), 18.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 0.0, c(0xE5, 0xE7, 0xEB));
                }
                let clicked = resp.clicked();
                ui.painter().text(
                    pos2(rect.left(), rect.center().y),
                    Align2::LEFT_CENTER,
                    txt,
                    egui::FontId::proportional(11.0),
                    c(0x6B, 0x72, 0x80),
                );
                resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(if expanded {
                        crate::i18n::t().collapse_code
                    } else {
                        crate::i18n::t().expand
                    });
                if clicked {
                    if expanded {
                        st.code_open.remove(&b.line0);
                    } else {
                        st.code_open.insert(b.line0);
                    }
                }
            }
        });
    sel_gap(ui, BASE_FS * 0.9);
}

fn show_mermaid(
    ui: &mut Ui,
    b: &MdBlock,
    mermaid: &mut MermaidCache,
    img_max: f32,
    events: &mut Vec<PreviewEvent>,
) {
    sel_gap(ui, BASE_FS * 0.5);
    Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0_f32, c(0xD1, 0xD5, 0xDB)))
        .corner_radius(4.0)
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("mermaid")
                        .italics()
                        .size(11.0)
                        .color(c(0x4B, 0x55, 0x63)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(crate::i18n::t().copy).clicked() {
                        ui.ctx().copy_text(b.text.clone());
                    }
                });
            });
            match mermaid.get(ui.ctx(), &b.text) {
                MermaidReady::Ready(raster) => {
                    let mut w = raster.size.x;
                    let mut h = raster.size.y;
                    if w > img_max {
                        h *= img_max / w;
                        w = img_max;
                    }
                    let title = "mermaid".to_string();
                    let resp = ui
                        .add(
                            egui::Image::new((raster.tex.id(), Vec2::new(w, h)))
                                .sense(Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(crate::i18n::t().dblclick_preview_copy);
                    push_thumb(events, &resp, raster, title);
                }
                MermaidReady::Loading => {
                    ui.label(
                        RichText::new(crate::i18n::t().rendering_chart)
                            .size(12.0)
                            .color(c(0x4B, 0x55, 0x63)),
                    );
                }
                MermaidReady::Failed(err) => {
                    ui.label(
                        RichText::new(crate::i18n::mermaid_fail(err))
                            .size(12.0)
                            .color(c(0xB9, 0x1C, 0x1C)),
                    );
                    let job = highlight::code_job(&b.text, "text");
                    add_sel_label(ui, egui::Label::new(job).wrap());
                }
            }
        });
    sel_gap(ui, BASE_FS * 0.9);
}

fn task_checkbox(ui: &mut Ui, checked: bool) {
    let size = Vec2::splat(16.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let vis = ui.visuals().widgets.noninteractive;
    ui.painter()
        .rect(rect, 2.0, vis.bg_fill, vis.bg_stroke, StrokeKind::Inside);
    if checked {
        let r = rect.shrink(3.5);
        ui.painter().add(Shape::line(
            vec![
                pos2(r.left(), r.center().y),
                pos2(r.center().x - 0.5, r.bottom() - 1.0),
                pos2(r.right(), r.top()),
            ],
            vis.fg_stroke,
        ));
    }
}

fn show_list(
    ui: &mut Ui,
    b: &MdBlock,
    ol: &mut HashMap<u32, i32>,
    img: &mut ImgCache,
    base: Option<&Path>,
    events: &mut Vec<PreviewEvent>,
    img_max: f32,
    opts: PreviewOpts,
    hint: &str,
) {
    let cols = b.level;
    ol.retain(|&k, _| k <= cols);
    let mark = if b.ordered {
        let n = ol.entry(cols).or_insert(0);
        *n += 1;
        format!("{}.", *n)
    } else {
        "•".to_string()
    };
    let tab = opts.tab_size.max(1) as f32;
    let pad = (cols as f32) * LIST_INDENT_STEP / tab;
    sel_gap(ui, BASE_FS * 0.12);
    text_flow(ui, |ui| {
        ui.add_space(pad);
        ui.label(
            RichText::new(format!("{mark}  "))
                .color(c(0x37, 0x41, 0x51))
                .size(BASE_FS),
        );
        if let Some(checked) = b.task {
            task_checkbox(ui, checked);
            ui.add_space(6.0);
        }
        show_spans(
            ui,
            &b.spans,
            img,
            base,
            events,
            img_max,
            BASE_FS,
            c(0x11, 0x18, 0x27),
            false,
            hint,
        );
    });
    sel_gap(ui, BASE_FS * 0.12);
}

fn show_table(
    ui: &mut Ui,
    b: &MdBlock,
    img: &mut ImgCache,
    base: Option<&Path>,
    events: &mut Vec<PreviewEvent>,
    img_max: f32,
    page_w: f32,
    hint: &str,
) {
    if b.table_rows.is_empty() {
        return;
    }
    let ncol = b
        .table_rows
        .iter()
        .map(|r| r.len())
        .max()
        .unwrap_or(0)
        .max(1);
    let widths: Vec<f32> = tbl::allocate_columns_dip(&b.table_rows, ncol, page_w as f64)
        .into_iter()
        .map(|w| w as f32)
        .collect();
    let nowrap = tbl::short_nowrap_columns(&b.table_rows, ncol, 24);
    let table_w: f32 = widths.iter().sum();
    let border_c = c(0xD1, 0xD5, 0xDB);
    let border = Stroke::new(1.0_f32, border_c);
    let head_bg = c(0xF3, 0xF4, 0xF6);
    sel_gap(ui, BASE_FS * 0.5);
    ui.push_id(b.line0, |ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let mut row_rects: Vec<Rect> = Vec::new();
        let mut col_xs: Vec<f32> = Vec::new();
        for (ri, row) in b.table_rows.iter().enumerate() {
            let bg_idx = ui.painter().add(Shape::Noop);
            let row_out = ui.push_id(ri, |ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(table_w, 0.0),
                egui::Layout::left_to_right(egui::Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    for ci in 0..ncol {
                        let cell = row.get(ci).map(|s| s.as_str()).unwrap_or("");
                        let w = widths.get(ci).copied().unwrap_or(80.0);
                        let align = b.table_align.get(ci).copied().unwrap_or(TableAlign::Left);
                        let cell_out = ui.push_id(ci, |ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(w, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_width(w);
                                ui.set_max_width(w);
                                ui.set_width(w);
                                ui.set_min_height(BASE_FS + 16.0);
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    ui.add_space(8.0);
                                    let inner = (w - 16.0).max(8.0);
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(inner, 0.0),
                                        egui::Layout::left_to_right(egui::Align::Min),
                                        |ui| {
                                            ui.set_max_width(inner);
                                            let col_nowrap =
                                                nowrap.get(ci).copied().unwrap_or(false);
                                            ui.style_mut().wrap_mode = Some(if col_nowrap {
                                                egui::TextWrapMode::Extend
                                            } else {
                                                egui::TextWrapMode::Wrap
                                            });
                                            let spans = crate::parser::parse_inlines(cell);
                                            if align != TableAlign::Left {
                                                let nat = inlines_natural_width(
                                                    ui,
                                                    &spans,
                                                    img,
                                                    base,
                                                    img_max.min(inner),
                                                    BASE_FS,
                                                    ri == 0,
                                                );
                                                let extra = (inner - nat).max(0.0);
                                                let pad = if align == TableAlign::Center {
                                                    extra * 0.5
                                                } else {
                                                    extra
                                                };
                                                if pad > 0.5 {
                                                    ui.add_space(pad);
                                                }
                                            }
                                            show_spans(
                                                ui,
                                                &spans,
                                                img,
                                                base,
                                                events,
                                                img_max.min(inner),
                                                BASE_FS,
                                                c(0x11, 0x18, 0x27),
                                                ri == 0,
                                                hint,
                                            );
                                        },
                                    );
                                });
                                ui.add_space(6.0);
                            },
                        )
                        });
                        if ri == 0 {
                            if ci == 0 {
                                col_xs.push(cell_out.inner.response.rect.left());
                            }
                            col_xs.push(cell_out.inner.response.rect.right());
                        }
                    }
                },
            )
            });
            let row_rect = row_out.inner.response.rect;
            let fill = if ri == 0 { head_bg } else { Color32::WHITE };
            ui.painter()
                .set(bg_idx, egui::epaint::RectShape::filled(row_rect, 0.0, fill));
            row_rects.push(row_rect);
        }
        if row_rects.is_empty() {
            return;
        }
        let mut table_rect = row_rects[0];
        for r in row_rects.iter().skip(1) {
            table_rect = table_rect.union(*r);
        }
        let painter = ui.painter();
        painter.rect_stroke(table_rect, 0.0, border, StrokeKind::Inside);
        if col_xs.len() >= 3 {
            for x in col_xs.iter().skip(1).take(col_xs.len() - 2) {
                painter.vline(*x, table_rect.y_range(), border);
            }
        }
        for r in row_rects.iter().take(row_rects.len() - 1) {
            painter.hline(table_rect.x_range(), r.bottom(), border);
        }
    });
    sel_gap(ui, BASE_FS * 0.9);
}

/// 软换行：吃掉当前行剩余宽度，下一截从整行左缘起排，避免嵌进上一行窄缝。
fn force_wrap_line(ui: &mut Ui) {
    let rem = ui.available_size_before_wrap().x;
    if rem.is_finite() && rem > 0.5 {
        ui.allocate_exact_size(Vec2::new(rem, 0.0), Sense::hover());
    }
}

fn is_cjk(c: char) -> bool {
    matches!(
        c,
        '\u{2E80}'..='\u{2EFF}'
            | '\u{2F00}'..='\u{2FDF}'
            | '\u{3000}'..='\u{303F}'
            | '\u{3040}'..='\u{30FF}'
            | '\u{3100}'..='\u{312F}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FF00}'..='\u{FFEF}'
            | '\u{AC00}'..='\u{D7AF}'
    )
}

/// 0 标点/空白，1 汉字，2 拉丁字母数字。egui 混字体不会对齐基线，中英必须拆开成各自 Label。
fn script_kind(s: &str) -> u8 {
    for c in s.chars() {
        if c.is_whitespace() {
            continue;
        }
        if is_cjk(c) {
            return 1;
        }
        if c.is_ascii_alphanumeric() {
            return 2;
        }
    }
    0
}

/// 按空白切开（空白附着在前一段），再按中/英脚本切开。
fn wrap_pieces(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    if s.is_empty() {
        return out;
    }
    let mut start = 0usize;
    let mut it = s.char_indices().peekable();
    while let Some((i, ch)) = it.next() {
        if ch.is_whitespace() {
            continue;
        }
        let ps = start;
        let mut end = i + ch.len_utf8();
        while let Some(&(j, c2)) = it.peek() {
            if c2.is_whitespace() {
                break;
            }
            it.next();
            end = j + c2.len_utf8();
        }
        while let Some(&(j, c2)) = it.peek() {
            if !c2.is_whitespace() {
                break;
            }
            it.next();
            end = j + c2.len_utf8();
        }
        split_script(&s[ps..end], &mut out);
        start = end;
    }
    if out.is_empty() {
        out.push(s);
    }
    out
}

fn split_script<'a>(tok: &'a str, out: &mut Vec<&'a str>) {
    if tok.is_empty() {
        return;
    }
    let mut run_start = 0usize;
    let mut run_kind: Option<u8> = None;
    for (byte_i, ch) in tok.char_indices() {
        if ch.is_whitespace() {
            continue;
        }
        let k = if is_cjk(ch) {
            1
        } else if ch.is_ascii_alphanumeric() {
            2
        } else {
            0
        };
        if k == 0 {
            continue;
        }
        match run_kind {
            None => run_kind = Some(k),
            Some(rk) if rk != k => {
                out.push(&tok[run_start..byte_i]);
                run_start = byte_i;
                run_kind = Some(k);
            }
            _ => {}
        }
    }
    out.push(&tok[run_start..]);
}

fn rich_width(ui: &Ui, rt: &RichText) -> f32 {
    egui::WidgetText::from(rt.clone())
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::FontSelection::Default,
        )
        .size()
        .x
}

fn pieces_width(ui: &Ui, text: &str, mut rt_fn: impl FnMut(&str) -> RichText) -> f32 {
    wrap_pieces(text)
        .iter()
        .map(|p| rich_width(ui, &rt_fn(p)))
        .sum()
}

/// 不折行时行内内容的自然宽度，用来在 LTR 布局里用左侧留白做右/居中对齐。
fn inlines_natural_width(
    ui: &Ui,
    spans: &[MdSpan],
    img: &mut ImgCache,
    base: Option<&Path>,
    img_max: f32,
    size: f32,
    heading_strong: bool,
) -> f32 {
    let color = c(0x11, 0x18, 0x27);
    let mut w = 0.0;
    for sp in spans {
        w += match sp.kind {
            MdSpanKind::SoftBr => 0.0,
            MdSpanKind::Image => {
                if let Some(raster) = img.get(ui.ctx(), &sp.href, base) {
                    raster.size.x.min(img_max)
                } else {
                    rich_width(
                        ui,
                        &RichText::new(format!("[img] {}", sp.href)).color(c(0x4B, 0x55, 0x63)),
                    )
                }
            }
            MdSpanKind::Text => pieces_width(ui, &sp.text, |t| {
                span_style(t, &sp.href, size, color, heading_strong, false)
            }),
            MdSpanKind::Bold => pieces_width(ui, &sp.text, |t| {
                span_style(t, &sp.href, size, color, true, false)
            }),
            MdSpanKind::Italic => pieces_width(ui, &sp.text, |t| {
                span_style(t, &sp.href, size, color, heading_strong, true)
            }),
            MdSpanKind::BoldItalic => pieces_width(ui, &sp.text, |t| {
                span_style(t, &sp.href, size, color, true, true)
            }),
            MdSpanKind::Code => CODE_PAD_X * 2.0 + pieces_width(ui, &sp.text, |t| code_rt(t, size)),
            MdSpanKind::Mark => pieces_width(ui, &sp.text, |t| {
                RichText::new(t)
                    .size(size)
                    .color(c(0x37, 0x41, 0x51))
                    .background_color(c(0xFE, 0xF0, 0x8A))
            }),
            MdSpanKind::Strike => pieces_width(ui, &sp.text, |t| {
                RichText::new(t).size(size).color(color).strikethrough()
            }),
            MdSpanKind::Link => pieces_width(ui, &sp.text, |t| {
                RichText::new(t)
                    .size(size)
                    .color(c(0x25, 0x63, 0xEB))
                    .underline()
            }),
        };
    }
    w
}

/// 剩余宽度不够放下整段时：能放下至少一个字就先填满当前行（break-word），否则换到下一行。
fn maybe_break_word(ui: &mut Ui, rt: &RichText) {
    let rem = ui.available_size_before_wrap().x;
    let line_w = ui.max_rect().width();
    if !rem.is_finite() || rem <= 1.0 || rem >= line_w - 2.0 {
        return;
    }
    let galley = egui::WidgetText::from(rt.clone()).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::FontSelection::Default,
    );
    if galley.size().x > rem + 1.0 && rem < 18.0 {
        force_wrap_line(ui);
    }
}

fn add_flow_text(
    ui: &mut Ui,
    text: &str,
    size: f32,
    rt_fn: impl Fn(&str, bool) -> RichText,
    sense: Option<Sense>,
    hint: &str,
) -> bool {
    let mut clicked = false;
    let pieces = wrap_pieces(text);
    let mut i = 0;
    while i < pieces.len() {
        let p0 = pieces[i];
        if p0.is_empty() {
            i += 1;
            continue;
        }
        let hit = piece_in_sel(hint, p0);
        maybe_break_word(ui, &rt_fn(p0, hit));
        let rem = ui.available_size_before_wrap().x;
        let mut acc = String::from(p0);
        let mut n = 1;
        let w0 = rich_width(ui, &rt_fn(&acc, hit));
        if !(rem.is_finite() && w0 > rem + 1.0) {
            let mut used = w0;
            while i + n < pieces.len() {
                let p = pieces[i + n];
                if p.is_empty() {
                    n += 1;
                    continue;
                }
                if piece_in_sel(hint, p) != hit {
                    break;
                }
                let ka = script_kind(&acc);
                let kp = script_kind(p);
                if ka != 0 && kp != 0 && ka != kp {
                    break;
                }
                let wp = rich_width(ui, &rt_fn(p, hit));
                if rem.is_finite() && used + wp > rem + 1.0 {
                    break;
                }
                acc.push_str(p);
                used += wp;
                n += 1;
            }
        }
        let rt = rt_fn(&acc, hit);
        let rem = ui.available_size_before_wrap().x;
        let natural = egui::WidgetText::from(rt.clone()).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::FontSelection::Default,
        );
        let wrapping = rem.is_finite() && natural.size().x > rem + 1.0;
        let lab = if wrapping {
            Label::new(rt).wrap()
        } else {
            Label::new(rt).wrap_mode(egui::TextWrapMode::Extend)
        };
        let lab = if let Some(s) = sense {
            lab.sense(s)
        } else {
            lab
        };
        let row_h = size * 1.45;
        let r = if wrapping {
            let r = add_sel_label(ui, lab);
            if sense.is_some() {
                r.on_hover_cursor(egui::CursorIcon::PointingHand)
            } else {
                r
            }
        } else {
            let h = row_h.max(natural.size().y);
            ui.allocate_ui_with_layout(
                vec2(natural.size().x.max(1.0), h),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.add_space((h - natural.size().y).max(0.0));
                    let r = add_sel_label(ui, lab);
                    if sense.is_some() {
                        r.on_hover_cursor(egui::CursorIcon::PointingHand)
                    } else {
                        r
                    }
                },
            )
            .inner
        };
        if r.clicked() && !r.double_clicked() && !r.triple_clicked() {
            clicked = true;
        }
        i += n;
    }
    clicked
}

fn code_rt(text: &str, size: f32) -> RichText {
    RichText::new(text)
        .size(size * 0.9)
        .family(theme::preview_mono_family())
        .color(CODE_FG)
}

fn add_inline_code(ui: &mut Ui, text: &str, size: f32, hint: &str) {
    let row_h = size * 1.45;
    let chip_h = (size * 1.2).min(row_h);
    let pieces = wrap_pieces(text);
    let mut i = 0;
    while i < pieces.len() {
        let p0 = pieces[i];
        if p0.is_empty() {
            i += 1;
            continue;
        }
        let hit = piece_in_sel(hint, p0);
        maybe_break_word(ui, &code_rt(p0, size));
        let rem = ui.available_size_before_wrap().x;
        let mut acc = String::from(p0);
        let mut n = 1;
        let w0 = rich_width(ui, &code_rt(&acc, size)) + CODE_PAD_X * 2.0;
        if !(rem.is_finite() && w0 > rem + 1.0) {
            let mut used = w0;
            while i + n < pieces.len() {
                let p = pieces[i + n];
                if p.is_empty() {
                    n += 1;
                    continue;
                }
                if piece_in_sel(hint, p) != hit {
                    break;
                }
                let ka = script_kind(&acc);
                let kp = script_kind(p);
                if ka != 0 && kp != 0 && ka != kp {
                    break;
                }
                let wp = rich_width(ui, &code_rt(p, size));
                if rem.is_finite() && used + wp > rem + 1.0 {
                    break;
                }
                acc.push_str(p);
                used += wp;
                n += 1;
            }
        }
        let rt = code_rt(&acc, size);
        let rem = ui.available_size_before_wrap().x;
        let natural = egui::WidgetText::from(rt.clone()).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::FontSelection::Default,
        );
        let need = natural.size().x + CODE_PAD_X * 2.0;
        let wrapping = rem.is_finite() && need > rem + 1.0;
        let galley = if wrapping {
            let wrap_w = (rem - CODE_PAD_X * 2.0).max(1.0);
            egui::WidgetText::from(rt).into_galley(
                ui,
                Some(egui::TextWrapMode::Wrap),
                wrap_w,
                egui::FontSelection::Default,
            )
        } else {
            natural
        };
        let w = (galley.size().x + CODE_PAD_X * 2.0).max(1.0);
        let h = row_h.max(galley.size().y);
        let bg = if hit { SEL_BG } else { CODE_CHIP };
        paint_code_chip(ui, galley, w, h, chip_h, bg, size);
        i += n;
    }
}

fn add_sel_label(ui: &mut Ui, lab: Label) -> egui::Response {
    let (pos, galley, response) = lab.layout_in_ui(ui);
    if ui.is_rect_visible(response.rect) {
        let color = ui.style().visuals.text_color();
        crate::view::text_sel::paint_selectable_galley(
            ui,
            &response,
            pos,
            galley,
            color,
            Stroke::NONE,
        );
    }
    span_group_note(ui, &response, true);
    response
}

fn paint_code_chip(
    ui: &mut Ui,
    galley: std::sync::Arc<egui::Galley>,
    w: f32,
    h: f32,
    chip_h: f32,
    bg: Color32,
    size: f32,
) {
    // 灰底自绘；文字走 Label 选区，否则跨行内 code 拖选选不中。
    let (rect, response) = ui.allocate_exact_size(vec2(w, h), select_sense());
    let galley_top = rect.center().y - galley.size().y * 0.5;
    let chip_down = size * CODE_CHIP_DOWN;
    let origin = pos2(
        rect.left() + CODE_PAD_X,
        galley_top + size * CODE_INK_DOWN + chip_down,
    );
    let painter = ui.painter();
    for row in &galley.rows {
        let rr = row.rect();
        let chip_w = (rr.width() + CODE_PAD_X * 2.0).max(1.0);
        let chip_cy = galley_top + rr.center().y + chip_down;
        let chip = Rect::from_center_size(
            pos2(rect.left() + CODE_PAD_X + rr.width() * 0.5, chip_cy),
            vec2(chip_w, chip_h.min(h)),
        );
        painter.rect_filled(chip, CODE_ROUND, bg);
    }
    crate::view::text_sel::paint_selectable_galley(
        ui,
        &response,
        origin,
        galley,
        CODE_FG,
        Stroke::NONE,
    );
    span_group_note(ui, &response, true);
}

fn spans_plain_text(spans: &[MdSpan]) -> String {
    let mut s = String::new();
    for sp in spans {
        match sp.kind {
            MdSpanKind::SoftBr => s.push('\n'),
            MdSpanKind::Image => {}
            _ => s.push_str(&sp.text),
        }
    }
    s
}

#[derive(Clone)]
struct SpanGroupAcc {
    id: egui::Id,
    bg: egui::layers::ShapeIdx,
    rects: Vec<Rect>,
    dbl: bool,
}

#[derive(Clone)]
struct PreviewLinePick {
    id: egui::Id,
    text: String,
}

fn span_group_begin(ui: &Ui, id: egui::Id) {
    let bg = ui.painter().add(Shape::Noop);
    ui.ctx().data_mut(|d| {
        let key = egui::Id::new("preview_span_stack");
        let mut stack = d.get_temp::<Vec<SpanGroupAcc>>(key).unwrap_or_default();
        stack.push(SpanGroupAcc {
            id,
            bg,
            rects: Vec::new(),
            dbl: false,
        });
        d.insert_temp(key, stack);
    });
}

/// `paint`：是否计入双击高亮矩形。正文 true；行尾/块间 gap false（仍可触发 dbl）。
fn span_group_note(ui: &Ui, resp: &egui::Response, paint: bool) {
    let dbl = resp.double_clicked() || resp.triple_clicked();
    let rect = resp.rect;
    ui.ctx().data_mut(|d| {
        let key = egui::Id::new("preview_span_stack");
        if let Some(mut stack) = d.get_temp::<Vec<SpanGroupAcc>>(key) {
            if let Some(acc) = stack.last_mut() {
                if paint {
                    acc.rects.push(rect);
                }
                if dbl {
                    acc.dbl = true;
                }
            }
            d.insert_temp(key, stack);
        }
    });
}

fn span_group_end(ui: &Ui, text: &str) {
    let acc = ui.ctx().data_mut(|d| {
        let key = egui::Id::new("preview_span_stack");
        let mut stack = d.get_temp::<Vec<SpanGroupAcc>>(key).unwrap_or_default();
        let v = stack.pop();
        if stack.is_empty() {
            d.remove::<Vec<SpanGroupAcc>>(key);
        } else {
            d.insert_temp(key, stack);
        }
        v
    });
    let Some(acc) = acc else {
        return;
    };
    let pick_key = egui::Id::new("preview_line_pick");
    let mut pick = ui
        .ctx()
        .data(|d| d.get_temp::<PreviewLinePick>(pick_key));
    if acc.dbl {
        pick = Some(PreviewLinePick {
            id: acc.id,
            text: text.trim().to_string(),
        });
        ui.ctx()
            .plugin::<LabelSelectionState>()
            .lock()
            .clear_selection();
    }
    let active = pick.as_ref().is_some_and(|p| p.id == acc.id);
    if active {
        let mut shapes: Vec<Shape> = Vec::new();
        let clip = ui.clip_rect();
        for r in &acc.rects {
            let r = r.intersect(clip);
            if r.width() > 0.5 && r.height() > 0.5 {
                shapes.push(Shape::rect_filled(r, 0.0, SEL_BG));
            }
        }
        let shape = match shapes.len() {
            0 => Shape::Noop,
            1 => shapes.remove(0),
            _ => Shape::Vec(shapes),
        };
        ui.painter().set(acc.bg, shape);
        if ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy))) {
            if let Some(p) = &pick {
                ui.ctx().copy_text(p.text.clone());
                ui.ctx().input_mut(|i| {
                    i.events.retain(|e| !matches!(e, egui::Event::Copy));
                });
            }
        }
    } else {
        ui.painter().set(acc.bg, Shape::Noop);
    }
    if let Some(p) = pick {
        ui.ctx().data_mut(|d| d.insert_temp(pick_key, p));
    }
}

fn show_spans(
    ui: &mut Ui,
    spans: &[MdSpan],
    img: &mut ImgCache,
    base: Option<&Path>,
    events: &mut Vec<PreviewEvent>,
    img_max: f32,
    size: f32,
    color: Color32,
    strong: bool,
    hint: &str,
) {
    let plain = spans_plain_text(spans);
    let gid = ui.id().with("span_group").with(&plain);
    span_group_begin(ui, gid);
    let already_wrap = ui.layout().is_horizontal() && ui.layout().main_wrap();
    let mut add = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for sp in spans {
            if sp.kind == MdSpanKind::SoftBr {
                force_wrap_line(ui);
            } else {
                one_span(
                    ui, sp, img, base, events, img_max, size, color, strong, hint,
                );
            }
        }
        eat_line_rest(ui, size);
    };
    if already_wrap {
        add(ui);
    } else {
        text_flow(ui, add);
    }
    span_group_end(ui, &plain);
}

fn one_span(
    ui: &mut Ui,
    sp: &MdSpan,
    img: &mut ImgCache,
    base: Option<&Path>,
    events: &mut Vec<PreviewEvent>,
    img_max: f32,
    size: f32,
    color: Color32,
    heading_strong: bool,
    hint: &str,
) {
    match sp.kind {
        MdSpanKind::Text => {
            add_flow_text(
                ui,
                &sp.text,
                size,
                |t, hit| {
                    rt_sel(
                        span_style(t, &sp.href, size, color, heading_strong, false),
                        hit,
                    )
                },
                None,
                hint,
            );
        }
        MdSpanKind::Bold => {
            add_flow_text(
                ui,
                &sp.text,
                size,
                |t, hit| rt_sel(span_style(t, &sp.href, size, color, true, false), hit),
                None,
                hint,
            );
        }
        MdSpanKind::Italic => {
            add_flow_text(
                ui,
                &sp.text,
                size,
                |t, hit| {
                    rt_sel(
                        span_style(t, &sp.href, size, color, heading_strong, true),
                        hit,
                    )
                },
                None,
                hint,
            );
        }
        MdSpanKind::BoldItalic => {
            add_flow_text(
                ui,
                &sp.text,
                size,
                |t, hit| rt_sel(span_style(t, &sp.href, size, color, true, true), hit),
                None,
                hint,
            );
        }
        MdSpanKind::Code => add_inline_code(ui, &sp.text, size, hint),
        MdSpanKind::Mark => {
            add_flow_text(
                ui,
                &sp.text,
                size,
                |t, hit| {
                    rt_sel(
                        RichText::new(t)
                            .size(size)
                            .color(c(0x37, 0x41, 0x51))
                            .background_color(c(0xFE, 0xF0, 0x8A)),
                        hit,
                    )
                },
                None,
                hint,
            );
        }
        MdSpanKind::Strike => {
            add_flow_text(
                ui,
                &sp.text,
                size,
                |t, hit| {
                    rt_sel(
                        RichText::new(t).size(size).color(color).strikethrough(),
                        hit,
                    )
                },
                None,
                hint,
            );
        }
        MdSpanKind::Link => {
            if add_flow_text(
                ui,
                &sp.text,
                size,
                |t, hit| {
                    rt_sel(
                        RichText::new(t)
                            .size(size)
                            .color(c(0x25, 0x63, 0xEB))
                            .underline(),
                        hit,
                    )
                },
                Some(Sense::click()),
                hint,
            ) {
                events.push(PreviewEvent::OpenHref(sp.href.clone()));
            }
        }
        MdSpanKind::Image => {
            let rem = ui.available_size_before_wrap().x;
            let line_w = ui.max_rect().width();
            if rem.is_finite() && rem < line_w - 2.0 && rem < 48.0 {
                force_wrap_line(ui);
            }
            show_image(
                ui, &sp.href, &sp.text, img, base, img_max, None, None, events,
            );
        }
        MdSpanKind::SoftBr => {}
    }
}

fn span_style(
    text: &str,
    href: &str,
    size: f32,
    fallback: Color32,
    bold: bool,
    italic: bool,
) -> RichText {
    let (fg, bg) = href_colors(href);
    let fam = if script_kind(text) == 2 {
        theme::latin_family()
    } else if bold {
        theme::bold_family()
    } else {
        theme::preview_family()
    };
    let mut r = RichText::new(text)
        .size(size)
        .color(fg.unwrap_or(fallback))
        .family(fam)
        .line_height(Some(size * 1.45));
    if italic {
        r = r.italics();
    }
    if let Some(bg) = bg {
        r = r.background_color(bg);
    }
    r
}

fn href_colors(href: &str) -> (Option<Color32>, Option<Color32>) {
    if href.is_empty() || href.starts_with("http") {
        return (None, None);
    }
    let (a, b) = href.split_once(';').unwrap_or((href, ""));
    (parse_hash_color(a), parse_hash_color(b))
}

fn parse_hash_color(s: &str) -> Option<Color32> {
    let s = s.trim().strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(s, 16).ok()?;
    Some(Color32::from_rgb(
        ((n >> 16) & 0xff) as u8,
        ((n >> 8) & 0xff) as u8,
        (n & 0xff) as u8,
    ))
}

fn show_image(
    ui: &mut Ui,
    href: &str,
    cap: &str,
    img: &mut ImgCache,
    base: Option<&Path>,
    max_w: f32,
    force_w: Option<f32>,
    force_h: Option<f32>,
    events: &mut Vec<PreviewEvent>,
) {
    if let Some(raster) = img.get(ui.ctx(), href, base) {
        let mut w = raster.size.x;
        let mut h = raster.size.y;
        if let Some(fw) = force_w {
            if fw > 0.0 {
                h *= fw / w.max(1.0);
                w = fw;
            }
        }
        if let Some(fh) = force_h {
            if fh > 0.0 {
                w *= fh / h.max(1.0);
                h = fh;
            }
        }
        if w > max_w {
            h *= max_w / w;
            w = max_w;
        }
        let title = if cap.is_empty() {
            href.rsplit(['/', '\\'])
                .next()
                .unwrap_or(crate::i18n::t().image)
                .to_string()
        } else {
            cap.to_string()
        };
        let resp = ui
            .add(egui::Image::new((raster.tex.id(), Vec2::new(w, h))).sense(Sense::click()))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(crate::i18n::t().dblclick_preview_copy);
        push_thumb(events, &resp, raster, title);
        if !cap.is_empty() {
            ui.label(RichText::new(cap).size(12.0).color(c(0x4B, 0x55, 0x63)));
        }
    } else {
        ui.label(RichText::new(format!("[img] {href}")).color(c(0x4B, 0x55, 0x63)));
    }
}

fn push_thumb(
    events: &mut Vec<PreviewEvent>,
    resp: &egui::Response,
    raster: Raster,
    title: String,
) {
    match img_preview::interact_thumb(resp) {
        ThumbAction::Preview => events.push(PreviewEvent::OpenImage { raster, title }),
        ThumbAction::CopyImage => events.push(PreviewEvent::CopyImage(raster)),
        ThumbAction::CopyFile => events.push(PreviewEvent::CopyAsFile(raster)),
        ThumbAction::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    #[test]
    fn squeeze_cjk_spaces_around_link() {
        assert_eq!(squeeze_cjk_spaces("请看 文档 了解"), "请看文档了解");
        assert_eq!(squeeze_cjk_spaces("请看  安装说明  即可"), "请看安装说明即可");
        assert_eq!(squeeze_cjk_spaces("E2025242 扬子嘉盛： 4 台电脑"), "E2025242扬子嘉盛：4台电脑");
        assert_eq!(squeeze_cjk_spaces("（ 90% ）"), "（90%）");
        assert_eq!(squeeze_cjk_spaces("中文 English 汉字"), "中文English汉字");
        assert_eq!(squeeze_cjk_spaces("访问 https://x.com 即可"), "访问https://x.com即可");
        assert_eq!(squeeze_cjk_spaces("hello world"), "hello world");
    }

    #[test]
    fn preview_double_click_does_not_panic() {
        let ctx = egui::Context::default();
        crate::view::theme::install_fonts(&ctx);
        let doc = parser::parse(
            "- E2025242扬子嘉盛：4台电脑安装常用软件、系统更新、驱动更新（90%）。\n",
        );
        let mut st = PreviewState::default();
        let mut img = crate::io::imgcache::ImgCache::default();
        let mut mermaid = crate::io::mermaid::MermaidCache::default();
        let opts = PreviewOpts::default();
        let want = "E2025242扬子嘉盛：4台电脑安装常用软件、系统更新、驱动更新（90%）。";
        let mut t = 0.0_f64;
        let mut step = |events: Vec<egui::Event>, pos: egui::Pos2| -> String {
            let mut evs = events;
            evs.insert(0, egui::Event::PointerMoved(pos));
            let mut raw = egui::RawInput::default();
            raw.screen_rect = Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 700.0)));
            raw.events = evs;
            raw.focused = true;
            raw.time = Some(t);
            t += 0.08;
            let mut pick_text = String::new();
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut ev = Vec::new();
                    show(
                        ui, &doc, &mut st, &mut img, &mut mermaid, None, &mut ev, opts, None,
                        None,
                    );
                });
                if let Some(p) = ctx.data(|d| {
                    d.get_temp::<PreviewLinePick>(egui::Id::new("preview_line_pick"))
                }) {
                    pick_text = p.text;
                }
            });
            pick_text
        };
        let btn = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut pick_text = String::new();
        let mut ok = false;
        for y in [40.0, 70.0, 100.0, 130.0, 160.0] {
            for x in [80.0, 160.0, 240.0, 320.0] {
                let click = pos2(x, y);
                let _ = step(vec![], click);
                let _ = step(vec![btn(click, true)], click);
                let _ = step(vec![btn(click, false)], click);
                let _ = step(vec![btn(click, true)], click);
                let _ = step(vec![btn(click, false)], click);
                pick_text = step(vec![], click);
                if pick_text.contains("扬子嘉盛") && pick_text.contains("90%") {
                    ok = true;
                    break;
                }
            }
            if ok {
                break;
            }
        }
        assert!(
            ok,
            "预览双击应选中整条列表项，got={pick_text:?} want contains 扬子嘉盛/90%"
        );
        assert!(
            !pick_text.contains(" 台"),
            "复制文本不应在数字和汉字之间多空格: {pick_text:?}"
        );
        assert!(
            pick_text.contains(want) || pick_text.replace(' ', "") == want.replace(' ', ""),
            "got={pick_text:?}"
        );
    }

    #[test]
    fn table_row_push_id_keeps_cell_widget_ids_unique() {
        let ctx = egui::Context::default();
        crate::view::theme::install_fonts(&ctx);
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 700.0)));
        let mut ids = Vec::new();
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.push_id("table", |ui| {
                    for ri in 0..3 {
                        ui.push_id(ri, |ui| {
                            let r = ui.add(
                                Label::new("密码.md").sense(egui::Sense::click()),
                            );
                            ids.push(r.id);
                        });
                    }
                });
            });
        });
        assert_eq!(ids.len(), 3);
        assert_ne!(ids[0], ids[1], "表格行要用 push_id，否则相同单元格文字会撞 widget id");
        assert_ne!(ids[1], ids[2]);
    }

    #[test]
    fn heading_fold_skips_until_same_or_higher() {
        let doc = parser::parse("# A\n\npara\n\n## A1\n\nx\n\n# B\n\ny\n");
        let i0 = doc
            .blocks
            .iter()
            .position(|b| b.kind == MdBlockKind::Heading && b.level == 1)
            .unwrap();
        assert!(heading_has_body(&doc.blocks, i0));
        let end = skip_heading_section(&doc.blocks, i0);
        assert!(doc.blocks[end].kind == MdBlockKind::Heading);
        assert_eq!(doc.blocks[end].level, 1);
        assert_eq!(doc.blocks[end].text, "B");
    }

    #[test]
    fn empty_heading_not_foldable() {
        let doc = parser::parse("# A\n\n# B\n");
        let heads: Vec<_> = doc
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.kind == MdBlockKind::Heading)
            .collect();
        assert!(!heading_has_body(&doc.blocks, heads[0].0));
    }

    #[test]
    fn blank_between_list_items_takes_space() {
        let doc = parser::parse("- a\n\n- b");
        let blanks: Vec<usize> = doc
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.kind == MdBlockKind::Blank)
            .map(|(i, _)| i)
            .collect();
        assert!(!blanks.is_empty());
        assert!(blank_takes_space(&doc.blocks, blanks[0]));
        let para = parser::parse("p1\n\np2");
        let pb: Vec<usize> = para
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.kind == MdBlockKind::Blank)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(pb.len(), 1);
        assert!(blank_takes_space(&para.blocks, pb[0]));
        let extra = parser::parse("p1\n\n\np2");
        let eb: Vec<usize> = extra
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.kind == MdBlockKind::Blank)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(eb.len(), 2);
        assert!(blank_takes_space(&extra.blocks, eb[0]));
        assert!(blank_takes_space(&extra.blocks, eb[1]));
    }

    #[test]
    fn first_n_lines_cuts() {
        let s = "a\nb\nc\nd";
        assert_eq!(first_n_lines(s, 10), s);
        assert_eq!(first_n_lines(s, 2), "a\nb");
    }

    #[test]
    fn piece_in_sel_only_matching_text() {
        assert!(piece_in_sel("README", "README"));
        assert!(piece_in_sel("[README](../../README.md)", "README"));
        assert!(!piece_in_sel("README", "打开本文件后执行"));
        assert!(!piece_in_sel("README", "对照"));
    }

    #[test]
    fn wrap_pieces_break_word() {
        assert_eq!(wrap_pieces("hello world"), vec!["hello ", "world"]);
        assert_eq!(wrap_pieces(":MdSideView"), vec![":MdSideView"]);
        let cjk = wrap_pieces("对照README");
        assert_eq!(cjk, vec!["对照", "README"]);
        assert_eq!(wrap_pieces("斜体"), vec!["斜体"]);
        assert_eq!(wrap_pieces("更新后bug处理"), vec!["更新后", "bug", "处理"]);
    }
}

