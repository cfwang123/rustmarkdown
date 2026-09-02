//! 源码 / 预览双击扩选、三击选行。避开 egui `ccursor_previous_word` 整篇倒序拷贝。

use std::sync::Arc;

use egui::text::{CCursor, CCursorRange};
use egui::text_selection::LabelSelectionState;
use egui::{Color32, Event, Galley, Id, PointerButton, Pos2, Response, Stroke, Ui};

/// 双击扩选停在中英文标点之前（不含）。空格、Tab 算进词里；换行仍分隔。
/// `: ： % # / \` 与 `.` 不算分隔符，路径、URL、锚点、百分号会整段选中。
pub fn is_sel_break(c: char) -> bool {
    if c == '\n' || c == '\r' {
        return true;
    }
    matches!(
        c,
        // 英文标点（不含 . : % # / \）
        '\'' | '"' | '(' | ')' | '[' | ']' | '{' | '}' | '`' | ',' | ';' | '!' | '?'
        // 中文标点（不含全角 ：）
            | '，' | '。' | '、' | '；' | '！' | '？' | '‘' | '“' | '’' | '”' | '（' | '）' | '【'
            | '】' | '《' | '》' | '「' | '」' | '『' | '』'
    )
}

fn is_line_break(c: char) -> bool {
    c == '\n' || c == '\r'
}

/// 从 `char_idx` 向两侧扩到分隔符之前。点在分隔符上则只选该字。
pub fn expand_token(text: &str, char_idx: usize) -> (usize, usize) {
    let mut start = 0usize;
    let mut i = 0usize;
    let mut hit = false;
    for c in text.chars() {
        if i == char_idx {
            hit = true;
            if is_sel_break(c) {
                return (i, i + 1);
            }
        }
        if is_sel_break(c) {
            if hit {
                return (start, i);
            }
            start = i + 1;
        }
        i += 1;
    }
    (start, i)
}

/// 选中 `char_idx` 所在行的文字（不含换行）。
pub fn expand_line(text: &str, char_idx: usize) -> (usize, usize) {
    let mut start = 0usize;
    let mut i = 0usize;
    let mut hit = false;
    for c in text.chars() {
        if i == char_idx {
            hit = true;
        }
        if is_line_break(c) {
            if hit {
                return (start, i);
            }
            start = i + 1;
        }
        i += 1;
    }
    (start, i)
}

/// 去掉 `[start, end)` 两端空白字符（char 下标）。全是空白则返回空区间。
pub fn trim_ws_range(text: &str, start: usize, end: usize) -> (usize, usize) {
    let mut a = start.min(end);
    let b = start.max(end);
    let mut i = 0usize;
    for c in text.chars() {
        if i >= b {
            break;
        }
        if i >= a {
            if c.is_whitespace() {
                a += 1;
            } else {
                break;
            }
        }
        i += 1;
    }
    if a >= b {
        return (a, a);
    }
    let mut end_ex = a;
    let mut found = false;
    i = 0;
    for c in text.chars() {
        if i >= a && i < b {
            if !c.is_whitespace() {
                end_ex = i + 1;
                found = true;
            }
        }
        i += 1;
        if i >= b {
            break;
        }
    }
    if found {
        (a, end_ex)
    } else {
        (a, a)
    }
}

/// 预览 galley 里光标所在的视觉行（折行后的一行）。
pub fn visual_row_range(galley: &Galley, char_idx: usize) -> (usize, usize) {
    let mut i = 0usize;
    let mut last = (0usize, 0usize);
    for row in &galley.rows {
        let n_ex = row.char_count_excluding_newline();
        let n_in = row.char_count_including_newline();
        last = (i, i + n_ex);
        if char_idx < i + n_in {
            return last;
        }
        i += n_in;
    }
    last
}

pub fn range_at(text: &str, char_idx: usize, triple: bool) -> CCursorRange {
    let (a, b) = if triple {
        expand_line(text, char_idx)
    } else {
        expand_token(text, char_idx)
    };
    let (a, b) = trim_ws_range(text, a, b);
    CCursorRange::two(CCursor::new(a), CCursor::new(b))
}

/// 预览：双击按分隔符扩词；三击选中当前视觉行（折行后的一行）。
pub fn range_at_galley(galley: &Galley, char_idx: usize, triple: bool) -> CCursorRange {
    let text = galley.text();
    let (a, b) = if triple {
        visual_row_range(galley, char_idx)
    } else {
        expand_token(text, char_idx)
    };
    let (a, b) = trim_ws_range(text, a, b);
    CCursorRange::two(CCursor::new(a), CCursor::new(b))
}

