use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use egui::{ColorImage, Context, TextureHandle, TextureOptions, Vec2};

enum Status {
    Ready(Raster),
    Failed,
}

/// 已解码图片：纹理 + 原始 RGBA（供复制 / 弹层）。
#[derive(Clone)]
pub struct Raster {
    pub tex: TextureHandle,
    pub size: Vec2,
    pub rgba: Arc<Vec<u8>>,
    pub local_path: Option<PathBuf>,
}

/// 本地 / http 图片纹理缓存。
pub struct ImgCache {
    map: HashMap<String, Status>,
    inflight: HashSet<String>,
    tx: Sender<(String, Result<(u32, u32, Vec<u8>), ()>)>,
    rx: Receiver<(String, Result<(u32, u32, Vec<u8>), ()>)>,
}

impl Default for ImgCache {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            map: HashMap::new(),
            inflight: HashSet::new(),
            tx,
            rx,
        }
    }
}

impl ImgCache {
    pub fn poll(&mut self, ctx: &Context) {
        while let Ok((key, res)) = self.rx.try_recv() {
            self.inflight.remove(&key);
            match res {
                Ok((w, h, px)) => {
                    let img = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
                    let tex = ctx.load_texture(key.clone(), img, TextureOptions::LINEAR);
                    let local_path = if key.starts_with("http://") || key.starts_with("https://") {
                        None
                    } else {
                        let p = PathBuf::from(&key);
                        if p.is_file() {
                            Some(p)
                        } else {
                            None
                        }
                    };
                    self.map.insert(
                        key,
                        Status::Ready(Raster {
                            tex,
                            size: Vec2::new(w as f32, h as f32),
                            rgba: Arc::new(px),
                            local_path,
                        }),
                    );
                }
                Err(()) => {
                    self.map.insert(key, Status::Failed);
                }
            }
            ctx.request_repaint();
        }
    }

    pub fn get(&mut self, ctx: &Context, href: &str, base: Option<&Path>) -> Option<Raster> {
        if href.is_empty() {
            return None;
        }
        let key = resolve(href, base);
        if let Some(Status::Ready(r)) = self.map.get(&key) {
            return Some(r.clone());
        }
        if matches!(self.map.get(&key), Some(Status::Failed)) || self.inflight.contains(&key) {
            return None;
        }
        self.inflight.insert(key.clone());
        let tx = self.tx.clone();
        let k2 = key.clone();
        std::thread::spawn(move || {
            let r = load_pixels(&k2);
            let _ = tx.send((k2, r));
        });
        let _ = ctx;
        None
    }
}

pub fn resolve(href: &str, base: Option<&Path>) -> String {
    let h = href.trim();
    if h.starts_with("http://") || h.starts_with("https://") {
        return h.to_string();
    }
    let p = PathBuf::from(h);
    if p.is_absolute() {
        return p.to_string_lossy().into_owned();
    }
    if let Some(base) = base.and_then(|b| b.parent()) {
        return base.join(h).to_string_lossy().into_owned();
    }
    h.to_string()
}

fn load_pixels(key: &str) -> Result<(u32, u32, Vec<u8>), ()> {
    let bytes = if key.starts_with("http://") || key.starts_with("https://") {
        let resp = ureq::get(key)
            .timeout(std::time::Duration::from_secs(8))
            .call()
            .map_err(|_| ())?;
        let mut buf = Vec::new();
        resp.into_reader().read_to_end(&mut buf).map_err(|_| ())?;
        buf
    } else {
        std::fs::read(key).map_err(|_| ())?
    };
    let img = image::load_from_memory(&bytes).map_err(|_| ())?.to_rgba8();
    let w = img.width();
    let h = img.height();
    Ok((w, h, img.into_raw()))
}
