use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;

const TAB_CHOICES: [i32; 4] = [2, 3, 4, 8];
const IMG_MAX_CHOICES: [i32; 7] = [0, 400, 600, 800, 1000, 1200, 1600];

/// Markdown 相关参数（对齐 docview `AppSettings` 的 md 字段）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Tab 显示/缩进列宽（字符），默认 3。影响列表嵌套与预览缩进。
    #[serde(rename = "mdTabSize")]
    pub md_tab_size: i32,
    /// 预览标题自动编号（不改源码）。默认开启。
    #[serde(rename = "mdHeadingAutoNumber")]
    pub md_heading_auto_number: bool,
    /// 预览图片最大显示宽度（像素）。0 = 随预览区宽度。
    #[serde(rename = "mdImgMaxWidth")]
    pub md_img_max_width: i32,
    /// 左侧大纲侧栏是否显示。
    #[serde(rename = "sidePanelVisible")]
    pub side_panel_visible: bool,
    /// 左侧大纲侧栏宽度（像素）。
    #[serde(rename = "sidePanelWidth")]
    pub side_panel_width: i32,
    /// 最近打开的文件（最多 20，新的在前）。
    #[serde(rename = "recentFiles", default)]
    pub recent_files: Vec<PathBuf>,
    /// 诊断日志总开关（含 UI 卡顿）。默认关。
    #[serde(rename = "enableLogs", default)]
    pub enable_logs: bool,
    /// 界面语言（zh / en）。默认中文。
    #[serde(rename = "uiLang", default)]
    pub ui_lang: Lang,
    /// 自动检查更新间隔（天）。0 = 关闭。默认 7。
    /// 不写字段级 default：结构体级 `#[serde(default)]` 会回落到 `Settings::default()`（7）。
    #[serde(rename = "updateCheckDays")]
    pub update_check_days: i32,
    /// 上次成功检查更新的 Unix 秒。0 = 从未。
    #[serde(rename = "lastUpdateCheck")]
    pub last_update_check: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            md_tab_size: 3,
            md_heading_auto_number: true,
            md_img_max_width: 0,
            side_panel_visible: true,
            side_panel_width: 240,
            recent_files: Vec::new(),
            enable_logs: false,
            ui_lang: Lang::Zh,
            update_check_days: 7,
            last_update_check: 0,
        }
    }
}

impl Settings {
    pub fn tab_choices() -> &'static [i32] {
        &TAB_CHOICES
    }

    pub fn img_max_choices() -> &'static [i32] {
        &IMG_MAX_CHOICES
    }

    pub fn img_max_label(n: i32) -> String {
        if n <= 0 {
            crate::i18n::t().img_unlimited.to_string()
        } else {
            format!("{n} px")
        }
    }

    /// 自动检查更新间隔（天）可选值。
    pub fn update_days_choices() -> &'static [i32] {
        &[1, 3, 7, 14, 30]
    }

    pub fn normalize(&mut self) {
        if self.md_tab_size < 1 {
            self.md_tab_size = 1;
        }
        if self.md_tab_size > 8 {
            self.md_tab_size = 8;
        }
        if self.md_img_max_width < 0 {
            self.md_img_max_width = 0;
        }
        if self.md_img_max_width > 4000 {
            self.md_img_max_width = 4000;
        }
        if self.side_panel_width < 140 {
            self.side_panel_width = 140;
        }
        if self.side_panel_width > 480 {
            self.side_panel_width = 480;
        }
        if self.update_check_days < 0 {
            self.update_check_days = 0;
        }
        if self.update_check_days > 3650 {
            self.update_check_days = 3650;
        }
        if self.last_update_check < 0 {
            self.last_update_check = 0;
        }
        if self.recent_files.len() > 20 {
            self.recent_files.truncate(20);
        }
        for p in &mut self.recent_files {
            *p = crate::doc::strip_win_prefix(p);
        }
    }

    pub fn push_recent(&mut self, path: PathBuf) {
        let path = crate::doc::norm_path(&path);
        self.recent_files.retain(|p| crate::doc::norm_path(p) != path);
        self.recent_files.insert(0, path);
        if self.recent_files.len() > 20 {
            self.recent_files.truncate(20);
        }
    }

    pub fn load() -> Self {
        let path = file_path();
        let Ok(bytes) = std::fs::read(&path) else {
            return Self::default();
        };
        match serde_json::from_slice::<Settings>(&bytes) {
            Ok(mut s) => {
                s.normalize();
                s
            }
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let mut s = self.clone();
        s.normalize();
        let path = file_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(json) = serde_json::to_string_pretty(&s) else {
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
    data_dir().join("settings.json")
}

pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(base) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(base).join("rustmarkdown");
        }
    }
    std::env::temp_dir().join("rustmarkdown")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip() {
        let s = Settings {
            md_tab_size: 4,
            md_heading_auto_number: false,
            md_img_max_width: 800,
            ..Default::default()
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("mdTabSize"));
        assert!(j.contains("mdImgMaxWidth"));
        let t: Settings = serde_json::from_str(&j).unwrap();
        assert_eq!(t, s);
    }

    #[test]
    fn missing_fields_default() {
        let t: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(t, Settings::default());
        assert_eq!(t.md_img_max_width, 0);
        assert_eq!(t.update_check_days, 7);
        assert_eq!(t.last_update_check, 0);
    }

    #[test]
    fn update_days_normalize() {
        let mut s = Settings {
            update_check_days: -5,
            ..Default::default()
        };
        s.normalize();
        assert_eq!(s.update_check_days, 0);
        s.update_check_days = 99999;
        s.normalize();
        assert_eq!(s.update_check_days, 3650);
        s.last_update_check = -2;
        s.normalize();
        assert_eq!(s.last_update_check, 0);
        let j = serde_json::to_string(&Settings {
            update_check_days: 3,
            last_update_check: 123,
            ..Default::default()
        })
        .unwrap();
        assert!(j.contains("\"updateCheckDays\":3"));
        assert!(j.contains("\"lastUpdateCheck\":123"));
    }

    #[test]
    fn normalize_clamps_tab() {
        let mut s = Settings {
            md_tab_size: 99,
            md_heading_auto_number: true,
            md_img_max_width: 9000,
            ..Default::default()
        };
        s.normalize();
        assert_eq!(s.md_tab_size, 8);
        assert_eq!(s.md_img_max_width, 4000);
        s.md_tab_size = 0;
        s.normalize();
        assert_eq!(s.md_tab_size, 1);
    }

    #[test]
    fn recent_dedupe_and_cap() {
        let mut s = Settings::default();
        for i in 0..25 {
            s.push_recent(PathBuf::from(format!(r"D:\f{i}.md")));
        }
        assert_eq!(s.recent_files.len(), 20);
        assert_eq!(s.recent_files[0], PathBuf::from(r"D:\f24.md"));
        s.push_recent(PathBuf::from(r"D:\f10.md"));
        assert_eq!(s.recent_files[0], PathBuf::from(r"D:\f10.md"));
        assert_eq!(s.recent_files.iter().filter(|p| p.ends_with("f10.md")).count(), 1);
    }
}
