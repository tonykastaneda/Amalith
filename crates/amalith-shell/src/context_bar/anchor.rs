//! The "Anchor Point ▸ Convert" cluster — shown whenever one or more path
//! anchors are selected (Direct Selection, or the Pen tool over a picked
//! path). Two buttons set the selected anchors to a sharp corner or a
//! smooth point, mirroring Illustrator's Control-bar Convert pair.

use vello::kurbo::{BezPath, Circle, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::panels::Action;
use crate::text::TextContext;

use super::{baseline, Ctx, SegKind, Segment};

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::Anchor,
    applies: |ctx| ctx.anchor_sel_len > 0,
    measure: |_| 168.0,
    paint,
    hit,
};

const BTN: f64 = 22.0;

/// (corner button, smooth button) rects.
fn buttons(r: Rect) -> (Rect, Rect) {
    let cy = r.center().y;
    let x = r.x0 + 118.0;
    let corner = Rect::new(x, cy - BTN * 0.5, x + BTN, cy + BTN * 0.5);
    let smooth = Rect::new(x + BTN + 6.0, cy - BTN * 0.5, x + BTN * 2.0 + 6.0, cy + BTN * 0.5);
    (corner, smooth)
}

fn paint(scene: &mut Scene, text: &mut TextContext, r: Rect, ctx: &Ctx) {
    let th = ctx.theme;
    text.draw(scene, "Anchor Point", 11.5, th.text_dim, r.x0, baseline(r));
    text.draw(scene, "Convert", 11.5, th.text_dim, r.x0 + 74.0, baseline(r));

    let (corner, smooth) = buttons(r);
    let border = th.text_dim.with_alpha(0.5);
    for b in [corner, smooth] {
        scene.fill(Fill::NonZero, super::ID, th.bg, None, &b);
        scene.stroke(&Stroke::new(1.0), super::ID, border, None, &b);
    }

    // Corner glyph: an angular bend with square endpoints.
    let c = corner.center();
    let (w, h) = (6.0, 5.0);
    let mut bend = BezPath::new();
    bend.move_to((c.x - w, c.y + h));
    bend.line_to((c.x, c.y - h));
    bend.line_to((c.x + w, c.y + h));
    scene.stroke(&Stroke::new(1.4), super::ID, th.text, None, &bend);
    for p in [
        Point::new(c.x - w, c.y + h),
        Point::new(c.x + w, c.y + h),
        Point::new(c.x, c.y - h),
    ] {
        scene.fill(
            Fill::NonZero,
            super::ID,
            th.text,
            None,
            &Rect::from_center_size(p, (3.0, 3.0)),
        );
    }

    // Smooth glyph: a shallow arc with round endpoints.
    let s = smooth.center();
    let mut arc = BezPath::new();
    arc.move_to((s.x - 7.0, s.y + 4.0));
    arc.curve_to(
        (s.x - 3.0, s.y - 6.0),
        (s.x + 3.0, s.y - 6.0),
        (s.x + 7.0, s.y + 4.0),
    );
    scene.stroke(&Stroke::new(1.4), super::ID, th.text, None, &arc);
    for p in [Point::new(s.x - 7.0, s.y + 4.0), Point::new(s.x + 7.0, s.y + 4.0)] {
        scene.fill(Fill::NonZero, super::ID, th.text, None, &Circle::new(p, 1.8));
    }
}

fn hit(r: Rect, local: Point, _ctx: &Ctx) -> Action {
    let (corner, smooth) = buttons(r);
    if corner.contains(local) {
        Action::ConvertAnchor { smooth: false }
    } else if smooth.contains(local) {
        Action::ConvertAnchor { smooth: true }
    } else {
        Action::None
    }
}
