//! Tools panel: five slots — Select, Direct Select, Pen, a Shape slot
//! that stands in for whichever primitive tool is current (press-and-hold
//! for the flyout), and Artboard — in a grid that reflows to 1 or 2
//! columns with the panel width. Fill / stroke chips sit at the bottom.

use amalith_core::{Color as CoreColor, Paint};
use vello::kurbo::{BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::icons;
use crate::text::TextContext;
use crate::theme::Theme;
use crate::tool::Tool;

use super::{Action, Ctx, PaintSlot, ID};

const SLASH_RED: Color = Color::from_rgb8(0xff, 0x18, 0x18);

/// One tool button, square.
const CELL: f64 = 36.0;
/// Gap above the grid.
const TOP: f64 = 4.0;
/// Index of the Shape slot among [`slots`].
const SHAPE_SLOT: usize = 5;

/// The primitive tools the Shape slot collects, in flyout order.
pub const SHAPE_TOOLS: [Tool; 5] = [
    Tool::Rectangle,
    Tool::RoundedRect,
    Tool::Ellipse,
    Tool::Polygon,
    Tool::Star,
];

/// The visible slots; the Shape slot shows `shape`'s icon.
fn slots(shape: Tool) -> [Tool; 12] {
    [
        Tool::Select,
        Tool::DirectSelect,
        Tool::Pen,
        Tool::Line,
        Tool::Text,
        shape,
        Tool::Artboard,
        Tool::Hand,
        Tool::Zoom,
        Tool::Eyedropper,
        Tool::Gradient,
        Tool::Rotate,
    ]
}

fn cols(body: Rect) -> usize {
    if body.width() >= 2.0 * CELL + 6.0 {
        2
    } else {
        1
    }
}

/// Shortest body that still shows every tool plus the fill / stroke chips,
/// for the splitter-drag minimum. Depends on width via the column reflow.
pub(super) fn natural_height(width: f64) -> f64 {
    let cols = if width >= 2.0 * CELL + 6.0 { 2 } else { 1 };
    let rows = 12usize.div_ceil(cols) as f64;
    // grid + the bottom-anchored Fill/Stroke proxy block (see `proxy`).
    TOP + rows * CELL + 12.0 + PROXY_H
}

/// Vertical space the colour proxy reserves at the panel bottom.
const PROXY_H: f64 = 105.0;

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

/// Screen rects of every hit target in the Fill/Stroke proxy.
struct Proxy {
    fill: Rect,
    stroke: Rect,
    swap: Rect,
    default: Rect,
    color: Rect,
    gradient: Rect,
    none: Rect,
}

fn proxy(body: Rect) -> Proxy {
    let cx = body.center().x;
    let top = body.y1 - PROXY_H + 3.0;
    let sw = 44.0;
    let fill = Rect::new(cx - 34.0, top, cx - 34.0 + sw, top + sw);
    let stroke = fill + vello::kurbo::Vec2::new(22.0, 22.0);
    let swap = Rect::new(cx + 21.0, top + 1.0, cx + 36.0, top + 16.0);
    let default = Rect::new(cx - 34.0, top + 51.0, cx - 19.0, top + 66.0);
    let mode_y = stroke.y1 + 8.0;
    let mode_w = 24.0;
    let gap = 1.0;
    let mode_x = cx - (mode_w * 3.0 + gap * 2.0) * 0.5;
    let color = Rect::new(mode_x, mode_y, mode_x + mode_w, mode_y + mode_w);
    let gradient = color + vello::kurbo::Vec2::new(mode_w + gap, 0.0);
    let none = gradient + vello::kurbo::Vec2::new(mode_w + gap, 0.0);
    Proxy {
        fill,
        stroke,
        swap,
        default,
        color,
        gradient,
        none,
    }
}

/// Fill or stroke color for the proxy: the selection's if there is one,
/// else the document's current "next object" paint.
fn slot_paints(ctx: &Ctx) -> (Paint, Paint) {
    match ctx.representative {
        Some(a) => (a.fill, a.stroke),
        None => (ctx.cur_fill, ctx.cur_stroke),
    }
}

/// One proxy swatch. The foreground colour has a white inset; the rear
/// swatch is hollow so fill and stroke stay legible where they overlap.
/// `mixed` overrides the colour with a grey "?" pattern.
fn swatch(
    scene: &mut Scene,
    text: &mut crate::text::TextContext,
    theme: &Theme,
    r: Rect,
    paint: Paint,
    hollow: bool,
    mixed: bool,
) {
    let bg = theme.panel_bg;
    if mixed {
        scene.fill(Fill::NonZero, ID, Color::from_rgb8(0x3c, 0x3c, 0x3c), None, &r);
        super::mixed_marks(scene, text, r);
        scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &r);
        return;
    }
    match paint {
        Paint::None => {
            scene.fill(Fill::NonZero, ID, Color::WHITE, None, &r);
        }
        Paint::Solid(c) => {
            if hollow {
                scene.fill(Fill::NonZero, ID, crate::convert::color(c), None, &r);
            } else {
                scene.fill(Fill::NonZero, ID, Color::WHITE, None, &r);
                let inset = Rect::new(r.x0 + 2.0, r.y0 + 2.0, r.x1 - 2.0, r.y1 - 2.0);
                scene.fill(Fill::NonZero, ID, crate::convert::color(c), None, &inset);
            }
        }
        Paint::Gradient(_) => {
            if hollow {
                super::gradient_ramp(scene, r);
            } else {
                scene.fill(Fill::NonZero, ID, Color::WHITE, None, &r);
                let inset = Rect::new(r.x0 + 2.0, r.y0 + 2.0, r.x1 - 2.0, r.y1 - 2.0);
                super::gradient_ramp(scene, inset);
            }
        }
    }
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &r);
    if hollow {
        let inner = Rect::new(r.x0 + 12.0, r.y0 + 12.0, r.x1 - 12.0, r.y1 - 12.0);
        scene.fill(Fill::NonZero, ID, bg, None, &inner);
        scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &inner);
    }
    if matches!(paint, Paint::None) {
        let mut slash = BezPath::new();
        slash.move_to((r.x0 + 1.0, r.y1 - 1.0));
        slash.line_to((r.x1 - 1.0, r.y0 + 1.0));
        scene.stroke(&Stroke::new(2.0), ID, SLASH_RED, None, &slash);
    }
}

