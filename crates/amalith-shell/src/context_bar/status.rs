//! Selection-count readout — "No Selection" / "N Selected". Always shown.

use vello::kurbo::Rect;
use vello::Scene;

use crate::panels::Action;
use crate::text::TextContext;

use super::{baseline, Ctx, SegKind, Segment};

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::Status,
    applies: |_| true,
    measure: |_| 106.0,
    paint,
    hit: |_, _, _| Action::None,
};

fn paint(scene: &mut Scene, text: &mut TextContext, r: Rect, ctx: &Ctx) {
    let label = match ctx.selection_len {
        0 => "No Selection".to_string(),
        1 => "1 Selected".to_string(),
        n => format!("{n} Selected"),
    };
    text.draw(scene, &label, 13.0, ctx.theme.text_dim, r.x0, baseline(r));
}
