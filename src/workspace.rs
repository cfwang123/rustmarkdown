//! 文件夹工作区：懒加载目录树（VS Code 式资源管理器）。

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Color32, CursorIcon, FontId, Rect, Sense, Ui};

use crate::io::file;
use crate::view::icons::{self, Icon};

const HIST_CAP: usize = 32;
const DIR_FG: Color32 = Color32::from_rgb(0x7A, 0x4E, 0x1A);
const DIR_FG_SEL: Color32 = Color32::from_rgb(0x4A, 0x2E, 0x0E);

#[derive(Clone, Debug)]
pub struct FsEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

pub struct Workspace {
    pub root: PathBuf,
    expanded: HashSet<PathBuf>,
    children: HashMap<PathBuf, Vec<FsEntry>>,
    selected: Option<PathBuf>,
    back: VecDeque<PathBuf>,
    forward: VecDeque<PathBuf>,
    path_edit: String,
}

pub enum ExplorerAction {
    Open(PathBuf),
    Reveal(PathBuf),
    CopyPath(PathBuf),
    Refresh,
    RootChanged,
    BadPath,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarTab {
    Explorer,
    Outline,
}

impl Workspace {
    pub fn new(root: PathBuf) -> Self {
        let root = crate::doc::norm_path(&root);
        let mut ws = Self {
            root: root.clone(),
            expanded: HashSet::new(),
            children: HashMap::new(),
            selected: None,
            back: VecDeque::new(),
            forward: VecDeque::new(),
            path_edit: abs_path_string(&root),
        };
        ws.expanded.insert(root.clone());
        ws.load(&root);
        ws
    }

    pub fn refresh(&mut self) {
        self.children.clear();
        let open: Vec<PathBuf> = self.expanded.iter().cloned().collect();
        for p in open {
            self.load(&p);
        }
    }

    fn load(&mut self, dir: &Path) {
        let t0 = std::time::Instant::now();
        let mut ents = Vec::new();
        let rd = match std::fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => {
                self.children.insert(dir.to_path_buf(), Vec::new());
                return;
            }
        };
        for e in rd.flatten() {
            let p = crate::io::shell_link::resolve(&e.path());
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let is_dir = p.is_dir();
            ents.push(FsEntry {
                path: p,
                name,
                is_dir,
            });
        }
        ents.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });
        let n = ents.len();
        self.children.insert(dir.to_path_buf(), ents);
        if crate::io::log::enabled() {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            crate::io::log::write(&format!(
                "explorer.load {} n={} {ms:.0}ms",
                dir.display(),
                n
            ));
        }
    }

    fn children_of(&mut self, dir: &Path) -> &[FsEntry] {
        if !self.children.contains_key(dir) {
            self.load(dir);
        }
        self.children.get(dir).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn can_up(&self) -> bool {
        parent_dir(&self.root).is_some()
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn go_up(&mut self) -> bool {
        let Some(p) = parent_dir(&self.root) else {
            return false;
        };
        self.navigate(p)
    }

    pub fn go_back(&mut self) -> bool {
        let Some(prev) = self.back.pop_back() else {
            return false;
        };
        self.forward.push_back(self.root.clone());
        trim_hist(&mut self.forward);
        self.apply_root(prev);
        true
    }

    pub fn go_forward(&mut self) -> bool {
        let Some(next) = self.forward.pop_back() else {
            return false;
        };
        self.back.push_back(self.root.clone());
        trim_hist(&mut self.back);
        self.apply_root(next);
        true
    }

    /// 切到新根目录并记入后退历史。同一路径则只同步路径框。
    pub fn navigate(&mut self, dir: PathBuf) -> bool {
        if !dir.is_dir() {
            return false;
        }
        let dir = crate::doc::norm_path(&dir);
        if dir == self.root {
            self.path_edit = abs_path_string(&self.root);
            return true;
        }
        self.back.push_back(self.root.clone());
        trim_hist(&mut self.back);
        self.forward.clear();
        self.apply_root(dir);
        true
    }

    fn apply_root(&mut self, dir: PathBuf) {
        self.root = dir;
        self.expanded.clear();
        self.children.clear();
        self.selected = None;
        self.expanded.insert(self.root.clone());
        self.load(&self.root.clone());
        self.path_edit = abs_path_string(&self.root);
    }
}

pub fn show(ui: &mut Ui, ws: &mut Workspace) -> Option<ExplorerAction> {
    let t0 = std::time::Instant::now();
    let mut action = None;
    ui.style_mut().interaction.selectable_labels = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_enabled_ui(ws.can_up(), |ui| {
            if icons::button(ui, Icon::Up, false, crate::i18n::t().parent_folder).clicked() {
                if ws.go_up() {
                    action = Some(ExplorerAction::RootChanged);
                }
            }
        });
        ui.add_enabled_ui(ws.can_back(), |ui| {
            if icons::button(ui, Icon::Back, false, crate::i18n::t().back).clicked() {
                if ws.go_back() {
                    action = Some(ExplorerAction::RootChanged);
                }
            }
        });
        ui.add_enabled_ui(ws.can_forward(), |ui| {
            if icons::button(ui, Icon::Forward, false, crate::i18n::t().forward).clicked() {
                if ws.go_forward() {
                    action = Some(ExplorerAction::RootChanged);
                }
            }
        });
        if icons::button(ui, Icon::Refresh, false, crate::i18n::t().refresh).clicked() {
            action = Some(ExplorerAction::Refresh);
        }
    });
    let path_id = ui.make_persistent_id("explorer_abs_path");
    let focused = ui.memory(|m| m.has_focus(path_id));
    if !focused {
        let shown = abs_path_string(&ws.root);
        if ws.path_edit != shown {
            ws.path_edit = shown;
        }
    }
    if focused && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        ws.path_edit = abs_path_string(&ws.root);
        ui.memory_mut(|m| m.surrender_focus(path_id));
    }
    let te = egui::TextEdit::singleline(&mut ws.path_edit)
        .id(path_id)
        .desired_width(ui.available_width())
        .font(FontId::proportional(12.0))
        .hint_text(crate::i18n::t().abs_path);
    let resp = ui.add(te);
    if resp.lost_focus() {
        if let Some(a) = commit_path_edit(ws) {
            action = Some(a);
        }
    }
    ui.add_space(2.0);
    ui.separator();
    egui::ScrollArea::both()
        .id_salt("explorer_tree")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.style_mut().interaction.selectable_labels = false;
            let root = ws.root.clone();
            if let Some(a) = show_dir(ui, ws, &root, 0) {
                action = Some(a);
            }
        });
    crate::io::log::slow("explorer.show", t0, crate::io::log::SPAN_MS);
    action
}

