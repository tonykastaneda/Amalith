//! Layers panel: the layer / object tree with disclosure triangles, eye
//! and lock toggles, inline rename, and a footer button strip.

use crate::metrics::px as ui_px;

use std::collections::HashSet;

use amalith_core::{Document, LayerId, LayerKind, ObjectId, ObjectKind, ObjectParent};
use vello::kurbo::{BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;

use super::{
    draw_eye, draw_name_field, footer_color, panel_footer_rects, paint_panel_footer, Action, Ctx, MenuEntry, RenameId,
    metric_footer_h, ID, metric_pad,
};

/// Per-depth indent, and the width of each icon column (triangle, eye,
/// lock) in a Layers row.
fn metric_indent() -> f64 { crate::metrics::with(|m| m.panels_layers_indent) }
fn metric_col() -> f64 { crate::metrics::with(|m| m.panels_layers_col) }

/// Height reserved at the top of the panel body for the search field.
pub(super) fn metric_search_h() -> f64 { crate::metrics::with(|m| m.panels_layers_search_h) }

/// The layer / object rows to show, after the search filter. A blank
/// query shows the whole tree; otherwise only rows whose name contains
/// the query (case-insensitive), flattened. Empty while the focused pane
/// has no document (`ctx.document_open`) — `ctx.doc` is then leftover
/// boot/last-document state, not something to present.
fn visible_rows(ctx: &Ctx) -> Vec<LayerRow> {
    if !ctx.document_open {
        return Vec::new();
    }
    rows_filtered(ctx.doc, ctx.expanded, ctx.collapsed_layers, ctx.layer_query)
}

fn rows_filtered(
    doc: &Document,
    expanded: &HashSet<ObjectId>, collapsed_layers: &HashSet<LayerId>,
    query: &str,
) -> Vec<LayerRow> {
    let rows = layer_rows(doc, expanded, collapsed_layers);
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
    let live = ctx.document_open;
    let round = box_.to_rounded_rect(ui_px(5.0));
    scene.fill(Fill::NonZero, ID, th.strip_bg, None, &round);
    let border = if !live {
        th.border.with_alpha(0.45)
    } else if ctx.layer_search_focused {
        th.accent
    } else {
        th.border
    };
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, border, None, &round);

    let ink = if live { th.text_dim } else { th.border };
    let cy = box_.y0 + box_.height() * 0.5;
    let gx = box_.x0 + ui_px(12.0);
    let ring = vello::kurbo::Circle::new((gx, cy - 0.5), ui_px(4.0));
    scene.stroke(&Stroke::new(ui_px(1.4)), ID, ink, None, &ring);
    let mut handle = BezPath::new();
    handle.move_to((gx + ui_px(3.0), cy + ui_px(2.5)));
    handle.line_to((gx + ui_px(6.5), cy + ui_px(6.0)));
    scene.stroke(&Stroke::new(ui_px(1.4)), ID, ink, None, &handle);

    let tx = box_.x0 + ui_px(24.0);
    let baseline = cy + ui_px(4.0);
    let (label, color): (&str, Color) = if !live {
        ("Search layers", th.border)
    } else if ctx.layer_query.is_empty() {
        ("Search layers", th.text_dim)
    } else {
        (ctx.layer_query, th.text)
    };
    text.draw(scene, label, 12.0, color, tx, baseline);

    if live && ctx.layer_search_focused {
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
    /// Whether this layer or group is currently expanded.
    expanded: bool,
    /// Layer rows only (Layer Options' "Color") — unused for object rows.
    color: amalith_core::LayerColor,
    /// Layer rows only — Vector vs Raster badge.
    layer_kind: LayerKind,
}

fn layer_rows(doc: &Document, expanded: &HashSet<ObjectId>, collapsed_layers: &HashSet<LayerId>) -> Vec<LayerRow> {
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
                layer_kind: LayerKind::Vector,
            });
            if is_expanded {
                walk(doc, ObjectParent::Group(id), depth + 1, expanded, rows);
            }
        }
    }

    let mut rows = Vec::new();
    for layer in doc.layers() {
        rows.push(LayerRow {
            label: layer.name.clone(),
            kind: RowKind::Layer(layer.id),
            depth: 0,
            visible: layer.visible,
            locked: layer.locked,
            expanded: !collapsed_layers.contains(&layer.id),
            color: layer.color,
            layer_kind: layer.kind,
        });
        if !collapsed_layers.contains(&layer.id) {
            walk(doc, ObjectParent::Layer(layer.id), 1, expanded, &mut rows);
        }
    }
    rows
}

