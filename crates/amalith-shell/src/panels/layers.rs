//! Layers panel: blend/opacity/lock header, search + kind funnel, the
//! layer / object tree, and a Photoshop-style footer button strip.

use crate::metrics::px as ui_px;

use std::collections::HashSet;

use amalith_core::{BlendMode, Document, LayerId, LayerKind, ObjectId, ObjectKind, ObjectParent};
use vello::kurbo::{BezPath, Circle, Line, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

use super::{
    draw_eye, draw_footer_plus, draw_footer_trash, draw_name_field, footer_color, Action, Ctx, MenuEntry, RenameId,
    metric_footer_h, ID, metric_pad,
};

/// Per-depth indent, and the width of each icon column (triangle, kind)
/// in a Layers row.
fn metric_indent() -> f64 { crate::metrics::with(|m| m.panels_layers_indent) }
fn metric_col() -> f64 { crate::metrics::with(|m| m.panels_layers_col) }

/// Height reserved at the top of the panel body for the search field.
pub(super) fn metric_search_h() -> f64 { crate::metrics::with(|m| m.panels_layers_search_h) }

/// Blend / opacity / lock toolbar sitting above the search row.
pub(super) fn metric_toolbar_h() -> f64 { crate::metrics::with(|m| m.panels_layers_toolbar_h) }

fn chrome_h() -> f64 { metric_toolbar_h() + metric_search_h() }

/// The layer / object rows to show, after the search filter. A blank
/// query shows the whole tree; otherwise only rows whose name contains
/// the query (case-insensitive), flattened. Empty while the focused pane
/// has no document (`ctx.document_open`) — `ctx.doc` is then leftover
/// boot/last-document state, not something to present.
fn visible_rows(ctx: &Ctx) -> Vec<LayerRow> {
    if !ctx.document_open {
        return Vec::new();
    }
    rows_filtered(ctx.doc, ctx.expanded, ctx.collapsed_layers, ctx.layer_query, ctx.layer_kind_filter)
}

fn rows_filtered(
    doc: &Document,
    expanded: &HashSet<ObjectId>, collapsed_layers: &HashSet<LayerId>,
    query: &str,
    kind_filter: Option<LayerKind>,
) -> Vec<LayerRow> {
    let rows = layer_rows(doc, expanded, collapsed_layers);
    let rows = match kind_filter {
        None => rows,
        Some(kind) => {
            let mut keep = true;
            rows.into_iter()
                .filter(|r| match r.kind {
                    RowKind::Layer(_) => {
                        keep = r.layer_kind == kind;
                        keep
                    }
                    RowKind::Object { .. } => keep,
                })
                .collect()
        }
    };
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|r| r.label.to_lowercase().contains(&q))
        .collect()
}

fn toolbar_rect(body: Rect) -> Rect {
    Rect::new(body.x0, body.y0, body.x1, body.y0 + metric_toolbar_h())
}

fn search_row_rect(body: Rect) -> Rect {
    Rect::new(body.x0, body.y0 + metric_toolbar_h(), body.x1, body.y0 + chrome_h())
}

/// The search field box, inset from the search row, leaving room for the
/// kind-funnel button on the right.
fn search_box(body: Rect) -> Rect {
    let row = search_row_rect(body);
    let filter = filter_button_rect(body);
    Rect::new(
        row.x0 + metric_pad(),
        row.y0 + ui_px(6.0),
        filter.x0 - ui_px(6.0),
        row.y1 - ui_px(6.0),
    )
}

fn filter_button_rect(body: Rect) -> Rect {
    let row = search_row_rect(body);
    let sz = ui_px(20.0);
    Rect::from_center_size(Point::new(row.x1 - metric_pad() - sz * 0.5, row.center().y), (sz, sz))
}

struct Toolbar {
    blend: Rect,
    opacity: Rect,
    lock_transp: Rect,
    lock_paint: Rect,
    lock_pos: Rect,
    lock_all: Rect,
}

fn toolbar_layout(body: Rect) -> Toolbar {
    let bar = toolbar_rect(body);
    let pad = metric_pad();
    let y0 = bar.y0 + ui_px(5.0);
    let y1 = bar.y1 - ui_px(5.0);
    let h = (y1 - y0).max(ui_px(18.0));
    let lock_gap = ui_px(2.0);
    let lock_cluster = 4.0 * h + 3.0 * lock_gap;
    let lock_x1 = bar.x1 - pad;
    let lock_x0 = lock_x1 - lock_cluster;
    let lock_at = |i: usize| {
        let x = lock_x0 + i as f64 * (h + lock_gap);
        Rect::new(x, y0, x + h, y1)
    };
    let op_w = ui_px(52.0);
    let gap = ui_px(8.0);
    let op_x1 = (lock_x0 - gap).max(bar.x0 + pad + ui_px(120.0));
    let op_x0 = op_x1 - op_w;
    let blend_x1 = (op_x0 - gap).max(bar.x0 + pad + ui_px(72.0));
    Toolbar {
        blend: Rect::new(bar.x0 + pad, y0, blend_x1, y1),
        opacity: Rect::new(op_x0, y0, op_x1, y1),
        lock_transp: lock_at(0),
        lock_paint: lock_at(1),
        lock_pos: lock_at(2),
        lock_all: lock_at(3),
    }
}

/// Whether `p` sits on the header opacity field — used by the shell to
/// keep a live opacity edit focused while the pointer stays on it.
pub(super) fn opacity_field_at(body: Rect, p: Point) -> bool {
    toolbar_layout(body).opacity.contains(p)
}

