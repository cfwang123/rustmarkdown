use std::path::{Path, PathBuf};

use crate::doc::{DocKind, Newline};

const TEXT_EXT: &[&str] = &["md", "markdown", "mdown", "mkd", "txt", "text"];
const WORD_EXT: &[&str] = &["doc", "docx"];
const PDF_EXT: &[&str] = &["pdf"];
const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "ico", "tif", "tiff", "webp"];

pub fn ext_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
}

pub fn is_text_ext(path: &Path) -> bool {
    match ext_lower(path).as_deref() {
        Some(ext) => TEXT_EXT.iter().any(|t| *t == ext),
        None => false,
    }
}

pub fn is_word_ext(path: &Path) -> bool {
    match ext_lower(path).as_deref() {
        Some(ext) => WORD_EXT.iter().any(|t| *t == ext),
        None => false,
    }
}

pub fn is_pdf_ext(path: &Path) -> bool {
    match ext_lower(path).as_deref() {
        Some(ext) => PDF_EXT.iter().any(|t| *t == ext),
        None => false,
    }
}

pub fn is_image_ext(path: &Path) -> bool {
    match ext_lower(path).as_deref() {
        Some(ext) => IMAGE_EXT.iter().any(|t| *t == ext),
        None => false,
    }
}

pub fn kind_of(path: &Path) -> Option<DocKind> {
    if is_pdf_ext(path) {
        Some(DocKind::Pdf)
    } else if is_word_ext(path) {
        Some(DocKind::Word)
    } else if is_image_ext(path) {
        Some(DocKind::Image)
    } else if is_text_ext(path) {
        Some(DocKind::Markdown)
    } else {
        None
    }
}

pub fn is_openable_file(path: &Path) -> bool {
    path.is_file() && kind_of(path).is_some()
}

/// 拖放路径可能是 `file:///C:/...` 或带百分号编码。
pub fn normalize_incoming_path(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    let mut s = raw.as_ref();
    if let Some(rest) = s.strip_prefix("file:///") {
        s = rest;
    } else if let Some(rest) = s.strip_prefix("file://") {
        s = rest;
    }
    let decoded = percent_decode(s);
    let decoded = decoded.replace('/', std::path::MAIN_SEPARATOR_STR);
    PathBuf::from(decoded)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEnc {
    pub label: String,
    pub bom: bool,
}

impl Default for TextEnc {
    fn default() -> Self {
        Self::utf8(false)
    }
}

impl TextEnc {
    pub fn utf8(bom: bool) -> Self {
        Self {
            label: "UTF-8".into(),
            bom,
        }
    }

    pub fn status(&self) -> String {
        if self.bom && self.label == "UTF-8" {
            "UTF-8 BOM".into()
        } else {
            self.label.clone()
        }
    }
}

pub fn read_text(path: &Path) -> Result<(String, Newline, TextEnc), String> {
    let bytes = std::fs::read(path).map_err(|e| crate::i18n::read_fail_path(path.display(), e))?;
    Ok(decode_bytes(&bytes))
}

pub fn decode_bytes(bytes: &[u8]) -> (String, Newline, TextEnc) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        let (text, nl) = normalize_newlines(std::str::from_utf8(&bytes[3..]).unwrap_or(""));
        return (text, nl, TextEnc::utf8(true));
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (cow, _, _) = encoding_rs::UTF_16LE.decode(bytes);
        let (text, nl) = normalize_newlines(&cow);
        return (
            text,
            nl,
            TextEnc {
                label: "UTF-16LE".into(),
                bom: true,
            },
        );
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (cow, _, _) = encoding_rs::UTF_16BE.decode(bytes);
        let (text, nl) = normalize_newlines(&cow);
        return (
            text,
            nl,
            TextEnc {
                label: "UTF-16BE".into(),
                bom: true,
            },
        );
    }
    if std::str::from_utf8(bytes).is_ok() {
        let (text, nl) = normalize_newlines(std::str::from_utf8(bytes).unwrap());
        return (text, nl, TextEnc::utf8(false));
    }
    let (gbk, _, gbk_err) = encoding_rs::GBK.decode(bytes);
    if !gbk_err {
        let (text, nl) = normalize_newlines(&gbk);
        return (
            text,
            nl,
            TextEnc {
                label: "GBK".into(),
                bom: false,
            },
        );
    }
    let mut det = chardetng::EncodingDetector::new();
    det.feed(bytes, true);
    let enc = det.guess(None, true);
    let (cow, _, _) = enc.decode(bytes);
    let (text, nl) = normalize_newlines(&cow);
    (
        text,
        nl,
        TextEnc {
            label: enc.name().to_string(),
            bom: false,
        },
    )
}

