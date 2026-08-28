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
