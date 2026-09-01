//! DOC / DOCX → 原生排版模型（对齐 docview DocxViewer，不经 Markdown）。

use std::io::{Cursor, Read};
use std::path::Path;

use egui::{Align, Color32};
use office_oxide::ir::{
    DocumentIR, Element, Heading, Image as IrImage, InlineContent, List, ListStyle, PageSetup,
    Paragraph, ParagraphAlignment, Table as IrTable, TextSpan, UnderlineStyle,
};
use office_oxide::{Document, DocumentFormat};

use crate::io::file;

const PARSE_STACK: usize = 16 * 1024 * 1024;
/// twip → DIP（96 DPI），与 DocxViewer `TWIP2DIP` 相同。
pub const TWIP2DIP: f32 = 96.0 / 1440.0;
const EMU2DIP: f32 = 96.0 / 914_400.0;
/// A4 竖向（twip），DocxViewer 默认页。
const DEF_PAGE_W_TWIP: f32 = 11906.0;
const DEF_PAGE_H_TWIP: f32 = 16838.0;
const DEF_MARGIN_TWIP: f32 = 1440.0;
const BODY_PT: f32 = 10.5;

pub fn pt2dip(pt: f32) -> f32 {
    pt * 96.0 / 72.0
}

fn twip2dip(t: f32) -> f32 {
    t * TWIP2DIP
}

/// 打开后的 Word 文档（段落 / 表格 / 图 / 分页信息）。
#[derive(Clone)]
pub struct WordDoc {
    pub page_w: f32,
    pub page_h: f32,
    pub pad_l: f32,
    pub pad_t: f32,
    pub pad_r: f32,
    pub pad_b: f32,
    pub blocks: Vec<WordBlock>,
    pub images: Vec<WordImage>,
    pub toc: Vec<WordToc>,
    /// 每块一行，供 Ctrl+F 定位到块下标。
    pub plain: String,
    /// 另存为 Markdown 用。
    pub md_export: String,
}

#[derive(Clone, Debug)]
pub struct WordToc {
    pub title: String,
    pub level: u32,
    pub block_idx: usize,
}

#[derive(Clone)]
pub struct WordImage {
    pub bytes: Vec<u8>,
    pub w_dip: f32,
    pub h_dip: f32,
    pub alt: String,
}

#[derive(Clone)]
pub enum WordBlock {
    Para(WordPara),
    Table(WordTable),
    Image { id: usize, center: bool },
    Hr,
    PageBreak,
}

#[derive(Clone)]
pub struct WordPara {
    pub runs: Vec<WordRun>,
    pub align: Align,
    pub indent: f32,
    pub hanging: f32,
    pub space_before: f32,
    pub space_after: f32,
    pub heading: u8,
    pub marker: Option<String>,
}

#[derive(Clone)]
pub struct WordRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub underline: bool,
    pub size: f32,
    pub color: Color32,
    pub href: Option<String>,
}

#[derive(Clone)]
pub struct WordTable {
    pub rows: Vec<WordRow>,
}

#[derive(Clone)]
pub struct WordRow {
    pub cells: Vec<WordCell>,
    pub header: bool,
}

#[derive(Clone)]
pub struct WordCell {
    pub blocks: Vec<WordBlock>,
    pub col_span: u32,
}

pub fn load(path: &Path) -> Result<WordDoc, String> {
    let path = path.to_path_buf();
    std::thread::Builder::new()
        .name("word-parse".into())
        .stack_size(PARSE_STACK)
        .spawn(move || load_inner(&path))
        .map_err(|e| crate::i18n::word_parse_start(e))?
        .join()
        .unwrap_or_else(|_| Err(crate::i18n::t().word_parse_crash.into()))
}

fn load_inner(path: &Path) -> Result<WordDoc, String> {
    let fmt = DocumentFormat::from_path(path).ok_or_else(|| {
        crate::i18n::word_format(
            &file::ext_lower(path).unwrap_or_else(|| crate::i18n::t().no_ext.into()),
        )
    })?;
    let bytes = read_shared(path)?;
    if bytes.is_empty() {
        return Err(crate::i18n::t().word_empty.into());
    }
    let cursor = Cursor::new(bytes);
    let doc = Document::from_reader(cursor, fmt).map_err(|e| crate::i18n::word_open(e))?;
    let ir = doc.to_ir();
    Ok(from_ir(&ir))
}

fn read_shared(path: &Path) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x7)
            .open(path)
            .map_err(|e| crate::i18n::word_read(path.display(), e))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| crate::i18n::word_read_fail(e))?;
        Ok(buf)
    }
    #[cfg(not(windows))]
    {
        std::fs::read(path).map_err(|e| crate::i18n::word_read(path.display(), e))
    }
}

