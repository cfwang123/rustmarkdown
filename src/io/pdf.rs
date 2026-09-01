//! PDF 页光栅化 + 抽字（pdfium，对齐 docview 连续页阅读与选字）。

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;

use crate::io::pdfium;

/// 工作线程开渲前看一眼：已经滚走的页直接丢掉（Sumatra `PageVisibleNearby`）。
pub struct VisBand {
    lo: AtomicU32,
    hi: AtomicU32,
}

impl VisBand {
    fn new() -> Self {
        Self {
            lo: AtomicU32::new(0),
            hi: AtomicU32::new(u32::MAX),
        }
    }

    pub fn set(&self, lo: u32, hi: u32) {
        self.lo.store(lo, Ordering::Relaxed);
        self.hi.store(hi, Ordering::Relaxed);
    }

    pub fn nearby(&self, page: u32, pad: u32) -> bool {
        let lo = self.lo.load(Ordering::Relaxed);
        let hi = self.hi.load(Ordering::Relaxed);
        page + pad >= lo && page <= hi.saturating_add(pad)
    }
}

pub struct PdfMeta {
    pub page_count: u32,
    pub sizes: Vec<(f32, f32)>,
}

pub struct PdfPagePixels {
    pub page: u32,
    pub width: u32,
    pub px_w: u32,
    pub px_h: u32,
    pub rgba: Vec<u8>,
}

pub enum PdfEvent {
    Ready(PdfMeta),
    Page(PdfPagePixels),
    Text { page: u32, chars: Vec<PdfChar> },
    PageFailed(u32),
    Failed(String),
}

pub enum PdfCmd {
    Render { page: u32, width: u32 },
    ExtractText { page: u32 },
    Quit,
}

pub struct PdfEngine {
    pub tx: Sender<PdfCmd>,
    pub rx: Receiver<PdfEvent>,
    vis: Arc<VisBand>,
}

impl PdfEngine {
    pub fn start(path: &Path) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (ev_tx, ev_rx) = mpsc::channel();
        let vis = Arc::new(VisBand::new());
        let vis_w = vis.clone();
        let path = path.to_path_buf();
        thread::Builder::new()
            .name("pdf-worker".into())
            .spawn(move || worker(path, cmd_rx, ev_tx, vis_w))
            .expect("pdf worker");
        Self {
            tx: cmd_tx,
            rx: ev_rx,
            vis,
        }
    }

    pub fn set_vis(&self, lo: u32, hi: u32) {
        self.vis.set(lo, hi);
    }

    pub fn request(&self, page: u32, width: u32) {
        let _ = self.tx.send(PdfCmd::Render { page, width });
    }

    pub fn request_text(&self, page: u32) {
        let _ = self.tx.send(PdfCmd::ExtractText { page });
    }
}

impl Drop for PdfEngine {
    fn drop(&mut self) {
        let _ = self.tx.send(PdfCmd::Quit);
    }
}

fn worker(
    path: std::path::PathBuf,
    cmd_rx: Receiver<PdfCmd>,
    ev_tx: Sender<PdfEvent>,
    vis: Arc<VisBand>,
) {
    let bytes = match pdfium::read_pdf_bytes(&path) {
        Ok(b) => b,
        Err(e) => {
            let _ = ev_tx.send(PdfEvent::Failed(e));
            return;
        }
    };
    let doc = match pdfium::Doc::open_bytes(bytes) {
        Ok(d) => d,
        Err(e) => {
            let _ = ev_tx.send(PdfEvent::Failed(e));
            return;
        }
    };
    let _ = ev_tx.send(PdfEvent::Ready(PdfMeta {
        page_count: doc.page_count,
        sizes: doc.sizes.clone(),
    }));
    loop {
        let first = match cmd_rx.recv() {
            Ok(c) => c,
            Err(_) => break,
        };
        let mut batch = vec![first];
        while let Ok(c) = cmd_rx.try_recv() {
            batch.push(c);
        }
        if batch.iter().any(|c| matches!(c, PdfCmd::Quit)) {
            break;
        }
        let mut renders: Vec<(u32, u32)> = Vec::new();
        let mut texts: Vec<u32> = Vec::new();
        for c in batch {
            match c {
                PdfCmd::Quit => {}
                PdfCmd::Render { page, width } => renders.push((page, width)),
                PdfCmd::ExtractText { page } => {
                    if !texts.contains(&page) {
                        texts.push(page);
                    }
                }
            }
        }
        // 后进先出：最新可见页优先，同页只留最后一次 width。
        for (page, width) in merge_latest_renders(renders) {
            if page >= doc.page_count {
                continue;
            }
            if !vis.nearby(page, 2) {
                continue;
            }
            let t0 = std::time::Instant::now();
            match doc.render_page(page, width) {
                Ok((px_w, px_h, rgba)) => {
                    if crate::io::log::enabled() {
                        let ms = t0.elapsed().as_secs_f64() * 1000.0;
                        crate::io::log::write(&format!(
                            "pdf.render p{} {width}px {px_w}x{px_h} {ms:.0}ms",
                            page + 1
                        ));
                    }
                    let _ = ev_tx.send(PdfEvent::Page(PdfPagePixels {
                        page,
                        width,
                        px_w,
                        px_h,
                        rgba,
                    }));
                }
                Err(_) => {
                    let _ = ev_tx.send(PdfEvent::PageFailed(page));
                }
            }
        }
        for page in texts {
            if page >= doc.page_count || !vis.nearby(page, 2) {
                continue;
            }
            let chars = doc.extract_chars(page);
            let _ = ev_tx.send(PdfEvent::Text { page, chars });
        }
    }
}

fn merge_latest_renders(renders: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (page, width) in renders.into_iter().rev() {
        if seen.insert(page) {
            out.push((page, width));
        }
    }
    out
}

pub use pdfium::PdfChar;

#[cfg(test)]
mod tests {
    use super::merge_latest_renders;

    #[test]
    fn latest_width_per_page_newest_first() {
        let v = merge_latest_renders(vec![(0, 800), (1, 800), (0, 1600), (2, 800)]);
        assert_eq!(v, vec![(2, 800), (0, 1600), (1, 800)]);
    }

    #[test]
    fn vis_band_skips_pages_scrolled_away() {
        let v = super::VisBand::new();
        v.set(100, 102);
        assert!(v.nearby(100, 2));
        assert!(v.nearby(102, 2));
        assert!(v.nearby(98, 2));
        assert!(!v.nearby(50, 2));
        assert!(!v.nearby(200, 2));
    }
}