fn draw_chrome(scene: &mut Scene, text: &mut TextContext, ctx: &Ctx, body: Rect) {
    let th = ctx.theme;
    let chrome = Rect::new(body.x0, body.y0, body.x1, body.y0 + chrome_h());
    scene.fill(Fill::NonZero, ID, th.strip_bg, None, &chrome);
    let bar = toolbar_rect(body);
    scene.fill(Fill::NonZero, ID, th.border.with_alpha(0.55), None, &Rect::new(bar.x0, bar.y1 - 0.5, bar.x1, bar.y1));
    scene.fill(Fill::NonZero, ID, th.border.with_alpha(0.7), None, &Rect::new(chrome.x0, chrome.y1 - 0.5, chrome.x1, chrome.y1));
    draw_toolbar(scene, text, ctx, body);
    draw_search(scene, text, ctx, body);
}

fn draw_toolbar(scene: &mut Scene, text: &mut TextContext, ctx: &Ctx, body: Rect) {
    let th = ctx.theme;
    let live = ctx.document_open;
    let has_obj = live && !ctx.selection.is_empty();
    let t = toolbar_layout(body);
    let pointer = ctx.pointer;

    // A raster `Image` object has no Fill/Stroke appearance items to hold
    // a blend mode at all — dimmed there, same as a plain vector object
    // with no selection, rather than looking clickable and silently
    // no-op'ing (`App::set_selection_blend_mode` already skips items-less
    // objects; this just keeps the field honest about it).
    let has_blend = has_obj && ctx.representative.as_ref().is_some_and(|a| !a.items.is_empty());
    let blend_label = if !has_blend {
        "Normal"
    } else {
        selection_blend_label(ctx)
    };
    draw_header_field(scene, text, th, t.blend, blend_label, has_blend, t.blend.contains(pointer) && has_blend, true);

    let op_shown = match ctx.opacity_edit {
        Some(buf) if has_obj => format!("{buf}%"),
        _ if has_obj => {
            let op = ctx.representative.as_ref().map(|a| a.opacity).unwrap_or(1.0);
            format!("{:.0}%", op * 100.0)
        }
        _ => "100%".into(),
    };
    draw_header_field(
        scene,
        text,
        th,
        t.opacity,
        &op_shown,
        has_obj,
        t.opacity.contains(pointer) && has_obj,
        true,
    );
    if has_obj && ctx.opacity_edit.is_some() {
        let after = text.measure(&op_shown, 11.0);
        let cx = t.opacity.x0 + ui_px(8.0) + after + 1.0;
        scene.fill(
            Fill::NonZero,
            ID,
            th.text,
            None,
            &Rect::new(cx, t.opacity.y0 + ui_px(4.0), cx + 1.3, t.opacity.y1 - ui_px(4.0)),
        );
    }

    let lock_on = header_lock_engaged(ctx);
    let lock_live = live && (has_obj || ctx.selected_layer.is_some());
    draw_lock_glyph(scene, t.lock_transp, th, false, t.lock_transp.contains(pointer), LockGlyph::Transparency);
    draw_lock_glyph(scene, t.lock_paint, th, false, t.lock_paint.contains(pointer), LockGlyph::Paint);
    draw_lock_glyph(scene, t.lock_pos, th, false, t.lock_pos.contains(pointer), LockGlyph::Position);
    draw_lock_glyph(scene, t.lock_all, th, lock_live && lock_on, t.lock_all.contains(pointer) && lock_live, LockGlyph::All);
}

fn selection_blend_label(ctx: &Ctx) -> &'static str {
    let Some(app) = ctx.representative.as_ref() else { return "Normal" };
    let Some(item) = app.items.last() else { return "Normal" };
    blend_label(item.blend_mode())
}

fn header_lock_engaged(ctx: &Ctx) -> bool {
    if let Some(&id) = ctx.selection.first() {
        return ctx.doc.object(id).is_some_and(|o| o.locked);
    }
    ctx.selected_layer.and_then(|id| ctx.doc.layer(id)).is_some_and(|l| l.locked)
}

fn draw_header_field(
    scene: &mut Scene,
    text: &mut TextContext,
    th: &Theme,
    r: Rect,
    label: &str,
    enabled: bool,
    hot: bool,
    chevron: bool,
) {
    let round = r.to_rounded_rect(ui_px(4.0));
    let fill = if hot { th.bg } else { th.panel_bg };
    scene.fill(Fill::NonZero, ID, fill, None, &round);
    let border = if enabled { th.border } else { th.border.with_alpha(0.45) };
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, border, None, &round);
    let ink = if enabled { th.text } else { th.border };
    let tx = r.x0 + ui_px(8.0);
    let text_right = if chevron { r.x1 - ui_px(16.0) } else { r.x1 - ui_px(4.0) };
    scene.push_clip_layer(Fill::NonZero, ID, &Rect::new(r.x0, r.y0, text_right.max(r.x0 + 1.0), r.y1));
    text.draw(scene, label, 11.0, ink, tx, r.center().y + ui_px(3.5));
    scene.pop_layer();
    if chevron {
        let cx = r.x1 - ui_px(10.0);
        let cy = r.center().y;
        let mut p = BezPath::new();
        p.move_to((cx - ui_px(3.5), cy - ui_px(1.2)));
        p.line_to((cx, cy + ui_px(2.2)));
        p.line_to((cx + ui_px(3.5), cy - ui_px(1.2)));
        scene.stroke(&Stroke::new(ui_px(1.2)), ID, ink.with_alpha(0.8), None, &p);
    }
}

#[derive(Clone, Copy)]
enum LockGlyph {
    Transparency,
    Paint,
    Position,
    All,
}

