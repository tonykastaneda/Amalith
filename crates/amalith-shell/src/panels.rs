//! Panel body content: what fills a docked panel below its tab strip.
//!
//! Kept deliberately simple — a direct `match` on the panel id rather than
//! the `Panel` trait — until the set of panels and the state they need
//! settles.

use std::collections::HashSet;

use amalith_core::{
    Appearance, ArtboardId, Color as CoreColor, Document, LayerId, ObjectId, ObjectKind,
    ObjectParent, Paint,
};
use vello::kurbo::{Affine, BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::convert;
use crate::dock::PanelId;
use crate::icons;
use crate::text::TextContext;
use crate::theme::Theme;
use crate::tool::Tool;

const ID: Affine = Affine::IDENTITY;
const ROW_H: f64 = 26.0;
const TOOL_BTN: f64 = 40.0;
const PAD: f64 = 10.0;
const SWATCH: f64 = 22.0;

/// Which paint a swatch click targets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaintSlot {
    Fill,
    Stroke,
}

/// A renameable entity — the target of an inline panel edit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenameId {
    Layer(LayerId),
    Object(ObjectId),
    Artboard(ArtboardId),
}

/// The preset palette (plus a leading `Paint::None`).
pub fn palette() -> Vec<Paint> {
    let rgb = |r: f32, g: f32, b: f32| Paint::Solid(CoreColor::rgb(r, g, b));
    vec![
        Paint::None,
        rgb(0.0, 0.0, 0.0),
        rgb(0.33, 0.33, 0.33),
        rgb(0.6, 0.6, 0.6),
        rgb(0.85, 0.85, 0.85),
        rgb(1.0, 1.0, 1.0),
        rgb(0.90, 0.20, 0.18),
        rgb(0.96, 0.55, 0.15),
        rgb(0.98, 0.80, 0.18),
        rgb(0.40, 0.75, 0.30),
        rgb(0.18, 0.60, 0.55),
        rgb(0.20, 0.48, 0.90),
        rgb(0.42, 0.32, 0.82),
        rgb(0.80, 0.28, 0.62),
    ]
}

/// Read-only context a panel body draws from.
pub struct Ctx<'a> {
    pub theme: &'a Theme,
    pub doc: &'a Document,
    pub selection: &'a [ObjectId],
    pub active_tool: Tool,
    /// Cursor position in screen px, for hover styling.
    pub pointer: Point,
    /// Appearance of the first selected object, if any (for the swatches).
    pub representative: Option<Appearance>,
    pub active_slot: PaintSlot,
    /// Group ids the Layers panel currently shows expanded.
    pub expanded: &'a HashSet<ObjectId>,
    /// The row being inline-renamed, and its current edit buffer.
    pub renaming: Option<(RenameId, &'a str)>,
}

/// What a click in a panel body asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    SetTool(Tool),
    Select(ObjectId),
    /// Layers panel: a layer-header row was clicked.
    SelectLayer(LayerId),
    /// Artboards panel: an artboard row was clicked.
    SelectArtboard(ArtboardId),
    SetActiveSlot(PaintSlot),
    /// Open the colour picker for this slot.
    OpenPicker(PaintSlot),
    SetPaint(Paint),
    SetStrokeWidth(f64),
    /// Layers panel: flip an object's `visible` / `locked` flag.
    ToggleVisible(ObjectId),
    ToggleLocked(ObjectId),
    /// Layers panel: expand / collapse a group row.
    ToggleExpand(ObjectId),
    /// Panel footer buttons.
    NewLayer,
    NewArtboard,
    /// Layers footer: restack the selection (+1 up / −1 down).
    LayerRestack(i32),
    /// Layers footer: delete the object selection.
    DeleteObjects,
    /// Artboards footer: delete the selected artboard.
    DeleteArtboard,
}

/// Draw panel `id`'s body into `body`.
pub fn paint(scene: &mut Scene, text: &mut TextContext, id: PanelId, body: Rect, ctx: &Ctx) {
    match id.0 {
        "tools" => paint_tools(scene, text, body, ctx),
        "layers" => paint_layers(scene, text, body, ctx),
        "artboards" => paint_artboards(scene, text, body, ctx),
        "swatches" => paint_swatches(scene, text, body, ctx),
        _ => {}
    }
}