fn show_dir(ui: &mut Ui, ws: &mut Workspace, dir: &Path, depth: u32) -> Option<ExplorerAction> {
    if depth > 24 {
        return None;
    }
    let mut action = None;
    let ents = ws.children_of(dir).to_vec();
    for ent in ents {
        let selected = ws.selected.as_ref().is_some_and(|p| p == &ent.path);
        let open = ent.is_dir && ws.expanded.contains(&ent.path);
        let kind = if ent.is_dir {
            RowKind::Dir
        } else if file::is_image_ext(&ent.path) {
            RowKind::Image
        } else if file::kind_of(&ent.path).is_some() {
            RowKind::File
        } else {
            RowKind::Other
        };
        let color = match kind {
            RowKind::Dir => {
                if selected {
                    DIR_FG_SEL
                } else {
                    DIR_FG
                }
            }
            RowKind::File | RowKind::Image => ui.visuals().text_color(),
            RowKind::Other => Color32::from_gray(110),
        };
        let hit = tree_row(
            ui,
            depth,
            kind,
            open,
            selected,
            &ent.name,
            color,
            &ent.path,
        );
        if hit.chevron {
            if open {
                ws.expanded.remove(&ent.path);
            } else {
                ws.expanded.insert(ent.path.clone());
            }
            ws.selected = Some(ent.path.clone());
        } else if hit.double_clicked && ent.is_dir {
            if open {
                ws.expanded.remove(&ent.path);
            } else {
                ws.expanded.insert(ent.path.clone());
            }
            ws.selected = Some(ent.path.clone());
        } else if hit.double_clicked && !ent.is_dir {
            ws.selected = Some(ent.path.clone());
            action = Some(ExplorerAction::Open(ent.path.clone()));
        } else if hit.clicked {
            ws.selected = Some(ent.path.clone());
        } else if hit.secondary {
            ws.selected = Some(ent.path.clone());
        }
        // 菜单打开后必须每帧继续挂上：鼠标移向菜单后行不再 hovered，
        // 若只在 hover/右键帧调用，egui 会立刻关掉弹出层。
        if hit.resp.hovered()
            || hit.resp.secondary_clicked()
            || hit.resp.context_menu_opened()
        {
            hit.resp.context_menu(|ui| {
                if ent.is_dir && ui.button(crate::i18n::t().set_as_workspace).clicked() {
                    if ws.navigate(ent.path.clone()) {
                        action = Some(ExplorerAction::RootChanged);
                    }
                    ui.close();
                }
                if !ent.is_dir && ui.button(crate::i18n::t().open).clicked() {
                    ws.selected = Some(ent.path.clone());
                    action = Some(ExplorerAction::Open(ent.path.clone()));
                    ui.close();
                }
                if ui.button(crate::i18n::t().reveal_in_explorer).clicked() {
                    action = Some(ExplorerAction::Reveal(ent.path.clone()));
                    ui.close();
                }
                if ui.button(crate::i18n::t().copy_path).clicked() {
                    action = Some(ExplorerAction::CopyPath(ent.path.clone()));
                    ui.close();
                }
            });
        }
        if ent.is_dir && ws.expanded.contains(&ent.path) {
            if let Some(a) = show_dir(ui, ws, &ent.path, depth + 1) {
                action = Some(a);
            }
        }
    }
    action
}

