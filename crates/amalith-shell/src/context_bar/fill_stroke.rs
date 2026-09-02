//! Fill + Stroke colour chips. Shown for everything except a pure-text
//! focus (where the `character` segment takes over).

use vello::kurbo::{Point, Rect, Stroke};
use vello::Scene;

use crate::panels::{self, Action, PaintSlot};
use crate::text::TextContext;

use super::{baseline, Ctx, SegKind, Segment, ID};

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::FillStroke,
    applies: |ctx| !ctx.text_context,
    measure: |_| 124.0,
    paint,
    hit,
};

/// (fill chip, stroke chip) rects inside the segment.
fn chips(r: Rect) -> (Rect, Rect) {
    let cy = r.center().y;
    let fill = Rect::from_center_size(Point::new(r.x0 + 45.0, cy), (21.0, 21.0));
    let stroke = Rect::from_center_size(Point::new(r.x0 + 101.0, cy), (21.0, 21.0));
    (fill, stroke)
}

fn paint(scene: &mut Scene, text: &mut TextContext, r: Rect, ctx: &Ctx) {
    let theme = ctx.theme;
    let (fill, stroke) = chips(r);
    text.draw(scene, "Fill", 13.0, theme.text_dim, r.x0, baseline(r));

    let indicator = |scene: &mut Scene, chip: Rect| {
        let s = Rect::from_center_size(Point::new(chip.x1 + 13.0, chip.center().y), (13.0, 13.0));
        scene.stroke(&Stroke::new(1.0), ID, theme.text_dim, None, &s);
    };
    panels::draw_paint_swatch(
        scene,
        theme,
        fill,
        ctx.representative
            .map(|a| a.fill)
            .unwrap_or(amalith_core::Paint::Solid(amalith_core::Color::rgb(0.87, 0.87, 0.87))),
        ctx.active_slot == PaintSlot::Fill,
    );
    indicator(scene, fill);
    panels::draw_paint_swatch(
        scene,
        theme,
        stroke,
        ctx.representative
            .map(|a| a.stroke)
            .unwrap_or(amalith_core::Paint::None),
        ctx.active_slot == PaintSlot::Stroke,
    );
    indicator(scene, stroke);
}

fn hit(r: Rect, local: Point, _ctx: &Ctx) -> Action {
    let (fill, stroke) = chips(r);
    if fill.contains(local) {
        Action::OpenPicker(PaintSlot::Fill)
    } else if stroke.contains(local) {
        Action::OpenPicker(PaintSlot::Stroke)
    } else {
        Action::None
    }
}
