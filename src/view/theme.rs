use std::sync::Arc;

use egui::{Align, FontData, FontDefinitions, FontFamily, FontTweak};

/// 粗体字族（微软雅黑 Bold）。egui 的 `RichText::strong()` 只加深颜色，不加粗。
pub fn bold_family() -> FontFamily {
    FontFamily::Name("cjk_bold".into())
}

/// 源码编辑区粗体（Consolas Bold 等宽，接近 GVIM；避免换比例字体导致光标错位）。
pub fn mono_bold_family() -> FontFamily {
    FontFamily::Name("mono_bold".into())
}

/// 预览汉字：只用雅黑（不含 Ubuntu，避免拉丁误走未微调的字形）。
pub fn preview_family() -> FontFamily {
    FontFamily::Name("preview".into())
}

/// 预览拉丁：独立 Ubuntu 副本，可下移基线对齐雅黑。
pub fn latin_family() -> FontFamily {
    FontFamily::Name("preview_latin".into())
}

/// 预览行内代码：Consolas 副本，不下移基线（灰底芯片内垂直居中）。
pub fn preview_mono_family() -> FontFamily {
    FontFamily::Name("preview_mono".into())
}

/// 加载系统中文字体，避免界面与正文出现方框。
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let regular = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\Deng.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
    ];
    let bold = [
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\Dengb.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
    ];
    load_named(&mut fonts, "cjk", &regular);
    load_named(&mut fonts, "cjk_bold", &bold);
    if let Some(arc) = fonts.font_data.get("Ubuntu-Light").cloned() {
        let mut data = (*arc).clone();
        data.tweak = FontTweak {
            y_offset_factor: 0.25,
            ..FontTweak::default()
        };
        fonts
            .font_data
            .insert("preview_latin".to_owned(), Arc::new(data));
    }
    if fonts.font_data.contains_key("cjk") {
        let mut ui_stack = Vec::new();
        if fonts.font_data.contains_key("Ubuntu-Light") {
            ui_stack.push("Ubuntu-Light".to_owned());
        }
        ui_stack.push("cjk".to_owned());
        for name in ["NotoEmoji-Regular", "emoji-icon-font"] {
            if fonts.font_data.contains_key(name) {
                ui_stack.push(name.to_owned());
            }
        }
        fonts.families.insert(FontFamily::Proportional, ui_stack);
        let mut preview = vec!["cjk".to_owned()];
        for name in ["NotoEmoji-Regular", "emoji-icon-font"] {
            if fonts.font_data.contains_key(name) {
                preview.push(name.to_owned());
            }
        }
        fonts.families.insert(preview_family(), preview);
        let latin_stack = if fonts.font_data.contains_key("preview_latin") {
            vec!["preview_latin".to_owned(), "cjk".to_owned()]
        } else if fonts.font_data.contains_key("Ubuntu-Light") {
            vec!["Ubuntu-Light".to_owned(), "cjk".to_owned()]
        } else {
            vec!["cjk".to_owned()]
        };
        fonts.families.insert(latin_family(), latin_stack);
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push("cjk".to_owned());
    }
    let mut bold_stack = Vec::new();
    if fonts.font_data.contains_key("cjk_bold") {
        bold_stack.push("cjk_bold".to_owned());
    }
    if fonts.font_data.contains_key("cjk") {
        bold_stack.push("cjk".to_owned());
    }
    if !bold_stack.is_empty() {
        fonts.families.insert(bold_family(), bold_stack);
    }
    let mono = [
        r"C:\Windows\Fonts\consola.ttf",
        r"C:\Windows\Fonts\consola.TTF",
        r"C:\Windows\Fonts\cascadiamono.ttf",
        r"C:\Windows\Fonts\CascadiaMono.ttf",
    ];
    load_named(&mut fonts, "mono", &mono);
    if fonts.font_data.contains_key("mono") {
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "mono".to_owned());
        // 必须在 apply_tweak("mono") 之前克隆，预览行内代码不要源码用的 y_offset。
        if let Some(arc) = fonts.font_data.get("mono").cloned() {
            fonts.font_data.insert("preview_mono".to_owned(), arc);
            let mut stack = vec!["preview_mono".to_owned()];
            if fonts.font_data.contains_key("cjk") {
                stack.push("cjk".to_owned());
            }
            fonts.families.insert(preview_mono_family(), stack);
        }
    }
    let mono_bold = [
        r"C:\Windows\Fonts\consolab.ttf",
        r"C:\Windows\Fonts\CascadiaMono-Bold.ttf",
        r"C:\Windows\Fonts\cascadiamonobold.ttf",
    ];
    load_named(&mut fonts, "mono_bold", &mono_bold);
    let mut mono_bold_stack = Vec::new();
    if fonts.font_data.contains_key("mono_bold") {
        mono_bold_stack.push("mono_bold".to_owned());
    }
    if fonts.font_data.contains_key("cjk_bold") {
        mono_bold_stack.push("cjk_bold".to_owned());
    }
    if fonts.font_data.contains_key("mono") {
        mono_bold_stack.push("mono".to_owned());
    }
    if fonts.font_data.contains_key("cjk") {
        mono_bold_stack.push("cjk".to_owned());
    }
    if !mono_bold_stack.is_empty() {
        fonts.families.insert(mono_bold_family(), mono_bold_stack);
    }
    let src_tweak = FontTweak {
        y_offset_factor: 0.25,
        ..FontTweak::default()
    };
    apply_tweak(&mut fonts, "mono", src_tweak);
    apply_tweak(&mut fonts, "mono_bold", src_tweak);
    ctx.set_fonts(fonts);
}

