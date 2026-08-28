//! PDF 页光栅化 + 抽字（pdfium，对齐 docview 连续页阅读与选字）。

use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::io::pdfium;

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
}

impl PdfEngine {
    pub fn start(path: &Path) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (ev_tx, ev_rx) = mpsc::channel();
        let path = path.to_path_buf();
        thread::Builder::new()
            .name("pdf-worker".into())
            .spawn(move || worker(path, cmd_rx, ev_tx))
            .expect("pdf worker");
        Self {
            tx: cmd_tx,
            rx: ev_rx,
        }
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

fn worker(path: std::path::PathBuf, cmd_rx: Receiver<PdfCmd>, ev_tx: Sender<PdfEvent>) {
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
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            PdfCmd::Quit => break,
            PdfCmd::Render { page, width } => {
                if page >= doc.page_count {
                    continue;
                }
                match doc.render_page(page, width) {
                    Ok((px_w, px_h, rgba)) => {
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
            PdfCmd::ExtractText { page } => {
                if page >= doc.page_count {
                    continue;
                }
                let chars = doc.extract_chars(page);
                let _ = ev_tx.send(PdfEvent::Text { page, chars });
            }
        }
    }
}

pub use pdfium::PdfChar;
