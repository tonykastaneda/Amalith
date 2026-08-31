//! Layers panel: the layer / object tree with disclosure triangles, eye
//! and lock toggles, inline rename, and a footer button strip.

use std::collections::HashSet;

use amalith_core::{Document, LayerId, ObjectId, ObjectKind, ObjectParent};
use vello::kurbo::{BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;

use super::{
    draw_name_field, panel_footer_rects, paint_panel_footer, row_rect, Action, Ctx, RenameId, FOOTER_H,
    ID, PAD, ROW_H,
};

/// Per-depth indent, and the width of each icon column (triangle, eye,
/// lock) in a Layers row.
const INDENT: f64 = 14.0;
const COL: f64 = 16.0;

/// Height reserved at the top of the panel body for the search field.
pub(super) const SEARCH_H: f64 = 34.0;

/// The layer / object rows to show, after the search filter. A blank
/// query shows the whole tree; otherwise only rows whose name contains
/// the query (case-insensitive), flattened.
fn visible_rows(ctx: &Ctx) -> Vec<LayerRow> {
    let rows = layer_rows(ctx.doc, ctx.expanded);
    let q = ctx.layer_query.trim().to_lowercase();
    if q.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|r| r.label.to_lowercase().contains(&q))
        .collect()
}

/// The search field box, inset from the panel edges.
fn search_box(body: Rect) -> Rect {
    Rect::new(body.x0 + PAD, body.y0 + 7.0, body.x1 - PAD, body.y0 + SEARCH_H - 7.0)
}

fn draw_search(scene: &mut Scene, text: &mut TextContext, ctx: &Ctx, body: Rect) {
    let th = ctx.theme;
    let box_ = search_box(body);
    scene.fill(Fill::NonZero, ID, th.bg, None, &box_);
    let border = if ctx.layer_search_focused {
        th.accent
    } else {
        th.border
    };
    scene.stroke(&Stroke::new(1.25), ID, border, None, &box_);

    // Magnifier: a ring plus a short handle.
    let cy = box_.y0 + box_.height() * 0.5;
    let gx = box_.x0 + 12.0;
    let ring = vello::kurbo::Circle::new((gx, cy - 0.5), 4.0);
    scene.stroke(&Stroke::new(1.4), ID, th.text_dim, None, &ring);
    let mut handle = BezPath::new();
    handle.move_to((gx + 3.0, cy + 2.5));
    handle.line_to((gx + 6.5, cy + 6.0));
    scene.stroke(&Stroke::new(1.4), ID, th.text_dim, None, &handle);

    let tx = box_.x0 + 24.0;
    let baseline = cy + 4.0;
    let (label, color): (&str, Color) = if ctx.layer_query.is_empty() {
        ("Search All", th.text_dim)
    } else {
        (ctx.layer_query, th.text)
    };
    text.draw(scene, label, 12.0, color, tx, baseline);

    if ctx.layer_search_focused {
        let after = if ctx.layer_query.is_empty() {
            0.0
        } else {
            text.measure(ctx.layer_query, 12.0)
        };
        let cx = tx + after + 1.0;
        scene.fill(
            Fill::NonZero,
            ID,
            th.text,
            None,
            &Rect::new(cx, box_.y0 + 4.0, cx + 1.4, box_.y1 - 4.0),
        );
    }
}

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

/// The layer that ultimately contains `id` (walking out through groups).
fn owning_layer(doc: &Document, mut id: ObjectId) -> Option<LayerId> {
    loop {
        match doc.object(id)?.parent {
            ObjectParent::Layer(l) => return Some(l),
            ObjectParent::Group(g) => id = g,
        }
    }
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

pub(super) fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    draw_search(scene, text, ctx, body);
    let list = Rect::new(body.x0, body.y0 + SEARCH_H, body.x1, body.y1);
    let hot_row = if list.contains(ctx.pointer) {
        Some(((ctx.pointer.y - list.y0) / ROW_H).floor() as i64)
    } else {
        None
    };
    for (i, row) in visible_rows(ctx).into_iter().enumerate() {
        let r = row_rect(list, i);
        let indent = body.x0 + PAD + row.depth as f64 * INDENT;

        match row.kind {
            RowKind::Layer(lid) => {
                let has_obj_sel = !ctx.selection.is_empty();
                // The layer that holds the current object selection reads
                // as "the layer you're in": bold, no blue. A plain layer
                // selection (no objects) gets the usual blue row.
                let owns = has_obj_sel
                    && ctx
                        .selection
                        .iter()
                        .any(|o| owning_layer(ctx.doc, *o) == Some(lid));
                let show_blue = !has_obj_sel && ctx.selected_layer == Some(lid);
                let fill = if show_blue {
                    ctx.theme.accent.with_alpha(0.22)
                } else {
                    ctx.theme.strip_bg
                };
                scene.fill(Fill::NonZero, ID, fill, None, &r);
                let editing = match ctx.renaming {
                    Some((RenameId::Layer(l), buf)) if l == lid => Some(buf),
                    _ => None,
                };
                if editing.is_some() {
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
                } else {
                    let baseline = r.y0 + ROW_H * 0.5 + 4.0;
                    text.draw(scene, &row.label, 12.0, ctx.theme.text, body.x0 + PAD, baseline);
                    if owns {
                        // Faux-bold: a second pass nudged half a pixel.
                        text.draw(
                            scene,
                            &row.label,
                            12.0,
                            ctx.theme.text,
                            body.x0 + PAD + 0.6,
                            baseline,
                        );
                    }
                }
            }
            RowKind::Object { id, is_group } => {
                let selected = ctx.selection.contains(&id);
                if selected {
                    scene.fill(
                        Fill::NonZero,
                        ID,
                        ctx.theme.accent.with_alpha(0.22),
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
                draw_name_field(scene, text, ctx.theme, indent + COL * 3.0, r, &row.label, name_c, editing);
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

/// Resolve a click: the footer buttons, a disclosure triangle, the eye /
/// lock columns, or the name (select).
pub(super) fn hit(body: Rect, local: Point, ctx: &Ctx) -> Action {
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
    // The search field owns the top strip.
    if local.y < body.y0 + SEARCH_H {
        return Action::FocusLayerSearch;
    }
    let rows = visible_rows(ctx);
    let i = ((local.y - (body.y0 + SEARCH_H)) / ROW_H).floor();
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