fn normalize_newlines(s: &str) -> (String, Newline) {
    let newline = if s.contains("\r\n") {
        Newline::CrLf
    } else {
        Newline::Lf
    };
    let text = s.replace("\r\n", "\n").replace('\r', "\n");
    (text, newline)
}

fn encode_bytes(text: &str, enc: &TextEnc) -> Vec<u8> {
    let encoding = match enc.label.as_str() {
        "GBK" | "GB2312" => encoding_rs::GBK,
        "GB18030" => encoding_rs::GB18030,
        "UTF-16LE" => encoding_rs::UTF_16LE,
        "UTF-16BE" => encoding_rs::UTF_16BE,
        "windows-1252" => encoding_rs::WINDOWS_1252,
        _ => encoding_rs::UTF_8,
    };
    let (cow, _, _) = encoding.encode(text);
    let mut out = Vec::new();
    if enc.bom && encoding == encoding_rs::UTF_8 {
        out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    out.extend_from_slice(&cow);
    out
}

pub fn write_text(path: &Path, text: &str, newline: Newline, enc: &TextEnc) -> Result<(), String> {
    let body = match newline {
        Newline::CrLf => text.replace('\n', "\r\n"),
        Newline::Lf => text.to_string(),
    };
    let bytes = encode_bytes(&body, enc);
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, &bytes).map_err(|e| crate::i18n::write_tmp_fail(e))?;
    if cfg!(windows) && path.exists() {
        std::fs::remove_file(path).map_err(|e| crate::i18n::cannot_overwrite(e))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        crate::i18n::save_fail(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_from_ext() {
        assert_eq!(kind_of(Path::new("a.md")), Some(DocKind::Markdown));
        assert_eq!(kind_of(Path::new("a.DOCX")), Some(DocKind::Word));
        assert_eq!(kind_of(Path::new("a.doc")), Some(DocKind::Word));
        assert_eq!(kind_of(Path::new("a.PDF")), Some(DocKind::Pdf));
        assert_eq!(kind_of(Path::new("a.PNG")), Some(DocKind::Image));
        assert_eq!(kind_of(Path::new("a.jpeg")), Some(DocKind::Image));
        assert_eq!(kind_of(Path::new("a.webp")), Some(DocKind::Image));
        assert_eq!(kind_of(Path::new("a.xls")), None);
    }

    #[test]
    fn drop_path_file_uri() {
        let p = normalize_incoming_path(Path::new("file:///C:/docs/a.docx"));
        assert!(p.to_string_lossy().contains("a.docx"));
        assert!(!p.to_string_lossy().starts_with("file:"));
    }

    #[test]
    fn gbk_roundtrip() {
        let (bytes, _, _) = encoding_rs::GBK.encode("中文GBK测试");
        let (text, _, enc) = decode_bytes(bytes.as_ref());
        assert_eq!(text, "中文GBK测试");
        assert_eq!(enc.label, "GBK");
        let back = encode_bytes(&text, &enc);
        let (text2, _, _) = decode_bytes(&back);
        assert_eq!(text2, "中文GBK测试");
    }

    #[test]
    fn utf8_plain() {
        let (text, _, enc) = decode_bytes("hello 中文".as_bytes());
        assert_eq!(text, "hello 中文");
        assert_eq!(enc.label, "UTF-8");
        assert!(!enc.bom);
    }
}
