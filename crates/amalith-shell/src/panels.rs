//! Panel body content: what fills a docked panel below its tab strip.
//!
//! Kept deliberately simple — a direct `match` on the panel id rather than
//! the `Panel` trait — until the set of panels and the state they need
//! settles.

use amalith_core::{Document, ObjectId, ObjectKind};
use vello::kurbo::{Affine, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::dock::PanelId;
use crate::text::TextContext;
use crate::theme::Theme;
use crate::tool::Tool;

const ID: Affine = Affine::IDENTITY;
const ROW_H: f64 = 26.0;
const PAD: f64 = 10.0;

/// Read-only context a panel body draws from.
pub struct Ctx<'a> {
    pub theme: &'a Theme,
    pub doc: &'a Document,
    pub selection: &'a [ObjectId],
    pub active_tool: Tool,
}

/// What a click in a panel body asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    SetTool(Tool),
    Select(ObjectId),
}

/// Draw panel `id`'s body into `body`.
pub fn paint(scene: &mut Scene, text: &mut TextContext, id: PanelId, body: Rect, ctx: &Ctx) {
    match id.0 {
        "tools" => paint_tools(scene, text, body, ctx),
        "layers" => paint_layers(scene, text, body, ctx),
        "artboards" => paint_artboards(scene, text, body, ctx),
        _ => {}
    }
}

/// Resolve a click at `local` (panel-body coordinates, same space as
/// `body`) into an [`Action`].
pub fn hit(id: PanelId, body: Rect, local: vello::kurbo::Point, ctx: &Ctx) -> Action {
    let row = ((local.y - body.y0) / ROW_H).floor() as i64;
    if row < 0 {
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

fn paint_tools(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    for (i, tool) in Tool::ALL.into_iter().enumerate() {
        let r = row_rect(body, i);
        if tool == ctx.active_tool {
            scene.fill(Fill::NonZero, ID, ctx.theme.select_blue, None, &r);
        }
        let fg = if tool == ctx.active_tool {
            vello::peniko::Color::from_rgb8(0xff, 0xff, 0xff)
        } else {
            ctx.theme.text
        };
        let base = r.y0 + ROW_H * 0.5 + 4.0;
        text.draw(scene, tool.key(), 11.0, fg, body.x0 + PAD, base);
        text.draw(scene, tool.label(), 12.5, fg, body.x0 + PAD + 20.0, base);
    }
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
