//! XLS / XLSX / XLSM → 内存表（对齐 docview XlsxViewer，只读、不经 Markdown）。

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;

use calamine::{open_workbook_auto_from_rs, Data, Range, Reader};

use crate::io::file;

/// 与 DocviewWPF XlsxViewer 相同上限。
pub const MAX_ROWS: usize = 10_000;
pub const MAX_COLS: usize = 200;
const EMPTY_ROWS: usize = 30;
const EMPTY_COLS: usize = 10;
const DEF_COL_W: f32 = 64.0;
const DEF_ROW_H: f32 = 20.0;

#[derive(Clone, Copy, Debug)]
pub struct SheetMerge {
    pub r0: usize,
    pub c0: usize,
    pub r1: usize,
    pub c1: usize,
}

impl SheetMerge {
    pub fn contains(self, r: usize, c: usize) -> bool {
        r >= self.r0 && r <= self.r1 && c >= self.c0 && c <= self.c1
    }

    pub fn is_origin(self, r: usize, c: usize) -> bool {
        r == self.r0 && c == self.c0
    }
}

#[derive(Clone, Debug)]
pub struct Sheet {
    pub name: String,
    pub rows: usize,
    pub cols: usize,
    pub col_w: Vec<f32>,
    pub row_h: Vec<f32>,
    cells: HashMap<(u32, u32), String>,
    pub merges: Vec<SheetMerge>,
}

