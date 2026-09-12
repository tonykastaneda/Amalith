//! Layers panel: the layer / object tree with disclosure triangles, eye
//! and lock toggles, inline rename, and a footer button strip.

use crate::metrics::px as ui_px;

use std::collections::HashSet;

use amalith_core::{Document, LayerId, ObjectId, ObjectKind, ObjectParent};
use vello::kurbo::{BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;

use super::{
    draw_eye, draw_name_field, panel_footer_rects, paint_panel_footer, Action, Ctx, RenameId, metric_footer_h, ID,
    metric_pad, metric_row_h,
};

/// Per-depth indent, and the width of each icon column (triangle, eye,
/// lock) in a Layers row.
fn metric_indent() -> f64 { crate::metrics::with(|m| m.panels_layers_indent) }
fn metric_col() -> f64 { crate::metrics::with(|m| m.panels_layers_col) }

/// Height reserved at the top of the panel body for the search field.
pub(super) fn metric_search_h() -> f64 { crate::metrics::with(|m| m.panels_layers_search_h) }

/// The layer / object rows to show, after the search filter. A blank
/// query shows the whole tree; otherwise only rows whose name contains
/// the query (case-insensitive), flattened.
fn visible_rows(ctx: &Ctx) -> Vec<LayerRow> {
    rows_filtered(ctx.doc, ctx.expanded, ctx.layer_query)
}

fn rows_filtered(
    doc: &Document,
    expanded: &HashSet<ObjectId>,
    query: &str,
) -> Vec<LayerRow> {
    let rows = layer_rows(doc, expanded);
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|r| r.label.to_lowercase().contains(&q))
        .collect()
}

/// The search field box, inset from the panel edges.
fn search_box(body: Rect) -> Rect {
    Rect::new(body.x0 + metric_pad(), body.y0 + ui_px(7.0), body.x1 - metric_pad(), body.y0 + metric_search_h() - ui_px(7.0))
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
    scene.stroke(&Stroke::new(ui_px(1.25)), ID, border, None, &box_);

    // Magnifier: a ring plus a short handle.
    let cy = box_.y0 + box_.height() * 0.5;
    let gx = box_.x0 + ui_px(12.0);
    let ring = vello::kurbo::Circle::new((gx, cy - 0.5), ui_px(4.0));
    scene.stroke(&Stroke::new(ui_px(1.4)), ID, th.text_dim, None, &ring);
    let mut handle = BezPath::new();
    handle.move_to((gx + ui_px(3.0), cy + ui_px(2.5)));
    handle.line_to((gx + ui_px(6.5), cy + ui_px(6.0)));
    scene.stroke(&Stroke::new(ui_px(1.4)), ID, th.text_dim, None, &handle);

    let tx = box_.x0 + ui_px(24.0);
    let baseline = cy + ui_px(4.0);
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
            &Rect::new(cx, box_.y0 + ui_px(4.0), cx + 1.4, box_.y1 - ui_px(4.0)),
        );
    }
}

#[derive(Clone, Copy)]
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
    /// Layer rows only (Layer Options' "Color") — unused for object rows.
    color: amalith_core::LayerColor,
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
                color: amalith_core::LayerColor::Blue,
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
            color: layer.color,
        });
        walk(doc, ObjectParent::Layer(layer.id), 1, expanded, &mut rows);
    }
    rows
}

/// Number of rows the list would show for `doc` under the current filter.
fn row_count(doc: &Document, expanded: &HashSet<ObjectId>, query: &str) -> usize {
    let rows = layer_rows(doc, expanded);
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        rows.len()
    } else {
        rows.iter()
            .filter(|r| r.label.to_lowercase().contains(&q))
            .count()
    }
}

/// Full height the Layers panel wants: search strip + every row + footer.
/// The shell uses this to decide the wheel-scroll range.
pub(super) fn content_height(doc: &Document, expanded: &HashSet<ObjectId>, query: &str) -> f64 {
    metric_search_h() + row_count(doc, expanded, query) as f64 * metric_row_h() + metric_footer_h()
}

/// The scrollable list area (between the search strip and the footer).
fn list_rect(body: Rect) -> Rect {
    Rect::new(body.x0, body.y0 + metric_search_h(), body.x1, body.y1 - metric_footer_h())
}

