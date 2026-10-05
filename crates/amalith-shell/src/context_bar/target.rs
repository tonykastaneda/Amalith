//! Explicit destination for tools that can edit objects or image pixels.
use crate::metrics::px as ui_px;
use crate::panels::Action;
use crate::text::TextContext;
use crate::tool::TargetMode;
use vello::kurbo::{Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use super::{baseline, Ctx, SegKind, Segment};

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::Target,
    applies: |ctx| ctx.target_available,
    measure: |_| ui_px(202.0),
    paint,
    hit,
};

fn cells(r: Rect) -> [Rect; 3] {
    let x = r.x0 + ui_px(39.0);
    let w = (r.x1 - x) / 3.0;
    [0, 1, 2].map(|i| Rect::new(x + i as f64 * w, r.y0 + ui_px(5.0), x + (i + 1) as f64 * w, r.y1 - ui_px(5.0)))
}

fn paint(scene: &mut Scene, text: &mut TextContext, r: Rect, ctx: &Ctx) {
    text.draw(scene, "Edit", 12.0, ctx.theme.text_dim, r.x0, baseline(r));
    for (i, (cell, mode)) in cells(r).into_iter().zip([TargetMode::Auto, TargetMode::Objects, TargetMode::Pixels]).enumerate() {
        let active = if ctx.mask_active { mode == TargetMode::Pixels } else { ctx.target_mode == mode };
        if active { scene.fill(Fill::NonZero, vello::kurbo::Affine::IDENTITY, ctx.theme.accent.with_alpha(0.25), None, &cell); }
        scene.stroke(&Stroke::new(1.0), vello::kurbo::Affine::IDENTITY, ctx.theme.border, None, &cell);
        let label = match i {
            0 => "Auto",
            1 => "Objects",
            2 if ctx.mask_active => "Mask",
            _ => "Pixels",
        };
        text.draw(scene, label, 11.0, if active { ctx.theme.text } else { ctx.theme.text_dim }, cell.x0 + ui_px(3.0), baseline(cell));
    }
}

fn hit(r: Rect, p: Point, _: &Ctx) -> Action {
    for (cell, mode) in cells(r).into_iter().zip([TargetMode::Auto, TargetMode::Objects, TargetMode::Pixels]) {
        if cell.contains(p) { return Action::SetTargetMode(mode); }
    }
    Action::None
}
