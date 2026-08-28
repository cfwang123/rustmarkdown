use std::borrow::Cow;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use crate::io::imgcache::Raster;

/// 复制文本到剪贴板。
pub fn copy_text(text: &str) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(text.to_owned()).map_err(|e| e.to_string())
}

/// 复制位图到剪贴板（系统「粘贴」为图片）。
pub fn copy_image(raster: &Raster) -> Result<(), String> {
    let w = raster.size.x.round() as usize;
    let h = raster.size.y.round() as usize;
    if w == 0 || h == 0 || raster.rgba.len() < w * h * 4 {
        return Err("没有可用的图片数据".into());
    }
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_image(arboard::ImageData {
        width: w,
        height: h,
        bytes: Cow::Borrowed(raster.rgba.as_slice()),
    })
    .map_err(|e| e.to_string())
}

/// 另存为 png / jpg / bmp（对齐 docview ImageViewer.SaveAs）。
pub fn save_image(raster: &Raster, path: &Path) -> Result<(), String> {
    let w = raster.size.x.round() as u32;
    let h = raster.size.y.round() as u32;
    let img = image::RgbaImage::from_raw(w, h, raster.rgba.as_ref().clone())
        .ok_or_else(|| "无法编码图片".to_string())?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => {
            let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
            rgb.save(path).map_err(|e| format!("保存失败：{e}"))
        }
        "bmp" => {
            let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
            rgb.save(path).map_err(|e| format!("保存失败：{e}"))
        }
        _ => img.save(path).map_err(|e| format!("保存失败：{e}")),
    }
}

/// 复制为文件（资源管理器「粘贴」）：本地文件直接 FileDrop，否则写临时 PNG。
pub fn copy_as_file(raster: &Raster) -> Result<PathBuf, String> {
    let path = if let Some(p) = raster.local_path.as_ref() {
        if p.is_file() {
            p.clone()
        } else {
            write_temp_png(raster)?
        }
    } else {
        write_temp_png(raster)?
    };
    set_file_drop(&path)?;
    Ok(path)
}

fn write_temp_png(raster: &Raster) -> Result<PathBuf, String> {
    let w = raster.size.x.round() as u32;
    let h = raster.size.y.round() as u32;
    let img = image::RgbaImage::from_raw(w, h, raster.rgba.as_ref().clone())
        .ok_or_else(|| "无法编码图片".to_string())?;
    let dir = std::env::temp_dir().join("rustmarkdown");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = raster
        .local_path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("image.png");
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let ext = Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("png");
    let path = unique_path(&dir, stem, ext);
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, &buf).map_err(|e| e.to_string())?;
    Ok(path)
}

fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let p = dir.join(format!("{stem}.{ext}"));
    if !p.exists() {
        return p;
    }
    for i in 1..1000 {
        let p = dir.join(format!("{stem}_{i}.{ext}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}_{}.{}", std::process::id(), ext))
}

fn set_file_drop(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use clipboard_win::{formats, Clipboard, Setter};
        let s = path.to_string_lossy().into_owned();
        let _clip = Clipboard::new_attempts(10).map_err(|e| e.to_string())?;
        formats::FileList
            .write_clipboard(&[s.as_str()])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        cb.set_text(path.to_string_lossy().as_ref())
            .map_err(|e| e.to_string())
    }
}
