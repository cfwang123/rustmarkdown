mod heading_num;
mod inline;
pub mod table;

pub use heading_num::HeadingNumber;
pub use inline::parse_inlines;
pub(crate) use inline::{try_fs_path, try_http_url};

use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MdBlockKind {
    Paragraph,
    Heading,
    ListItem,
    Quote,
    Code,
    Hr,
    Table,
    Html,
    HtmlImg,
    Details,
    Blank,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MdSpanKind {
    Text,
    Bold,
    Italic,
    Code,
    Link,
    Image,
    Mark,
    Strike,
    SoftBr,
    BoldItalic,
}

#[derive(Clone, Debug)]
pub struct MdSpan {
    pub kind: MdSpanKind,
    pub text: String,
    pub href: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug)]
pub struct MdBlock {
    pub kind: MdBlockKind,
    pub line0: usize,
    pub line1: usize,
    pub level: u32,
    pub ordered: bool,
    pub task: Option<bool>,
    pub lang: String,
    pub text: String,
    pub spans: Vec<MdSpan>,
    pub table_rows: Vec<Vec<String>>,
    pub table_align: Vec<TableAlign>,
    pub children: Vec<MdBlock>,
    pub summary: String,
    pub details_open: bool,
    pub img_w: Option<f32>,
    pub img_h: Option<f32>,
}

impl MdBlock {
    fn new(kind: MdBlockKind, line0: usize, line1: usize) -> Self {
        Self {
            kind,
            line0,
            line1,
            level: 0,
            ordered: false,
            task: None,
            lang: String::new(),
            text: String::new(),
            spans: Vec::new(),
            table_rows: Vec::new(),
            table_align: Vec::new(),
            children: Vec::new(),
            summary: String::new(),
            details_open: false,
            img_w: None,
            img_h: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MdDoc {
    pub blocks: Vec<MdBlock>,
    pub line_to_block: Vec<i32>,
}

static RE_HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(#{1,6})\s+(.*?)(?:\s+#*\s*)?$").unwrap());
static RE_FENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(`{3,}|~{3,})\s*([^\s`]*)\s*$").unwrap());
static RE_UL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\s*)([*+●•○◦-])\s+(.*)$").unwrap());
static RE_OL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)(\d{1,9})[.)]\s+(.*)$").unwrap());
static RE_QUOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^>\s?(.*)$").unwrap());
static RE_TABLE_SEP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\|?(\s*:?-+:?\s*\|)+\s*:?-+:?\s*\|?\s*$").unwrap());
static RE_TASK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[([ xX])\]\s+(.*)$").unwrap());
static RE_IMG_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<img\b([^>]*)/?\s*>").unwrap());
static RE_ATTR_DQ: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"([A-Za-z_:][\w:.-]*)\s*=\s*"([^"]*)""#).unwrap());
static RE_ATTR_SQ: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z_:][\w:.-]*)\s*=\s*'([^']*)'").unwrap());
static RE_ATTR_NQ: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"([A-Za-z_:][\w:.-]*)\s*=\s*([^\s"'=<>`]+)"#).unwrap());
static RE_OPEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bopen\b").unwrap());
static RE_DETAILS_OPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)^.*?<details\b[^>]*>").unwrap());
static RE_DETAILS_CLOSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)</details>\s*$").unwrap());
static RE_SUMMARY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<summary\b[^>]*>(.*?)</summary>").unwrap());

const MAX_DETAILS_DEPTH: i32 = 8;

pub fn parse(text: &str) -> MdDoc {
    parse_with_tab(text, 3)
}