struct RowHit {
    clicked: bool,
    double_clicked: bool,
    chevron: bool,
    secondary: bool,
    resp: egui::Response,
}

#[derive(Clone, Copy)]
enum RowKind {
    Dir,
    File,
    Image,
    Other,
}

fn tree_row(
    ui: &mut Ui,
    depth: u32,
    kind: RowKind,
    open: bool,
    selected: bool,
    name: &str,
    color: Color32,
    path: &Path,
) -> RowHit {
    let h = 22.0;
    let w = ui.available_width().max(48.0);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    resp = resp.on_hover_cursor(CursorIcon::Default);
    let visible = rect.intersects(ui.clip_rect());
    if !visible {
        return RowHit {
            clicked: false,
            double_clicked: false,
            chevron: false,
            secondary: false,
            resp,
        };
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::Default);
    }
    let fill = if selected {
        ui.visuals().selection.bg_fill.gamma_multiply(0.55)
    } else if resp.hovered() {
        Color32::from_rgba_unmultiplied(0, 0, 0, 16)
    } else {
        Color32::TRANSPARENT
    };
    if fill.a() > 0 {
        ui.painter()
            .rect_filled(rect.intersect(ui.clip_rect()), 3.0, fill);
    }
    let indent = depth as f32 * 14.0;
    let mut x = rect.left() + 4.0 + indent;
    let mut chevron = false;
    let is_dir = matches!(kind, RowKind::Dir);
    if is_dir {
        let chev_rect = Rect::from_min_size(pos2(x, rect.top()), vec2(16.0, h));
        let chev = ui
            .interact(chev_rect, ui.id().with(path).with("chev"), Sense::click())
            .on_hover_cursor(CursorIcon::Default);
        let tri_r = Rect::from_center_size(chev_rect.center(), vec2(10.0, 10.0));
        icons::paint_tree_chevron(
            ui.painter(),
            tri_r,
            open,
            Color32::from_rgb(0x6B, 0x72, 0x80),
        );
        chevron = chev.clicked();
        x += 16.0;
    } else {
        x += 16.0;
    }
    let icon_r = Rect::from_center_size(pos2(x + 7.0, rect.center().y), vec2(14.0, 14.0));
    match kind {
        RowKind::Dir => icons::paint_tree_folder(ui.painter(), icon_r, color),
        RowKind::Image => icons::paint_tree_image(ui.painter(), icon_r, color),
        RowKind::File => icons::paint_tree_file(ui.painter(), icon_r, color),
        RowKind::Other => {
            ui.painter()
                .circle_filled(icon_r.center(), 1.6, Color32::from_gray(150));
        }
    }
    x += 16.0;
    let galley = ui.painter().layout_no_wrap(
        name.to_string(),
        FontId::proportional(13.0),
        color,
    );
    let text_pos = pos2(x, rect.center().y - galley.size().y * 0.5);
    let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(rect.right() - 2.0, rect.bottom()))
        .intersect(ui.clip_rect());
    ui.painter()
        .with_clip_rect(clip)
        .galley(text_pos, galley, color);
    if resp.hovered() && !resp.context_menu_opened() {
        resp = resp.on_hover_text(path.display().to_string());
    }
    RowHit {
        clicked: resp.clicked() && !chevron,
        double_clicked: resp.double_clicked() && !chevron,
        chevron,
        secondary: resp.secondary_clicked(),
        resp,
    }
}

