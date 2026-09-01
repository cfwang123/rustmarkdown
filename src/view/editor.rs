/// 源码编辑区：TextEdit + Markdown 着色 layouter（对齐 docview vim 风）。
use egui::text::{CCursor, CCursorRange};
use egui::{pos2, Rect, Shape};

pub struct EditorOut {
    pub changed: bool,
    pub top_line: usize,
    pub cursor_line: usize,
    pub sel_chars: usize,
    pub sel_start: usize,
    pub sel_end: usize,
    pub sel_line0: usize,
    pub sel_line1: usize,
    pub sel_byte0: usize,
    pub sel_byte1: usize,
    pub offset_y: f32,
    pub hovered: bool,
    /// 撤销/重做引起的位移，侧栏同步应忽略，避免当成用户滚动。
    pub ignore_scroll_sync: bool,
    /// Ctrl+点击源码链接。
    pub clicked_link: Option<crate::view::md_hl::SrcLink>,
}

pub fn show(
    ui: &mut egui::Ui,
    text: &mut String,
    jump_line: Option<usize>,
    hint: Option<(usize, usize)>,
    find_all: &[(usize, usize)],
    find_cur: Option<(usize, usize)>,
    undoer: &mut egui::util::undoer::Undoer<String>,
) -> EditorOut {
    let mut out = EditorOut {
        changed: false,
        top_line: 0,
        cursor_line: 0,
        sel_chars: 0,
        sel_start: 0,
        sel_end: 0,
        sel_line0: 0,
        sel_line1: 0,
        sel_byte0: 0,
        sel_byte1: 0,
        offset_y: 0.0,
        hovered: false,
        ignore_scroll_sync: false,
        clicked_link: None,
    };
    let mut ui = crate::view::pane_ui(ui);
    ui.painter()
        .rect_filled(ui.max_rect(), 0.0, egui::Color32::WHITE);
    let t0 = std::time::Instant::now();
    let max_h = ui.available_height();
    let pane_w = ui.available_width();
    let (want_undo, want_redo) = ui.input(|i| {
        let cmd = i.modifiers.matches_logically(egui::Modifiers::COMMAND);
        let undo = cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z);
        let redo = cmd
            && (i.key_pressed(egui::Key::Y)
                || (i.modifiers.shift && i.key_pressed(egui::Key::Z)));
        (undo, redo)
    });
    let undo_redo = want_undo || want_redo;
    if undo_redo {
        ui.ctx().input_mut(|i| {
            i.events.retain(|e| !is_undo_redo_key(e));
        });
    }
    let sa = crate::view::content_scroll(true)
        .id_salt("editor_scroll")
        .max_height(max_h)
        .show(&mut ui, |ui| {
            crate::view::wheel_while_dragging(ui);
            ui.set_min_width(pane_w);
            ui.set_max_width(pane_w);
            let h = ui.available_height();
            if h.is_finite() {
                ui.set_min_height(h);
            }
            let fence_bg_idx = ui.painter().add(Shape::Noop);
            let sticky = ui.input(|i| i.pointer.primary_down());
            let overlay = crate::view::md_hl::LayoutOverlay {
                hint,
                find_all,
                find_cur,
            };
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
                crate::view::md_hl::layout_galley(ui, buf.as_str(), wrap_width.max(1.0), sticky)
            };
            let undo_pin = apply_text_undo(undoer, text, want_undo, want_redo);
            if let Some(idx) = undo_pin {
                let id = crate::view::md_hl::editor_widget_id(ui);
                if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), id) {
                    st.cursor
                        .set_char_range(Some(CCursorRange::one(CCursor::new(idx))));
                    st.store(ui.ctx(), id);
                }
            }
            let t_te = std::time::Instant::now();
            let mut te = egui::TextEdit::multiline(text)
                .id_salt(crate::view::md_hl::EDITOR_ID_SALT)
                .code_editor()
                .desired_width(pane_w)
                .desired_rows(8)
                .frame(false)
                .layouter(&mut layouter)
                .show(ui);
            if crate::io::log::enabled() {
                let ms = t_te.elapsed().as_secs_f64() * 1000.0;
                if ms >= crate::io::log::SPAN_MS {
                    let n_rows = te.galley.rows.len();
                    let n_mesh = te.galley
                        .rows
                        .iter()
                        .filter(|r| !r.row.visuals.mesh.vertices.is_empty())
                        .count();
                    let n_vert: usize = te
                        .galley
                        .rows
                        .iter()
                        .map(|r| r.row.visuals.mesh.vertices.len())
                        .sum();
                    crate::io::log::write(&format!(
                        "editor.text_edit {ms:.0}ms rows={n_rows} mesh={n_mesh} vert={n_vert} {}",
                        crate::view::md_hl::last_hollow_log()
                    ));
                }
            }
            let clip = te.text_clip_rect.intersect(ui.clip_rect());
            ui.painter().set(
                fence_bg_idx,
                merge_bg(
                    fence_block_bg(
                        te.galley.as_ref(),
                        te.galley_pos,
                        clip,
                        te.response.rect,
                        text,
                    ),
                    crate::view::md_hl::overlay_bgs(
                        te.galley.as_ref(),
                        te.galley_pos,
                        clip,
                        text,
                        &overlay,
                    ),
                ),
            );
            out.changed = te.response.changed() || undo_pin.is_some();
            let mut pinned = None;
            if let Some(idx) = undo_pin {
                out.ignore_scroll_sync = true;
                pinned = Some(idx);
                te.state
                    .cursor
                    .set_char_range(Some(CCursorRange::one(CCursor::new(idx))));
                te.state.clone().store(ui.ctx(), te.response.id);
                let rect = te
                    .galley
                    .pos_from_cursor(CCursor::new(idx))
                    .translate(te.galley_pos.to_vec2());
                ui.scroll_to_rect(rect, None);
            }
            let clip_top = ui.clip_rect().top();
            let rel = (clip_top - te.galley_pos.y).max(0.0);
            let cc = te.galley.cursor_from_pos(egui::vec2(0.0, rel + 2.0));
            if let Some(idx) = pinned {
                fill_cursor(&mut out, text, idx, idx, cc.index);
            } else if let Some(range) = te.cursor_range {
                let a = range.primary.index;
                let b = range.secondary.index;
                fill_cursor(&mut out, text, a, b, cc.index);
            } else {
                out.top_line = char_index_to_line(text, cc.index);
                out.cursor_line = out.top_line;
            }
            crate::view::md_hl::note_sel_paint(out.sel_chars > 0 || sticky);
            if pinned.is_none() {
                split_text_undo(
                    ui,
                    undoer,
                    text,
                    out.changed,
                    undo_redo,
                    out.cursor_line,
                );
            }
            if pinned.is_none() {
                if let Some(line) = jump_line {
                    let idx = line_to_char_index(text, line);
                    let rect = te
                        .galley
                        .pos_from_cursor(CCursor::new(idx))
                        .translate(te.galley_pos.to_vec2());
                    ui.scroll_to_rect(rect, Some(egui::Align::TOP));
                }
            }
            let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
            if ctrl {
                if let Some(pos) = te.response.hover_pos() {
                    let ch = te.galley.cursor_from_pos(pos - te.galley_pos).index;
                    if let Some(link) = crate::view::md_hl::link_at_char(text, ch) {
                        ui.ctx()
                            .set_cursor_icon(egui::CursorIcon::PointingHand);
                        if te.response.clicked() {
                            out.clicked_link = Some(link);
                        }
                    }
                }
            }
        });
    out.offset_y = sa.state.offset.y;
    let pane = ui.max_rect();
    out.hovered = ui.rect_contains_pointer(pane);
    crate::io::log::slow("editor.show", t0, crate::io::log::SPAN_MS);
    out
}

