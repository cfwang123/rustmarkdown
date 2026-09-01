use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::doc::ViewMode;
use crate::io::settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SessionMode {
    Code,
    Side,
    Preview,
}

impl From<ViewMode> for SessionMode {
    fn from(m: ViewMode) -> Self {
        match m {
            ViewMode::Code => SessionMode::Code,
            ViewMode::Side => SessionMode::Side,
            ViewMode::Preview => SessionMode::Preview,
        }
    }
}

impl From<SessionMode> for ViewMode {
    fn from(m: SessionMode) -> Self {
        match m {
            SessionMode::Code => ViewMode::Code,
            SessionMode::Side => ViewMode::Side,
            SessionMode::Preview => ViewMode::Preview,
        }
    }
}

fn default_mode() -> SessionMode {
    SessionMode::Preview
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTab {
    pub path: PathBuf,
    #[serde(default = "default_mode")]
    mode: SessionMode,
    /// 视口位置：Markdown 为源行，PDF/Word 为页（0-based）。
    #[serde(default)]
    line: usize,
}

impl SessionTab {
    #[allow(dead_code)]
    pub fn new(path: PathBuf, mode: ViewMode) -> Self {
        Self::with_line(path, mode, 0)
    }

    pub fn with_line(path: PathBuf, mode: ViewMode, line: usize) -> Self {
        Self {
            path,
            mode: mode.into(),
            line,
        }
    }

    pub fn mode(&self) -> ViewMode {
        self.mode.into()
    }

    pub fn line(&self) -> usize {
        self.line
    }
}

/// 主窗口上次的外沿位置与客户区宽高（像素）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowGeom {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub const DEFAULT_SIZE: [f32; 2] = [1100.0, 720.0];
pub const MIN_SIZE: [f32; 2] = [640.0, 400.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl WindowGeom {
    pub fn from_pos_size(x: f32, y: f32, w: f32, h: f32) -> Option<Self> {
        if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() {
            return None;
        }
        if w < 200.0 || h < 100.0 {
            return None;
        }
        Some(Self {
            x: x.round() as i32,
            y: y.round() as i32,
            w: w.round() as i32,
            h: h.round() as i32,
        })
    }
}

impl ScreenRect {
    fn intersect_wh(self, other: Self) -> (i32, i32) {
        let l = self.left.max(other.left);
        let t = self.top.max(other.top);
        let r = self.right.min(other.right);
        let b = self.bottom.min(other.bottom);
        ((r - l).max(0), (b - t).max(0))
    }
}

/// 窗口与某块屏幕相交足够大（能抓住标题栏）则视为在屏上。
pub fn window_on_screen(win: ScreenRect, monitors: &[ScreenRect]) -> bool {
    if monitors.is_empty() {
        return true;
    }
    monitors.iter().any(|m| {
        let (w, h) = m.intersect_wh(win);
        w >= 80 && h >= 24
    })
}

/// 恢复启动时的客户区大小；位置超出所有屏幕则退回系统默认（不指定坐标）。
pub fn apply_saved_window(
    geom: Option<&WindowGeom>,
    monitors: &[ScreenRect],
) -> ([f32; 2], Option<[f32; 2]>) {
    let Some(g) = geom else {
        return (DEFAULT_SIZE, None);
    };
    let w = (g.w as f32).clamp(MIN_SIZE[0], 20_000.0);
    let h = (g.h as f32).clamp(MIN_SIZE[1], 20_000.0);
    let size = [w, h];
    let win = ScreenRect {
        left: g.x,
        top: g.y,
        right: g.x.saturating_add(w.round() as i32),
        bottom: g.y.saturating_add(h.round() as i32),
    };
    if window_on_screen(win, monitors) {
        (size, Some([g.x as f32, g.y as f32]))
    } else {
        (size, None)
    }
}

pub fn list_monitors() -> Vec<ScreenRect> {
    #[cfg(windows)]
    {
        list_monitors_win()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(windows)]
fn list_monitors_win() -> Vec<ScreenRect> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };

    unsafe extern "system" fn proc(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> windows_sys::core::BOOL {
        let out = data as *mut Vec<ScreenRect>;
        let mut info: MONITORINFO = zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) != 0 {
            let r = info.rcMonitor;
            (*out).push(ScreenRect {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            });
        }
        1
    }

    let mut out = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(proc),
            &mut out as *mut Vec<ScreenRect> as LPARAM,
        );
    }
    out
}

pub fn restore_startup_window() -> ([f32; 2], Option<[f32; 2]>) {
    let geom = Session::load().and_then(|s| s.window);
    apply_saved_window(geom.as_ref(), &list_monitors())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub tabs: Vec<SessionTab>,
    pub active: usize,
    /// 目录树工作区根目录；缺省/目录不存在则不恢复。
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    /// 最近打开过的文件的模式与位置（关标签后仍保留，最多 80）。
    #[serde(default)]
    pub file_views: Vec<SessionTab>,
    /// 主窗口位置与大小；缺省则用默认宽高、系统摆放。
    #[serde(default)]
    pub window: Option<WindowGeom>,
}

const MAX_FILE_VIEWS: usize = 80;

pub fn upsert_file_view(views: &mut Vec<SessionTab>, v: SessionTab) {
    let key = crate::doc::norm_path(&v.path);
    views.retain(|x| crate::doc::norm_path(&x.path) != key);
    views.insert(0, v);
    if views.len() > MAX_FILE_VIEWS {
        views.truncate(MAX_FILE_VIEWS);
    }
}

pub fn find_file_view<'a>(views: &'a [SessionTab], path: &std::path::Path) -> Option<&'a SessionTab> {
    let key = crate::doc::norm_path(path);
    views.iter().find(|x| crate::doc::norm_path(&x.path) == key)
}

