//! 侧边预览双向同步滚动（对齐 docview `PREVIEW_SYNC_SUPPRESS_MS`）。

use std::time::{Duration, Instant};

pub const SUPPRESS: Duration = Duration::from_millis(650);
const EPS: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Editor,
    Preview,
}

pub struct Guard {
    origin: Origin,
    until: Instant,
}

impl Guard {
    pub fn after(origin: Origin) -> Self {
        Self {
            origin,
            until: Instant::now() + SUPPRESS,
        }
    }

    pub fn active(&self) -> bool {
        Instant::now() < self.until
    }

    pub fn origin(&self) -> Origin {
        self.origin
    }
}

/// 该侧是否为用户滚动。
/// `blocked`：对侧程序化滚动引起的本侧位移应忽略。
/// 仅一侧在动时不要求 hover（滚动条可能落在 pane 外几个像素）。
pub fn user_scrolled(
    prev: f32,
    now: f32,
    other_prev: f32,
    other_now: f32,
    hovered: bool,
    armed: bool,
    blocked: bool,
) -> bool {
    if !armed || blocked {
        return false;
    }
    if (now - prev).abs() <= EPS {
        return false;
    }
    let other_moved = (other_now - other_prev).abs() > EPS;
    hovered || !other_moved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_until_armed() {
        assert!(!user_scrolled(0.0, 80.0, 0.0, 0.0, true, false, false));
        assert!(user_scrolled(0.0, 80.0, 0.0, 0.0, true, true, false));
    }

    #[test]
    fn ignore_when_blocked() {
        assert!(!user_scrolled(0.0, 80.0, 0.0, 0.0, true, true, true));
    }

    #[test]
    fn one_side_move_without_hover() {
        assert!(user_scrolled(0.0, 80.0, 10.0, 10.0, false, true, false));
        assert!(!user_scrolled(0.0, 80.0, 10.0, 90.0, false, true, false));
        assert!(user_scrolled(0.0, 80.0, 10.0, 90.0, true, true, false));
    }

    #[test]
    fn ignore_tiny_jitter() {
        assert!(!user_scrolled(10.0, 10.4, 0.0, 0.0, true, true, false));
    }
}