/// Resolve a click at `local` (panel-body coordinates, same space as
/// `body`) into an [`Action`].
pub fn hit(id: PanelId, body: Rect, local: Point, ctx: &Ctx) -> Action {
    if id.0 == "swatches" {
        let l = swatch_layout(body);
        if l.fill.contains(local) {
            return Action::OpenPicker(PaintSlot::Fill);
        }
        if l.stroke.contains(local) {
            return Action::OpenPicker(PaintSlot::Stroke);
        }
        for (w, r) in &l.widths {
            if r.contains(local) {
                return Action::SetStrokeWidth(*w);
            }
        }
        for (p, r) in &l.swatches {
            if r.contains(local) {
                return Action::SetPaint(*p);
            }
        }
        return Action::None;
    }
    if id.0 == "tools" {
        let (fr, sr) = tool_chips(body);
        if fr.contains(local) {
            return Action::OpenPicker(PaintSlot::Fill);
        }
        if sr.contains(local) {
            return Action::OpenPicker(PaintSlot::Stroke);
        }
    }
    if id.0 == "layers" {
        return layers_hit(body, local, ctx);
    }
    if id.0 == "artboards" {
        if local.y >= body.y1 - FOOTER_H {
            let [_, _, add, del] = panel_footer_rects(body);
            return if add.contains(local) {
                Action::NewArtboard
            } else if del.contains(local) {
                Action::DeleteArtboard
            } else {
                Action::None
            };
        }
        let row = ((local.y - body.y0) / ROW_H).floor();
        if row >= 0.0 {
            if let Some(ab) = ctx.doc.artboards().get(row as usize) {
                return Action::SelectArtboard(ab.id);
            }
        }
        return Action::None;
    }
    let unit = if id.0 == "tools" { TOOL_BTN } else { ROW_H };
    let row = ((local.y - body.y0) / unit).floor();
    if row < 0.0 {
        return Action::None;
    }
    let row = row as usize;
    match id.0 {
        "tools" => Tool::ALL
            .get(row)
            .map(|t| Action::SetTool(*t))
            .unwrap_or(Action::None),
        _ => Action::None,
    }
}

/// Resolve a click in the Layers panel: the footer buttons, a disclosure
/// triangle, the eye / lock columns, or the name (select).
fn layers_hit(body: Rect, local: Point, ctx: &Ctx) -> Action {
    if local.y >= body.y1 - FOOTER_H {
        let [up, down, add, del] = panel_footer_rects(body);
        return if up.contains(local) {
            Action::LayerRestack(1)
        } else if down.contains(local) {
            Action::LayerRestack(-1)
        } else if add.contains(local) {
            Action::NewLayer
        } else if del.contains(local) {
            Action::DeleteObjects
        } else {
            Action::None
        };
    }
    let rows = layer_rows(ctx.doc, ctx.expanded);
    let i = ((local.y - body.y0) / ROW_H).floor();
    if i < 0.0 {
        return Action::None;
    }
    let Some(row) = rows.get(i as usize) else {
        return Action::None;
    };
    match row.kind {
        RowKind::Layer(id) => Action::SelectLayer(id),
        RowKind::Object { id, is_group } => {
            let indent = PAD + row.depth as f64 * INDENT;
            let x = local.x - body.x0;
            if is_group && x < indent + COL {
                Action::ToggleExpand(id)
            } else if x < indent + COL * 2.0 {
                Action::ToggleVisible(id)
            } else if x < indent + COL * 3.0 {
                Action::ToggleLocked(id)
            } else {
                Action::Select(id)
            }
        }
    }
}

fn row_rect(body: Rect, i: usize) -> Rect {
    let y = body.y0 + i as f64 * ROW_H;
    Rect::new(body.x0, y, body.x1, y + ROW_H)
}

/// Height of a panel's bottom button strip.
const FOOTER_H: f64 = 30.0;

/// The four footer button rects, left→right: move-up, move-down, add,
/// delete — right-aligned in the strip along `body`'s bottom edge.
fn panel_footer_rects(body: Rect) -> [Rect; 4] {
    let sz = 20.0;
    let gap = 10.0;
    let cy = body.y1 - FOOTER_H * 0.5;
    std::array::from_fn(|k| {
        let cx = body.x1 - PAD - (3 - k) as f64 * (sz + gap) - sz * 0.5;
        Rect::from_center_size(Point::new(cx, cy), (sz, sz))
    })
}

