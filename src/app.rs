use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, Modifiers, ViewportBuilder, ViewportCommand, ViewportId};
use egui::{Align, Color32, Layout, RichText};

use crate::doc::{DocKind, DocSession, Tab, ViewMode};
use crate::i18n::{self, t, Lang};
use crate::io::file;
use crate::io::watch::WatchHub;
use crate::nav::{NavHist, NavPoint};
use crate::io::imgcache::ImgCache;
use crate::io::session::{find_file_view, Session, SessionTab, upsert_file_view};
use crate::io::settings::Settings;
use crate::tabs::{self, TabBarEvent};
use crate::view;
use crate::view::find::{self, FindBarEvent};
use crate::view::icons::Icon;
use crate::view::img_preview::{self, ImgPreview, OverlayAction};
use crate::view::md_hl::SrcLink;
use crate::view::preview::{PreviewEvent, PreviewOpts};
use crate::workspace::{self, ExplorerAction, SidebarTab, Workspace};

pub fn viewport_title(tab_title: Option<&str>) -> String {
    let ver = env!("CARGO_PKG_VERSION");
    match tab_title {
        Some(t) => format!("{t} — rustmarkdown v{ver}"),
        None => format!("rustmarkdown v{ver}"),
    }
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn tab_view_line(tab: &Tab) -> usize {
    match tab.kind {
        DocKind::Pdf => tab.pdf.as_ref().map(|p| p.current_page()).unwrap_or(0),
        DocKind::Word => tab.preview.word_page,
        DocKind::Image => 0,
        DocKind::Markdown => match tab.mode {
            ViewMode::Preview => tab.preview.top_line,
            ViewMode::Code | ViewMode::Side => tab.editor_top_line,
        },
    }
}

fn remember_tab_into(views: &mut Vec<SessionTab>, tab: &Tab) {
    let Some(path) = tab.doc.path.clone() else {
        return;
    };
    upsert_file_view(
        views,
        SessionTab::with_line(path, tab.mode, tab_view_line(tab)),
    );
}

const SEL_HINT_MAX_CHARS: usize = 96;

fn sel_hint_text(text: &str, byte0: usize, byte1: usize) -> String {
    let a = text.floor_char_boundary(byte0.min(text.len()));
    let b = text.ceil_char_boundary(byte1.min(text.len())).max(a);
    let slice = &text[a..b];
    let mut end = 0;
    let mut count = 0usize;
    for (i, c) in slice.char_indices() {
        if count >= SEL_HINT_MAX_CHARS {
            break;
        }
        end = i + c.len_utf8();
        count += 1;
    }
    slice[..end].to_string()
}

enum MenuCmd {
    New,
    Open,
    OpenFolder,
    Save,
    SaveAs,
    Close,
    CloseAll,
    Reopen,
    CopyPath,
    Reveal,
    Exit,
    Mode(ViewMode),
    Toggle,
    Settings,
    About,
    Sidebar,
    NavBack,
    NavFwd,
    Find,
    OpenRecent(PathBuf),
    ClearRecent,
    Lang(Lang),
}

pub struct ClosedTab {
    pub path: Option<PathBuf>,
    pub text: String,
    pub newline: crate::doc::Newline,
    pub mode: ViewMode,
    pub dirty: bool,
    pub saved_hash: u64,
    pub enc: crate::io::file::TextEnc,
}

/// 标签拖拽：条内跟手 / 离条立刻拆窗 / 拖入其它条合并（对齐 docview）。
struct TabDrag {
    tab_id: u64,
    grab_in_chip: egui::Vec2,
    grab_in_win: egui::Vec2,
    floated: bool,
}

/// 一个原生窗口。wins[0] 主窗口；其余为撕出的视口。
pub struct Win {
    pub viewport_id: ViewportId,
    pub extra_uid: u64,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub sidebar_open: bool,
    pub sidebar_width: f32,
    pub sidebar_tab: SidebarTab,
    pub outline_filter: String,
    pub outline_hl: Option<usize>,
    /// 上次同步到大纲的正文滚动位置；仅当正文位置变化时才同步大纲。
    pub outline_sync_line: Option<usize>,
    pub ignore_outline_until: Option<Instant>,
    pub nav: NavHist,
    pub workspace: Option<Workspace>,
    pub pending_close: bool,
    pub open_pos: Option<egui::Pos2>,
    inner_rect: Option<egui::Rect>,
    outer_rect: Option<egui::Rect>,
    tabbar_screen: Option<egui::Rect>,
    chip_mids: Vec<f32>,
    follow_pos: Option<egui::Pos2>,
    create_inner: Option<egui::Vec2>,
}

impl Win {
    fn new() -> Self {
        Self {
            viewport_id: ViewportId::ROOT,
            extra_uid: 0,
            tabs: Vec::new(),
            active: 0,
            sidebar_open: true,
            sidebar_width: 240.0,
            sidebar_tab: SidebarTab::Outline,
            outline_filter: String::new(),
            outline_hl: None,
            outline_sync_line: None,
            ignore_outline_until: None,
            nav: NavHist::default(),
            workspace: None,
            pending_close: false,
            open_pos: None,
            inner_rect: None,
            outer_rect: None,
            tabbar_screen: None,
            chip_mids: Vec::new(),
            follow_pos: None,
            create_inner: None,
        }
    }

    fn extra(uid: u64, vid: ViewportId, pos: Option<egui::Pos2>) -> Self {
        let mut w = Self::new();
        w.viewport_id = vid;
        w.extra_uid = uid;
        w.open_pos = pos;
        w.sidebar_open = false;
        w
    }

    fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }
}

enum Dialog {
    CloseTab(usize),
    CloseOthers(usize),
    CloseAll,
    Quit,
    About,
    Error(String),
    Reload { win: usize, tab: usize },
}

struct PendingImg {
    href: String,
    title: String,
    base: Option<PathBuf>,
}