pub fn parse_with_tab(text: &str, mut tab_size: i32) -> MdDoc {
    if tab_size < 1 {
        tab_size = 1;
    }
    if tab_size > 8 {
        tab_size = 8;
    }
    let mut doc = MdDoc::default();
    let lines = split_lines(text);
    doc.line_to_block = vec![-1; lines.len().max(1)];
    let mut i_line = 0;
    while i_line < lines.len() {
        let line = &lines[i_line];
        if line.trim().is_empty() {
            add(&mut doc, MdBlock::new(MdBlockKind::Blank, i_line, i_line));
            i_line += 1;
            continue;
        }
        if let Some(cap) = RE_FENCE.captures(line) {
            let fence = cap.get(1).unwrap().as_str().to_string();
            let lang = cap
                .get(2)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let start = i_line;
            i_line += 1;
            let mut sb = String::new();
            while i_line < lines.len() {
                let l = &lines[i_line];
                if is_fence_close(l, &fence) {
                    break;
                }
                if !sb.is_empty() {
                    sb.push('\n');
                }
                sb.push_str(l);
                i_line += 1;
            }
            let end = if i_line < lines.len() {
                i_line
            } else {
                lines.len() - 1
            };
            let mut b = MdBlock::new(MdBlockKind::Code, start, end);
            b.lang = lang;
            b.text = sb;
            add(&mut doc, b);
            if i_line < lines.len() {
                i_line += 1;
            }
            continue;
        }
        if is_hr(line) {
            let mut b = MdBlock::new(MdBlockKind::Hr, i_line, i_line);
            b.text = line.trim().to_string();
            add(&mut doc, b);
            i_line += 1;
            continue;
        }
        if let Some(cap) = RE_HEADING.captures(line) {
            let level = cap.get(1).unwrap().as_str().len() as u32;
            let body = cap.get(2).unwrap().as_str().trim().to_string();
            let mut b = MdBlock::new(MdBlockKind::Heading, i_line, i_line);
            b.level = level;
            b.text = body.clone();
            b.spans = parse_inlines(&body);
            add(&mut doc, b);
            i_line += 1;
            continue;
        }
        if line.contains('|')
            && i_line + 1 < lines.len()
            && RE_TABLE_SEP.is_match(&lines[i_line + 1])
        {
            let start = i_line;
            let mut rows = vec![split_table_row(line)];
            let align = parse_table_align(&lines[i_line + 1]);
            i_line += 2;
            while i_line < lines.len()
                && lines[i_line].contains('|')
                && !lines[i_line].trim().is_empty()
            {
                if RE_TABLE_SEP.is_match(&lines[i_line]) {
                    i_line += 1;
                    continue;
                }
                rows.push(split_table_row(&lines[i_line]));
                i_line += 1;
            }
            let mut b = MdBlock::new(MdBlockKind::Table, start, i_line.saturating_sub(1));
            b.table_rows = rows;
            b.table_align = align;
            add(&mut doc, b);
            continue;
        }
        if RE_QUOTE.is_match(line) {
            let start = i_line;
            let mut sb = String::new();
            while i_line < lines.len() {
                if let Some(m) = RE_QUOTE.captures(&lines[i_line]) {
                    if !sb.is_empty() {
                        sb.push('\n');
                    }
                    sb.push_str(m.get(1).unwrap().as_str());
                    i_line += 1;
                } else {
                    break;
                }
            }
            let mut b = MdBlock::new(MdBlockKind::Quote, start, i_line.saturating_sub(1));
            b.text = sb.clone();
            b.spans = parse_inlines(&sb);
            add(&mut doc, b);
            continue;
        }
        let um = RE_UL.captures(line);
        let om = if um.is_none() {
            RE_OL.captures(line)
        } else {
            None
        };
        if um.is_some() || om.is_some() {
            let ordered = om.is_some();
            let m = if ordered { om.unwrap() } else { um.unwrap() };
            let indent_ws = m.get(1).unwrap().as_str();
            let indent = indent_cols(indent_ws, tab_size);
            let start = i_line;
            let mut body = m.get(3).unwrap().as_str().to_string();
            let mut task = None;
            if let Some(tm) = RE_TASK.captures(&body) {
                let ch = tm.get(1).unwrap().as_str();
                task = Some(ch == "x" || ch == "X");
                body = tm.get(2).unwrap().as_str().to_string();
            }
            i_line += 1;
            while i_line < lines.len() {
                let l = &lines[i_line];
                if l.trim().is_empty() {
                    break;
                }
                if RE_HEADING.is_match(l)
                    || RE_FENCE.is_match(l)
                    || is_hr(l)
                    || RE_UL.is_match(l)
                    || RE_OL.is_match(l)
                    || RE_QUOTE.is_match(l)
                {
                    break;
                }
                if !l.is_empty() && (l.as_bytes()[0] == b' ' || l.as_bytes()[0] == b'\t') {
                    body.push('\n');
                    body.push_str(l.trim());
                    i_line += 1;
                    continue;
                }
                break;
            }
            let mut b = MdBlock::new(MdBlockKind::ListItem, start, i_line.saturating_sub(1));
            b.level = indent as u32;
            b.ordered = ordered;
            b.task = task;
            b.text = body.clone();
            b.spans = parse_inlines(&body);
            add(&mut doc, b);
            continue;
        }
        let t = line.trim_start();
        if is_details_tag(t) {
            if let Some(det) = try_details_block(&lines, i_line, tab_size, 0) {
                i_line = det.line1 + 1;
                add(&mut doc, det);
                continue;
            }
        }
        if is_img_tag(t) {
            if let Some(mut img) = try_html_img_block(t) {
                img.line0 = i_line;
                img.line1 = i_line;
                add(&mut doc, img);
                i_line += 1;
                continue;
            }
        }
        if t.starts_with('<')
            && t.find('>').is_some()
            && !t.to_ascii_lowercase().starts_with("<http")
        {
            let start = i_line;
            let mut sb = String::new();
            while i_line < lines.len() {
                if !sb.is_empty() {
                    sb.push('\n');
                }
                sb.push_str(&lines[i_line]);
                let done = lines[i_line].contains("</") || lines[i_line].trim_end().ends_with("/>");
                i_line += 1;
                if done {
                    break;
                }
                if i_line < lines.len() && lines[i_line].trim().is_empty() {
                    break;
                }
            }
            let mut b = MdBlock::new(MdBlockKind::Html, start, i_line.saturating_sub(1));
            b.text = sb;
            add(&mut doc, b);
            continue;
        }
        {
            let start = i_line;
            let mut sb = String::new();
            while i_line < lines.len() {
                let l = &lines[i_line];
                if l.trim().is_empty() {
                    break;
                }
                if RE_HEADING.is_match(l)
                    || RE_FENCE.is_match(l)
                    || is_hr(l)
                    || RE_UL.is_match(l)
                    || RE_OL.is_match(l)
                    || RE_QUOTE.is_match(l)
                {
                    break;
                }
                if l.contains('|')
                    && i_line + 1 < lines.len()
                    && RE_TABLE_SEP.is_match(&lines[i_line + 1])
                {
                    break;
                }
                let lt = l.trim_start();
                if is_details_tag(lt) || is_img_tag(lt) {
                    break;
                }
                if lt.starts_with('<')
                    && lt.find('>').is_some()
                    && !lt.to_ascii_lowercase().starts_with("<http")
                {
                    break;
                }
                if !sb.is_empty() {
                    sb.push('\n');
                }
                sb.push_str(l);
                i_line += 1;
            }
            let mut b = MdBlock::new(MdBlockKind::Paragraph, start, i_line.saturating_sub(1));
            b.text = sb.clone();
            b.spans = parse_inlines(&sb);
            add(&mut doc, b);
        }
    }
    doc
}