/// Scroll offset clamped to what the current row count allows.
fn clamp_scroll(raw: f64, n_rows: usize, list_h: f64) -> f64 {
    let max = (n_rows as f64 * metric_row_h() - list_h).max(0.0);
    raw.clamp(0.0, max)
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

/// Where a Layers-panel drag would land.
pub(crate) struct LayerDrop {
    /// Container the dragged objects move under.
    pub parent: ObjectParent,
    /// Insertion index into `parent`'s child list, in the document's
    /// current (pre-move) state — the `Reparent` command adjusts for slots
    /// vacated by objects already in `parent`.
    pub index: usize,
    /// Visible row the indicator anchors to (`0..=rows.len()`).
    pub row: i64,
    /// Highlight `rows[row]` as the drop container instead of drawing a
    /// gap line above it.
    pub into: bool,
}

/// `ids` reordered front-to-back to match the panel's row order, so the
/// `Reparent` command receives them the way Illustrator collapses a
/// multi-row drag.
pub(crate) fn order_front_to_back(
    doc: &Document,
    expanded: &HashSet<ObjectId>,
    ids: &[ObjectId],
) -> Vec<ObjectId> {
    let set: HashSet<ObjectId> = ids.iter().copied().collect();
    let mut out: Vec<ObjectId> = layer_rows(doc, expanded)
        .into_iter()
        .filter_map(|r| match r.kind {
            RowKind::Object { id, .. } if set.contains(&id) => Some(id),
            _ => None,
        })
        .collect();
    for &id in ids {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// Would putting the dragged set under `parent` nest a dragged group
/// inside itself? (The `Reparent` command rejects it too; this keeps the
/// drop indicator from showing there.)
fn parent_blocked(doc: &Document, moved: &[ObjectId], parent: ObjectParent) -> bool {
    let mut p = parent;
    loop {
        match p {
            ObjectParent::Group(g) if moved.contains(&g) => return true,
            ObjectParent::Group(g) => match doc.object(g) {
                Some(o) => p = o.parent,
                None => return false,
            },
            ObjectParent::Layer(_) => return false,
        }
    }
}

/// Resolve the drop target for a Layers-panel drag whose pointer is at
/// `pointer` (screen px). `moved` is the set being dragged. `None` when
/// the pointer is outside the row list or the drop would be illegal.
pub(crate) fn drop_target(
    body: Rect,
    pointer: Point,
    doc: &Document,
    expanded: &HashSet<ObjectId>,
    query: &str,
    scroll_raw: f64,
    moved: &[ObjectId],
) -> Option<LayerDrop> {
    let list = list_rect(body);
    if pointer.x < list.x0 || pointer.x > list.x1 || pointer.y < list.y0 || pointer.y > list.y1 {
        return None;
    }
    let rows = rows_filtered(doc, expanded, query);
    if rows.is_empty() {
        return None;
    }
    let scroll = clamp_scroll(scroll_raw, rows.len(), list.height());
    let f = (pointer.y - list.y0 + scroll) / metric_row_h();
    if f < 0.0 {
        return None;
    }
    let i = (f.floor() as usize).min(rows.len() - 1);
    let frac = f - f.floor();
    let row = &rows[i];

    // Index of `id` within its parent's child list.
    let child_index = |id: ObjectId| -> Option<(ObjectParent, usize)> {
        let parent = doc.object(id)?.parent;
        let k = doc.children_of(parent).iter().position(|&c| c == id)?;
        Some((parent, k))
    };
    let front_of = |parent: ObjectParent| doc.children_of(parent).len();

    // Three bands: top / middle / bottom of the hovered row.
    let zone_into = frac >= 0.30 && frac < 0.70;
    let above = if zone_into { false } else { frac < 0.5 };

    let make = |parent: ObjectParent, index: usize, drop_row: i64, into: bool| {
        if parent_blocked(doc, moved, parent) {
            None
        } else {
            Some(LayerDrop {
                parent,
                index,
                row: drop_row,
                into,
            })
        }
    };

    match row.kind {
        RowKind::Layer(lid) => {
            let p = ObjectParent::Layer(lid);
            if zone_into {
                make(p, front_of(p), i as i64, true)
            } else if above {
                // Front (top) of this layer.
                make(p, front_of(p), i as i64, false)
            } else {
                // Just below the header = above the frontmost child.
                make(p, front_of(p), i as i64 + 1, false)
            }
        }
        RowKind::Object { id, is_group } => {
            let (parent, k) = child_index(id)?;
            if zone_into && is_group {
                let gp = ObjectParent::Group(id);
                make(gp, front_of(gp), i as i64, true)
            } else if zone_into {
                // Not a container — treat as the nearer gap.
                if frac < 0.5 {
                    make(parent, k + 1, i as i64, false)
                } else {
                    make(parent, k, i as i64 + 1, false)
                }
            } else if above {
                make(parent, k + 1, i as i64, false)
            } else if is_group && row.expanded {
                // Below an open group header = front of its children.
                let gp = ObjectParent::Group(id);
                make(gp, front_of(gp), i as i64 + 1, false)
            } else {
                make(parent, k, i as i64 + 1, false)
            }
        }
    }
}

fn kind_name(doc: &Document, id: ObjectId) -> String {
    match doc.object(id).map(|o| &o.kind) {
        Some(ObjectKind::Path(_)) => "Path",
        Some(ObjectKind::CompoundPath(_)) => "Compound Path",
        Some(ObjectKind::Group(g)) if g.clip.is_some() => "Clip Group",
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
    let list = list_rect(body);
    let rows = visible_rows(ctx);
    let scroll = clamp_scroll(ctx.layer_scroll, rows.len(), list.height());
    let first = (scroll / metric_row_h()).floor() as usize;
    let last = (first + (list.height() / metric_row_h()).ceil() as usize + 2).min(rows.len());
    let content_h = rows.len() as f64 * metric_row_h();

    let hot_row = if list.contains(ctx.pointer) {
        Some(((ctx.pointer.y - list.y0 + scroll) / metric_row_h()).floor() as i64)
    } else {
        None
    };

    scene.push_clip_layer(Fill::NonZero, ID, &list);
    for i in first..last {
        let row = &rows[i];
        let ry = list.y0 + i as f64 * metric_row_h() - scroll;
        let r = Rect::new(list.x0, ry, list.x1, ry + metric_row_h());
        let indent = body.x0 + metric_pad() + row.depth as f64 * metric_indent();

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
                scene.fill(
                    Fill::NonZero,
                    ID,
                    crate::convert::color(row.color.rgb()),
                    None,
                    &layer_swatch_rect(r),
                );
                let editing = match ctx.renaming {
                    Some((RenameId::Layer(l), buf)) if l == lid => Some(buf),
                    _ => None,
                };
                let name_x = layer_name_x(body);
                if editing.is_some() {
                    draw_name_field(
                        scene,
                        text,
                        ctx.theme,
                        name_x,
                        r,
                        &row.label,
                        ctx.theme.text,
                        editing,
                    );
                } else {
                    let baseline = r.y0 + metric_row_h() * 0.5 + ui_px(4.0);
                    text.draw(scene, &row.label, 12.0, ctx.theme.text, name_x, baseline);
                    if owns {
                        // Faux-bold: a second pass nudged half a pixel.
                        text.draw(
                            scene,
                            &row.label,
                            12.0,
                            ctx.theme.text,
                            name_x + 0.6,
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
                let cy = r.y0 + metric_row_h() * 0.5;
                if is_group {
                    draw_triangle(scene, indent + metric_col() * 0.5, cy, row.expanded, ctx.theme.text_dim);
                }
                let eye_c = if row.visible {
                    ctx.theme.text_dim
                } else {
                    ctx.theme.border
                };
                draw_eye(scene, indent + metric_col() * 1.5, cy, row.visible, eye_c);
                if row.locked {
                    draw_lock(scene, indent + metric_col() * 2.5, cy, ctx.theme.text);
                } else if hot_row == Some(i as i64) {
                    draw_lock(scene, indent + metric_col() * 2.5, cy, ctx.theme.border);
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
                draw_name_field(scene, text, ctx.theme, indent + metric_col() * 3.0, r, &row.label, name_c, editing);
            }
        }
    }

    // Drag-reorder indicator.
    if let Some((drop_row, into)) = ctx.layer_drop {
        let y = list.y0 + drop_row as f64 * metric_row_h() - scroll;
        if into {
            let rr = Rect::new(list.x0 + 1.0, y, list.x1 - 1.0, y + metric_row_h());
            scene.stroke(
                &Stroke::new(ui_px(2.0)),
                ID,
                ctx.theme.accent,
                None,
                &rr.to_rounded_rect(ui_px(3.0)),
            );
        } else {
            scene.fill(
                Fill::NonZero,
                ID,
                ctx.theme.accent,
                None,
                &Rect::new(list.x0 + ui_px(3.0), y - 1.0, list.x1 - ui_px(3.0), y + 1.0),
            );
            scene.fill(
                Fill::NonZero,
                ID,
                ctx.theme.accent,
                None,
                &vello::kurbo::Circle::new((list.x0 + ui_px(4.0), y), ui_px(2.5)),
            );
        }
    }
    scene.pop_layer();

    // Scroll indicator down the list's right edge.
    if content_h > list.height() + 0.5 {
        let frac = (list.height() / content_h).min(1.0);
        let th = (list.height() * frac).max(ui_px(24.0));
        let ty = list.y0 + (list.height() - th) * (scroll / (content_h - list.height()));
        scene.fill(
            Fill::NonZero,
            ID,
            ctx.theme.text_dim.with_alpha(0.5),
            None,
            &Rect::new(list.x1 - ui_px(4.0), ty, list.x1 - 1.0, ty + th).to_rounded_rect(ui_px(1.5)),
        );
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
    if local.y >= body.y1 - metric_footer_h() {
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
    if local.y < body.y0 + metric_search_h() {
        return Action::FocusLayerSearch;
    }
    let list = list_rect(body);
    let rows = visible_rows(ctx);
    let scroll = clamp_scroll(ctx.layer_scroll, rows.len(), list.height());
    let i = ((local.y - (body.y0 + metric_search_h()) + scroll) / metric_row_h()).floor();
    if i < 0.0 {
        return Action::None;
    }
    let Some(row) = rows.get(i as usize) else {
        return Action::None;
    };
    match row.kind {
        // The color swatch is its own precise square — a single click
        // still selects the layer like the rest of the row; a double
        // click (resolved by the caller) opens Layer Options instead of
        // renaming.
        RowKind::Layer(id) => {
            let ry = list.y0 + i * metric_row_h() - scroll;
            let r = Rect::new(body.x0, ry, body.x1, ry + metric_row_h());
            if layer_swatch_rect(r).contains(local) {
                Action::LayerSwatch(id)
            } else {
                Action::SelectLayer(id)
            }
        }
        RowKind::Object { id, is_group } => {
            let indent = metric_pad() + row.depth as f64 * metric_indent();
            let x = local.x - body.x0;
            if is_group && x < indent + metric_col() {
                Action::ToggleExpand(id)
            } else if x < indent + metric_col() * 2.0 {
                Action::ToggleVisible(id)
            } else if x < indent + metric_col() * 3.0 {
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
        p.move_to((cx - ui_px(3.5), cy - ui_px(2.5)));
        p.line_to((cx + ui_px(3.5), cy - ui_px(2.5)));
        p.line_to((cx, cy + ui_px(3.5)));
    } else {
        p.move_to((cx - ui_px(2.5), cy - ui_px(3.5)));
        p.line_to((cx + ui_px(3.5), cy));
        p.line_to((cx - ui_px(2.5), cy + ui_px(3.5)));
    }
    p.close_path();
    scene.fill(Fill::NonZero, ID, color, None, &p);
}

/// A layer row's color swatch — Layer Options' "Color" swatch, shown as a
/// small square at the far left of the row (Illustrator's own
/// placement), vertically centered. Double-clicking it opens Layer
/// Options; a single click still selects the layer, same as clicking
/// anywhere else in the row.
fn layer_swatch_rect(row: Rect) -> Rect {
    let size = ui_px(10.0);
    let x0 = row.x0 + ui_px(16.0);
    let y0 = row.y0 + (row.height() - size) * 0.5;
    Rect::new(x0, y0, x0 + size, y0 + size)
}

/// Where a layer row's name starts — clear of the color swatch.
fn layer_name_x(body: Rect) -> f64 {
    body.x0 + ui_px(16.0) + ui_px(10.0) + ui_px(16.0)
}

/// A small padlock centred at `(cx, cy)`.
fn draw_lock(scene: &mut Scene, cx: f64, cy: f64, color: Color) {
    let body = Rect::new(cx - ui_px(3.5), cy - 0.5, cx + ui_px(3.5), cy + ui_px(4.5));
    scene.fill(Fill::NonZero, ID, color, None, &body);
    let shackle = vello::kurbo::Arc::new(
        (cx, cy - 0.5),
        (2.4, 2.4),
        std::f64::consts::PI,
        std::f64::consts::PI,
        0.0,
    );
    scene.stroke(&Stroke::new(ui_px(1.2)), ID, color, None, &shackle);
}
