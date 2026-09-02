//! The stroke Weight field, labelled "Stroke" — the label opens the
//! Stroke flyout. Shown alongside `fill_stroke`.

use vello::kurbo::{Line, Point, Rect, Stroke};
use vello::Scene;

use crate::panels::Action;
use crate::text::TextContext;

use super::{baseline, draw_field, field, Ctx, SegKind, Segment, ID};

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::Stroke,
    applies: |ctx| !ctx.text_context,
    measure: |_| 136.0,
    paint,
    hit,
};

/// (link, weight field, up, down) rects.
fn parts(r: Rect) -> (Rect, Rect, Rect, Rect) {
    let cy = r.center().y;
    let link = Rect::new(r.x0 - 4.0, cy - 10.5, r.x0 + 48.0, cy + 10.5);
    let (f, up, down) = field(r.x0 + 53.0, cy, 64.0);
    (link, f, up, down)
}

fn paint(scene: &mut Scene, text: &mut TextContext, r: Rect, ctx: &Ctx) {
    let theme = ctx.theme;
    let (_, f, up, down) = parts(r);
    let base = baseline(r);
    let link_color = if ctx.stroke_open {
        theme.accent
    } else {
        theme.text
    };
    text.draw(scene, "Stroke", 13.0, link_color, r.x0, base);
    let uw = text.measure("Stroke", 13.0);
    scene.stroke(
        &Stroke::new(1.0),
        ID,
        link_color.with_alpha(if ctx.stroke_open { 1.0 } else { 0.5 }),
        None,
        &Line::new((r.x0, base + 2.0), (r.x0 + uw, base + 2.0)),
    );

    let w = ctx
        .representative
        .map(|a| a.stroke_width)
        .unwrap_or(ctx.cur_weight);
    draw_field(scene, text, theme, f, up, down, &format!("{w:.1} px"));
}

fn hit(r: Rect, local: Point, _ctx: &Ctx) -> Action {
    let (link, _f, up, down) = parts(r);
    if link.contains(local) {
        Action::ToggleStrokeFlyout
    } else if up.contains(local) {
        Action::StepWeight(1)
    } else if down.contains(local) {
        Action::StepWeight(-1)
    } else {
        Action::None
    }
}