impl Default for Session {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            workspace: None,
            file_views: Vec::new(),
            window: None,
        }
    }
}

impl Session {
    pub fn load() -> Option<Self> {
        let path = file_path();
        let bytes = std::fs::read(&path).ok()?;
        serde_json::from_slice::<Session>(&bytes).ok()
    }

    pub fn save(&self) {
        let path = file_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(json) = serde_json::to_string_pretty(self) else {
            return;
        };
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json.as_bytes()).is_err() {
            return;
        }
        if cfg!(windows) && path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        if std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

pub fn file_path() -> PathBuf {
    settings::data_dir().join("session.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip() {
        let s = Session {
            tabs: vec![SessionTab::new(PathBuf::from(r"D:\a.md"), ViewMode::Code)],
            active: 0,
            workspace: Some(PathBuf::from(r"D:\docs")),
            file_views: vec![SessionTab::with_line(
                PathBuf::from(r"D:\a.md"),
                ViewMode::Code,
                12,
            )],
            window: Some(WindowGeom {
                x: 80,
                y: 40,
                w: 1200,
                h: 800,
            }),
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("code"));
        assert!(j.contains("workspace"));
        let t: Session = serde_json::from_str(&j).unwrap();
        assert_eq!(t, s);
        assert_eq!(t.tabs[0].mode(), ViewMode::Code);
        assert_eq!(t.file_views[0].line(), 12);
        let old: Session = serde_json::from_str(r#"{"tabs":[],"active":0}"#).unwrap();
        assert!(old.workspace.is_none());
        assert!(old.file_views.is_empty());
        assert!(old.window.is_none());
        assert!(j.contains("\"w\":1200"));
    }

    fn mon(l: i32, t: i32, r: i32, b: i32) -> ScreenRect {
        ScreenRect {
            left: l,
            top: t,
            right: r,
            bottom: b,
        }
    }

    #[test]
    fn off_screen_pos_falls_back_keeps_size() {
        let g = WindowGeom {
            x: 8000,
            y: 100,
            w: 1100,
            h: 720,
        };
        let monitors = [mon(0, 0, 1920, 1080)];
        let (size, pos) = apply_saved_window(Some(&g), &monitors);
        assert_eq!(size, [1100.0, 720.0]);
        assert!(pos.is_none(), "超出屏幕应退回默认位置, got={pos:?}");
    }

    #[test]
    fn on_screen_pos_kept() {
        let g = WindowGeom {
            x: 100,
            y: 80,
            w: 1100,
            h: 720,
        };
        let monitors = [mon(0, 0, 1920, 1080)];
        let (size, pos) = apply_saved_window(Some(&g), &monitors);
        assert_eq!(size, [1100.0, 720.0]);
        assert_eq!(pos, Some([100.0, 80.0]));
    }

    #[test]
    fn missing_geom_uses_default_size() {
        let (size, pos) = apply_saved_window(None, &[mon(0, 0, 1920, 1080)]);
        assert_eq!(size, DEFAULT_SIZE);
        assert!(pos.is_none());
    }

    #[test]
    fn tiny_saved_size_clamped() {
        let g = WindowGeom {
            x: 10,
            y: 10,
            w: 100,
            h: 50,
        };
        let (size, pos) = apply_saved_window(Some(&g), &[mon(0, 0, 1920, 1080)]);
        assert_eq!(size, MIN_SIZE);
        assert_eq!(pos, Some([10.0, 10.0]));
    }

    #[test]
    fn no_monitor_info_keeps_pos() {
        let g = WindowGeom {
            x: 50,
            y: 60,
            w: 1100,
            h: 720,
        };
        let (size, pos) = apply_saved_window(Some(&g), &[]);
        assert_eq!(size, [1100.0, 720.0]);
        assert_eq!(pos, Some([50.0, 60.0]));
    }

    #[cfg(windows)]
    #[test]
    fn list_monitors_not_empty_on_windows() {
        let m = list_monitors();
        assert!(!m.is_empty(), "应能枚举到显示器");
        assert!(m.iter().all(|r| r.right > r.left && r.bottom > r.top));
    }

    #[test]
    fn from_pos_size_skips_minimized() {
        assert!(WindowGeom::from_pos_size(10.0, 10.0, 100.0, 50.0).is_none());
        assert!(WindowGeom::from_pos_size(80.0, 40.0, 1100.0, 720.0).is_some());
    }

    #[test]
    fn second_monitor_counts_as_on_screen() {
        let g = WindowGeom {
            x: 2000,
            y: 100,
            w: 800,
            h: 600,
        };
        let monitors = [mon(0, 0, 1920, 1080), mon(1920, 0, 3840, 1080)];
        let (_, pos) = apply_saved_window(Some(&g), &monitors);
        assert_eq!(pos, Some([2000.0, 100.0]));
    }

    #[test]
    fn file_view_lru() {
        let mut v = Vec::new();
        upsert_file_view(
            &mut v,
            SessionTab::with_line(PathBuf::from(r"D:\a.md"), ViewMode::Preview, 1),
        );
        upsert_file_view(
            &mut v,
            SessionTab::with_line(PathBuf::from(r"D:\b.md"), ViewMode::Code, 2),
        );
        upsert_file_view(
            &mut v,
            SessionTab::with_line(PathBuf::from(r"D:\a.md"), ViewMode::Code, 9),
        );
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].line(), 9);
        assert_eq!(v[0].mode(), ViewMode::Code);
        assert!(find_file_view(&v, std::path::Path::new(r"D:\b.md")).is_some());
    }
}