fn is_hr(line: &str) -> bool {
    let lead = line.chars().take_while(|c| c.is_whitespace()).count();
    if lead > 3 {
        return false;
    }
    let t = line.trim();
    let mut chars = t.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    let mut count = 1;
    for ch in chars {
        if ch == first {
            count += 1;
        } else if ch.is_whitespace() {
            continue;
        } else {
            return false;
        }
    }
    count >= 3
}

fn is_fence_close(line: &str, fence: &str) -> bool {
    line.starts_with(fence)
        && line.trim_end().len() >= fence.len()
        && line.trim().trim_matches(['`', '~']).is_empty()
}

pub fn indent_cols(ws: &str, mut tab_size: i32) -> i32 {
    if ws.is_empty() {
        return 0;
    }
    if tab_size < 1 {
        tab_size = 1;
    }
    let mut col = 0;
    for ch in ws.chars() {
        if ch == '\t' {
            col += tab_size - (col % tab_size);
        } else {
            col += 1;
        }
    }
    col
}

pub fn expand_tabs(text: &str, tab_size: i32) -> String {
    expand_tabs_opt(text, tab_size, true)
}

pub fn expand_tabs_opt(text: &str, mut tab_size: i32, outside_fences_only: bool) -> String {
    if text.is_empty() || !text.contains('\t') {
        return text.to_string();
    }
    if tab_size < 1 {
        tab_size = 1;
    }
    let lines = split_lines(text);
    let mut sb = String::with_capacity(text.len() + 32);
    let mut in_fence = false;
    let mut fence_ch = '\0';
    let mut fence_len = 0usize;
    for (li, line) in lines.iter().enumerate() {
        if li > 0 {
            sb.push('\n');
        }
        if outside_fences_only {
            let t = line.trim_start();
            if t.starts_with("```") || t.starts_with("~~~") {
                let ch = t.chars().next().unwrap();
                let n = t.chars().take_while(|&c| c == ch).count();
                if n >= 3 {
                    if !in_fence {
                        in_fence = true;
                        fence_ch = ch;
                        fence_len = n;
                        sb.push_str(line);
                        continue;
                    }
                    if ch == fence_ch && n >= fence_len && t[n..].trim().is_empty() {
                        in_fence = false;
                        sb.push_str(line);
                        continue;
                    }
                }
            }
            if in_fence {
                sb.push_str(line);
                continue;
            }
        }
        let mut col = 0;
        for ch in line.chars() {
            if ch == '\t' {
                let mut nsp = tab_size - (col % tab_size);
                if nsp <= 0 {
                    nsp = tab_size;
                }
                for _ in 0..nsp {
                    sb.push(' ');
                }
                col += nsp;
            } else {
                sb.push(ch);
                col += 1;
            }
        }
    }
    sb
}