pub struct App {
    wins: Vec<Win>,
    cur: usize,
    next_tab_id: u64,
    dialog: Option<Dialog>,
    status: String,
    closed_stack: Vec<ClosedTab>,
    drop_hint: bool,
    imgcache: ImgCache,
    mermaid: crate::io::mermaid::MermaidCache,
    img_overlay: Option<ImgPreview>,
    pending_img: Option<PendingImg>,
    settings: Settings,
    settings_draft: Option<Settings>,
    last_session: Option<Session>,
    file_views: Vec<SessionTab>,
    session_save_at: Option<Instant>,
    watch: WatchHub,
    tab_drag: Option<TabDrag>,
    saw_ptr_down: bool,
    last_screen_ptr: Option<egui::Pos2>,
    incoming: Option<crate::io::single::Incoming>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        open_paths: Vec<PathBuf>,
        incoming: crate::io::single::Incoming,
    ) -> Self {
        view::theme::install_fonts(&cc.egui_ctx);
        view::theme::install_style(&cc.egui_ctx);
        std::thread::spawn(|| {
            view::highlight::warmup();
            crate::io::mermaid::warmup();
        });
        let mut app = Self {
            wins: vec![Win::new()],
            cur: 0,
            next_tab_id: 1,
            dialog: None,
            status: String::new(),
            closed_stack: Vec::new(),
            drop_hint: false,
            imgcache: ImgCache::default(),
            mermaid: crate::io::mermaid::MermaidCache::default(),
            img_overlay: None,
            pending_img: None,
            settings: Settings::load(),
            settings_draft: None,
            last_session: None,
            file_views: Vec::new(),
            session_save_at: None,
            watch: WatchHub::new(),
            tab_drag: None,
            saw_ptr_down: false,
            last_screen_ptr: None,
            incoming: Some(incoming),
        };
        crate::io::single::attach_ui(&cc.egui_ctx);
        i18n::set(app.settings.ui_lang);
        app.status = t().ready.to_string();
        crate::io::log::set_enabled(app.settings.enable_logs);
        app.wins[0].sidebar_open = app.settings.side_panel_visible;
        app.wins[0].sidebar_width = app.settings.side_panel_width as f32;
        let sess = Session::load();
        if let Some(s) = &sess {
            app.file_views = s.file_views.clone();
            for t in &s.tabs {
                upsert_file_view(&mut app.file_views, t.clone());
            }
        }
        if open_paths.is_empty() {
            if let Some(s) = &sess {
                app.restore_session(s);
            }
        } else {
            let mut folder_from_args = false;
            for p in open_paths {
                let resolved = crate::io::shell_link::resolve(&file::normalize_incoming_path(&p));
                if resolved.is_dir() {
                    folder_from_args = true;
                }
                if let Err(e) = app.open_incoming(&p) {
                    app.status = e;
                }
            }
            if !folder_from_args {
                if let Some(s) = &sess {
                    app.restore_workspace(s.workspace.as_deref());
                }
            }
        }
        let now = app.capture_session();
        now.save();
        app.last_session = Some(now);
        app
    }

    fn win(&self) -> &Win {
        &self.wins[self.cur.min(self.wins.len().saturating_sub(1))]
    }

    fn win_mut(&mut self) -> &mut Win {
        let i = self.cur.min(self.wins.len().saturating_sub(1));
        &mut self.wins[i]
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        id
    }

    fn find_open(&self, path: &Path) -> Option<(usize, usize)> {
        for (wi, w) in self.wins.iter().enumerate() {
            if let Some(ti) = w.tabs.iter().position(|t| t.doc.path_eq(path)) {
                return Some((wi, ti));
            }
        }
        None
    }

    fn sync_watch(&mut self) {
        let mut paths = Vec::new();
        for w in &self.wins {
            for t in &w.tabs {
                if let Some(p) = &t.doc.path {
                    paths.push(p.clone());
                }
            }
        }
        self.watch.sync(&paths);
    }

    fn capture_session(&self) -> Session {
        let win = &self.wins[0];
        let mut tabs = Vec::new();
        let mut active = 0;
        let mut file_views = self.file_views.clone();
        for w in &self.wins {
            for t in &w.tabs {
                remember_tab_into(&mut file_views, t);
            }
        }
        for (i, t) in win.tabs.iter().enumerate() {
            let Some(path) = t.doc.path.clone() else {
                continue;
            };
            if i == win.active {
                active = tabs.len();
            }
            tabs.push(SessionTab::with_line(path, t.mode, tab_view_line(t)));
        }
        let workspace = self
            .wins
            .iter()
            .find_map(|w| w.workspace.as_ref().map(|ws| ws.root.clone()));
        Session {
            tabs,
            active,
            workspace,
            file_views,
        }
    }

    fn restore_workspace(&mut self, dir: Option<&Path>) {
        let Some(dir) = dir else {
            return;
        };
        if !dir.is_dir() {
            return;
        }
        if self.wins[0].workspace.as_ref().is_some_and(|w| {
            crate::doc::norm_path(&w.root) == crate::doc::norm_path(dir)
        }) {
            return;
        }
        let ws = Workspace::new(dir.to_path_buf());
        self.wins[0].workspace = Some(ws);
    }

    fn restore_session(&mut self, sess: &Session) {
        self.restore_workspace(sess.workspace.as_deref());
        let mut n = 0u32;
        for t in &sess.tabs {
            if !t.path.is_file() {
                continue;
            }
            match self.open_path(0, &t.path) {
                Ok(()) => {
                    if let Some(tab) = self.win_mut().active_tab_mut() {
                        tab.apply_saved_view(t.mode(), t.line());
                    }
                    n += 1;
                }
                Err(e) => self.status = e,
            }
        }
        if n == 0 {
            return;
        }
        if let Some(want) = sess.tabs.get(sess.active) {
            if let Some((wi, ti)) = self.find_open(&want.path) {
                self.cur = wi;
                self.wins[wi].active = ti;
            }
        }
        self.status = i18n::restored(n);
    }

    fn persist_session(&mut self) {
        let now = self.capture_session();
        if self.last_session.as_ref() != Some(&now) {
            self.file_views = now.file_views.clone();
            self.last_session = Some(now);
            self.session_save_at = Some(Instant::now() + Duration::from_millis(500));
        }
    }

    fn flush_session(&mut self, ctx: Option<&egui::Context>, force: bool) {
        let Some(at) = self.session_save_at else {
            return;
        };
        if !force && Instant::now() < at {
            if let Some(ctx) = ctx {
                ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
            }
            return;
        }
        if let Some(s) = &self.last_session {
            s.save();
        }
        self.session_save_at = None;
    }

    fn open_incoming(&mut self, path: &Path) -> Result<(), String> {
        let path = file::normalize_incoming_path(path);
        let resolved = crate::io::shell_link::resolve(&path);
        if resolved.is_dir() {
            self.open_folder(&resolved)
        } else if file::kind_of(&resolved).is_some() || resolved.is_file() {
            self.open_path(0, &resolved)
        } else {
            Err(i18n::path_missing(resolved.display()))
        }
    }

    fn open_folder(&mut self, dir: &Path) -> Result<(), String> {
        if !dir.is_dir() {
            return Err(i18n::not_folder(dir.display()));
        }
        let win = self.win_mut();
        if let Some(ws) = win.workspace.as_mut() {
            ws.navigate(dir.to_path_buf());
        } else {
            win.workspace = Some(Workspace::new(dir.to_path_buf()));
        }
        win.sidebar_open = true;
        win.sidebar_tab = SidebarTab::Explorer;
        self.persist_sidebar();
        self.status = i18n::opened_folder(&file_label(dir));
        Ok(())
    }

    fn open_tab_as_workspace(&mut self, idx: usize) {
        let Some(path) = self.win().tabs.get(idx).and_then(|t| t.doc.path.clone()) else {
            self.status = t().untitled_no_parent.to_string();
            return;
        };
        let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) else {
            self.status = t().no_parent.to_string();
            return;
        };
        if let Err(e) = self.open_folder(dir) {
            self.status = e;
        }
    }

    fn open_path(&mut self, win_i: usize, path: &Path) -> Result<(), String> {
        let path = path.to_path_buf();
        if let Some((wi, ti)) = self.find_open(&path) {
            self.cur = wi;
            self.wins[wi].active = ti;
            self.remember_file(&path);
            self.status = i18n::switched_to(&file_label(&path));
            return Ok(());
        }
        let kind = file::kind_of(&path)
            .ok_or_else(|| i18n::unsupported_type(path.display()))?;
        let id = self.alloc_id();
        let tab_size = self.settings.md_tab_size;
        let tab = match kind {
            DocKind::Markdown => {
                let (text, newline, enc) = file::read_text(&path)?;
                Tab::new(
                    id,
                    DocSession::from_file(path.clone(), text, newline, enc),
                    ViewMode::Preview,
                    tab_size,
                )
            }
            DocKind::Word => {
                let (text, asset) = crate::io::word::load(&path)?;
                let mut tab = Tab::new(
                    id,
                    DocSession::from_file(
                        path.clone(),
                        text,
                        crate::doc::Newline::Lf,
                        file::TextEnc::utf8(false),
                    ),
                    ViewMode::Preview,
                    tab_size,
                );
                tab.kind = DocKind::Word;
                tab.asset_dir = Some(asset);
                tab
            }
            DocKind::Pdf => {
                let mut tab = Tab::new(
                    id,
                    DocSession::from_file(
                        path.clone(),
                        String::new(),
                        crate::doc::Newline::Lf,
                        file::TextEnc::utf8(false),
                    ),
                    ViewMode::Preview,
                    tab_size,
                );
                tab.kind = DocKind::Pdf;
                tab.pdf = Some(view::pdf::PdfSession::open(&path));
                tab
            }
            DocKind::Image => {
                let mut tab = Tab::new(
                    id,
                    DocSession::from_file(
                        path.clone(),
                        String::new(),
                        crate::doc::Newline::Lf,
                        file::TextEnc::utf8(false),
                    ),
                    ViewMode::Preview,
                    tab_size,
                );
                tab.kind = DocKind::Image;
                tab.image = Some(view::img_view::ImageSession::open(&path));
                tab
            }
        };
        let remembered = find_file_view(&self.file_views, &path).map(|v| (v.mode(), v.line()));
        let win = &mut self.wins[win_i];
        win.tabs.push(tab);
        win.active = win.tabs.len() - 1;
        if let Some((mode, line)) = remembered {
            if let Some(t) = win.tabs.last_mut() {
                t.apply_saved_view(mode, line);
            }
        }
        self.sync_watch();
        self.remember_file(&path);
        self.status = i18n::opened(&file_label(&path));
        Ok(())
    }

    fn remember_file(&mut self, path: &Path) {
        if !path.is_file() {
            return;
        }
        self.settings.push_recent(path.to_path_buf());
        self.settings.save();
    }

    fn open_recent(&mut self, path: PathBuf) {
        if !path.is_file() {
            self.settings
                .recent_files
                .retain(|p| crate::doc::norm_path(p) != crate::doc::norm_path(&path));
            self.settings.save();
            self.dialog = Some(Dialog::Error(i18n::file_missing_named(&file_label(&path))));
            return;
        }
        if let Err(e) = self.open_incoming(&path) {
            self.dialog = Some(Dialog::Error(e));
        }
    }

    fn new_untitled(&mut self) {
        let id = self.alloc_id();
        let tab = Tab::new(
            id,
            DocSession::untitled(String::new()),
            ViewMode::Code,
            self.settings.md_tab_size,
        );
        let win = self.win_mut();
        win.tabs.push(tab);
        win.active = win.tabs.len() - 1;
        self.status = t().new_untitled.to_string();
    }

    fn push_closed(&mut self, tab: Tab) {
        self.closed_stack.push(ClosedTab {
            path: tab.doc.path,
            text: tab.doc.text,
            newline: tab.doc.newline,
            mode: tab.mode,
            dirty: tab.doc.dirty,
            saved_hash: tab.doc.saved_hash,
            enc: tab.doc.enc,
        });
        if self.closed_stack.len() > 20 {
            self.closed_stack.remove(0);
        }
    }

    fn reopen_closed(&mut self) {
        let Some(c) = self.closed_stack.pop() else {
            self.status = t().no_reopen.to_string();
            return;
        };
        if let Some(path) = &c.path {
            if !c.dirty {
                if let Err(e) = self.open_path(0, path) {
                    self.status = e;
                }
                return;
            }
        }
        let id = self.alloc_id();
        let mut doc = if let Some(path) = c.path {
            DocSession::from_file(path, c.text, c.newline, c.enc)
        } else {
            DocSession::untitled(c.text)
        };
        doc.dirty = c.dirty;
        doc.saved_hash = c.saved_hash;
        doc.newline = c.newline;
        let mut tab = Tab::new(id, doc, c.mode, self.settings.md_tab_size);
        tab.last_edit_mode = if c.mode == ViewMode::Preview {
            ViewMode::Code
        } else {
            c.mode
        };
        let win = self.win_mut();
        win.tabs.push(tab);
        win.active = win.tabs.len() - 1;
        self.status = t().reopened_tab.to_string();
    }

    fn request_close_tab(&mut self, idx: usize) {
        let Some(tab) = self.win().tabs.get(idx) else {
            return;
        };
        if tab.doc.dirty {
            self.dialog = Some(Dialog::CloseTab(idx));
        } else {
            self.close_tab(idx);
        }
    }

    fn close_tab(&mut self, idx: usize) {
        let win = self.win_mut();
        if idx >= win.tabs.len() {
            return;
        }
        let tab = win.tabs.remove(idx);
        if win.tabs.is_empty() {
            win.active = 0;
        } else if idx < win.active {
            win.active -= 1;
        } else if win.active >= win.tabs.len() {
            win.active = win.tabs.len() - 1;
        }
        remember_tab_into(&mut self.file_views, &tab);
        self.push_closed(tab);
        self.sync_watch();
        if self.cur != 0 && self.win().tabs.is_empty() {
            self.win_mut().pending_close = true;
        }
        self.status = t().closed_tab.to_string();
    }

    fn close_all_tabs(&mut self) {
        while !self.win().tabs.is_empty() {
            self.close_tab(self.win().tabs.len() - 1);
        }
    }

    fn request_close_all(&mut self) {
        if self.win().tabs.is_empty() {
            return;
        }
        if self.win().tabs.iter().any(|t| t.doc.dirty) {
            self.dialog = Some(Dialog::CloseAll);
        } else {
            self.close_all_tabs();
            self.status = t().closed_all.to_string();
        }
    }

    /// 关闭除 `keep` 外的全部标签（按 tab id 跟踪，避免下标移位误删）。
    fn close_other_tabs(&mut self, keep: usize) {
        let Some(keep_id) = self.win().tabs.get(keep).map(|t| t.id) else {
            return;
        };
        while self.win().tabs.len() > 1 {
            let idx = self
                .win()
                .tabs
                .iter()
                .position(|t| t.id != keep_id)
                .unwrap_or(0);
            self.close_tab(idx);
        }
        self.win_mut().active = 0;
        self.status = t().closed_others.to_string();
    }

    fn request_close_others(&mut self, keep: usize) {
        if keep >= self.win().tabs.len() {
            return;
        }
        let others_dirty = self
            .win()
            .tabs
            .iter()
            .enumerate()
            .any(|(i, t)| i != keep && t.doc.dirty);
        if others_dirty {
            self.dialog = Some(Dialog::CloseOthers(keep));
        } else {
            self.close_other_tabs(keep);
        }
    }

    fn copy_active_path(&mut self) {
        let Some(tab) = self.win().active_tab() else {
            return;
        };
        let Some(path) = &tab.doc.path else {
            self.status = t().doc_not_saved.to_string();
            return;
        };
        match crate::io::clipboard::copy_text(&path.display().to_string()) {
            Ok(()) => self.status = t().copied_path.to_string(),
            Err(e) => self.status = i18n::copy_path_fail(e),
        }
    }

    fn reveal_active(&mut self) {
        let Some(tab) = self.win().active_tab() else {
            return;
        };
        let Some(path) = tab.doc.path.clone() else {
            self.status = t().doc_not_saved.to_string();
            return;
        };
        if !path.exists() {
            self.status = t().file_missing.to_string();
            return;
        }
        #[cfg(windows)]
        {
            let arg = format!("/select,{}", path.display());
            if std::process::Command::new("explorer")
                .arg(&arg)
                .spawn()
                .is_err()
            {
                self.status = t().cannot_open_explorer.to_string();
            }
        }
        #[cfg(not(windows))]
        {
            let dir = path.parent().unwrap_or(path.as_path());
            if opener::open(dir).is_err() {
                self.status = t().cannot_open_dir.to_string();
            }
        }
    }

    fn apply_menu(&mut self, cmd: MenuCmd, ctx: &egui::Context) {
        match cmd {
            MenuCmd::New => self.new_untitled(),
            MenuCmd::Open => self.pick_open(),
            MenuCmd::OpenFolder => self.pick_open_folder(),
            MenuCmd::Save => {
                let _ = self.save_active(false);
            }
            MenuCmd::SaveAs => {
                let _ = self.save_active(true);
            }
            MenuCmd::Close => {
                if !self.win().tabs.is_empty() {
                    let idx = self.win().active;
                    self.request_close_tab(idx);
                }
            }
            MenuCmd::CloseAll => self.request_close_all(),
            MenuCmd::Reopen => self.reopen_closed(),
            MenuCmd::CopyPath => self.copy_active_path(),
            MenuCmd::Reveal => self.reveal_active(),
            MenuCmd::Exit => ctx.send_viewport_cmd(ViewportCommand::Close),
            MenuCmd::Mode(m) => {
                if let Some(tab) = self.win_mut().active_tab_mut() {
                    tab.set_mode(m);
                }
            }
            MenuCmd::Toggle => {
                if let Some(tab) = self.win_mut().active_tab_mut() {
                    tab.toggle_preview_edit();
                }
            }
            MenuCmd::Settings => self.open_settings(),
            MenuCmd::About => self.dialog = Some(Dialog::About),
            MenuCmd::Sidebar => self.toggle_sidebar(),
            MenuCmd::NavBack => self.nav_back(),
            MenuCmd::NavFwd => self.nav_fwd(),
            MenuCmd::Find => self.open_find(),
            MenuCmd::OpenRecent(p) => self.open_recent(p),
            MenuCmd::ClearRecent => {
                self.settings.recent_files.clear();
                self.settings.save();
                self.status = t().cleared_recent.to_string();
            }
            MenuCmd::Lang(lang) => self.apply_lang(lang),
        }
    }

    fn toggle_sidebar(&mut self) {
        let win = self.win_mut();
        win.sidebar_open = !win.sidebar_open;
        self.persist_sidebar();
    }

    fn open_find(&mut self) {
        let Some(tab) = self.win_mut().active_tab_mut() else {
            return;
        };
        if !tab.find.open {
            tab.find.open = true;
            tab.find.focus = true;
        } else {
            tab.find.focus = true;
        }
        if tab.find.query.is_empty() && tab.sel_chars > 0 {
            let a = tab.sel_start.min(tab.sel_end);
            let b = tab.sel_start.max(tab.sel_end);
            let t = &tab.doc.text;
            let b0 = t.char_indices().nth(a).map(|(i, _)| i).unwrap_or(0);
            let b1 = t.char_indices().nth(b).map(|(i, _)| i).unwrap_or(t.len());
            if b0 < b1 {
                tab.find.query = t[b0..b1].to_string();
            }
        }
        let text = tab.doc.text.clone();
        tab.find.recompute(&text);
    }

    fn find_step(&mut self, next: bool) {
        let Some(tab) = self.win_mut().active_tab_mut() else {
            return;
        };
        tab.find.open = true;
        if tab.find.hits.is_empty() {
            let text = tab.doc.text.clone();
            tab.find.recompute(&text);
        }
        if next {
            tab.find.next();
        } else {
            tab.find.prev();
        }
        if let Some(line) = tab.find.current().map(|h| h.line) {
            tab.request_jump(line);
            self.status = i18n::find_status(tab.find.cur + 1, tab.find.hits.len().max(1));
        } else if !tab.find.query.trim().is_empty() {
            self.status = t().no_match.to_string();
        }
    }

    fn persist_sidebar(&mut self) {
        let vis = self.win().sidebar_open;
        let width = self.win().sidebar_width.round() as i32;
        if self.settings.side_panel_visible != vis || self.settings.side_panel_width != width {
            self.settings.side_panel_visible = vis;
            self.settings.side_panel_width = width;
            self.settings.normalize();
            self.settings.save();
        }
    }

    fn pick_open(&mut self) {
        let picked = rfd::FileDialog::new()
            .add_filter(
                t().filter_docs,
                &[
                    "md", "markdown", "txt", "docx", "doc", "pdf", "lnk", "png", "jpg", "jpeg",
                    "gif", "bmp", "ico", "tif", "tiff", "webp",
                ],
            )
            .add_filter("Markdown", &["md", "markdown", "txt"])
            .add_filter("Word", &["doc", "docx"])
            .add_filter("PDF", &["pdf"])
            .add_filter(
                t().filter_images,
                &["png", "jpg", "jpeg", "gif", "bmp", "ico", "tif", "tiff", "webp"],
            )
            .add_filter(t().filter_shortcut, &["lnk"])
            .add_filter(t().filter_all, &["*"])
            .pick_files();
        if let Some(paths) = picked {
            for p in paths {
                if let Err(e) = self.open_incoming(&p) {
                    self.dialog = Some(Dialog::Error(e));
                    break;
                }
            }
        }
    }

    fn pick_open_folder(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            if let Err(e) = self.open_incoming(&dir) {
                self.dialog = Some(Dialog::Error(e));
            }
        }
    }

    fn save_active(&mut self, save_as: bool) -> bool {
        let Some(tab) = self.win().active_tab() else {
            return true;
        };
        if tab.kind == DocKind::Pdf {
            self.status = t().pdf_readonly_save.to_string();
            return false;
        }
        if tab.kind == DocKind::Image {
            if !save_as {
                self.status = t().image_readonly_save.to_string();
                return false;
            }
            return self.save_image_as();
        }
        if tab.kind == DocKind::Word && !save_as {
            self.status = t().word_readonly_save.to_string();
            return false;
        }
        let need_dialog = save_as || tab.doc.path.is_none() || tab.kind != DocKind::Markdown;
        let path = if need_dialog {
            let mut dlg = rfd::FileDialog::new().add_filter("Markdown", &["md"]);
            if let Some(p) = &tab.doc.path {
                if let Some(parent) = p.parent() {
                    dlg = dlg.set_directory(parent);
                }
                if let Some(name) = p.file_stem() {
                    dlg = dlg.set_file_name(format!("{}.md", name.to_string_lossy()));
                }
            } else {
                dlg = dlg.set_file_name(t().untitled_md);
            }
            match dlg.save_file() {
                Some(p) => p,
                None => return false,
            }
        } else {
            tab.doc.path.clone().unwrap()
        };
        let win = self.win_mut();
        let Some(tab) = win.active_tab_mut() else {
            return true;
        };
        match file::write_text(&path, &tab.doc.text, tab.doc.newline, &tab.doc.enc) {
            Ok(()) => {
                tab.doc.path = Some(path.clone());
                tab.doc.mark_clean();
                if file::is_text_ext(&path) {
                    tab.kind = DocKind::Markdown;
                    tab.asset_dir = None;
                }
                self.watch.ignore(&path);
                self.sync_watch();
                self.remember_file(&path);
                self.status = i18n::saved(&file_label(&path));
                true
            }
            Err(e) => {
                self.dialog = Some(Dialog::Error(e));
                false
            }
        }
    }

    fn poll_incoming(&mut self, ctx: &egui::Context) {
        let Some(inc) = self.incoming.as_ref() else {
            return;
        };
        let batches = inc.poll();
        if batches.is_empty() {
            return;
        }
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        let mut nfile = 0u32;
        for batch in batches {
            for p in batch {
                match self.open_incoming(&p) {
                    Ok(()) => nfile += 1,
                    Err(e) => {
                        self.status = e.clone();
                        self.dialog = Some(Dialog::Error(e));
                    }
                }
            }
        }
        if nfile > 0 {
            self.status = i18n::opened_n_files(nfile);
        }
    }

    fn handle_dropped(&mut self, ctx: &egui::Context) {
        self.drop_hint = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let dropped: Vec<egui::DroppedFile> = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        for f in dropped {
            if let Err(e) = self.open_dropped(&f) {
                self.status = e.clone();
                self.dialog = Some(Dialog::Error(e));
            }
        }
    }

    fn open_dropped(&mut self, f: &egui::DroppedFile) -> Result<(), String> {
        if let Some(path) = f.path.as_ref() {
            return self.open_incoming(path);
        }
        let bytes = f
            .bytes
            .as_ref()
            .ok_or_else(|| t().drop_no_content.to_string())?;
        let name = if f.name.is_empty() {
            "dropped.bin".to_string()
        } else {
            Path::new(&f.name)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "dropped.bin".into())
        };
        let name: String = name
            .chars()
            .map(|c| {
                if "<>:\"/\\|?*".contains(c) {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let dir = std::env::temp_dir().join("rustmarkdown-drop");
        std::fs::create_dir_all(&dir).map_err(|e| i18n::tmp_dir_fail(e))?;
        let dest = dir.join(name);
        std::fs::write(&dest, bytes.as_ref()).map_err(|e| i18n::tmp_write_fail(e))?;
        self.open_incoming(&dest)
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let mut new_file = false;
        let mut open = false;
        let mut save = false;
        let mut save_as = false;
        let mut close = false;
        let mut reopen = false;
        let mut next = false;
        let mut prev = false;
        let mut mode = None;
        let mut toggle = false;
        let mut open_folder = false;
        let mut open_settings = false;
        let mut toggle_sidebar = false;
        let mut nav_back = false;
        let mut nav_fwd = false;
        let mut find_open = false;
        let mut find_next = false;
        let mut find_prev = false;
        let mut find_close = false;
        ctx.input_mut(|i| {
            if i.key_pressed(Key::F4) && !i.modifiers.alt && !i.modifiers.ctrl {
                i.consume_key(Modifiers::NONE, Key::F4);
                toggle_sidebar = true;
            }
            if i.key_pressed(Key::F3) && !i.modifiers.ctrl && !i.modifiers.alt {
                if i.modifiers.shift {
                    i.consume_key(Modifiers::SHIFT, Key::F3);
                    find_prev = true;
                } else {
                    i.consume_key(Modifiers::NONE, Key::F3);
                    find_next = true;
                }
            }
            if i.key_pressed(Key::Escape) {
                find_close = true;
            }
            if i.modifiers.alt && !i.modifiers.ctrl && !i.modifiers.shift {
                if i.consume_key(Modifiers::ALT, Key::ArrowLeft) {
                    nav_back = true;
                } else if i.consume_key(Modifiers::ALT, Key::ArrowRight) {
                    nav_fwd = true;
                }
            }
            let ctrl = i.modifiers.command || i.modifiers.ctrl;
            if !ctrl {
                return;
            }
            if i.consume_key(Modifiers::CTRL, Key::F) {
                find_open = true;
            } else if i.consume_key(Modifiers::CTRL, Key::N) {
                new_file = true;
            } else if i.modifiers.shift && i.key_pressed(Key::O) {
                i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::O);
                open_folder = true;
            } else if i.consume_key(Modifiers::CTRL, Key::O) {
                open = true;
            } else if i.modifiers.shift && i.key_pressed(Key::S) {
                i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::S);
                save_as = true;
            } else if i.consume_key(Modifiers::CTRL, Key::S) {
                save = true;
            } else if i.consume_key(Modifiers::CTRL, Key::W) {
                close = true;
            } else if i.modifiers.shift && i.key_pressed(Key::T) {
                i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::T);
                reopen = true;
            } else if i.consume_key(Modifiers::CTRL, Key::E) {
                toggle = true;
            } else if i.consume_key(Modifiers::CTRL, Key::Comma) {
                open_settings = true;
            } else if i.consume_key(Modifiers::CTRL, Key::Num1) {
                mode = Some(ViewMode::Code);
            } else if i.consume_key(Modifiers::CTRL, Key::Num2) {
                mode = Some(ViewMode::Side);
            } else if i.consume_key(Modifiers::CTRL, Key::Num3) {
                mode = Some(ViewMode::Preview);
            } else if i.key_pressed(Key::Tab) {
                if i.modifiers.shift {
                    i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab);
                    prev = true;
                } else {
                    i.consume_key(Modifiers::CTRL, Key::Tab);
                    next = true;
                }
            }
        });

        if new_file {
            self.new_untitled();
        }
        if open {
            self.pick_open();
        }
        if open_folder {
            self.pick_open_folder();
        }
        if save {
            let _ = self.save_active(false);
        }
        if save_as {
            let _ = self.save_active(true);
        }
        if close {
            let idx = self.win().active;
            if !self.win().tabs.is_empty() {
                self.request_close_tab(idx);
            }
        }
        if reopen {
            self.reopen_closed();
        }
        if next || prev {
            let n = self.win().tabs.len();
            if n > 0 {
                let a = self.win().active;
                self.win_mut().active = if next { (a + 1) % n } else { (a + n - 1) % n };
            }
        }
        if let Some(m) = mode {
            if let Some(tab) = self.win_mut().active_tab_mut() {
                tab.set_mode(m);
            }
        }
        if toggle {
            if let Some(tab) = self.win_mut().active_tab_mut() {
                tab.toggle_preview_edit();
            }
        }
        if open_settings {
            self.open_settings();
        }
        if toggle_sidebar {
            self.toggle_sidebar();
        }
        if nav_back {
            self.nav_back();
        }
        if nav_fwd {
            self.nav_fwd();
        }
        if find_open {
            self.open_find();
        }
        if find_next {
            self.find_step(true);
        }
        if find_prev {
            self.find_step(false);
        }
        if find_close {
            if let Some(tab) = self.win_mut().active_tab_mut() {
                tab.find.open = false;
            }
        }
    }

    fn open_settings(&mut self) {
        self.settings_draft = Some(self.settings.clone());
    }

    fn apply_settings(&mut self, mut draft: Settings) {
        draft.normalize();
        self.settings = draft;
        self.settings.save();
        i18n::set(self.settings.ui_lang);
        crate::io::log::set_enabled(self.settings.enable_logs);
        let ts = self.settings.md_tab_size;
        for win in &mut self.wins {
            for tab in &mut win.tabs {
                tab.reparse(ts);
            }
        }
        self.status = t().settings_saved.to_string();
    }

    fn apply_lang(&mut self, lang: Lang) {
        self.settings.ui_lang = lang;
        i18n::set(lang);
        self.settings.save();
        if let Some(d) = self.settings_draft.as_mut() {
            d.ui_lang = lang;
        }
    }

    fn preview_opts(&self) -> PreviewOpts {
        PreviewOpts {
            heading_auto_number: self.settings.md_heading_auto_number,
            tab_size: self.settings.md_tab_size,
            img_max_width: self.settings.md_img_max_width,
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        let want_close = ctx.input(|i| i.viewport().close_requested());
        if !want_close {
            return;
        }
        let dirty = self.win().tabs.iter().any(|t| t.doc.dirty);
        if dirty {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::Quit);
        }
    }

    fn show_menubar(&mut self, ui: &mut egui::Ui) {
        let has_tab = !self.win().tabs.is_empty();
        let kind = self.win().active_tab().map(|t| t.kind);
        let can_save = kind == Some(DocKind::Markdown);
        let can_save_as = has_tab && kind != Some(DocKind::Pdf);
        let can_mode = kind == Some(DocKind::Markdown);
        let has_path = self
            .win()
            .active_tab()
            .and_then(|t| t.doc.path.as_ref())
            .is_some();
        let can_reopen = !self.closed_stack.is_empty();
        let cur = self.win().active_tab().map(|t| t.mode);
        let mut cmd = None;

        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(t().menu_file, |ui| {
                if menu_item(ui, t().new, "Ctrl+N", true) {
                    cmd = Some(MenuCmd::New);
                }
                if menu_item(ui, t().open_ellipsis, "Ctrl+O", true) {
                    cmd = Some(MenuCmd::Open);
                }
                if menu_item(ui, t().open_folder_ellipsis, "Ctrl+Shift+O", true) {
                    cmd = Some(MenuCmd::OpenFolder);
                }
                ui.menu_button(t().recent_files, |ui| {
                    if self.settings.recent_files.is_empty() {
                        ui.label(RichText::new(t().recent_empty).weak());
                    } else {
                        let recents = self.settings.recent_files.clone();
                        for p in recents {
                            let name = file_label(&p);
                            let parent = p
                                .parent()
                                .map(|d| d.display().to_string())
                                .unwrap_or_default();
                            let label = if parent.is_empty() {
                                name
                            } else {
                                format!("{name}  —  {parent}")
                            };
                            if ui
                                .button(RichText::new(label).size(13.0))
                                .on_hover_text(p.display().to_string())
                                .clicked()
                            {
                                cmd = Some(MenuCmd::OpenRecent(p));
                                ui.close();
                            }
                        }
                        ui.separator();
                        if menu_item(ui, t().clear_recent, "", true) {
                            cmd = Some(MenuCmd::ClearRecent);
                        }
                    }
                });
                ui.separator();
                if menu_item(ui, t().save, "Ctrl+S", can_save) {
                    cmd = Some(MenuCmd::Save);
                }
                if menu_item(ui, t().save_as, "Ctrl+Shift+S", can_save_as) {
                    cmd = Some(MenuCmd::SaveAs);
                }
                ui.separator();
                if menu_item(ui, t().close, "Ctrl+W", has_tab) {
                    cmd = Some(MenuCmd::Close);
                }
                if menu_item(ui, t().close_all, "", has_tab) {
                    cmd = Some(MenuCmd::CloseAll);
                }
                if menu_item(ui, t().reopen_tab, "Ctrl+Shift+T", can_reopen) {
                    cmd = Some(MenuCmd::Reopen);
                }
                ui.separator();
                if menu_item(ui, t().copy_file_path, "", has_path) {
                    cmd = Some(MenuCmd::CopyPath);
                }
                if menu_item(ui, t().reveal_in_explorer, "", has_path) {
                    cmd = Some(MenuCmd::Reveal);
                }
                ui.separator();
                if menu_item(ui, t().exit, "Alt+F4", true) {
                    cmd = Some(MenuCmd::Exit);
                }
            });
            ui.menu_button(t().menu_view, |ui| {
                if menu_item(ui, t().back, "Alt+←", self.win().nav.can_back()) {
                    cmd = Some(MenuCmd::NavBack);
                }
                if menu_item(ui, t().forward, "Alt+→", self.win().nav.can_fwd()) {
                    cmd = Some(MenuCmd::NavFwd);
                }
                if menu_item(ui, t().find, "Ctrl+F", has_tab) {
                    cmd = Some(MenuCmd::Find);
                }
                ui.separator();
                if menu_check(ui, t().mode_code, "Ctrl+1", can_mode, cur == Some(ViewMode::Code)) {
                    cmd = Some(MenuCmd::Mode(ViewMode::Code));
                }
                if menu_check(
                    ui,
                    t().mode_side,
                    "Ctrl+2",
                    can_mode,
                    cur == Some(ViewMode::Side),
                ) {
                    cmd = Some(MenuCmd::Mode(ViewMode::Side));
                }
                if menu_check(
                    ui,
                    t().mode_preview,
                    "Ctrl+3",
                    can_mode,
                    cur == Some(ViewMode::Preview),
                ) {
                    cmd = Some(MenuCmd::Mode(ViewMode::Preview));
                }
                ui.separator();
                if menu_item(ui, t().toggle_preview_edit, "Ctrl+E", can_mode) {
                    cmd = Some(MenuCmd::Toggle);
                }
                ui.separator();
                if menu_check(ui, t().sidebar, "F4", true, self.win().sidebar_open) {
                    cmd = Some(MenuCmd::Sidebar);
                }
                ui.separator();
                ui.menu_button(t().menu_language, |ui| {
                    if menu_check(ui, t().lang_zh, "", true, i18n::get() == Lang::Zh) {
                        cmd = Some(MenuCmd::Lang(Lang::Zh));
                    }
                    if menu_check(ui, t().lang_en, "", true, i18n::get() == Lang::En) {
                        cmd = Some(MenuCmd::Lang(Lang::En));
                    }
                });
            });
            ui.menu_button(t().menu_tools, |ui| {
                if menu_item(ui, t().settings_ellipsis, "Ctrl+,", true) {
                    cmd = Some(MenuCmd::Settings);
                }
            });
            ui.menu_button(t().menu_help, |ui| {
                if menu_item(ui, t().about_app, "", true) {
                    cmd = Some(MenuCmd::About);
                }
            });
        });

        if let Some(c) = cmd {
            self.apply_menu(c, ui.ctx());
        }
    }

    fn show_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 28.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if view::icons::button(ui, Icon::New, false, t().tip_new).clicked() {
                    self.new_untitled();
                }
                if view::icons::button(ui, Icon::Open, false, t().tip_open).clicked() {
                    self.pick_open();
                }
                let kind = self.win().active_tab().map(|t| t.kind);
                let can_save = kind == Some(DocKind::Markdown);
                let can_save_as = kind.is_some() && kind != Some(DocKind::Pdf);
                let can_mode = kind == Some(DocKind::Markdown);
                ui.add_enabled_ui(can_save, |ui| {
                    if view::icons::button(ui, Icon::Save, false, t().tip_save).clicked() {
                        let _ = self.save_active(false);
                    }
                });
                ui.add_enabled_ui(can_save_as, |ui| {
                    if view::icons::button(ui, Icon::SaveAs, false, t().tip_save_as)
                        .clicked()
                    {
                        let _ = self.save_active(true);
                    }
                });
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                let can_back = self.win().nav.can_back();
                let can_fwd = self.win().nav.can_fwd();
                let mut go_back = false;
                let mut go_fwd = false;
                ui.add_enabled_ui(can_back, |ui| {
                    if view::icons::button(ui, Icon::Back, false, t().tip_back).clicked() {
                        go_back = true;
                    }
                });
                ui.add_enabled_ui(can_fwd, |ui| {
                    if view::icons::button(ui, Icon::Forward, false, t().tip_forward).clicked() {
                        go_fwd = true;
                    }
                });
                if go_back {
                    self.nav_back();
                }
                if go_fwd {
                    self.nav_fwd();
                }
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                let mut set_mode = None;
                let cur = self.win().active_tab().map(|t| t.mode);
                ui.add_enabled_ui(can_mode, |ui| {
                    if view::icons::button(ui, Icon::Code, cur == Some(ViewMode::Code), t().tip_code)
                        .clicked()
                    {
                        set_mode = Some(ViewMode::Code);
                    }
                    if view::icons::button(
                        ui,
                        Icon::Side,
                        cur == Some(ViewMode::Side),
                        t().tip_side,
                    )
                    .clicked()
                    {
                        set_mode = Some(ViewMode::Side);
                    }
                    if view::icons::button(
                        ui,
                        Icon::Preview,
                        cur == Some(ViewMode::Preview),
                        t().tip_preview,
                    )
                    .clicked()
                    {
                        set_mode = Some(ViewMode::Preview);
                    }
                });
                ui.add_space(4.0);
                let toc_on = self.win().sidebar_open;
                if view::icons::button(ui, Icon::Toc, toc_on, t().tip_sidebar).clicked() {
                    self.toggle_sidebar();
                }
                if let Some(m) = set_mode {
                    if let Some(tab) = self.win_mut().active_tab_mut() {
                        tab.set_mode(m);
                    }
                }
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                if view::icons::button(ui, Icon::Settings, false, t().tip_settings).clicked()
                {
                    self.open_settings();
                }
            },
        );
    }

    fn show_tabbar(&mut self, ui: &mut egui::Ui) {
        let cur = self.cur.min(self.wins.len().saturating_sub(1));
        if self.wins[cur].tabs.is_empty() {
            return;
        }
        let dragging_id = match &self.tab_drag {
            Some(d)
                if !d.floated && self.wins[cur].tabs.iter().any(|t| t.id == d.tab_id) =>
            {
                Some(d.tab_id)
            }
            _ => None,
        };
        let ghost_x = match (&self.tab_drag, dragging_id) {
            (Some(d), Some(_)) => ui
                .ctx()
                .pointer_latest_pos()
                .map(|p| p.x - d.grab_in_chip.x),
            _ => None,
        };
        let (ev, geom) = tabs::show(
            ui,
            &self.wins[cur].tabs,
            self.wins[cur].active,
            dragging_id,
            ghost_x,
        );
        self.record_tabbar_geom(ui.ctx(), geom.bar_rect, &geom.chips);
        if let Some(ev) = ev {
            match ev {
                TabBarEvent::Select(i) => self.win_mut().active = i,
                TabBarEvent::Close(i) => self.request_close_tab(i),
                TabBarEvent::CloseOthers(i) => self.request_close_others(i),
                TabBarEvent::CloseAll => self.request_close_all(),
                TabBarEvent::Reorder { from, to } => {
                    let win = self.win_mut();
                    if from < win.tabs.len() && to < win.tabs.len() {
                        let tab = win.tabs.remove(from);
                        win.tabs.insert(to, tab);
                        win.active = to;
                    }
                }
                TabBarEvent::TearOff(i) => self.tear_off(i, None),
                TabBarEvent::OpenAsWorkspace(i) => self.open_tab_as_workspace(i),
                TabBarEvent::DragStart { idx, grab } => {
                    if self.tab_drag.is_none() {
                        if let Some(tab) = self.win().tabs.get(idx) {
                            let id = tab.id;
                            self.tab_drag = Some(TabDrag {
                                tab_id: id,
                                grab_in_chip: grab,
                                grab_in_win: egui::Vec2::ZERO,
                                floated: false,
                            });
                            self.win_mut().active = idx;
                        }
                    }
                }
            }
        }
    }

    fn record_tabbar_geom(&mut self, ctx: &egui::Context, bar: egui::Rect, chips: &[egui::Rect]) {
        let (inner, outer) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().outer_rect));
        let w = self.win_mut();
        w.inner_rect = inner;
        w.outer_rect = outer;
        if let Some(inner) = inner {
            w.tabbar_screen = Some(egui::Rect::from_min_max(
                inner.min + bar.min.to_vec2(),
                inner.min + bar.max.to_vec2(),
            ));
            w.chip_mids = chips
                .iter()
                .map(|c| inner.min.x + c.center().x)
                .collect();
        }
    }

    fn note_pointer(&mut self, ctx: &egui::Context) {
        let down = ctx.input(|i| i.pointer.primary_down());
        if down {
            self.saw_ptr_down = true;
            if let Some(p) = viewport_pointer_screen(ctx) {
                self.last_screen_ptr = Some(p);
            }
        }
    }

    fn find_tab_id(&self, id: u64) -> Option<(usize, usize)> {
        for (wi, w) in self.wins.iter().enumerate() {
            if let Some(ti) = w.tabs.iter().position(|t| t.id == id) {
                return Some((wi, ti));
            }
        }
        None
    }

    fn detach_tab(&mut self, wi: usize, ti: usize) -> Option<Tab> {
        if wi >= self.wins.len() || ti >= self.wins[wi].tabs.len() {
            return None;
        }
        let tab = self.wins[wi].tabs.remove(ti);
        let w = &mut self.wins[wi];
        if w.tabs.is_empty() {
            w.active = 0;
            if wi != 0 {
                w.pending_close = true;
            }
        } else if ti < w.active {
            w.active = w.active.saturating_sub(1);
        } else if w.active >= w.tabs.len() {
            w.active = w.tabs.len().saturating_sub(1);
        }
        Some(tab)
    }

    fn attach_tab(&mut self, wi: usize, mut insert: usize, tab: Tab) -> usize {
        let w = &mut self.wins[wi];
        if insert > w.tabs.len() {
            insert = w.tabs.len();
        }
        w.tabs.insert(insert, tab);
        w.active = insert;
        insert
    }

    fn tear_off(&mut self, tab_idx: usize, pos: Option<egui::Pos2>) {
        let wi = self.cur;
        if wi >= self.wins.len() || tab_idx >= self.wins[wi].tabs.len() {
            return;
        }
        if self.wins[wi].tabs.len() <= 1 && wi != 0 {
            return;
        }
        let src_size = self.wins[wi]
            .inner_rect
            .map(|r| r.size())
            .unwrap_or(egui::vec2(960.0, 640.0));
        let Some(tab) = self.detach_tab(wi, tab_idx) else {
            return;
        };
        let pos = pos.or(Some(egui::pos2(120.0, 80.0)));
        let uid = self.alloc_id();
        let vid = ViewportId::from_hash_of(("extra-win", uid));
        let mut w = Win::extra(uid, vid, pos);
        w.create_inner = Some(egui::vec2(
            (src_size.x * 0.85).max(640.0),
            (src_size.y * 0.85).max(420.0),
        ));
        w.tabs.push(tab);
        w.active = 0;
        self.wins.push(w);
        self.status = t().moved_window.to_string();
    }

    fn over_tabstrip(w: &Win, p: egui::Pos2) -> bool {
        let Some(r) = w.tabbar_screen else {
            return false;
        };
        p.x >= r.min.x - 12.0
            && p.x <= r.max.x + 12.0
            && p.y >= r.min.y - 10.0
            && p.y <= r.max.y + 14.0
    }

    fn tick_tab_drag(&mut self, ctx: &egui::Context) {
        let Some(screen) = self.last_screen_ptr else {
            return;
        };
        let Some(drag) = self.tab_drag.as_ref() else {
            return;
        };
        let tab_id = drag.tab_id;
        let floated = drag.floated;
        let grab_in_win = drag.grab_in_win;
        let grab_in_chip = drag.grab_in_chip;
        ctx.request_repaint();

        if floated {
            if let Some((wi, _)) = self.find_tab_id(tab_id) {
                let pos = screen - grab_in_win;
                self.wins[wi].follow_pos = Some(pos);
                ctx.send_viewport_cmd_to(
                    self.wins[wi].viewport_id,
                    ViewportCommand::OuterPosition(pos),
                );
            }
            let mut target = None;
            for (wi, w) in self.wins.iter().enumerate() {
                if w.tabs.iter().any(|t| t.id == tab_id) {
                    continue;
                }
                if w.pending_close {
                    continue;
                }
                if !Self::over_tabstrip(w, screen) {
                    continue;
                }
                let insert = tabs::insert_index(&w.chip_mids, None, screen.x);
                target = Some((wi, insert));
                break;
            }
            if let Some((wi, insert)) = target {
                self.merge_dragged_tab(wi, insert);
            }
            return;
        }

        let Some((src, _ti)) = self.find_tab_id(tab_id) else {
            self.tab_drag = None;
            return;
        };
        let mut hit: Option<(usize, usize)> = None;
        for (wi, w) in self.wins.iter().enumerate() {
            if w.pending_close {
                continue;
            }
            if !Self::over_tabstrip(w, screen) {
                continue;
            }
            let exclude = if wi == src {
                self.wins[wi].tabs.iter().position(|t| t.id == tab_id)
            } else {
                None
            };
            let insert = tabs::insert_index(&w.chip_mids, exclude, screen.x);
            hit = Some((wi, insert));
            break;
        }
        if let Some((wi, insert)) = hit {
            if wi != src {
                self.merge_dragged_tab(wi, insert);
            }
            return;
        }
        if self.wins[src].tabs.len() > 1 {
            self.undock_mid_drag(screen, grab_in_chip);
        }
    }

    fn undock_mid_drag(&mut self, screen: egui::Pos2, grab_in_chip: egui::Vec2) {
        let Some(drag) = self.tab_drag.as_ref() else {
            return;
        };
        if drag.floated {
            return;
        }
        let tab_id = drag.tab_id;
        let Some((wi, ti)) = self.find_tab_id(tab_id) else {
            return;
        };
        if self.wins[wi].tabs.len() <= 1 {
            return;
        }
        let outer = self.wins[wi]
            .outer_rect
            .or(self.wins[wi].inner_rect)
            .unwrap_or(egui::Rect::from_min_size(screen, egui::vec2(1.0, 1.0)));
        let grab_y = self.wins[wi]
            .tabbar_screen
            .map(|tb| (tb.center().y - outer.min.y).clamp(8.0, 96.0))
            .unwrap_or(18.0);
        let grab = egui::vec2(grab_in_chip.x.max(8.0) + 16.0, grab_y);
        let src_size = self.wins[wi]
            .inner_rect
            .map(|r| r.size())
            .unwrap_or(egui::vec2(960.0, 640.0));
        let Some(tab) = self.detach_tab(wi, ti) else {
            return;
        };
        let pos = screen - grab;
        let uid = self.alloc_id();
        let vid = ViewportId::from_hash_of(("extra-win", uid));
        let mut w = Win::extra(uid, vid, Some(pos));
        w.follow_pos = Some(pos);
        w.create_inner = Some(egui::vec2(
            (src_size.x * 0.85).max(640.0),
            (src_size.y * 0.85).max(420.0),
        ));
        w.tabs.push(tab);
        w.active = 0;
        self.wins.push(w);
        if let Some(d) = self.tab_drag.as_mut() {
            d.floated = true;
            d.grab_in_win = grab;
        }
        self.status = t().moved_window.to_string();
    }

    fn merge_dragged_tab(&mut self, target_wi: usize, insert: usize) {
        let Some(drag) = self.tab_drag.as_ref() else {
            return;
        };
        let tab_id = drag.tab_id;
        let Some((src, ti)) = self.find_tab_id(tab_id) else {
            return;
        };
        if src == target_wi {
            return;
        }
        let Some(tab) = self.detach_tab(src, ti) else {
            return;
        };
        self.attach_tab(target_wi, insert, tab);
        for w in &mut self.wins {
            w.follow_pos = None;
        }
        if let Some(d) = self.tab_drag.as_mut() {
            d.floated = false;
        }
        self.status = t().merged_window.to_string();
    }

    fn end_tab_drag(&mut self) {
        if let Some(d) = self.tab_drag.take() {
            if let Some((wi, _)) = self.find_tab_id(d.tab_id) {
                self.wins[wi].follow_pos = None;
            }
        }
        for w in &mut self.wins {
            w.follow_pos = None;
        }
    }

    fn show_content(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        ui.advance_cursor_after_rect(full);
        let mut content = full;
        if self.win().sidebar_open {
            let split_w = 4.0;
            let w = self.win().sidebar_width.clamp(140.0, 480.0);
            let side = egui::Rect::from_min_size(full.min, egui::vec2(w, full.height()));
            let split = egui::Rect::from_min_size(
                egui::pos2(side.right(), full.top()),
                egui::vec2(split_w, full.height()),
            );
            content = egui::Rect::from_min_max(egui::pos2(split.right(), full.top()), full.max);

            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(side)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    ui.set_clip_rect(side);
                    ui.set_min_size(side.size());
                    ui.set_max_size(side.size());
                    let t_side = std::time::Instant::now();
                    self.show_sidebar(ui);
                    crate::io::log::slow("sidebar", t_side, crate::io::log::SPAN_MS);
                },
            );

            let resp = ui.interact(split, ui.id().with("outline_split"), egui::Sense::drag());
            let vis = ui.visuals();
            ui.painter().rect_filled(
                split,
                0.0,
                if resp.hovered() || resp.dragged() {
                    vis.selection.bg_fill
                } else {
                    vis.widgets.noninteractive.bg_stroke.color
                },
            );
            if resp.dragged() {
                let nw = (w + resp.drag_delta().x).clamp(140.0, 480.0);
                self.win_mut().sidebar_width = nw;
            }
            if resp.drag_stopped() {
                self.persist_sidebar();
            }
            resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        }

        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(content)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                ui.set_clip_rect(content);
                ui.set_min_size(content.size());
                ui.set_max_size(content.size());
                let t_doc = std::time::Instant::now();
                self.show_doc_pane(ui);
                crate::io::log::slow("doc_pane", t_doc, crate::io::log::SPAN_MS);
            },
        );
    }

    fn show_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let cur = self.win().sidebar_tab;
            if ui
                .selectable_label(cur == SidebarTab::Explorer, t().explorer)
                .clicked()
            {
                self.win_mut().sidebar_tab = SidebarTab::Explorer;
            }
            if ui
                .selectable_label(cur == SidebarTab::Outline, t().outline)
                .clicked()
            {
                self.win_mut().sidebar_tab = SidebarTab::Outline;
            }
        });
        ui.separator();
        match self.win().sidebar_tab {
            SidebarTab::Explorer => self.show_explorer_pane(ui),
            SidebarTab::Outline => self.show_outline_pane(ui),
        }
    }

    fn show_explorer_pane(&mut self, ui: &mut egui::Ui) {
        if self.win().workspace.is_none() {
            ui.add_space(8.0);
            ui.label(
                RichText::new(t().no_folder)
                    .size(12.0)
                    .color(Color32::from_rgb(0x88, 0x88, 0x88)),
            );
            ui.add_space(6.0);
            if ui.button(t().open_folder_btn).clicked() {
                self.pick_open_folder();
            }
            return;
        }
        let mut action = None;
        if let Some(ws) = self.win_mut().workspace.as_mut() {
            action = workspace::show(ui, ws);
        }
        match action {
            Some(ExplorerAction::Open(p)) => {
                if file::is_openable_file(&p) {
                    if let Err(e) = self.open_path(self.cur, &p) {
                        self.dialog = Some(Dialog::Error(e));
                    }
                } else if let Err(_) = opener::open(&p) {
                    self.status = i18n::cannot_open(&file_label(&p));
                }
            }
            Some(ExplorerAction::Reveal(p)) => {
                self.reveal_path(&p);
            }
            Some(ExplorerAction::CopyPath(p)) => {
                match crate::io::clipboard::copy_text(&p.display().to_string()) {
                    Ok(()) => self.status = t().copied_path_ok.to_string(),
                    Err(e) => self.status = e,
                }
            }
            Some(ExplorerAction::Refresh) => {
                if let Some(ws) = self.win_mut().workspace.as_mut() {
                    ws.refresh();
                }
            }
            Some(ExplorerAction::RootChanged) => {
                if let Some(ws) = self.win().workspace.as_ref() {
                    self.status = i18n::opened_folder(&file_label(&ws.root));
                }
            }
            Some(ExplorerAction::BadPath) => {
                self.status = t().bad_folder_path.to_string();
            }
            None => {}
        }
    }

    fn reveal_path(&mut self, path: &Path) {
        #[cfg(windows)]
        {
            let arg = format!("/select,{}", path.display());
            if std::process::Command::new("explorer").arg(&arg).spawn().is_err()
            {
                self.status = t().cannot_open_explorer.to_string();
            }
        }
        #[cfg(not(windows))]
        {
            if let Some(dir) = path.parent() {
                let _ = opener::open(dir);
            }
        }
    }

    fn show_outline_pane(&mut self, ui: &mut egui::Ui) {
        let auto_num = self.settings.md_heading_auto_number;
        let in_cooldown = match self.win().ignore_outline_until {
            Some(t) => {
                if Instant::now() < t {
                    true
                } else {
                    self.win_mut().ignore_outline_until = None;
                    false
                }
            }
            None => false,
        };

        let mut filter = std::mem::take(&mut self.win_mut().outline_filter);
        let mut last_hl = self.win().outline_hl;
        let mut action = None;
        {
            let sync_line = self.win().outline_sync_line;
            let win = self.win_mut();
            if let Some(tab) = win.active_tab_mut() {
                let entries = if tab.kind == DocKind::Pdf {
                    view::outline::collect_pages(
                        tab.pdf.as_ref().map(|p| p.page_count as usize).unwrap_or(0),
                    )
                } else {
                    view::outline::collect(&tab.md, auto_num)
                };
                let current = if tab.kind == DocKind::Pdf {
                    tab.pdf.as_ref().map(|p| p.current_page()).unwrap_or(0)
                } else if tab.mode == ViewMode::Code {
                    tab.editor_top_line
                } else {
                    tab.preview.top_line
                };
                let follow = !in_cooldown && sync_line != Some(current);
                action = view::outline::show(
                    ui,
                    &entries,
                    &mut filter,
                    &mut tab.outline_expanded,
                    &mut tab.outline_inited,
                    Some(current),
                    &mut last_hl,
                    follow,
                );
                if follow {
                    win.outline_sync_line = Some(current);
                }
            } else {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    Color32::from_rgb(0xF3, 0xF3, 0xF3),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(t().no_headings)
                        .size(12.0)
                        .color(Color32::from_rgb(0x88, 0x88, 0x88)),
                );
            }
        }
        self.win_mut().outline_filter = filter;
        self.win_mut().outline_hl = last_hl;
        if let Some(view::outline::OutlineAction::Jump(line)) = action {
            self.nav_record_leave(line);
            self.win_mut().ignore_outline_until = Some(Instant::now() + Duration::from_millis(650));
            self.win_mut().outline_hl = Some(line);
            if let Some(tab) = self.win_mut().active_tab_mut() {
                tab.request_jump(line);
            }
        }
    }

    fn show_doc_pane(&mut self, ui: &mut egui::Ui) {
        if self.win().tabs.is_empty() {
            show_welcome(ui);
            return;
        }
        let active = self.win().active;
        let id = self.win().tabs[active].id;
        ui.push_id(id, |ui| {
            if self.win().tabs[active].kind == DocKind::Pdf {
                self.show_pdf_pane(ui, active);
                return;
            }
            if self.win().tabs[active].kind == DocKind::Image {
                self.show_image_pane(ui, active);
                return;
            }
            let mode = self.win().tabs[active].mode;
            let href = match mode {
                ViewMode::Code => {
                    let tab = &mut self.win_mut().tabs[active];
                    let jump = tab.pending_editor_line.or(tab.pending_jump);
                    let hint = tab.editor_hint_range();
                    let (find_all, find_cur) = if tab.find.open {
                        tab.find.paint_ranges()
                    } else {
                        (Vec::new(), None)
                    };
                    let ed = view::editor::show(
                        ui,
                        &mut tab.doc.text,
                        jump,
                        hint,
                        &find_all,
                        find_cur,
                        &mut tab.text_undo,
                    );
                    Self::apply_editor_out(tab, ed)
                }
                ViewMode::Preview => {
                    self.show_preview_pane(ui, active, None);
                    None
                }
                ViewMode::Side => self.show_side(ui, active),
            };
            if let Some(link) = href {
                match link {
                    SrcLink::Href(h) => self.open_href(&h),
                    SrcLink::Image { href, alt } => self.open_src_image(ui.ctx(), &href, &alt),
                }
            }
        });
        if let Some(tab) = self.win_mut().active_tab_mut() {
            tab.tick_jump();
        }
    }

    fn show_side(&mut self, ui: &mut egui::Ui, active: usize) -> Option<SrcLink> {
        let full = ui.available_rect_before_wrap();
        ui.advance_cursor_after_rect(full);
        let height = full.height();
        let avail = full.width();
        let ratio = self.win().tabs[active].split_ratio.clamp(0.2, 0.8);
        let split_w = 6.0;
        let left_w = ((avail - split_w) * ratio).max(80.0);
        let left = egui::Rect::from_min_size(full.min, egui::vec2(left_w, height));
        let split = egui::Rect::from_min_size(
            egui::pos2(left.right(), full.top()),
            egui::vec2(split_w, height),
        );
        let right = egui::Rect::from_min_max(egui::pos2(split.right(), full.top()), full.max);

        let mut ed_off = 0.0f32;
        let mut ed_hovered = false;
        let mut ignore_sync = false;
        let mut href = None;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(left)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                ui.set_clip_rect(left);
                ui.set_min_size(left.size());
                ui.set_max_size(left.size());
                let tab = &mut self.win_mut().tabs[active];
                let jump = tab.pending_editor_line.or(tab.pending_jump);
                let hint = tab.editor_hint_range();
                let (find_all, find_cur) = if tab.find.open {
                    tab.find.paint_ranges()
                } else {
                    (Vec::new(), None)
                };
                let ed = view::editor::show(
                    ui,
                    &mut tab.doc.text,
                    jump,
                    hint,
                    &find_all,
                    find_cur,
                    &mut tab.text_undo,
                );
                ed_off = ed.offset_y;
                ed_hovered = ed.hovered;
                ignore_sync = ed.ignore_scroll_sync;
                let sel = ed.sel_chars > 0 && ed.hovered;
                href = Self::apply_editor_out(tab, ed);
                if sel {
                    tab.preview.clear_pick();
                }
            },
        );

        let resp = ui.interact(split, ui.id().with("side_split"), egui::Sense::drag());
        let vis = ui.visuals();
        ui.painter().rect_filled(
            split,
            0.0,
            if resp.hovered() || resp.dragged() {
                vis.selection.bg_fill
            } else {
                vis.widgets.noninteractive.bg_stroke.color
            },
        );
        if resp.dragged() {
            let delta = resp.drag_delta().x;
            let tab = &mut self.win_mut().tabs[active];
            tab.split_ratio = ((left_w + delta) / (avail - split_w)).clamp(0.2, 0.8);
        }
        resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);

        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(right)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                ui.set_clip_rect(right);
                ui.set_min_size(right.size());
                ui.set_max_size(right.size());
                let caret = Some(self.win().tabs[active].cursor_line);
                self.show_preview_pane(ui, active, caret);
            },
        );
        self.win_mut().tabs[active].apply_side_sync(ed_off, ed_hovered, ignore_sync);
        href
    }

    fn show_pdf_pane(&mut self, ui: &mut egui::Ui, active: usize) {
        let jump = self.win().tabs[active]
            .pending_preview_line
            .or(self.win().tabs[active].pending_jump);
        let action = if let Some(pdf) = self.win_mut().tabs[active].pdf.as_mut() {
            view::pdf::show(ui, pdf, jump)
        } else {
            view::pdf::PdfAction::None
        };
        if let Some(pdf) = self.win().tabs[active].pdf.as_ref() {
            let page = pdf.current_page();
            let nsel = pdf.sel_chars;
            let tab = &mut self.win_mut().tabs[active];
            tab.preview.top_line = page;
            tab.sel_chars = nsel;
        }
        match action {
            view::pdf::PdfAction::None => {}
            view::pdf::PdfAction::Open(r) => {
                let title = self.win().tabs[active].doc.display_name();
                self.img_overlay = Some(ImgPreview::new(title, r));
            }
            view::pdf::PdfAction::Copy(r) => self.copy_image(&r),
            view::pdf::PdfAction::CopyFile(r) => self.copy_image_file(&r),
            view::pdf::PdfAction::CopyText(t) => {
                ui.ctx().copy_text(t.clone());
                let n = t.chars().filter(|c| !c.is_control()).count();
                self.status = i18n::copied_n_chars(n);
            }
        }
    }

    fn show_image_pane(&mut self, ui: &mut egui::Ui, active: usize) {
        let action = if let Some(img) = self.win_mut().tabs[active].image.as_mut() {
            view::img_view::show(ui, img)
        } else {
            view::img_view::ImgAction::None
        };
        match action {
            view::img_view::ImgAction::None => {}
            view::img_view::ImgAction::Copy => {
                if let Some(r) = self.win().tabs[active]
                    .image
                    .as_ref()
                    .and_then(|i| i.raster.clone())
                {
                    self.copy_image(&r);
                }
            }
            view::img_view::ImgAction::CopyFile => {
                if let Some(r) = self.win().tabs[active]
                    .image
                    .as_ref()
                    .and_then(|i| i.raster.clone())
                {
                    self.copy_image_file(&r);
                }
            }
        }
    }

    fn save_image_as(&mut self) -> bool {
        let (raster, stem, parent) = {
            let Some(tab) = self.win().active_tab() else {
                return false;
            };
            let Some(raster) = tab.image.as_ref().and_then(|i| i.raster.clone()) else {
                self.status = t().image_not_ready.to_string();
                return false;
            };
            let stem = tab
                .doc
                .path
                .as_ref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "image".into());
            let parent = tab.doc.path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf());
            (raster, stem, parent)
        };
        let mut dlg = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("BMP", &["bmp"]);
        if let Some(dir) = parent {
            dlg = dlg.set_directory(dir);
        }
        dlg = dlg.set_file_name(format!("{stem}.png"));
        let Some(mut path) = dlg.save_file() else {
            return false;
        };
        if path.extension().is_none() {
            path.set_extension("png");
        }
        match crate::io::clipboard::save_image(&raster, &path) {
            Ok(()) => {
                self.status = i18n::image_saved(&file_label(&path));
                true
            }
            Err(e) => {
                self.dialog = Some(Dialog::Error(e));
                false
            }
        }
    }

    fn show_preview_pane(&mut self, ui: &mut egui::Ui, active: usize, caret_line: Option<usize>) {
        self.imgcache.poll(ui.ctx());
        self.mermaid.poll(ui.ctx());
        let opts = self.preview_opts();
        let mut events = Vec::new();
        {
            let cur = self.cur;
            let tab = &mut self.wins[cur].tabs[active];
            let path = tab
                .asset_dir
                .as_ref()
                .map(|d| d.join("doc.md"))
                .or_else(|| tab.doc.path.clone());
            let jump = tab.pending_preview_line.or(tab.pending_jump);
            if caret_line.is_some() && tab.sel_chars > 0 {
                if !ui.input(|i| i.pointer.primary_down()) {
                    tab.preview.hint_line0 = Some(tab.sel_line0);
                    tab.preview.hint_line1 = Some(tab.sel_line1);
                    tab.preview.hint_text =
                        sel_hint_text(&tab.doc.text, tab.sel_byte0, tab.sel_byte1);
                }
            } else if tab.find.open && !tab.find.query.trim().is_empty() {
                tab.preview.hint_line0 = Some(0);
                tab.preview.hint_line1 = Some(1_000_000);
                tab.preview.hint_text = tab.find.query.trim().to_string();
            } else {
                tab.preview.hint_line0 = None;
                tab.preview.hint_line1 = None;
                tab.preview.hint_text.clear();
            }
            if tab.kind == DocKind::Word {
                view::preview::show_paged(
                    ui,
                    &tab.md,
                    &mut tab.preview,
                    &mut self.imgcache,
                    &mut self.mermaid,
                    path.as_deref(),
                    &mut events,
                    opts,
                    jump,
                );
            } else {
                view::preview::show(
                    ui,
                    &tab.md,
                    &mut tab.preview,
                    &mut self.imgcache,
                    &mut self.mermaid,
                    path.as_deref(),
                    &mut events,
                    opts,
                    jump,
                    caret_line,
                );
            }
        }
        self.handle_preview_events(events);
    }

    fn copy_image(&mut self, raster: &crate::io::imgcache::Raster) {
        match crate::io::clipboard::copy_image(raster) {
            Ok(()) => self.status = t().copied_image.to_string(),
            Err(e) => self.status = i18n::copy_image_fail(e),
        }
    }

    fn copy_image_file(&mut self, raster: &crate::io::imgcache::Raster) {
        match crate::io::clipboard::copy_as_file(raster) {
            Ok(p) => self.status = i18n::copied_as_file(p.display()),
            Err(e) => self.status = i18n::copy_as_file_fail(e),
        }
    }

    fn show_img_overlay(&mut self, ctx: &egui::Context) {
        let Some(st) = self.img_overlay.as_mut() else {
            return;
        };
        match img_preview::show(ctx, st) {
            Some(OverlayAction::Close) => self.img_overlay = None,
            Some(OverlayAction::CopyImage) => {
                if let Some(st) = self.img_overlay.as_ref() {
                    let r = st.raster.clone();
                    self.copy_image(&r);
                }
            }
            Some(OverlayAction::CopyAsFile) => {
                if let Some(st) = self.img_overlay.as_ref() {
                    let r = st.raster.clone();
                    self.copy_image_file(&r);
                }
            }
            None => {}
        }
    }

    fn apply_editor_out(tab: &mut Tab, ed: view::editor::EditorOut) -> Option<SrcLink> {
        tab.editor_top_line = ed.top_line;
        tab.cursor_line = ed.cursor_line;
        tab.sel_chars = ed.sel_chars;
        tab.sel_start = ed.sel_start;
        tab.sel_end = ed.sel_end;
        tab.sel_line0 = ed.sel_line0;
        tab.sel_line1 = ed.sel_line1;
        tab.sel_byte0 = ed.sel_byte0;
        tab.sel_byte1 = ed.sel_byte1;
        if ed.changed {
            tab.mark_edited();
            if tab.find.open {
                let text = tab.doc.text.clone();
                tab.find.recompute(&text);
            }
        }
        ed.clicked_link
    }

    fn handle_preview_events(&mut self, events: Vec<PreviewEvent>) {
        for e in events {
            match e {
                PreviewEvent::OpenHref(href) => self.open_href(&href),
                PreviewEvent::OpenImage { raster, title } => {
                    self.img_overlay = Some(ImgPreview::new(title, raster));
                }
                PreviewEvent::CopyImage(r) => self.copy_image(&r),
                PreviewEvent::CopyAsFile(r) => self.copy_image_file(&r),
            }
        }
    }

    fn img_title(alt: &str, href: &str) -> String {
        if !alt.trim().is_empty() {
            return alt.trim().to_string();
        }
        href.rsplit(['/', '\\'])
            .next()
            .unwrap_or(t().image)
            .to_string()
    }

    fn open_src_image(&mut self, ctx: &egui::Context, href: &str, alt: &str) {
        let href = href.trim();
        if href.is_empty() {
            return;
        }
        let base = self.win().active_tab().and_then(|t| t.doc.path.clone());
        let title = Self::img_title(alt, href);
        if self.imgcache.is_failed(href, base.as_deref()) {
            self.status = i18n::cannot_load_image(href);
            return;
        }
        if let Some(raster) = self.imgcache.get(ctx, href, base.as_deref()) {
            self.pending_img = None;
            self.img_overlay = Some(ImgPreview::new(title, raster));
            return;
        }
        self.pending_img = Some(PendingImg {
            href: href.to_string(),
            title,
            base,
        });
        self.status = t().loading_image.to_string();
        ctx.request_repaint();
    }

    fn poll_pending_img(&mut self, ctx: &egui::Context) {
        self.imgcache.poll(ctx);
        let Some(p) = self.pending_img.as_ref() else {
            return;
        };
        let href = p.href.clone();
        let title = p.title.clone();
        let base = p.base.clone();
        if self.imgcache.is_failed(&href, base.as_deref()) {
            self.pending_img = None;
            self.status = i18n::cannot_load_image(href);
            return;
        }
        if let Some(raster) = self.imgcache.get(ctx, &href, base.as_deref()) {
            self.pending_img = None;
            self.img_overlay = Some(ImgPreview::new(title, raster));
        }
    }

    fn open_href(&mut self, href: &str) {
        let href = href.trim();
        if href.is_empty() {
            return;
        }
        if href.starts_with("http://") || href.starts_with("https://") {
            if opener::open(href).is_err() {
                self.status = i18n::cannot_open_link(href);
            }
            return;
        }
        let (path_part, frag) = match href.find('#') {
            Some(i) => (&href[..i], Some(&href[i + 1..])),
            None => (href, None),
        };
        if path_part.is_empty() {
            if let Some(f) = frag {
                self.jump_anchor(f, true);
            }
            return;
        }
        let base = self.win().active_tab().and_then(|t| t.doc.path.clone());
        let p = Path::new(path_part);
        let path = if p.is_absolute() {
            p.to_path_buf()
        } else if let Some(dir) = base.as_ref().and_then(|b| b.parent()) {
            dir.join(p)
        } else {
            p.to_path_buf()
        };
        let resolved = crate::io::shell_link::resolve(&path);
        if resolved.is_dir()
            || file::is_openable_file(&resolved)
            || crate::io::shell_link::is_lnk(&path)
        {
            let same = self
                .win()
                .active_tab()
                .map(|t| t.doc.path_eq(&resolved))
                .unwrap_or(false);
            if same {
                if let Some(f) = frag {
                    self.jump_anchor(f, true);
                }
                return;
            }
            let here = self.nav_here();
            if let Err(e) = self.open_incoming(&path) {
                self.status = e;
                return;
            }
            if let Some(here) = here {
                self.win_mut().nav.push(here);
            }
            if let Some(f) = frag {
                self.jump_anchor(f, false);
            }
            return;
        }
        if opener::open(&resolved).is_err() {
            self.status = i18n::cannot_open(path.display());
        }
    }

    fn nav_here(&self) -> Option<NavPoint> {
        let tab = self.win().active_tab()?;
        let line = if tab.kind == DocKind::Pdf {
            tab.pdf.as_ref().map(|p| p.current_page()).unwrap_or(0)
        } else if tab.mode == ViewMode::Code {
            tab.cursor_line
        } else {
            tab.preview.top_line
        };
        Some(NavPoint {
            tab_id: tab.id,
            path: tab.doc.path.clone(),
            line,
        })
    }

    fn nav_record_leave(&mut self, dest_line: usize) {
        let Some(here) = self.nav_here() else {
            return;
        };
        if here.line == dest_line {
            return;
        }
        self.win_mut().nav.push(here);
    }

    fn jump_anchor(&mut self, frag: &str, record: bool) {
        let line = {
            let Some(tab) = self.win().active_tab() else {
                return;
            };
            match crate::parser::heading_line_for_anchor(&tab.md, frag) {
                Some(l) => l,
                None => {
                    self.status = i18n::anchor_missing(frag);
                    return;
                }
            }
        };
        if record {
            self.nav_record_leave(line);
        }
        self.win_mut().ignore_outline_until = Some(Instant::now() + Duration::from_millis(650));
        self.win_mut().outline_hl = Some(line);
        if let Some(tab) = self.win_mut().active_tab_mut() {
            tab.request_jump(line);
        }
    }

    fn restore_nav(&mut self, pt: NavPoint) {
        let win_i = 0;
        if let Some(i) = self.wins[win_i].tabs.iter().position(|t| t.id == pt.tab_id) {
            self.wins[win_i].active = i;
            self.wins[win_i].ignore_outline_until =
                Some(Instant::now() + Duration::from_millis(650));
            self.wins[win_i].outline_hl = Some(pt.line);
            self.wins[win_i].tabs[i].request_jump(pt.line);
            return;
        }
        if let Some(path) = pt.path.as_ref() {
            if let Some((wi, ti)) = self.find_open(path) {
                self.cur = wi;
                self.wins[wi].active = ti;
                self.wins[wi].ignore_outline_until =
                    Some(Instant::now() + Duration::from_millis(650));
                self.wins[wi].outline_hl = Some(pt.line);
                self.wins[wi].tabs[ti].request_jump(pt.line);
                return;
            }
            match self.open_path(win_i, path) {
                Ok(()) => {
                    self.wins[win_i].ignore_outline_until =
                        Some(Instant::now() + Duration::from_millis(650));
                    self.wins[win_i].outline_hl = Some(pt.line);
                    if let Some(tab) = self.wins[win_i].active_tab_mut() {
                        tab.request_jump(pt.line);
                    }
                }
                Err(e) => {
                    self.status = i18n::cannot_restore(e);
                }
            }
            return;
        }
        self.status = t().cannot_restore_closed.to_string();
    }

    fn nav_back(&mut self) {
        let Some(here) = self.nav_here() else {
            return;
        };
        let Some(pt) = self.win_mut().nav.go_back(here) else {
            return;
        };
        self.restore_nav(pt);
    }

    fn nav_fwd(&mut self) {
        let Some(here) = self.nav_here() else {
            return;
        };
        let Some(pt) = self.win_mut().nav.go_fwd(here) else {
            return;
        };
        self.restore_nav(pt);
    }

    fn show_status(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(tab) = self.win().active_tab() {
                ui.label(tab.mode.label());
                if tab.kind == DocKind::Pdf {
                    ui.separator();
                    let (cur, n) = tab
                        .pdf
                        .as_ref()
                        .map(|p| (p.current_page() + 1, p.page_count.max(1)))
                        .unwrap_or((1, 1));
                    let z = tab.pdf.as_ref().map(|p| p.zoom).unwrap_or(1.0);
                    ui.label(i18n::pdf_status(cur, n, (z * 100.0).round() as i32));
                } else if tab.kind == DocKind::Word {
                    ui.separator();
                    let fmt = file::ext_lower(tab.doc.path.as_deref().unwrap_or(Path::new("")))
                        .map(|e| e.to_ascii_uppercase())
                        .unwrap_or_else(|| "DOCX".into());
                    let page = tab.preview.word_page + 1;
                    let n = tab.preview.word_pages.max(1);
                    ui.label(i18n::word_status(
                        &fmt,
                        page,
                        n,
                        (tab.preview.word_zoom * 100.0).round() as i32,
                    ));
                } else if tab.kind == DocKind::Image {
                    ui.separator();
                    let s = tab
                        .image
                        .as_ref()
                        .map(|i| i.status_text())
                        .unwrap_or_else(|| "IMG".into());
                    ui.label(i18n::image_readonly_status(&s));
                }
                if tab.kind != DocKind::Image {
                    ui.separator();
                    let lines = if tab.doc.text.is_empty() {
                        0
                    } else {
                        tab.doc.text.lines().count().max(1)
                    };
                    ui.label(i18n::n_lines(lines));
                    if tab.sel_chars > 0 {
                        ui.separator();
                        ui.label(i18n::n_selected(tab.sel_chars));
                    }
                    ui.separator();
                    ui.label(tab.doc.enc.status());
                    ui.separator();
                    ui.label(format!("Tab {}", self.settings.md_tab_size));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(&self.status).color(Color32::from_gray(160)));
            });
        });
    }

    fn show_settings(&mut self, ctx: &egui::Context) {
        if self.settings_draft.is_none() {
            return;
        }
        if let Some(d) = &self.settings_draft {
            i18n::set(d.ui_lang);
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        let notes = t().settings_notes;
        egui::Window::new(t().settings)
            .collapsible(false)
            .resizable(true)
            .default_size([480.0, 420.0])
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let Some(draft) = self.settings_draft.as_mut() else {
                    return;
                };
                ui.add_space(4.0);
                ui.label(RichText::new(t().language).strong().size(15.0));
                ui.label(RichText::new(t().language_help).weak());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(t().language);
                    let label = match draft.ui_lang {
                        Lang::Zh => t().lang_zh,
                        Lang::En => t().lang_en,
                    };
                    egui::ComboBox::from_id_salt("ui_lang")
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut draft.ui_lang, Lang::Zh, t().lang_zh);
                            ui.selectable_value(&mut draft.ui_lang, Lang::En, t().lang_en);
                        });
                });
                ui.add_space(12.0);
                ui.label(RichText::new(t().md_tab).strong().size(15.0));
                ui.label(RichText::new(t().tab_width_help).weak());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(t().width_chars);
                    let label = draft.md_tab_size.to_string();
                    egui::ComboBox::from_id_salt("md_tab_size")
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for n in Settings::tab_choices() {
                                ui.selectable_value(&mut draft.md_tab_size, *n, n.to_string());
                            }
                        });
                });
                ui.add_space(12.0);
                ui.checkbox(&mut draft.md_heading_auto_number, t().heading_auto_number);
                ui.label(RichText::new(t().heading_auto_help).weak());
                ui.add_space(12.0);
                ui.label(RichText::new(t().img_max_width).strong().size(15.0));
                ui.label(RichText::new(t().img_max_help).weak());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(t().width);
                    let label = Settings::img_max_label(draft.md_img_max_width);
                    egui::ComboBox::from_id_salt("md_img_max_width")
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for n in Settings::img_max_choices() {
                                ui.selectable_value(
                                    &mut draft.md_img_max_width,
                                    *n,
                                    Settings::img_max_label(*n),
                                );
                            }
                        });
                });
                ui.add_space(12.0);
                ui.checkbox(&mut draft.enable_logs, t().enable_logs);
                ui.label(RichText::new(t().logs_help).weak());
                ui.add_space(16.0);
                ui.label(RichText::new(t().notes).strong().size(15.0));
                ui.label(RichText::new(notes).weak());
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(t().ok).clicked() {
                            apply = true;
                        }
                        if ui.button(t().cancel).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            cancel = true;
        }
        if !open || cancel {
            i18n::set(self.settings.ui_lang);
            self.settings_draft = None;
        } else if apply {
            if let Some(draft) = self.settings_draft.take() {
                self.apply_settings(draft);
            }
        }
    }

    fn show_dialogs(&mut self, ctx: &egui::Context) {
        match &self.dialog {
            None => {}
            Some(Dialog::Reload { win, tab }) => {
                let wi = *win;
                let ti = *tab;
                let name = self
                    .wins
                    .get(wi)
                    .and_then(|w| w.tabs.get(ti))
                    .map(|t| t.doc.display_name())
                    .unwrap_or_default();
                let mut action = 0;
                egui::Window::new(t().file_changed)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(i18n::file_changed_dirty(&name));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button(t().reload_discard).clicked() {
                                action = 1;
                            }
                            if ui.button(t().keep_edits).clicked() {
                                action = 2;
                            }
                        });
                    });
                match action {
                    1 => {
                        self.reload_tab(wi, ti);
                        self.dialog = None;
                    }
                    2 => self.dialog = None,
                    _ => {}
                }
            }
            Some(Dialog::Error(_)) => {
                let msg = if let Some(Dialog::Error(m)) = &self.dialog {
                    m.clone()
                } else {
                    String::new()
                };
                let mut open = true;
                egui::Window::new(t().error)
                    .collapsible(false)
                    .resizable(false)
                    .open(&mut open)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(&msg);
                        if ui.button(t().ok).clicked() {
                            self.dialog = None;
                        }
                    });
                if !open {
                    self.dialog = None;
                }
            }
            Some(Dialog::CloseTab(idx)) => {
                let idx = *idx;
                let name = self
                    .win()
                    .tabs
                    .get(idx)
                    .map(|t| t.doc.display_name())
                    .unwrap_or_default();
                let mut close_dialog = false;
                let mut action = 0; // 0 none 1 save 2 discard 3 cancel
                egui::Window::new(t().unsaved_changes)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(i18n::unsaved_named(&name));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button(t().save).clicked() {
                                action = 1;
                            }
                            if ui.button(t().dont_save).clicked() {
                                action = 2;
                            }
                            if ui.button(t().cancel).clicked() {
                                action = 3;
                            }
                        });
                    });
                match action {
                    1 => {
                        self.win_mut().active = idx;
                        if self.save_active(false) {
                            self.dialog = None;
                            self.close_tab(idx);
                        }
                    }
                    2 => {
                        close_dialog = true;
                        self.close_tab(idx);
                    }
                    3 => close_dialog = true,
                    _ => {}
                }
                if close_dialog {
                    self.dialog = None;
                }
            }
            Some(Dialog::CloseOthers(keep)) => {
                let keep = *keep;
                let mut action = 0;
                egui::Window::new(t().close_others)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(t().close_others_confirm);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button(t().save_all).clicked() {
                                action = 1;
                            }
                            if ui.button(t().close_without_save).clicked() {
                                action = 2;
                            }
                            if ui.button(t().cancel).clicked() {
                                action = 3;
                            }
                        });
                    });
                match action {
                    1 => {
                        // 只保存待关闭的脏标签，保留的标签不动。
                        let keep_id = self.win().tabs.get(keep).map(|t| t.id);
                        let n = self.win().tabs.len();
                        let mut ok = true;
                        for i in 0..n {
                            if self.win().tabs[i].doc.dirty
                                && Some(self.win().tabs[i].id) != keep_id
                            {
                                self.win_mut().active = i;
                                if !self.save_active(false) {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        if ok {
                            self.dialog = None;
                            self.close_other_tabs(keep);
                        }
                    }
                    2 => {
                        self.dialog = None;
                        self.close_other_tabs(keep);
                    }
                    3 => self.dialog = None,
                    _ => {}
                }
            }
            Some(Dialog::CloseAll) => {
                let mut action = 0;
                egui::Window::new(t().close_all)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(t().close_all_confirm);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button(t().save_all).clicked() {
                                action = 1;
                            }
                            if ui.button(t().close_without_save).clicked() {
                                action = 2;
                            }
                            if ui.button(t().cancel).clicked() {
                                action = 3;
                            }
                        });
                    });
                match action {
                    1 => {
                        let n = self.win().tabs.len();
                        let mut ok = true;
                        for i in 0..n {
                            if self.win().tabs[i].doc.dirty {
                                self.win_mut().active = i;
                                if !self.save_active(false) {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        if ok {
                            self.dialog = None;
                            self.close_all_tabs();
                            self.status = t().closed_all.to_string();
                        }
                    }
                    2 => {
                        self.dialog = None;
                        self.close_all_tabs();
                        self.status = t().closed_all.to_string();
                    }
                    3 => self.dialog = None,
                    _ => {}
                }
            }
            Some(Dialog::About) => {
                let mut open = true;
                egui::Window::new(t().about)
                    .collapsible(false)
                    .resizable(false)
                    .open(&mut open)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.heading("rustmarkdown");
                        ui.label(i18n::version(env!("CARGO_PKG_VERSION")));
                        ui.add_space(8.0);
                        ui.label(t().about_line1);
                        ui.label(t().about_line2);
                        ui.add_space(8.0);
                        if ui.button(t().ok).clicked() {
                            self.dialog = None;
                        }
                    });
                if !open {
                    self.dialog = None;
                }
            }
            Some(Dialog::Quit) => {
                let mut action = 0;
                egui::Window::new(t().quit)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(t().quit_confirm);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button(t().save_all).clicked() {
                                action = 1;
                            }
                            if ui.button(t().quit_without_save).clicked() {
                                action = 2;
                            }
                            if ui.button(t().cancel).clicked() {
                                action = 3;
                            }
                        });
                    });
                match action {
                    1 => {
                        let n = self.win().tabs.len();
                        let mut ok = true;
                        for i in 0..n {
                            if self.win().tabs[i].doc.dirty {
                                self.win_mut().active = i;
                                if !self.save_active(false) {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        if ok {
                            self.dialog = None;
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                    }
                    2 => {
                        self.dialog = None;
                        for t in &mut self.win_mut().tabs {
                            t.doc.dirty = false;
                        }
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                    3 => self.dialog = None,
                    _ => {}
                }
            }
        }
    }

    fn show_find_bar(&mut self, ui: &mut egui::Ui) {
        let ev = {
            let Some(tab) = self.win_mut().active_tab_mut() else {
                return;
            };
            find::show_bar(ui, &mut tab.find)
        };
        match ev {
            Some(FindBarEvent::Close) => {
                if let Some(tab) = self.win_mut().active_tab_mut() {
                    tab.find.open = false;
                }
            }
            Some(FindBarEvent::Changed) => {
                if let Some(tab) = self.win_mut().active_tab_mut() {
                    let text = tab.doc.text.clone();
                    tab.find.recompute(&text);
                }
            }
            Some(FindBarEvent::Next) => self.find_step(true),
            Some(FindBarEvent::Prev) => self.find_step(false),
            None => {}
        }
    }

    fn poll_watch(&mut self) {
        let changed = self.watch.poll();
        for p in changed {
            if let Some((wi, ti)) = self.find_open(&p) {
                if self.wins[wi].tabs[ti].doc.dirty {
                    if self.dialog.is_none() {
                        self.dialog = Some(Dialog::Reload { win: wi, tab: ti });
                    }
                } else {
                    self.reload_tab(wi, ti);
                }
            }
        }
    }

    fn reload_tab(&mut self, wi: usize, ti: usize) {
        let Some(path) = self.wins.get(wi).and_then(|w| w.tabs.get(ti)).and_then(|t| t.doc.path.clone()) else {
            return;
        };
        match file::kind_of(&path) {
            Some(DocKind::Markdown) => match file::read_text(&path) {
                Ok((text, nl, enc)) => {
                    let ts = self.settings.md_tab_size;
                    let tab = &mut self.wins[wi].tabs[ti];
                    tab.doc.text = text;
                    tab.doc.newline = nl;
                    tab.doc.enc = enc;
                    tab.doc.mark_clean();
                    tab.reset_text_undo();
                    tab.reparse(ts);
                    self.status = i18n::reloaded(&file_label(&path));
                }
                Err(e) => self.status = e,
            },
            Some(DocKind::Pdf) => {
                self.wins[wi].tabs[ti].pdf = Some(view::pdf::PdfSession::open(&path));
                self.status = i18n::reloaded(&file_label(&path));
            }
            Some(DocKind::Image) => {
                self.wins[wi].tabs[ti].image = Some(view::img_view::ImageSession::open(&path));
                self.status = i18n::reloaded(&file_label(&path));
            }
            Some(DocKind::Word) => match crate::io::word::load(&path) {
                Ok((text, asset)) => {
                    let ts = self.settings.md_tab_size;
                    let tab = &mut self.wins[wi].tabs[ti];
                    tab.doc.text = text;
                    tab.asset_dir = Some(asset);
                    tab.reset_text_undo();
                    tab.reparse(ts);
                    self.status = i18n::reloaded(&file_label(&path));
                }
                Err(e) => self.status = e,
            },
            None => {}
        }
    }

    fn prune_windows(&mut self, ctx: &egui::Context) {
        let mut i = 1;
        while i < self.wins.len() {
            if self.wins[i].pending_close || self.wins[i].tabs.is_empty() {
                let vid = self.wins[i].viewport_id;
                ctx.send_viewport_cmd_to(vid, ViewportCommand::Close);
                while !self.wins[i].tabs.is_empty() {
                    let old = self.cur;
                    self.cur = i;
                    let last = self.wins[i].tabs.len() - 1;
                    self.close_tab(last);
                    self.cur = old;
                }
                self.wins.remove(i);
                if self.cur >= self.wins.len() {
                    self.cur = 0;
                }
            } else {
                i += 1;
            }
        }
    }

    fn ui_window(&mut self, ctx: &egui::Context, win_i: usize) {
        self.cur = win_i;
        self.note_pointer(ctx);
        if self.dialog.is_none() && self.settings_draft.is_none() && self.img_overlay.is_none() {
            self.handle_shortcuts(ctx);
        }
        let ts = self.settings.md_tab_size;
        if let Some(tab) = self.win_mut().active_tab_mut() {
            tab.reparse_if_due(ctx, ts);
        }
        egui::TopBottomPanel::top("chrome").show(ctx, |ui| {
            self.show_menubar(ui);
            ui.separator();
            self.show_toolbar(ui);
            ui.add_space(2.0);
        });
        egui::TopBottomPanel::top("tabbar").show(ctx, |ui| {
            self.show_tabbar(ui);
        });
        if self.win().active_tab().map(|t| t.find.open).unwrap_or(false) {
            egui::TopBottomPanel::top("findbar").show(ctx, |ui| {
                self.show_find_bar(ui);
            });
        }
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            self.show_status(ui);
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(ctx.style().visuals.panel_fill))
            .show(ctx, |ui| {
                self.show_content(ui);
            });
        self.update_title(ctx);
    }

    fn update_title(&self, ctx: &egui::Context) {
        let tab = self.win().active_tab().map(|t| t.title());
        ctx.send_viewport_cmd(ViewportCommand::Title(viewport_title(tab.as_deref())));
    }
}