fn load_named(fonts: &mut FontDefinitions, name: &str, paths: &[&str]) {
    for path in paths {
        if let Ok(bytes) = std::fs::read(path) {
            let bytes = extract_ttc_face(&bytes, 0).unwrap_or(bytes);
            fonts
                .font_data
                .insert(name.to_owned(), FontData::from_owned(bytes).into());
            return;
        }
    }
}

fn apply_tweak(fonts: &mut FontDefinitions, name: &str, tweak: FontTweak) {
    let Some(arc) = fonts.font_data.get(name).cloned() else {
        return;
    };
    let mut data = (*arc).clone();
    data.tweak = tweak;
    fonts.font_data.insert(name.to_owned(), Arc::new(data));
}

/// TTC 直接塞给 ab_glyph 时拉丁字母常判成缺字，英文会落到 Ubuntu。抽出指定面成独立 TTF。
fn extract_ttc_face(data: &[u8], index: u32) -> Option<Vec<u8>> {
    if data.len() < 16 || data.get(0..4) != Some(&b"ttcf"[..]) {
        return None;
    }
    let nfont = u32::from_be_bytes(data.get(8..12)?.try_into().ok()?);
    if index >= nfont {
        return None;
    }
    let loc = 12usize + index as usize * 4;
    let face = u32::from_be_bytes(data.get(loc..loc + 4)?.try_into().ok()?) as usize;
    let num_tables = u16::from_be_bytes(data.get(face + 4..face + 6)?.try_into().ok()?) as usize;
    let rec0 = face + 12;
    let header_size = 12 + num_tables * 16;
    let mut out = vec![0u8; header_size];
    out[..12].copy_from_slice(data.get(face..face + 12)?);
    let mut cursor = header_size;
    for i in 0..num_tables {
        let o = rec0 + i * 16;
        let rec = data.get(o..o + 16)?;
        let off = u32::from_be_bytes(rec[8..12].try_into().ok()?) as usize;
        let len = u32::from_be_bytes(rec[12..16].try_into().ok()?) as usize;
        while cursor % 4 != 0 {
            out.push(0);
            cursor += 1;
        }
        let d = 12 + i * 16;
        out[d..d + 8].copy_from_slice(&rec[0..8]);
        out[d + 8..d + 12].copy_from_slice(&(cursor as u32).to_be_bytes());
        out[d + 12..d + 16].copy_from_slice(&rec[12..16]);
        out.extend_from_slice(data.get(off..off.checked_add(len)?)?);
        cursor = out.len();
    }
    Some(out)
}

/// 滚动条贴容器边缘；深浅色都固定为展开宽度，浮动不占内容宽（避免把正文挤进窄缝）。
pub fn install_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|s| {
        let mut scroll = egui::style::ScrollStyle::thin();
        scroll.bar_outer_margin = 0.0;
        scroll.floating_width = scroll.bar_width;
        scroll.floating_allocated_width = 0.0;
        scroll.foreground_color = true;
        s.spacing.scroll = scroll;
        // 默认 Center 会把矮的英文字盒垂直居中，中英基线错开。
        s.override_text_valign = Some(Align::BOTTOM);
    });
}

#[cfg(test)]
mod tests {
    use super::extract_ttc_face;

    #[test]
    fn extract_yahei_ttc() {
        let data = std::fs::read(r"C:\Windows\Fonts\msyh.ttc").expect("msyh.ttc");
        let ttf = extract_ttc_face(&data, 0).expect("extract");
        assert!(ttf.len() > 1000);
        assert_ne!(&ttf[0..4], b"ttcf");
    }
}