pub fn retarget_leading_indent(text: &str, mut from_tab: i32, mut to_tab: i32) -> String {
    if text.is_empty() {
        return text.to_string();
    }
    if from_tab < 1 {
        from_tab = 1;
    }
    if to_tab < 1 {
        to_tab = 1;
    }
    if from_tab == to_tab {
        return text.to_string();
    }
    let lines = split_lines(text);
    let mut sb = String::with_capacity(text.len() + 32);
    let mut in_fence = false;
    let mut fence_ch = '\0';
    let mut fence_len = 0usize;
    for (li, line) in lines.iter().enumerate() {
        if li > 0 {
            sb.push('\n');
        }
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            let ch = t.chars().next().unwrap();
            let n = t.chars().take_while(|&c| c == ch).count();
            if n >= 3 {
                if !in_fence {
                    in_fence = true;
                    fence_ch = ch;
                    fence_len = n;
                    sb.push_str(line);
                    continue;
                }
                if ch == fence_ch && n >= fence_len && t[n..].trim().is_empty() {
                    in_fence = false;
                    sb.push_str(line);
                    continue;
                }
            }
        }
        if in_fence {
            sb.push_str(line);
            continue;
        }
        let mut i = 0;
        let bytes = line.as_bytes();
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }
        if i == 0 {
            sb.push_str(line);
            continue;
        }
        let cols = indent_cols(&line[..i], from_tab);
        let levels = cols / from_tab;
        let rem = cols % from_tab;
        let new_cols = levels * to_tab + rem;
        for _ in 0..new_cols {
            sb.push(' ');
        }
        sb.push_str(&line[i..]);
    }
    sb
}

pub fn split_lines(text: &str) -> Vec<String> {
    let mut list = Vec::new();
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    if text.is_empty() {
        list.push(String::new());
        return list;
    }
    let mut i = 0;
    let bytes = text.as_bytes();
    loop {
        if let Some(rel) = text[i..].find('\n') {
            list.push(text[i..i + rel].to_string());
            i += rel + 1;
            if i == bytes.len() {
                list.push(String::new());
                break;
            }
        } else {
            list.push(text[i..].to_string());
            break;
        }
    }
    list
}

fn split_table_row(line: &str) -> Vec<String> {
    let mut line = line.trim();
    if line.starts_with('|') {
        line = &line[1..];
    }
    if line.ends_with('|') {
        line = &line[..line.len() - 1];
    }
    line.split('|').map(|p| p.trim().to_string()).collect()
}

fn parse_table_align(sep: &str) -> Vec<TableAlign> {
    split_table_row(sep)
        .into_iter()
        .map(|c| {
            let t = c.trim();
            let left = t.starts_with(':');
            let right = t.ends_with(':');
            if left && right {
                TableAlign::Center
            } else if right {
                TableAlign::Right
            } else {
                TableAlign::Left
            }
        })
        .collect()
}

fn add(doc: &mut MdDoc, b: MdBlock) {
    let idx = doc.blocks.len() as i32;
    if !doc.line_to_block.is_empty() {
        let a = b.line0;
        let z = b.line1.min(doc.line_to_block.len().saturating_sub(1));
        if a < doc.line_to_block.len() {
            for i in a..=z {
                if doc.line_to_block[i] < 0 {
                    doc.line_to_block[i] = idx;
                }
            }
        }
    }
    doc.blocks.push(b);
}

pub fn block_index_for_line(doc: &MdDoc, mut line0: usize) -> usize {
    if doc.line_to_block.is_empty() {
        return 0;
    }
    if line0 >= doc.line_to_block.len() {
        line0 = doc.line_to_block.len() - 1;
    }
    let b = doc.line_to_block[line0];
    if b >= 0 {
        return b as usize;
    }
    for i in (0..=line0).rev() {
        if doc.line_to_block[i] >= 0 {
            return doc.line_to_block[i] as usize;
        }
    }
    for i in line0..doc.line_to_block.len() {
        if doc.line_to_block[i] >= 0 {
            return doc.line_to_block[i] as usize;
        }
    }
    0
}

