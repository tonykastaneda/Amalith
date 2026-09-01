//! Shape W/H + Transform X/Y — shown when objects are selected.
//!
//! Same numbers and commands as the Transform panel; this is the compact
//! extract for the options bar.

use amalith_core::xform::{self, TransformValues};
use vello::kurbo::{BezPath, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::panels::{transform::XformField, Action};
use crate::text::TextContext;

use super::{baseline, draw_field, field, Ctx, SegKind, Segment, ID};

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::Xform,
    applies: |ctx| ctx.selection_len > 0 && !ctx.text_context,
    measure: |_| 456.0,
    paint,
    hit,
};

struct Parts {
    w: Rect,
    h: Rect,
    lock: Rect,
    x: Rect,
    x_up: Rect,
    x_down: Rect,
    y: Rect,
    y_up: Rect,
    y_down: Rect,
}

fn parts(r: Rect) -> Parts {
    let cy = r.center().y;
    let mut x = r.x0 + 46.0; // after "Shape:"
    let w = Rect::new(x, cy - 10.0, x + 68.0, cy + 10.0);
    x = w.x1 + 6.0;
    let lock = Rect::new(x, cy - 10.0, x + 20.0, cy + 10.0);
    x = lock.x1 + 6.0;
    let h = Rect::new(x, cy - 10.0, x + 68.0, cy + 10.0);
    x = h.x1 + 16.0 + 62.0; // gap + "Transform"
    let (xf, x_up, x_down) = field(x + 16.0, cy, 64.0); // after "X:"
    x = x_down.x1 + 10.0;
    let (yf, y_up, y_down) = field(x + 16.0, cy, 64.0); // after "Y:"
    Parts {
        w,
        h,
        lock,
        x: xf,
        x_up,
        x_down,
        y: yf,
        y_up,
        y_down,
    }
}

fn shown(ctx: &Ctx, field: XformField, v: Option<TransformValues>) -> String {
    if let Some((f, s)) = ctx.xform_edit {
        if f == field {
            return s.to_string();
        }
    }
    let Some(v) = v else {
        return "—".into();
    };
    let n = match field {
        XformField::X => v.x,
        XformField::Y => v.y,
        XformField::W => v.w,
        XformField::H => v.h,
        _ => return "—".into(),
    };
    xform::fmt_px(n)
}

fn paint(scene: &mut Scene, text: &mut TextContext, r: Rect, ctx: &Ctx) {
    let theme = ctx.theme;
    let p = parts(r);
    let base = baseline(r);
    let v = ctx.xform;

    text.draw(scene, "Shape:", 11.5, theme.text_dim, r.x0, base);
    draw_box(scene, text, theme, p.w, &shown(ctx, XformField::W, v));
    paint_lock(scene, p.lock, ctx.xform_constrain, theme);
    draw_box(scene, text, theme, p.h, &shown(ctx, XformField::H, v));

    text.draw(scene, "Transform", 11.5, theme.text_dim, p.h.x1 + 16.0, base);
    text.draw(scene, "X:", 11.5, theme.text, p.x.x0 - 16.0, base);
    draw_field(
        scene,
        text,
        theme,
        p.x,
        p.x_up,
        p.x_down,
        &shown(ctx, XformField::X, v),
    );
    text.draw(scene, "Y:", 11.5, theme.text, p.y.x0 - 16.0, base);
    draw_field(
        scene,
        text,
        theme,
        p.y,
        p.y_up,
        p.y_down,
        &shown(ctx, XformField::Y, v),
    );
}

fn draw_box(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &crate::theme::Theme,
    r: Rect,
    value: &str,
) {
    let border = theme.text_dim.with_alpha(0.5);
    scene.fill(Fill::NonZero, ID, theme.bg, None, &r);
    scene.stroke(&Stroke::new(1.0), ID, border, None, &r);
    text.draw(
        scene,
        value,
        11.5,
        theme.text,
        r.x0 + 6.0,
        r.y0 + r.height() * 0.5 + 4.0,
    );
}

fn paint_lock(scene: &mut Scene, r: Rect, on: bool, theme: &crate::theme::Theme) {
    let col = if on {
        theme.accent
    } else {
        theme.text_dim.with_alpha(0.7)
    };
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &r);
    let c = r.center();
    if on {
        // Linked chain: two rings.
        let a = Rect::from_center_size(Point::new(c.x, c.y - 3.0), (7.0, 8.0));
        let b = Rect::from_center_size(Point::new(c.x, c.y + 3.0), (7.0, 8.0));
        scene.stroke(&Stroke::new(1.4), ID, col, None, &a.to_rounded_rect(2.5));
        scene.stroke(&Stroke::new(1.4), ID, col, None, &b.to_rounded_rect(2.5));
    } else {
        // Broken link — a slash through a ring.
        let a = Rect::from_center_size(c, (8.0, 10.0));
        scene.stroke(&Stroke::new(1.4), ID, col, None, &a.to_rounded_rect(2.5));
        let mut slash = BezPath::new();
        slash.move_to((c.x - 5.0, c.y + 6.0));
        slash.line_to((c.x + 5.0, c.y - 6.0));
        scene.stroke(&Stroke::new(1.4), ID, col, None, &slash);
    }
}

fn hit(r: Rect, local: Point, _ctx: &Ctx) -> Action {
    let p = parts(r);
    if p.lock.contains(local) {
        return Action::ToggleXformConstrain;
    }
    if p.w.contains(local) {
        return Action::BeginXformEdit(XformField::W);
    }
    if p.h.contains(local) {
        return Action::BeginXformEdit(XformField::H);
    }
    if p.x_up.contains(local) {
        return Action::NudgeXform {
            field: XformField::X,
            delta: 1.0,
        };
    }
    if p.x_down.contains(local) {
        return Action::NudgeXform {
            field: XformField::X,
            delta: -1.0,
        };
    }
    if p.y_up.contains(local) {
        return Action::NudgeXform {
            field: XformField::Y,
            delta: 1.0,
        };
    }
    if p.y_down.contains(local) {
        return Action::NudgeXform {
            field: XformField::Y,
            delta: -1.0,
        };
    }
    if p.x.contains(local) {
        return Action::BeginXformEdit(XformField::X);
    }
    if p.y.contains(local) {
        return Action::BeginXformEdit(XformField::Y);
    }
    Action::None
}

/// Which numeric field the pointer is over, for scroll-to-nudge.
pub fn field_at(r: Rect, p: Point) -> Option<XformField> {
    let s = parts(r);
    if s.w.contains(p) {
        return Some(XformField::W);
    }
    if s.h.contains(p) {
        return Some(XformField::H);
    }
    if s.x.contains(p) || s.x_up.contains(p) || s.x_down.contains(p) {
        return Some(XformField::X);
    }
    if s.y.contains(p) || s.y_up.contains(p) || s.y_down.contains(p) {
        return Some(XformField::Y);
    }
    None
}