pub fn from_ir(ir: &DocumentIR) -> WordDoc {
    let (page_w, page_h, pad_l, pad_t, pad_r, pad_b) = page_metrics(ir);
    let mut images = Vec::new();
    let mut blocks = Vec::new();
    for (si, sec) in ir.sections.iter().enumerate() {
        if si > 0 {
            blocks.push(WordBlock::PageBreak);
        }
        convert_elements(&sec.elements, &mut blocks, &mut images, 0.0);
    }
    if blocks.is_empty() {
        blocks.push(WordBlock::Para(empty_para()));
    }
    let toc = collect_toc(&blocks);
    let plain = blocks
        .iter()
        .map(|b| block_plain(b, &images))
        .collect::<Vec<_>>()
        .join("\n");
    let md_export = ir.to_markdown();
    WordDoc {
        page_w,
        page_h,
        pad_l,
        pad_t,
        pad_r,
        pad_b,
        blocks,
        images,
        toc,
        plain,
        md_export,
    }
}

fn page_metrics(ir: &DocumentIR) -> (f32, f32, f32, f32, f32, f32) {
    let ps = ir
        .sections
        .iter()
        .find_map(|s| s.page_setup.as_ref())
        .cloned()
        .unwrap_or(PageSetup {
            width_twips: DEF_PAGE_W_TWIP as u32,
            height_twips: DEF_PAGE_H_TWIP as u32,
            margin_top_twips: DEF_MARGIN_TWIP as u32,
            margin_bottom_twips: DEF_MARGIN_TWIP as u32,
            margin_left_twips: DEF_MARGIN_TWIP as u32,
            margin_right_twips: DEF_MARGIN_TWIP as u32,
            ..PageSetup::default()
        });
    let mut w = twip2dip(ps.width_twips as f32);
    let mut h = twip2dip(ps.height_twips as f32);
    if ps.landscape && w < h {
        std::mem::swap(&mut w, &mut h);
    }
    let pad_l = twip2dip(ps.margin_left_twips as f32).max(24.0);
    let pad_t = twip2dip(ps.margin_top_twips as f32).max(24.0);
    let pad_r = twip2dip(ps.margin_right_twips as f32).max(24.0);
    let pad_b = twip2dip(ps.margin_bottom_twips as f32).max(24.0);
    (
        w.max(120.0),
        h.max(160.0),
        pad_l,
        pad_t,
        pad_r,
        pad_b,
    )
}

fn convert_elements(
    els: &[Element],
    out: &mut Vec<WordBlock>,
    images: &mut Vec<WordImage>,
    indent: f32,
) {
    for el in els {
        match el {
            Element::Heading(h) => out.push(WordBlock::Para(convert_heading(h))),
            Element::Paragraph(p) => {
                let para = convert_para(p, None, indent, 0.0, 0);
                if !para_empty(&para) || p.page_break_before {
                    if p.page_break_before {
                        out.push(WordBlock::PageBreak);
                    }
                    out.push(WordBlock::Para(para));
                }
            }
            Element::List(list) => convert_list(list, 0, &mut [0i32; 9], out, images),
            Element::Table(t) => out.push(WordBlock::Table(convert_table(t, images))),
            Element::Image(img) => {
                if let Some(id) = push_image(img, images) {
                    out.push(WordBlock::Image { id, center: true });
                }
            }
            Element::ThematicBreak => out.push(WordBlock::Hr),
            Element::PageBreak => out.push(WordBlock::PageBreak),
            Element::ColumnBreak => out.push(WordBlock::PageBreak),
            Element::TextBox(tb) => convert_elements(&tb.content, out, images, indent),
            Element::CodeBlock(cb) => {
                let size = pt2dip(BODY_PT);
                out.push(WordBlock::Para(WordPara {
                    runs: vec![WordRun {
                        text: cb.content.clone(),
                        bold: false,
                        italic: false,
                        strike: false,
                        underline: false,
                        size,
                        color: Color32::from_rgb(0x1F, 0x29, 0x37),
                        href: None,
                    }],
                    align: Align::LEFT,
                    indent,
                    hanging: 0.0,
                    space_before: 6.0,
                    space_after: 6.0,
                    heading: 0,
                    marker: None,
                }));
            }
            Element::Footnote(_) | Element::Endnote(_) | Element::Shape(_) => {}
            _ => {}
        }
    }
}

