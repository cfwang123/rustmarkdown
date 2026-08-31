use egui::{
    pos2, vec2, Color32, CornerRadius, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};

/// 工具栏线标（不依赖额外字体/图片资源）。
#[derive(Clone, Copy)]
pub enum Icon {
    New,
    Open,
    Save,
    SaveAs,
    Back,
    Forward,
    Code,
    Side,
    Preview,
    Settings,
    Toc,
    Up,
    Refresh,
}

pub fn button(ui: &mut Ui, icon: Icon, selected: bool, tip: &str) -> Response {
    let size = vec2(30.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.style().interact(&resp);
    if selected || resp.hovered() {
        let fill = if selected {
            ui.visuals().selection.bg_fill
        } else {
            visuals.weak_bg_fill
        };
        ui.painter()
            .rect_filled(rect.shrink(1.0), CornerRadius::same(4), fill);
    }
    let color = if selected {
        ui.visuals().strong_text_color()
    } else {
        visuals.text_color()
    };
    let icon_rect = Rect::from_center_size(rect.center(), vec2(16.0, 16.0));
    paint(ui.painter(), icon, icon_rect, color);
    resp.on_hover_text(tip)
}

fn paint(p: &Painter, icon: Icon, r: Rect, color: Color32) {
    let s = Stroke::new(1.35_f32, color);
    match icon {
        Icon::New => paint_new(p, r, s),
        Icon::Open => paint_open(p, r, s),
        Icon::Save => paint_save(p, r, s, false),
        Icon::SaveAs => paint_save(p, r, s, true),
        Icon::Back => paint_chevron(p, r, s, true),
        Icon::Forward => paint_chevron(p, r, s, false),
        Icon::Code => paint_code(p, r, s),
        Icon::Side => paint_side(p, r, s),
        Icon::Preview => paint_preview(p, r, s),
        Icon::Settings => paint_settings(p, r, s),
        Icon::Toc => paint_toc(p, r, s),
        Icon::Up => paint_up(p, r, s),
        Icon::Refresh => paint_refresh(p, r, s),
    }
}

fn paint_new(p: &Painter, r: Rect, s: Stroke) {
    let fold = 4.0;
    let l = r.left() + 2.0;
    let t = r.top() + 1.0;
    let rt = r.right() - 1.5;
    let b = r.bottom() - 1.0;
    let pts = [
        pos2(l, t),
        pos2(rt - fold, t),
        pos2(rt, t + fold),
        pos2(rt, b),
        pos2(l, b),
    ];
    p.add(egui::Shape::closed_line(pts.to_vec(), s));
    p.line_segment([pos2(rt - fold, t), pos2(rt - fold, t + fold)], s);
    p.line_segment([pos2(rt - fold, t + fold), pos2(rt, t + fold)], s);
    let y1 = t + 7.5;
    let y2 = t + 10.5;
    p.line_segment([pos2(l + 2.5, y1), pos2(rt - 3.5, y1)], s);
    p.line_segment([pos2(l + 2.5, y2), pos2(rt - 5.0, y2)], s);
}

fn paint_open(p: &Painter, r: Rect, s: Stroke) {
    let l = r.left() + 1.0;
    let t = r.top() + 3.0;
    let rt = r.right() - 1.0;
    let b = r.bottom() - 1.5;
    p.rect_stroke(
        Rect::from_min_max(pos2(l, t), pos2(l + 6.0, t + 4.0)),
        CornerRadius::same(1),
        s,
        StrokeKind::Middle,
    );
    p.rect_stroke(
        Rect::from_min_max(pos2(l, t + 3.5), pos2(rt, b)),
        CornerRadius::same(1),
        s,
        StrokeKind::Middle,
    );
}

fn paint_save(p: &Painter, r: Rect, s: Stroke, save_as: bool) {
    let body = if save_as {
        Rect::from_min_max(
            pos2(r.left() + 1.0, r.top() + 1.0),
            pos2(r.right() - 4.5, r.bottom() - 1.0),
        )
    } else {
        r.shrink2(vec2(2.0, 1.0))
    };
    p.rect_stroke(body, CornerRadius::same(1), s, StrokeKind::Middle);
    let shutter = Rect::from_min_max(
        pos2(body.left() + 3.0, body.top() + 1.5),
        pos2(body.right() - 3.0, body.top() + 5.0),
    );
    p.rect_stroke(shutter, CornerRadius::ZERO, s, StrokeKind::Middle);
    p.circle_stroke(pos2(body.center().x, body.bottom() - 4.0), 1.6, s);
    if save_as {
        let tip = pos2(r.right() - 1.0, r.bottom() - 2.0);
        let origin = pos2(r.right() - 6.5, r.bottom() - 7.5);
        p.arrow(origin, tip - origin, s);
    }
}

fn paint_chevron(p: &Painter, r: Rect, s: Stroke, back: bool) {
    let c = r.center();
    let tip_x = if back {
        r.left() + 2.5
    } else {
        r.right() - 2.5
    };
    let tail_x = if back {
        r.right() - 3.5
    } else {
        r.left() + 3.5
    };
    p.line_segment([pos2(tail_x, c.y), pos2(tip_x, c.y)], s);
    p.line_segment([pos2(c.x, r.top() + 3.0), pos2(tip_x, c.y)], s);
    p.line_segment([pos2(c.x, r.bottom() - 3.0), pos2(tip_x, c.y)], s);
}

fn paint_code(p: &Painter, r: Rect, s: Stroke) {
    let c = r.center();
    let left = [
        pos2(c.x - 2.0, r.top() + 2.5),
        pos2(r.left() + 1.5, c.y),
        pos2(c.x - 2.0, r.bottom() - 2.5),
    ];
    p.line_segment([left[0], left[1]], s);
    p.line_segment([left[1], left[2]], s);
    let right = [
        pos2(c.x + 2.0, r.top() + 2.5),
        pos2(r.right() - 1.5, c.y),
        pos2(c.x + 2.0, r.bottom() - 2.5),
    ];
    p.line_segment([right[0], right[1]], s);
    p.line_segment([right[1], right[2]], s);
}

fn paint_side(p: &Painter, r: Rect, s: Stroke) {
    let gap = 2.0;
    let w = (r.width() - gap) * 0.5;
    let left = Rect::from_min_size(
        r.left_top() + vec2(1.0, 1.0),
        vec2(w - 1.0, r.height() - 2.0),
    );
    let right = Rect::from_min_size(
        pos2(left.right() + gap, r.top() + 1.0),
        vec2(w - 1.0, r.height() - 2.0),
    );
    p.rect_stroke(left, CornerRadius::same(1), s, StrokeKind::Middle);
    p.rect_stroke(right, CornerRadius::same(1), s, StrokeKind::Middle);
}

fn paint_preview(p: &Painter, r: Rect, s: Stroke) {
    let c = r.center();
    let eye_w = r.width() * 0.46;
    let eye_h = r.height() * 0.28;
    let almond = [
        pos2(c.x - eye_w, c.y),
        pos2(c.x, c.y - eye_h),
        pos2(c.x + eye_w, c.y),
        pos2(c.x, c.y + eye_h),
    ];
    p.add(egui::Shape::closed_line(almond.to_vec(), s));
    p.circle_stroke(c, 2.0, s);
}

fn paint_settings(p: &Painter, r: Rect, s: Stroke) {
    let c = r.center();
    let col = s.color;
    p.circle_stroke(c, 3.4, Stroke::new(1.5_f32, col));
    for i in 0..8 {
        let a = (i as f32) * std::f32::consts::TAU / 8.0;
        let dir = Vec2::new(a.cos(), a.sin());
        p.line_segment([c + dir * 4.6, c + dir * 7.2], Stroke::new(2.0_f32, col));
    }
}

fn paint_toc(p: &Painter, r: Rect, s: Stroke) {
    let l = r.left() + 2.0;
    let rt = r.right() - 2.0;
    let y0 = r.top() + 3.0;
    p.line_segment([pos2(l, y0), pos2(rt, y0)], s);
    p.line_segment([pos2(l + 3.0, y0 + 4.5), pos2(rt - 1.0, y0 + 4.5)], s);
    p.line_segment([pos2(l + 3.0, y0 + 9.0), pos2(rt - 2.5, y0 + 9.0)], s);
    p.line_segment([pos2(l, y0 + 13.5), pos2(rt - 1.0, y0 + 13.5)], s);
}

fn paint_up(p: &Painter, r: Rect, s: Stroke) {
    let c = r.center();
    let tip = pos2(c.x, r.top() + 2.5);
    p.line_segment([pos2(c.x, r.bottom() - 2.5), tip], s);
    p.line_segment([pos2(r.left() + 3.0, c.y + 0.5), tip], s);
    p.line_segment([pos2(r.right() - 3.0, c.y + 0.5), tip], s);
}

fn paint_refresh(p: &Painter, r: Rect, s: Stroke) {
    let c = r.center();
    let rad = 5.2;
    let col = s.color;
    let sw = s.width;
    for i in 3..21 {
        let a0 = (i as f32) * std::f32::consts::TAU / 24.0 - 0.4;
        let a1 = ((i + 1) as f32) * std::f32::consts::TAU / 24.0 - 0.4;
        p.line_segment(
            [
                c + vec2(a0.cos(), a0.sin()) * rad,
                c + vec2(a1.cos(), a1.sin()) * rad,
            ],
            Stroke::new(sw, col),
        );
    }
    let tip = c + vec2(0.35_f32.cos(), 0.35_f32.sin()) * rad;
    p.line_segment([tip, tip + vec2(3.2, -1.2)], s);
    p.line_segment([tip, tip + vec2(-0.4, -3.4)], s);
}