fn footer_color(theme: &Theme, enabled: bool, hot: bool) -> Color {
    if !enabled {
        theme.border
    } else if hot {
        theme.text
    } else {
        theme.text_dim
    }
}

/// Draws the footer strip and its four icons. `enabled` gates each of
/// [up, down, add, delete].
fn paint_panel_footer(
    scene: &mut Scene,
    body: Rect,
    theme: &Theme,
    pointer: Point,
    enabled: [bool; 4],
) {
    let strip = Rect::new(body.x0, body.y1 - FOOTER_H, body.x1, body.y1);
    scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &strip);
    scene.fill(
        Fill::NonZero,
        ID,
        theme.border,
        None,
        &Rect::new(strip.x0, strip.y0, strip.x1, strip.y0 + 1.0),
    );
    let rects = panel_footer_rects(body);
    for (k, r) in rects.iter().enumerate() {
        let c = footer_color(theme, enabled[k], r.contains(pointer));
        match k {
            0 => draw_footer_arrow(scene, *r, true, c),
            1 => draw_footer_arrow(scene, *r, false, c),
            2 => draw_footer_plus(scene, *r, c),
            _ => draw_footer_trash(scene, *r, c),
        }
    }
}

fn draw_footer_arrow(scene: &mut Scene, r: Rect, up: bool, color: Color) {
    let cx = r.center().x;
    let (y_head, y_tail, y_tip) = if up {
        (r.y0 + 6.0, r.y1 - 3.0, r.y0 + 2.0)
    } else {
        (r.y1 - 6.0, r.y0 + 3.0, r.y1 - 2.0)
    };
    scene.stroke(
        &Stroke::new(1.6),
        ID,
        color,
        None,
        &vello::kurbo::Line::new((cx, y_tail), (cx, y_tip)),
    );
    let mut head = BezPath::new();
    head.move_to((cx - 4.0, y_head));
    head.line_to((cx, y_tip));
    head.line_to((cx + 4.0, y_head));
    scene.stroke(&Stroke::new(1.6), ID, color, None, &head);
}

fn draw_footer_plus(scene: &mut Scene, r: Rect, color: Color) {
    let c = r.center();
    scene.stroke(
        &Stroke::new(1.6),
        ID,
        color,
        None,
        &vello::kurbo::Line::new((c.x - 5.0, c.y), (c.x + 5.0, c.y)),
    );
    scene.stroke(
        &Stroke::new(1.6),
        ID,
        color,
        None,
        &vello::kurbo::Line::new((c.x, c.y - 5.0), (c.x, c.y + 5.0)),
    );
}

