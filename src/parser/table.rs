use regex::Regex;
use std::sync::LazyLock;

static RE_IMG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!\[([^\]]*)\]\([^)]*\)").unwrap());
static RE_LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\([^)]*\)").unwrap());

/// 半角 1、全角/CJK 2、Tab 4（移植 MdTableLayout.StrDisplayWidth）。
pub fn str_display_width(s: &str) -> i32 {
    let mut w = 0;
    for ch in s.chars() {
        if ch == '\t' {
            w += 4;
        } else if ch < '\u{80}' {
            if !ch.is_control() {
                w += 1;
            }
        } else if is_wide(ch) {
            w += 2;
        } else {
            w += 1;
        }
    }
    w
}

fn is_wide(ch: char) -> bool {
    let u = ch as u32;
    (0x1100..=0x115F).contains(&u)
        || (0x2E80..=0xA4CF).contains(&u)
        || (0xAC00..=0xD7A3).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0xFE10..=0xFE6F).contains(&u)
        || (0xFF00..=0xFF60).contains(&u)
        || (0xFFE0..=0xFFE6).contains(&u)
        || (0x3000..=0x303F).contains(&u)
}

pub fn strip_inline_markers(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut s = RE_IMG.replace_all(text, "$1").into_owned();
    s = RE_LINK.replace_all(&s, "$1").into_owned();
    s = s.replace("**", "").replace("__", "");
    s = s.replace("~~", "").replace("==", "");
    s = s.replace('`', "");
    strip_lone_emph(&s)
}

