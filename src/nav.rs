//! 跳转历史：后退 / 前进（对齐 mdview jumplist + 跨文件栈）。

use std::path::PathBuf;

const MAX: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavPoint {
    pub tab_id: u64,
    pub path: Option<PathBuf>,
    pub line: usize,
}

#[derive(Clone, Debug, Default)]
pub struct NavHist {
    back: Vec<NavPoint>,
    fwd: Vec<NavPoint>,
}

impl NavHist {
    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_fwd(&self) -> bool {
        !self.fwd.is_empty()
    }

    pub fn push(&mut self, here: NavPoint) {
        if self.back.last() == Some(&here) {
            return;
        }
        self.back.push(here);
        self.fwd.clear();
        while self.back.len() > MAX {
            self.back.remove(0);
        }
    }

    pub fn go_back(&mut self, here: NavPoint) -> Option<NavPoint> {
        let t = self.back.pop()?;
        if self.fwd.last() != Some(&here) {
            self.fwd.push(here);
        }
        while self.fwd.len() > MAX {
            self.fwd.remove(0);
        }
        Some(t)
    }

    pub fn go_fwd(&mut self, here: NavPoint) -> Option<NavPoint> {
        let t = self.fwd.pop()?;
        if self.back.last() != Some(&here) {
            self.back.push(here);
        }
        while self.back.len() > MAX {
            self.back.remove(0);
        }
        Some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: u64, line: usize) -> NavPoint {
        NavPoint {
            tab_id: id,
            path: None,
            line,
        }
    }

    #[test]
    fn back_and_fwd() {
        let mut h = NavHist::default();
        assert!(!h.can_back());
        h.push(p(1, 0));
        h.push(p(1, 10));
        assert!(h.can_back());
        let t = h.go_back(p(1, 20)).unwrap();
        assert_eq!(t.line, 10);
        assert!(h.can_fwd());
        let t = h.go_fwd(p(1, 0)).unwrap();
        assert_eq!(t.line, 20);
        assert!(!h.can_fwd());
    }

    #[test]
    fn push_clears_fwd() {
        let mut h = NavHist::default();
        h.push(p(1, 0));
        let _ = h.go_back(p(1, 5));
        h.push(p(1, 8));
        assert!(!h.can_fwd());
    }

    #[test]
    fn skip_duplicate_push() {
        let mut h = NavHist::default();
        h.push(p(1, 3));
        h.push(p(1, 3));
        assert_eq!(h.back.len(), 1);
    }
}
