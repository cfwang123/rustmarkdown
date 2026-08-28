/// 标题自动编号（仅显示，不改源码）：1 / 1.1 / 1.1.1。
#[derive(Default)]
pub struct HeadingNumber {
    counters: [i32; 7],
}

impl HeadingNumber {
    pub fn next(&mut self, mut level: i32) -> String {
        if level < 1 {
            level = 1;
        }
        if level > 6 {
            level = 6;
        }
        let lv = level as usize;
        self.counters[lv] += 1;
        for i in (lv + 1)..=6 {
            self.counters[i] = 0;
        }
        for i in 1..lv {
            if self.counters[i] <= 0 {
                self.counters[i] = 1;
            }
        }
        let mut s = String::with_capacity(16);
        for i in 1..=lv {
            if i > 1 {
                s.push('.');
            }
            s.push_str(&self.counters[i].to_string());
        }
        s
    }

    /// 为标题加「编号 + 空格」前缀；空标题不编号。
    pub fn prefix_title(&mut self, level: i32, title: &str) -> String {
        if title.trim().is_empty() {
            return title.to_string();
        }
        format!("{} {}", self.next(level), title)
    }
}