fn paint_proxy(scene: &mut Scene, text: &mut crate::text::TextContext, body: Rect, ctx: &Ctx) {
    let th = ctx.theme;
    let p = proxy(body);
    let (fill, stroke) = slot_paints(ctx);
    let fill_active = ctx.active_slot == PaintSlot::Fill;
    let (fm, sm) = (ctx.fill_mixed, ctx.stroke_mixed);

    // Inactive swatch first so the active one sits on top.
    if fill_active {
        swatch(scene, text, th, p.stroke, stroke, true, sm);
        swatch(scene, text, th, p.fill, fill, false, fm);
    } else {
        swatch(scene, text, th, p.fill, fill, false, fm);
        swatch(scene, text, th, p.stroke, stroke, true, sm);
    }

    // Swap arrows (top-right): a right-angle elbow with a head at each end.
    let s = p.swap;
    let mut elbow = BezPath::new();
    elbow.move_to((s.x0 + 3.0, s.y1 - 2.0));
    elbow.line_to((s.x0 + 3.0, s.y0 + 4.0));
    elbow.line_to((s.x1 - 3.0, s.y0 + 4.0));
    let dim = th.text_dim;
    scene.stroke(&Stroke::new(1.6), ID, dim, None, &elbow);
    let mut head = |tip: Point, a: Point, b: Point| {
        let mut h = BezPath::new();
        h.move_to(tip);
        h.line_to(a);
        h.move_to(tip);
        h.line_to(b);
        scene.stroke(&Stroke::new(1.6), ID, dim, None, &h);
    };
    head(
        Point::new(s.x0 + 3.0, s.y1 - 2.0),
        Point::new(s.x0, s.y1 - 5.0),
        Point::new(s.x0 + 6.0, s.y1 - 5.0),
    );
    head(
        Point::new(s.x1 - 3.0, s.y0 + 4.0),
        Point::new(s.x1 - 6.0, s.y0 + 1.0),
        Point::new(s.x1 - 6.0, s.y0 + 7.0),
    );

    // Default button (bottom-left): a black square behind a white square.
    let d = p.default;
    let back = Rect::new(d.x0 + 4.0, d.y0 + 4.0, d.x1, d.y1);
    scene.fill(Fill::NonZero, ID, Color::BLACK, None, &back);
    scene.stroke(&Stroke::new(1.0), ID, th.text_dim, None, &back);
    let front = Rect::new(d.x0, d.y0, d.x1 - 4.0, d.y1 - 4.0);
    scene.fill(Fill::NonZero, ID, Color::WHITE, None, &front);
    scene.stroke(&Stroke::new(1.0), ID, th.text_dim, None, &front);

    // Fill/Stroke mode row: Color · Gradient (display-only) · None.
    let active_paint = if fill_active { fill } else { stroke };
    let mode_cell = |scene: &mut Scene, r: Rect| {
        scene.fill(Fill::NonZero, ID, th.panel_bg, None, &r);
        scene.stroke(&Stroke::new(1.0), ID, th.border, None, &r);
        let icon = Rect::new(r.x0 + 4.0, r.y0 + 4.0, r.x1 - 4.0, r.y1 - 4.0);
        scene.fill(Fill::NonZero, ID, Color::BLACK, None, &icon);
        icon
    };
    let icon_inner = |r: Rect| Rect::new(r.x0 + 2.0, r.y0 + 2.0, r.x1 - 2.0, r.y1 - 2.0);

    let color_icon = icon_inner(mode_cell(scene, p.color));
    let color = match active_paint {
        Paint::Solid(c) => crate::convert::color(c),
        Paint::None | Paint::Gradient(_) => Color::BLACK,
    };
    scene.fill(Fill::NonZero, ID, color, None, &color_icon);

    let gradient_icon = icon_inner(mode_cell(scene, p.gradient));
    for x in 0..gradient_icon.width() as i64 {
        let t = x as f32 / gradient_icon.width() as f32;
        let gray = 1.0 - t * 0.78;
        scene.fill(
            Fill::NonZero,
            ID,
            Color::new([gray, gray, gray, 1.0]),
            None,
            &Rect::new(
                gradient_icon.x0 + x as f64,
                gradient_icon.y0,
                gradient_icon.x0 + x as f64 + 1.0,
                gradient_icon.y1,
            ),
        );
    }

    let none_icon = icon_inner(mode_cell(scene, p.none));
    scene.fill(Fill::NonZero, ID, Color::WHITE, None, &none_icon);
    let mut slash = BezPath::new();
    slash.move_to((none_icon.x0, none_icon.y1));
    slash.line_to((none_icon.x1, none_icon.y0));
    scene.stroke(&Stroke::new(2.0), ID, SLASH_RED, None, &slash);
}