fn draw_lock_glyph(scene: &mut Scene, r: Rect, th: &Theme, on: bool, hot: bool, kind: LockGlyph) {
    if hot || on {
        let fill = if on { th.accent.with_alpha(0.18) } else { th.text.with_alpha(0.08) };
        scene.fill(Fill::NonZero, ID, fill, None, &r.to_rounded_rect(ui_px(3.0)));
    }
    let enabled = matches!(kind, LockGlyph::All);
    let color = if !enabled {
        th.border
    } else if on {
        th.text
    } else if hot {
        th.text_dim
    } else {
        th.border
    };
    let c = r.center();
    match kind {
        LockGlyph::Transparency => {
            let s = ui_px(3.2);
            for (i, (dx, dy)) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].into_iter().enumerate() {
                if i % 2 == 0 {
                    scene.fill(
                        Fill::NonZero,
                        ID,
                        color,
                        None,
                        &Rect::from_center_size((c.x + dx * s, c.y + dy * s), (s, s)),
                    );
                } else {
                    scene.stroke(
                        &Stroke::new(ui_px(0.9)),
                        ID,
                        color,
                        None,
                        &Rect::from_center_size((c.x + dx * s, c.y + dy * s), (s, s)),
                    );
                }
            }
        }
        LockGlyph::Paint => {
            let mut p = BezPath::new();
            p.move_to((c.x + ui_px(2.5), c.y - ui_px(4.8)));
            p.line_to((c.x + ui_px(4.2), c.y - ui_px(3.1)));
            p.line_to((c.x - ui_px(1.6), c.y + ui_px(2.7)));
            p.line_to((c.x - ui_px(3.3), c.y + ui_px(1.0)));
            p.close_path();
            scene.stroke(&Stroke::new(ui_px(1.15)), ID, color, None, &p);
            scene.stroke(
                &Stroke::new(ui_px(1.15)),
                ID,
                color,
                None,
                &Line::new((c.x - ui_px(3.6), c.y + ui_px(3.4)), (c.x - ui_px(1.2), c.y + ui_px(4.6))),
            );
        }
        LockGlyph::Position => {
            let arm = ui_px(4.6);
            let head = ui_px(2.2);
            scene.stroke(&Stroke::new(ui_px(1.2)), ID, color, None, &Line::new((c.x - arm, c.y), (c.x + arm, c.y)));
            scene.stroke(&Stroke::new(ui_px(1.2)), ID, color, None, &Line::new((c.x, c.y - arm), (c.x, c.y + arm)));
            for (dx, dy, hx, hy) in [
                (arm, 0.0, -head, -head),
                (arm, 0.0, -head, head),
                (-arm, 0.0, head, -head),
                (-arm, 0.0, head, head),
                (0.0, arm, -head, -head),
                (0.0, arm, head, -head),
                (0.0, -arm, -head, head),
                (0.0, -arm, head, head),
            ] {
                scene.stroke(
                    &Stroke::new(ui_px(1.1)),
                    ID,
                    color,
                    None,
                    &Line::new((c.x + dx, c.y + dy), (c.x + dx + hx, c.y + dy + hy)),
                );
            }
        }
        LockGlyph::All => draw_lock(scene, c.x, c.y, color),
    }
}

fn draw_search(scene: &mut Scene, text: &mut TextContext, ctx: &Ctx, body: Rect) {
    let th = ctx.theme;
    let box_ = search_box(body);
    let live = ctx.document_open;
    let round = box_.to_rounded_rect(ui_px(6.0));
    scene.fill(Fill::NonZero, ID, th.bg, None, &round);
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
    let gx = box_.x0 + ui_px(11.0);
    let ring = Circle::new((gx, cy - 0.5), ui_px(4.0));
    scene.stroke(&Stroke::new(ui_px(1.4)), ID, ink, None, &ring);
    let mut handle = BezPath::new();
    handle.move_to((gx + ui_px(3.0), cy + ui_px(2.5)));
    handle.line_to((gx + ui_px(6.5), cy + ui_px(6.0)));
    scene.stroke(&Stroke::new(ui_px(1.4)), ID, ink, None, &handle);

    let tx = box_.x0 + ui_px(22.0);
    let baseline = cy + ui_px(4.0);
    let placeholder = match ctx.layer_kind_filter {
        None => "Search All",
        Some(LayerKind::Vector) => "Search Vector",
        Some(LayerKind::Raster) => "Search Raster",
    };
    let (label, color): (&str, Color) = if !live {
        (placeholder, th.border)
    } else if ctx.layer_query.is_empty() {
        (placeholder, th.text_dim)
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

    let filter = filter_button_rect(body);
    let filter_on = ctx.layer_kind_filter.is_some();
    let filter_hot = live && filter.contains(ctx.pointer);
    if filter_hot || filter_on {
        let fill = if filter_on { th.accent.with_alpha(0.18) } else { th.text.with_alpha(0.08) };
        scene.fill(Fill::NonZero, ID, fill, None, &filter.to_rounded_rect(ui_px(3.0)));
    }
    let fink = if !live {
        th.border
    } else if filter_on {
        th.accent
    } else if filter_hot {
        th.text
    } else {
        th.text_dim
    };
    draw_funnel(scene, filter.center(), fink);
}

fn draw_funnel(scene: &mut Scene, c: Point, color: Color) {
    let mut p = BezPath::new();
    p.move_to((c.x - ui_px(5.5), c.y - ui_px(4.5)));
    p.line_to((c.x + ui_px(5.5), c.y - ui_px(4.5)));
    p.line_to((c.x + ui_px(1.6), c.y + ui_px(1.2)));
    p.line_to((c.x + ui_px(1.6), c.y + ui_px(5.0)));
    p.line_to((c.x - ui_px(1.6), c.y + ui_px(3.4)));
    p.line_to((c.x - ui_px(1.6), c.y + ui_px(1.2)));
    p.close_path();
    scene.stroke(&Stroke::new(ui_px(1.2)), ID, color, None, &p);
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
    /// Object rows only — this image's layer mask, if any.
    mask: Option<amalith_core::ImageMask>,
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
                mask: obj.kind.mask(),
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
            mask: None,
        });
        if !collapsed_layers.contains(&layer.id) {
            walk(doc, ObjectParent::Layer(layer.id), 1, expanded, &mut rows);
        }
    }
    rows
}

