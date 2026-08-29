//! Panel body content: what fills a docked panel below its tab strip.
//!
//! Kept deliberately simple — a direct `match` on the panel id rather than
//! the `Panel` trait — until the set of panels and the state they need
//! settles.

use amalith_core::{Appearance, Color as CoreColor, Document, ObjectId, ObjectKind, Paint};
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
}

/// What a click in a panel body asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    SetTool(Tool),
    Select(ObjectId),
    SetActiveSlot(PaintSlot),
    /// Open the colour picker for this slot.
    OpenPicker(PaintSlot),
    SetPaint(Paint),
    SetStrokeWidth(f64),
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
        "layers" => layer_rows(ctx.doc)
            .get(row)
            .and_then(|r| r.object)
            .map(Action::Select)
            .unwrap_or(Action::None),
        _ => Action::None,
    }
}

fn row_rect(body: Rect, i: usize) -> Rect {
    let y = body.y0 + i as f64 * ROW_H;
    Rect::new(body.x0, y, body.x1, y + ROW_H)
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

struct LayerRow {
    label: String,
    object: Option<ObjectId>,
    indent: f64,
}

fn layer_rows(doc: &Document) -> Vec<LayerRow> {
    let mut rows = Vec::new();
    for layer in doc.layers() {
        let n = layer.children.len();
        rows.push(LayerRow {
            label: format!("{}  ({n})", layer.name),
            object: None,
            indent: 0.0,
        });
        for &id in &layer.children {
            let name = doc
                .object(id)
                .and_then(|o| o.name.clone())
                .unwrap_or_else(|| kind_name(doc, id));
            rows.push(LayerRow {
                label: name,
                object: Some(id),
                indent: 16.0,
            });
        }
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

fn paint_layers(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    for (i, row) in layer_rows(ctx.doc).into_iter().enumerate() {
        let r = row_rect(body, i);
        let selected = row.object.is_some_and(|id| ctx.selection.contains(&id));
        if selected {
            scene.fill(
                Fill::NonZero,
                ID,
                ctx.theme.select_blue.with_alpha(0.22),
                None,
                &r,
            );
        }
        let color = if row.object.is_none() {
            ctx.theme.text
        } else {
            ctx.theme.text_dim
        };
        text.draw(
            scene,
            &row.label,
            12.0,
            color,
            body.x0 + PAD + row.indent,
            r.y0 + ROW_H * 0.5 + 4.0,
        );
    }
}

fn paint_artboards(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    for (i, ab) in ctx.doc.artboards().iter().enumerate() {
        let r = row_rect(body, i);
        let _ = &r;
        let w = ab.rect.width().round() as i64;
        let h = ab.rect.height().round() as i64;
        text.draw(
            scene,
            &format!("{:02}  {}", i + 1, ab.name),
            12.0,
            ctx.theme.text,
            body.x0 + PAD,
            body.y0 + i as f64 * ROW_H + ROW_H * 0.5 + 4.0,
        );
        text.draw(
            scene,
            &format!("{w} × {h}"),
            11.0,
            ctx.theme.text_dim,
            body.x1 - PAD - 90.0,
            body.y0 + i as f64 * ROW_H + ROW_H * 0.5 + 4.0,
        );
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

fn draw_paint_swatch(scene: &mut Scene, theme: &Theme, r: Rect, paint: Paint, active: bool) {
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