fn viewport_pointer_screen(ctx: &egui::Context) -> Option<egui::Pos2> {
    ctx.input(|i| {
        let inner = i.viewport().inner_rect?;
        let local = i.pointer.latest_pos()?;
        Some(inner.min + local.to_vec2())
    })
}

fn menu_item(ui: &mut egui::Ui, text: &str, shortcut: &str, enabled: bool) -> bool {
    let btn = if shortcut.is_empty() {
        egui::Button::new(text)
    } else {
        egui::Button::new(text).shortcut_text(shortcut)
    };
    let clicked = ui.add_enabled(enabled, btn).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

fn menu_check(
    ui: &mut egui::Ui,
    text: &str,
    shortcut: &str,
    enabled: bool,
    selected: bool,
) -> bool {
    let btn = egui::Button::new(text)
        .selected(selected)
        .shortcut_text(shortcut);
    let clicked = ui.add_enabled(enabled, btn).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

fn show_welcome(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(80.0);
        ui.heading("rustmarkdown");
        ui.add_space(12.0);
        ui.label(t().welcome_hint);
        ui.add_space(8.0);
        ui.label(RichText::new(t().welcome_keys).weak());
    });
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let t0 = std::time::Instant::now();
        self.saw_ptr_down = false;
        self.handle_close_request(ctx);
        self.poll_incoming(ctx);
        self.handle_dropped(ctx);
        self.poll_watch();
        self.poll_pending_img(ctx);
        self.show_dialogs(ctx);
        self.show_settings(ctx);
        self.ui_window(ctx, 0);
        self.show_img_overlay(ctx);
        if self.drop_hint {
            egui::Area::new(egui::Id::new("drop_hint"))
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .interactable(false)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(16.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new(t().drop_to_open).size(18.0));
                        });
                });
        }
        self.tick_tab_drag(ctx);
        let mut i = 1;
        while i < self.wins.len() {
            if self.wins[i].pending_close {
                i += 1;
                continue;
            }
            let vid = self.wins[i].viewport_id;
            let title = {
                let t = self.wins[i].active_tab().map(|t| t.title());
                viewport_title(t.as_deref())
            };
            let size = self.wins[i]
                .create_inner
                .unwrap_or(egui::vec2(960.0, 640.0));
            let mut builder = ViewportBuilder::default()
                .with_title(title)
                .with_inner_size([size.x, size.y])
                .with_min_inner_size([480.0, 320.0]);
            if let Some(p) = self.wins[i].follow_pos {
                builder = builder.with_position(p);
            } else if let Some(p) = self.wins[i].open_pos.take() {
                builder = builder.with_position(p);
            }
            ctx.show_viewport_immediate(vid, builder, |ctx, class| {
                let _ = class;
                if ctx.input(|inp| inp.viewport().close_requested()) {
                    let dirty = self.wins[i].tabs.iter().any(|t| t.doc.dirty);
                    if dirty {
                        ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                        self.cur = i;
                        self.dialog = Some(Dialog::CloseAll);
                    } else {
                        self.wins[i].pending_close = true;
                    }
                }
                self.ui_window(ctx, i);
            });
            self.tick_tab_drag(ctx);
            i += 1;
        }
        if self.tab_drag.is_some() && !self.saw_ptr_down {
            self.end_tab_drag();
        }
        self.prune_windows(ctx);
        self.cur = 0;
        self.persist_session();
        self.flush_session(Some(ctx), false);
        if crate::io::log::enabled() {
            let n = self
                .win()
                .active_tab()
                .map(|t| t.doc.text.len())
                .unwrap_or(0);
            let mode = self
                .win()
                .active_tab()
                .map(|t| format!("{:?}", t.mode))
                .unwrap_or_else(|| "-".into());
            let side = if self.win().sidebar_open {
                format!("{:?}", self.win().sidebar_tab)
            } else {
                "off".into()
            };
            crate::io::log::ui_lag(t0, &format!("chars={n} mode={mode} side={side}"));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist_session();
        self.flush_session(None, true);
    }
}