fn convert_heading(h: &Heading) -> WordPara {
    let lv = h.level.clamp(1, 6);
    let def_pt = match lv {
        1 => 18.0,
        2 => 14.0,
        3 => 12.0,
        _ => 11.0,
    };
    let def_size = pt2dip(def_pt);
    let runs = convert_inlines(&h.content, def_size, true);
    let space_before = match lv {
        1 => 14.0,
        2 => 12.0,
        _ => 10.0,
    };
    let space_after = match lv {
        1 => 8.0,
        2 => 6.0,
        _ => 4.0,
    };
    WordPara {
        runs,
        align: map_align(h.alignment.as_ref()),
        indent: 0.0,
        hanging: 0.0,
        space_before,
        space_after,
        heading: lv,
        marker: None,
    }
}

fn convert_para(
    p: &Paragraph,
    marker: Option<String>,
    indent: f32,
    hanging: f32,
    heading: u8,
) -> WordPara {
    let def_size = pt2dip(BODY_PT);
    let mut runs = convert_inlines(&p.content, def_size, heading > 0);
    if runs.is_empty() {
        runs.push(WordRun {
            text: "\u{00A0}".into(),
            bold: heading > 0,
            italic: false,
            strike: false,
            underline: false,
            size: def_size,
            color: Color32::from_rgb(0x1F, 0x29, 0x37),
            href: None,
        });
    }
    let mut m_l = p
        .indent_left_twips
        .map(|v| twip2dip(v as f32).max(0.0))
        .unwrap_or(indent);
    let mut hang = hanging;
    if hang < 0.1 {
        if let Some(first) = p.first_line_indent_twips {
            if first < 0 {
                hang = twip2dip((-first) as f32);
            }
        }
    }
    if marker.is_some() {
        if m_l < 8.0 {
            m_l = indent.max(hang.max(18.0));
        }
        if hang < 12.0 {
            hang = 14.0;
        }
    }
    let mut before = p
        .space_before_twips
        .map(|v| twip2dip(v as f32))
        .unwrap_or(0.0);
    let mut after = p
        .space_after_twips
        .map(|v| twip2dip(v as f32))
        .unwrap_or(0.0);
    if marker.is_some() && after < 2.0 {
        after = 3.0;
    }
    if para_is_empty_inlines(&p.content) && marker.is_none() {
        before = before.max(0.0);
        after = after.max(0.0);
    }
    WordPara {
        runs,
        align: map_align(p.alignment.as_ref()),
        indent: m_l,
        hanging: hang,
        space_before: before,
        space_after: after,
        heading,
        marker,
    }
}

fn convert_inlines(content: &[InlineContent], def_size: f32, heading_bold: bool) -> Vec<WordRun> {
    let mut runs = Vec::new();
    for ic in content {
        match ic {
            InlineContent::Text(sp) => {
                if sp.text.is_empty() {
                    continue;
                }
                runs.push(span_to_run(sp, def_size, heading_bold));
            }
            InlineContent::LineBreak => {
                if let Some(last) = runs.last_mut() {
                    last.text.push('\n');
                } else {
                    runs.push(WordRun {
                        text: "\n".into(),
                        bold: heading_bold,
                        italic: false,
                        strike: false,
                        underline: false,
                        size: def_size,
                        color: Color32::from_rgb(0x1F, 0x29, 0x37),
                        href: None,
                    });
                }
            }
            InlineContent::FootnoteRef(r) | InlineContent::EndnoteRef(r) => {
                let mark = r.marker.clone().unwrap_or_else(|| r.note_id.to_string());
                runs.push(WordRun {
                    text: mark,
                    bold: false,
                    italic: false,
                    strike: false,
                    underline: false,
                    size: def_size * 0.75,
                    color: Color32::from_rgb(0x4B, 0x55, 0x63),
                    href: None,
                });
            }
            _ => {}
        }
    }
    runs
}

fn span_to_run(sp: &TextSpan, def_size: f32, heading_bold: bool) -> WordRun {
    let size = sp
        .font_size_half_pt
        .map(|hp| pt2dip(hp as f32 / 2.0))
        .filter(|s| *s > 1.0)
        .unwrap_or(def_size);
    let color = sp
        .color
        .map(|c| Color32::from_rgb(c[0], c[1], c[2]))
        .unwrap_or(Color32::from_rgb(0x1F, 0x29, 0x37));
    let underline = match sp.underline.as_ref() {
        Some(UnderlineStyle::None) | None => false,
        Some(_) => true,
    };
    WordRun {
        text: sp.text.clone(),
        bold: sp.bold || heading_bold,
        italic: sp.italic,
        strike: sp.strikethrough,
        underline,
        size,
        color,
        href: sp.hyperlink.clone(),
    }
}