/// GFM 近似 slug：空白变 `-`，去掉 ASCII 标点，保留中文。
pub fn heading_slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_whitespace() {
            if !out.ends_with('-') {
                out.push('-');
            }
        } else if c == '*' || c == '`' || c == '_' {
            continue;
        } else if c.is_ascii_alphanumeric() || c == '-' || !c.is_ascii() {
            for x in c.to_lowercase() {
                out.push(x);
            }
        }
    }
    out.trim_matches('-').to_string()
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(hex) = std::str::from_utf8(&b[i + 1..i + 3]) {
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `#锚点` → 标题源行（0-based）。
pub fn heading_line_for_anchor(doc: &MdDoc, frag: &str) -> Option<usize> {
    let frag = percent_decode(frag.trim());
    if frag.is_empty() {
        return Some(0);
    }
    let want = heading_slug(&frag);
    for b in &doc.blocks {
        if b.kind != MdBlockKind::Heading {
            continue;
        }
        if heading_slug(&b.text) == want {
            return Some(b.line0);
        }
        if b.text.trim() == frag {
            return Some(b.line0);
        }
    }
    None
}

fn is_details_tag(t: &str) -> bool {
    let lower = t.to_ascii_lowercase();
    if !lower.starts_with("<details") {
        return false;
    }
    if t.len() == 8 {
        return true;
    }
    let c = t.as_bytes().get(8).copied().unwrap_or(0);
    c.is_ascii_whitespace() || c == b'>'
}

fn is_img_tag(t: &str) -> bool {
    let lower = t.to_ascii_lowercase();
    if !lower.starts_with("<img") {
        return false;
    }
    if t.len() == 4 {
        return true;
    }
    let c = t.as_bytes().get(4).copied().unwrap_or(0);
    c.is_ascii_whitespace() || c == b'/' || c == b'>'
}

fn try_html_img_block(line: &str) -> Option<MdBlock> {
    let m = RE_IMG_TAG.captures(line)?;
    let attrs = parse_html_attrs(m.get(1).map(|x| x.as_str()).unwrap_or(""));
    let src = attrs.get("src")?.trim();
    if src.is_empty() {
        return None;
    }
    let alt = attrs.get("alt").cloned().unwrap_or_default();
    let mut w = attrs.get("width").and_then(|v| parse_css_px(v));
    let mut h = attrs.get("height").and_then(|v| parse_css_px(v));
    if let Some(style) = attrs.get("style") {
        let st = parse_css_style(style);
        if let Some(sw) = st.get("width").and_then(|v| parse_css_px(v)) {
            w = Some(sw);
        }
        if let Some(sh) = st.get("height").and_then(|v| parse_css_px(v)) {
            h = Some(sh);
        }
    }
    let mut b = MdBlock::new(MdBlockKind::HtmlImg, 0, 0);
    b.text = src.to_string();
    b.spans = vec![MdSpan {
        kind: MdSpanKind::Image,
        text: alt,
        href: src.to_string(),
    }];
    b.img_w = w;
    b.img_h = h;
    Some(b)
}

fn parse_html_attrs(tag: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    for cap in RE_ATTR_DQ.captures_iter(tag) {
        attrs.insert(
            cap.get(1).unwrap().as_str().to_ascii_lowercase(),
            cap.get(2).unwrap().as_str().to_string(),
        );
    }
    for cap in RE_ATTR_SQ.captures_iter(tag) {
        let k = cap.get(1).unwrap().as_str().to_ascii_lowercase();
        attrs
            .entry(k)
            .or_insert_with(|| cap.get(2).unwrap().as_str().to_string());
    }
    for cap in RE_ATTR_NQ.captures_iter(tag) {
        let k = cap.get(1).unwrap().as_str().to_ascii_lowercase();
        attrs
            .entry(k)
            .or_insert_with(|| cap.get(2).unwrap().as_str().to_string());
    }
    if RE_OPEN.is_match(tag) && !attrs.contains_key("open") {
        attrs.insert("open".into(), "open".into());
    }
    attrs
}

fn parse_css_style(style: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for part in style.split(';') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        if let Some(colon) = p.find(':') {
            let k = p[..colon].trim().to_ascii_lowercase();
            let v = p[colon + 1..].trim().to_string();
            if !k.is_empty() {
                out.insert(k, v);
            }
        }
    }
    out
}

fn parse_css_px(v: &str) -> Option<f32> {
    let mut v = v.trim();
    if v.to_ascii_lowercase().ends_with("px") {
        v = v[..v.len() - 2].trim();
    }
    v.parse::<f32>().ok().filter(|n| *n > 0.0 && *n < 20000.0)
}

