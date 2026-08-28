//! 已打开文件的外部修改监视（notify）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

pub struct WatchHub {
    watcher: Option<RecommendedWatcher>,
    rx: Option<Receiver<notify::Result<notify::Event>>>,
    watching: HashSet<PathBuf>,
    ignore_until: HashMap<PathBuf, Instant>,
}

impl WatchHub {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let watcher = RecommendedWatcher::new(tx, notify::Config::default()).ok();
        Self {
            watcher,
            rx: Some(rx),
            watching: HashSet::new(),
            ignore_until: HashMap::new(),
        }
    }

    pub fn ignore(&mut self, path: &Path) {
        let p = crate::doc::norm_path(path);
        self.ignore_until
            .insert(p, Instant::now() + Duration::from_millis(1500));
    }

    pub fn sync(&mut self, paths: &[PathBuf]) {
        let Some(w) = self.watcher.as_mut() else {
            return;
        };
        let want: HashSet<PathBuf> = paths.iter().map(|p| crate::doc::norm_path(p)).collect();
        let gone: Vec<PathBuf> = self.watching.difference(&want).cloned().collect();
        for p in gone {
            let _ = w.unwatch(&p);
            self.watching.remove(&p);
        }
        let add: Vec<PathBuf> = want.difference(&self.watching).cloned().collect();
        for p in add {
            if w.watch(&p, RecursiveMode::NonRecursive).is_ok() {
                self.watching.insert(p);
            }
        }
    }

    pub fn poll(&mut self) -> Vec<PathBuf> {
        let Some(rx) = self.rx.as_ref() else {
            return Vec::new();
        };
        let mut dirty: HashSet<PathBuf> = HashSet::new();
        while let Ok(ev) = rx.try_recv() {
            let Ok(ev) = ev else {
                continue;
            };
            match ev.kind {
                EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {}
                _ => continue,
            }
            for p in ev.paths {
                dirty.insert(crate::doc::norm_path(&p));
            }
        }
        let now = Instant::now();
        self.ignore_until.retain(|_, t| *t > now);
        dirty
            .into_iter()
            .filter(|p| {
                self.ignore_until
                    .get(p)
                    .map(|t| *t <= now)
                    .unwrap_or(true)
            })
            .collect()
    }
}

impl Default for WatchHub {
    fn default() -> Self {
        Self::new()
    }
}