fn convert_list(
    list: &List,
    level: u8,
    counters: &mut [i32; 9],
    out: &mut Vec<WordBlock>,
    images: &mut Vec<WordImage>,
) {
    for item in &list.items {
        let marker = take_marker(list.ordered, list.style.as_ref(), level, counters);
        let indent = ((level as f32) + 1.0) * 21.0;
        let mut first_para = true;
        for el in &item.content {
            match el {
                Element::Paragraph(p) => {
                    let m = if first_para {
                        first_para = false;
                        Some(marker.clone())
                    } else {
                        None
                    };
                    out.push(WordBlock::Para(convert_para(p, m, indent, 18.0, 0)));
                }
                Element::Heading(h) => {
                    let mut para = convert_heading(h);
                    if first_para {
                        para.marker = Some(marker.clone());
                        para.indent = indent;
                        para.hanging = 18.0;
                        first_para = false;
                    }
                    out.push(WordBlock::Para(para));
                }
                other => {
                    convert_elements(std::slice::from_ref(other), out, images, indent);
                }
            }
        }
        if first_para {
            let mut para = empty_para();
            para.marker = Some(marker);
            para.indent = indent;
            para.hanging = 18.0;
            out.push(WordBlock::Para(para));
        }
        if let Some(nested) = &item.nested {
            convert_list(nested, level.saturating_add(1), counters, out, images);
        }
    }
}

fn take_marker(ordered: bool, style: Option<&ListStyle>, level: u8, counters: &mut [i32; 9]) -> String {
    let i = (level as usize).min(8);
    counters[i] += 1;
    for c in counters.iter_mut().skip(i + 1) {
        *c = 0;
    }
    if !ordered {
        return "• ".into();
    }
    let n = counters[i].max(1) as u32;
    let kind = style.unwrap_or(&ListStyle::Decimal);
    let body = match kind {
        ListStyle::Bullet | ListStyle::Dash | ListStyle::Square | ListStyle::Circle => {
            return "• ".into();
        }
        ListStyle::LowerAlpha => to_letters(n, false),
        ListStyle::UpperAlpha => to_letters(n, true),
        ListStyle::LowerRoman => to_roman(n, false),
        ListStyle::UpperRoman => to_roman(n, true),
        ListStyle::Decimal => n.to_string(),
    };
    format!("{body}. ")
}

fn to_letters(mut n: u32, upper: bool) -> String {
    if n == 0 {
        n = 1;
    }
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        let ch = if upper { b'A' } else { b'a' } + (n % 26) as u8;
        s.insert(0, ch as char);
        n /= 26;
    }
    s
}

fn to_roman(mut n: u32, upper: bool) -> String {
    if n == 0 {
        return if upper { "I".into() } else { "i".into() };
    }
    if n > 3999 {
        return n.to_string();
    }
    let map = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for &(v, g) in &map {
        while n >= v {
            s.push_str(g);
            n -= v;
        }
    }
    if upper {
        s
    } else {
        s.to_lowercase()
    }
}

fn convert_table(t: &IrTable, images: &mut Vec<WordImage>) -> WordTable {
    let mut rows = Vec::new();
    for row in &t.rows {
        let mut cells = Vec::new();
        for cell in &row.cells {
            let mut blocks = Vec::new();
            convert_elements(&cell.content, &mut blocks, images, 0.0);
            if blocks.is_empty() {
                blocks.push(WordBlock::Para(empty_para()));
            }
            cells.push(WordCell {
                blocks,
                col_span: cell.col_span.max(1),
            });
        }
        if cells.is_empty() {
            continue;
        }
        rows.push(WordRow {
            cells,
            header: row.is_header,
        });
    }
    WordTable { rows }
}

fn push_image(img: &IrImage, images: &mut Vec<WordImage>) -> Option<usize> {
    let data = img.data.as_ref()?;
    if data.is_empty() {
        return None;
    }
    let mut w = img
        .display_width_emu
        .map(|e| e as f32 * EMU2DIP)
        .unwrap_or(0.0);
    let mut h = img
        .display_height_emu
        .map(|e| e as f32 * EMU2DIP)
        .unwrap_or(0.0);
    if w < 8.0 || h < 8.0 {
        w = w.max(200.0);
        h = h.max(150.0);
    }
    let id = images.len();
    images.push(WordImage {
        bytes: data.clone(),
        w_dip: w,
        h_dip: h,
        alt: img.alt_text.clone().unwrap_or_default(),
    });
    Some(id)
}

fn map_align(a: Option<&ParagraphAlignment>) -> Align {
    match a {
        Some(ParagraphAlignment::Center) => Align::Center,
        Some(ParagraphAlignment::Right) => Align::RIGHT,
        _ => Align::LEFT,
    }
}

