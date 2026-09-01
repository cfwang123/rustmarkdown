//! 大纲侧栏（对齐 docview 章节列表 + mdview TOC：缩进树、筛选、滚动高亮、点击跳转）。

use std::collections::HashSet;

use egui::{
    text::LayoutJob, Align, Color32, CursorIcon, FontId, Frame, Label, Layout, Margin, RichText,
    Sense, TextFormat, Ui, Vec2,
};

use crate::parser::{HeadingNumber, MdBlock, MdBlockKind, MdDoc};

/// 扁平标题项（含父下标，便于折叠时只高亮可见路径）。
#[derive(Clone, Debug)]
pub struct TocEntry {
    pub title: String,
    pub level: u32,
    pub line0: usize,
    pub parent: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutlineAction {
    Jump(usize),
}

pub fn collect(doc: &MdDoc, auto_number: bool) -> Vec<TocEntry> {
    let mut out = Vec::new();
    let mut num = if auto_number {
        Some(HeadingNumber::default())
    } else {
        None
    };
    walk(&doc.blocks, &mut out, &mut num);
    attach_parents(&mut out);
    out
}

pub fn collect_pages(page_count: usize) -> Vec<TocEntry> {
    (0..page_count)
        .map(|i| TocEntry {
            title: crate::i18n::page_n(i + 1),
            level: 1,
            line0: i,
            parent: None,
        })
        .collect()
}

fn walk(blocks: &[MdBlock], out: &mut Vec<TocEntry>, num: &mut Option<HeadingNumber>) {
    for b in blocks {
        match b.kind {
            MdBlockKind::Heading => {
                let raw = b.text.trim();
                let title = if let Some(n) = num.as_mut() {
                    n.prefix_title(b.level as i32, raw)
                } else {
                    raw.to_string()
                };
                out.push(TocEntry {
                    title,
                    level: b.level.clamp(1, 6),
                    line0: b.line0,
                    parent: None,
                });
            }
            MdBlockKind::Details => walk(&b.children, out, num),
            _ => {}
        }
    }
}

fn attach_parents(entries: &mut [TocEntry]) {
    let mut stack: Vec<usize> = Vec::new();
    for i in 0..entries.len() {
        let lv = entries[i].level;
        while let Some(&top) = stack.last() {
            if entries[top].level < lv {
                break;
            }
            stack.pop();
        }
        entries[i].parent = stack.last().copied();
        stack.push(i);
    }
}

fn has_children(entries: &[TocEntry], i: usize) -> bool {
    entries.get(i + 1).is_some_and(|n| n.parent == Some(i))
}

pub fn ensure_expanded(entries: &[TocEntry], expanded: &mut HashSet<usize>, inited: &mut bool) {
    if *inited || entries.is_empty() {
        return;
    }
    *inited = true;
    let min_lv = entries.iter().map(|e| e.level).min().unwrap_or(1);
    for (i, e) in entries.iter().enumerate() {
        if has_children(entries, i) && e.level <= min_lv {
            expanded.insert(e.line0);
        }
    }
}

fn filter_shown(entries: &[TocEntry], query: &str) -> Vec<bool> {
    let q = query.trim();
    if q.is_empty() {
        return vec![true; entries.len()];
    }
    let q = q.to_lowercase();
    let mut shown = vec![false; entries.len()];
    for (i, e) in entries.iter().enumerate() {
        if e.title.to_lowercase().contains(&q) {
            shown[i] = true;
            let mut p = e.parent;
            while let Some(pi) = p {
                shown[pi] = true;
                p = entries[pi].parent;
            }
        }
    }
    shown
}

/// 当前源行对应的理想标题下标（最后一个 line0 ≤ line 的项）。
pub fn ideal_index(entries: &[TocEntry], line: usize) -> Option<usize> {
    let mut best = None;
    for (i, e) in entries.iter().enumerate() {
        if e.line0 <= line {
            best = Some(i);
        }
    }
    best
}

/// 不自动展开：沿祖先路径取已展开的最深可见节点（对齐 docview `FindVisibleOnPath`）。
pub fn visible_on_path(entries: &[TocEntry], expanded: &HashSet<usize>, ideal: usize) -> usize {
    if ideal >= entries.len() {
        return 0;
    }
    let mut path = Vec::new();
    let mut i = Some(ideal);
    while let Some(idx) = i {
        path.push(idx);
        i = entries[idx].parent;
    }
    path.reverse();
    if path.is_empty() {
        return ideal;
    }
    let mut last = path[0];
    for k in 1..path.len() {
        if !expanded.contains(&entries[path[k - 1]].line0) {
            break;
        }
        last = path[k];
    }
    last
}

fn expand_ancestors(entries: &[TocEntry], expanded: &mut HashSet<usize>, i: usize) {
    let mut p = entries.get(i).and_then(|e| e.parent);
    while let Some(pi) = p {
        expanded.insert(entries[pi].line0);
        p = entries[pi].parent;
    }
}

fn row_visible(
    entries: &[TocEntry],
    expanded: &HashSet<usize>,
    shown: &[bool],
    i: usize,
    filtering: bool,
) -> bool {
    if !shown.get(i).copied().unwrap_or(false) {
        return false;
    }
    if filtering {
        return true;
    }
    let mut p = entries[i].parent;
    while let Some(pi) = p {
        if !expanded.contains(&entries[pi].line0) {
            return false;
        }
        p = entries[pi].parent;
    }
    true
}

fn highlight_match(title: &str, query: &str, selected: bool) -> LayoutJob {
    let font = FontId::proportional(12.5);
    let fg = if selected {
        Color32::from_rgb(0x11, 0x18, 0x27)
    } else {
        Color32::from_rgb(0x1F, 0x29, 0x37)
    };
    let q = query.trim();
    let mut job = LayoutJob::default();
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    if q.is_empty() {
        job.append(
            title,
            0.0,
            TextFormat {
                font_id: font,
                color: fg,
                ..Default::default()
            },
        );
        return job;
    }
    let lower = title.to_lowercase();
    let q_l = q.to_lowercase();
    let mut pos = 0usize;
    let mut byte = 0usize;
    let title_bytes = title.as_bytes();
    while pos < title.len() {
        let rest_l = &lower[pos..];
        if let Some(rel) = rest_l.find(&q_l) {
            let i = pos + rel;
            if i > pos {
                job.append(
                    std::str::from_utf8(&title_bytes[byte..byte + (i - pos)]).unwrap_or(""),
                    0.0,
                    TextFormat {
                        font_id: font.clone(),
                        color: fg,
                        ..Default::default()
                    },
                );
                byte += i - pos;
            }
            let hit_len = q.len();
            let hit = std::str::from_utf8(&title_bytes[byte..byte + hit_len]).unwrap_or(q);
            job.append(
                hit,
                0.0,
                TextFormat {
                    font_id: font.clone(),
                    color: fg,
                    background: Color32::from_rgb(0xFE, 0xF3, 0xC7),
                    ..Default::default()
                },
            );
            byte += hit_len;
            pos = i + hit_len;
        } else {
            job.append(
                &title[pos..],
                0.0,
                TextFormat {
                    font_id: font,
                    color: fg,
                    ..Default::default()
                },
            );
            break;
        }
    }
    job
}

/// 绘制大纲侧栏。`current_line` 为视口/光标对应源行。
pub fn show(
    ui: &mut Ui,
    entries: &[TocEntry],
    filter: &mut String,
    expanded: &mut HashSet<usize>,
    inited: &mut bool,
    current_line: Option<usize>,
    last_hl: &mut Option<usize>,
    follow_hl: bool,
) -> Option<OutlineAction> {
    ensure_expanded(entries, expanded, inited);
    let mut action = None;
    let bg = Color32::from_rgb(0xF3, 0xF3, 0xF3);
    ui.painter()
        .rect_filled(ui.available_rect_before_wrap(), 0.0, bg);
    ui.style_mut().interaction.selectable_labels = false;

    Frame::new()
        .fill(bg)
        .inner_margin(Margin::symmetric(6, 6))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::t().chapter_list).strong().size(12.5));
            });
            ui.add_space(4.0);
            let hint = RichText::new(crate::i18n::t().filter_chapters).size(12.0).weak();
            let filter_id = ui.make_persistent_id("outline_filter");
            let filter_focus = ui.memory(|m| m.has_focus(filter_id));
            if filter_focus && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                filter.clear();
                ui.memory_mut(|m| m.surrender_focus(filter_id));
            }
            ui.add(
                egui::TextEdit::singleline(filter)
                    .id(filter_id)
                    .hint_text(hint)
                    .desired_width(ui.available_width())
                    .font(FontId::proportional(12.5)),
            );
            ui.add_space(4.0);

            if entries.is_empty() {
                ui.label(
                    RichText::new(crate::i18n::t().no_headings)
                        .size(12.0)
                        .color(Color32::from_rgb(0x88, 0x88, 0x88)),
                );
                return;
            }

            let filtering = !filter.trim().is_empty();
            let shown = filter_shown(entries, filter);
            let hl_line = if follow_hl { current_line } else { *last_hl };
            let hl_idx = hl_line.and_then(|line| {
                let ideal = ideal_index(entries, line)?;
                let vis = if filtering {
                    ideal
                } else {
                    visible_on_path(entries, expanded, ideal)
                };
                if shown.get(vis).copied().unwrap_or(false) {
                    Some(vis)
                } else {
                    None
                }
            });
            if follow_hl {
                *last_hl = hl_idx.map(|i| entries[i].line0);
            }

            let min_lv = entries.iter().map(|e| e.level).min().unwrap_or(1);
            let sel_bg = ui.visuals().selection.bg_fill.gamma_multiply(0.55);
            crate::view::content_scroll(true)
                .id_salt("outline_scroll")
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    for i in 0..entries.len() {
                        if !row_visible(entries, expanded, &shown, i, filtering) {
                            continue;
                        }
                        let e = &entries[i];
                        let kids = has_children(entries, i);
                        let depth = e.level.saturating_sub(min_lv);
                        let selected = hl_idx == Some(i);
                        let open = filtering || expanded.contains(&e.line0);
                        let fill = if selected {
                            sel_bg
                        } else {
                            Color32::TRANSPARENT
                        };
                        let row = ui.push_id(e.line0, |ui| {
                        Frame::new()
                            .fill(fill)
                            .corner_radius(3.0)
                            .inner_margin(Margin::symmetric(2, 1))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    ui.add_space(depth as f32 * 12.0);
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(14.0, 18.0),
                                        Layout::left_to_right(Align::Center),
                                        |ui| {
                                            if kids {
                                                let tri = if open { "▼" } else { "▶" };
                                                let r = ui
                                                    .add(
                                                        Label::new(
                                                            RichText::new(tri).size(10.0).color(
                                                                Color32::from_rgb(0x9C, 0xA3, 0xAF),
                                                            ),
                                                        )
                                                        .selectable(false)
                                                        .sense(Sense::click()),
                                                    )
                                                    .on_hover_cursor(CursorIcon::Default);
                                                if r.clicked() {
                                                    if expanded.contains(&e.line0) {
                                                        expanded.remove(&e.line0);
                                                    } else {
                                                        expanded.insert(e.line0);
                                                    }
                                                }
                                            }
                                        },
                                    );
                                    let job = highlight_match(&e.title, filter, selected);
                                    let lab = ui
                                        .add(
                                            Label::new(job)
                                                .truncate()
                                                .selectable(false)
                                                .sense(Sense::click()),
                                        )
                                        .on_hover_text(&e.title)
                                        .on_hover_cursor(CursorIcon::Default);
                                    if lab.clicked() {
                                        action = Some(OutlineAction::Jump(e.line0));
                                        if filtering {
                                            expand_ancestors(entries, expanded, i);
                                            filter.clear();
                                        }
                                    }
                                });
                            })
                        });
                        if selected && follow_hl {
                            let r = row.inner.response.rect;
                            let clip = ui.clip_rect();
                            let margin = 6.0;
                            if r.top() < clip.top() + margin || r.bottom() > clip.bottom() - margin
                            {
                                ui.scroll_to_rect(r, None);
                            }
                        }
                    }
                });
        });
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    #[test]
    fn collect_nested_parents() {
        let doc = parser::parse("# A\n\n## A1\n\n### A1a\n\n# B\n");
        let toc = collect(&doc, false);
        assert_eq!(toc.len(), 4);
        assert_eq!(toc[0].title, "A");
        assert_eq!(toc[1].parent, Some(0));
        assert_eq!(toc[2].parent, Some(1));
        assert_eq!(toc[3].parent, None);
        assert_eq!(toc[3].title, "B");
    }

    #[test]
    fn auto_number_prefix() {
        let doc = parser::parse("# A\n\n## A1\n");
        let toc = collect(&doc, true);
        assert!(toc[0].title.starts_with("1 "));
        assert!(toc[1].title.starts_with("1.1 "));
    }

    #[test]
    fn visible_path_stops_at_collapsed() {
        let doc = parser::parse("# A\n\n## A1\n\n### deep\n");
        let toc = collect(&doc, false);
        let mut exp = HashSet::new();
        exp.insert(toc[0].line0);
        let ideal = toc.len() - 1;
        let vis = visible_on_path(&toc, &exp, ideal);
        assert_eq!(vis, 1);
        exp.insert(toc[1].line0);
        let vis = visible_on_path(&toc, &exp, ideal);
        assert_eq!(vis, 2);
    }

    #[test]
    fn filter_keeps_ancestors() {
        let doc = parser::parse("# A\n\n## foo\n\n# B\n");
        let toc = collect(&doc, false);
        let shown = filter_shown(&toc, "foo");
        assert!(shown[0]);
        assert!(shown[1]);
        assert!(!shown[2]);
    }

    #[test]
    fn jump_expands_ancestors() {
        let doc = parser::parse("# A\n\n## A1\n\n### deep\n");
        let toc = collect(&doc, false);
        let mut exp = HashSet::new();
        expand_ancestors(&toc, &mut exp, 2);
        assert!(exp.contains(&toc[0].line0));
        assert!(exp.contains(&toc[1].line0));
        assert!(!exp.contains(&toc[2].line0));
    }

    #[test]
    fn details_headings_included() {
        let doc = parser::parse("<details>\n<summary>s</summary>\n\n# In\n\n</details>\n");
        let toc = collect(&doc, false);
        assert!(toc.iter().any(|e| e.title == "In"));
    }
}
