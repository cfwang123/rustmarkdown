//! 查找条：Ctrl+F，不区分大小写，F3 / Shift+F3 上下一个。

use egui::{Color32, Key, Modifiers, RichText, Ui};

#[derive(Clone, Debug)]
pub struct FindHit {
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

#[derive(Clone, Debug, Default)]
pub struct FindState {
    pub open: bool,
    pub query: String,
    pub hits: Vec<FindHit>,
    pub cur: usize,
    pub focus: bool,
}

pub enum FindBarEvent {
    Close,
    Next,
    Prev,
    Changed,
}

impl FindState {
    pub fn recompute(&mut self, text: &str) {
        self.hits = search(text, &self.query);
        self.cur = 0;
    }

    pub fn current(&self) -> Option<&FindHit> {
        self.hits.get(self.cur)
    }

    pub fn next(&mut self) {
        if self.hits.is_empty() {
            return;
        }
        self.cur = (self.cur + 1) % self.hits.len();
    }

    pub fn prev(&mut self) {
        if self.hits.is_empty() {
            return;
        }
        if self.cur == 0 {
            self.cur = self.hits.len() - 1;
        } else {
            self.cur -= 1;
        }
    }

    pub fn paint_ranges(&self) -> (Vec<(usize, usize)>, Option<(usize, usize)>) {
        let all = self
            .hits
            .iter()
            .map(|h| (h.start, h.end))
            .collect();
        let cur = self.current().map(|h| (h.start, h.end));
        (all, cur)
    }
}

pub fn search(text: &str, query: &str) -> Vec<FindHit> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    if text.is_ascii() && q.is_ascii() {
        search_ascii(text, q)
    } else {
        search_chars(text, q)
    }
}

fn search_ascii(text: &str, query: &str) -> Vec<FindHit> {
    let hay = text.to_ascii_lowercase();
    let needle = query.to_ascii_lowercase();
    let mut hits = Vec::new();
    let mut from = 0usize;
    while from + needle.len() <= hay.len() {
        match hay[from..].find(&needle) {
            Some(rel) => {
                let start = from + rel;
                let end = start + needle.len();
                let line = text[..start].bytes().filter(|b| *b == b'\n').count();
                hits.push(FindHit { start, end, line });
                from = end.max(from + 1);
            }
            None => break,
        }
    }
    hits
}

fn search_chars(text: &str, query: &str) -> Vec<FindHit> {
    let q: Vec<char> = query.to_lowercase().chars().collect();
    if q.is_empty() {
        return Vec::new();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut hits = Vec::new();
    let mut i = 0;
    while i + q.len() <= chars.len() {
        let mut ok = true;
        for (k, qc) in q.iter().enumerate() {
            if chars[i + k].1.to_lowercase().next() != Some(*qc) {
                ok = false;
                break;
            }
        }
        if ok {
            let start = chars[i].0;
            let end = if i + q.len() < chars.len() {
                chars[i + q.len()].0
            } else {
                text.len()
            };
            let line = text[..start].bytes().filter(|b| *b == b'\n').count();
            hits.push(FindHit { start, end, line });
            i += q.len();
        } else {
            i += 1;
        }
    }
    hits
}

pub fn show_bar(ui: &mut Ui, st: &mut FindState) -> Option<FindBarEvent> {
    let mut ev = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::i18n::t().find).strong());
        let edit = egui::TextEdit::singleline(&mut st.query)
            .desired_width(220.0)
            .hint_text(crate::i18n::t().find_hint);
        let r = ui.add(edit);
        if st.focus {
            r.request_focus();
            st.focus = false;
        }
        if r.changed() {
            ev = Some(FindBarEvent::Changed);
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            ev = Some(FindBarEvent::Next);
        }
        if r.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            ev = Some(FindBarEvent::Close);
        }
        let n = st.hits.len();
        let label = if n == 0 {
            if st.query.trim().is_empty() {
                " ".to_string()
            } else {
                crate::i18n::t().no_match.to_string()
            }
        } else {
            format!("{}/{}", st.cur + 1, n)
        };
        ui.label(RichText::new(label).color(Color32::from_gray(90)).monospace());
        if ui.button(crate::i18n::t().find_prev).clicked() {
            ev = Some(FindBarEvent::Prev);
        }
        if ui.button(crate::i18n::t().find_next).clicked() {
            ev = Some(FindBarEvent::Next);
        }
        if ui.button("×").clicked() {
            ev = Some(FindBarEvent::Close);
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::F3)) {
            ev = Some(FindBarEvent::Prev);
        } else if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F3)) {
            ev = Some(FindBarEvent::Next);
        }
    });
    ev
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_case_insensitive() {
        let h = search("Hello HELLO heLLo", "hello");
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].start, 0);
        assert_eq!(h[0].end, 5);
    }

    #[test]
    fn counts_lines() {
        let h = search("a\nbb\nccc find me", "find");
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].line, 2);
    }

    #[test]
    fn chinese() {
        let h = search("查找中文查找", "查找");
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn empty_query() {
        assert!(search("abc", "  ").is_empty());
    }

    #[test]
    fn german_ss_does_not_panic() {
        let h = search("Straße STRASSE", "ss");
        assert!(!h.is_empty());
    }

    #[test]
    fn chinese_many_lines() {
        let t = "单元格内容\n".repeat(200);
        let h = search(&t, "内容");
        assert_eq!(h.len(), 200);
        assert_eq!(h[1].line, 1);
    }
}