fn draw_footer_trash(scene: &mut Scene, r: Rect, color: Color) {
    let c = r.center();
    let can = Rect::new(c.x - 4.5, c.y - 2.5, c.x + 4.5, c.y + 6.0);
    scene.stroke(&Stroke::new(1.4), ID, color, None, &can);
    scene.stroke(
        &Stroke::new(1.4),
        ID,
        color,
        None,
        &vello::kurbo::Line::new((c.x - 7.0, c.y - 2.5), (c.x + 7.0, c.y - 2.5)),
    );
    scene.stroke(
        &Stroke::new(1.4),
        ID,
        color,
        None,
        &vello::kurbo::Line::new((c.x - 2.0, c.y - 5.0), (c.x + 2.0, c.y - 5.0)),
    );
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

fn paint_tools(scene: &mut Scene, _text: &mut TextContext, body: Rect, ctx: &Ctx) {
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
            scene.fill(
                Fill::NonZero,
                ID,
                ctx.theme.select_blue.with_alpha(0.14),
                None,
                &r,
            );
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

/// Per-depth indent, and the width of each icon column (triangle, eye,
/// lock) in a Layers row.
const INDENT: f64 = 14.0;
const COL: f64 = 16.0;

enum RowKind {
    Layer(LayerId),
    Object { id: ObjectId, is_group: bool },
}

struct LayerRow {
    label: String,
    kind: RowKind,
    depth: usize,
    visible: bool,
    locked: bool,
    /// Groups only: whether this row is currently expanded.
    expanded: bool,
}

fn layer_rows(doc: &Document, expanded: &HashSet<ObjectId>) -> Vec<LayerRow> {
    fn walk(
        doc: &Document,
        parent: ObjectParent,
        depth: usize,
        expanded: &HashSet<ObjectId>,
        rows: &mut Vec<LayerRow>,
    ) {
        // Frontmost object on top, like Illustrator's Layers panel.
        for &id in doc.children_of(parent).iter().rev() {
            let Some(obj) = doc.object(id) else { continue };
            let is_group = matches!(obj.kind, ObjectKind::Group(_));
            let is_expanded = is_group && expanded.contains(&id);
            rows.push(LayerRow {
                label: obj.name.clone().unwrap_or_else(|| kind_name(doc, id)),
                kind: RowKind::Object { id, is_group },
                depth,
                visible: obj.visible,
                locked: obj.locked,
                expanded: is_expanded,
            });
            if is_expanded {
                walk(doc, ObjectParent::Group(id), depth + 1, expanded, rows);
            }
        }
    }

    let mut rows = Vec::new();
    for layer in doc.layers() {
        rows.push(LayerRow {
            label: format!("{}  ({})", layer.name, layer.children.len()),
            kind: RowKind::Layer(layer.id),
            depth: 0,
            visible: layer.visible,
            locked: layer.locked,
            expanded: false,
        });
        walk(doc, ObjectParent::Layer(layer.id), 1, expanded, &mut rows);
    }
    rows
}

fn kind_name(doc: &Document, id: ObjectId) -> String {
    match doc.object(id).map(|o| &o.kind) {
        Some(ObjectKind::Path(_)) => "Path",
        Some(ObjectKind::CompoundPath(_)) => "Compound Path",
        Some(ObjectKind::Group(_)) => "Group",
        Some(ObjectKind::Text(_)) => "Text",
        Some(ObjectKind::Image(_)) => "Image",
        Some(ObjectKind::Symbol(_)) => "Symbol",
        None => "?",
    }
    .to_string()
}

/// Draw a row's name: either the plain `label`, or — when `editing` is
/// `Some(buffer)` — an inline text field with the buffer and a caret.
fn draw_name_field(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    x: f64,
    row: Rect,
    label: &str,
    color: Color,
    editing: Option<&str>,
) {
    let baseline = row.y0 + row.height() * 0.5 + 4.0;
    match editing {
        None => text.draw(scene, label, 12.0, color, x, baseline),
        Some(buf) => {
            let field = Rect::new(x - 4.0, row.y0 + 3.0, row.x1 - PAD, row.y1 - 3.0);
            scene.fill(Fill::NonZero, ID, theme.bg, None, &field);
            scene.stroke(&Stroke::new(1.25), ID, theme.select_blue, None, &field);
            text.draw(scene, buf, 12.0, theme.text, x, baseline);
            let caret_x = x + text.measure(buf, 12.0) + 1.0;
            scene.stroke(
                &Stroke::new(1.0),
                ID,
                theme.text,
                None,
                &vello::kurbo::Line::new(
                    (caret_x, row.y0 + 5.0),
                    (caret_x, row.y1 - 5.0),
                ),
            );
        }
    }
}

fn paint_layers(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let hot_row = if body.contains(ctx.pointer) {
        Some(((ctx.pointer.y - body.y0) / ROW_H).floor() as i64)
    } else {
        None
    };
    for (i, row) in layer_rows(ctx.doc, ctx.expanded).into_iter().enumerate() {
        let r = row_rect(body, i);
        let indent = body.x0 + PAD + row.depth as f64 * INDENT;

        match row.kind {
            RowKind::Layer(lid) => {
                scene.fill(Fill::NonZero, ID, ctx.theme.strip_bg, None, &r);
                let editing = match ctx.renaming {
                    Some((RenameId::Layer(l), buf)) if l == lid => Some(buf),
                    _ => None,
                };
                draw_name_field(
                    scene,
                    text,
                    ctx.theme,
                    body.x0 + PAD,
                    r,
                    &row.label,
                    ctx.theme.text,
                    editing,
                );
            }
            RowKind::Object { id, is_group } => {
                let selected = ctx.selection.contains(&id);
                if selected {
                    scene.fill(
                        Fill::NonZero,
                        ID,
                        ctx.theme.select_blue.with_alpha(0.22),
                        None,
                        &r,
                    );
                }
                let cy = r.y0 + ROW_H * 0.5;
                if is_group {
                    draw_triangle(scene, indent + COL * 0.5, cy, row.expanded, ctx.theme.text_dim);
                }
                let eye_c = if row.visible {
                    ctx.theme.text_dim
                } else {
                    ctx.theme.border
                };
                draw_eye(scene, indent + COL * 1.5, cy, row.visible, eye_c);
                if row.locked {
                    draw_lock(scene, indent + COL * 2.5, cy, ctx.theme.text);
                } else if hot_row == Some(i as i64) {
                    draw_lock(scene, indent + COL * 2.5, cy, ctx.theme.border);
                }
                let name_c = if !row.visible || row.locked {
                    ctx.theme.border
                } else if selected {
                    ctx.theme.text
                } else {
                    ctx.theme.text_dim
                };
                let editing = match ctx.renaming {
                    Some((RenameId::Object(o), buf)) if o == id => Some(buf),
                    _ => None,
                };
                draw_name_field(
                    scene,
                    text,
                    ctx.theme,
                    indent + COL * 3.0,
                    r,
                    &row.label,
                    name_c,
                    editing,
                );
            }
        }
    }

    let has_sel = !ctx.selection.is_empty();
    paint_panel_footer(
        scene,
        body,
        ctx.theme,
        ctx.pointer,
        [has_sel, has_sel, true, has_sel],
    );
}

/// A disclosure triangle centred at `(cx, cy)`: pointing right when
/// collapsed, down when expanded.
fn draw_triangle(scene: &mut Scene, cx: f64, cy: f64, expanded: bool, color: Color) {
    let mut p = BezPath::new();
    if expanded {
        p.move_to((cx - 3.5, cy - 2.5));
        p.line_to((cx + 3.5, cy - 2.5));
        p.line_to((cx, cy + 3.5));
    } else {
        p.move_to((cx - 2.5, cy - 3.5));
        p.line_to((cx + 3.5, cy));
        p.line_to((cx - 2.5, cy + 3.5));
    }
    p.close_path();
    scene.fill(Fill::NonZero, ID, color, None, &p);
}

/// A small eye centred at `(cx, cy)`, with a slash through it when `off`.
fn draw_eye(scene: &mut Scene, cx: f64, cy: f64, on: bool, color: Color) {
    use vello::kurbo::Ellipse;
    let outer = Ellipse::new((cx, cy), (5.0, 3.2), 0.0);
    scene.stroke(&Stroke::new(1.2), ID, color, None, &outer);
    if on {
        let pupil = Ellipse::new((cx, cy), (1.6, 1.6), 0.0);
        scene.fill(Fill::NonZero, ID, color, None, &pupil);
    } else {
        let mut slash = BezPath::new();
        slash.move_to((cx - 5.5, cy + 4.0));
        slash.line_to((cx + 5.5, cy - 4.0));
        scene.stroke(&Stroke::new(1.4), ID, color, None, &slash);
    }
}

/// A small padlock centred at `(cx, cy)`.
fn draw_lock(scene: &mut Scene, cx: f64, cy: f64, color: Color) {
    let body = Rect::new(cx - 3.5, cy - 0.5, cx + 3.5, cy + 4.5);
    scene.fill(Fill::NonZero, ID, color, None, &body);
    let shackle = vello::kurbo::Arc::new(
        (cx, cy - 0.5),
        (2.4, 2.4),
        std::f64::consts::PI,
        std::f64::consts::PI,
        0.0,
    );
    scene.stroke(&Stroke::new(1.2), ID, color, None, &shackle);
}

fn paint_artboards(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    for (i, ab) in ctx.doc.artboards().iter().enumerate() {
        let r = row_rect(body, i);
        let w = ab.rect.width().round() as i64;
        let h = ab.rect.height().round() as i64;
        text.draw(
            scene,
            &format!("{:02}", i + 1),
            12.0,
            ctx.theme.text_dim,
            body.x0 + PAD,
            r.y0 + ROW_H * 0.5 + 4.0,
        );
        let editing = match ctx.renaming {
            Some((RenameId::Artboard(a), buf)) if a == ab.id => Some(buf),
            _ => None,
        };
        draw_name_field(
            scene,
            text,
            ctx.theme,
            body.x0 + PAD + 34.0,
            r,
            &ab.name,
            ctx.theme.text,
            editing,
        );
        if editing.is_none() {
            text.draw(
                scene,
                &format!("{w} × {h}"),
                11.0,
                ctx.theme.text_dim,
                body.x1 - PAD - 90.0,
                r.y0 + ROW_H * 0.5 + 4.0,
            );
        }
    }
    // Hairline separators.
    for i in 1..ctx.doc.artboards().len() {
        let y = body.y0 + i as f64 * ROW_H;
        scene.stroke(
            &Stroke::new(1.0),
            ID,
            ctx.theme.border,
            None,
            &vello::kurbo::Line::new((body.x0, y), (body.x1, y)),
        );
    }

    // Footer: reordering artboards isn't wired yet, so up/down are
    // disabled; add and delete are always live.
    paint_panel_footer(
        scene,
        body,
        ctx.theme,
        ctx.pointer,
        [false, false, true, true],
    );
}

// ---- Swatches panel ----------------------------------------------------

const STROKE_WIDTHS: [f64; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];

struct SwatchLayout {
    fill: Rect,
    stroke: Rect,
    widths: Vec<(f64, Rect)>,
    swatches: Vec<(Paint, Rect)>,
}

fn swatch_layout(body: Rect) -> SwatchLayout {
    let fill = Rect::new(
        body.x0 + PAD,
        body.y0 + 10.0,
        body.x0 + PAD + 24.0,
        body.y0 + 34.0,
    );
    let stroke = fill.with_origin(Point::new(fill.x0 + 14.0, fill.y0 + 14.0));

    let wx = stroke.x1 + 18.0;
    let widths = STROKE_WIDTHS
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let x = wx + i as f64 * 26.0;
            (*w, Rect::new(x, body.y0 + 14.0, x + 22.0, body.y0 + 36.0))
        })
        .collect();

    let top = body.y0 + 56.0;
    let cols = (((body.width() - PAD * 2.0) / (SWATCH + 4.0)).floor() as usize).max(1);
    let swatches = palette()
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let (col, row) = (i % cols, i / cols);
            let x = body.x0 + PAD + col as f64 * (SWATCH + 4.0);
            let y = top + row as f64 * (SWATCH + 4.0);
            (p, Rect::new(x, y, x + SWATCH, y + SWATCH))
        })
        .collect();

    SwatchLayout {
        fill,
        stroke,
        widths,
        swatches,
    }
}

