//! pdfium.dll 动态加载（渲染 + 抽字）。Windows 优先，dll 放在 exe 旁。

use std::ffi::c_void;
use std::os::raw::{c_char, c_int, c_uint};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use libloading::{Library, Symbol};

const FPDF_ANNOT: c_int = 0x01;
const FPDF_LCD_TEXT: c_int = 0x02;

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
    let lib = unsafe { Library::new(&path) }.map_err(|e| format!("无法加载 pdfium.dll（{}）：{e}", path.display()))?;
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
    unsafe { lib.get(name) }.map_err(|e| format!("pdfium 缺少符号 {n}：{e}"))
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
    Err("找不到 pdfium.dll（应与 exe 同目录，或 native/pdfium/）".into())
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
            return Err("PDF 为空".into());
        }
        let ptr = unsafe { (api.load_mem)(bytes.as_ptr(), bytes.len() as c_int, std::ptr::null()) };
        if ptr.is_null() {
            return Err("无法打开 PDF（格式错误或已加密）".into());
        }
        let n = unsafe { (api.page_count)(ptr) };
        if n <= 0 {
            unsafe { (api.close_doc)(ptr) };
            return Err("PDF 没有页面".into());
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
            return Err("页码超出范围".into());
        }
        let (pw, ph) = self.sizes[page as usize];
        let w = width.max(120).min(2400);
        let scale = w as f32 / pw.max(1.0);
        let h = (ph * scale).round().max(1.0) as u32;
        unsafe {
            let pg = (api.load_page)(self.ptr, page as c_int);
            if pg.is_null() {
                return Err("无法加载页面".into());
            }
            let bmp = (api.bmp_create)(w as c_int, h as c_int, 1);
            if bmp.is_null() {
                (api.close_page)(pg);
                return Err("无法创建位图".into());
            }
            (api.bmp_fill)(bmp, 0, 0, w as c_int, h as c_int, 0xFFFFFFFF);
            (api.render)(
                bmp,
                pg,
                0,
                0,
                w as c_int,
                h as c_int,
                0,
                FPDF_ANNOT | FPDF_LCD_TEXT,
            );
            let buf = (api.bmp_buf)(bmp);
            let stride = (api.bmp_stride)(bmp);
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            if !buf.is_null() && stride >= (w as c_int) * 4 {
                for y in 0..h as usize {
                    let src = buf.add(y * stride as usize);
                    for x in 0..w as usize {
                        let i = (y * w as usize + x) * 4;
                        let b = *src.add(x * 4);
                        let g = *src.add(x * 4 + 1);
                        let r = *src.add(x * 4 + 2);
                        let a = *src.add(x * 4 + 3);
                        rgba[i] = r;
                        rgba[i + 1] = g;
                        rgba[i + 2] = b;
                        rgba[i + 3] = a;
                    }
                }
            }
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

pub fn read_pdf_bytes(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("无法读取 PDF：{} ({e})", path.display()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn find_or_warn() {
        let _ = super::find_dll();
    }
}
