//! 按段增量（对齐 mdview：前后缀指纹对齐，只重做脏段）。

/// 脏区 `[lo, hi_old)` / `[lo, hi_new)`。`lo == hi_new && lo == hi_old` 表示全同。
pub fn diff_fps(old: &[u64], new: &[u64]) -> (usize, usize, usize) {
    let n_old = old.len();
    let n_new = new.len();
    let mut lo = 0usize;
    while lo < n_old && lo < n_new && old[lo] == new[lo] {
        lo += 1;
    }
    let mut hi_old = n_old;
    let mut hi_new = n_new;
    while hi_old > lo && hi_new > lo && old[hi_old - 1] == new[hi_new - 1] {
        hi_old -= 1;
        hi_new -= 1;
    }
    (lo, hi_old, hi_new)
}

/// 源码行区间，不含换行（文件以换行结尾时最后一段含该换行，对齐 epaint 分段）。
/// 空行是 `(i, i)`，由排版给一行高度，不能把 `\n` 算进段里（否则每行变成两行高）。
pub fn paragraph_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if text.is_empty() {
        return out;
    }
    let mut start = 0usize;
    while start < text.len() {
        let mut end = text[start..]
            .find('\n')
            .map_or(text.len(), |i| start + i);
        if end == text.len() - 1 && text.ends_with('\n') {
            end += 1;
        }
        out.push((start, end));
        if end >= text.len() {
            break;
        }
        start = end + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{diff_fps, paragraph_ranges};

    #[test]
    fn diff_all_same() {
        let a = [1u64, 2, 3];
        assert_eq!(diff_fps(&a, &a), (3, 3, 3));
    }

    #[test]
    fn diff_middle_one() {
        let old = [1u64, 2, 3, 4];
        let new = [1u64, 9, 3, 4];
        assert_eq!(diff_fps(&old, &new), (1, 2, 2));
    }

    #[test]
    fn diff_insert_in_middle() {
        let old = [1u64, 2, 3];
        let new = [1u64, 8, 2, 3];
        assert_eq!(diff_fps(&old, &new), (1, 1, 2));
    }

    #[test]
    fn diff_delete_suffix() {
        let old = [1u64, 2, 3];
        let new = [1u64, 2];
        assert_eq!(diff_fps(&old, &new), (2, 3, 2));
    }

    #[test]
    fn paragraph_ranges_no_nl() {
        assert_eq!(paragraph_ranges("hello"), vec![(0, 5)]);
    }

    #[test]
    fn paragraph_ranges_two_lines() {
        let s = "ab\ncd";
        assert_eq!(paragraph_ranges(s), vec![(0, 2), (3, 5)]);
        assert_eq!(&s[0..2], "ab");
        assert_eq!(&s[3..5], "cd");
    }

    #[test]
    fn paragraph_ranges_trailing_nl() {
        let s = "a\n";
        assert_eq!(paragraph_ranges(s), vec![(0, 2)]);
        assert_eq!(&s[0..2], "a\n");
    }

    #[test]
    fn paragraph_ranges_keeps_blank_line() {
        let s = "a\n\nb";
        assert_eq!(paragraph_ranges(s), vec![(0, 1), (2, 2), (3, 4)]);
        assert_eq!(&s[0..1], "a");
        assert_eq!(&s[2..2], "");
        assert_eq!(&s[3..4], "b");
    }

    #[test]
    fn paragraph_ranges_triple_blank() {
        let s = "x\n\n\n\ny";
        assert_eq!(
            paragraph_ranges(s),
            vec![(0, 1), (2, 2), (3, 3), (4, 4), (5, 6)]
        );
    }
}
