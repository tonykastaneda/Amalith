//! Tools panel: a grid of tool buttons that reflows to 1 or 2 columns
//! with the panel width, plus the fill / stroke chips at the bottom.

use amalith_core::{Color as CoreColor, Paint};
use vello::kurbo::{Point, Rect};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::icons;
use crate::text::TextContext;
use crate::tool::Tool;

use super::{draw_paint_swatch, Action, Ctx, PaintSlot, ID};

/// One tool button, square.
const CELL: f64 = 36.0;
/// Gap above the grid.
const TOP: f64 = 4.0;

/// How many columns fit in `body` — 2 once it's wide enough, else 1.
fn cols(body: Rect) -> usize {
    if body.width() >= 2.0 * CELL + 6.0 {
        2
    } else {
        1
    }
}

/// The button rect for tool index `i`, row-major, grid centred in `body`.
fn cell(body: Rect, i: usize, cols: usize) -> Rect {
    let grid_w = cols as f64 * CELL;
    let x0 = body.x0 + (body.width() - grid_w).max(0.0) * 0.5;
    let (col, row) = (i % cols, i / cols);
    let x = x0 + col as f64 * CELL;
    let y = body.y0 + TOP + row as f64 * CELL;
    Rect::new(x, y, x + CELL, y + CELL)
}

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
    let cols = cols(body);
    for (i, tool) in Tool::ALL.into_iter().enumerate() {
        let r = cell(body, i, cols);
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
    let cols = cols(body);
    for (i, tool) in Tool::ALL.into_iter().enumerate() {
        if cell(body, i, cols).contains(local) {
            return Action::SetTool(tool);
        }
    }
    Action::None
}
