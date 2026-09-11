//! Area Type cluster for the options bar — Illustrator's "Area Type"
//! dropdown, shown only while a vertical Area Type object is the editing
//! focus: controls where the block of columns sits within the box's drawn
//! width (`TextData::cross_align`), an axis horizontal text has no
//! equivalent of (a horizontal line always spans the box's full width).

use crate::metrics::px as ui_px;

use amalith_core::TextAlign;
use vello::kurbo::{Point, Rect};
use vello::Scene;

use crate::panels::Action;
use crate::text::TextContext;

use super::{baseline, draw_combo, Ctx, SegKind, Segment};

/// The dropdown's four rows, in the order Illustrator's own "Area Type"
/// alignment control lists them. `Start` hugs the box's right edge (see
/// `vertical_text::cross_align_dx`'s doc comment) — that's "Right", not
/// "Left"; `End` hugs the left edge and is "Left". Mirrored in
/// `areatypedlg::ALIGNS`/`ALIGN_LABELS` and `app/action.rs`'s
/// `AreaTypeHit::Align` handler — keep all three in sync.
pub(crate) const OPTIONS: [(TextAlign, &str); 4] = [
    (TextAlign::Start, "Right"),
    (TextAlign::Center, "Center"),
    (TextAlign::End, "Left"),
    (TextAlign::JustifyAll, "Justify"),
];

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::AreaType,
    applies: |ctx| ctx.text_context && ctx.text_vertical && ctx.text_kind_is_area,
    measure: |_| ui_px(220.0),
    paint,
    hit,
};

fn combo_rect(r: Rect) -> Rect {
    let cy = r.center().y;
    let x = r.x0 + ui_px(85.0);
    Rect::new(x, cy - ui_px(11.5), x + ui_px(110.0), cy + ui_px(11.5))
}

pub(crate) fn label(align: TextAlign) -> &'static str {
    OPTIONS
        .iter()
        .find(|(a, _)| *a == align)
        .map(|(_, l)| *l)
        .unwrap_or("Right")
}

fn paint(scene: &mut Scene, text: &mut TextContext, r: Rect, ctx: &Ctx) {
    text.draw(scene, "Area Type:", 13.0, ctx.theme.text_dim, r.x0, baseline(r));
    draw_combo(scene, text, ctx.theme, combo_rect(r), label(ctx.text_cross_align));
}

fn hit(r: Rect, local: Point, _ctx: &Ctx) -> Action {
    let c = combo_rect(r);
    if c.contains(local) {
        return Action::OpenAreaAlignMenu(c);
    }
    Action::None
}