/// Number of rows the list would show for `doc` under the current filter.
fn row_count(doc: &Document, expanded: &HashSet<ObjectId>, collapsed_layers: &HashSet<LayerId>, query: &str) -> usize {
    let rows = layer_rows(doc, expanded, collapsed_layers);
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
/// The shell uses this to decide the wheel-scroll range. No rows while
/// the focused pane has no document.
pub(super) fn content_height(
    doc: &Document,
    expanded: &HashSet<ObjectId>, collapsed_layers: &HashSet<LayerId>,
    query: &str,
    document_open: bool,
    size: crate::prefs::LayerThumbnailSize,
) -> f64 {
    let metric_row_h = || ui_px(size.row_h());
    let n = if document_open { row_count(doc, expanded, collapsed_layers, query) } else { 0 };
    metric_search_h() + n as f64 * metric_row_h() + metric_footer_h()
}

/// The scrollable list area (between the search strip and the footer).
fn list_rect(body: Rect) -> Rect {
    Rect::new(body.x0, body.y0 + metric_search_h(), body.x1, body.y1 - metric_footer_h())
}

/// Scroll offset clamped to what the current row count allows.
fn clamp_scroll(raw: f64, n_rows: usize, list_h: f64, row_h: f64) -> f64 {
    let max = (n_rows as f64 * row_h - list_h).max(0.0);
    raw.clamp(0.0, max)
}

/// Row index of `id` in the (already-expanded) row list, or `None` if it
/// isn't an object row at all — used by "Locate Object" to know where to
/// scroll. Ignores the search filter: locating is expected to win over an
/// active query that would otherwise hide the target row (the caller
/// clears the query for that reason).
fn row_index_of(doc: &Document, expanded: &HashSet<ObjectId>, collapsed_layers: &HashSet<LayerId>, id: ObjectId) -> Option<usize> {
    layer_rows(doc, expanded, collapsed_layers)
        .iter()
        .position(|r| matches!(r.kind, RowKind::Object { id: rid, .. } if rid == id))
}

/// Scroll offset (in the same raw, pre-clamp units `App::panel_scroll`
/// stores) that brings `id`'s row into view, with a little context above
/// it. The caller's later `clamp_scroll` (run every paint/hit regardless
/// of what's stored) bounds this to whatever the panel's actual height
/// turns out to be, so this doesn't need to know it.
pub(crate) fn locate_scroll_target(doc: &Document, expanded: &HashSet<ObjectId>, collapsed_layers: &HashSet<LayerId>, id: ObjectId, size: crate::prefs::LayerThumbnailSize) -> Option<f64> {
    let metric_row_h = || ui_px(size.row_h());
    let idx = row_index_of(doc, expanded, collapsed_layers, id)?;
    const LEAD_ROWS: f64 = 4.0;
    Some((idx as f64 - LEAD_ROWS).max(0.0) * metric_row_h())
}

/// The layer that ultimately contains `id` (walking out through groups).
pub(crate) fn owning_layer(doc: &Document, mut id: ObjectId) -> Option<LayerId> {
    loop {
        match doc.object(id)?.parent {
            ObjectParent::Layer(l) => return Some(l),
            ObjectParent::Group(g) => id = g,
            ObjectParent::Symbol(_) => return None,
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
    expanded: &HashSet<ObjectId>, collapsed_layers: &HashSet<LayerId>,
    ids: &[ObjectId],
) -> Vec<ObjectId> {
    let set: HashSet<ObjectId> = ids.iter().copied().collect();
    let mut out: Vec<ObjectId> = layer_rows(doc, expanded, collapsed_layers)
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
            ObjectParent::Layer(_) | ObjectParent::Symbol(_) => return false,
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
    expanded: &HashSet<ObjectId>, collapsed_layers: &HashSet<LayerId>,
    query: &str,
    scroll_raw: f64,
    moved: &[ObjectId],
    size: crate::prefs::LayerThumbnailSize,
) -> Option<LayerDrop> {
    let metric_row_h = || ui_px(size.row_h());
    let list = list_rect(body);
    if pointer.x < list.x0 || pointer.x > list.x1 || pointer.y < list.y0 || pointer.y > list.y1 {
        return None;
    }
    let rows = rows_filtered(doc, expanded, collapsed_layers, query);
    if rows.is_empty() {
        return None;
    }
    let scroll = clamp_scroll(scroll_raw, rows.len(), list.height(), metric_row_h());
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
        Some(ObjectKind::Unknown { .. }) | None => "?",
    }
    .to_string()
}

/// Stack navigation and thumbnail preferences in the panel flyout.
pub(super) fn menu() -> Vec<MenuEntry> {
    vec![
        MenuEntry::Item { id: "layers-expand-all", label: "Expand All Layers", checked: false },
        MenuEntry::Item { id: "layers-collapse-all", label: "Collapse All Layers", checked: false },
        MenuEntry::Item { id: "layers-panel-options", label: "Panel Options…", checked: false },
    ]
}

pub(super) fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let metric_row_h = || ui_px(ctx.layer_thumbnail_size.row_h());
    draw_search(scene, text, ctx, body);
    let list = list_rect(body);
    let rows = visible_rows(ctx);
    let scroll = clamp_scroll(ctx.layer_scroll, rows.len(), list.height(), metric_row_h());
    let first = (scroll / metric_row_h()).floor() as usize;
    let last = (first + (list.height() / metric_row_h()).ceil() as usize + 2).min(rows.len());
    let content_h = rows.len() as f64 * metric_row_h();

    let hot_row = if list.contains(ctx.pointer) {
        Some(((ctx.pointer.y - list.y0 + scroll) / metric_row_h()).floor() as i64)
    } else {
        None
    };

    scene.push_clip_layer(Fill::NonZero, ID, &list);
    // Only a real, open-but-empty document gets an explanatory message —
    // no document at all just sits blank, like every other panel does in
    // that state, rather than calling it out with its own text.
    if rows.is_empty() && ctx.document_open {
        paint_empty(scene, text, ctx, list);
    }
    for i in first..last {
        let row = &rows[i];
        let ry = list.y0 + i as f64 * metric_row_h() - scroll;
        let r = Rect::new(list.x0, ry, list.x1, ry + metric_row_h());
        let indent = layer_disclosure_rect(r).x0 + row.depth as f64 * metric_indent();
        let hot = hot_row == Some(i as i64);
        let (eye_r, lock_r) = trailing_eye_lock(r);
        let cy = r.center().y;

        match row.kind {
            RowKind::Layer(lid) => {
                let has_obj_sel = !ctx.selection.is_empty();
                let owns = has_obj_sel
                    && ctx
                        .selection
                        .iter()
                        .any(|o| owning_layer(ctx.doc, *o) == Some(lid));
                let selected = !has_obj_sel && ctx.selected_layer == Some(lid);
                if selected {
                    scene.fill(Fill::NonZero, ID, ctx.theme.accent.with_alpha(0.22), None, &r);
                } else if owns {
                    scene.fill(Fill::NonZero, ID, ctx.theme.accent.with_alpha(0.10), None, &r);
                } else if hot {
                    scene.fill(Fill::NonZero, ID, ctx.theme.text.with_alpha(0.05), None, &r);
                }
                let swatch = crate::convert::color(row.color.rgb());
                let rail_a = if row.visible { 1.0 } else { 0.35 };
                scene.fill(
                    Fill::NonZero,
                    ID,
                    swatch.with_alpha(rail_a),
                    None,
                    &layer_rail_rect(r),
                );
                draw_triangle(scene, layer_disclosure_rect(r).center().x, cy, row.expanded, ctx.theme.text_dim);
                if ctx.layer_thumbnail_size == crate::prefs::LayerThumbnailSize::None {
                    draw_layer_kind(scene, layer_name_x(r) - ui_px(8.0), cy, row.layer_kind, ctx.theme.text_dim);
                }
                let editing = match ctx.renaming {
                    Some((RenameId::Layer(l), buf)) if l == lid => Some(buf),
                    _ => None,
                };
                let name_x = paint_thumbnail(scene, text, ctx, row, r, layer_name_x(r));
                let name_c = if !row.visible {
                    ctx.theme.text_dim
                } else {
                    ctx.theme.text
                };
                let name_right = target_rect(r).x0 - ui_px(4.0);
                if editing.is_some() {
                    let clip = Rect::new(r.x0, r.y0, name_right, r.y1);
                    scene.push_clip_layer(Fill::NonZero, ID, &clip);
                    draw_name_field(scene, text, ctx.theme, name_x, r, &row.label, name_c, editing);
                    scene.pop_layer();
                } else {
                    let baseline = r.y0 + metric_row_h() * 0.5 + ui_px(4.0);
                    scene.push_clip_layer(Fill::NonZero, ID, &Rect::new(name_x, r.y0, name_right.max(name_x), r.y1));
                    text.draw(scene, &row.label, 12.0, name_c, name_x, baseline);
                    if owns {
                        text.draw(scene, &row.label, 12.0, name_c, name_x + 0.6, baseline);
                    }
                    scene.pop_layer();
                }
                let target = target_rect(r).center();
                scene.stroke(&Stroke::new(ui_px(1.0)), ID, ctx.theme.text_dim, None, &vello::kurbo::Circle::new(target, ui_px(3.5)));
                if selected || owns {
                    scene.fill(Fill::NonZero, ID, swatch, None, &vello::kurbo::Circle::new(target, ui_px(2.0)));
                }
                let eye_c = if row.visible { ctx.theme.text_dim } else { ctx.theme.border };
                draw_eye(scene, eye_r.center().x, cy, row.visible, eye_c);
                if row.locked {
                    draw_lock(scene, lock_r.center().x, cy, ctx.theme.text);
                } else if hot {
                    draw_lock(scene, lock_r.center().x, cy, ctx.theme.border);
                }
            }
            RowKind::Object { id, is_group } => {
                let selected = ctx.selection.contains(&id);
                if selected {
                    scene.fill(Fill::NonZero, ID, ctx.theme.accent.with_alpha(0.22), None, &r);
                } else if hot {
                    scene.fill(Fill::NonZero, ID, ctx.theme.text.with_alpha(0.05), None, &r);
                }
                for d in 1..=row.depth {
                    let x = layer_disclosure_rect(r).x0 + (d as f64 - 0.5) * metric_indent();
                    scene.stroke(
                        &Stroke::new(ui_px(1.0)),
                        ID,
                        ctx.theme.border.with_alpha(0.35),
                        None,
                        &vello::kurbo::Line::new((x, r.y0), (x, r.y1)),
                    );
                }
                if is_group {
                    draw_triangle(scene, indent + metric_col() * 0.5, cy, row.expanded, ctx.theme.text_dim);
                }
                let icon_x = indent + if is_group { metric_col() * 1.5 } else { metric_col() * 0.5 };
                draw_object_kind(scene, icon_x, cy, ctx.doc.object(id).map(|o| &o.kind), ctx.theme.text_dim);
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
                let name_x = paint_thumbnail(scene, text, ctx, row, r, icon_x + metric_col() * 0.7);
                scene.push_clip_layer(Fill::NonZero, ID, &Rect::new(r.x0, r.y0, target_rect(r).x0 - ui_px(4.0), r.y1));
                draw_name_field(scene, text, ctx.theme, name_x, r, &row.label, name_c, editing);
                scene.pop_layer();
                let eye_c = if row.visible { ctx.theme.text_dim } else { ctx.theme.border };
                draw_eye(scene, eye_r.center().x, cy, row.visible, eye_c);
                if row.locked {
                    draw_lock(scene, lock_r.center().x, cy, ctx.theme.text);
                } else if hot {
                    draw_lock(scene, lock_r.center().x, cy, ctx.theme.border);
                }
            }
        }
        for x in [eye_r.x1, lock_r.x1, target_rect(r).x0] {
            scene.stroke(&Stroke::new(ui_px(0.5)), ID, ctx.theme.border.with_alpha(0.4), None, &vello::kurbo::Line::new((x, r.y0), (x, r.y1)));
        }
        scene.stroke(&Stroke::new(ui_px(0.5)), ID, ctx.theme.border.with_alpha(0.5), None, &vello::kurbo::Line::new((r.x0, r.y1), (r.x1, r.y1)));
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

    let can_edit = ctx.document_open;
    let has_sel = can_edit && !ctx.selection.is_empty();
    paint_panel_footer(
        scene,
        body,
        ctx.theme,
        ctx.pointer,
        [has_sel, has_sel, can_edit, has_sel],
    );
    let locate_r = locate_button_rect(body);
    let locate_c = footer_color(ctx.theme, has_sel, locate_r.contains(ctx.pointer));
    draw_footer_locate(scene, locate_r, locate_c);
    if ctx.layers_new_menu && can_edit {
        paint_new_layer_menu(scene, text, ctx, body);
    }
}

/// The "+" button's own popup — Vector or Raster, matching the
/// Appearance panel's fx/blend footer-menu convention (modal while open,
/// entries mapped by position, closes on any click outside them).
fn new_layer_menu_entries() -> [(&'static str, amalith_core::LayerKind); 2] {
    [("Vector Layer", amalith_core::LayerKind::Vector), ("Raster Layer", amalith_core::LayerKind::Raster)]
}

const NEW_LAYER_MENU_ITEM_H: f64 = 30.0;
const NEW_LAYER_MENU_W: f64 = 168.0;

/// Opens upward from the "+" button — same clamped-to-`body` reasoning as
/// `appearance::fx_menu_rect` (its own doc comment explains why: a short
/// flyout preview can otherwise put the naive top edge above the body's
/// own clip region, making the menu exist but never actually be visible).
fn new_layer_menu_rect(body: Rect) -> Rect {
    let [_, _, add, _] = panel_footer_rects(body);
    let entries = new_layer_menu_entries();
    let h = ui_px(NEW_LAYER_MENU_ITEM_H) * entries.len() as f64;
    let y1 = (add.y0 - ui_px(6.0)).max(body.y0 + ui_px(4.0));
    let y0 = (y1 - h).max(body.y0 + ui_px(4.0));
    let x1 = add.x1 + metric_pad();
    Rect::new((x1 - ui_px(NEW_LAYER_MENU_W)).max(body.x0 + ui_px(4.0)), y0, x1, y1)
}

fn new_layer_menu_entry_rect(body: Rect, i: usize) -> Rect {
    let menu = new_layer_menu_rect(body);
    let y = menu.y0 + i as f64 * ui_px(NEW_LAYER_MENU_ITEM_H);
    Rect::new(menu.x0, y, menu.x1, y + ui_px(NEW_LAYER_MENU_ITEM_H))
}

fn paint_new_layer_menu(scene: &mut Scene, text: &mut TextContext, ctx: &Ctx, body: Rect) {
    let th = ctx.theme;
    let menu = new_layer_menu_rect(body);
    let round = menu.to_rounded_rect(ui_px(5.0));
    scene.fill(Fill::NonZero, ID, th.panel_bg, None, &round);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &round);
    for (i, &(label, kind)) in new_layer_menu_entries().iter().enumerate() {
        let r = new_layer_menu_entry_rect(body, i);
        if r.contains(ctx.pointer) {
            scene.fill(Fill::NonZero, ID, th.accent.with_alpha(0.16), None, &r);
        }
        let kind_c = match kind {
            LayerKind::Vector => th.accent,
            LayerKind::Raster => th.raster_accent,
        };
        draw_layer_kind(scene, r.x0 + ui_px(16.0), r.center().y, kind, kind_c);
        text.draw(scene, label, 12.0, th.text, r.x0 + ui_px(30.0), r.center().y + ui_px(4.0));
    }
}

/// Resolve a click: the footer buttons, a disclosure triangle, the eye /
/// lock columns, or the name (select).
pub(super) fn hit(body: Rect, local: Point, ctx: &Ctx) -> Action {
    let metric_row_h = || ui_px(ctx.layer_thumbnail_size.row_h());
    // Modal while open — same "first click away just dismisses"
    // convention as the Appearance panel's fx/blend menus. Checked before
    // the footer-vs-list split below: the menu opens *upward* from the
    // "+" button, so its entries actually sit in the list's y-range, not
    // the footer strip's.
    if ctx.layers_new_menu {
        if ctx.document_open {
            for (i, &(_, kind)) in new_layer_menu_entries().iter().enumerate() {
                if new_layer_menu_entry_rect(body, i).contains(local) {
                    return Action::NewLayerOfKind(kind);
                }
            }
        }
        return Action::ToggleNewLayerMenu;
    }
    if local.y >= body.y1 - metric_footer_h() {
        if !ctx.document_open {
            return Action::None;
        }
        if locate_button_rect(body).contains(local) {
            return if ctx.selection.is_empty() { Action::None } else { Action::LocateSelection };
        }
        let [up, down, add, del] = panel_footer_rects(body);
        return if up.contains(local) {
            Action::LayerRestack(1)
        } else if down.contains(local) {
            Action::LayerRestack(-1)
        } else if add.contains(local) {
            Action::ToggleNewLayerMenu
        } else if del.contains(local) {
            Action::DeleteObjects
        } else {
            Action::None
        };
    }
    // The search field owns the top strip.
    if local.y < body.y0 + metric_search_h() {
        return if ctx.document_open { Action::FocusLayerSearch } else { Action::None };
    }
    let list = list_rect(body);
    let rows = visible_rows(ctx);
    let scroll = clamp_scroll(ctx.layer_scroll, rows.len(), list.height(), metric_row_h());
    let i = ((local.y - (body.y0 + metric_search_h()) + scroll) / metric_row_h()).floor();
    if i < 0.0 {
        return Action::None;
    }
    let Some(row) = rows.get(i as usize) else {
        return Action::None;
    };
    let ry = list.y0 + i * metric_row_h() - scroll;
    let r = Rect::new(body.x0, ry, body.x1, ry + metric_row_h());
    let (eye_r, lock_r) = trailing_eye_lock(r);
    match row.kind {
        // The color swatch is its own precise square — a single click
        // still selects the layer like the rest of the row; a double
        // click (resolved by the caller) opens Layer Options instead of
        // renaming.
        RowKind::Layer(id) => {
            if eye_r.contains(local) {
                Action::ToggleLayerVisible(id)
            } else if lock_r.contains(local) {
                Action::ToggleLayerLocked(id)
            } else if layer_disclosure_rect(r).contains(local) {
                Action::ToggleLayerExpand(id)
            } else if layer_rail_rect(r).contains(local) {
                Action::LayerSwatch(id)
            } else {
                Action::SelectLayer(id)
            }
        }
        RowKind::Object { id, is_group } => {
            let indent = layer_disclosure_rect(r).x0 - body.x0 + row.depth as f64 * metric_indent();
            let x = local.x - body.x0;
            if eye_r.contains(local) {
                Action::ToggleVisible(id)
            } else if lock_r.contains(local) {
                Action::ToggleLocked(id)
            } else if is_group && x < indent + metric_col() {
                Action::ToggleExpand(id)
            } else {
                Action::Select(id)
            }
        }
    }
}

/// "Locate Object" button — bottom-left of the footer strip, apart from
/// the shared right-aligned New/Delete/restack cluster (`panel_footer_rects`,
/// also used by the Artboards panel), matching where Illustrator puts its
/// own Layers-panel magnifying-glass button.
fn locate_button_rect(body: Rect) -> Rect {
    let sz = ui_px(20.0);
    let cy = body.y1 - metric_footer_h() * 0.5;
    let cx = body.x0 + metric_pad() + sz * 0.5;
    Rect::from_center_size(Point::new(cx, cy), (sz, sz))
}

/// A magnifying glass, matching `draw_search`'s ring-plus-handle shape but
/// centred in a footer button rect.
fn draw_footer_locate(scene: &mut Scene, r: Rect, color: Color) {
    let c = r.center();
    let ring = vello::kurbo::Circle::new((c.x - ui_px(1.0), c.y - ui_px(1.0)), ui_px(4.0));
    scene.stroke(&Stroke::new(ui_px(1.4)), ID, color, None, &ring);
    let mut handle = BezPath::new();
    handle.move_to((c.x + ui_px(1.7), c.y + ui_px(1.7)));
    handle.line_to((c.x + ui_px(5.5), c.y + ui_px(5.5)));
    scene.stroke(&Stroke::new(ui_px(1.6)), ID, color, None, &handle);
}

/// A disclosure triangle centred at `(cx, cy)`: pointing right when
/// collapsed, down when expanded.
fn draw_triangle(scene: &mut Scene, cx: f64, cy: f64, expanded: bool, color: Color) {
    let mut p = BezPath::new();
    if expanded {
        p.move_to((cx - ui_px(3.5), cy - ui_px(2.5)));
        p.line_to((cx, cy + ui_px(3.5)));
        p.line_to((cx + ui_px(3.5), cy - ui_px(2.5)));
    } else {
        p.move_to((cx - ui_px(2.5), cy - ui_px(3.5)));
        p.line_to((cx + ui_px(3.5), cy));
        p.line_to((cx - ui_px(2.5), cy + ui_px(3.5)));
    }
    scene.stroke(&Stroke::new(ui_px(1.3)), ID, color, None, &p);
}

/// Color rail follows the visibility and lock columns.
fn layer_rail_rect(row: Rect) -> Rect {
    let x = row.x0 + ui_px(42.0);
    Rect::new(x, row.y0 + ui_px(2.0), x + ui_px(3.0), row.y1 - ui_px(2.0))
}

fn layer_disclosure_rect(row: Rect) -> Rect {
    Rect::new(row.x0 + ui_px(46.0), row.y0, row.x0 + ui_px(64.0), row.y1)
}

fn target_rect(row: Rect) -> Rect {
    Rect::new(row.x1 - ui_px(32.0), row.y0, row.x1, row.y1)
}

/// Where a layer row's name starts — clear of the rail, swatch, and kind.
fn layer_name_x(row: Rect) -> f64 {
    row.x0 + ui_px(70.0)
}

/// Full-height visibility and lock hit areas at the left of every row.
fn trailing_eye_lock(row: Rect) -> (Rect, Rect) {
    let eye = Rect::new(row.x0, row.y0, row.x0 + ui_px(20.0), row.y1);
    let lock = Rect::new(eye.x1, row.y0, row.x0 + ui_px(40.0), row.y1);
    (eye, lock)
}

/// Preview actual artwork, using the same renderer as the canvas. The
/// document framing option keeps relative position and scale across rows.
fn paint_thumbnail(scene: &mut Scene, text: &mut TextContext, ctx: &Ctx, row: &LayerRow, rect: Rect, x: f64) -> f64 {
    let size = ui_px(ctx.layer_thumbnail_size.image_px());
    if size == 0.0 { return x; }
    let tile = Rect::from_origin_size((x, rect.center().y - size * 0.5), (size, size));
    scene.push_clip_layer(Fill::NonZero, ID, &tile);
    let cell = ui_px(4.0);
    for y in 0..(size / cell).ceil() as usize {
        for col in 0..(size / cell).ceil() as usize {
            let color = if (y + col) % 2 == 0 { Color::from_rgb8(220, 220, 220) } else { Color::from_rgb8(170, 170, 170) };
            scene.fill(Fill::NonZero, ID, color, None, &Rect::from_origin_size((x + col as f64 * cell, tile.y0 + y as f64 * cell), (cell, cell)));
        }
    }
    let ids = match row.kind {
        RowKind::Layer(id) => ctx.doc.children_of(ObjectParent::Layer(id)).to_vec(),
        RowKind::Object { id, .. } => vec![id],
    };
    let bounds = if ctx.layer_thumbnail_contents == crate::prefs::LayerThumbnailContents::EntireDocument {
        ctx.doc.artboards().iter().map(|a| a.rect).reduce(|a, b| a.union(b))
            .or_else(|| ctx.doc.layers().iter().flat_map(|l| l.children.iter()).filter_map(|id| ctx.doc.bounds_of(*id)).reduce(|a, b| a.union(b)))
    } else {
        ids.iter().filter_map(|id| ctx.doc.bounds_of(*id)).reduce(|a, b| a.union(b))
    };
    if let Some(bounds) = bounds {
        let src = crate::convert::rect(bounds).inflate(0.5, 0.5);
        let scale = (size - ui_px(4.0)) / src.width().max(src.height()).max(1.0);
        let preview = crate::canvas::export_scene_of(ctx.doc, &ids, src, scale, None, ctx.layer_images, false, text, ctx.theme.text_dim);
        let origin = (tile.center().x - src.width() * scale * 0.5, tile.center().y - src.height() * scale * 0.5);
        scene.append(&preview, Some(vello::kurbo::Affine::translate(origin)));
    }
    scene.pop_layer();
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, ctx.theme.border, None, &tile);
    x + size + ui_px(8.0)
}

/// Only ever called for an open document with zero layers — a real, if
/// rare, state (e.g. every layer was deleted) worth explaining; a closed
/// document is handled by the caller staying blank instead.
fn paint_empty(scene: &mut Scene, text: &mut TextContext, ctx: &Ctx, list: Rect) {
    let (title, sub) = ("No layers", "Use + to add a vector or raster layer");
    let _ = ctx;
    let cx = list.center().x;
    let cy = list.center().y;
    let title_x = cx - text.measure(title, 13.0) * 0.5;
    let sub_x = cx - text.measure(sub, 11.0) * 0.5;
    text.draw(scene, title, 13.0, ctx.theme.text_dim, title_x.max(list.x0 + metric_pad()), cy - ui_px(4.0));
    text.draw(scene, sub, 11.0, ctx.theme.border, sub_x.max(list.x0 + metric_pad()), cy + ui_px(14.0));
}

fn draw_layer_kind(scene: &mut Scene, cx: f64, cy: f64, kind: LayerKind, color: Color) {
    match kind {
        LayerKind::Vector => {
            let mut p = BezPath::new();
            p.move_to((cx - ui_px(4.5), cy + ui_px(3.5)));
            p.quad_to((cx - ui_px(1.0), cy - ui_px(5.5)), (cx + ui_px(4.5), cy - ui_px(1.5)));
            scene.stroke(&Stroke::new(ui_px(1.4)), ID, color, None, &p);
            scene.fill(
                Fill::NonZero,
                ID,
                color,
                None,
                &vello::kurbo::Circle::new((cx + ui_px(4.5), cy - ui_px(1.5)), ui_px(1.4)),
            );
        }
        LayerKind::Raster => {
            let s = ui_px(3.2);
            let g = ui_px(1.1);
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let x = cx + dx * (s + g) * 0.5 - s * 0.5;
                let y = cy + dy * (s + g) * 0.5 - s * 0.5;
                scene.fill(Fill::NonZero, ID, color, None, &Rect::new(x, y, x + s, y + s));
            }
        }
    }
}

