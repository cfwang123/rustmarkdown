use eframe::egui;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::parser::MdDoc;
use crate::view::preview::PreviewState;
use crate::view::sync::{self, Guard};
use std::collections::HashSet;

/// 三种视图模式（对齐 docview：代码 / 侧边预览 / 预览）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Code,
    Side,
    Preview,
}

/// 标签文档类型。Word / PDF / 图片只读预览。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    Markdown,
    Word,
    Pdf,
    Image,
}

impl ViewMode {
    pub fn label(self) -> &'static str {
        let t = crate::i18n::t();
        match self {
            ViewMode::Code => t.mode_code,
            ViewMode::Side => t.mode_side,
            ViewMode::Preview => t.mode_preview,
        }
    }
}

/// 保存时用的换行风格（内存中统一 `\n`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Newline {
    Lf,
    CrLf,
}

impl Default for Newline {
    fn default() -> Self {
        if cfg!(windows) {
            Newline::CrLf
        } else {
            Newline::Lf
        }
    }
}

fn content_hash(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// 单文档会话。
pub struct DocSession {
    pub path: Option<PathBuf>,
    pub text: String,
    pub dirty: bool,
    /// 上次打开/保存时的正文哈希；撤销回原文时清 dirty。
    pub saved_hash: u64,
    pub newline: Newline,
    pub enc: crate::io::file::TextEnc,
    /// Vim 加密文件的解密信息（zip / blowfish / blowfish2）；非加密文件为 None。
    pub vim: Option<crate::io::vimcrypt::VimSecret>,
}

impl DocSession {
    pub fn untitled(text: String) -> Self {
        let saved_hash = content_hash(&text);
        Self {
            path: None,
            text,
            dirty: false,
            saved_hash,
            newline: Newline::default(),
            enc: crate::io::file::TextEnc::utf8(false),
            vim: None,
        }
    }

    pub fn from_file(
        path: PathBuf,
        text: String,
        newline: Newline,
        enc: crate::io::file::TextEnc,
    ) -> Self {
        let saved_hash = content_hash(&text);
        Self {
            path: Some(path),
            text,
            dirty: false,
            saved_hash,
            newline,
            enc,
            vim: None,
        }
    }

    pub fn mark_clean(&mut self) {
        self.saved_hash = content_hash(&self.text);
        self.dirty = false;
    }

    pub fn sync_dirty(&mut self) {
        self.dirty = content_hash(&self.text) != self.saved_hash;
    }

    pub fn display_name(&self) -> String {
        match &self.path {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string_lossy().into_owned()),
            None => crate::i18n::t().untitled.to_string(),
        }
    }

    pub fn path_eq(&self, other: &Path) -> bool {
        let Some(p) = &self.path else {
            return false;
        };
        norm_path(p) == norm_path(other)
    }
}

/// 一个标签：文档 + 该标签独立的视图状态。
pub struct Tab {
    pub id: u64,
    pub kind: DocKind,
    pub doc: DocSession,
    pub mode: ViewMode,
    pub last_edit_mode: ViewMode,
    pub split_ratio: f32,
    pub md: MdDoc,
    pub preview: PreviewState,
    pub reparse_at: Option<Instant>,
    /// Word 抽出的图片目录（Markdown 相对路径以此为根）。
    pub asset_dir: Option<PathBuf>,
    pub pdf: Option<crate::view::pdf::PdfSession>,
    pub image: Option<crate::view::img_view::ImageSession>,
    /// 大纲/锚点跳转到该源行（0-based）；跳转后保留若干帧以便布局完成。
    pub pending_jump: Option<usize>,
    pub jump_frames: u8,
    pub outline_expanded: HashSet<usize>,
    pub outline_inited: bool,
    /// 代码模式视口顶行（预览模式用 `preview.top_line`）。
    pub editor_top_line: usize,
    pub cursor_line: usize,
    pub sel_chars: usize,
    pub sel_start: usize,
    pub sel_end: usize,
    pub sel_line0: usize,
    pub sel_line1: usize,
    pub sel_byte0: usize,
    pub sel_byte1: usize,
    pub pending_editor_line: Option<usize>,
    pub pending_preview_line: Option<usize>,
    pub find: crate::view::find::FindState,
    /// 只记录正文增删，不含光标移动。
    pub text_undo: egui::util::undoer::Undoer<String>,
    last_editor_off: f32,
    last_preview_off: f32,
    sync_armed: bool,
    sync_guard: Option<Guard>,
}

