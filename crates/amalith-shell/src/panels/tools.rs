//! Tools panel: five slots — Select, Direct Select, Pen, a Shape slot
//! that stands in for whichever primitive tool is current (press-and-hold
//! for the flyout), and Artboard — in a grid that reflows to 1 or 2
//! columns with the panel width. Fill / stroke chips sit at the bottom.

use amalith_core::{Color as CoreColor, Paint};
use vello::kurbo::{BezPath, Point, Rect};
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
/// Index of the Shape slot among [`slots`].
const SHAPE_SLOT: usize = 4;

/// The primitive tools the Shape slot collects, in flyout order.
pub const SHAPE_TOOLS: [Tool; 5] = [
    Tool::Rectangle,
    Tool::RoundedRect,
    Tool::Ellipse,
    Tool::Polygon,
    Tool::Star,
];

/// The six visible slots; the Shape slot shows `shape`'s icon.
fn slots(shape: Tool) -> [Tool; 6] {
    [
        Tool::Select,
        Tool::DirectSelect,
        Tool::Pen,
        Tool::Text,
        shape,
        Tool::Artboard,
    ]
}

fn cols(body: Rect) -> usize {
    if body.width() >= 2.0 * CELL + 6.0 {
        2
    } else {
        1
    }
}

/// Button rect for slot index `i`, row-major, grid centred in `body`.
fn cell(body: Rect, i: usize, cols: usize) -> Rect {
    let grid_w = cols as f64 * CELL;
    let x0 = body.x0 + (body.width() - grid_w).max(0.0) * 0.5;
    let (col, row) = (i % cols, i / cols);
    let x = x0 + col as f64 * CELL;
    let y = body.y0 + TOP + row as f64 * CELL;
    Rect::new(x, y, x + CELL, y + CELL)
}

/// Screen rect of the Shape slot — the flyout anchors to it.
pub fn shape_slot_rect(body: Rect) -> Rect {
    cell(body, SHAPE_SLOT, cols(body))
}

fn tool_chips(body: Rect) -> (Rect, Rect) {
    let s = 20.0;
    let cx = body.x0 + (body.width() * 0.5) - 5.0;
    let y = body.y1 - s - 14.0 - 10.0;
    let fill = Rect::new(cx - s * 0.5, y, cx + s * 0.5, y + s);
    let stroke = fill.with_origin(Point::new(fill.x0 + 11.0, fill.y0 + 11.0));
    (fill, stroke)
}

pub(super) fn paint(scene: &mut Scene, _text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let cols = cols(body);
    for (i, tool) in slots(ctx.shape_tool).into_iter().enumerate() {
        let r = cell(body, i, cols);
        let active = if i == SHAPE_SLOT {
            ctx.active_tool.is_shape()
        } else {
            tool == ctx.active_tool
        };
        if active {
            scene.fill(Fill::NonZero, ID, ctx.theme.accent, None, &r);
        } else if r.contains(ctx.pointer) {
            scene.fill(Fill::NonZero, ID, ctx.theme.accent.with_alpha(0.14), None, &r);
        }
        // Dark glyph over the gold accent so it stays legible.
        let color = if active {
            Color::from_rgb8(0x1a, 0x14, 0x00)
        } else {
            ctx.theme.text_dim
        };
        icons::draw(scene, tool.icon(), Rect::from_center_size(r.center(), (22.0, 22.0)), color);
        if i == SHAPE_SLOT {
            // Bottom-right triangle: this slot has a flyout.
            let mut t = BezPath::new();
            t.move_to((r.x1 - 6.0, r.y1 - 2.0));
            t.line_to((r.x1 - 2.0, r.y1 - 2.0));
            t.line_to((r.x1 - 2.0, r.y1 - 6.0));
            t.close_path();
            scene.fill(Fill::NonZero, ID, color, None, &t);
        }
    }

    // Fill / stroke chips.
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

pub(super) fn hit(body: Rect, local: Point, ctx: &Ctx) -> Action {
    let (fr, sr) = tool_chips(body);
    if fr.contains(local) {
        return Action::OpenPicker(PaintSlot::Fill);
    }
    if sr.contains(local) {
        return Action::OpenPicker(PaintSlot::Stroke);
    }
    let cols = cols(body);
    for (i, tool) in slots(ctx.shape_tool).into_iter().enumerate() {
        if cell(body, i, cols).contains(local) {
            return if i == SHAPE_SLOT {
                Action::ShapeSlot
            } else {
                Action::SetTool(tool)
            };
        }
    }
    Action::None
}

/// Hover text: tool name plus its keyboard shortcut.
pub(super) fn tip(body: Rect, local: Point, ctx: &Ctx) -> Option<String> {
    let (fr, sr) = tool_chips(body);
    if fr.contains(local) {
        return Some("Fill".into());
    }
    if sr.contains(local) {
        return Some("Stroke".into());
    }
    let cols = cols(body);
    for (i, tool) in slots(ctx.shape_tool).into_iter().enumerate() {
        if cell(body, i, cols).contains(local) {
            let key = tool.key();
            return Some(if key.is_empty() {
                tool.label().to_string()
            } else {
                format!("{} ({key})", tool.label())
            });
        }
    }
    None
}
