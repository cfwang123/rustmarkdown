//! 源码 / 预览双击扩选、三击选行。避开 egui `ccursor_previous_word` 整篇倒序拷贝。

use std::sync::Arc;

use egui::text::{CCursor, CCursorRange};
use egui::text_selection::LabelSelectionState;
use egui::{Color32, Galley, PointerButton, Pos2, Response, Stroke, Ui};

/// 双击扩选停在这些字符之前（不含）：空白、引号、括号、反引号、逗号、中文标点。
pub fn is_sel_break(c: char) -> bool {
    if c.is_whitespace() {
        return true;
    }
    matches!(
        c,
        '\'' | '"' | '(' | ')' | '`' | ',' | '，' | '。' | '、' | '‘' | '“' | '’' | '”' | '（'
            | '）' | '？'
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
    CCursorRange::two(CCursor::new(a), CCursor::new(b))
}

/// 指针在 `area` 内的双击=2、三击=3，否则 0。
pub fn multi_click_over(ui: &Ui, area: egui::Rect) -> u8 {
    ui.input(|i| {
        let over = i
            .pointer
            .interact_pos()
            .or(i.pointer.hover_pos())
            .is_some_and(|p| area.contains(p));
        if !over {
            return 0;
        }
        if i.pointer.button_triple_clicked(PointerButton::Primary) {
            3
        } else if i.pointer.button_double_clicked(PointerButton::Primary) {
            2
        } else {
            0
        }
    })
}

/// 预览 Label 选区：双击/三击时改 galley 文本，让 egui 扩到我们算出的范围（小段文本，无整篇倒序）。
pub fn paint_selectable_galley(
    ui: &Ui,
    response: &Response,
    galley_pos: Pos2,
    mut galley: Arc<Galley>,
    color: Color32,
    underline: Stroke,
) {
    if let Some(masked) = mask_for_multi_click(ui, response, galley_pos, &galley) {
        let g = Arc::make_mut(&mut galley);
        let job = Arc::make_mut(&mut g.job);
        job.text = masked;
    }
    LabelSelectionState::label_text_selection(ui, response, galley_pos, galley, color, underline);
}

fn mask_for_multi_click(
    ui: &Ui,
    response: &Response,
    galley_pos: Pos2,
    galley: &Galley,
) -> Option<String> {
    let triple = response.triple_clicked();
    if !triple && !response.double_clicked() {
        return None;
    }
    let pointer = response
        .interact_pointer_pos()
        .or_else(|| ui.input(|i| i.pointer.hover_pos()))?;
    let idx = galley.cursor_from_pos(pointer - galley_pos).index;
    let text = galley.text();
    let (lo, hi) = if triple {
        visual_row_range(galley, idx)
    } else {
        expand_token(text, idx)
    };
    if lo >= hi {
        return None;
    }
    Some(mask_range(text, lo, hi, triple))
}

fn mask_range(text: &str, lo: usize, hi: usize, as_line: bool) -> String {
    let fill = if as_line { '\n' } else { ' ' };
    let mut out = String::with_capacity(text.len());
    for (i, _) in text.chars().enumerate() {
        if i >= lo && i < hi {
            out.push('x');
        } else {
            out.push(fill);
        }
    }
    out
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
    fn token_whitespace() {
        assert_eq!(expand_token("ab cd", 1), (0, 2));
        assert_eq!(expand_token("ab cd", 2), (2, 3));
        assert_eq!(expand_token("ab\tcd", 0), (0, 2));
    }

    #[test]
    fn token_path_and_dot() {
        let s = r"D:\a\b.md";
        assert_eq!(expand_token(s, 3), (0, s.chars().count()));
        assert_eq!(expand_token("foo.bar", 4), (0, 7));
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
}
