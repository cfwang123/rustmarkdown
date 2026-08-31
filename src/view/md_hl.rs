//! Markdown 源码着色（对齐 docview 纯代码/vim 风：同字号、标题分层色、标记灰色）。
//! LayoutJob 文本必须与源码逐字节一致，否则 TextEdit 光标会错位。

use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex};

use egui::epaint::text::RowVisuals;
use egui::{pos2, Align, Color32, FontId, Rect, Shape, Stroke, TextFormat, TextStyle};

use crate::view::highlight::LineHl;
use crate::view::theme;

struct Faces {
    regular: FontId,
    /// 正文 `**粗体**`（等宽粗体，避免改字宽导致光标错位）。
    bold: FontId,
    /// 标题行（与正文同一等宽字族，接近 GVIM）。
    heading: FontId,
}

const BODY: Color32 = Color32::from_rgb(0x11, 0x18, 0x27);
const MARKER: Color32 = Color32::from_rgb(0x9C, 0xA3, 0xAF);
const CODE: Color32 = Color32::from_rgb(0xB9, 0x1C, 0x1C);
const MARK: Color32 = Color32::from_rgb(0xB4, 0x53, 0x09);
const STRIKE: Color32 = Color32::from_rgb(0x6B, 0x72, 0x80);
const LINK: Color32 = Color32::from_rgb(0x25, 0x63, 0xEB);
const IMAGE: Color32 = Color32::from_rgb(0x05, 0x96, 0x69);
const FENCE: Color32 = Color32::from_rgb(0x6B, 0x72, 0x80);
const FENCE_LANG: Color32 = Color32::from_rgb(0x7C, 0x3A, 0xED);
const FENCE_BODY: Color32 = Color32::from_rgb(0x4B, 0x55, 0x63);
pub(crate) const CODE_BG: Color32 = Color32::from_rgb(0xF3, 0xF4, 0xF6);
const QUOTE: Color32 = Color32::from_rgb(0x4B, 0x55, 0x63);
/// 源码任务框 `[ ]`：灰字淡灰底。
const TASK_OPEN_FG: Color32 = Color32::from_rgb(0x6B, 0x72, 0x80);
const TASK_OPEN_BG: Color32 = Color32::from_rgb(0xE5, 0xE7, 0xEB);
/// 源码任务框 `[x]` / `[X]`：绿字淡绿底。
const TASK_DONE_FG: Color32 = Color32::from_rgb(0x04, 0x78, 0x57);
const TASK_DONE_BG: Color32 = Color32::from_rgb(0xD1, 0xFA, 0xE5);
const MAX_CHARS: usize = 250_000;

struct HlLine {
    hash: u64,
    in_fence: bool,
    sec_end: usize,
    byte_end: usize,
}

struct Cache {
    hash: u64,
    font_sz: u32,
    job: egui::text::LayoutJob,
    lines: Vec<HlLine>,
}

struct GalleyEntry {
    hash: u64,
    font_sz: u32,
    wrap: u32,
    galley: Arc<egui::Galley>,
}

const GALLEY_SLOTS: usize = 8;
const WRAP_QUANT: u32 = 8;
/// 折行宽差在此内复用 Galley（滚动条显隐约 12–16px）。
const WRAP_SLACK: u32 = 64;
/// 光标附近保留实网格的行数（上下各这么多），其余选中行掏空网格以免拖选复制整篇。
const SEL_KEEP_ROWS: usize = 64;

struct FenceCache {
    hash: u64,
    spans: Vec<(usize, usize)>,
}

struct ParaMem {
    wrap: u32,
    font_sz: u32,
    keys: Vec<u64>,
    galleys: Vec<Arc<egui::Galley>>,
}

static CACHE: LazyLock<Mutex<Option<Cache>>> = LazyLock::new(|| Mutex::new(None));
static GALLEY: LazyLock<Mutex<Vec<GalleyEntry>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static PARA: LazyLock<Mutex<Option<ParaMem>>> = LazyLock::new(|| Mutex::new(None));
static FENCE_CACHE: LazyLock<Mutex<Option<FenceCache>>> = LazyLock::new(|| Mutex::new(None));

fn heading_fg(lv: usize) -> Color32 {
    match lv {
        1 => Color32::from_rgb(0x1D, 0x4E, 0xD8),
        2 => Color32::from_rgb(0x6D, 0x28, 0xD9),
        3 => Color32::from_rgb(0x0F, 0x76, 0x6E),
        4 => Color32::from_rgb(0xC2, 0x41, 0x0C),
        5 => Color32::from_rgb(0xBE, 0x18, 0x5D),
        _ => Color32::from_rgb(0x47, 0x55, 0x69),
    }
}

fn heading_mark(lv: usize) -> Color32 {
    match lv {
        1 => Color32::from_rgb(0x60, 0xA5, 0xFA),
        2 => Color32::from_rgb(0xA7, 0x8B, 0xFA),
        3 => Color32::from_rgb(0x5E, 0xEA, 0xD4),
        4 => Color32::from_rgb(0xFD, 0xBA, 0x74),
        5 => Color32::from_rgb(0xF9, 0xA8, 0xD4),
        _ => Color32::from_rgb(0x94, 0xA3, 0xB8),
    }
}

pub fn layout_job(ui: &egui::Ui, text: &str) -> egui::text::LayoutJob {
    let font = TextStyle::Monospace.resolve(ui.style());
    let font_sz = font.size.to_bits();
    let hash = hash_text(text);
    {
        let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = g.as_ref() {
            if c.hash == hash && c.font_sz == font_sz {
                return c.job.clone();
            }
        }
        let faces = Faces {
            regular: font.clone(),
            bold: FontId::new(font.size, theme::mono_bold_family()),
            heading: FontId::new(font.size, theme::mono_bold_family()),
        };
        incr_highlight(text, &faces, font_sz, hash, &mut g)
    }
}

/// 查找命中 / 预览映射到底的叠加层。画在已排版文字上，不参与 Galley 缓存键。
pub struct LayoutOverlay<'a> {
    pub hint: Option<(usize, usize)>,
    pub find_all: &'a [(usize, usize)],
    pub find_cur: Option<(usize, usize)>,
}

impl LayoutOverlay<'_> {
    #[cfg(test)]
    fn extra_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.hint.hash(&mut h);
        self.find_all.hash(&mut h);
        self.find_cur.hash(&mut h);
        h.finish()
    }

    fn is_empty(&self) -> bool {
        self.hint.is_none() && self.find_all.is_empty() && self.find_cur.is_none()
    }
}

fn quantize_wrap(wrap_width: f32) -> u32 {
    let q = (wrap_width / WRAP_QUANT as f32).round() as u32;
    q.max(1).saturating_mul(WRAP_QUANT).min(100_000)
}

fn wrap_close(a: u32, b: u32, sticky: bool) -> bool {
    sticky || a == b || a.abs_diff(b) <= WRAP_SLACK
}

fn galley_lookup(
    hash: u64,
    font_sz: u32,
    wrap: u32,
    sticky_wrap: bool,
) -> Option<Arc<egui::Galley>> {
    let mut g = GALLEY.lock().unwrap_or_else(|e| e.into_inner());
    let pos = g
        .iter()
        .position(|e| e.hash == hash && e.font_sz == font_sz && wrap_close(e.wrap, wrap, sticky_wrap))?;
    let e = g.remove(pos);
    let galley = e.galley.clone();
    g.insert(0, e);
    Some(galley)
}

fn galley_store(entry: GalleyEntry) {
    let mut g = GALLEY.lock().unwrap_or_else(|e| e.into_inner());
    g.retain(|e| !(e.hash == entry.hash && e.font_sz == entry.font_sz && e.wrap == entry.wrap));
    g.insert(0, entry);
    g.truncate(GALLEY_SLOTS);
}