fn empty_para() -> WordPara {
    WordPara {
        runs: vec![WordRun {
            text: "\u{00A0}".into(),
            bold: false,
            italic: false,
            strike: false,
            underline: false,
            size: pt2dip(BODY_PT),
            color: Color32::from_rgb(0x1F, 0x29, 0x37),
            href: None,
        }],
        align: Align::LEFT,
        indent: 0.0,
        hanging: 0.0,
        space_before: 0.0,
        space_after: 0.0,
        heading: 0,
        marker: None,
    }
}

fn para_empty(p: &WordPara) -> bool {
    p.marker.is_none()
        && p.runs.iter().all(|r| {
            r.text.chars().all(|c| c.is_whitespace() || c == '\u{00A0}')
        })
}

fn para_is_empty_inlines(content: &[InlineContent]) -> bool {
    content.iter().all(|ic| match ic {
        InlineContent::Text(s) => s.text.trim().is_empty(),
        InlineContent::LineBreak => true,
        _ => false,
    })
}

fn collect_toc(blocks: &[WordBlock]) -> Vec<WordToc> {
    let mut out = Vec::new();
    collect_toc_in(blocks, &mut out);
    out
}

fn collect_toc_in(blocks: &[WordBlock], out: &mut Vec<WordToc>) {
    for (i, b) in blocks.iter().enumerate() {
        match b {
            WordBlock::Para(p) if p.heading > 0 => {
                let title = para_text(p).trim().to_string();
                if title.is_empty() {
                    continue;
                }
                let title = if title.chars().count() > 80 {
                    let t: String = title.chars().take(80).collect();
                    format!("{t}…")
                } else {
                    title
                };
                out.push(WordToc {
                    title,
                    level: p.heading as u32,
                    block_idx: i,
                });
            }
            WordBlock::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        collect_toc_in(&cell.blocks, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn para_text(p: &WordPara) -> String {
    let mut s = String::new();
    if let Some(m) = &p.marker {
        s.push_str(m);
    }
    for r in &p.runs {
        s.push_str(&r.text);
    }
    s.replace('\u{00A0}', " ")
}

fn block_plain(b: &WordBlock, images: &[WordImage]) -> String {
    let t = match b {
        WordBlock::Para(p) => para_text(p),
        WordBlock::Table(t) => {
            let mut s = String::new();
            for row in &t.rows {
                for cell in &row.cells {
                    for cb in &cell.blocks {
                        if !s.is_empty() {
                            s.push(' ');
                        }
                        s.push_str(&block_plain(cb, images));
                    }
                }
            }
            s
        }
        WordBlock::Image { id, .. } => images
            .get(*id)
            .map(|im| im.alt.clone())
            .unwrap_or_default(),
        WordBlock::Hr => "---".into(),
        WordBlock::PageBreak => String::new(),
    };
    t.replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use office_oxide::ir::{DocumentIR, Heading, InlineContent, Section, TextSpan};

    #[test]
    fn heading_becomes_toc() {
        let ir = DocumentIR {
            sections: vec![Section {
                elements: vec![Element::Heading(Heading {
                    level: 1,
                    content: vec![InlineContent::Text(TextSpan::plain("概述"))],
                    ..Default::default()
                })],
                ..Default::default()
            }],
            ..Default::default()
        };
        let doc = from_ir(&ir);
        assert_eq!(doc.toc.len(), 1);
        assert_eq!(doc.toc[0].title, "概述");
        assert_eq!(doc.toc[0].level, 1);
        assert!(doc.plain.contains("概述"));
        assert!(!doc.md_export.trim().is_empty());
    }

    #[test]
    fn list_markers() {
        let mut c = [0i32; 9];
        assert_eq!(take_marker(false, None, 0, &mut c), "• ");
        let mut c = [0i32; 9];
        assert_eq!(take_marker(true, Some(&ListStyle::Decimal), 0, &mut c), "1. ");
        assert_eq!(take_marker(true, Some(&ListStyle::Decimal), 0, &mut c), "2. ");
        assert_eq!(to_letters(1, false), "a");
        assert_eq!(to_letters(26, false), "z");
        assert_eq!(to_roman(4, true), "IV");
    }

    #[test]
    fn a4_default_page() {
        let ir = DocumentIR::default();
        let doc = from_ir(&ir);
        assert!((doc.page_w - DEF_PAGE_W_TWIP * TWIP2DIP).abs() < 1.0);
        assert!(doc.pad_l >= 24.0);
        assert_eq!(doc.blocks.len(), 1);
    }
}