pub fn draw_paint_swatch(scene: &mut Scene, theme: &Theme, r: Rect, paint: Paint, active: bool) {
    match paint {
        Paint::None => {
            scene.fill(
                Fill::NonZero,
                ID,
                Color::from_rgb8(0xff, 0xff, 0xff),
                None,
                &r,
            );
            let mut slash = BezPath::new();
            slash.move_to((r.x0, r.y1));
            slash.line_to((r.x1, r.y0));
            scene.stroke(
                &Stroke::new(1.5),
                ID,
                Color::from_rgb8(0xd0, 0x30, 0x30),
                None,
                &slash,
            );
        }
        Paint::Solid(c) => {
            scene.fill(Fill::NonZero, ID, convert::color(c), None, &r);
        }
    }
    let (w, col) = if active {
        (1.5, theme.select_blue)
    } else {
        (1.0, theme.border)
    };
    scene.stroke(&Stroke::new(w), ID, col, None, &r);
}

fn paint_swatches(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let l = swatch_layout(body);
    let rep = ctx.representative;
    let white = Color::from_rgb8(0xff, 0xff, 0xff);

    // Stroke behind, fill in front (Illustrator's overlap).
    draw_paint_swatch(
        scene,
        ctx.theme,
        l.stroke,
        rep.map(|a| a.stroke).unwrap_or(Paint::None),
        ctx.active_slot == PaintSlot::Stroke,
    );
    draw_paint_swatch(
        scene,
        ctx.theme,
        l.fill,
        rep.map(|a| a.fill)
            .unwrap_or(Paint::Solid(CoreColor::rgb(0.87, 0.87, 0.87))),
        ctx.active_slot == PaintSlot::Fill,
    );

    let cur_w = rep.map(|a| a.stroke_width);
    for (w, r) in &l.widths {
        let on = cur_w.is_some_and(|c| (c - *w).abs() < 0.01);
        scene.fill(
            Fill::NonZero,
            ID,
            if on {
                ctx.theme.select_blue
            } else {
                ctx.theme.strip_bg
            },
            None,
            r,
        );
        text.draw(
            scene,
            &format!("{}", *w as i64),
            11.0,
            if on { white } else { ctx.theme.text_dim },
            r.x0 + 6.0,
            r.y0 + 15.0,
        );
    }

    for (p, r) in &l.swatches {
        draw_paint_swatch(scene, ctx.theme, *r, *p, false);
    }
}