fn draw_object_kind(scene: &mut Scene, cx: f64, cy: f64, kind: Option<&ObjectKind>, color: Color) {
    match kind {
        Some(ObjectKind::Group(g)) if g.clip.is_some() => {
            scene.stroke(
                &Stroke::new(ui_px(1.2)),
                ID,
                color,
                None,
                &Rect::new(cx - ui_px(4.5), cy - ui_px(3.5), cx + ui_px(4.5), cy + ui_px(3.5)),
            );
            scene.stroke(
                &Stroke::new(ui_px(1.0)),
                ID,
                color,
                None,
                &vello::kurbo::Circle::new((cx, cy), ui_px(2.0)),
            );
        }
        Some(ObjectKind::Group(_)) => {
            let mut p = BezPath::new();
            p.move_to((cx - ui_px(5.0), cy + ui_px(3.5)));
            p.line_to((cx - ui_px(5.0), cy - ui_px(1.5)));
            p.line_to((cx - ui_px(1.5), cy - ui_px(1.5)));
            p.line_to((cx - ui_px(0.2), cy - ui_px(3.8)));
            p.line_to((cx + ui_px(5.0), cy - ui_px(3.8)));
            p.line_to((cx + ui_px(5.0), cy + ui_px(3.5)));
            p.close_path();
            scene.stroke(&Stroke::new(ui_px(1.2)), ID, color, None, &p);
        }
        Some(ObjectKind::Text(_)) => {
            let mut p = BezPath::new();
            p.move_to((cx - ui_px(4.0), cy - ui_px(3.5)));
            p.line_to((cx + ui_px(4.0), cy - ui_px(3.5)));
            scene.stroke(&Stroke::new(ui_px(1.4)), ID, color, None, &p);
            scene.stroke(
                &Stroke::new(ui_px(1.4)),
                ID,
                color,
                None,
                &vello::kurbo::Line::new((cx, cy - ui_px(3.5)), (cx, cy + ui_px(4.0))),
            );
        }
        Some(ObjectKind::Image(_)) => {
            scene.stroke(
                &Stroke::new(ui_px(1.2)),
                ID,
                color,
                None,
                &Rect::new(cx - ui_px(5.0), cy - ui_px(3.5), cx + ui_px(5.0), cy + ui_px(3.5)).to_rounded_rect(ui_px(1.2)),
            );
            let mut p = BezPath::new();
            p.move_to((cx - ui_px(4.0), cy + ui_px(2.5)));
            p.line_to((cx - ui_px(1.0), cy - ui_px(1.0)));
            p.line_to((cx + ui_px(1.5), cy + ui_px(1.0)));
            p.line_to((cx + ui_px(4.0), cy - ui_px(0.5)));
            scene.stroke(&Stroke::new(ui_px(1.1)), ID, color, None, &p);
        }
        Some(ObjectKind::Symbol(_)) => {
            let mut p = BezPath::new();
            p.move_to((cx, cy - ui_px(4.5)));
            p.line_to((cx + ui_px(1.4), cy - ui_px(1.2)));
            p.line_to((cx + ui_px(4.8), cy - ui_px(1.2)));
            p.line_to((cx + ui_px(2.0), cy + ui_px(0.8)));
            p.line_to((cx + ui_px(3.0), cy + ui_px(4.2)));
            p.line_to((cx, cy + ui_px(2.2)));
            p.line_to((cx - ui_px(3.0), cy + ui_px(4.2)));
            p.line_to((cx - ui_px(2.0), cy + ui_px(0.8)));
            p.line_to((cx - ui_px(4.8), cy - ui_px(1.2)));
            p.line_to((cx - ui_px(1.4), cy - ui_px(1.2)));
            p.close_path();
            scene.stroke(&Stroke::new(ui_px(1.1)), ID, color, None, &p);
        }
        Some(ObjectKind::CompoundPath(_)) => {
            scene.stroke(
                &Stroke::new(ui_px(1.2)),
                ID,
                color,
                None,
                &vello::kurbo::Circle::new((cx - ui_px(1.5), cy), ui_px(3.2)),
            );
            scene.stroke(
                &Stroke::new(ui_px(1.2)),
                ID,
                color,
                None,
                &vello::kurbo::Circle::new((cx + ui_px(1.5), cy), ui_px(3.2)),
            );
        }
        _ => {
            scene.stroke(
                &Stroke::new(ui_px(1.2)),
                ID,
                color,
                None,
                &Rect::new(cx - ui_px(4.5), cy - ui_px(3.2), cx + ui_px(4.5), cy + ui_px(3.2)).to_rounded_rect(ui_px(1.4)),
            );
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapsing_layer_hides_descendants_and_restores_expanded_groups() {
        let mut doc = Document::new("Layers");
        let layer = LayerId::new();
        doc.insert_layer(amalith_core::Layer::new(layer, "Artwork"), 0);
        let group = ObjectId::new();
        doc.insert_object(amalith_core::Object::new(group, ObjectParent::Layer(layer), ObjectKind::Group(Default::default())), 0).unwrap();
        let child = ObjectId::new();
        doc.insert_object(amalith_core::Object::rectangle(child, ObjectParent::Group(group), amalith_core::Rect::new(0.0, 0.0, 10.0, 10.0)), 0).unwrap();
        let expanded = HashSet::from([group]);
        let mut collapsed = HashSet::new();
        assert_eq!(layer_rows(&doc, &expanded, &collapsed).len(), 3);
        collapsed.insert(layer);
        let rows = layer_rows(&doc, &expanded, &collapsed);
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].expanded);
        assert!(row_index_of(&doc, &expanded, &collapsed, child).is_none());
        let size = crate::prefs::LayerThumbnailSize::Small;
        assert_eq!(content_height(&doc, &expanded, &collapsed, "", true, size), metric_search_h() + ui_px(size.row_h()) + metric_footer_h());
        collapsed.remove(&layer);
        assert_eq!(layer_rows(&doc, &expanded, &collapsed).len(), 3);
        assert_eq!(row_index_of(&doc, &expanded, &collapsed, child), Some(2));
        assert!(doc.object(child).unwrap().visible);
    }

    #[test]
    fn thumbnail_sizes_keep_scroll_and_drop_geometry_aligned() {
        let mut doc = Document::new("Layers");
        let layer = LayerId::new();
        doc.insert_layer(amalith_core::Layer::new(layer, "Artwork"), 0);
        let expanded = HashSet::new();
        let body = Rect::new(0.0, 0.0, 320.0, 400.0);
        for size in [crate::prefs::LayerThumbnailSize::None, crate::prefs::LayerThumbnailSize::Small, crate::prefs::LayerThumbnailSize::Medium, crate::prefs::LayerThumbnailSize::Large] {
            let row_h = ui_px(size.row_h());
            assert_eq!(content_height(&doc, &expanded, &HashSet::new(), "", true, size), metric_search_h() + row_h + metric_footer_h());
            let pointer = Point::new(100.0, metric_search_h() + row_h * 0.5);
            let target = drop_target(body, pointer, &doc, &expanded, &HashSet::new(), "", 0.0, &[], size).unwrap();
            assert_eq!(target.parent, ObjectParent::Layer(layer));
            assert!(target.into);
            assert_eq!(clamp_scroll(1000.0, 10, row_h * 3.0, row_h), row_h * 7.0);
        }
    }
}