/// 按当前字号与折行宽取 Galley；文本未改时复用。
/// 查找/映射底色改为叠画，不进缓存键。`sticky_wrap`：拖选时忽略折行宽，避免整篇重排。
pub fn layout_galley(
    ui: &egui::Ui,
    text: &str,
    wrap_width: f32,
    sticky_wrap: bool,
) -> Arc<egui::Galley> {
    let font = TextStyle::Monospace.resolve(ui.style());
    let font_sz = font.size.to_bits();
    let wrap = quantize_wrap(wrap_width);
    let hash = hash_text(text);
    let galley = if let Some(g) = galley_lookup(hash, font_sz, wrap, sticky_wrap) {
        g
    } else {
        let mut job = layout_job(ui, text);
        job.wrap.max_width = wrap as f32;
        job.wrap.break_anywhere = true;
        let galley = layout_by_paragraphs(ui, job, wrap, font_sz);
        galley_store(GalleyEntry {
            hash,
            font_sz,
            wrap,
            galley: galley.clone(),
        });
        galley
    };
    hollow_offscreen_sel(ui, galley)
}

fn hash_para_span(
    text: &str,
    sections: &[egui::text::LayoutSection],
    start: usize,
    end: usize,
    wrap: u32,
    font_sz: u32,
) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    wrap.hash(&mut h);
    font_sz.hash(&mut h);
    for s in sections {
        if s.byte_range.end <= start {
            continue;
        }
        if s.byte_range.start >= end {
            break;
        }
        let ns = s.byte_range.start.saturating_sub(start);
        let ne = s.byte_range.end.min(end).saturating_sub(start);
        if ns >= ne {
            continue;
        }
        ns.hash(&mut h);
        ne.hash(&mut h);
        s.format.color.hash(&mut h);
        s.format.background.hash(&mut h);
        s.format.italics.hash(&mut h);
        s.format.font_id.size.to_bits().hash(&mut h);
        if start <= s.byte_range.start {
            s.leading_space.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

fn paragraph_keys(
    job: &egui::text::LayoutJob,
    wrap: u32,
    font_sz: u32,
) -> (Vec<(usize, usize)>, Vec<u64>) {
    let ranges = crate::view::incr::paragraph_ranges(&job.text);
    let mut keys = Vec::with_capacity(ranges.len());
    let mut si = 0usize;
    for &(start, end) in &ranges {
        while si < job.sections.len() && job.sections[si].byte_range.end <= start {
            si += 1;
        }
        keys.push(hash_para_span(
            &job.text[start..end],
            &job.sections[si..],
            start,
            end,
            wrap,
            font_sz,
        ));
    }
    (ranges, keys)
}

fn paragraph_job(
    job: &egui::text::LayoutJob,
    start: usize,
    end: usize,
) -> egui::text::LayoutJob {
    let mut para = egui::text::LayoutJob {
        text: job.text[start..end].to_owned(),
        wrap: job.wrap.clone(),
        sections: Vec::new(),
        break_on_newline: job.break_on_newline,
        halign: job.halign,
        justify: job.justify,
        first_row_min_height: if start == 0 {
            job.first_row_min_height
        } else {
            0.0
        },
        round_output_to_gui: job.round_output_to_gui,
    };
    for sec in &job.sections {
        if sec.byte_range.end <= start {
            continue;
        }
        if sec.byte_range.start >= end {
            break;
        }
        let ns = sec.byte_range.start.saturating_sub(start);
        let ne = sec.byte_range.end.min(end).saturating_sub(start);
        if ns < ne {
            para.sections.push(egui::text::LayoutSection {
                leading_space: if start <= sec.byte_range.start {
                    sec.leading_space
                } else {
                    0.0
                },
                byte_range: ns..ne,
                format: sec.format.clone(),
            });
        }
    }
    para
}

fn layout_by_paragraphs(
    ui: &egui::Ui,
    job: egui::text::LayoutJob,
    wrap: u32,
    font_sz: u32,
) -> Arc<egui::Galley> {
    if !job.break_on_newline || !job.text.contains('\n') {
        return ui.fonts_mut(|f| f.layout_job(job));
    }
    let (ranges, keys) = paragraph_keys(&job, wrap, font_sz);
    if ranges.len() <= 1 {
        return ui.fonts_mut(|f| f.layout_job(job));
    }
    let mut mem = PARA.lock().unwrap_or_else(|e| e.into_inner());
    let (lo, hi_old, hi_new) = match mem.as_ref() {
        Some(m) if m.font_sz == font_sz && wrap_close(m.wrap, wrap, false) => {
            crate::view::incr::diff_fps(&m.keys, &keys)
        }
        _ => (0, 0, keys.len()),
    };
    let mut galleys: Vec<Option<Arc<egui::Galley>>> = vec![None; ranges.len()];
    if let Some(m) = mem.as_ref() {
        if m.font_sz == font_sz && wrap_close(m.wrap, wrap, false) {
            for i in 0..lo {
                if let Some(g) = m.galleys.get(i) {
                    galleys[i] = Some(g.clone());
                }
            }
            let n_suf = keys.len() - hi_new;
            for k in 0..n_suf {
                if let Some(g) = m.galleys.get(hi_old + k) {
                    galleys[hi_new + k] = Some(g.clone());
                }
            }
        }
    }
    for (i, &(start, end)) in ranges.iter().enumerate() {
        if galleys[i].is_none() {
            let para = paragraph_job(&job, start, end);
            galleys[i] = Some(ui.fonts_mut(|f| f.layout_job(para)));
        }
    }
    let galleys: Vec<Arc<egui::Galley>> = galleys.into_iter().map(|g| g.unwrap()).collect();
    *mem = Some(ParaMem {
        wrap,
        font_sz,
        keys,
        galleys: galleys.clone(),
    });
    let ppp = ui.ctx().pixels_per_point();
    Arc::new(egui::Galley::concat(Arc::new(job), &galleys, ppp))
}

fn hash_text(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// 与 `TextEdit::id_salt` 一致，供取出拖选范围。
pub const EDITOR_ID_SALT: &str = "editor";

fn dummy_row(ui: &egui::Ui) -> Option<Arc<egui::epaint::text::Row>> {
    static DUMMY: LazyLock<Mutex<Option<Arc<egui::epaint::text::Row>>>> =
        LazyLock::new(|| Mutex::new(None));
    {
        let g = DUMMY.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_some() {
            return g.clone();
        }
    }
    let font = TextStyle::Monospace.resolve(ui.style());
    let job = egui::text::LayoutJob::simple(" ".into(), font, Color32::WHITE, 64.0);
    let laid = ui.fonts_mut(|f| f.layout_job(job));
    let row = laid.rows.first()?.row.clone();
    let mut g = DUMMY.lock().unwrap_or_else(|e| e.into_inner());
    *g = Some(row.clone());
    Some(row)
}

/// egui 拖选会 `Arc::make_mut` 每一行网格并把字形改成选区色。
/// 按段缓存的行是共享的，若不先拆开，上一行/未选中的相同段会一起变色。
/// 长选区只给光标附近行保留实网格，其余换成无网格副本（字数/行高仍对）。
fn hollow_offscreen_sel(ui: &egui::Ui, galley: Arc<egui::Galley>) -> Arc<egui::Galley> {
    let id = ui.make_persistent_id(EDITOR_ID_SALT);
    let Some(state) = egui::TextEdit::load_state(ui.ctx(), id) else {
        return galley;
    };
    let Some(range) = state.cursor.char_range() else {
        return galley;
    };
    if range.is_empty() {
        return galley;
    }
    let [min, max] = range.sorted_cursors();
    let min_row = galley.layout_from_cursor(min).row;
    let max_row = galley.layout_from_cursor(max).row;
    let lo = min_row.min(max_row);
    let hi = min_row.max(max_row);
    let keep = galley.layout_from_cursor(range.primary).row;
    let keep_lo = keep.saturating_sub(SEL_KEEP_ROWS);
    let keep_hi = keep.saturating_add(SEL_KEEP_ROWS);
    let long = hi.saturating_sub(lo) > SEL_KEEP_ROWS * 2;
    let dummy = if long { dummy_row(ui) } else { None };
    let mut g = (*galley).clone();
    let last = g.rows.len().saturating_sub(1);
    for ri in lo..=hi.min(last) {
        let hollow = long && dummy.is_some() && !(ri >= keep_lo && ri <= keep_hi);
        if hollow {
            let src = &g.rows[ri].row;
            let mut row = (**dummy.as_ref().unwrap()).clone();
            row.glyphs.clone_from(&src.glyphs);
            row.size = src.size;
            row.ends_with_newline = src.ends_with_newline;
            row.visuals = RowVisuals::default();
            g.rows[ri].row = Arc::new(row);
        } else {
            Arc::make_mut(&mut g.rows[ri].row);
        }
    }
    Arc::new(g)
}

/// 查找命中与预览映射底色：叠在已排版行上，避免为此重排整篇。
pub fn overlay_bgs(
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    clip: Rect,
    text: &str,
    overlay: &LayoutOverlay<'_>,
) -> Shape {
    if overlay.is_empty() {
        return Shape::Noop;
    }
    let mut ranges: Vec<(usize, usize, Color32)> = Vec::new();
    if let Some((a, b)) = overlay.hint {
        let lo = a.min(b);
        let hi = a.max(b);
        if lo < hi {
            ranges.push((
                lo,
                hi,
                Color32::from_rgba_unmultiplied(0x93, 0xC5, 0xFD, 120),
            ));
        }
    }
    for &(c0, c1) in &byte_ranges_to_chars(text, overlay.find_all) {
        ranges.push((
            c0,
            c1,
            Color32::from_rgba_unmultiplied(0xFE, 0xF0, 0x8A, 140),
        ));
    }
    if let Some((b0, b1)) = overlay.find_cur {
        if let Some((c0, c1)) = byte_ranges_to_chars(text, &[(b0, b1)]).into_iter().next() {
            ranges.push((
                c0,
                c1,
                Color32::from_rgba_unmultiplied(0xFD, 0xBA, 0x74, 200),
            ));
        }
    }
    paint_char_bgs(galley, galley_pos, clip, &ranges)
}

fn byte_ranges_to_chars(text: &str, ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    if ranges.is_empty() {
        return Vec::new();
    }
    let mut marks: Vec<(usize, usize, bool)> = Vec::with_capacity(ranges.len() * 2);
    for (i, &(a, b)) in ranges.iter().enumerate() {
        if a >= b {
            continue;
        }
        marks.push((a.min(text.len()), i, false));
        marks.push((b.min(text.len()), i, true));
    }
    marks.sort_unstable_by_key(|m| m.0);
    let mut char_at = vec![0usize; ranges.len() * 2];
    let mut mi = 0usize;
    let mut char_i = 0usize;
    let mut byte_i = 0usize;
    for c in text.chars() {
        while mi < marks.len() && marks[mi].0 == byte_i {
            char_at[mi] = char_i;
            mi += 1;
        }
        if mi >= marks.len() {
            break;
        }
        byte_i += c.len_utf8();
        char_i += 1;
    }
    while mi < marks.len() {
        char_at[mi] = char_i;
        mi += 1;
    }
    let mut starts = vec![0usize; ranges.len()];
    let mut ends = vec![0usize; ranges.len()];
    for (k, &(_, i, is_end)) in marks.iter().enumerate() {
        if is_end {
            ends[i] = char_at[k];
        } else {
            starts[i] = char_at[k];
        }
    }
    ranges
        .iter()
        .enumerate()
        .filter_map(|(i, &(a, b))| {
            if a >= b {
                None
            } else {
                Some((starts[i], ends[i]))
            }
        })
        .filter(|(a, b)| a < b)
        .collect()
}

fn paint_char_bgs(
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    clip: Rect,
    ranges: &[(usize, usize, Color32)],
) -> Shape {
    if ranges.is_empty() {
        return Shape::Noop;
    }
    let mut shapes: Vec<Shape> = Vec::new();
    let mut char_i = 0usize;
    for row in &galley.rows {
        let n = row.char_count_including_newline();
        let c0 = char_i;
        let c1 = char_i + n;
        char_i = c1;
        let r = row.rect().translate(galley_pos.to_vec2());
        if r.bottom() < clip.top() - 2.0 || r.top() > clip.bottom() + 2.0 {
            continue;
        }
        let n_ex = row.char_count_excluding_newline();
        for &(a, b, col) in ranges {
            if a >= c1 || b <= c0 {
                continue;
            }
            let col0 = a.saturating_sub(c0).min(n_ex);
            let col1 = if b >= c1 {
                n_ex
            } else {
                b.saturating_sub(c0).min(n_ex)
            };
            let x0 = r.left() + row.x_offset(col0);
            let x1 = if b >= c1 {
                r.right().max(x0 + 2.0)
            } else {
                r.left() + row.x_offset(col1)
            };
            if x1 - x0 < 0.5 {
                continue;
            }
            let rect = Rect::from_min_max(pos2(x0, r.top()), pos2(x1, r.bottom())).intersect(clip);
            if rect.width() > 0.5 && rect.height() > 0.5 {
                shapes.push(Shape::rect_filled(rect, 0.0, col));
            }
        }
    }
    match shapes.len() {
        0 => Shape::Noop,
        1 => shapes.remove(0),
        _ => Shape::Vec(shapes),
    }
}

fn line_body(chunk: &str) -> &str {
    let mut s = chunk;
    if s.ends_with('\n') {
        s = &s[..s.len() - 1];
    }
    if s.ends_with('\r') {
        s = &s[..s.len() - 1];
    }
    s
}

fn color_chunk(
    job: &mut egui::text::LayoutJob,
    faces: &Faces,
    chunk: &str,
    in_fence: &mut bool,
    fence_ch: &mut char,
    hl: &mut Option<LineHl>,
) {
    let has_nl = chunk.ends_with('\n');
    let raw = if has_nl {
        &chunk[..chunk.len() - 1]
    } else {
        chunk
    };
    let cr = raw.ends_with('\r');
    let line = if cr { &raw[..raw.len() - 1] } else { raw };
    color_line(job, faces, line, in_fence, fence_ch, hl);
    if cr {
        append(
            job,
            &faces.regular,
            "\r",
            BODY,
            Color32::TRANSPARENT,
            false,
            false,
        );
    }
    if has_nl {
        append(
            job,
            &faces.regular,
            "\n",
            BODY,
            Color32::TRANSPARENT,
            false,
            false,
        );
    }
}

fn highlight_full(text: &str, faces: &Faces) -> (egui::text::LayoutJob, Vec<HlLine>) {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let mut lines = Vec::new();
    if text.is_empty() {
        return (job, lines);
    }
    let mut in_fence = false;
    let mut fence_ch = '\0';
    let mut hl: Option<LineHl> = None;
    let mut byte_end = 0usize;
    for chunk in text.split_inclusive('\n') {
        color_chunk(&mut job, faces, chunk, &mut in_fence, &mut fence_ch, &mut hl);
        byte_end += chunk.len();
        lines.push(HlLine {
            hash: hash_text(chunk),
            in_fence,
            sec_end: job.sections.len(),
            byte_end,
        });
    }
    debug_assert_eq!(job.text, text);
    (job, lines)
}

fn incr_highlight(
    text: &str,
    faces: &Faces,
    font_sz: u32,
    hash: u64,
    mem: &mut Option<Cache>,
) -> egui::text::LayoutJob {
    if text.is_empty() || text.len() > MAX_CHARS {
        let job = build(text, faces);
        *mem = None;
        return job;
    }
    let do_full = match mem.as_ref() {
        None => true,
        Some(c) => c.font_sz != font_sz || c.lines.is_empty(),
    };
    if do_full {
        return store_full(text, faces, font_sz, hash, mem);
    }
    let chunks: Vec<&str> = text.split_inclusive('\n').collect();
    let hashes: Vec<u64> = chunks.iter().map(|c| hash_text(c)).collect();
    let prev = mem.as_ref().unwrap();
    let old_h: Vec<u64> = prev.lines.iter().map(|l| l.hash).collect();
    let (mut lo, mut hi_old, mut hi_new) = crate::view::incr::diff_fps(&old_h, &hashes);
    while lo > 0 && prev.lines[lo - 1].in_fence {
        lo -= 1;
    }
    let fence = (lo..hi_old).any(|i| {
        prev.lines
            .get(i)
            .map(|l| l.in_fence || (i > 0 && prev.lines[i - 1].in_fence))
            .unwrap_or(false)
    }) || chunks
        .get(lo)
        .is_some_and(|c| try_fence(line_body(c)).is_some())
        || (lo > 0 && lo <= prev.lines.len() && prev.lines[lo - 1].in_fence);
    if fence {
        while lo > 0 && prev.lines[lo - 1].in_fence {
            lo -= 1;
        }
        let mut ho = (lo + 1).min(prev.lines.len());
        while ho < prev.lines.len() && prev.lines[ho - 1].in_fence {
            ho += 1;
        }
        hi_old = ho;
        hi_new = hi_new.max(lo);
    }
    if lo == 0 && hi_new == hashes.len() && hi_old == old_h.len() {
        return store_full(text, faces, font_sz, hash, mem);
    }

    let mut mid = egui::text::LayoutJob::default();
    mid.wrap.max_width = f32::INFINITY;
    let mut in_fence = false;
    let mut fence_ch = '\0';
    let mut hl: Option<LineHl> = None;
    let mut mid_lines: Vec<HlLine> = Vec::new();
    let mut i = lo;
    while i < chunks.len() {
        color_chunk(
            &mut mid,
            faces,
            chunks[i],
            &mut in_fence,
            &mut fence_ch,
            &mut hl,
        );
        mid_lines.push(HlLine {
            hash: hashes[i],
            in_fence,
            sec_end: mid.sections.len(),
            byte_end: mid.text.len(),
        });
        i += 1;
        if i >= hi_new && !in_fence {
            break;
        }
    }
    hi_new = i;
    let n_suf = hashes.len() - hi_new;
    if n_suf != old_h.len().saturating_sub(hi_old) {
        return store_full(text, faces, font_sz, hash, mem);
    }

    let prev = mem.as_mut().unwrap();
    let sec_lo = if lo == 0 {
        0
    } else {
        prev.lines[lo - 1].sec_end
    };
    let sec_hi = if hi_old == 0 {
        0
    } else {
        prev.lines[hi_old - 1].sec_end
    };
    let byte_lo = if lo == 0 {
        0
    } else {
        prev.lines[lo - 1].byte_end
    };
    let byte_hi_old = if hi_old == 0 {
        0
    } else {
        prev.lines[hi_old - 1].byte_end
    };

    let mut secs = std::mem::take(&mut prev.job.sections);
    let mut suffix: Vec<egui::text::LayoutSection> = if sec_hi <= secs.len() {
        secs.split_off(sec_hi)
    } else {
        Vec::new()
    };
    secs.truncate(sec_lo);

    let delta = (byte_lo + mid.text.len()) as isize - byte_hi_old as isize;
    for s in &mut suffix {
        s.byte_range.start = (s.byte_range.start as isize + delta) as usize;
        s.byte_range.end = (s.byte_range.end as isize + delta) as usize;
    }
    for s in &mut mid.sections {
        s.byte_range.start += byte_lo;
        s.byte_range.end += byte_lo;
    }
    secs.append(&mut mid.sections);
    secs.append(&mut suffix);

    let suf_old = prev.lines.split_off(hi_old.min(prev.lines.len()));
    prev.lines.truncate(lo);
    let sec_mid_base = prev.lines.last().map(|l| l.sec_end).unwrap_or(0);
    for mut ln in mid_lines {
        ln.sec_end += sec_mid_base;
        ln.byte_end += byte_lo;
        prev.lines.push(ln);
    }
    let sec_after_mid = prev.lines.last().map(|l| l.sec_end).unwrap_or(0);
    for (k, mut ln) in suf_old.into_iter().enumerate() {
        ln.hash = hashes[hi_new + k];
        ln.sec_end = sec_after_mid + ln.sec_end.saturating_sub(sec_hi);
        ln.byte_end = (ln.byte_end as isize + delta) as usize;
        prev.lines.push(ln);
    }

    prev.hash = hash;
    prev.font_sz = font_sz;
    prev.job.text = text.to_owned();
    prev.job.sections = secs;
    prev.job.wrap.max_width = f32::INFINITY;
    debug_assert_eq!(prev.job.text, text);
    prev.job.clone()
}

fn store_full(
    text: &str,
    faces: &Faces,
    font_sz: u32,
    hash: u64,
    mem: &mut Option<Cache>,
) -> egui::text::LayoutJob {
    let (job, lines) = highlight_full(text, faces);
    *mem = Some(Cache {
        hash,
        font_sz,
        job: job.clone(),
        lines,
    });
    job
}

fn build(text: &str, faces: &Faces) -> egui::text::LayoutJob {
    if text.len() > MAX_CHARS {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = f32::INFINITY;
        append(
            &mut job,
            &faces.regular,
            text,
            BODY,
            Color32::TRANSPARENT,
            false,
            false,
        );
        return job;
    }
    highlight_full(text, faces).0
}

/// 围栏代码（含开/闭标记行）的字符区间 `[start, end)`，供整行铺灰底。
pub fn fence_char_spans(text: &str) -> Vec<(usize, usize)> {
    let hash = hash_text(text);
    {
        let g = FENCE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = g.as_ref() {
            if c.hash == hash {
                return c.spans.clone();
            }
        }
    }
    let spans = fence_char_spans_uncached(text);
    {
        let mut g = FENCE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        *g = Some(FenceCache {
            hash,
            spans: spans.clone(),
        });
    }
    spans
}

fn fence_char_spans_uncached(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut in_fence = false;
    let mut fence_ch = '\0';
    let mut block_start = 0usize;
    let mut char_i = 0usize;
    for chunk in text.split_inclusive('\n') {
        let line_start = char_i;
        let n_chars = chunk.chars().count();
        let has_nl = chunk.ends_with('\n');
        let raw = if has_nl {
            &chunk[..chunk.len() - 1]
        } else {
            chunk
        };
        let line = if raw.ends_with('\r') {
            &raw[..raw.len() - 1]
        } else {
            raw
        };
        if let Some((ch, _, close_only)) = try_fence(line) {
            if !in_fence {
                in_fence = true;
                fence_ch = ch;
                block_start = line_start;
            } else if ch == fence_ch && close_only {
                spans.push((block_start, line_start + n_chars));
                in_fence = false;
                fence_ch = '\0';
            }
        }
        char_i += n_chars;
    }
    if in_fence {
        spans.push((block_start, char_i));
    }
    spans
}

fn char_to_byte(text: &str, n: usize) -> usize {
    text.char_indices()
        .nth(n)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

/// 给源码区间加淡蓝底（预览选区映射到编辑器）。
#[cfg(test)]
fn apply_sel_bg(job: &mut egui::text::LayoutJob, text: &str, char0: usize, char1: usize) {
    let b0 = char_to_byte(text, char0.min(char1));
    let b1 = char_to_byte(text, char0.max(char1));
    if b0 >= b1 {
        return;
    }
    let bg = Color32::from_rgba_unmultiplied(0x93, 0xC5, 0xFD, 120);
    let old = std::mem::take(&mut job.sections);
    let mut out = Vec::with_capacity(old.len() + 4);
    for sec in old {
        let s = sec.byte_range.start;
        let e = sec.byte_range.end;
        if e <= b0 || s >= b1 {
            out.push(sec);
            continue;
        }
        if s < b0 {
            out.push(egui::text::LayoutSection {
                leading_space: sec.leading_space,
                byte_range: s..b0,
                format: sec.format.clone(),
            });
        }
        let hs = s.max(b0);
        let he = e.min(b1);
        if hs < he {
            let mut f = sec.format.clone();
            if f.background.a() == 0 {
                f.background = bg;
            }
            out.push(egui::text::LayoutSection {
                leading_space: if hs == s { sec.leading_space } else { 0.0 },
                byte_range: hs..he,
                format: f,
            });
        }
        if b1 < e {
            out.push(egui::text::LayoutSection {
                leading_space: 0.0,
                byte_range: b1..e,
                format: sec.format,
            });
        }
    }
    job.sections = out;
}

fn color_line(
    job: &mut egui::text::LayoutJob,
    faces: &Faces,
    line: &str,
    in_fence: &mut bool,
    fence_ch: &mut char,
    hl: &mut Option<LineHl>,
) {
    let font = &faces.regular;
    if let Some((ch, lang, close_only)) = try_fence(line) {
        if !*in_fence {
            fill_fence_marker(job, font, line, false);
            *in_fence = true;
            *fence_ch = ch;
            *hl = LineHl::try_new(lang);
            return;
        }
        if ch == *fence_ch && close_only {
            fill_fence_marker(job, font, line, true);
            *in_fence = false;
            *fence_ch = '\0';
            *hl = None;
            return;
        }
    }
    if *in_fence {
        if let Some(h) = hl.as_mut() {
            h.append_line(job, line, font, Color32::TRANSPARENT);
        } else {
            append(
                job,
                font,
                line,
                FENCE_BODY,
                Color32::TRANSPARENT,
                false,
                false,
            );
        }
        return;
    }
    if is_hr(line) {
        append(job, font, line, MARKER, Color32::TRANSPARENT, false, false);
        return;
    }
    let ws = leading_ws(line);
    let rest = &line[ws..];
    if rest.starts_with('>') {
        if ws > 0 {
            append(
                job,
                font,
                &line[..ws],
                BODY,
                Color32::TRANSPARENT,
                false,
                false,
            );
        }
        append(job, font, ">", MARKER, Color32::TRANSPARENT, false, false);
        let after = &rest[1..];
        if after.starts_with(' ') {
            append(job, font, " ", MARKER, Color32::TRANSPARENT, false, false);
            color_inlines(job, faces, &after[1..], QUOTE, false);
        } else {
            color_inlines(job, faces, after, QUOTE, false);
        }
        return;
    }
    if let Some((lv, mark_end)) = heading_mark_end(rest) {
        if ws > 0 {
            append(
                job,
                font,
                &line[..ws],
                BODY,
                Color32::TRANSPARENT,
                false,
                false,
            );
        }
        append(
            job,
            &faces.heading,
            &rest[..mark_end],
            heading_mark(lv),
            Color32::TRANSPARENT,
            false,
            false,
        );
        color_inlines(job, faces, &rest[mark_end..], heading_fg(lv), true);
        return;
    }
    if let Some((pref, body)) = split_list(line) {
        append(job, font, pref, MARKER, Color32::TRANSPARENT, false, false);
        color_inlines(job, faces, body, BODY, false);
        return;
    }
    color_inlines(job, faces, line, BODY, false);
}

fn heading_mark_end(rest: &str) -> Option<(usize, usize)> {
    let n = rest.bytes().take_while(|&b| b == b'#').count();
    if n == 0 || n > 6 {
        return None;
    }
    if rest.len() > n && rest.as_bytes()[n] == b' ' {
        Some((n, n + 1))
    } else {
        None
    }
}

fn split_list(line: &str) -> Option<(&str, &str)> {
    let ws = leading_ws(line);
    let rest = &line[ws..];
    let b = rest.as_bytes();
    if b.is_empty() {
        return None;
    }
    // - * + 后跟空白
    if matches!(b[0], b'-' | b'*' | b'+') {
        if rest.len() > 1 && (b[1] == b' ' || b[1] == b'\t') {
            let mut i = 1;
            while i < rest.len() && (rest.as_bytes()[i] == b' ' || rest.as_bytes()[i] == b'\t') {
                i += 1;
            }
            return Some((&line[..ws + i], &rest[i..]));
        }
        return None;
    }
    // 有序 1. / 1)
    let mut i = 0;
    while i < rest.len() && rest.as_bytes()[i].is_ascii_digit() {
        i += 1;
        if i > 9 {
            break;
        }
    }
    if i > 0 && i < rest.len() && (rest.as_bytes()[i] == b'.' || rest.as_bytes()[i] == b')') {
        let mut j = i + 1;
        if j < rest.len() && (rest.as_bytes()[j] == b' ' || rest.as_bytes()[j] == b'\t') {
            while j < rest.len() && (rest.as_bytes()[j] == b' ' || rest.as_bytes()[j] == b'\t') {
                j += 1;
            }
            return Some((&line[..ws + j], &rest[j..]));
        }
    }
    None
}

fn fill_fence_marker(job: &mut egui::text::LayoutJob, font: &FontId, line: &str, close: bool) {
    let ws = leading_ws(line);
    if ws > 0 {
        append(
            job,
            font,
            &line[..ws],
            BODY,
            Color32::TRANSPARENT,
            false,
            false,
        );
    }
    let rest = &line[ws..];
    let ch = rest.chars().next().unwrap_or('`');
    let n = rest.chars().take_while(|&c| c == ch).count();
    let tick_len = ch.len_utf8() * n;
    append(
        job,
        font,
        &rest[..tick_len],
        FENCE,
        Color32::TRANSPARENT,
        false,
        false,
    );
    let after = &rest[tick_len..];
    if after.is_empty() {
        return;
    }
    if close {
        append(job, font, after, MARKER, Color32::TRANSPARENT, false, false);
    } else {
        append(
            job,
            font,
            after,
            FENCE_LANG,
            Color32::TRANSPARENT,
            false,
            false,
        );
    }
}

fn try_fence(line: &str) -> Option<(char, &str, bool)> {
    let t = line.trim_start();
    let ch = t.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let n = t.chars().take_while(|&c| c == ch).count();
    if n < 3 {
        return None;
    }
    let rest = t[ch.len_utf8() * n..].trim();
    if rest.is_empty() {
        Some((ch, "", true))
    } else {
        let lang = rest
            .split(|c: char| c == ' ' || c == '\t')
            .next()
            .unwrap_or("");
        Some((ch, lang, false))
    }
}

fn is_hr(line: &str) -> bool {
    let mut lead = 0usize;
    let bytes = line.as_bytes();
    while lead < bytes.len() && bytes[lead] == b' ' {
        lead += 1;
        if lead > 3 {
            return false;
        }
    }
    let t = line[lead..].trim_end();
    if t.is_empty() {
        return false;
    }
    let mut n = 0u32;
    let mut ch: Option<char> = None;
    for c in t.chars() {
        if c == ' ' || c == '\t' {
            continue;
        }
        match ch {
            None if c == '-' || c == '*' || c == '_' => {
                ch = Some(c);
                n = 1;
            }
            Some(x) if c == x => n += 1,
            _ => return false,
        }
    }
    n >= 3
}

fn leading_ws(s: &str) -> usize {
    s.bytes().take_while(|&b| b == b' ' || b == b'\t').count()
}

/// 行首（列表前缀已剥掉）的 GFM 任务框。整段返回，便于铺一块底色。
fn task_box_token(text: &str) -> Option<(&str, Color32, Color32)> {
    if text.starts_with("[ ]") {
        Some(("[ ]", TASK_OPEN_FG, TASK_OPEN_BG))
    } else if text.starts_with("[x]") || text.starts_with("[X]") {
        Some((&text[..3], TASK_DONE_FG, TASK_DONE_BG))
    } else {
        None
    }
}

fn color_inlines(
    job: &mut egui::text::LayoutJob,
    faces: &Faces,
    text: &str,
    fg: Color32,
    default_bold: bool,
) {
    if text.is_empty() {
        return;
    }
    let font = &faces.regular;
    let body = if default_bold {
        &faces.heading
    } else {
        &faces.regular
    };
    // 任务框 - [ ] / - [x]：整段一块铺底，避免括号与中间字各刷一层。
    if let Some((tok, tfg, tbg)) = task_box_token(text) {
        append(job, font, tok, tfg, tbg, false, false);
        color_inlines(job, faces, &text[tok.len()..], fg, default_bold);
        return;
    }
    let n = text.len();
    let mut i = 0;
    let mut buf_from = 0;
    let flush = |job: &mut egui::text::LayoutJob, from: usize, to: usize| {
        if to > from {
            append(
                job,
                body,
                &text[from..to],
                fg,
                Color32::TRANSPARENT,
                false,
                false,
            );
        }
    };
    while i < n {
        let b = text.as_bytes()[i];
        if b == b'`' {
            if let Some(rel) = text[i + 1..].find('`') {
                let j = i + 1 + rel;
                if j > i {
                    flush(job, buf_from, i);
                    append(job, font, "`", MARKER, CODE_BG, false, false);
                    append(job, font, &text[i + 1..j], CODE, CODE_BG, false, false);
                    append(job, font, "`", MARKER, CODE_BG, false, false);
                    i = j + 1;
                    buf_from = i;
                    continue;
                }
            }
        }
        if b == b'!' && i + 1 < n && text.as_bytes()[i + 1] == b'[' {
            if let Some((end, lab_a, lab_b, href_a, href_b)) = link_parts(text, i + 1) {
                flush(job, buf_from, i);
                append(job, font, "![", MARKER, Color32::TRANSPARENT, false, false);
                append(
                    job,
                    font,
                    &text[lab_a..lab_b],
                    IMAGE,
                    Color32::TRANSPARENT,
                    false,
                    false,
                );
                append(job, font, "](", MARKER, Color32::TRANSPARENT, false, false);
                append(
                    job,
                    font,
                    &text[href_a..href_b],
                    MARKER,
                    Color32::TRANSPARENT,
                    false,
                    false,
                );
                append(job, font, ")", MARKER, Color32::TRANSPARENT, false, false);
                i = end;
                buf_from = i;
                continue;
            }
        }
        if b == b'[' {
            if let Some((end, lab_a, lab_b, href_a, href_b)) = link_parts(text, i) {
                flush(job, buf_from, i);
                append(job, font, "[", MARKER, Color32::TRANSPARENT, false, false);
                append_link_text(job, font, &text[lab_a..lab_b]);
                append(job, font, "](", MARKER, Color32::TRANSPARENT, false, false);
                append(
                    job,
                    font,
                    &text[href_a..href_b],
                    MARKER,
                    Color32::TRANSPARENT,
                    false,
                    false,
                );
                append(job, font, ")", MARKER, Color32::TRANSPARENT, false, false);
                i = end;
                buf_from = i;
                continue;
            }
        }
        if b == b'=' && i + 1 < n && text.as_bytes()[i + 1] == b'=' {
            if let Some(rel) = text[i + 2..].find("==") {
                let j = i + 2 + rel;
                if j > i + 2 {
                    flush(job, buf_from, i);
                    append(job, font, "==", MARKER, Color32::TRANSPARENT, false, false);
                    append(
                        job,
                        font,
                        &text[i + 2..j],
                        MARK,
                        Color32::TRANSPARENT,
                        false,
                        false,
                    );
                    append(job, font, "==", MARKER, Color32::TRANSPARENT, false, false);
                    i = j + 2;
                    buf_from = i;
                    continue;
                }
            }
        }
        if b == b'~' && i + 1 < n && text.as_bytes()[i + 1] == b'~' {
            if let Some(rel) = text[i + 2..].find("~~") {
                let j = i + 2 + rel;
                if j > i + 2 {
                    flush(job, buf_from, i);
                    append(job, font, "~~", MARKER, Color32::TRANSPARENT, false, false);
                    append(
                        job,
                        font,
                        &text[i + 2..j],
                        STRIKE,
                        Color32::TRANSPARENT,
                        false,
                        true,
                    );
                    append(job, font, "~~", MARKER, Color32::TRANSPARENT, false, false);
                    i = j + 2;
                    buf_from = i;
                    continue;
                }
            }
        }
        if (b == b'*' || b == b'_')
            && i + 2 < n
            && text.as_bytes()[i + 1] == b
            && text.as_bytes()[i + 2] == b
        {
            let mark = &text[i..i + 3];
            if let Some(rel) = text[i + 3..].find(mark) {
                let j = i + 3 + rel;
                if j > i + 3 {
                    flush(job, buf_from, i);
                    append(job, font, mark, MARKER, Color32::TRANSPARENT, false, false);
                    append(
                        job,
                        &faces.bold,
                        &text[i + 3..j],
                        fg,
                        Color32::TRANSPARENT,
                        true,
                        false,
                    );
                    append(job, font, mark, MARKER, Color32::TRANSPARENT, false, false);
                    i = j + 3;
                    buf_from = i;
                    continue;
                }
            }
        }
        if (b == b'*' || b == b'_') && i + 1 < n && text.as_bytes()[i + 1] == b {
            let mark_len = 2;
            let mark = &text[i..i + mark_len];
            if let Some(rel) = text[i + mark_len..].find(mark) {
                let j = i + mark_len + rel;
                if j > i + mark_len {
                    flush(job, buf_from, i);
                    append(job, font, mark, MARKER, Color32::TRANSPARENT, false, false);
                    append(
                        job,
                        &faces.bold,
                        &text[i + mark_len..j],
                        fg,
                        Color32::TRANSPARENT,
                        false,
                        false,
                    );
                    append(job, font, mark, MARKER, Color32::TRANSPARENT, false, false);
                    i = j + mark_len;
                    buf_from = i;
                    continue;
                }
            }
        }
        if b == b'*' || b == b'_' {
            let ch = b as char;
            if let Some(rel) = text[i + 1..].find(ch) {
                let j = i + 1 + rel;
                if j > i + 1 && (j + 1 >= n || text.as_bytes()[j + 1] != b) {
                    flush(job, buf_from, i);
                    append(
                        job,
                        font,
                        &text[i..i + 1],
                        MARKER,
                        Color32::TRANSPARENT,
                        false,
                        false,
                    );
                    append(
                        job,
                        body,
                        &text[i + 1..j],
                        fg,
                        Color32::TRANSPARENT,
                        true,
                        false,
                    );
                    append(
                        job,
                        font,
                        &text[j..j + 1],
                        MARKER,
                        Color32::TRANSPARENT,
                        false,
                        false,
                    );
                    i = j + 1;
                    buf_from = i;
                    continue;
                }
            }
        }
        if b == b'h' && (text[i..].starts_with("http://") || text[i..].starts_with("https://")) {
            let mut j = i;
            while j < n {
                let c = text.as_bytes()[j];
                if c.is_ascii_whitespace() || c == b')' {
                    break;
                }
                j += 1;
            }
            while j > i {
                let prev = text.as_bytes()[j - 1];
                if matches!(prev, b'.' | b',' | b';' | b':') {
                    j -= 1;
                } else {
                    break;
                }
            }
            flush(job, buf_from, i);
            append_link_text(job, font, &text[i..j]);
            i = j;
            buf_from = i;
            continue;
        }
        let ch = text[i..].chars().next().unwrap();
        i += ch.len_utf8();
    }
    flush(job, buf_from, n);
}

/// 源码可跳转目标。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SrcLink {
    Href(String),
    Image { href: String, alt: String },
}

/// 源码光标处的链接（`[文字](href)`、`![alt](src)` 或 `http(s)://`）。围栏代码、行内 code 不算。
pub fn link_at_char(text: &str, char_idx: usize) -> Option<SrcLink> {
    if text.is_empty() {
        return None;
    }
    let n_chars = text.chars().count();
    let char_idx = char_idx.min(n_chars.saturating_sub(1));
    for &(a, b) in &fence_char_spans(text) {
        if char_idx >= a && char_idx < b {
            return None;
        }
    }
    let byte = char_to_byte(text, char_idx);
    let line_start = text[..byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = text[byte..].find('\n').map(|i| byte + i).unwrap_or(text.len());
    link_in_line(text, line_start, line_end, byte)
}

fn link_in_line(text: &str, start: usize, end: usize, byte: usize) -> Option<SrcLink> {
    let mut i = start;
    while i < end {
        let b = text.as_bytes()[i];
        if b == b'`' {
            if let Some(rel) = text[i + 1..end].find('`') {
                let j = i + 1 + rel;
                if byte >= i && byte <= j {
                    return None;
                }
                i = j + 1;
                continue;
            }
        }
        if b == b'!' && i + 1 < end && text.as_bytes()[i + 1] == b'[' {
            if let Some((link_end, lab_a, lab_b, href_a, href_b)) = link_parts(text, i + 1) {
                if byte >= i && byte < link_end {
                    let href = clean_href(&text[href_a..href_b]);
                    if href.is_empty() {
                        return None;
                    }
                    return Some(SrcLink::Image {
                        href,
                        alt: text[lab_a..lab_b].to_string(),
                    });
                }
                i = link_end.min(end);
                continue;
            }
        }
        if b == b'[' {
            if let Some((link_end, _, _, href_a, href_b)) = link_parts(text, i) {
                if byte >= i && byte < link_end {
                    let href = clean_href(&text[href_a..href_b]);
                    if !href.is_empty() {
                        return Some(SrcLink::Href(href));
                    }
                    return None;
                }
                i = link_end.min(end);
                continue;
            }
        }
        if b == b'h' && (text[i..].starts_with("http://") || text[i..].starts_with("https://")) {
            let mut j = i;
            while j < end {
                let c = text.as_bytes()[j];
                if c.is_ascii_whitespace() || c == b')' {
                    break;
                }
                j += 1;
            }
            while j > i {
                let prev = text.as_bytes()[j - 1];
                if matches!(prev, b'.' | b',' | b';' | b':') {
                    j -= 1;
                } else {
                    break;
                }
            }
            if byte >= i && byte < j {
                return Some(SrcLink::Href(text[i..j].to_string()));
            }
            i = j;
            continue;
        }
        let ch = text[i..].chars().next()?;
        i += ch.len_utf8();
    }
    None
}

fn clean_href(raw: &str) -> String {
    let s = raw.trim();
    let s = s
        .strip_prefix('<')
        .and_then(|x| x.strip_suffix('>'))
        .unwrap_or(s)
        .trim();
    let cut = s
        .find(|c| c == '"' || c == '\'')
        .map(|i| s[..i].trim())
        .unwrap_or(s);
    cut.to_string()
}

fn link_parts(text: &str, open_bracket: usize) -> Option<(usize, usize, usize, usize, usize)> {
    if open_bracket >= text.len() || text.as_bytes()[open_bracket] != b'[' {
        return None;
    }
    let close = text[open_bracket + 1..].find(']')?;
    let close = open_bracket + 1 + close;
    if close + 1 >= text.len() || text.as_bytes()[close + 1] != b'(' {
        return None;
    }
    let endp = text[close + 2..].find(')')?;
    let endp = close + 2 + endp;
    Some((endp + 1, open_bracket + 1, close, close + 2, endp))
}

fn append_link_text(job: &mut egui::text::LayoutJob, font: &FontId, s: &str) {
    append(job, font, s, LINK, Color32::TRANSPARENT, false, false);
    // underline via last section — LayoutJob doesn't allow editing last format easily;
    // re-append with underline by using append() then we need underline in append.
}

fn append(
    job: &mut egui::text::LayoutJob,
    font: &FontId,
    text: &str,
    color: Color32,
    bg: Color32,
    italics: bool,
    strike: bool,
) {
    if text.is_empty() {
        return;
    }
    let underline = if color == LINK {
        Stroke::new(1.0_f32, LINK)
    } else {
        Stroke::NONE
    };
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: font.clone(),
            color,
            background: bg,
            italics,
            valign: Align::BOTTOM,
            line_height: Some((font.size * 1.45).round()),
            underline,
            strikethrough: if strike {
                Stroke::new(1.0_f32, color)
            } else {
                Stroke::NONE
            },
            ..Default::default()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn faces() -> Faces {
        Faces {
            regular: FontId::monospace(13.0),
            bold: FontId::new(13.0, theme::mono_bold_family()),
            heading: FontId::new(13.0, theme::mono_bold_family()),
        }
    }

    fn job_of(s: &str) -> egui::text::LayoutJob {
        build(s, &faces())
    }

    #[test]
    fn job_text_matches_source() {
        let s = "# Title\n\nHello **bold** and `code`\n- item [link](https://a)\n```rs\nfn x() {}\n```\n";
        let job = job_of(s);
        assert_eq!(job.text, s);
    }

    fn assert_jobs_eq(a: &egui::text::LayoutJob, b: &egui::text::LayoutJob) {
        assert_eq!(a.text, b.text);
        assert_eq!(a.sections.len(), b.sections.len());
        for (x, y) in a.sections.iter().zip(b.sections.iter()) {
            assert_eq!(x.byte_range, y.byte_range);
            assert_eq!(x.format.color, y.format.color);
            assert_eq!(x.format.background, y.format.background);
        }
    }

    fn incr_then(a: &str, b: &str) -> (egui::text::LayoutJob, egui::text::LayoutJob) {
        let faces = faces();
        let sz = 13.0f32.to_bits();
        let mut mem = None;
        let _ = incr_highlight(a, &faces, sz, hash_text(a), &mut mem);
        let incr = incr_highlight(b, &faces, sz, hash_text(b), &mut mem);
        (incr, build(b, &faces))
    }

    #[test]
    fn incr_edit_one_line_matches_full() {
        let (incr, full) = incr_then("# Title\nhello world\n- item\n", "# Title\nhello W\n- item\n");
        assert_jobs_eq(&incr, &full);
    }

    #[test]
    fn incr_insert_line_matches_full() {
        let (incr, full) = incr_then("aaa\nccc\n", "aaa\nbbb\nccc\n");
        assert_jobs_eq(&incr, &full);
    }

    #[test]
    fn incr_edit_inside_fence_matches_full() {
        let a = "before\n```rs\nfn a() {}\nfn b() {}\n```\nafter\n";
        let b = "before\n```rs\nfn a() { x }\nfn b() {}\n```\nafter\n";
        let (incr, full) = incr_then(a, b);
        assert_jobs_eq(&incr, &full);
    }

    #[test]
    fn heading_uses_heading_color() {
        let job = job_of("# Hello\n");
        let colors: Vec<_> = job.sections.iter().map(|s| s.format.color).collect();
        assert!(colors.contains(&heading_fg(1)));
        assert!(colors.contains(&heading_mark(1)));
    }

    #[test]
    fn fence_lang_is_purple() {
        let job = job_of("```rust\nlet x = 1;\n```\n");
        assert_eq!(job.text, "```rust\nlet x = 1;\n```\n");
        assert!(job.sections.iter().any(|s| s.format.color == FENCE_LANG));
    }

    #[test]
    fn inline_code_has_preview_gray_bg() {
        let job = job_of("say `xx` ok\n");
        let hit = job.sections.iter().any(|s| {
            s.format.background == CODE_BG && job.text[s.byte_range.clone()].contains("xx")
        });
        assert!(hit);
        let ticks = job
            .sections
            .iter()
            .filter(|s| &job.text[s.byte_range.clone()] == "`")
            .all(|s| s.format.background == CODE_BG);
        assert!(ticks);
    }

    #[test]
    fn crlf_preserved() {
        let s = "# A\r\npara\r\n";
        assert_eq!(job_of(s).text, s);
    }

    #[test]
    fn apply_sel_bg_splits_section() {
        let mut job = job_of("hello world\n");
        apply_sel_bg(&mut job, "hello world\n", 0, 5);
        assert_eq!(job.text, "hello world\n");
        let hit = job.sections.iter().any(|s| {
            s.format.background.a() > 0 && job.text[s.byte_range.clone()].contains("hello")
        });
        assert!(hit);
    }

    #[test]
    fn fence_spans_include_markers() {
        let s = "a\n```lua\nx\n```\nb\n";
        assert_eq!(fence_char_spans(s), vec![(2, 15)]);
        let open = "```\nfoo";
        assert_eq!(fence_char_spans(open), vec![(0, open.chars().count())]);
        assert!(fence_char_spans("# hi\npara\n").is_empty());
    }

    fn char_of(s: &str, byte: usize) -> usize {
        s[..byte].chars().count()
    }

    #[test]
    fn href_at_hash_and_md() {
        let s = "见 [节](#标题) 和 [文](a.md#x)\n";
        let a = s.find("节").unwrap();
        let b = s.find("a.md").unwrap();
        assert_eq!(
            link_at_char(s, char_of(s, a)),
            Some(SrcLink::Href("#标题".into()))
        );
        assert_eq!(
            link_at_char(s, char_of(s, b)),
            Some(SrcLink::Href("a.md#x".into()))
        );
    }

    #[test]
    fn href_skips_fence_and_code() {
        let s = "```\n[x](#a)\n```\n`[y](#b)` [z](c.md)\n";
        let x = s.find("[x]").unwrap() + 1;
        let y = s.find("[y]").unwrap() + 1;
        let z = s.find("[z]").unwrap() + 1;
        assert!(link_at_char(s, char_of(s, x)).is_none());
        assert!(link_at_char(s, char_of(s, y)).is_none());
        assert_eq!(
            link_at_char(s, char_of(s, z)),
            Some(SrcLink::Href("c.md".into()))
        );
    }

    #[test]
    fn href_image_and_md() {
        let s = "![图](a.png) [去](b.md)\n";
        let img = s.find("图").unwrap();
        let md = s.find("去").unwrap();
        assert_eq!(
            link_at_char(s, char_of(s, img)),
            Some(SrcLink::Image {
                href: "a.png".into(),
                alt: "图".into(),
            })
        );
        assert_eq!(
            link_at_char(s, char_of(s, md)),
            Some(SrcLink::Href("b.md".into()))
        );
    }

    #[test]
    fn heading_and_bold_use_bold_font() {
        let job = job_of("# Title\npara **x** and ***y***\n");
        let heading_fam = theme::mono_bold_family();
        let mono_bold = theme::mono_bold_family();
        let slice = |s: &egui::text::LayoutSection| job.text[s.byte_range.clone()].to_string();
        assert!(job
            .sections
            .iter()
            .any(|s| { s.format.font_id.family == heading_fam && slice(s).contains("Title") }));
        assert!(job
            .sections
            .iter()
            .any(|s| { s.format.font_id.family == heading_fam && slice(s).contains('#') }));
        assert!(job
            .sections
            .iter()
            .any(|s| s.format.font_id.family == mono_bold && slice(s) == "x"));
        assert!(job.sections.iter().any(|s| {
            s.format.font_id.family == mono_bold && s.format.italics && slice(s) == "y"
        }));
    }

    #[test]
    fn overlay_key_ignores_identical_slices() {
        let a = LayoutOverlay {
            hint: Some((1, 8)),
            find_all: &[(2, 4), (10, 12)],
            find_cur: Some((2, 4)),
        };
        let b = LayoutOverlay {
            hint: Some((1, 8)),
            find_all: &[(2, 4), (10, 12)],
            find_cur: Some((2, 4)),
        };
        assert_eq!(a.extra_key(), b.extra_key());
        let c = LayoutOverlay {
            hint: None,
            find_all: &[],
            find_cur: None,
        };
        assert_ne!(a.extra_key(), c.extra_key());
    }

    #[test]
    fn wrap_width_quantized_to_8px() {
        assert_eq!(quantize_wrap(16.0), 16);
        assert_eq!(quantize_wrap(15.0), 16);
        assert_eq!(quantize_wrap(1.0), 8);
        assert_eq!(quantize_wrap(20.0), 24);
    }

    #[test]
    fn wrap_slack_reuses_nearby_width() {
        assert!(wrap_close(800, 800, false));
        assert!(wrap_close(800, 848, false));
        assert!(wrap_close(800, 864, false));
        assert!(!wrap_close(800, 880, false));
        assert!(wrap_close(800, 2000, true));
    }

    #[test]
    fn byte_ranges_to_chars_ascii_and_cjk() {
        let s = "ab你好cd";
        // "你" starts at byte 2, "好" at 5, "c" at 8
        let out = byte_ranges_to_chars(s, &[(2, 8)]);
        assert_eq!(out, vec![(2, 4)]);
        let multi = byte_ranges_to_chars(s, &[(0, 2), (8, 10)]);
        assert_eq!(multi, vec![(0, 2), (4, 6)]);
    }

    #[test]
    fn task_box_has_distinct_color_and_bg() {
        let job = job_of("- [ ] open\n- [x] done\n- [X] also\n");
        assert_eq!(job.text, "- [ ] open\n- [x] done\n- [X] also\n");
        let slice = |s: &egui::text::LayoutSection| job.text[s.byte_range.clone()].to_string();
        let open = job
            .sections
            .iter()
            .find(|s| slice(s) == "[ ]")
            .expect("[ ]");
        assert_eq!(open.format.color, TASK_OPEN_FG);
        assert_eq!(open.format.background, TASK_OPEN_BG);
        let done = job
            .sections
            .iter()
            .find(|s| slice(s) == "[x]")
            .expect("[x]");
        assert_eq!(done.format.color, TASK_DONE_FG);
        assert_eq!(done.format.background, TASK_DONE_BG);
        let done_upper = job
            .sections
            .iter()
            .find(|s| slice(s) == "[X]")
            .expect("[X]");
        assert_eq!(done_upper.format.color, TASK_DONE_FG);
        assert_eq!(done_upper.format.background, TASK_DONE_BG);
        assert_ne!(open.format.color, done.format.color);
        assert_ne!(open.format.background, done.format.background);
    }
}