impl Tab {
    pub fn new(id: u64, doc: DocSession, mode: ViewMode, tab_size: i32) -> Self {
        let last_edit_mode = match mode {
            ViewMode::Preview => ViewMode::Code,
            other => other,
        };
        let md = crate::parser::parse_with_tab(&doc.text, tab_size);
        let text_undo = new_text_undo(&doc.text);
        Self {
            id,
            kind: DocKind::Markdown,
            doc,
            mode,
            last_edit_mode,
            split_ratio: 0.5,
            md,
            preview: PreviewState::default(),
            reparse_at: None,
            asset_dir: None,
            pdf: None,
            image: None,
            pending_jump: None,
            jump_frames: 0,
            outline_expanded: HashSet::new(),
            outline_inited: false,
            editor_top_line: 0,
            cursor_line: 0,
            sel_chars: 0,
            sel_start: 0,
            sel_end: 0,
            sel_line0: 0,
            sel_line1: 0,
            sel_byte0: 0,
            sel_byte1: 0,
            pending_editor_line: None,
            pending_preview_line: None,
            find: crate::view::find::FindState::default(),
            text_undo,
            last_editor_off: 0.0,
            last_preview_off: 0.0,
            sync_armed: false,
            sync_guard: None,
        }
    }

    pub fn is_readonly(&self) -> bool {
        self.kind != DocKind::Markdown
    }

    pub fn reset_text_undo(&mut self) {
        self.text_undo = new_text_undo(&self.doc.text);
    }

    pub fn mark_edited(&mut self) {
        if self.is_readonly() {
            return;
        }
        self.doc.sync_dirty();
        self.reparse_at = Some(Instant::now() + Duration::from_millis(180));
    }

    pub fn reparse(&mut self, tab_size: i32) {
        self.md = crate::parser::parse_with_tab(&self.doc.text, tab_size);
        self.reparse_at = None;
    }

    pub fn reparse_if_due(&mut self, ctx: &egui::Context, tab_size: i32) {
        if let Some(at) = self.reparse_at {
            if Instant::now() >= at {
                self.reparse(tab_size);
            } else {
                ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
            }
        }
    }

    pub fn title(&self) -> String {
        let name = self.doc.display_name();
        if self.doc.dirty {
            format!("{name} *")
        } else {
            name
        }
    }

    pub fn set_mode(&mut self, mode: ViewMode) {
        if self.is_readonly() {
            self.mode = ViewMode::Preview;
            return;
        }
        if self.mode == mode {
            return;
        }
        if self.mode != ViewMode::Preview {
            self.last_edit_mode = self.mode;
        }
        if mode == ViewMode::Side {
            self.sync_armed = false;
        }
        self.mode = mode;
    }

    pub fn request_jump(&mut self, line0: usize) {
        self.pending_jump = Some(line0);
        self.pending_editor_line = Some(line0);
        self.pending_preview_line = Some(line0);
        self.jump_frames = 3;
        self.sync_guard = Some(Guard::after(sync::Origin::Editor));
    }

    /// 打开已记住的文件时恢复模式与视口（Markdown 行 / PDF·Word 页）。
    pub fn apply_saved_view(&mut self, mode: ViewMode, line: usize) {
        match self.kind {
            DocKind::Markdown => {
                self.set_mode(mode);
                if line > 0 {
                    self.request_jump(line);
                }
            }
            DocKind::Pdf => {
                if line > 0 {
                    if let Some(pdf) = self.pdf.as_mut() {
                        pdf.jump_to(line);
                    }
                }
            }
            DocKind::Word => {
                if line > 0 {
                    self.preview.request_word_page(line);
                }
            }
            DocKind::Image => {}
        }
    }

