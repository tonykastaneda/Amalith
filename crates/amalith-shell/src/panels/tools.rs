//! Tools panel: five slots — Select, Direct Select, Pen, a Shape slot
//! that stands in for whichever primitive tool is current (press-and-hold
//! for the flyout), and Artboard — in a grid that reflows to 1 or 2
//! columns with the panel width. Fill / stroke chips sit at the bottom.

use crate::metrics::px as ui_px;

use amalith_core::{Color as CoreColor, Paint};
use vello::kurbo::{BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::icons;
use crate::text::TextContext;
use crate::theme::Theme;
use crate::tool::Tool;

use super::{Action, Ctx, PaintSlot, ID, MIXED_SWATCH_BG};

std::thread_local! {
    /// Mirrors `Settings::hide_wip_tools` for the layout helpers below
    /// (`natural_height`, `shape_slot_rect`, `group_slot_rect` via
    /// `panels::mod.rs`'s generic `min_body_height`/`fixed_content_height`),
    /// which have no `Ctx`/`Settings` of their own to read it from — same
    /// snapshot-on-the-UI-thread idea as `crate::metrics::with`. Kept in
    /// sync by `set_hide_wip`, called at startup and whenever Preferences
    /// are committed.
    static HIDE_WIP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Update the layout snapshot above. Call whenever `Settings::hide_wip_tools`
/// may have changed (app init, Preferences ▸ OK).
pub fn set_hide_wip(v: bool) {
    HIDE_WIP.with(|c| c.set(v));
}

pub(crate) fn hide_wip() -> bool {
    HIDE_WIP.with(|c| c.get())
}

const SLASH_RED: Color = Color::from_rgb8(0xff, 0x18, 0x18);

/// One tool button, square.
fn metric_cell() -> f64 { crate::metrics::with(|m| m.panels_tools_cell) }
/// Gap above the grid.
fn metric_top() -> f64 { crate::metrics::with(|m| m.panels_tools_top) }
/// The primitive tools the Shape slot collects, in flyout order.
pub const SHAPE_TOOLS: [Tool; 5] = [
    Tool::Rectangle,
    Tool::RoundedRect,
    Tool::Ellipse,
    Tool::Polygon,
    Tool::Star,
];

/// One toolbar slot's identity. Most are a single real, clickable tool;
/// three are flyout-group slots that show whichever tool in that group
/// was last used (Shape, Rotate/Reflect, Scale/Shear, Type). `Wip` is a
/// disabled placeholder for a real Illustrator tool Amalith doesn't
/// implement yet: its icon renders dimmed and unclickable, and its name
/// (with "(WIP)" appended) shows as a hover tooltip - the toolbar
/// equivalent of app/native_menu.rs's wip() helper for menu items.
#[derive(Clone, Copy)]
enum Slot {
    Tool(Tool),
    Shape(Tool),
    Flyout(crate::tool::ToolGroup, Tool),
    Wip(&'static str, icons::Icon),
}

fn slots(shape: Tool, rotate_group: Tool, scale_group: Tool, type_group: Tool, hide_wip: bool) -> Vec<Slot> {
    use crate::tool::ToolGroup;
    use icons::Icon;
    let mut v = vec![Slot::Tool(Tool::Select), Slot::Tool(Tool::DirectSelect)];
    if !hide_wip {
        v.push(Slot::Wip("Magic Wand", Icon::MagicWand));
        v.push(Slot::Wip("Lasso", Icon::Lasso));
    }
    v.push(Slot::Tool(Tool::Pen));
    if !hide_wip {
        v.push(Slot::Wip("Curvature Pen", Icon::CurvaturePen));
    }
    v.push(Slot::Flyout(ToolGroup::Type, type_group));
    v.push(Slot::Tool(Tool::Line));
    v.push(Slot::Shape(shape));
    if !hide_wip {
        v.push(Slot::Wip("Paintbrush", Icon::Paintbrush));
        v.push(Slot::Wip("Pencil", Icon::Pencil));
    }
    v.push(Slot::Flyout(ToolGroup::RotateReflect, rotate_group));
    v.push(Slot::Flyout(ToolGroup::ScaleShear, scale_group));
    v.push(Slot::Tool(Tool::Gradient));
    if !hide_wip {
        v.push(Slot::Wip("Mesh", Icon::Mesh));
    }
    v.push(Slot::Tool(Tool::Eyedropper));
    if !hide_wip {
        v.push(Slot::Wip("Measure", Icon::Measure));
    }
    v.push(Slot::Tool(Tool::Blend));
    if !hide_wip {
        v.push(Slot::Wip("Symbol Sprayer", Icon::SymbolSprayer));
    }
    v.push(Slot::Tool(Tool::Artboard));
    if !hide_wip {
        v.push(Slot::Wip("Slice", Icon::Slice));
    }
    v.push(Slot::Tool(Tool::Hand));
    v.push(Slot::Tool(Tool::Zoom));
    v.push(Slot::Tool(Tool::Width));
    v.push(Slot::Tool(Tool::Arc));
    v.push(Slot::Tool(Tool::Spiral));
    v.push(Slot::Tool(Tool::FreeTransform));
    v.push(Slot::Tool(Tool::Join));
    v.push(Slot::Tool(Tool::ShapeBuilder));
    v.push(Slot::Tool(Tool::Eraser));
    if !hide_wip {
        v.push(Slot::Wip("Shaper", Icon::Shaper));
        v.push(Slot::Wip("Perspective Grid", Icon::PerspectiveGrid));
        v.push(Slot::Wip("Column Graph", Icon::ColumnGraph));
    }
    v
}

fn cols(body: Rect) -> usize {
    if body.width() >= 2.0 * metric_cell() + ui_px(6.0) {
        2
    } else {
        1
    }
}

/// Shortest body that still shows every tool plus the fill / stroke chips,
/// for the splitter-drag minimum. Depends on width via the column reflow.
pub fn natural_height(width: f64, hide_wip: bool) -> f64 {
    let cols = if width >= 2.0 * metric_cell() + ui_px(6.0) { 2 } else { 1 };
    let n = slots(Tool::Select, Tool::Select, Tool::Select, Tool::Select, hide_wip).len();
    let rows = n.div_ceil(cols) as f64;
    // grid + the bottom-anchored Fill/Stroke proxy block (see `proxy`).
    metric_top() + rows * metric_cell() + ui_px(12.0) + metric_proxy_h()
}

/// Vertical space the colour proxy reserves at the panel bottom.
fn metric_proxy_h() -> f64 { crate::metrics::with(|m| m.panels_tools_proxy_h) }

/// Button rect for slot index `i`, row-major, grid centred in `body`.
fn cell(body: Rect, i: usize, cols: usize) -> Rect {
    let grid_w = cols as f64 * metric_cell();
    let x0 = body.x0 + (body.width() - grid_w).max(0.0) * 0.5;
    let (col, row) = (i % cols, i / cols);
    let x = x0 + col as f64 * metric_cell();
    let y = body.y0 + metric_top() + row as f64 * metric_cell();
    Rect::new(x, y, x + metric_cell(), y + metric_cell())
}

/// Screen rect of the Shape slot — the flyout anchors to it.
pub fn shape_slot_rect(body: Rect, hide_wip: bool) -> Rect {
    let s = slots(Tool::Select, Tool::Select, Tool::Select, Tool::Select, hide_wip);
    let i = s.iter().position(|s| matches!(s, Slot::Shape(_))).unwrap_or(0);
    cell(body, i, cols(body))
}

/// Screen rect of a flyout-group slot — its labeled flyout anchors here.
pub fn group_slot_rect(body: Rect, group: crate::tool::ToolGroup, hide_wip: bool) -> Rect {
    let s = slots(Tool::Select, Tool::Select, Tool::Select, Tool::Select, hide_wip);
    let i = s
        .iter()
        .position(|s| matches!(s, Slot::Flyout(g, _) if *g == group))
        .unwrap_or(0);
    cell(body, i, cols(body))
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
    let top = body.y1 - metric_proxy_h() + ui_px(3.0);
    let sw = ui_px(44.0);
    let fill = Rect::new(cx - ui_px(34.0), top, cx - ui_px(34.0) + sw, top + sw);
    let stroke = fill + vello::kurbo::Vec2::new(ui_px(22.0), ui_px(22.0));
    let swap = Rect::new(cx + ui_px(21.0), top + 1.0, cx + ui_px(36.0), top + ui_px(16.0));
    let default = Rect::new(cx - ui_px(34.0), top + ui_px(51.0), cx - ui_px(19.0), top + ui_px(66.0));
    let mode_y = stroke.y1 + ui_px(8.0);
    let mode_w = ui_px(24.0);
    let gap = ui_px(1.0);
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
    match &ctx.representative {
        Some(a) => (a.fill(), a.stroke()),
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
        scene.fill(Fill::NonZero, ID, MIXED_SWATCH_BG, None, &r);
        super::mixed_marks(scene, text, r);
        scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &r);
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
                let inset = Rect::new(r.x0 + ui_px(2.0), r.y0 + ui_px(2.0), r.x1 - ui_px(2.0), r.y1 - ui_px(2.0));
                scene.fill(Fill::NonZero, ID, crate::convert::color(c), None, &inset);
            }
        }
        Paint::Gradient(_) => {
            if hollow {
                super::gradient_ramp(scene, r);
            } else {
                scene.fill(Fill::NonZero, ID, Color::WHITE, None, &r);
                let inset = Rect::new(r.x0 + ui_px(2.0), r.y0 + ui_px(2.0), r.x1 - ui_px(2.0), r.y1 - ui_px(2.0));
                super::gradient_ramp(scene, inset);
            }
        }
    }
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &r);
    if hollow {
        let inner = Rect::new(r.x0 + ui_px(12.0), r.y0 + ui_px(12.0), r.x1 - ui_px(12.0), r.y1 - ui_px(12.0));
        scene.fill(Fill::NonZero, ID, bg, None, &inner);
        scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &inner);
    }
    if matches!(paint, Paint::None) {
        let mut slash = BezPath::new();
        slash.move_to((r.x0 + 1.0, r.y1 - 1.0));
        slash.line_to((r.x1 - 1.0, r.y0 + 1.0));
        scene.stroke(&Stroke::new(ui_px(2.0)), ID, SLASH_RED, None, &slash);
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
    elbow.move_to((s.x0 + ui_px(3.0), s.y1 - ui_px(2.0)));
    elbow.line_to((s.x0 + ui_px(3.0), s.y0 + ui_px(4.0)));
    elbow.line_to((s.x1 - ui_px(3.0), s.y0 + ui_px(4.0)));
    let dim = th.text_dim;
    scene.stroke(&Stroke::new(ui_px(1.6)), ID, dim, None, &elbow);
    let mut head = |tip: Point, a: Point, b: Point| {
        let mut h = BezPath::new();
        h.move_to(tip);
        h.line_to(a);
        h.move_to(tip);
        h.line_to(b);
        scene.stroke(&Stroke::new(ui_px(1.6)), ID, dim, None, &h);
    };
    head(
        Point::new(s.x0 + ui_px(3.0), s.y1 - ui_px(2.0)),
        Point::new(s.x0, s.y1 - ui_px(5.0)),
        Point::new(s.x0 + ui_px(6.0), s.y1 - ui_px(5.0)),
    );
    head(
        Point::new(s.x1 - ui_px(3.0), s.y0 + ui_px(4.0)),
        Point::new(s.x1 - ui_px(6.0), s.y0 + 1.0),
        Point::new(s.x1 - ui_px(6.0), s.y0 + ui_px(7.0)),
    );

    // Default button (bottom-left): a black square behind a white square.
    let d = p.default;
    let back = Rect::new(d.x0 + ui_px(4.0), d.y0 + ui_px(4.0), d.x1, d.y1);
    scene.fill(Fill::NonZero, ID, Color::BLACK, None, &back);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.text_dim, None, &back);
    let front = Rect::new(d.x0, d.y0, d.x1 - ui_px(4.0), d.y1 - ui_px(4.0));
    scene.fill(Fill::NonZero, ID, Color::WHITE, None, &front);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.text_dim, None, &front);

    // Fill/Stroke mode row: Color · Gradient (display-only) · None.
    let active_paint = if fill_active { fill } else { stroke };
    let mode_cell = |scene: &mut Scene, r: Rect| {
        scene.fill(Fill::NonZero, ID, th.panel_bg, None, &r);
        scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &r);
        let icon = Rect::new(r.x0 + ui_px(4.0), r.y0 + ui_px(4.0), r.x1 - ui_px(4.0), r.y1 - ui_px(4.0));
        scene.fill(Fill::NonZero, ID, Color::BLACK, None, &icon);
        icon
    };
    let icon_inner = |r: Rect| Rect::new(r.x0 + ui_px(2.0), r.y0 + ui_px(2.0), r.x1 - ui_px(2.0), r.y1 - ui_px(2.0));

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
    scene.stroke(&Stroke::new(ui_px(2.0)), ID, SLASH_RED, None, &slash);
}