fn merge_bg(a: Shape, b: Shape) -> Shape {
    match (a, b) {
        (Shape::Noop, x) | (x, Shape::Noop) => x,
        (Shape::Vec(mut v), Shape::Vec(w)) => {
            v.extend(w);
            Shape::Vec(v)
        }
        (Shape::Vec(mut v), x) | (x, Shape::Vec(mut v)) => {
            v.push(x);
            Shape::Vec(v)
        }
        (x, y) => Shape::Vec(vec![x, y]),
    }
}

fn fence_block_bg(
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    clip: Rect,
    widget: Rect,
    text: &str,
) -> Shape {
    let spans = crate::view::md_hl::fence_char_spans(text);
    if spans.is_empty() {
        return Shape::Noop;
    }
    let x0 = widget.left().max(clip.left());
    let x1 = widget.right().min(clip.right());
    if x1 - x0 < 1.0 {
        return Shape::Noop;
    }
    let mut shapes: Vec<Shape> = Vec::new();
    let mut char_i = 0usize;
    let mut run_top: Option<f32> = None;
    let mut run_bot = 0.0_f32;
    let flush = |shapes: &mut Vec<Shape>, top: f32, bot: f32| {
        let r = Rect::from_min_max(pos2(x0, top), pos2(x1, bot)).intersect(clip);
        if r.width() > 0.5 && r.height() > 0.5 {
            shapes.push(Shape::rect_filled(r, 0.0, crate::view::md_hl::CODE_BG));
        }
    };
    for row in &galley.rows {
        let n = row.char_count_including_newline();
        let c0 = char_i;
        let c1 = char_i + n;
        char_i = c1;
        let r = row.rect().translate(galley_pos.to_vec2());
        if r.bottom() < clip.top() - 2.0 || r.top() > clip.bottom() + 2.0 {
            if let Some(top) = run_top.take() {
                flush(&mut shapes, top, run_bot);
            }
            continue;
        }
        let hit = n > 0 && spans.iter().any(|&(a, b)| c0 < b && c1 > a);
        if hit {
            if run_top.is_none() {
                run_top = Some(r.top());
            }
            run_bot = r.bottom();
        } else if let Some(top) = run_top.take() {
            flush(&mut shapes, top, run_bot);
        }
    }
    if let Some(top) = run_top {
        flush(&mut shapes, top, run_bot);
    }
    match shapes.len() {
        0 => Shape::Noop,
        1 => shapes.remove(0),
        _ => Shape::Vec(shapes),
    }
}