pub(super) fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
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
            ctx.theme.on_accent
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

    // Fill / Stroke colour proxy.
    paint_proxy(scene, text, body, ctx);
}

pub(super) fn hit(body: Rect, local: Point, ctx: &Ctx) -> Action {
    let p = proxy(body);
    if p.fill.contains(local) && !(ctx.active_slot == PaintSlot::Stroke && p.stroke.contains(local)) {
        return Action::OpenPicker(PaintSlot::Fill);
    }
    if p.stroke.contains(local) {
        return Action::OpenPicker(PaintSlot::Stroke);
    }
    if p.swap.contains(local) {
        return Action::SwapPaints;
    }
    if p.default.contains(local) {
        return Action::DefaultPaints;
    }
    if p.color.contains(local) {
        let (fill, stroke) = slot_paints(ctx);
        let active = if ctx.active_slot == PaintSlot::Fill {
            fill
        } else {
            stroke
        };
        return Action::SetPaint(match active {
            Paint::Solid(c) => Paint::Solid(c),
            Paint::None | Paint::Gradient(_) => Paint::Solid(CoreColor::rgb(0.0, 0.0, 0.0)),
        });
    }
    if p.gradient.contains(local) {
        return Action::ApplyGradientPaint;
    }
    if p.none.contains(local) {
        return Action::SetPaint(Paint::None);
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
    let p = proxy(body);
    if p.swap.contains(local) {
        return Some("Swap Fill and Stroke (X)".into());
    }
    if p.default.contains(local) {
        return Some("Default Fill and Stroke (D)".into());
    }
    if p.color.contains(local) {
        return Some("Color".into());
    }
    if p.gradient.contains(local) {
        return Some("Gradient — not yet implemented".into());
    }
    if p.none.contains(local) {
        return Some("None".into());
    }
    if p.stroke.contains(local) {
        return Some("Stroke".into());
    }
    if p.fill.contains(local) {
        return Some("Fill".into());
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