pub fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let cols = cols(body);
    let all = slots(ctx.shape_tool, ctx.rotate_group_tool, ctx.scale_group_tool, ctx.type_group_tool, ctx.hide_wip_tools);
    for (i, slot) in all.into_iter().enumerate() {
        let r = cell(body, i, cols);

        let Slot::Wip(_, wip_icon) = slot else {
            let tool = match slot {
                Slot::Tool(t) | Slot::Shape(t) | Slot::Flyout(_, t) => t,
                Slot::Wip(..) => unreachable!(),
            };
            let active = match slot {
                Slot::Shape(_) => ctx.active_tool.is_shape(),
                Slot::Flyout(g, _) => g.contains(ctx.active_tool),
                _ => tool == ctx.active_tool,
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
            icons::draw(scene, tool.icon(), Rect::from_center_size(r.center(), (ui_px(22.0), ui_px(22.0))), color);
            if matches!(slot, Slot::Shape(_) | Slot::Flyout(..)) {
                // Bottom-right triangle: this slot has a flyout.
                let mut t = BezPath::new();
                t.move_to((r.x1 - ui_px(6.0), r.y1 - ui_px(2.0)));
                t.line_to((r.x1 - ui_px(2.0), r.y1 - ui_px(2.0)));
                t.line_to((r.x1 - ui_px(2.0), r.y1 - ui_px(6.0)));
                t.close_path();
                scene.fill(Fill::NonZero, ID, color, None, &t);
            }
            continue;
        };
        // WIP placeholder: the real tool's icon, dimmed and unclickable —
        // never highlights on hover/active; its name only ever surfaces
        // via `tip()`.
        icons::draw(
            scene,
            wip_icon,
            Rect::from_center_size(r.center(), (ui_px(22.0), ui_px(22.0))),
            ctx.theme.text_dim.with_alpha(0.35),
        );
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
    let all = slots(ctx.shape_tool, ctx.rotate_group_tool, ctx.scale_group_tool, ctx.type_group_tool, ctx.hide_wip_tools);
    for (i, slot) in all.into_iter().enumerate() {
        if cell(body, i, cols).contains(local) {
            return match slot {
                Slot::Wip(..) => Action::None,
                Slot::Shape(_) => Action::ShapeSlot,
                Slot::Flyout(g, _) => Action::ToolFlyout(g),
                Slot::Tool(t) => Action::SetTool(t),
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
    let all = slots(ctx.shape_tool, ctx.rotate_group_tool, ctx.scale_group_tool, ctx.type_group_tool, ctx.hide_wip_tools);
    for (i, slot) in all.into_iter().enumerate() {
        if cell(body, i, cols).contains(local) {
            return Some(match slot {
                Slot::Wip(name, _) => format!("{name} (WIP)"),
                Slot::Tool(t) | Slot::Shape(t) | Slot::Flyout(_, t) => {
                    let key = t.key();
                    if key.is_empty() {
                        t.label().to_string()
                    } else {
                        format!("{} ({key})", t.label())
                    }
                }
            });
        }
    }
    None
}