fn fill_cursor(out: &mut EditorOut, text: &str, a: usize, b: usize, top_char: usize) {
    let lo = a.min(b);
    let hi = a.max(b);
    out.sel_start = lo;
    out.sel_end = hi;
    out.sel_chars = hi - lo;
    out.cursor_line = 0;
    out.top_line = 0;
    out.sel_line0 = 0;
    out.sel_line1 = 0;
    out.sel_byte0 = 0;
    out.sel_byte1 = if hi == lo { 0 } else { text.len() };
    if text.is_empty() {
        return;
    }
    let mut line = 0usize;
    let mut byte = 0usize;
    let mut i = 0usize;
    let last_sel = if hi > lo { hi - 1 } else { lo };
    let stop = a.max(hi).max(top_char);
    for c in text.chars() {
        if i == a {
            out.cursor_line = line;
        }
        if i == top_char {
            out.top_line = line;
        }
        if i == lo {
            out.sel_line0 = line;
            out.sel_byte0 = byte;
        }
        if i == last_sel {
            out.sel_line1 = line;
        }
        if i == hi {
            out.sel_byte1 = byte;
        }
        if i >= stop {
            break;
        }
        if c == '\n' {
            line += 1;
        }
        byte += c.len_utf8();
        i += 1;
    }
    if i <= a {
        out.cursor_line = line;
    }
    if i <= top_char {
        out.top_line = line;
    }
    if i <= lo {
        out.sel_line0 = line;
        out.sel_byte0 = byte;
    }
    if i <= last_sel {
        out.sel_line1 = line;
    }
    if i <= hi {
        out.sel_byte1 = byte;
    }
    if hi == lo {
        out.sel_line1 = out.sel_line0;
        out.sel_byte1 = out.sel_byte0;
    }
}

/// 与 egui Undoer 默认 `stable_time` 一致：停手这么久后会自动提交。
const UNDO_STABLE_SECS: f64 = 1.0;

#[derive(Clone)]
struct UndoSplit {
    line: usize,
    uncommitted: bool,
    last_change: f64,
}

/// 光标换到其它行时结束当前撤销合并（点击 / 方向键 / 回车）。
/// 同一行连续输入仍合并。
fn undo_split_tick(
    st: &mut UndoSplit,
    line: usize,
    text_changed: bool,
    undo_redo: bool,
    now: f64,
) -> bool {
    let mut commit = false;
    if undo_redo {
        st.uncommitted = false;
    } else {
        if line != st.line && (st.uncommitted || text_changed) {
            commit = true;
            st.uncommitted = false;
        }
        if text_changed {
            st.uncommitted = true;
            st.last_change = now;
        } else if st.uncommitted && now - st.last_change >= UNDO_STABLE_SECS {
            st.uncommitted = false;
        }
    }
    st.line = line;
    commit
}

fn is_undo_redo_key(e: &egui::Event) -> bool {
    match e {
        egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => {
            let cmd = modifiers.matches_logically(egui::Modifiers::COMMAND);
            cmd && (*key == egui::Key::Z || *key == egui::Key::Y)
        }
        _ => false,
    }
}

fn apply_text_undo(
    undoer: &mut egui::util::undoer::Undoer<String>,
    text: &mut String,
    want_undo: bool,
    want_redo: bool,
) -> Option<usize> {
    if want_undo {
        let prev = undoer.undo(text)?.clone();
        let idx = first_diff_char(text, &prev);
        *text = prev;
        return Some(idx);
    }
    if want_redo {
        let next = undoer.redo(text)?.clone();
        let idx = first_diff_char(text, &next);
        *text = next;
        return Some(idx);
    }
    None
}

