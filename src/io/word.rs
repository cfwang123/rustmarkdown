//! DOC / DOCX → Markdown 预览（对齐 docview DocxViewer 只读阅读）。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use office_oxide::ir::{DocumentIR, Element, Image, List};
use office_oxide::{Document, DocumentFormat};

use crate::io::file;
use crate::io::settings;

const PARSE_STACK: usize = 16 * 1024 * 1024;

pub fn load(path: &Path) -> Result<(String, PathBuf), String> {
    let path = path.to_path_buf();
    std::thread::Builder::new()
        .name("word-parse".into())
        .stack_size(PARSE_STACK)
        .spawn(move || load_inner(&path))
        .map_err(|e| format!("无法启动 Word 解析：{e}"))?
        .join()
        .unwrap_or_else(|_| Err("解析 Word 时崩溃（可能栈溢出或文档损坏）".into()))
}

fn load_inner(path: &Path) -> Result<(String, PathBuf), String> {
    let fmt = DocumentFormat::from_path(path).ok_or_else(|| {
        format!(
            "无法识别 Word 格式：{}",
            file::ext_lower(path).unwrap_or_else(|| "(无扩展名)".into())
        )
    })?;
    let bytes = read_shared(path)?;
    if bytes.is_empty() {
        return Err("Word 文档是空文件".into());
    }
    let cursor = Cursor::new(bytes);
    let doc = Document::from_reader(cursor, fmt).map_err(|e| format!("无法打开 Word 文档：{e}"))?;
    let md = doc.to_markdown();
    let ir = doc.to_ir();
    let dir = cache_dir(path);
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建预览缓存：{e}"))?;
    let files = write_images(&ir, &dir);
    let md = rewrite_images(&md, &files);
    let md = if md.trim().is_empty() {
        "(空文档)".to_string()
    } else {
        md
    };
    Ok((md, dir))
}

fn read_shared(path: &Path) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x7)
            .open(path)
            .map_err(|e| format!("无法读取 Word 文档：{} ({e})", path.display()))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| format!("读取 Word 文档失败：{e}"))?;
        Ok(buf)
    }
    #[cfg(not(windows))]
    {
        std::fs::read(path).map_err(|e| format!("无法读取 Word 文档：{} ({e})", path.display()))
    }
}

fn cache_dir(path: &Path) -> PathBuf {
    let mut h = DefaultHasher::new();
    path.to_string_lossy().hash(&mut h);
    if let Ok(meta) = std::fs::metadata(path) {
        if let Ok(modified) = meta.modified() {
            modified.hash(&mut h);
        }
        meta.len().hash(&mut h);
    }
    settings::data_dir()
        .join("wordcache")
        .join(format!("{:x}", h.finish()))
}

fn write_images(ir: &DocumentIR, dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut n = 0u32;
    for sec in &ir.sections {
        collect_images(&sec.elements, dir, &mut files, &mut n);
    }
    files
}

fn collect_images(els: &[Element], dir: &Path, files: &mut Vec<PathBuf>, n: &mut u32) {
    for el in els {
        match el {
            Element::Image(img) => push_image(img, dir, files, n),
            Element::List(list) => collect_list(list, dir, files, n),
            Element::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        collect_images(&cell.content, dir, files, n);
                    }
                }
            }
            Element::TextBox(tb) => collect_images(&tb.content, dir, files, n),
            _ => {}
        }
    }
}

fn collect_list(list: &List, dir: &Path, files: &mut Vec<PathBuf>, n: &mut u32) {
    for item in &list.items {
        collect_images(&item.content, dir, files, n);
        if let Some(nested) = &item.nested {
            collect_list(nested, dir, files, n);
        }
    }
}

fn push_image(img: &Image, dir: &Path, files: &mut Vec<PathBuf>, n: &mut u32) {
    let Some(data) = img.data.as_ref() else {
        return;
    };
    if data.is_empty() {
        return;
    }
    *n += 1;
    let ext = img
        .format
        .as_ref()
        .map(|f| f.extension())
        .unwrap_or("png");
    let name = format!("img_{n:03}.{ext}");
    let dest = dir.join(&name);
    if std::fs::write(&dest, data).is_ok() {
        files.push(dest);
    }
}

fn rewrite_images(md: &str, files: &[PathBuf]) -> String {
    if files.is_empty() {
        return md.to_string();
    }
    let mut out = String::with_capacity(md.len() + 64);
    let mut i = 0;
    let mut idx = 0usize;
    while i < md.len() {
        if md[i..].starts_with("![") {
            if let Some((end, href_range)) = find_md_image(md, i) {
                let href = &md[href_range.clone()];
                let abs = if is_remote(href) {
                    href.to_string()
                } else if idx < files.len() {
                    let p = unix_path(&files[idx]);
                    idx += 1;
                    p
                } else {
                    href.to_string()
                };
                out.push_str(&md[i..href_range.start]);
                out.push_str(&abs);
                out.push(')');
                i = end;
                continue;
            }
        }
        let ch = md[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    if idx < files.len() {
        out.push('\n');
        for f in &files[idx..] {
            out.push_str("\n![](");
            out.push_str(&unix_path(f));
            out.push_str(")\n");
        }
    }
    out
}

fn is_remote(href: &str) -> bool {
    let h = href.trim();
    h.starts_with("http://") || h.starts_with("https://")
}

fn unix_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// `![alt](href)` → (index after `)`, href byte range)
fn find_md_image(s: &str, bang: usize) -> Option<(usize, std::ops::Range<usize>)> {
    let rest = &s[bang + 1..];
    if !rest.starts_with('[') {
        return None;
    }
    let alt_end = rest.find(']')?;
    let after = &rest[alt_end + 1..];
    if !after.starts_with('(') {
        return None;
    }
    let href_start = bang + 1 + alt_end + 1 + 1;
    let href_rel = after[1..].find(')')?;
    let href_end = href_start + href_rel;
    let end = href_end + 1;
    Some((end, href_start..href_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_image_span() {
        let s = "a ![x](foo.png) b";
        let bang = s.find('!').unwrap();
        let (end, href) = find_md_image(s, bang).unwrap();
        assert_eq!(&s[href], "foo.png");
        assert_eq!(&s[end..], " b");
    }
}
