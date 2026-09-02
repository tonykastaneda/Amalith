//! The Opacity field — the same code whether a shape, a path, or text is
//! selected, so it sits in every context.

use vello::kurbo::{Point, Rect};
use vello::Scene;

use crate::panels::Action;
use crate::text::TextContext;

use super::{baseline, draw_field, field, Ctx, SegKind, Segment};

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::Opacity,
    applies: |_| true,
    measure: |_| 129.0,
    paint,
    hit,
};

/// (opacity field, up, down) rects.
fn parts(r: Rect) -> (Rect, Rect, Rect) {
    field(r.x0 + 60.0, r.center().y, 53.0)
}

fn paint(scene: &mut Scene, text: &mut TextContext, r: Rect, ctx: &Ctx) {
    let (f, up, down) = parts(r);
    text.draw(scene, "Opacity", 13.0, ctx.theme.text_dim, r.x0, baseline(r));
    let op = ctx
        .representative
        .map(|a| a.opacity)
        .unwrap_or(ctx.cur_opacity);
    draw_field(
        scene,
        text,
        ctx.theme,
        f,
        up,
        down,
        &format!("{:.0}%", op * 100.0),
    );
}

fn hit(r: Rect, local: Point, _ctx: &Ctx) -> Action {
    let (_f, up, down) = parts(r);
    if up.contains(local) {
        Action::StepOpacity(1)
    } else if down.contains(local) {
        Action::StepOpacity(-1)
    } else {
        Action::None
    }
}