fn try_details_block(lines: &[String], start: usize, tab_size: i32, depth: i32) -> Option<MdBlock> {
    if start >= lines.len() || depth >= MAX_DETAILS_DEPTH {
        return None;
    }
    let first = &lines[start];
    if !is_details_tag(first.trim_start()) {
        return None;
    }
    let open_angle = first.find('<').unwrap_or(0);
    let close_angle = first[open_angle..].find('>').map(|x| x + open_angle);
    let mut attr_str = "";
    if let Some(close) = close_angle {
        if close > open_angle {
            let tag_inner = &first[open_angle + 1..close];
            if tag_inner.len() >= 7 {
                attr_str = &tag_inner[7..];
            }
        }
    }
    let attrs = parse_html_attrs(attr_str);
    let is_open = attrs.contains_key("open");

    let mut nest = 0;
    let mut end_idx = None;
    let mut buf = String::new();
    for i in start..lines.len() {
        let line = &lines[i];
        let lower = line.to_ascii_lowercase();
        let mut pos = 0;
        while pos < lower.len() {
            let a = lower[pos..].find("<details").map(|x| x + pos);
            let b = lower[pos..].find("</details>").map(|x| x + pos);
            match (a, b) {
                (None, None) => break,
                (Some(a), b) if b.map(|bb| a < bb).unwrap_or(true) => {
                    nest += 1;
                    pos = a + 8;
                }
                (_, Some(b)) => {
                    nest -= 1;
                    pos = b + 10;
                    if nest == 0 {
                        end_idx = Some(i);
                        break;
                    }
                }
                _ => break,
            }
        }
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(line);
        if end_idx.is_some() {
            break;
        }
    }
    let end_idx = end_idx?;
    let mut inner = RE_DETAILS_OPEN.replace(&buf, "").into_owned();
    inner = RE_DETAILS_CLOSE.replace(&inner, "").into_owned();
    let mut summary = "Details".to_string();
    if let Some(sm) = RE_SUMMARY.captures(&inner) {
        summary = sm
            .get(1)
            .unwrap()
            .as_str()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if summary.is_empty() {
            summary = "Details".into();
        }
        let whole = sm.get(0).unwrap();
        inner.replace_range(whole.start()..whole.end(), "");
    }
    inner = inner.trim_matches(['\r', '\n']).to_string();
    let mut body_off = start;
    for i in start..=end_idx {
        if lines[i].to_ascii_lowercase().contains("</summary>") {
            body_off = i + 1;
            break;
        }
    }
    let mut children = if inner.trim().is_empty() {
        Vec::new()
    } else {
        parse_with_tab(&inner, tab_size).blocks
    };
    shift_block_lines(&mut children, body_off);
    let mut b = MdBlock::new(MdBlockKind::Details, start, end_idx);
    b.summary = summary.clone();
    b.details_open = is_open;
    b.children = children;
    b.spans = parse_inlines(&summary);
    Some(b)
}

fn shift_block_lines(blocks: &mut [MdBlock], delta: usize) {
    if delta == 0 {
        return;
    }
    for b in blocks {
        b.line0 += delta;
        b.line1 += delta;
        shift_block_lines(&mut b.children, delta);
    }
}

