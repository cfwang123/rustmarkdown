//! Markdown 源码着色（对齐 docview 纯代码/vim 风：同字号、标题分层色、标记灰色）。
//! LayoutJob 文本必须与源码逐字节一致，否则 TextEdit 光标会错位。

use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use egui::{Align, Color32, FontId, Stroke, TextFormat, TextStyle};

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

struct Cache {
    hash: u64,
    font_sz: u32,
    job: egui::text::LayoutJob,
}

struct GalleyEntry {
    hash: u64,
    font_sz: u32,
    wrap: u32,
    extra: u64,
    galley: std::sync::Arc<egui::Galley>,
}

const GALLEY_SLOTS: usize = 8;
const WRAP_QUANT: u32 = 8;
const STICKY_WRAP_SLACK: u32 = 32;

struct FenceCache {
    hash: u64,
    spans: Vec<(usize, usize)>,
}

static CACHE: LazyLock<Mutex<Option<Cache>>> = LazyLock::new(|| Mutex::new(None));
static GALLEY: LazyLock<Mutex<Vec<GalleyEntry>>> = LazyLock::new(|| Mutex::new(Vec::new()));
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
        let job = build(text, &faces);
        *g = Some(Cache {
            hash,
            font_sz,
            job: job.clone(),
        });
        job
    }
}

/// 查找命中 / 预览映射到底的叠加层。空则只排正文。
pub struct LayoutOverlay<'a> {
    pub hint: Option<(usize, usize)>,
    pub find_all: &'a [(usize, usize)],
    pub find_cur: Option<(usize, usize)>,
}

impl LayoutOverlay<'_> {
    fn extra_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.hint.hash(&mut h);
        self.find_all.hash(&mut h);
        self.find_cur.hash(&mut h);
        h.finish()
    }
}

fn quantize_wrap(wrap_width: f32) -> u32 {
    let q = (wrap_width / WRAP_QUANT as f32).round() as u32;
    q.max(1).saturating_mul(WRAP_QUANT).min(100_000)
}

fn galley_lookup(
    hash: u64,
    font_sz: u32,
    wrap: u32,
    extra: u64,
    sticky_wrap: bool,
) -> Option<std::sync::Arc<egui::Galley>> {
    let mut g = GALLEY.lock().unwrap_or_else(|e| e.into_inner());
    let pos = g.iter().position(|e| {
        e.hash == hash
            && e.font_sz == font_sz
            && e.extra == extra
            && (e.wrap == wrap || (sticky_wrap && e.wrap.abs_diff(wrap) <= STICKY_WRAP_SLACK))
    })?;
    let e = g.remove(pos);
    let galley = e.galley.clone();
    g.insert(0, e);
    Some(galley)
}

fn galley_store(entry: GalleyEntry) {
    let mut g = GALLEY.lock().unwrap_or_else(|e| e.into_inner());
    g.retain(|e| {
        !(e.hash == entry.hash
            && e.font_sz == entry.font_sz
            && e.wrap == entry.wrap
            && e.extra == entry.extra)
    });
    g.insert(0, entry);
    g.truncate(GALLEY_SLOTS);
}

/// 按当前字号与折行宽取 Galley；文本/叠加层未改时复用。
/// `sticky_wrap`：拖选时折行宽微变仍用上一份，避免整篇重排卡几秒。
pub fn layout_galley(
    ui: &egui::Ui,
    text: &str,
    wrap_width: f32,
    overlay: &LayoutOverlay<'_>,
    sticky_wrap: bool,
) -> std::sync::Arc<egui::Galley> {
    let font = TextStyle::Monospace.resolve(ui.style());
    let font_sz = font.size.to_bits();
    let wrap = quantize_wrap(wrap_width);
    let hash = hash_text(text);
    let extra = overlay.extra_key();
    if let Some(g) = galley_lookup(hash, font_sz, wrap, extra, sticky_wrap) {
        return g;
    }
    let mut job = layout_job(ui, text);
    if let Some((a, b)) = overlay.hint {
        apply_sel_bg(&mut job, text, a, b);
    }
    if !overlay.find_all.is_empty() {
        let bg = Color32::from_rgba_unmultiplied(0xFE, 0xF0, 0x8A, 140);
        apply_byte_bgs(&mut job, overlay.find_all, bg, false);
    }
    if let Some(cur) = overlay.find_cur {
        let bg = Color32::from_rgba_unmultiplied(0xFD, 0xBA, 0x74, 200);
        apply_byte_bgs(&mut job, &[cur], bg, true);
    }
    job.wrap.max_width = wrap as f32;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    galley_store(GalleyEntry {
        hash,
        font_sz,
        wrap,
        extra,
        galley: galley.clone(),
    });
    galley
}

fn hash_text(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

fn build(text: &str, faces: &Faces) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    if text.is_empty() {
        return job;
    }
    if text.len() > MAX_CHARS {
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
    let mut in_fence = false;
    let mut fence_ch = '\0';
    let mut hl: Option<LineHl> = None;
    for chunk in text.split_inclusive('\n') {
        let has_nl = chunk.ends_with('\n');
        let raw = if has_nl {
            &chunk[..chunk.len() - 1]
        } else {
            chunk
        };
        let cr = raw.ends_with('\r');
        let line = if cr { &raw[..raw.len() - 1] } else { raw };
        color_line(&mut job, faces, line, &mut in_fence, &mut fence_ch, &mut hl);
        if cr {
            append(
                &mut job,
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
                &mut job,
                &faces.regular,
                "\n",
                BODY,
                Color32::TRANSPARENT,
                false,
                false,
            );
        }
    }
    debug_assert_eq!(job.text, text);
    job
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
pub fn apply_sel_bg(job: &mut egui::text::LayoutJob, text: &str, char0: usize, char1: usize) {
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

/// 按字节区间铺底色（查找命中）。`force` 覆盖已有底。
pub fn apply_byte_bgs(
    job: &mut egui::text::LayoutJob,
    ranges: &[(usize, usize)],
    bg: Color32,
    force: bool,
) {
    for &(b0, b1) in ranges {
        if b0 >= b1 {
            continue;
        }
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
                if force || f.background.a() == 0 {
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
                    append(job, font, "`", MARKER, Color32::TRANSPARENT, false, false);
                    append(
                        job,
                        font,
                        &text[i + 1..j],
                        CODE,
                        Color32::TRANSPARENT,
                        false,
                        false,
                    );
                    append(job, font, "`", MARKER, Color32::TRANSPARENT, false, false);
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