impl Sheet {
    pub fn cell(&self, r: usize, c: usize) -> &str {
        self.cells
            .get(&(r as u32, c as u32))
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// 格内换行当成空格，避免查找按全文行号对不上单元格。
    fn cell_find_hay(&self, r: usize, c: usize) -> String {
        self.cell(r, c)
            .chars()
            .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
            .collect()
    }

    pub fn matches_find(&self, r: usize, c: usize, q_lower: &str) -> bool {
        !q_lower.is_empty() && self.cell_find_hay(r, c).to_lowercase().contains(q_lower)
    }

    pub fn merge_at(&self, r: usize, c: usize) -> Option<SheetMerge> {
        self.merges.iter().copied().find(|m| m.contains(r, c))
    }

    pub fn resolve_origin(&self, r: usize, c: usize) -> (usize, usize) {
        match self.merge_at(r, c) {
            Some(m) => (m.r0, m.c0),
            None => (r, c),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CellAddr {
    pub sheet: usize,
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Debug)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
    pub legacy: bool,
    /// 每非空格一行（格内换行已换成空格），供兼容。
    pub plain: String,
}

const FIND_HIT_CAP: usize = 10_000;

impl Workbook {
    /// 按单元格搜索（不拼整表），命中顺序为表序 × 行列。
    pub fn search_cells(&self, query: &str) -> Vec<CellAddr> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (si, sh) in self.sheets.iter().enumerate() {
            let mut keys: Vec<(u32, u32)> = sh.cells.keys().copied().collect();
            keys.sort_unstable();
            for (r, c) in keys {
                if sh.cell_find_hay(r as usize, c as usize).to_lowercase().contains(&q) {
                    out.push(CellAddr {
                        sheet: si,
                        row: r as usize,
                        col: c as usize,
                    });
                    if out.len() >= FIND_HIT_CAP {
                        return out;
                    }
                }
            }
        }
        out
    }
}

pub fn load(path: &Path) -> Result<Workbook, String> {
    let ext = file::ext_lower(path).unwrap_or_default();
    let legacy = ext == "xls";
    let bytes = read_shared(path)?;
    if bytes.is_empty() {
        return Err(crate::i18n::t().xlsx_empty.into());
    }
    let mut wb = open_workbook_auto_from_rs(Cursor::new(bytes))
        .map_err(|e| crate::i18n::xlsx_open(e))?;
    let mut merge_map: std::collections::HashMap<String, Vec<SheetMerge>> = HashMap::new();
    if let calamine::Sheets::Xlsx(x) = &mut wb {
        if x.load_merged_regions().is_ok() {
            for (sheet, _, d) in x.merged_regions().iter() {
                let r0 = d.start.0 as usize;
                let c0 = d.start.1 as usize;
                let r1 = d.end.0 as usize;
                let c1 = d.end.1 as usize;
                if r0 >= MAX_ROWS || c0 >= MAX_COLS {
                    continue;
                }
                merge_map.entry(sheet.clone()).or_default().push(SheetMerge {
                    r0,
                    c0,
                    r1: r1.min(MAX_ROWS - 1),
                    c1: c1.min(MAX_COLS - 1),
                });
            }
        }
    }
    let names = wb.sheet_names();
    if names.is_empty() {
        return Err(crate::i18n::t().xlsx_no_sheets.into());
    }
    let mut sheets = Vec::with_capacity(names.len());
    for name in names {
        let merges = merge_map.remove(&name).unwrap_or_default();
        let range = wb.worksheet_range(&name).map_err(|e| crate::i18n::xlsx_open(e))?;
        sheets.push(from_range(name, &range, merges));
    }
    let plain = build_plain(&sheets);
    Ok(Workbook {
        sheets,
        legacy,
        plain,
    })
}

fn from_range(name: String, range: &Range<Data>, merges: Vec<SheetMerge>) -> Sheet {
    let (h, w) = range.get_size();
    let start = range.start().unwrap_or((0, 0));
    let mut max_r = if h == 0 { 0 } else { start.0 as usize + h - 1 };
    let mut max_c = if w == 0 { 0 } else { start.1 as usize + w - 1 };
    for m in &merges {
        max_r = max_r.max(m.r1);
        max_c = max_c.max(m.c1);
    }
    let empty = h == 0 && w == 0 && merges.is_empty();
    let rows = if empty {
        EMPTY_ROWS
    } else {
        (max_r + 1).clamp(1, MAX_ROWS)
    };
    let cols = if empty {
        EMPTY_COLS.max(max_c + 1)
    } else {
        (max_c + 1).clamp(1, MAX_COLS)
    };

    let mut cells = HashMap::new();
    let mut col_max: Vec<f32> = vec![8.0; cols];
    if !empty {
        for r in 0..h.min(MAX_ROWS) {
            let rr = start.0 as usize + r;
            if rr >= MAX_ROWS {
                break;
            }
            for c in 0..w.min(MAX_COLS) {
                let cc = start.1 as usize + c;
                if cc >= MAX_COLS {
                    break;
                }
                let Some(v) = range.get_value((start.0 + r as u32, start.1 + c as u32)) else {
                    continue;
                };
                if matches!(v, Data::Empty) {
                    continue;
                }
                let text = data_text(v);
                if text.is_empty() {
                    continue;
                }
                let dw = display_width(&text);
                if cc < col_max.len() {
                    col_max[cc] = col_max[cc].max(dw);
                }
                cells.insert((rr as u32, cc as u32), text);
            }
        }
    }

    let col_w: Vec<f32> = (0..cols)
        .map(|c| {
            let ch = col_max.get(c).copied().unwrap_or(8.0);
            (ch * 7.0 + 14.0).clamp(48.0, 280.0)
        })
        .collect();
    let row_h = vec![DEF_ROW_H; rows];
    let _ = DEF_COL_W;
    Sheet {
        name,
        rows,
        cols,
        col_w,
        row_h,
        cells,
        merges,
    }
}

fn data_text(v: &Data) -> String {
    match v {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => {
            if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                format!("{f}")
            }
        }
        Data::Bool(b) => {
            if *b {
                "TRUE".into()
            } else {
                "FALSE".into()
            }
        }
        Data::DateTime(dt) => dt.to_string(),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#{e:?}"),
    }
}

fn display_width(s: &str) -> f32 {
    s.chars()
        .map(|c| if c.is_ascii() { 1.0 } else { 2.0 })
        .sum::<f32>()
        .min(80.0)
}

fn build_plain(sheets: &[Sheet]) -> String {
    let mut plain = String::new();
    for sh in sheets {
        let mut keys: Vec<(u32, u32)> = sh.cells.keys().copied().collect();
        keys.sort_unstable();
        for (r, c) in keys {
            let t = sh.cell(r as usize, c as usize);
            if t.is_empty() {
                continue;
            }
            if !plain.is_empty() {
                plain.push('\n');
            }
            for ch in t.chars() {
                plain.push(if ch == '\n' || ch == '\r' { ' ' } else { ch });
            }
        }
    }
    plain
}

/// Excel 列标：0 → A，25 → Z，26 → AA。
pub fn col_name(n: usize) -> String {
    let mut n = n + 1;
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    s
}

fn read_shared(path: &Path) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x7)
            .open(path)
            .map_err(|e| crate::i18n::xlsx_read(path.display(), e))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| crate::i18n::xlsx_read_fail(e))?;
        Ok(buf)
    }
    #[cfg(not(windows))]
    {
        std::fs::read(path).map_err(|e| crate::i18n::xlsx_read(path.display(), e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excel_col_letters() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
        assert_eq!(col_name(27), "AB");
    }

    #[test]
    fn merge_origin() {
        let m = SheetMerge {
            r0: 1,
            c0: 1,
            r1: 2,
            c1: 3,
        };
        assert!(m.contains(2, 2));
        assert!(m.is_origin(1, 1));
        assert!(!m.is_origin(1, 2));
    }

    fn test_sheet(cells: &[((u32, u32), &str)]) -> Sheet {
        let mut map = HashMap::new();
        let mut max_r = 0usize;
        let mut max_c = 0usize;
        for ((r, c), t) in cells {
            map.insert((*r, *c), (*t).to_string());
            max_r = max_r.max(*r as usize);
            max_c = max_c.max(*c as usize);
        }
        Sheet {
            name: "S".into(),
            rows: max_r + 1,
            cols: max_c + 1,
            col_w: vec![64.0; max_c + 1],
            row_h: vec![20.0; max_r + 1],
            cells: map,
            merges: vec![],
        }
    }

    #[test]
    fn search_cells_ignores_inner_newlines_as_separate_hits() {
        let book = Workbook {
            sheets: vec![test_sheet(&[
                ((0, 0), "hello\nworld"),
                ((1, 0), "foo"),
            ])],
            legacy: false,
            plain: String::new(),
        };
        let hits = book.search_cells("world");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].row, 0);
        assert_eq!(hits[0].col, 0);
        let hits = book.search_cells("FOO");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].row, 1);
    }

    #[test]
    fn search_cells_chinese() {
        let book = Workbook {
            sheets: vec![test_sheet(&[((0, 0), "标题"), ((2, 3), "查找中文查找")])],
            legacy: false,
            plain: String::new(),
        };
        let hits = book.search_cells("查找");
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].row, hits[0].col), (2, 3));
    }
}
