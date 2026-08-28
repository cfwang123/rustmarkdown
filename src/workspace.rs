//! 文件夹工作区：懒加载目录树（VS Code 式资源管理器）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Color32, CursorIcon, FontId, Rect, RichText, Sense, Ui};

use crate::io::file;

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
}

pub enum ExplorerAction {
    Open(PathBuf),
    Reveal(PathBuf),
    CopyPath(PathBuf),
    Refresh,
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
        self.children.insert(dir.to_path_buf(), ents);
    }

    fn children_of(&mut self, dir: &Path) -> &[FsEntry] {
        if !self.children.contains_key(dir) {
            self.load(dir);
        }
        self.children.get(dir).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

pub fn show(ui: &mut Ui, ws: &mut Workspace) -> Option<ExplorerAction> {
    let mut action = None;
    ui.style_mut().interaction.selectable_labels = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(file_stem(&ws.root)).strong().size(13.0));
        if ui.small_button("刷新").clicked() {
            action = Some(ExplorerAction::Refresh);
        }
    });
    ui.label(
        RichText::new(ws.root.display().to_string())
            .small()
            .color(Color32::from_gray(120)),
    );
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
        let color = if ent.is_dir {
            Color32::from_rgb(0xDC, 0xB6, 0x7A)
        } else if file::is_openable_file(&ent.path) {
            ui.visuals().text_color()
        } else {
            Color32::from_gray(130)
        };
        let icon = if ent.is_dir {
            "📁"
        } else if file::is_image_ext(&ent.path) {
            "🖼"
        } else if file::is_openable_file(&ent.path) {
            "📄"
        } else {
            "·"
        };
        let hit = tree_row(
            ui,
            depth,
            ent.is_dir,
            open,
            selected,
            icon,
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
        } else if hit.clicked {
            ws.selected = Some(ent.path.clone());
            if !ent.is_dir {
                action = Some(ExplorerAction::Open(ent.path.clone()));
            }
        } else if hit.secondary {
            ws.selected = Some(ent.path.clone());
        }
        hit.resp.context_menu(|ui| {
            if !ent.is_dir && ui.button("打开").clicked() {
                ws.selected = Some(ent.path.clone());
                action = Some(ExplorerAction::Open(ent.path.clone()));
                ui.close();
            }
            if ui.button("在资源管理器中显示").clicked() {
                action = Some(ExplorerAction::Reveal(ent.path.clone()));
                ui.close();
            }
            if ui.button("复制路径").clicked() {
                action = Some(ExplorerAction::CopyPath(ent.path.clone()));
                ui.close();
            }
        });
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
    chevron: bool,
    secondary: bool,
    resp: egui::Response,
}

fn tree_row(
    ui: &mut Ui,
    depth: u32,
    is_dir: bool,
    open: bool,
    selected: bool,
    icon: &str,
    name: &str,
    color: Color32,
    path: &Path,
) -> RowHit {
    let h = 22.0;
    let w = ui.available_width().max(48.0);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    resp = resp.on_hover_cursor(CursorIcon::Default);
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
    if is_dir {
        let chev_rect = Rect::from_min_size(pos2(x, rect.top()), vec2(16.0, h));
        let chev = ui
            .interact(chev_rect, ui.id().with(path).with("chev"), Sense::click())
            .on_hover_cursor(CursorIcon::Default);
        let tri = if open { "▼" } else { "▶" };
        let galley = ui.painter().layout_no_wrap(
            tri.to_string(),
            FontId::proportional(10.0),
            Color32::from_rgb(0x9C, 0xA3, 0xAF),
        );
        let tp = pos2(
            chev_rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        );
        ui.painter().galley(tp, galley, Color32::from_rgb(0x9C, 0xA3, 0xAF));
        chevron = chev.clicked();
        x += 16.0;
    } else {
        x += 16.0;
    }
    let label = format!("{icon} {name}");
    let galley = ui.painter().layout_no_wrap(
        label,
        FontId::proportional(13.0),
        color,
    );
    let text_pos = pos2(x, rect.center().y - galley.size().y * 0.5);
    let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(rect.right() - 2.0, rect.bottom()))
        .intersect(ui.clip_rect());
    ui.painter()
        .with_clip_rect(clip)
        .galley(text_pos, galley, color);
    resp = resp.on_hover_text(path.display().to_string());
    RowHit {
        clicked: resp.clicked() && !chevron,
        chevron,
        secondary: resp.secondary_clicked(),
        resp,
    }
}

fn file_stem(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
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
}