    pub fn tick_jump(&mut self) {
        if self.jump_frames == 0 {
            self.pending_jump = None;
            self.pending_editor_line = None;
            self.pending_preview_line = None;
            return;
        }
        self.jump_frames -= 1;
        if self.jump_frames == 0 {
            self.pending_jump = None;
            self.pending_editor_line = None;
            self.pending_preview_line = None;
        }
    }

    pub fn apply_side_sync(&mut self, ed_off: f32, ed_hovered: bool, ignore_ed: bool) {
        if self.sync_guard.as_ref().is_some_and(|g| !g.active()) {
            self.sync_guard = None;
        }
        let guard_on = self.sync_guard.as_ref().is_some_and(|g| g.active());
        let origin = self.sync_guard.as_ref().map(|g| g.origin());
        let block_ed = guard_on && origin == Some(sync::Origin::Preview);
        let block_pv = guard_on && origin == Some(sync::Origin::Editor);
        let ed_scroll = !ignore_ed
            && sync::user_scrolled(
                self.last_editor_off,
                ed_off,
                self.last_preview_off,
                self.preview.offset_y,
                ed_hovered,
                self.sync_armed,
                block_ed,
            );
        let pv_scroll = sync::user_scrolled(
            self.last_preview_off,
            self.preview.offset_y,
            self.last_editor_off,
            ed_off,
            self.preview.hovered,
            self.sync_armed,
            block_pv,
        );
        self.last_editor_off = ed_off;
        self.last_preview_off = self.preview.offset_y;
        if ignore_ed {
            self.sync_guard = Some(Guard::after(sync::Origin::Editor));
        }
        if !self.sync_armed {
            self.sync_armed = true;
            return;
        }
        if ignore_ed {
            return;
        }
        if ed_scroll {
            self.pending_preview_line = Some(self.editor_top_line);
            self.jump_frames = 3;
            self.sync_guard = Some(Guard::after(sync::Origin::Editor));
        } else if pv_scroll {
            self.pending_editor_line = Some(self.preview.top_line);
            self.jump_frames = 3;
            self.sync_guard = Some(Guard::after(sync::Origin::Preview));
        }
    }

    pub fn editor_hint_range(&self) -> Option<(usize, usize)> {
        let (a, b) = self.preview.pick_lines?;
        let s = crate::view::editor::line_to_char_index(&self.doc.text, a);
        let e = crate::view::editor::line_to_char_index(&self.doc.text, b.saturating_add(1));
        if s < e {
            Some((s, e))
        } else {
            None
        }
    }

    pub fn toggle_preview_edit(&mut self) {
        if self.is_readonly() {
            return;
        }
        if self.mode == ViewMode::Preview {
            self.mode = self.last_edit_mode;
        } else {
            self.last_edit_mode = self.mode;
            self.mode = ViewMode::Preview;
        }
    }
}

pub fn norm_path(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn new_text_undo(text: &str) -> egui::util::undoer::Undoer<String> {
    let mut u = egui::util::undoer::Undoer::default();
    u.add_undo(&text.to_owned());
    u
}

#[cfg(test)]
mod tests {
    use super::DocSession;

    #[test]
    fn undo_to_saved_clears_dirty() {
        let mut d = DocSession::untitled("hello".into());
        assert!(!d.dirty);
        d.text.push('!');
        d.sync_dirty();
        assert!(d.dirty);
        d.text.pop();
        d.sync_dirty();
        assert!(!d.dirty);
    }

    #[test]
    fn save_then_edit_then_undo() {
        let mut d = DocSession::untitled(String::new());
        d.text.push_str("abc");
        d.sync_dirty();
        assert!(d.dirty);
        d.mark_clean();
        assert!(!d.dirty);
        d.text.push('d');
        d.sync_dirty();
        assert!(d.dirty);
        d.text.pop();
        d.sync_dirty();
        assert!(!d.dirty);
    }
}