fn slice_chars(text: &str, a: usize, b: usize) -> String {
    text.chars().skip(a).take(b.saturating_sub(a)).collect()
}

#[derive(Clone, Copy)]
pub struct ClickSeq {
    pub t: f64,
    pub x: f32,
    pub y: f32,
    pub n: u8,
}

impl Default for ClickSeq {
    fn default() -> Self {
        Self {
            t: f64::NEG_INFINITY,
            x: 0.0,
            y: 0.0,
            n: 0,
        }
    }
}

/// 自己数单击/双击/三击。松开且在区域内才返回 1/2/3，否则 0。
pub fn tick_click_seq(
    st: &mut ClickSeq,
    now: f64,
    x: f32,
    y: f32,
    released: bool,
    over: bool,
) -> u8 {
    if !released || !over {
        return 0;
    }
    let dist = (st.x - x).hypot(st.y - y);
    if now - st.t < 0.45 && dist < 10.0 {
        st.n = st.n.saturating_add(1).min(3);
    } else {
        st.n = 1;
    }
    st.t = now;
    st.x = x;
    st.y = y;
    st.n
}

/// 指针在 `area` 或 clip 内松开时的连击次数（自己计数，并与 egui 取较大值）。
pub fn multi_click_over(ui: &Ui, area: egui::Rect) -> u8 {
    let clip = ui.clip_rect();
    let (now, pos, released, egui_n) = ui.input(|i| {
        let pos = i
            .pointer
            .interact_pos()
            .or(i.pointer.hover_pos())
            .or(i.pointer.latest_pos());
        let released = i.pointer.primary_released();
        let egui_n = if i.pointer.button_triple_clicked(PointerButton::Primary) {
            3
        } else if i.pointer.button_double_clicked(PointerButton::Primary) {
            2
        } else {
            0
        };
        (i.time, pos, released, egui_n)
    });
    let over = pos.is_some_and(|p| area.contains(p) || clip.contains(p));
    let id = Id::new("editor_click_seq");
    let mut st = ui
        .ctx()
        .data(|d| d.get_temp::<ClickSeq>(id))
        .unwrap_or_default();
    let ours = match pos {
        Some(p) => tick_click_seq(&mut st, now, p.x, p.y, released, over),
        None => 0,
    };
    ui.ctx().data_mut(|d| d.insert_temp(id, st));
    if released && over {
        ours.max(egui_n)
    } else {
        0
    }
}

const STICKY_ID: &str = "editor_sticky_sel";

pub fn sticky_range(ui: &Ui) -> Option<CCursorRange> {
    ui.ctx().data(|d| d.get_temp(Id::new(STICKY_ID)))
}

pub fn set_sticky(ui: &Ui, range: CCursorRange) {
    ui.ctx()
        .data_mut(|d| d.insert_temp(Id::new(STICKY_ID), range));
}

pub fn clear_sticky(ui: &Ui) {
    ui.ctx()
        .data_mut(|d| d.remove::<CCursorRange>(Id::new(STICKY_ID)));
}

/// 新的按下或改字才丢掉双击粘住的选区（不要见任何 Key 就清）。
pub fn should_clear_sticky(ui: &Ui) -> bool {
    ui.input(|i| {
        i.pointer.primary_pressed()
            || i.events.iter().any(|e| {
                matches!(
                    e,
                    Event::Text(_)
                        | Event::Paste(_)
                        | Event::Cut
                        | Event::Key {
                            pressed: true,
                            key: egui::Key::Backspace
                                | egui::Key::Delete
                                | egui::Key::Enter
                                | egui::Key::Tab
                                | egui::Key::ArrowLeft
                                | egui::Key::ArrowRight
                                | egui::Key::ArrowUp
                                | egui::Key::ArrowDown
                                | egui::Key::Home
                                | egui::Key::End
                                | egui::Key::PageUp
                                | egui::Key::PageDown,
                            ..
                        }
                )
            })
    })
}

const PREVIEW_STICKY_ID: &str = "preview_click_sel";

#[derive(Clone)]
pub struct PreviewClickSel {
    pub id: Id,
    pub range: CCursorRange,
    pub text: String,
}

pub fn preview_click_sel(ui: &Ui) -> Option<PreviewClickSel> {
    ui.ctx()
        .data(|d| d.get_temp(Id::new(PREVIEW_STICKY_ID)))
}

pub fn clear_preview_click_sel(ui: &Ui) {
    ui.ctx()
        .data_mut(|d| d.remove::<PreviewClickSel>(Id::new(PREVIEW_STICKY_ID)));
}

