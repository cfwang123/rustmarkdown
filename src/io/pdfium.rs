//! pdfium.dll 动态加载（渲染 + 抽字）。Windows 优先，dll 放在 exe 旁。

use std::ffi::c_void;
use std::os::raw::{c_char, c_int, c_uint};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use libloading::{Library, Symbol};

const FPDF_ANNOT: c_int = 0x01;

#[derive(Clone)]
pub struct PdfChar {
    pub index: i32,
    pub ch: char,
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

struct Api {
    _lib: Library,
    init: unsafe extern "C" fn(),
    load_mem: unsafe extern "C" fn(*const u8, c_int, *const c_char) -> *mut c_void,
    close_doc: unsafe extern "C" fn(*mut c_void),
    page_count: unsafe extern "C" fn(*mut c_void) -> c_int,
    page_size: unsafe extern "C" fn(*mut c_void, c_int, *mut f64, *mut f64) -> c_int,
    load_page: unsafe extern "C" fn(*mut c_void, c_int) -> *mut c_void,
    close_page: unsafe extern "C" fn(*mut c_void),
    bmp_create: unsafe extern "C" fn(c_int, c_int, c_int) -> *mut c_void,
    bmp_fill: unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int, c_uint),
    bmp_buf: unsafe extern "C" fn(*mut c_void) -> *mut u8,
    bmp_stride: unsafe extern "C" fn(*mut c_void) -> c_int,
    bmp_destroy: unsafe extern "C" fn(*mut c_void),
    render: unsafe extern "C" fn(*mut c_void, *mut c_void, c_int, c_int, c_int, c_int, c_int, c_int),
    text_load: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    text_close: unsafe extern "C" fn(*mut c_void),
    text_count: unsafe extern "C" fn(*mut c_void) -> c_int,
    text_uni: unsafe extern "C" fn(*mut c_void, c_int) -> c_uint,
    text_box: unsafe extern "C" fn(*mut c_void, c_int, *mut f64, *mut f64, *mut f64, *mut f64),
}

static API: OnceLock<Result<Api, String>> = OnceLock::new();

fn api() -> Result<&'static Api, String> {
    match API.get_or_init(load_api) {
        Ok(a) => Ok(a),
        Err(e) => Err(e.clone()),
    }
}

fn load_api() -> Result<Api, String> {
    let path = find_dll()?;
    let lib = unsafe { Library::new(&path) }.map_err(|e| crate::i18n::pdfium_load(path.display(), e))?;
    unsafe {
        let init = *sym(&lib, b"FPDF_InitLibrary\0")?;
        let api = Api {
            init,
            load_mem: *sym(&lib, b"FPDF_LoadMemDocument\0")?,
            close_doc: *sym(&lib, b"FPDF_CloseDocument\0")?,
            page_count: *sym(&lib, b"FPDF_GetPageCount\0")?,
            page_size: *sym(&lib, b"FPDF_GetPageSizeByIndex\0")?,
            load_page: *sym(&lib, b"FPDF_LoadPage\0")?,
            close_page: *sym(&lib, b"FPDF_ClosePage\0")?,
            bmp_create: *sym(&lib, b"FPDFBitmap_Create\0")?,
            bmp_fill: *sym(&lib, b"FPDFBitmap_FillRect\0")?,
            bmp_buf: *sym(&lib, b"FPDFBitmap_GetBuffer\0")?,
            bmp_stride: *sym(&lib, b"FPDFBitmap_GetStride\0")?,
            bmp_destroy: *sym(&lib, b"FPDFBitmap_Destroy\0")?,
            render: *sym(&lib, b"FPDF_RenderPageBitmap\0")?,
            text_load: *sym(&lib, b"FPDFText_LoadPage\0")?,
            text_close: *sym(&lib, b"FPDFText_ClosePage\0")?,
            text_count: *sym(&lib, b"FPDFText_CountChars\0")?,
            text_uni: *sym(&lib, b"FPDFText_GetUnicode\0")?,
            text_box: *sym(&lib, b"FPDFText_GetCharBox\0")?,
            _lib: lib,
        };
        (api.init)();
        Ok(api)
    }
}

fn sym<'a, T>(lib: &'a Library, name: &[u8]) -> Result<Symbol<'a, T>, String> {
    let n = std::str::from_utf8(name).unwrap_or("?").trim_end_matches('\0');
    unsafe { lib.get(name) }.map_err(|e| crate::i18n::pdfium_sym(n, e))
}

fn find_dll() -> Result<PathBuf, String> {
    let mut cands = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            cands.push(dir.join("pdfium.dll"));
        }
    }
    cands.push(PathBuf::from("pdfium.dll"));
    let native = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("native/pdfium/pdfium.dll");
    cands.push(native);
    for p in cands {
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(crate::i18n::t().pdfium_missing.into())
}

pub struct Doc {
    ptr: *mut c_void,
    /// 必须与 LoadMemDocument 同寿命。
    _bytes: Vec<u8>,
    pub page_count: u32,
    pub sizes: Vec<(f32, f32)>,
}

unsafe impl Send for Doc {}