fn split_text_undo(
    ui: &egui::Ui,
    undoer: &mut egui::util::undoer::Undoer<String>,
    text: &String,
    text_changed: bool,
    undo_redo: bool,
    cursor_line: usize,
) {
    let now = ui.input(|i| i.time);
    let id = ui.id().with("undo_split");
    let mut st = ui
        .ctx()
        .data(|d| d.get_temp::<UndoSplit>(id))
        .unwrap_or(UndoSplit {
            line: cursor_line,
            uncommitted: false,
            last_change: now,
        });
    let commit = undo_split_tick(&mut st, cursor_line, text_changed, undo_redo, now);
    ui.ctx().data_mut(|d| d.insert_temp(id, st));
    undoer.feed_state(now, text);
    if commit {
        undoer.add_undo(text);
    }
}

/// 新旧文本第一个不同字符下标（按 char，不是 byte）。
/// 撤销一串输入后，光标应停在改动处，而不是 Undoer 快照里更早的光标。
fn first_diff_char(old: &str, new: &str) -> usize {
    let n = new.chars().count();
    old.chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .count()
        .min(n)
}

pub(crate) fn line_to_char_index(text: &str, line0: usize) -> usize {
    let mut line = 0usize;
    let mut chars = 0usize;
    for c in text.chars() {
        if line >= line0 {
            return chars;
        }
        if c == '\n' {
            line += 1;
        }
        chars += 1;
    }
    chars
}

fn char_index_to_line(text: &str, index: usize) -> usize {
    let mut line = 0usize;
    for (i, c) in text.chars().enumerate() {
        if i >= index {
            break;
        }
        if c == '\n' {
            line += 1;
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::{first_diff_char, undo_split_tick, UndoSplit, UNDO_STABLE_SECS};

    #[test]
    fn undo_typing_at_end() {
        assert_eq!(first_diff_char("abcXYZ", "abc"), 3);
    }

    #[test]
    fn undo_typing_in_middle() {
        assert_eq!(first_diff_char("abXYZcd", "abcd"), 2);
    }

    #[test]
    fn undo_chinese() {
        assert_eq!(first_diff_char("你好世界", "你好"), 2);
    }

    #[test]
    fn redo_insert_at_end() {
        assert_eq!(first_diff_char("abc", "abcXYZ"), 3);
    }

    #[test]
    fn empty_new() {
        assert_eq!(first_diff_char("hello", ""), 0);
    }

    fn split_st(line: usize) -> UndoSplit {
        UndoSplit {
            line,
            uncommitted: false,
            last_change: 0.0,
        }
    }

    #[test]
    fn undo_merges_same_line_typing() {
        let mut st = split_st(0);
        assert!(!undo_split_tick(&mut st, 0, true, false, 0.1));
        assert!(!undo_split_tick(&mut st, 0, true, false, 0.2));
        assert!(!undo_split_tick(&mut st, 0, false, false, 0.3));
        assert!(st.uncommitted);
    }

    #[test]
    fn undo_splits_after_cursor_moves_to_other_line() {
        let mut st = split_st(0);
        assert!(!undo_split_tick(&mut st, 0, true, false, 0.1));
        assert!(undo_split_tick(&mut st, 2, false, false, 0.2));
        assert!(!st.uncommitted);
        assert!(!undo_split_tick(&mut st, 3, false, false, 0.3));
    }

    #[test]
    fn undo_enter_splits_each_line() {
        let mut st = split_st(0);
        assert!(!undo_split_tick(&mut st, 0, true, false, 0.1));
        assert!(undo_split_tick(&mut st, 1, true, false, 0.2));
        assert!(st.uncommitted);
        assert!(!undo_split_tick(&mut st, 1, true, false, 0.3));
        assert!(undo_split_tick(&mut st, 2, true, false, 0.4));
        assert!(!undo_split_tick(&mut st, 2, true, false, 0.5));
    }

    #[test]
    fn undo_stable_clears_without_extra_point() {
        let mut st = split_st(0);
        assert!(!undo_split_tick(&mut st, 0, true, false, 0.0));
        assert!(!undo_split_tick(
            &mut st,
            0,
            false,
            false,
            UNDO_STABLE_SECS,
        ));
        assert!(!st.uncommitted);
        assert!(!undo_split_tick(&mut st, 5, false, false, UNDO_STABLE_SECS + 0.1));
    }

    #[test]
    fn undo_redo_does_not_mark_uncommitted() {
        let mut st = split_st(0);
        assert!(!undo_split_tick(&mut st, 0, true, true, 0.1));
        assert!(!st.uncommitted);
        assert!(!undo_split_tick(&mut st, 2, false, false, 0.2));
    }
}
