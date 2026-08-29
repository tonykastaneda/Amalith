//! Tools panel: the vertical tool-button strip plus the fill / stroke
//! chips at the bottom.

use amalith_core::{Color as CoreColor, Paint};
use vello::kurbo::{Point, Rect};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::icons;
use crate::text::TextContext;
use crate::tool::Tool;

use super::{draw_paint_swatch, Action, Ctx, PaintSlot, ID, TOOL_BTN};

/// The overlapping fill / stroke chips at the bottom of the tool strip.
fn tool_chips(body: Rect) -> (Rect, Rect) {
    let s = 20.0;
    let cx = body.x0 + (body.width() * 0.5) - 5.0;
    let y = body.y1 - s - 14.0 - 10.0;
    let fill = Rect::new(cx - s * 0.5, y, cx + s * 0.5, y + s);
    let stroke = fill.with_origin(Point::new(fill.x0 + 11.0, fill.y0 + 11.0));
    (fill, stroke)
}

pub(super) fn paint(scene: &mut Scene, _text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let white = Color::from_rgb8(0xff, 0xff, 0xff);
    for (i, tool) in Tool::ALL.into_iter().enumerate() {
        let r = Rect::new(
            body.x0,
            body.y0 + i as f64 * TOOL_BTN,
            body.x1,
            body.y0 + (i + 1) as f64 * TOOL_BTN,
        );
        let active = tool == ctx.active_tool;
        if active {
            scene.fill(Fill::NonZero, ID, ctx.theme.select_blue, None, &r);
        } else if r.contains(ctx.pointer) {
            scene.fill(Fill::NonZero, ID, ctx.theme.select_blue.with_alpha(0.14), None, &r);
        }
        let color = if active { white } else { ctx.theme.text_dim };
        let icon_box = Rect::from_center_size(r.center(), (22.0, 22.0));
        icons::draw(scene, tool.icon(), icon_box, color);
    }

    // Fill / stroke chips — same as the Swatches panel, so the current
    // paints are visible (and slot-switchable) from the tool strip.
    let (fr, sr) = tool_chips(body);
    let rep = ctx.representative;
    draw_paint_swatch(
        scene,
        ctx.theme,
        sr,
        rep.map(|a| a.stroke).unwrap_or(Paint::None),
        ctx.active_slot == PaintSlot::Stroke,
    );
    draw_paint_swatch(
        scene,
        ctx.theme,
        fr,
        rep.map(|a| a.fill)
            .unwrap_or(Paint::Solid(CoreColor::rgb(0.87, 0.87, 0.87))),
        ctx.active_slot == PaintSlot::Fill,
    );
}

pub(super) fn hit(body: Rect, local: Point, _ctx: &Ctx) -> Action {
    let (fr, sr) = tool_chips(body);
    if fr.contains(local) {
        return Action::OpenPicker(PaintSlot::Fill);
    }
    if sr.contains(local) {
        return Action::OpenPicker(PaintSlot::Stroke);
    }
    let row = ((local.y - body.y0) / TOOL_BTN).floor();
    if row < 0.0 {
        return Action::None;
    }
    Tool::ALL
        .get(row as usize)
        .map(|t| Action::SetTool(*t))
        .unwrap_or(Action::None)
}