impl Doc {
    pub fn open_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        let api = api()?;
        if bytes.is_empty() {
            return Err(crate::i18n::t().pdf_empty.into());
        }
        let ptr = unsafe { (api.load_mem)(bytes.as_ptr(), bytes.len() as c_int, std::ptr::null()) };
        if ptr.is_null() {
            return Err(crate::i18n::t().pdf_open_fail.into());
        }
        let n = unsafe { (api.page_count)(ptr) };
        if n <= 0 {
            unsafe { (api.close_doc)(ptr) };
            return Err(crate::i18n::t().pdf_no_pages.into());
        }
        let mut sizes = Vec::with_capacity(n as usize);
        for i in 0..n {
            let mut w = 0.0f64;
            let mut h = 0.0f64;
            let ok = unsafe { (api.page_size)(ptr, i, &mut w, &mut h) };
            if ok == 0 {
                w = 612.0;
                h = 792.0;
            }
            sizes.push((w.max(1.0) as f32, h.max(1.0) as f32));
        }
        Ok(Self {
            ptr,
            _bytes: bytes,
            page_count: n as u32,
            sizes,
        })
    }

    pub fn render_page(&self, page: u32, width: u32) -> Result<(u32, u32, Vec<u8>), String> {
        let api = api()?;
        if page >= self.page_count {
            return Err(crate::i18n::t().pdf_page_range.into());
        }
        let (pw, ph) = self.sizes[page as usize];
        let w = width.max(120).min(2400);
        let scale = w as f32 / pw.max(1.0);
        let h = (ph * scale).round().max(1.0) as u32;
        unsafe {
            let pg = (api.load_page)(self.ptr, page as c_int);
            if pg.is_null() {
                return Err(crate::i18n::t().pdf_page_load.into());
            }
            let bmp = (api.bmp_create)(w as c_int, h as c_int, 1);
            if bmp.is_null() {
                (api.close_page)(pg);
                return Err(crate::i18n::t().pdf_bitmap.into());
            }
            (api.bmp_fill)(bmp, 0, 0, w as c_int, h as c_int, 0xFFFFFFFF);
            // 不对文字做 LCD 亚像素（Sumatra 预览也不强制），整页光栅明显更快。
            (api.render)(
                bmp,
                pg,
                0,
                0,
                w as c_int,
                h as c_int,
                0,
                FPDF_ANNOT,
            );
            let buf = (api.bmp_buf)(bmp);
            let stride = (api.bmp_stride)(bmp);
            let rgba = if !buf.is_null() && stride >= (w as c_int) * 4 {
                bgra_to_rgba(buf, stride, w, h)
            } else {
                vec![0u8; (w * h * 4) as usize]
            };
            (api.bmp_destroy)(bmp);
            (api.close_page)(pg);
            Ok((w, h, rgba))
        }
    }

    pub fn extract_chars(&self, page: u32) -> Vec<PdfChar> {
        let Ok(api) = api() else {
            return Vec::new();
        };
        if page >= self.page_count {
            return Vec::new();
        }
        let page_h = self.sizes[page as usize].1 as f64;
        let mut list = Vec::new();
        unsafe {
            let pg = (api.load_page)(self.ptr, page as c_int);
            if pg.is_null() {
                return list;
            }
            let tp = (api.text_load)(pg);
            if tp.is_null() {
                (api.close_page)(pg);
                return list;
            }
            let n = (api.text_count)(tp);
            for i in 0..n {
                let u = (api.text_uni)(tp, i);
                if u == 0 || u == 0xFFFF {
                    continue;
                }
                let mut left = 0.0;
                let mut right = 0.0;
                let mut bottom = 0.0;
                let mut top = 0.0;
                (api.text_box)(tp, i, &mut left, &mut right, &mut bottom, &mut top);
                let mut t = page_h - top;
                let mut b = page_h - bottom;
                if b < t {
                    std::mem::swap(&mut t, &mut b);
                }
                if right - left < 0.01 && b - t < 0.01 {
                    continue;
                }
                let ch = char::from_u32(u).unwrap_or('?');
                if ch.is_control() && ch != '\t' && ch != '\n' && ch != '\r' {
                    continue;
                }
                list.push(PdfChar {
                    index: i,
                    ch,
                    left: left as f32,
                    top: t as f32,
                    right: right as f32,
                    bottom: b as f32,
                });
            }
            (api.text_close)(tp);
            (api.close_page)(pg);
        }
        list
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        if let Ok(api) = api() {
            if !self.ptr.is_null() {
                unsafe { (api.close_doc)(self.ptr) };
                self.ptr = std::ptr::null_mut();
            }
        }
    }
}

/// pdfium `FPDFBitmap_Create(..., alpha=1)` 为 BGRA。按 u32 交换 R/B。
pub fn bgra_to_rgba(src: *const u8, stride: i32, w: u32, h: u32) -> Vec<u8> {
    let w = w as usize;
    let h = h as usize;
    let mut out = vec![0u8; w * h * 4];
    unsafe {
        for y in 0..h {
            let row = src.add(y * stride as usize) as *const u32;
            let dst = out.as_mut_ptr().add(y * w * 4) as *mut u32;
            for x in 0..w {
                let p = *row.add(x);
                *dst.add(x) =
                    (p & 0xFF00_FF00) | ((p & 0xFF) << 16) | ((p >> 16) & 0xFF);
            }
        }
    }
    out
}

pub fn read_pdf_bytes(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| crate::i18n::pdf_read(path.display(), e))
}

#[cfg(test)]
mod tests {
    #[test]
    fn find_or_warn() {
        let _ = super::find_dll();
    }

    #[test]
    fn bgra_to_rgba_swaps_channels() {
        let src = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let out = super::bgra_to_rgba(src.as_ptr(), 8, 2, 1);
        assert_eq!(out, vec![3, 2, 1, 4, 7, 6, 5, 8]);
    }

    #[test]
    fn bgra_to_rgba_respects_stride() {
        let src = [1u8, 2, 3, 4, 9, 9, 9, 9, 5, 6, 7, 8, 9, 9, 9, 9];
        let out = super::bgra_to_rgba(src.as_ptr(), 8, 1, 2);
        assert_eq!(out, vec![3, 2, 1, 4, 7, 6, 5, 8]);
    }
}