fn strip_lone_emph(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '*' || ch == '_' {
            let prev_word = i > 0 && chars[i - 1].is_alphanumeric();
            let next_word = i + 1 < chars.len() && chars[i + 1].is_alphanumeric();
            if prev_word || next_word {
                out.push(ch);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn cell_content_need(text: &str, table_w: i32, ncol: i32) -> i32 {
    let has_img = text.contains("![") || text.to_ascii_lowercase().contains("<img");
    let plain = strip_inline_markers(text);
    let mut w = str_display_width(&plain).max(1);
    if has_img {
        let share = (table_w / ncol.max(1)).max(4);
        w = w.max(share);
    }
    w
}

pub fn content_needs(rows: &[Vec<String>], ncol: usize, table_w: i32) -> Vec<i32> {
    let mut need = vec![1i32; ncol];
    for row in rows {
        for c in 0..ncol {
            let cell = row.get(c).map(|s| s.as_str()).unwrap_or("");
            need[c] = need[c].max(cell_content_need(cell, table_w, ncol as i32));
        }
    }
    need
}

/// 整数显示列单位分配（minCol=1）。
pub fn allocate(need: &[i32], avail: i32) -> Vec<i32> {
    let d = allocate_dip(
        &need.iter().map(|&x| x as f64).collect::<Vec<_>>(),
        avail as f64,
        1.0,
    );
    let mut r: Vec<i32> = d.iter().map(|x| x.round().max(1.0) as i32).collect();
    let sum_need: i32 = need.iter().map(|n| n.max(&1)).sum();
    if sum_need > avail && !r.is_empty() {
        let sum: i32 = r.iter().sum();
        if sum != avail {
            let last = r.len() - 1;
            r[last] = (r[last] + (avail - sum)).max(1);
        }
    }
    r
}

pub fn allocate_dip(need: &[f64], mut avail: f64, mut min_col: f64) -> Vec<f64> {
    let ncol = need.len();
    if ncol == 0 {
        return Vec::new();
    }
    if min_col < 1.0 {
        min_col = 1.0;
    }
    avail = (ncol as f64 * min_col).max(avail);
    let mut ideal = vec![0.0; ncol];
    let mut sum_need = 0.0;
    for c in 0..ncol {
        ideal[c] = need[c].max(min_col);
        sum_need += ideal[c];
    }
    if sum_need <= avail + 0.5 {
        return ideal;
    }
    let mut order: Vec<usize> = (0..ncol).collect();
    order.sort_by(|&a, &b| ideal[a].partial_cmp(&ideal[b]).unwrap());
    let mut col_w = vec![0.0; ncol];
    let mut assigned = vec![false; ncol];
    let mut remain = avail;
    let mut left = ncol;
    for c in order {
        let n = ideal[c];
        let fair = remain / (left.max(1) as f64);
        if n <= fair + 0.01 {
            col_w[c] = n;
            assigned[c] = true;
            remain -= n;
            left -= 1;
        }
    }
    let mut flex_idx = Vec::new();
    let mut flex_need_sum = 0.0;
    for c in 0..ncol {
        if !assigned[c] {
            flex_idx.push(c);
            flex_need_sum += ideal[c];
        }
    }
    if flex_idx.is_empty() {
        if remain > 0.5 {
            col_w[ncol - 1] += remain;
        }
        return col_w;
    }
    remain = (flex_idx.len() as f64 * min_col).max(remain);
    let mut used_flex = 0.0;
    for i in 0..flex_idx.len() {
        let c = flex_idx[i];
        let n = ideal[c];
        let w = if i + 1 == flex_idx.len() {
            (remain - used_flex).max(min_col)
        } else {
            let w = (remain * n / flex_need_sum.max(1.0)).floor().max(min_col);
            used_flex += w;
            w
        };
        col_w[c] = w;
    }
    let mut total = 0.0;
    for c in 0..ncol {
        total += if col_w[c] > 0.0 { col_w[c] } else { min_col };
    }
    if total < avail - 0.5 {
        let last = *flex_idx.last().unwrap();
        col_w[last] += avail - total;
    } else if total > avail + 0.5 {
        let mut over = total - avail;
        for i in (0..flex_idx.len()).rev() {
            if over <= 0.5 {
                break;
            }
            let c = flex_idx[i];
            let cut = over.min((col_w[c] - min_col).max(0.0));
            col_w[c] -= cut;
            over -= cut;
        }
    }
    col_w
}

pub fn allocate_fill_need_dip(need_dip: &[f64], mut avail_dip: f64) -> Vec<f64> {
    let ncol = need_dip.len();
    if ncol == 0 {
        return Vec::new();
    }
    avail_dip = (ncol as f64 * 28.0).max(avail_dip);
    let mut ideal = vec![0.0; ncol];
    let mut sum_ideal = 0.0;
    for c in 0..ncol {
        ideal[c] = need_dip[c].max(28.0);
        sum_ideal += ideal[c];
    }
    if sum_ideal <= avail_dip + 0.5 {
        return ideal;
    }
    let mut order: Vec<usize> = (0..ncol).collect();
    order.sort_by(|&a, &b| ideal[a].partial_cmp(&ideal[b]).unwrap());
    let mut col_w = vec![0.0; ncol];
    let mut pinned = vec![false; ncol];
    let mut remain = avail_dip;
    let mut left = ncol;
    for c in order {
        let n = ideal[c];
        let fair = remain / (left.max(1) as f64);
        if n <= fair + 0.01 {
            col_w[c] = n;
            pinned[c] = true;
            remain -= n;
            left -= 1;
        }
    }
    let mut flex = Vec::new();
    let mut flex_need = 0.0;
    for c in 0..ncol {
        if !pinned[c] {
            flex.push(c);
            flex_need += ideal[c];
        }
    }
    if flex.is_empty() {
        if remain > 0.5 {
            col_w[ncol - 1] += remain;
        }
        return col_w;
    }
    remain = (flex.len() as f64 * 28.0).max(remain);
    let mut used = 0.0;
    for i in 0..flex.len() {
        let c = flex[i];
        let w = if i + 1 == flex.len() {
            (remain - used).max(28.0)
        } else {
            let w = (remain * ideal[c] / flex_need.max(1.0)).floor().max(28.0);
            used += w;
            w
        };
        col_w[c] = w;
    }
    let mut total = 0.0;
    for c in 0..ncol {
        total += if col_w[c] > 0.0 { col_w[c] } else { 28.0 };
    }
    if (total - avail_dip).abs() > 0.5 {
        let last = *flex.last().unwrap();
        col_w[last] = (col_w[last] + (avail_dip - total)).max(28.0);
    }
    col_w
}

#[allow(dead_code)]
fn cell_content_need_dip(
    text: &str,
    table_avail: f64,
    ncol: usize,
    font_size: f64,
    cell_pad: f64,
) -> f64 {
    let has_img = text.contains("![") || text.to_ascii_lowercase().contains("<img");
    let plain = strip_inline_markers(text);
    let unit = (font_size * 0.72).max(4.0);
    let mut w = if plain.is_empty() {
        8.0
    } else {
        str_display_width(&plain) as f64 * unit
    } + cell_pad;
    if has_img {
        let share = (table_avail / ncol.max(1) as f64).max(40.0);
        w = w.max(share);
    }
    w.max(28.0)
}

#[allow(dead_code)]
pub fn content_needs_dip(
    rows: &[Vec<String>],
    ncol: usize,
    table_avail: f64,
    font_size: f64,
    cell_pad: f64,
) -> Vec<f64> {
    let mut need: Vec<f64> = vec![28.0; ncol];
    for row in rows {
        for c in 0..ncol {
            let cell = row.get(c).map(|s| s.as_str()).unwrap_or("");
            need[c] = f64::max(
                need[c],
                cell_content_need_dip(cell, table_avail, ncol, font_size, cell_pad),
            );
        }
    }
    need
}

/// 列是否适合 nowrap：去标记后无空白，且显示宽度 ≤ 24（对齐 mdview / MdTableLayout.ShortNoWrapColumns）。
pub fn short_nowrap_columns(rows: &[Vec<String>], ncol: usize, max_disp: i32) -> Vec<bool> {
    let mut nowrap = vec![true; ncol];
    if ncol == 0 {
        return nowrap;
    }
    for c in 0..ncol {
        let mut max_w = 0;
        for row in rows {
            let cell = row.get(c).map(|s| s.as_str()).unwrap_or("");
            let plain = strip_inline_markers(cell);
            if plain.is_empty() {
                continue;
            }
            if plain.chars().any(|ch| ch.is_whitespace()) {
                nowrap[c] = false;
                break;
            }
            max_w = max_w.max(str_display_width(&plain));
        }
        if nowrap[c] && max_w > max_disp {
            nowrap[c] = false;
        }
    }
    nowrap
}

/// 预览列宽（DIP）。先按 mdview 显示列单位 `allocate`，再换成像素；
/// 短列（need ≤ 均分份额）钉死，避免 `mdview` / `Docs` 被挤成一字一折。
pub fn allocate_columns_dip(rows: &[Vec<String>], ncol: usize, page_width: f64) -> Vec<f64> {
    if ncol == 0 {
        return Vec::new();
    }
    let unit = (14.0_f64 * 0.72).max(4.0);
    let cell_pad = 16.0;
    let min_col = 28.0;
    let width_u = (page_width / unit).floor() as i32;
    let avail_u = (width_u - ncol as i32 - 1).max(ncol as i32);
    let need = content_needs(rows, ncol, avail_u);
    let cols_u = allocate(&need, avail_u);
    let mut dips: Vec<f64> = vec![0.0; ncol];
    let mut flex: Vec<usize> = Vec::new();
    let mut pinned_sum = 0.0;
    for i in 0..ncol {
        let dip = (cols_u[i].max(1) as f64 * unit + cell_pad).max(min_col);
        if cols_u[i] >= need[i] {
            dips[i] = dip;
            pinned_sum += dip;
        } else {
            flex.push(i);
        }
    }
    if flex.is_empty() {
        return dips;
    }
    let remain = (page_width - pinned_sum).max(flex.len() as f64 * min_col);
    let flex_u: i32 = flex.iter().map(|&i| cols_u[i].max(1)).sum();
    let mut used = 0.0;
    for (k, &i) in flex.iter().enumerate() {
        let w = if k + 1 == flex.len() {
            (remain - used).max(min_col)
        } else {
            let w = (remain * cols_u[i].max(1) as f64 / flex_u.max(1) as f64)
                .floor()
                .max(min_col);
            used += w;
            w
        };
        dips[i] = w;
    }
    let sum: f64 = dips.iter().sum();
    if (sum - page_width).abs() > 0.5 {
        let last = *flex.last().unwrap();
        dips[last] = (dips[last] + (page_width - sum)).max(min_col);
    }
    dips
}