/// Number of rows the list would show for `doc` under the current filter.
fn row_count(
    doc: &Document,
    expanded: &HashSet<ObjectId>, collapsed_layers: &HashSet<LayerId>,
    query: &str,
    kind_filter: Option<LayerKind>,
) -> usize {
    rows_filtered(doc, expanded, collapsed_layers, query, kind_filter).len()
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
    kind_filter: Option<LayerKind>,
) -> f64 {
    let metric_row_h = || ui_px(size.row_h());
    let n = if document_open { row_count(doc, expanded, collapsed_layers, query, kind_filter) } else { 0 };
    chrome_h() + n as f64 * metric_row_h() + metric_footer_h()
}

/// The scrollable list area (between the chrome and the footer).
fn list_rect(body: Rect) -> Rect {
    Rect::new(body.x0, body.y0 + chrome_h(), body.x1, body.y1 - metric_footer_h())
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
    kind_filter: Option<LayerKind>,
) -> Option<LayerDrop> {
    let metric_row_h = || ui_px(size.row_h());
    let list = list_rect(body);
    if pointer.x < list.x0 || pointer.x > list.x1 || pointer.y < list.y0 || pointer.y > list.y1 {
        return None;
    }
    let rows = rows_filtered(doc, expanded, collapsed_layers, query, kind_filter);
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
        Some(ObjectKind::Group(g)) if g.sublayer => "Sublayer",
        Some(ObjectKind::Group(g)) if g.clip.is_some() => "Clip Group",
        Some(ObjectKind::Group(_)) => "Group",
        Some(ObjectKind::Text(_)) => "Text",
        Some(ObjectKind::Image(_)) => "Image",
        Some(ObjectKind::Symbol(_)) => "Symbol",
        Some(ObjectKind::Adjustment(a)) => a.op.kind().label(),
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
    draw_chrome(scene, text, ctx, body);
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
        let hot = hot_row == Some(i as i64);
        paint_row(scene, text, ctx, row, r, hot);
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
    let raster_ready = has_sel && current_layer_is_raster(ctx);
    let mask_active = ctx.editing_mask.is_some() && ctx.editing_mask == ctx.selection.first().copied();
    paint_layers_footer(scene, text, body, ctx.theme, ctx.pointer, has_sel, can_edit, raster_ready, mask_active);
    let locate_r = locate_button_rect(body);
    let locate_hot = locate_r.contains(ctx.pointer);
    if locate_hot && has_sel {
        scene.fill(Fill::NonZero, ID, ctx.theme.text.with_alpha(0.08), None, &locate_r.to_rounded_rect(ui_px(3.0)));
    }
    let locate_c = footer_color(ctx.theme, has_sel, locate_hot);
    draw_footer_locate(scene, locate_r, locate_c);
    if ctx.layers_new_menu && can_edit {
        paint_new_layer_menu(scene, text, ctx, body);
    }
    if ctx.layers_blend_menu && has_sel {
        paint_layers_blend_menu(scene, text, ctx, body);
    }
}

fn paint_row(scene: &mut Scene, text: &mut TextContext, ctx: &Ctx, row: &LayerRow, r: Rect, hot: bool) {
    let th = ctx.theme;
    let cy = r.center().y;
    let (eye_r, lock_r) = row_eye_lock(r);
    let name_right = target_rect(r).x0 - ui_px(4.0);

    match row.kind {
        RowKind::Layer(lid) => {
            let has_obj_sel = !ctx.selection.is_empty();
            let owns = has_obj_sel
                && ctx
                    .selection
                    .iter()
                    .any(|o| owning_layer(ctx.doc, *o) == Some(lid));
            let selected = !has_obj_sel && ctx.selected_layer == Some(lid);
            paint_row_fill(scene, r, selected, owns, hot, th);
            let swatch = crate::convert::color(row.color.rgb());
            let rail_a = if row.visible { 1.0 } else { 0.35 };
            scene.fill(Fill::NonZero, ID, swatch.with_alpha(rail_a), None, &layer_rail_rect(r));
            draw_triangle(scene, layer_disclosure_rect(r).center().x, cy, row.expanded, th.text_dim);
            if ctx.layer_thumbnail_size == crate::prefs::LayerThumbnailSize::None {
                draw_layer_kind(scene, layer_name_x(r) - ui_px(8.0), cy, row.layer_kind, th.text_dim);
            }
            let editing = match ctx.renaming {
                Some((RenameId::Layer(l), buf)) if l == lid => Some(buf),
                _ => None,
            };
            let name_x = paint_thumbnail(scene, text, ctx, row, r, layer_name_x(r));
            let name_c = if !row.visible { th.text_dim } else { th.text };
            scene.push_clip_layer(Fill::NonZero, ID, &Rect::new(name_x, r.y0, name_right.max(name_x), r.y1));
            if editing.is_some() {
                draw_name_field(scene, text, th, name_x, r, &row.label, name_c, editing);
            } else {
                let baseline = r.center().y + ui_px(4.0);
                text.draw(scene, &row.label, 12.0, name_c, name_x, baseline);
                if owns {
                    text.draw(scene, &row.label, 12.0, name_c, name_x + 0.6, baseline);
                }
            }
            scene.pop_layer();
            let target = target_rect(r).center();
            scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.text_dim, None, &Circle::new(target, ui_px(3.5)));
            if selected || owns {
                scene.fill(Fill::NonZero, ID, swatch, None, &Circle::new(target, ui_px(2.0)));
            }
            let eye_c = if row.visible { th.text_dim } else { th.border };
            draw_eye(scene, eye_r.center().x, cy, row.visible, eye_c);
            if row.locked {
                draw_lock(scene, lock_r.center().x, cy, th.text);
            } else if hot {
                draw_lock(scene, lock_r.center().x, cy, th.border);
            }
        }
        RowKind::Object { id, is_group } => {
            let selected = ctx.selection.contains(&id);
            paint_row_fill(scene, r, selected, false, hot, th);
            let indent = layer_disclosure_rect(r).x0 + row.depth as f64 * metric_indent();
            for d in 1..=row.depth {
                let x = layer_disclosure_rect(r).x0 + (d as f64 - 0.5) * metric_indent();
                scene.stroke(
                    &Stroke::new(ui_px(1.0)),
                    ID,
                    th.border.with_alpha(0.35),
                    None,
                    &Line::new((x, r.y0), (x, r.y1)),
                );
            }
            if is_group {
                draw_triangle(scene, indent + metric_col() * 0.5, cy, row.expanded, th.text_dim);
            }
            let icon_x = indent + if is_group { metric_col() * 1.5 } else { metric_col() * 0.5 };
            draw_object_kind(scene, icon_x, cy, ctx.doc.object(id).map(|o| &o.kind), th.text_dim);
            let name_c = if !row.visible || row.locked {
                th.border
            } else if selected {
                th.text
            } else {
                th.text_dim
            };
            let editing = match ctx.renaming {
                Some((RenameId::Object(o), buf)) if o == id => Some(buf),
                _ => None,
            };
            let name_x = paint_thumbnail(scene, text, ctx, row, r, icon_x + metric_col() * 0.7);
            scene.push_clip_layer(Fill::NonZero, ID, &Rect::new(r.x0, r.y0, name_right, r.y1));
            draw_name_field(scene, text, th, name_x, r, &row.label, name_c, editing);
            scene.pop_layer();
            let eye_c = if row.visible { th.text_dim } else { th.border };
            draw_eye(scene, eye_r.center().x, cy, row.visible, eye_c);
            if row.locked {
                draw_lock(scene, lock_r.center().x, cy, th.text);
            } else if hot {
                draw_lock(scene, lock_r.center().x, cy, th.border);
            }
        }
    }
    for x in [eye_r.x1, lock_r.x1, target_rect(r).x0] {
        scene.stroke(&Stroke::new(ui_px(0.5)), ID, th.border.with_alpha(0.4), None, &Line::new((x, r.y0), (x, r.y1)));
    }
    scene.stroke(&Stroke::new(ui_px(0.5)), ID, th.border.with_alpha(0.5), None, &Line::new((r.x0, r.y1), (r.x1, r.y1)));
}

