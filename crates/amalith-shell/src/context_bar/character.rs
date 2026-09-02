//! The Character cluster — font family, style, and size — shown when text
//! is the editing focus. Clicks open the shared font dropdowns.

use vello::kurbo::{BezPath, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::panels::{character::face_label, Action, FontMenu};
use crate::text::TextContext;

use super::{baseline, draw_combo, Ctx, SegKind, Segment, ID};

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::Character,
    applies: |ctx| ctx.text_context,
    measure: |_| 575.0,
    paint,
    hit,
};

struct Parts {
    family: Rect,
    style: Rect,
    size_field: Rect,
    size_up: Rect,
    size_down: Rect,
}

fn parts(r: Rect) -> Parts {
    let cy = r.center().y;
    let combo = |x: f64, w: f64| Rect::new(x, cy - 11.5, x + w, cy + 11.5);
    let mut x = r.x0 + 85.0; // after "Character:"
    let family = combo(x, 225.0);
    x += 225.0 + 14.0;
    let style = combo(x, 133.0);
    x += 133.0 + 16.0;
    let size_up = Rect::new(x, cy - 11.5, x + 15.0, cy);
    let size_down = Rect::new(x, cy, x + 15.0, cy + 11.5);
    x += 15.0 + 5.0;
    let size_field = combo(x, 99.0);
    Parts {
        family,
        style,
        size_field,
        size_up,
        size_down,
    }
}

fn paint(scene: &mut Scene, text: &mut TextContext, r: Rect, ctx: &Ctx) {
    let theme = ctx.theme;
    let s = &ctx.text_style;
    let p = parts(r);
    text.draw(scene, "Character:", 13.0, theme.text_dim, r.x0, baseline(r));
    draw_combo(scene, text, theme, p.family, &s.family);
    draw_combo(scene, text, theme, p.style, &face_label(s.weight, s.italic));

    // Size stepper column.
    let col = Rect::new(p.size_up.x0, p.size_up.y0, p.size_up.x1, p.size_down.y1);
    scene.fill(Fill::NonZero, ID, theme.bg, None, &col);
    scene.stroke(
        &Stroke::new(1.0),
        ID,
        theme.text_dim.with_alpha(0.5),
        None,
        &col,
    );
    let cx = col.x0 + col.width() * 0.5;
    let mut up = BezPath::new();
    up.move_to((cx - 3.0, p.size_up.y1 - 3.0));
    up.line_to((cx + 3.0, p.size_up.y1 - 3.0));
    up.line_to((cx, p.size_up.y0 + 3.0));
    up.close_path();
    scene.fill(Fill::NonZero, ID, theme.text_dim, None, &up);
    let mut dn = BezPath::new();
    dn.move_to((cx - 3.0, p.size_down.y0 + 3.0));
    dn.line_to((cx + 3.0, p.size_down.y0 + 3.0));
    dn.line_to((cx, p.size_down.y1 - 3.0));
    dn.close_path();
    scene.fill(Fill::NonZero, ID, theme.text_dim, None, &dn);

    let size_str = if s.size.fract().abs() < 0.05 {
        format!("{} pt", s.size.round() as i64)
    } else {
        format!("{:.2} pt", s.size)
    };
    draw_combo(scene, text, theme, p.size_field, &size_str);
}

fn hit(r: Rect, local: Point, _ctx: &Ctx) -> Action {
    let p = parts(r);
    if p.family.contains(local) {
        Action::OpenFontMenu(FontMenu::Family, p.family)
    } else if p.style.contains(local) {
        Action::OpenFontMenu(FontMenu::Style, p.style)
    } else if p.size_field.contains(local) {
        Action::OpenFontMenu(FontMenu::Size, p.size_field)
    } else if p.size_up.contains(local) {
        Action::StepFontSize(1.0)
    } else if p.size_down.contains(local) {
        Action::StepFontSize(-1.0)
    } else {
        Action::None
    }
}
