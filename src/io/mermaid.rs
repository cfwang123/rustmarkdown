use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::LazyLock;

use std::sync::Arc;

use egui::{ColorImage, Context, TextureOptions, Vec2};
use resvg::{tiny_skia, usvg};

use crate::io::imgcache::Raster;

const MAX_SIDE: f32 = 2400.0;

enum Status {
    Ready(Raster),
    Failed(String),
}

/// 预览用 Mermaid 图缓存（源码 hash → 纹理）。
pub struct MermaidCache {
    map: HashMap<u64, Status>,
    inflight: HashSet<u64>,
    tx: Sender<(u64, Result<(u32, u32, Vec<u8>), String>)>,
    rx: Receiver<(u64, Result<(u32, u32, Vec<u8>), String>)>,
}

pub enum MermaidReady {
    Ready(Raster),
    Loading,
    Failed(String),
}

impl Default for MermaidCache {
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

static SVG_OPTS: LazyLock<usvg::Options<'static>> = LazyLock::new(|| {
    let mut opt = usvg::Options::default();
    opt.font_family = "Microsoft YaHei".to_owned();
    opt.fontdb_mut().load_system_fonts();
    opt
});

/// 预加载字体库，避免第一张图卡住。
pub fn warmup() {
    let _ = &*SVG_OPTS;
}

pub fn is_mermaid_lang(lang: &str) -> bool {
    lang.trim().eq_ignore_ascii_case("mermaid")
}

impl MermaidCache {
    pub fn poll(&mut self, ctx: &Context) {
        while let Ok((key, res)) = self.rx.try_recv() {
            self.inflight.remove(&key);
            match res {
                Ok((w, h, premul)) => {
                    let rgba = unpremultiply(&premul);
                    let img = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                    let tex =
                        ctx.load_texture(format!("mermaid-{key}"), img, TextureOptions::LINEAR);
                    self.map.insert(
                        key,
                        Status::Ready(Raster {
                            tex,
                            size: Vec2::new(w as f32, h as f32),
                            rgba: Arc::new(rgba),
                            local_path: None,
                        }),
                    );
                }
                Err(e) => {
                    self.map.insert(key, Status::Failed(e));
                }
            }
            ctx.request_repaint();
        }
    }

    pub fn get(&mut self, ctx: &Context, source: &str) -> MermaidReady {
        let key = hash_src(source);
        match self.map.get(&key) {
            Some(Status::Ready(r)) => {
                return MermaidReady::Ready(r.clone());
            }
            Some(Status::Failed(e)) => return MermaidReady::Failed(e.clone()),
            None => {}
        }
        if self.inflight.contains(&key) {
            return MermaidReady::Loading;
        }
        self.inflight.insert(key);
        let tx = self.tx.clone();
        let src = source.to_string();
        std::thread::spawn(move || {
            let r = render_pixels(&src);
            let _ = tx.send((key, r));
        });
        let _ = ctx;
        MermaidReady::Loading
    }
}

fn unpremultiply(px: &[u8]) -> Vec<u8> {
    let mut out = px.to_vec();
    for c in out.chunks_exact_mut(4) {
        let a = c[3];
        if a > 0 && a < 255 {
            let s = 255.0 / a as f32;
            c[0] = (c[0] as f32 * s).round().min(255.0) as u8;
            c[1] = (c[1] as f32 * s).round().min(255.0) as u8;
            c[2] = (c[2] as f32 * s).round().min(255.0) as u8;
        }
    }
    out
}

fn hash_src(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn render_pixels(src: &str) -> Result<(u32, u32, Vec<u8>), String> {
    let svg = mermaid_rs_renderer::render_with_options(
        src,
        mermaid_rs_renderer::RenderOptions::mermaid_default(),
    )
    .map_err(|e| e.to_string())?;
    let svg = force_absolute_size(&svg);
    raster_svg(&svg)
}

fn force_absolute_size(svg: &str) -> String {
    if !svg.contains("width=\"100%\"") && !svg.contains("width='100%'") {
        return svg.to_string();
    }
    let Some(vb) = viewbox_wh(svg) else {
        return svg.to_string();
    };
    let (w, h) = vb;
    svg.replace("width=\"100%\"", &format!("width=\"{w}\""))
        .replace("width='100%'", &format!("width='{w}'"))
        .replace("height=\"100%\"", &format!("height=\"{h}\""))
        .replace("height='100%'", &format!("height='{h}'"))
}

fn viewbox_wh(svg: &str) -> Option<(f32, f32)> {
    let key = "viewBox=\"";
    let i = svg.find(key)?;
    let rest = &svg[i + key.len()..];
    let end = rest.find('"')?;
    let parts: Vec<&str> = rest[..end].split_whitespace().collect();
    if parts.len() != 4 {
        return None;
    }
    let w = parts[2].parse().ok()?;
    let h = parts[3].parse().ok()?;
    Some((w, h))
}

fn raster_svg(svg: &str) -> Result<(u32, u32, Vec<u8>), String> {
    let tree = usvg::Tree::from_str(svg, &SVG_OPTS).map_err(|e| e.to_string())?;
    let size = tree.size();
    let w0 = size.width().max(1.0);
    let h0 = size.height().max(1.0);
    let scale = (MAX_SIDE / w0).min(MAX_SIDE / h0).min(2.0);
    let w = (w0 * scale).ceil().max(1.0) as u32;
    let h = (h0 * scale).ceil().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or_else(|| "pixmap".to_string())?;
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok((w, h, pixmap.data().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flowchart_svg() {
        let svg = mermaid_rs_renderer::render("flowchart LR\n  A-->B\n").unwrap();
        assert!(svg.contains("<svg"), "{svg}");
    }

    #[test]
    fn flowchart_pixels() {
        let (w, h, px) = render_pixels("flowchart LR\n  A[开始] --> B[结束]\n").unwrap();
        assert!(w > 10 && h > 10, "{w}x{h}");
        assert_eq!(px.len(), (w * h * 4) as usize);
    }

    #[test]
    fn lang_detect() {
        assert!(is_mermaid_lang("mermaid"));
        assert!(is_mermaid_lang("Mermaid"));
        assert!(!is_mermaid_lang("rust"));
    }
}