fn paint_row_fill(scene: &mut Scene, r: Rect, selected: bool, owns: bool, hot: bool, th: &Theme) {
    if selected {
        scene.fill(Fill::NonZero, ID, th.accent.with_alpha(0.22), None, &r);
    } else if owns {
        scene.fill(Fill::NonZero, ID, th.accent.with_alpha(0.10), None, &r);
    } else if hot {
        scene.fill(Fill::NonZero, ID, th.text.with_alpha(0.05), None, &r);
    }
}

/// Which layer's `kind` governs the footer's Raster-only buttons —
/// mirrors `App::current_layer_kind`/`panels::tools::ctx_layer_kind`,
/// computed here from plain `Ctx` fields.
fn current_layer_is_raster(ctx: &Ctx) -> bool {
    let layer_id = ctx.selection.first().and_then(|&id| owning_layer(ctx.doc, id)).or(ctx.selected_layer);
    layer_id.and_then(|id| ctx.doc.layer(id)).map(|l| l.kind) == Some(LayerKind::Raster)
}

/// The Layers footer's own button set, left→right: Link, fx (layer
/// effects), Add Layer Mask, New Fill/Adjustment Layer, New Group, New
/// Layer, Delete. Deliberately not the generic four-slot
/// `panels::panel_footer_rects` other panels share — Photoshop's layer
/// footer needs a much richer set, and the redundant move up/down arrows
/// are gone (drag-reorder and the restack shortcuts/menu items already
/// cover that).
fn layers_footer_rects(body: Rect) -> [Rect; 7] {
    let sz = ui_px(20.0);
    let gap = ui_px(10.0);
    let cy = body.y1 - metric_footer_h() * 0.5;
    std::array::from_fn(|k| {
        let cx = body.x1 - metric_pad() - (6 - k) as f64 * (sz + gap) - sz * 0.5;
        Rect::from_center_size(Point::new(cx, cy), (sz, sz))
    })
}

fn draw_footer_link(scene: &mut Scene, r: Rect, color: Color) {
    let c = r.center();
    let (s, rad) = (ui_px(3.5), ui_px(4.5));
    scene.stroke(&Stroke::new(ui_px(1.5)), ID, color, None, &vello::kurbo::Circle::new((c.x - s, c.y), rad));
    scene.stroke(&Stroke::new(ui_px(1.5)), ID, color, None, &vello::kurbo::Circle::new((c.x + s, c.y), rad));
}

fn draw_footer_mask(scene: &mut Scene, r: Rect, color: Color) {
    let ri = r.inset(-ui_px(3.0));
    scene.stroke(&Stroke::new(ui_px(1.3)), ID, color, None, &ri.to_rounded_rect(ui_px(2.0)));
    scene.stroke(&Stroke::new(ui_px(1.3)), ID, color, None, &vello::kurbo::Circle::new(ri.center(), ri.width().min(ri.height()) * 0.3));
}

fn draw_footer_adjustment(scene: &mut Scene, r: Rect, color: Color) {
    let c = r.center();
    let rad = ui_px(7.0);
    let circle = vello::kurbo::Circle::new(c, rad);
    scene.push_clip_layer(Fill::NonZero, ID, &circle);
    scene.fill(Fill::NonZero, ID, color, None, &Rect::new(c.x - rad, c.y - rad, c.x, c.y + rad));
    scene.pop_layer();
    scene.stroke(&Stroke::new(ui_px(1.2)), ID, color, None, &circle);
}