fn parent_dir(p: &Path) -> Option<PathBuf> {
    let parent = p.parent()?;
    if parent.as_os_str().is_empty() {
        return None;
    }
    Some(parent.to_path_buf())
}

fn abs_path_string(p: &Path) -> String {
    crate::doc::display_path(p)
}

fn trim_hist(h: &mut VecDeque<PathBuf>) {
    while h.len() > HIST_CAP {
        h.pop_front();
    }
}

fn parse_dir_input(s: &str) -> Option<PathBuf> {
    let t = s.trim().trim_matches('"');
    if t.is_empty() {
        return None;
    }
    let p = crate::io::shell_link::resolve(&PathBuf::from(t));
    if p.is_dir() {
        Some(p)
    } else if p.is_file() {
        parent_dir(&p)
    } else {
        None
    }
}

fn commit_path_edit(ws: &mut Workspace) -> Option<ExplorerAction> {
    let shown = abs_path_string(&ws.root);
    if ws.path_edit.trim() == shown {
        ws.path_edit = shown;
        return None;
    }
    match parse_dir_input(&ws.path_edit) {
        Some(dir) => {
            if ws.navigate(dir) {
                Some(ExplorerAction::RootChanged)
            } else {
                ws.path_edit = shown;
                Some(ExplorerAction::BadPath)
            }
        }
        None => {
            ws.path_edit = shown;
            Some(ExplorerAction::BadPath)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_dotfiles() {
        let dir = std::env::temp_dir().join("rustmarkdown-ws-test");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("a.md"), "x");
        let _ = std::fs::write(dir.join(".hidden"), "y");
        let mut ws = Workspace::new(dir.clone());
        let kids = ws.children_of(&dir);
        assert!(kids.iter().any(|e| e.name == "a.md"));
        assert!(!kids.iter().any(|e| e.name.starts_with('.')));
        let _ = std::fs::remove_file(dir.join("a.md"));
        let _ = std::fs::remove_file(dir.join(".hidden"));
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn navigate_back_forward_up() {
        let base = std::env::temp_dir().join("rustmarkdown-ws-nav");
        let child = base.join("sub");
        let _ = std::fs::create_dir_all(&child);
        let mut ws = Workspace::new(child.clone());
        assert!(ws.can_up());
        assert!(!ws.can_back());
        assert!(ws.go_up());
        assert!(ws.can_back());
        assert!(!ws.can_forward());
        assert_eq!(ws.root, crate::doc::norm_path(&base));
        assert!(ws.go_back());
        assert_eq!(ws.root, crate::doc::norm_path(&child));
        assert!(ws.can_forward());
        assert!(ws.go_forward());
        assert_eq!(ws.root, crate::doc::norm_path(&base));
        let _ = std::fs::remove_dir(&child);
        let _ = std::fs::remove_dir(&base);
    }

    #[test]
    fn parse_dir_and_file() {
        let dir = std::env::temp_dir().join("rustmarkdown-ws-path");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("a.md");
        let _ = std::fs::write(&f, "x");
        let got = parse_dir_input(&dir.display().to_string()).unwrap();
        assert!(got.is_dir());
        let from_file = parse_dir_input(&f.display().to_string()).unwrap();
        assert_eq!(
            crate::doc::norm_path(&from_file),
            crate::doc::norm_path(&dir)
        );
        assert!(parse_dir_input("\"not-a-real-path-xyz\"").is_none());
        let _ = std::fs::remove_file(&f);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn abs_path_strips_verbatim() {
        assert_eq!(abs_path_string(Path::new(r"\\?\C:\docs")), r"C:\docs");
        assert_eq!(
            abs_path_string(Path::new(r"\\?\UNC\server\share")),
            r"\\server\share"
        );
    }
}