fn set_preview_click_sel(ui: &Ui, sel: PreviewClickSel) {
    ui.ctx()
        .data_mut(|d| d.insert_temp(Id::new(PREVIEW_STICKY_ID), sel));
}

/// 预览 Label 选区。双击扩词、三击选视觉行（与源码分隔符规则一致）；空 galley 多击帧不走 egui 分词。
/// `multi_click_sel == false` 时双击/三击不扩选（供长代码块双击折叠占用）。
pub fn paint_selectable_galley(
    ui: &Ui,
    response: &Response,
    galley_pos: Pos2,
    galley: Arc<Galley>,
    color: Color32,
    underline: Stroke,
) {
    paint_selectable_galley_ex(ui, response, galley_pos, galley, color, underline, true);
}

pub fn paint_selectable_galley_ex(
    ui: &Ui,
    response: &Response,
    galley_pos: Pos2,
    galley: Arc<Galley>,
    color: Color32,
    underline: Stroke,
    multi_click_sel: bool,
) {
    let text = galley.text();
    let empty = text.is_empty();
    let (click_n, pressed, pointer) = ui.input(|i| {
        let n = if i.pointer.button_triple_clicked(PointerButton::Primary) {
            3
        } else if i.pointer.button_double_clicked(PointerButton::Primary) {
            2
        } else {
            0
        };
        let pos = i
            .pointer
            .interact_pos()
            .or(i.pointer.hover_pos())
            .or(i.pointer.latest_pos());
        (n, i.pointer.primary_pressed(), pos)
    });

    if pressed && click_n < 2 {
        clear_preview_click_sel(ui);
    }

    if multi_click_sel && !empty && click_n >= 2 && response.contains_pointer() {
        if let Some(pos) = pointer {
            let idx = galley.cursor_from_pos(pos - galley_pos).index;
            let range = range_at_galley(galley.as_ref(), idx, click_n >= 3);
            let [a, b] = range.sorted_cursors();
            let selected = slice_chars(text, a.index, b.index);
            set_preview_click_sel(
                ui,
                PreviewClickSel {
                    id: response.id,
                    range,
                    text: selected,
                },
            );
            ui.ctx()
                .plugin::<LabelSelectionState>()
                .lock()
                .clear_selection();
        }
    }

    if let Some(sel) = preview_click_sel(ui).filter(|s| s.id == response.id) {
        let clip = ui.clip_rect();
        let bg = crate::view::md_hl::selection_bgs(
            galley.as_ref(),
            galley_pos,
            clip,
            sel.range,
            ui.visuals().selection.bg_fill,
        );
        ui.painter().add(bg);
        ui.painter().galley(galley_pos, galley, color);
        let _ = underline;
        if ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Copy))) {
            if !sel.text.is_empty() {
                ui.ctx().copy_text(sel.text.clone());
                ui.ctx().input_mut(|i| {
                    i.events.retain(|e| !matches!(e, Event::Copy));
                });
            }
        }
        return;
    }

    // 空 galley，或禁用多击扩选时的多击帧：不走 egui 分词（会崩 / 抢折叠手势）。
    if click_n >= 2 && (empty || !multi_click_sel) {
        ui.painter().galley(galley_pos, galley, color);
        let _ = underline;
        return;
    }

    LabelSelectionState::label_text_selection(ui, response, galley_pos, galley, color, underline);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_hyphen_and_comma() {
        let s = "hello-world,foo";
        let i = s.chars().take_while(|c| *c != 'w').count();
        assert_eq!(expand_token(s, i), (0, 11));
        let comma = s.chars().take_while(|c| *c != ',').count();
        assert_eq!(expand_token(s, comma), (comma, comma + 1));
        let f = s.chars().take_while(|c| *c != 'f').count();
        assert_eq!(expand_token(s, f), (12, 15));
    }

    #[test]
    fn token_quotes_and_parens() {
        assert_eq!(expand_token("'ab'", 2), (1, 3));
        assert_eq!(expand_token("(ab)", 2), (1, 3));
        assert_eq!(expand_token("a`b", 0), (0, 1));
        assert_eq!(expand_token("a`b", 2), (2, 3));
    }

    #[test]
    fn token_cjk_punct() {
        let s = "你好，世界";
        assert_eq!(expand_token(s, 0), (0, 2));
        assert_eq!(expand_token(s, 2), (2, 3));
        assert_eq!(expand_token(s, 3), (3, 5));
        let s = "他说：“行”。";
        let q = s.chars().take_while(|c| *c != '行').count();
        assert_eq!(expand_token(s, q), (q, q + 1));
    }

    #[test]
    fn token_space_joins_until_punct() {
        // 空格不算分隔；扩到中英文标点为止。`/` 也不算分隔。
        assert_eq!(expand_token("ab cd", 1), (0, 5));
        assert_eq!(expand_token("ab cd", 2), (0, 5));
        assert_eq!(expand_token("ab\tcd", 0), (0, 5));
        let s = "Markdown 预览 / 编辑器，用 Rust";
        let i = s.chars().take_while(|c| *c != '预').count();
        assert_eq!(expand_token(s, i), (0, "Markdown 预览 / 编辑器".chars().count()));
        let j = s.chars().take_while(|c| *c != '用').count();
        assert_eq!(
            expand_token(s, j),
            (j, s.chars().count())
        );
    }

    #[test]
    fn token_path_and_dot() {
        let s = r"D:\a\b.md";
        assert_eq!(expand_token(s, 3), (0, s.chars().count()));
        assert_eq!(expand_token("foo.bar", 4), (0, 7));
    }

    #[test]
    fn token_colon_slash_backslash_pct_hash_not_break() {
        let url = "http://example.com/a";
        assert_eq!(expand_token(url, 4), (0, url.chars().count()));
        assert_eq!(expand_token("a/b/c", 2), (0, 5));
        let win = r"C:\Windows\a.md";
        assert_eq!(expand_token(win, 3), (0, win.chars().count()));
        assert_eq!(expand_token("key:value", 1), (0, 9));
        assert_eq!(expand_token("他说：好的", 2), (0, "他说：好的".chars().count()));
        assert_eq!(expand_token("100%", 2), (0, 4));
        assert_eq!(expand_token("#anchor", 0), (0, 7));
        assert_eq!(expand_token("a#b%c:d", 3), (0, 7));
    }

    #[test]
    fn token_click_past_end() {
        assert_eq!(expand_token("ab", 9), (0, 2));
        assert_eq!(expand_token("", 0), (0, 0));
    }

    #[test]
    fn line_selects_text_without_newline() {
        let s = "ab\ncd\nef";
        assert_eq!(expand_line(s, 0), (0, 2));
        assert_eq!(expand_line(s, 2), (0, 2));
        assert_eq!(expand_line(s, 3), (3, 5));
        assert_eq!(expand_line(s, 6), (6, 8));
        assert_eq!(expand_line("only", 2), (0, 4));
        assert_eq!(expand_line("", 0), (0, 0));
    }

    #[test]
    fn range_at_token_and_line() {
        let r = range_at("a-b,c", 2, false);
        let [a, b] = r.sorted_cursors();
        assert_eq!((a.index, b.index), (0, 3));
        let r = range_at("a-b\nc", 1, true);
        let [a, b] = r.sorted_cursors();
        assert_eq!((a.index, b.index), (0, 3));
    }

    #[test]
    fn trim_ws_range_ends() {
        assert_eq!(trim_ws_range("  ab  ", 0, 6), (2, 4));
        assert_eq!(trim_ws_range("\tab\t", 0, 4), (1, 3));
        let (a, b) = trim_ws_range("   ", 0, 3);
        assert_eq!(a, b);
        assert_eq!(trim_ws_range("ab", 0, 2), (0, 2));
        assert_eq!(trim_ws_range("a b", 0, 3), (0, 3));
        let (a, b) = trim_ws_range("  ", 1, 2);
        assert_eq!(a, b);
    }

    #[test]
    fn range_at_trims_line_indent_and_space_token() {
        let s = "  hello  \n";
        let r = range_at(s, 4, true);
        let [a, b] = r.sorted_cursors();
        assert_eq!((a.index, b.index), (2, 7));
        let r = range_at("ab cd", 2, false);
        let [a, b] = r.sorted_cursors();
        assert_eq!((a.index, b.index), (0, 5));
    }

    #[test]
    fn click_seq_counts_double_and_triple() {
        let mut st = ClickSeq::default();
        assert_eq!(tick_click_seq(&mut st, 1.0, 10.0, 10.0, true, true), 1);
        assert_eq!(tick_click_seq(&mut st, 1.2, 11.0, 10.0, true, true), 2);
        assert_eq!(tick_click_seq(&mut st, 1.35, 12.0, 10.0, true, true), 3);
        assert_eq!(tick_click_seq(&mut st, 2.5, 12.0, 10.0, true, true), 1);
        assert_eq!(tick_click_seq(&mut st, 2.6, 12.0, 10.0, true, false), 0);
        assert_eq!(tick_click_seq(&mut st, 2.6, 12.0, 10.0, false, true), 0);
    }
}