/// Create Sublayer: a hooked arrow dropping into a small boxed plus (↳⊞).
fn draw_footer_sublayer(scene: &mut Scene, r: Rect, color: Color) {
    let bx = Rect::new(r.x0 + ui_px(8.5), r.y0 + ui_px(7.0), r.x1 - ui_px(1.5), r.y1 - ui_px(1.5));
    scene.stroke(&Stroke::new(ui_px(1.15)), ID, color, None, &bx.to_rounded_rect(ui_px(1.5)));
    let c = bx.center();
    let arm = ui_px(2.5);
    let mut plus = BezPath::new();
    plus.move_to((c.x - arm, c.y));
    plus.line_to((c.x + arm, c.y));
    plus.move_to((c.x, c.y - arm));
    plus.line_to((c.x, c.y + arm));
    scene.stroke(&Stroke::new(ui_px(1.15)), ID, color, None, &plus);
    let x = r.x0 + ui_px(3.0);
    let tip = Point::new(bx.x0 - ui_px(1.5), c.y);
    let mut hook = BezPath::new();
    hook.move_to((x, r.y0 + ui_px(2.5)));
    hook.line_to((x, c.y));
    hook.line_to(tip);
    hook.move_to((tip.x - ui_px(2.2), c.y - ui_px(2.2)));
    hook.line_to(tip);
    hook.line_to((tip.x - ui_px(2.2), c.y + ui_px(2.2)));
    scene.stroke(&Stroke::new(ui_px(1.15)), ID, color, None, &hook);
}