/// 命令行 `--selftest`：解析器与表格算法对照 docview 用例。
pub fn selftest() -> i32 {
    let mut fail = 0;
    let mut check = |name: &str, ok: bool, detail: &str| {
        if ok {
            println!("[PASS] {name}");
        } else {
            fail += 1;
            println!("[FAIL] {name} · {detail}");
        }
    };

    let spans = parse_inlines(
        "hello **bold** and *it* with `code` and [link](https://x.com) ==mark== ~~del~~",
    );
    let has = |k: MdSpanKind| spans.iter().any(|s| s.kind == k);
    check("inline.bold", has(MdSpanKind::Bold), "no bold span");
    check("inline.italic", has(MdSpanKind::Italic), "no italic span");
    check("inline.code", has(MdSpanKind::Code), "no code span");
    check("inline.link", has(MdSpanKind::Link), "no link span");
    check("inline.mark", has(MdSpanKind::Mark), "no mark span");
    check("inline.strike", has(MdSpanKind::Strike), "no strike span");
    let bi = parse_inlines("***both***");
    check(
        "inline.bolditalic",
        bi.iter().any(|s| s.kind == MdSpanKind::BoldItalic),
        "no *** span",
    );
    let font = parse_inlines("<font style=\"font-weight:bold\">红粗</font>");
    check(
        "inline.font.bold",
        font.iter().any(|s| s.kind == MdSpanKind::Bold),
        "font bold",
    );
    let href = spans
        .iter()
        .find(|s| s.kind == MdSpanKind::Link)
        .map(|s| s.href.as_str())
        .unwrap_or("");
    check("inline.link.href", href == "https://x.com", href);
    let nested = parse_inlines("**[mdview](mdview/)**");
    let nested_link = nested.iter().find(|s| s.kind == MdSpanKind::Link);
    check(
        "inline.bold.link",
        nested_link.map(|s| s.text.as_str()) == Some("mdview"),
        &format!("{:?}", nested.iter().map(|s| (s.kind, s.text.as_str())).collect::<Vec<_>>()),
    );
    check(
        "inline.bold.link.href",
        nested_link.map(|s| s.href.as_str()) == Some("mdview/"),
        nested_link.map(|s| s.href.as_str()).unwrap_or(""),
    );
    let labeled = parse_inlines("[**mdview**](mdview/)");
    check(
        "inline.link.boldlabel",
        labeled
            .iter()
            .any(|s| s.kind == MdSpanKind::Link && s.text == "mdview" && s.href == "mdview/"),
        &format!("{:?}", labeled.iter().map(|s| (s.kind, s.text.as_str())).collect::<Vec<_>>()),
    );

    check(
        "indent.tab3",
        indent_cols("\t", 3) == 3,
        &indent_cols("\t", 3).to_string(),
    );
    check(
        "indent.tab3x2",
        indent_cols("\t\t", 3) == 6,
        &indent_cols("\t\t", 3).to_string(),
    );
    check(
        "indent.spaces",
        indent_cols("  ", 3) == 2,
        &indent_cols("  ", 3).to_string(),
    );

    let list_doc = parse_with_tab("- a\n\t- b\n", 3);
    let levels: Vec<u32> = list_doc
        .blocks
        .iter()
        .filter(|b| b.kind == MdBlockKind::ListItem)
        .map(|b| b.level)
        .collect();
    check(
        "indent.list.level",
        levels.len() >= 2 && levels[0] == 0 && levels[1] == 3,
        &format!("{levels:?}"),
    );
    let bullet_doc = parse_with_tab("- a\n\t● b\n\t\t○ c\n", 3);
    let bl: Vec<u32> = bullet_doc
        .blocks
        .iter()
        .filter(|b| b.kind == MdBlockKind::ListItem)
        .map(|b| b.level)
        .collect();
    check(
        "indent.list.unicode",
        bl.len() >= 3 && bl[0] == 0 && bl[1] == 3 && bl[2] == 6,
        &format!("{bl:?}"),
    );
    let expanded = expand_tabs("a\tb", 3);
    check("expand.tabs", expanded == "a  b", &expanded);
    let fence_keep = expand_tabs("x\ty\n```\n\tz\n```\n", 3);
    check(
        "expand.tabs.fence",
        fence_keep.contains('\t'),
        "fence tab lost",
    );
    let retarget = retarget_leading_indent("   a\n      b\n", 3, 6);
    check(
        "retarget.indent",
        retarget == "      a\n            b\n",
        &retarget.replace('\n', "\\n"),
    );

    let br = parse_inlines("一行\n二行");
    check(
        "inline.softbr",
        br.iter().any(|s| s.kind == MdSpanKind::SoftBr),
        "no softbr",
    );
    let pdoc = parse("alpha\nbeta\n\ngamma");
    let para0 = pdoc
        .blocks
        .iter()
        .find(|b| b.kind == MdBlockKind::Paragraph);
    check(
        "block.para.softbr",
        para0
            .map(|p| p.spans.iter().any(|s| s.kind == MdSpanKind::SoftBr))
            .unwrap_or(false),
        "newline collapsed",
    );

    let md = "# Title\n\nPara **x**\n\n- item1\n- item2\n\n```cs\nvar a=1;\n```\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n> quote\n\n---\n";
    let doc = parse(md);
    let has_kind = |k: MdBlockKind| doc.blocks.iter().any(|b| b.kind == k);
    check("block.heading", has_kind(MdBlockKind::Heading), "missing");
    check("block.para", has_kind(MdBlockKind::Paragraph), "missing");
    check("block.list", has_kind(MdBlockKind::ListItem), "missing");
    check("block.code", has_kind(MdBlockKind::Code), "missing");
    check("block.table", has_kind(MdBlockKind::Table), "missing");
    check("block.quote", has_kind(MdBlockKind::Quote), "missing");
    check("block.hr", has_kind(MdBlockKind::Hr), "missing");
    let lang = doc
        .blocks
        .iter()
        .find(|b| b.kind == MdBlockKind::Code)
        .map(|b| b.lang.as_str())
        .unwrap_or("");
    check("block.code.lang", lang == "cs", lang);
    let bi = block_index_for_line(&doc, 0);
    check(
        "block.lineMap",
        doc.blocks.get(bi).map(|b| b.kind) == Some(MdBlockKind::Heading),
        "line0 not heading",
    );

    let task_doc = parse("- [ ] open\n- [x] done\n- plain\n");
    let tasks: Vec<_> = task_doc
        .blocks
        .iter()
        .filter(|b| b.kind == MdBlockKind::ListItem)
        .collect();
    check(
        "task.open",
        tasks
            .first()
            .map(|t| t.task == Some(false) && t.text == "open")
            .unwrap_or(false),
        "open",
    );
    check(
        "task.done",
        tasks
            .get(1)
            .map(|t| t.task == Some(true) && t.text == "done")
            .unwrap_or(false),
        "done",
    );
    check(
        "task.plain",
        tasks.get(2).map(|t| t.task.is_none()).unwrap_or(false),
        "plain",
    );

    let idoc = parse("<img src=\"a.png\" style=\"width:120px;height:80px;\" />\n");
    let img = idoc.blocks.first();
    check(
        "html.img.size",
        img.map(|b| {
            b.kind == MdBlockKind::HtmlImg && b.img_w == Some(120.0) && b.img_h == Some(80.0)
        })
        .unwrap_or(false),
        "size",
    );
    let ddoc = parse("<details>\n<summary>S</summary>\n\nhello **x**\n\n</details>\n");
    let det = ddoc.blocks.iter().find(|b| b.kind == MdBlockKind::Details);
    check(
        "html.details",
        det.map(|d| d.summary == "S" && !d.children.is_empty())
            .unwrap_or(false),
        "details",
    );

    check("table.strw.ascii", table::str_display_width("ab") == 2, "");
    check("table.strw.cjk", table::str_display_width("中") == 2, "");
    let got_small = table::allocate(&[4, 6, 3], 100);
    check(
        "table.alloc.fit",
        got_small == [4, 6, 3],
        &format!("{got_small:?}"),
    );
    let got_big = table::allocate(&[4, 40, 6], 30);
    let sum_big: i32 = got_big.iter().sum();
    check(
        "table.alloc.tight",
        sum_big == 30 && got_big[0] <= 5,
        &format!("{got_big:?} sum={sum_big}"),
    );
    let rows = vec![
        vec!["a".into(), "hello".into(), "x".into()],
        vec!["bb".into(), "hi".into(), "中文".into()],
    ];
    let needs = table::content_needs(&rows, 3, 40);
    check(
        "table.contentNeeds",
        needs[0] >= 2 && needs[2] >= 4,
        &format!("{needs:?}"),
    );
    let img_need = table::cell_content_need("![alt](a.png)", 40, 4);
    check("table.need.img", img_need >= 10, &img_need.to_string());
    let path_rows = vec![
        vec!["名称".into(), "文件".into()],
        vec!["公司".into(), r"D:\VS_Projects\我的文件\公司账号.md".into()],
    ];
    let path_need = table::content_needs(&path_rows, 2, 120);
    check(
        "table.need.path",
        path_need[1] > path_need[0] + 10,
        &format!("{path_need:?}"),
    );
    let col_dip = table::allocate_columns_dip(&path_rows, 2, 900.0);
    check(
        "table.allocCols.ratio",
        col_dip.len() == 2 && col_dip[1] > col_dip[0],
        &format!("{col_dip:?}"),
    );
    let filled = table::allocate_fill_need_dip(&[40.0, 2000.0], 844.0);
    let fill_sum: f64 = filled.iter().sum();
    check(
        "table.allocCols.fill",
        filled.len() == 2 && filled[0] <= 45.0 && (fill_sum - 844.0).abs() <= 8.0,
        &format!("{filled:?} sum={fill_sum:.0}"),
    );
    let plug_rows = vec![
        vec!["Plugin".into(), "Overview".into(), "Docs".into(), String::new()],
        vec![
            "**[mdview](mdview/)**".into(),
            "Markdown preview: single-window or side-by-side with lots of overview text here."
                .into(),
            "[EN](mdview/README.md) · [中文](mdview/README.zh.md)".into(),
            String::new(),
        ],
        vec![
            "**[colorpicker](colorpicker/)**".into(),
            "HSV color picker with more overview text so the middle column is long.".into(),
            "[EN](a.md) · [中文](b.md)".into(),
            String::new(),
        ],
    ];
    let plug_w = table::allocate_columns_dip(&plug_rows, 4, 480.0);
    check(
        "table.allocCols.shortPin",
        plug_w.len() == 4 && plug_w[0] >= 90.0 && plug_w[2] >= 80.0 && plug_w[0] < plug_w[1],
        &format!("{plug_w:?}"),
    );
    let nw = table::short_nowrap_columns(&plug_rows, 4, 24);
    check(
        "table.nowrap.plugin",
        nw.len() == 4 && nw[0] && !nw[1] && !nw[2],
        &format!("{nw:?}"),
    );

    if fail == 0 {
        println!("SELFTEST MD OK");
    } else {
        println!("SELFTEST MD FAILED count={fail}");
    }
    fail
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selftest_ok() {
        assert_eq!(selftest(), 0);
    }

    #[test]
    fn heading_anchor_slug() {
        let doc = parse("# Hello World\n\n## 中文标题\n");
        assert_eq!(heading_line_for_anchor(&doc, "hello-world"), Some(0));
        assert_eq!(heading_line_for_anchor(&doc, "Hello World"), Some(0));
        assert_eq!(heading_line_for_anchor(&doc, "中文标题"), Some(2));
        assert_eq!(heading_line_for_anchor(&doc, "missing"), None);
        assert_eq!(
            heading_line_for_anchor(&doc, "%E4%B8%AD%E6%96%87%E6%A0%87%E9%A2%98"),
            Some(2)
        );
    }
}