/// `raster_only` gates Link/fx/Mask/Adjustment: dimmed unless the current
/// context is confidently a Raster layer with a selection — they have no
/// vector-layer meaning yet, and no backing implementation at all yet
/// (clicking is a harmless no-op either way; see `hit`'s own comment).
fn paint_layers_footer(scene: &mut Scene, text: &mut TextContext, body: Rect, theme: &Theme, pointer: Point, has_sel: bool, can_edit: bool, raster_only: bool, mask_active: bool) {
    let strip = Rect::new(body.x0, body.y1 - metric_footer_h(), body.x1, body.y1);
    scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &strip);
    scene.fill(Fill::NonZero, ID, theme.border.with_alpha(0.7), None, &Rect::new(strip.x0, strip.y0, strip.x1, strip.y0 + 0.5));
    let rects = layers_footer_rects(body);
    let enabled = [raster_only, raster_only, raster_only, raster_only, can_edit, can_edit, has_sel];
    for (k, r) in rects.iter().enumerate() {
        let hot = enabled[k] && r.contains(pointer);
        if hot {
            scene.fill(Fill::NonZero, ID, theme.text.with_alpha(0.08), None, &r.to_rounded_rect(ui_px(3.0)));
        }
        let c = if k == 2 && mask_active { theme.accent } else { footer_color(theme, enabled[k], hot) };
        match k {
            0 => draw_footer_link(scene, *r, c),
            1 => text.draw(scene, "fx", 11.0, c, r.x0 + ui_px(1.0), r.center().y + ui_px(4.0)),
            2 => draw_footer_mask(scene, *r, c),
            3 => draw_footer_adjustment(scene, *r, c),
            4 => draw_footer_sublayer(scene, *r, c),
            5 => {
                scene.stroke(&Stroke::new(ui_px(1.15)), ID, c, None, &r.inset(ui_px(2.0)).to_rounded_rect(ui_px(2.0)));
                draw_footer_plus(scene, *r, c);
            }
            _ => draw_footer_trash(scene, *r, c),
        }
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
    let [_, _, _, _, _, add, _] = layers_footer_rects(body);
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
    if ctx.layers_blend_menu {
        let entries = blend_entries();
        for (i, &(_, mode)) in entries.iter().enumerate() {
            if layers_blend_entry_rect(body, entries.len(), i).contains(local) {
                return Action::SetSelectionBlendMode(mode);
            }
        }
        return Action::ToggleLayersBlendMenu;
    }
    if local.y >= body.y1 - metric_footer_h() {
        if !ctx.document_open {
            return Action::None;
        }
        if locate_button_rect(body).contains(local) {
            return if ctx.selection.is_empty() { Action::None } else { Action::LocateSelection };
        }
        let [link, fx, mask, adjustment, sublayer, add, del] = layers_footer_rects(body);
        // Link/fx/Adjustment have no backing implementation yet (see
        // `paint_layers_footer`'s own doc comment) — a harmless no-op
        // click either way, gated to look enabled only on a Raster layer
        // per the mockup this footer follows.
        return if link.contains(local) || fx.contains(local) || adjustment.contains(local) {
            Action::None
        } else if mask.contains(local) {
            if ctx.selection.is_empty() { Action::None } else { Action::AddOrToggleLayerMask }
        } else if sublayer.contains(local) {
            Action::CreateSublayer
        } else if add.contains(local) {
            Action::ToggleNewLayerMenu
        } else if del.contains(local) {
            Action::DeleteObjects
        } else {
            Action::None
        };
    }
    if local.y < body.y0 + metric_toolbar_h() {
        if !ctx.document_open {
            return Action::None;
        }
        let t = toolbar_layout(body);
        let has_blend = ctx.representative.as_ref().is_some_and(|a| !a.items.is_empty());
        return if t.blend.contains(local) {
            if ctx.selection.is_empty() || !has_blend { Action::None } else { Action::ToggleLayersBlendMenu }
        } else if t.opacity.contains(local) {
            if ctx.selection.is_empty() { Action::None } else { Action::BeginOpacityEdit }
        } else if t.lock_all.contains(local) {
            if ctx.selection.is_empty() && ctx.selected_layer.is_none() { Action::None } else { Action::ToggleLockAll }
        } else {
            Action::None
        };
    }
    if local.y < body.y0 + chrome_h() {
        if !ctx.document_open {
            return Action::None;
        }
        if filter_button_rect(body).contains(local) {
            return Action::CycleLayerFilter;
        }
        if search_box(body).contains(local) {
            return Action::FocusLayerSearch;
        }
        return Action::None;
    }
    let list = list_rect(body);
    let rows = visible_rows(ctx);
    let scroll = clamp_scroll(ctx.layer_scroll, rows.len(), list.height(), metric_row_h());
    let i = ((local.y - list.y0 + scroll) / metric_row_h()).floor();
    if i < 0.0 {
        return Action::None;
    }
    let Some(row) = rows.get(i as usize) else {
        return Action::None;
    };
    let ry = list.y0 + i * metric_row_h() - scroll;
    let r = Rect::new(body.x0, ry, body.x1, ry + metric_row_h());
    let (eye_r, lock_r) = row_eye_lock(r);
    match row.kind {
        // The color rail is its own hit target — a single click still
        // selects the layer like the rest of the row; a double click
        // (resolved by the caller) opens Layer Options instead of renaming.
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

pub(super) fn tip(body: Rect, local: Point, ctx: &Ctx) -> Option<&'static str> {
    if local.y >= body.y1 - metric_footer_h() {
        if locate_button_rect(body).contains(local) {
            return Some("Locate Object");
        }
        let [link, fx, mask, adjustment, sublayer, add, del] = layers_footer_rects(body);
        return if link.contains(local) {
            Some("Link Layers")
        } else if fx.contains(local) {
            Some("Layer Effects")
        } else if mask.contains(local) {
            Some("Add Layer Mask")
        } else if adjustment.contains(local) {
            Some("New Fill or Adjustment Layer")
        } else if sublayer.contains(local) {
            Some("Create Sublayer")
        } else if add.contains(local) {
            Some("New Layer")
        } else if del.contains(local) {
            Some("Delete")
        } else {
            None
        };
    }
    if local.y < body.y0 + metric_toolbar_h() {
        let t = toolbar_layout(body);
        return if t.blend.contains(local) {
            Some("Blend Mode")
        } else if t.opacity.contains(local) {
            Some("Opacity")
        } else if t.lock_transp.contains(local) {
            Some("Lock Transparent Pixels")
        } else if t.lock_paint.contains(local) {
            Some("Lock Image Pixels")
        } else if t.lock_pos.contains(local) {
            Some("Lock Position")
        } else if t.lock_all.contains(local) {
            Some("Lock All")
        } else {
            None
        };
    }
    if local.y < body.y0 + chrome_h() {
        if filter_button_rect(body).contains(local) {
            return Some(match ctx.layer_kind_filter {
                None => "Filter: All",
                Some(LayerKind::Vector) => "Filter: Vector",
                Some(LayerKind::Raster) => "Filter: Raster",
            });
        }
        if search_box(body).contains(local) {
            return Some("Search layers");
        }
    }
    None
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

/// Eye and lock columns on the left of every row, matching the
/// Illustrator Layers list: visibility, then lock, then the color rail.
fn row_eye_lock(row: Rect) -> (Rect, Rect) {
    let eye = Rect::new(row.x0, row.y0, row.x0 + ui_px(20.0), row.y1);
    let lock = Rect::new(eye.x1, row.y0, row.x0 + ui_px(40.0), row.y1);
    (eye, lock)
}

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

fn blend_entries() -> [(&'static str, BlendMode); 16] {
    [
        ("Normal", BlendMode::Normal),
        ("Multiply", BlendMode::Multiply),
        ("Screen", BlendMode::Screen),
        ("Overlay", BlendMode::Overlay),
        ("Darken", BlendMode::Darken),
        ("Lighten", BlendMode::Lighten),
        ("Color Dodge", BlendMode::ColorDodge),
        ("Color Burn", BlendMode::ColorBurn),
        ("Hard Light", BlendMode::HardLight),
        ("Soft Light", BlendMode::SoftLight),
        ("Difference", BlendMode::Difference),
        ("Exclusion", BlendMode::Exclusion),
        ("Hue", BlendMode::Hue),
        ("Saturation", BlendMode::Saturation),
        ("Color", BlendMode::Color),
        ("Luminosity", BlendMode::Luminosity),
    ]
}

fn blend_label(mode: BlendMode) -> &'static str {
    blend_entries()
        .iter()
        .find(|&&(_, m)| m == mode)
        .map(|&(label, _)| label)
        .unwrap_or("Normal")
}

const BLEND_MENU_ITEM_H: f64 = 22.0;
const BLEND_MENU_W: f64 = 140.0;

fn layers_blend_menu_rect(body: Rect, entry_count: usize) -> Rect {
    let blend = toolbar_layout(body).blend;
    let h = ui_px(BLEND_MENU_ITEM_H) * entry_count as f64;
    let y0 = (blend.y1 + ui_px(4.0)).min((body.y1 - h - ui_px(4.0)).max(body.y0 + ui_px(4.0)));
    let y1 = (y0 + h).min(body.y1 - ui_px(4.0));
    let x1 = (blend.x0 + ui_px(BLEND_MENU_W)).min(body.x1 - ui_px(4.0));
    Rect::new(blend.x0, y0, x1, y1)
}

fn layers_blend_entry_rect(body: Rect, entry_count: usize, i: usize) -> Rect {
    let menu = layers_blend_menu_rect(body, entry_count);
    let y = menu.y0 + i as f64 * ui_px(BLEND_MENU_ITEM_H);
    Rect::new(menu.x0, y, menu.x1, y + ui_px(BLEND_MENU_ITEM_H))
}

fn paint_layers_blend_menu(scene: &mut Scene, text: &mut TextContext, ctx: &Ctx, body: Rect) {
    let th = ctx.theme;
    let entries = blend_entries();
    let menu = layers_blend_menu_rect(body, entries.len());
    let round = menu.to_rounded_rect(ui_px(5.0));
    scene.fill(Fill::NonZero, ID, th.panel_bg, None, &round);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &round);
    let current = ctx.representative.as_ref().and_then(|a| a.items.last()).map(|i| i.blend_mode());
    scene.push_clip_layer(Fill::NonZero, ID, &menu);
    for (i, &(label, mode)) in entries.iter().enumerate() {
        let r = layers_blend_entry_rect(body, entries.len(), i);
        if current == Some(mode) {
            scene.fill(Fill::NonZero, ID, th.accent.with_alpha(0.18), None, &r);
        } else if r.contains(ctx.pointer) {
            scene.fill(Fill::NonZero, ID, th.text.with_alpha(0.06), None, &r);
        }
        text.draw(scene, label, 11.5, th.text, r.x0 + ui_px(10.0), r.center().y + ui_px(3.5));
    }
    scene.pop_layer();
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
    let x = x + size + ui_px(8.0);
    if let Some(mask) = row.mask {
        paint_mask_thumbnail(scene, ctx, row, mask, rect, x)
    } else {
        x
    }
}

/// A small square next to the main thumbnail showing a layer mask's own
/// pixels. Its RGB is unused (see `amalith_core::ImageMask`'s doc comment)
/// so this draws exactly like any other thumbnail — a white-RGB, variable-
/// alpha image over a dark backdrop already reads as light-to-dark, no
/// grayscale conversion needed. Bordered in the accent color while it's
/// what Brush/Eraser currently paint into.
fn paint_mask_thumbnail(scene: &mut Scene, ctx: &Ctx, row: &LayerRow, mask: amalith_core::ImageMask, rect: Rect, x: f64) -> f64 {
    let size = ui_px(ctx.layer_thumbnail_size.image_px());
    if size == 0.0 { return x; }
    let tile = Rect::from_origin_size((x, rect.center().y - size * 0.5), (size, size));
    scene.fill(Fill::NonZero, ID, Color::from_rgb8(0x20, 0x20, 0x20), None, &tile);
    if let Some(gpu) = ctx.layer_images.get(&mask.asset).and_then(|l| l.pick(size.max(1.0))) {
        let scale = (size / gpu.width.max(1) as f64).min(size / gpu.height.max(1) as f64);
        let (w, h) = (gpu.width as f64 * scale, gpu.height as f64 * scale);
        let origin = (tile.center().x - w * 0.5, tile.center().y - h * 0.5);
        scene.draw_image(gpu, vello::kurbo::Affine::translate(origin) * vello::kurbo::Affine::scale(scale));
    }
    let object = match row.kind { RowKind::Object { id, .. } => Some(id), RowKind::Layer(_) => None };
    let editing = mask.enabled && object.is_some() && ctx.editing_mask == object;
    let border = if editing { ctx.theme.accent } else { ctx.theme.border };
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, border, None, &tile);
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
        // Two stacked sheets: a sublayer is a layer, not a group.
        Some(ObjectKind::Group(g)) if g.sublayer => {
            let back = Rect::new(cx - ui_px(3.0), cy - ui_px(4.5), cx + ui_px(5.0), cy + ui_px(1.5));
            let front = Rect::new(cx - ui_px(5.0), cy - ui_px(2.0), cx + ui_px(3.0), cy + ui_px(4.0));
            scene.stroke(&Stroke::new(ui_px(1.1)), ID, color, None, &back.to_rounded_rect(ui_px(1.0)));
            scene.fill(Fill::NonZero, ID, color.with_alpha(0.25), None, &front.to_rounded_rect(ui_px(1.0)));
            scene.stroke(&Stroke::new(ui_px(1.1)), ID, color, None, &front.to_rounded_rect(ui_px(1.0)));
        }
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
        assert_eq!(content_height(&doc, &expanded, &collapsed, "", true, size, None), chrome_h() + ui_px(size.row_h()) + metric_footer_h());
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
            assert_eq!(content_height(&doc, &expanded, &HashSet::new(), "", true, size, None), chrome_h() + row_h + metric_footer_h());
            let pointer = Point::new(100.0, chrome_h() + row_h * 0.5);
            let target = drop_target(body, pointer, &doc, &expanded, &HashSet::new(), "", 0.0, &[], size, None).unwrap();
            assert_eq!(target.parent, ObjectParent::Layer(layer));
            assert!(target.into);
            assert_eq!(clamp_scroll(1000.0, 10, row_h * 3.0, row_h), row_h * 7.0);
        }
    }

    #[test]
    fn kind_filter_hides_other_layers_and_their_objects() {
        let mut doc = Document::new("Layers");
        let vector = LayerId::new();
        doc.insert_layer(amalith_core::Layer::new(vector, "Artwork"), 0);
        let id = ObjectId::new();
        doc.insert_object(amalith_core::Object::rectangle(id, ObjectParent::Layer(vector), amalith_core::Rect::new(0.0, 0.0, 10.0, 10.0)), 0).unwrap();
        let mut raster = amalith_core::Layer::new(LayerId::new(), "Photos");
        raster.kind = LayerKind::Raster;
        doc.insert_layer(raster, 1);
        let expanded = HashSet::new();
        let collapsed = HashSet::new();
        assert_eq!(rows_filtered(&doc, &expanded, &collapsed, "", None).len(), 3);
        let vector_rows = rows_filtered(&doc, &expanded, &collapsed, "", Some(LayerKind::Vector));
        assert_eq!(vector_rows.len(), 2);
        assert!(matches!(vector_rows[0].kind, RowKind::Layer(_)));
        let raster_rows = rows_filtered(&doc, &expanded, &collapsed, "", Some(LayerKind::Raster));
        assert_eq!(raster_rows.len(), 1);
        assert_eq!(raster_rows[0].label, "Photos");
    }

    #[test]
    fn opacity_field_sits_in_the_toolbar_not_the_search_row() {
        let body = Rect::new(0.0, 0.0, 320.0, 400.0);
        let t = toolbar_layout(body);
        assert!(opacity_field_at(body, t.opacity.center()));
        assert!(!opacity_field_at(body, search_box(body).center()));
        assert!(t.blend.y1 <= search_row_rect(body).y0 + 0.5);
    }
}
