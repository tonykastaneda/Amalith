//! Document hit-testing for the selection tool, ported from
//! `amalith-app`'s `topmost_selectable_at` / marquee logic.
//!
//! Everything is bounds-based (document-space AABBs — [`bounds`], which
//! already unions a group's descendants) and culled to the visible rect,
//! so a click selects the whole group and never reaches an off-screen
//! object. A Path/CompoundPath's own bounds and hit test both come from
//! [`item_shapes`] — its own Appearance items' *actually painted* shape,
//! live effects included, never the raw stored geometry alone — so a
//! click always matches what's really on screen, for any current or
//! future effect kind, without this module needing its own per-kind
//! carve-out every time one is added. Coordinates are vello kurbo.

use amalith_core::{Document, ObjectId, ObjectKind, ObjectParent};
use vello::kurbo::{Affine, ParamCurveNearest, PathSeg, Point, Rect, Shape};

use crate::convert;

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

/// The mask child of `id` if it's a clip group, else `None`.
pub fn clip_mask_of(doc: &Document, id: ObjectId) -> Option<ObjectId> {
    clip_target(doc, id)
}

/// A clip group's mask child — the object whose bounds should stand in
/// for the whole group's, since the clipped-away parts aren't visible.
fn clip_target(doc: &Document, id: ObjectId) -> Option<ObjectId> {
    match doc.object(id).map(|o| &o.kind) {
        Some(ObjectKind::Group(g)) => g.clip.filter(|c| doc.object(*c).is_some()),
        _ => None,
    }
}

/// Whether `id` is one of a blend group's own *generated* in-between
/// steps — computed geometry, discarded and rebuilt on the next
/// regeneration, not an independently selectable object of its own
/// (see `topmost_in`'s own doc comment for the click-selection side of
/// this same rule) — as opposed to the blend's two real originals
/// (`start`/`end`), which still are. Any consumer that walks every path
/// in the document regardless of grouping (Object Highlighting's hover,
/// via `anchors::path_leaves`) needs this same exclusion, or it ends up
/// treating a step as a real, independently hoverable/selectable shape
/// when it visually is not one.
pub fn is_blend_step(doc: &Document, id: ObjectId) -> bool {
    let Some(obj) = doc.object(id) else { return false };
    let ObjectParent::Group(parent) = obj.parent else {
        return false;
    };
    match doc.object(parent).map(|o| &o.kind) {
        Some(ObjectKind::Group(g)) => g.blend.is_some_and(|b| id != b.start && id != b.end),
        _ => false,
    }
}

/// Document-space bounds of `id` (vello kurbo), or `None` if it has none.
/// For a clip group this is the mask shape's bounds, not the union of the
/// (partly hidden) contents.
pub fn bounds(doc: &Document, id: ObjectId) -> Option<Rect> {
    let target = clip_target(doc, id).unwrap_or(id);
    effective_bounds(doc, target)
}

/// Like [`Document::bounds_of`], except a Path/CompoundPath's own bounds
/// union every one of its own visible Appearance items' *own* painted
/// shape (see [`item_shapes`]) instead of just the raw stored geometry.
/// A live effect (Zig Zag, Offset Path, ...) can make the rendered shape
/// noticeably bigger than its base path — a near-flat line with a large
/// Zig Zag, say, has a near-zero-height base bbox but real height once
/// drawn — so without this, every bounds-based pre-filter in this module
/// (the `b.contains(point)` / `overlaps` checks below) would reject
/// clicks squarely on the visible shape. A group still unions its
/// children's (possibly grown) bounds, recursively.
fn effective_bounds(doc: &Document, id: ObjectId) -> Option<Rect> {
    match doc.object(id).map(|o| &o.kind) {
        Some(ObjectKind::Group(g)) => g.children.iter().filter_map(|&c| effective_bounds(doc, c)).reduce(|a, b| a.union(b)),
        Some(ObjectKind::Path(_) | ObjectKind::CompoundPath(_)) => item_shapes(doc, id)
            .into_iter()
            .map(|s| s.contour.bounding_box())
            .reduce(|a, b| a.union(b))
            .or_else(|| doc.bounds_of(id).map(convert::rect)),
        Some(_) => doc.bounds_of(id).map(convert::rect),
        None => None,
    }
}

/// Frontmost layer-child whose bounds contain `point` and overlap
/// `visible`. Layer direct children only. A plain path or compound path
/// has to have `point` inside its *real* outline, not just its
/// circumscribing box — a circle's own bounding box has empty corners
/// that read as "inside" under a pure box test, which used to let a
/// click there wrongly hit that circle instead of whatever (or nothing)
/// is actually drawn there. A `Group` is selected as a unit, but only
/// when the click actually lands on one of its *descendants'* real
/// content (recursing through nested groups) — its own bounding box
/// alone isn't enough: a sparse group (a radial "starburst" of thin
/// spokes, say) has enormous empty space inside its own AABB, and a
/// click on visibly empty canvas there must deselect, not grab the group.
///
/// `tol` (document units) is extra grab slop around a stroke's own
/// half-width — see [`path_hit`]'s own doc comment; pass
/// [`DEFAULT_CLICK_TOLERANCE`] `/ zoom` unless the caller has a specific
/// reason not to (a hairline test, say).
pub fn topmost_selectable_at(doc: &Document, point: Point, visible: Rect, tol: f64) -> Option<ObjectId> {
    for layer in doc.layers().iter().rev() {
        if !layer.visible {
            continue;
        }
        for &id in doc.children_of(ObjectParent::Layer(layer.id)).iter().rev() {
            let Some(obj) = doc.object(id) else { continue };
            if !obj.visible || obj.locked {
                continue;
            }
            let Some(b) = bounds(doc, id) else { continue };
            if !overlaps(b, visible) {
                continue;
            }
            let hit = match &obj.kind {
                ObjectKind::Path(_) | ObjectKind::CompoundPath(_) => path_hit(doc, id, point, tol),
                ObjectKind::Group(_) => group_hit(doc, id, point, tol),
                _ => b.contains(point),
            };
            if !hit {
                continue;
            }
            return Some(id);
        }
    }
    None
}

/// Default extra grab slop (screen px, divide by zoom before use) for
/// stroke-proximity hit-testing — [`topmost_selectable_at`],
/// [`group_hit`], [`topmost_in`]. Matches the grab radius already used
/// elsewhere in the app for a similar "small fixed screen-space target"
/// (Direct Selection's own anchor/segment grab radius is a bit larger,
/// 6.0, since an anchor is a much smaller point target than a stroke).
pub const DEFAULT_CLICK_TOLERANCE: f64 = 3.0;

/// One visible Fill or Stroke Appearance item's own painted shape (doc
/// space) — this item's own live effect stack applied to the object's
/// base geometry, not the raw stored path. A Path/CompoundPath can
/// carry several items (several fills, several strokes — see
/// [`amalith_core::AppearanceItem`]), each independently reshaped by
/// its own effects, so there is no single "the" contour that correctly
/// stands in for the whole object once effects are involved: hit-
/// testing and bounds both have to check every one of them on its own
/// terms. This is the one place that computation happens for this
/// module — every hit-test / bounds function below is built from it, so
/// a future effect kind or a future consumer of this module can't
/// silently drift back to testing the wrong (raw, un-shaped) geometry.
struct ItemShape {
    is_fill: bool,
    paint: amalith_core::Paint,
    contour: vello::kurbo::BezPath,
    /// Meaningless for a fill.
    stroke_width: f64,
}

/// For a Stroke item with an effect: both its own live-rendered line
/// *and* its plain underlying base path are valid hit targets — an
/// effect is non-destructive, so the real path is still the object
/// you'd select (Direct Selection edits its actual anchors, and the
/// hover / selection outline traces this same base path — see
/// [`base_contour`]); it shouldn't stop being clickable just because
/// its effect happens to have moved the painted line somewhere else.
/// Fill items don't get this fallback: an effect like Offset Path
/// genuinely changes what area is filled (an inset fill's vacated ring
/// is deliberately *not* still clickable as fill — see
/// `fill_and_stroke_items_are_hit_tested_against_their_own_independent_effects`),
/// where a stroke effect like Zig Zag never stops being "the same path,"
/// just drawn differently.
fn item_shapes(doc: &Document, id: ObjectId) -> Vec<ItemShape> {
    let Some(obj) = doc.object(id) else { return Vec::new() };
    let base: vello::kurbo::BezPath = match &obj.kind {
        ObjectKind::Path(pd) => convert::bez_path(&pd.geometry),
        ObjectKind::CompoundPath(cp) => {
            let mut b = vello::kurbo::BezPath::new();
            for sub in &cp.subpaths {
                b.extend(convert::bez_path(sub));
            }
            b
        }
        _ => return Vec::new(),
    };
    let m: Affine = convert::affine(doc.world_transform(id));
    let core_base = convert::bez_path_to_core(&base);
    let mut out = Vec::new();
    for item in obj.appearance.items.iter().filter(|item| item.visible()) {
        let is_fill = item.is_fill();
        let paint = item.paint();
        let stroke_width = match item {
            amalith_core::AppearanceItem::Stroke { width, .. } => *width,
            amalith_core::AppearanceItem::Fill { .. } => 0.0,
        };
        let effects = item.effects();
        if effects.is_empty() {
            out.push(ItemShape { is_fill, paint, contour: m * base.clone(), stroke_width });
            continue;
        }
        let shaped = crate::canvas::apply_effect_chain(&core_base, effects)
            .map(|g| convert::bez_path(&g))
            .unwrap_or_else(|| base.clone());
        out.push(ItemShape { is_fill, paint, contour: m * shaped, stroke_width });
        if !is_fill {
            out.push(ItemShape { is_fill, paint, contour: m * base.clone(), stroke_width });
        }
    }
    out
}

/// Whether `point` hits a path/compound path's own real content: inside
/// any of its own visible fills, or near any of its own visible
/// strokes' actual line — checked per Appearance item via
/// [`item_shapes`] rather than against one representative shape, since
/// a Fill and a Stroke row can each carry their own independent live
/// effect stack and so can each paint a genuinely different outline.
/// Same as Illustrator, where a filled shape's outline is just as
/// clickable as its interior. The outline test isn't just for unfilled
/// shapes: a geometrically degenerate path (near-zero area — e.g. a
/// perfectly straight line, open or closed) has no real "inside" for
/// the fill/winding test to ever find true, filled or not, so it would
/// otherwise be permanently unclickable despite rendering a visible
/// line. Skips the caller's `bounds(..).contains(point)` box pre-filter
/// entirely — a straight line's bounding box is zero-width along one
/// axis (e.g. `x0 == x1`), which would reject all but a click landing
/// on that exact float coordinate.
/// `tol` is extra grab slop (document units) added on top of a stroke's
/// own half-width — without it, clicking a thin (or hairline) stroke
/// demanded landing within a fraction of a pixel of its exact center,
/// which read as needing to hit with the literal cursor tip. A fill's
/// own interior needs no such slop (it's already a generous target).
fn path_hit(doc: &Document, id: ObjectId, point: Point, tol: f64) -> bool {
    item_shapes(doc, id).into_iter().any(|shape| {
        if shape.paint == amalith_core::Paint::None {
            return false;
        }
        if shape.is_fill {
            return close_open_subpaths(&shape.contour).winding(point) != 0;
        }
        // Same "no invisible sticky halo" reasoning as before: only a
        // real (positive-width) stroke is clickable near its line.
        if shape.stroke_width <= 0.0 {
            return false;
        }
        let t = shape.stroke_width * 0.5 + tol;
        let t2 = t * t;
        shape.contour.segments().any(|seg: PathSeg| seg.nearest(point, 0.1).distance_sq <= t2)
    })
}

/// Whether `point` (doc space) lands on real content somewhere inside
/// group `id`, recursing through nested groups — see
/// [`topmost_selectable_at`]'s doc comment for why a group's bounding
/// box alone can't answer this. Deliberately does *not* gate a Path /
/// CompoundPath / nested-Group child on `bounds(..).contains(point)`
/// first: [`path_hit`] is already self-sufficient (and, per its own doc
/// comment, deliberately skips exactly that kind of box pre-filter), so
/// requiring the box to *also* contain the point first only reintroduces
/// the same false-miss it was written to avoid — a click landing right
/// on a shape's outermost silhouette pixel sits exactly on that box's
/// edge, and `Rect::contains` is a half-open test (exclusive of `x1`/
/// `y1`), so it can reject a point `path_hit` itself would happily
/// accept. Text/Image/Symbol children have no such precise test of
/// their own, so they still fall back to plain bounds containment.
fn group_hit(doc: &Document, id: ObjectId, point: Point, tol: f64) -> bool {
    doc.children_of(ObjectParent::Group(id)).iter().any(|&child| {
        let Some(obj) = doc.object(child) else { return false };
        if !obj.visible {
            return false;
        }
        match &obj.kind {
            ObjectKind::Path(_) | ObjectKind::CompoundPath(_) => path_hit(doc, child, point, tol),
            ObjectKind::Group(_) => group_hit(doc, child, point, tol),
            _ => bounds(doc, child).is_some_and(|b| b.contains(point)),
        }
    })
}

/// Frontmost of `ids` (searched back-to-front, so later entries win)
/// with any Appearance item's own *painted* shape (live effects
/// applied — see [`item_shapes`]) at `point` — Smart Guides' Object
/// Highlighting hover, which exists to trace exactly what's visually
/// under the cursor: inside any visible fill, or within `tol` of any
/// visible stroke's own line (`tol` on top of the stroke's own width,
/// so hovering doesn't need to land pixel-perfectly on a thin line —
/// unlike a click, which is a deliberate, precise gesture). Not the raw
/// stored geometry a Zig Zag or Offset Path effect may have long since
/// displaced. `ids` is typically `anchors::path_leaves(doc)` (every
/// path, recursing through every group — Object Highlighting reaches
/// sub-objects a plain click-select wouldn't, by design).
pub fn nearest_painted_leaf(doc: &Document, ids: &[ObjectId], point: Point, tol: f64) -> Option<ObjectId> {
    ids.iter().rev().copied().find(|&id| {
        item_shapes(doc, id).iter().any(|shape| {
            if shape.paint == amalith_core::Paint::None {
                return false;
            }
            if shape.is_fill {
                return close_open_subpaths(&shape.contour).winding(point) != 0;
            }
            if shape.stroke_width <= 0.0 {
                return false;
            }
            let t = shape.stroke_width * 0.5 + tol;
            shape.contour.segments().any(|seg: PathSeg| seg.nearest(point, 0.1).distance_sq <= t * t)
        })
    })
}

/// Returns `bez` with a `ClosePath` inserted at the end of every subpath
/// that doesn't already have one, mirroring the implicit close a fill
/// rasterizer applies to open contours.
fn close_open_subpaths(bez: &vello::kurbo::BezPath) -> vello::kurbo::BezPath {
    use vello::kurbo::PathEl;
    let mut out = vello::kurbo::BezPath::new();
    let mut open = false;
    for el in bez.elements() {
        if matches!(el, PathEl::MoveTo(_)) && open {
            out.close_path();
        }
        open = !matches!(el, PathEl::ClosePath);
        out.push(*el);
    }
    if open {
        out.close_path();
    }
    out
}

/// `(id, bounds)` for every visible, layer-direct-child object overlapping
/// `visible` — Smart Guides' Alignment Guides candidate list. Same walk as
/// `topmost_selectable_at`, minus `excluding` (the object(s) currently
/// being dragged, which shouldn't snap to their own bounds) — but unlike
/// that hit-test, a **locked** object is still included: Illustrator keeps
/// locked artwork as a live alignment reference (it's exactly the kind of
/// fixed geometry you'd want to align new work to), it just can't be
/// clicked or dragged itself.
pub fn visible_top_level_bounds(doc: &Document, visible: Rect, excluding: &[ObjectId]) -> Vec<(ObjectId, Rect)> {
    let mut out = Vec::new();
    for layer in doc.layers().iter().rev() {
        if !layer.visible {
            continue;
        }
        for &id in doc.children_of(ObjectParent::Layer(layer.id)).iter().rev() {
            if excluding.contains(&id) {
                continue;
            }
            let Some(obj) = doc.object(id) else { continue };
            if !obj.visible {
                continue;
            }
            if let Some(b) = bounds(doc, id) {
                if overlaps(b, visible) {
                    out.push((id, b));
                }
            }
        }
    }
    out
}

/// The isolation-scoped sibling of [`visible_top_level_bounds`]: `(id,
/// bounds)` for every visible child of `group` overlapping `visible` —
/// Alignment Guides while isolated into a group, matching Illustrator's
/// own scoping (isolating hides the rest of the document as alignment
/// noise, same as it hides it from selection). Skips a clip group's own
/// mask child, same as [`topmost_in`] — it's a cutout shape, not content
/// to align against — and a blend group's generated in-between steps,
/// which aren't independent objects at all. Locked children still count,
/// same reasoning as `visible_top_level_bounds`.
pub fn bounds_within(doc: &Document, group: ObjectId, visible: Rect, excluding: &[ObjectId]) -> Vec<(ObjectId, Rect)> {
    let (clip, blend) = match doc.object(group).map(|o| &o.kind) {
        Some(ObjectKind::Group(g)) => (g.clip, g.blend),
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for &id in doc.children_of(ObjectParent::Group(group)).iter().rev() {
        if Some(id) == clip || excluding.contains(&id) {
            continue;
        }
        if let Some(b) = blend {
            if id != b.start && id != b.end {
                continue;
            }
        }
        let Some(obj) = doc.object(id) else { continue };
        if !obj.visible {
            continue;
        }
        if let Some(b) = bounds(doc, id) {
            if overlaps(b, visible) {
                out.push((id, b));
            }
        }
    }
    out
}

/// Frontmost direct child of `group` whose *real content* `point` lands
/// on — isolation-mode hit-testing, where selection is scoped to one
/// group. Deliberately the same precision [`topmost_selectable_at`]
/// uses at the top level (a path/compound path needs [`path_hit`], not
/// just its bounding box) — isolating into a group is a change of
/// *scope*, not a downgrade to a coarser click test; a click in a
/// circle's empty bbox corner, or in the hollow middle of a donut
/// shape, has to miss here exactly as it would un-isolated.
///
/// In a clip group the mask shape is skipped by the normal pass so
/// clicks fall through to the masked content; the mask is only picked
/// when the click lands within `contour_tol` (document units) of its
/// actual outline.
pub fn topmost_in(
    doc: &Document,
    group: ObjectId,
    point: Point,
    contour_tol: f64,
) -> Option<ObjectId> {
    let (clip, blend) = match doc.object(group).map(|o| &o.kind) {
        Some(ObjectKind::Group(g)) => (g.clip, g.blend),
        Some(kind) => {
            // A bare object was isolated: only it is selectable, hit the
            // same way any other path/compound path would be, or (for a
            // shape with no precise test of its own — text, an image, a
            // symbol) by its bounds.
            let hit = match kind {
                ObjectKind::Path(_) | ObjectKind::CompoundPath(_) => path_hit(doc, group, point, contour_tol),
                _ => bounds(doc, group).is_some_and(|b| b.contains(point)),
            };
            return hit.then_some(group);
        }
        None => return None,
    };
    for &id in doc.children_of(ObjectParent::Group(group)).iter().rev() {
        if Some(id) == clip {
            continue;
        }
        // A blend group's generated in-between steps aren't independently
        // selectable — only the two shapes it interpolates between are,
        // matching Illustrator (the steps are computed geometry, not
        // editable objects of their own, and get discarded on the next
        // rebuild regardless of anything done to them directly).
        if let Some(b) = blend {
            if id != b.start && id != b.end {
                continue;
            }
        }
        let Some(obj) = doc.object(id) else { continue };
        if !obj.visible || obj.locked {
            continue;
        }
        // No eager `bounds(..).contains(point)` gate before the precise
        // tests below — see `group_hit`'s own doc comment for exactly
        // why that reintroduces a false-miss at a shape's outer edge.
        let hit = match &obj.kind {
            ObjectKind::Path(_) | ObjectKind::CompoundPath(_) => path_hit(doc, id, point, contour_tol),
            ObjectKind::Group(_) => group_hit(doc, id, point, contour_tol),
            _ => bounds(doc, id).is_some_and(|b| b.contains(point)),
        };
        if hit {
            return Some(id);
        }
    }
    // Only the mask's contour is clickable.
    clip.filter(|&cid| near_contour(doc, cid, point, contour_tol))
}

/// `id`'s own stored geometry, world-transformed — paths and compound
/// paths only, no live Appearance effect applied. An effect like Zig
/// Zag or Offset Path is non-destructive: the anchors you'd actually
/// edit with Direct Selection still sit on this plain underlying shape,
/// not on whatever the effect renders — so this (not [`object_contour`],
/// which deliberately *does* follow the effect, for hit-testing) is
/// what the hover highlight and the per-object selection outline trace.
pub fn base_contour(doc: &Document, id: ObjectId) -> Option<vello::kurbo::BezPath> {
    let obj = doc.object(id)?;
    let base: vello::kurbo::BezPath = match &obj.kind {
        ObjectKind::Path(pd) => convert::bez_path(&pd.geometry),
        ObjectKind::CompoundPath(cp) => {
            let mut b = vello::kurbo::BezPath::new();
            for sub in &cp.subpaths {
                b.extend(convert::bez_path(sub));
            }
            b
        }
        _ => return None,
    };
    let m: Affine = convert::affine(doc.world_transform(id));
    Some(m * base)
}

/// `id`'s outline as a document-space `BezPath` — paths and compound
/// paths only. Reflects a live Appearance effect (Zig Zag, Offset
/// Path, ...) when it has one, not just the raw stored geometry — a
/// Fill and a Stroke item can each carry their own independent effect
/// stack (see `amalith_core::AppearanceItem`), but every caller here
/// wants one representative outline, so this picks the Stroke item's
/// effects if it has any, else the Fill item's, else the base
/// geometry unchanged. Without this, a click landing squarely on the
/// *rendered* (effect-shaped) outline — what `canvas.rs::paint_object`
/// actually draws — would miss every hit test in this module, since
/// the un-shaped base path underneath can sit anywhere up to the
/// effect's own displacement away from what's actually on screen.
pub fn object_contour(doc: &Document, id: ObjectId) -> Option<vello::kurbo::BezPath> {
    let obj = doc.object(id)?;
    let base: vello::kurbo::BezPath = match &obj.kind {
        ObjectKind::Path(pd) => convert::bez_path(&pd.geometry),
        ObjectKind::CompoundPath(cp) => {
            let mut b = vello::kurbo::BezPath::new();
            for sub in &cp.subpaths {
                b.extend(convert::bez_path(sub));
            }
            b
        }
        _ => return None,
    };
    let m: Affine = convert::affine(doc.world_transform(id));
    let effects = obj
        .appearance
        .items
        .iter()
        .rev()
        .find(|i| i.is_stroke() && !i.effects().is_empty())
        .or_else(|| obj.appearance.items.iter().rev().find(|i| i.is_fill() && !i.effects().is_empty()))
        .map(amalith_core::AppearanceItem::effects)
        .unwrap_or(&[]);
    if effects.is_empty() {
        return Some(m * base);
    }
    let core_base = convert::bez_path_to_core(&base);
    let shaped = crate::canvas::apply_effect_chain(&core_base, effects)
        .map(|g| convert::bez_path(&g))
        .unwrap_or(base);
    Some(m * shaped)
}

/// Whether `point` (document space) is within `tol` of `id`'s stroked
/// outline. Paths and compound paths only.
fn near_contour(doc: &Document, id: ObjectId, point: Point, tol: f64) -> bool {
    let Some(obj) = doc.object(id) else {
        return false;
    };
    if obj.locked || !obj.visible {
        return false;
    }
    let Some(bez) = object_contour(doc, id) else {
        return false;
    };
    let t2 = tol * tol;
    let hit = bez
        .segments()
        .any(|seg: PathSeg| seg.nearest(point, 0.1).distance_sq <= t2);
    hit
}

/// Frontmost visible, unlocked `Path` (not `CompoundPath` — see
/// `pathtext::resolve`'s doc comment) whose outline `point` is within
/// `tol` of, searching every layer front-to-back. Layer children only,
/// same scope as [`topmost_selectable_at`] — the Type tool doesn't reach
/// into groups to start type-on-a-path. Drives its click-on-a-path
/// detection, so a click has to be a Path a plain click wouldn't already
/// select as text or fall through to drawing a new text box on.
pub fn topmost_path_near(doc: &Document, point: Point, visible: Rect, tol: f64) -> Option<ObjectId> {
    for layer in doc.layers().iter().rev() {
        if !layer.visible || layer.locked {
            continue;
        }
        for &id in doc.children_of(ObjectParent::Layer(layer.id)).iter().rev() {
            let Some(obj) = doc.object(id) else { continue };
            if !obj.visible || obj.locked || !matches!(obj.kind, ObjectKind::Path(_)) {
                continue;
            }
            if let ObjectKind::Path(pd) = &obj.kind {
                if pd.subpaths().len() != 1 { continue; }
            }
            if let Some(b) = bounds(doc, id) {
                if !overlaps(b, visible) {
                    continue;
                }
            }
            if near_contour(doc, id, point, tol) {
                return Some(id);
            }
        }
    }
    None
}

/// The clip mask of `group` if `point` (doc space) is within `tol` of its
/// contour — drives the isolation-mode hover highlight.
pub fn clip_mask_at_contour(
    doc: &Document,
    group: ObjectId,
    point: Point,
    tol: f64,
) -> Option<ObjectId> {
    let clip = match doc.object(group).map(|o| &o.kind) {
        Some(ObjectKind::Group(g)) => g.clip?,
        _ => return None,
    };
    near_contour(doc, clip, point, tol).then_some(clip)
}

/// Direct children of `group` whose bounds intersect `marquee`. When a
/// bare object is isolated it is the only candidate.
pub fn within_in(doc: &Document, group: ObjectId, marquee: Rect) -> Vec<ObjectId> {
    if !matches!(doc.object(group).map(|o| &o.kind), Some(ObjectKind::Group(_))) {
        return match bounds(doc, group) {
            Some(b) if overlaps(b, marquee) => vec![group],
            _ => Vec::new(),
        };
    }
    let blend = match doc.object(group).map(|o| &o.kind) {
        Some(ObjectKind::Group(g)) => g.blend,
        _ => None,
    };
    doc.children_of(ObjectParent::Group(group))
        .iter()
        .copied()
        // A blend group's generated in-between steps aren't independently
        // selectable — see `topmost_in`'s doc comment for why.
        .filter(|&id| match blend {
            Some(b) => id == b.start || id == b.end,
            None => true,
        })
        .filter(|id| doc.object(*id).is_some_and(|o| o.visible && !o.locked))
        .filter(|id| bounds(doc, *id).is_some_and(|b| overlaps(b, marquee)))
        .collect()
}

/// Layer-children whose bounds intersect `marquee` (document space).
pub fn within(doc: &Document, marquee: Rect) -> Vec<ObjectId> {
    let mut out = Vec::new();
    for layer in doc.layers() {
        if !layer.visible {
            continue;
        }
        for &id in doc.children_of(ObjectParent::Layer(layer.id)) {
            if doc.object(id).is_some_and(|o| !o.visible || o.locked) {
                continue;
            }
            if bounds(doc, id).is_some_and(|b| overlaps(b, marquee)) {
                out.push(id);
            }
        }
    }
    out
}

/// Union of the given objects' bounds — the axis-aligned selection box.
pub fn union_bounds(doc: &Document, ids: &[ObjectId]) -> Option<Rect> {
    let mut acc: Option<Rect> = None;
    for &id in ids {
        if let Some(b) = bounds(doc, id) {
            acc = Some(acc.map_or(b, |a| a.union(b)));
        }
    }
    acc
}

/// The oriented selection box: a single object's rotated corner quad, or
/// the axis-aligned union box (as a quad) for a multi-selection.
pub fn selection_quad(doc: &Document, ids: &[ObjectId]) -> Option<[vello::kurbo::Point; 4]> {
    if ids.len() == 1 {
        if let Some(ObjectKind::Text(td)) = doc.object(ids[0]).map(|o| &o.kind) {
            if let Some(path) = &td.path_geometry {
                let m = convert::affine(doc.world_transform(ids[0]));
                return Some(crate::handles::rect_quad(convert::rect(path.local_bounds())).map(|p| m * p));
            }
        }
        // A clip group's oriented box is the mask shape's.
        let id = clip_target(doc, ids[0]).unwrap_or(ids[0]);
        let local = convert::rect(doc.local_bounds_of(id)?);
        let m = convert::affine(doc.world_transform(id));
        return Some(crate::handles::rect_quad(local).map(|p| m * p));
    }
    union_bounds(doc, ids).map(crate::handles::rect_quad)
}

#[cfg(test)]
mod smart_guide_bounds_tests {
    use super::*;
    use amalith_core::{GroupData, Layer, LayerId, Object, Paint, PathData};

    fn rect_path(id: ObjectId, parent: ObjectParent, r: amalith_core::geom::Rect, locked: bool) -> Object {
        let mut o = Object::new(id, parent, ObjectKind::Path(PathData::rectangle(r)));
        o.locked = locked;
        o
    }

    /// A click on *either* of a blend's two originals — un-isolated, at
    /// the top level — has to select the blend group as a whole, the
    /// same as clicking any other child of any other ordinary group:
    /// there is nothing blend-specific about `topmost_selectable_at`'s
    /// own group handling, so this only needs a blend group built by
    /// hand (no `amalith-commands::Editor` involved) to prove it.
    #[test]
    fn clicking_either_blend_endpoint_selects_the_blend_group_without_isolating() {
        let mut doc = Document::new("blend-select");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let group = ObjectId::new();
        let start = ObjectId::new();
        let end = ObjectId::new();
        doc.insert_object(
            Object::new(
                group,
                ObjectParent::Layer(layer),
                ObjectKind::Group(GroupData {
                    children: Vec::new(),
                    clip: None,
                    blend: Some(amalith_core::BlendData {
                        start,
                        end,
                        spine: None,
                        spacing: amalith_core::BlendSpacing::SmoothColor,
                        spine_reversed: false,
                        stack_reversed: false,
                    }),
                }),
            ),
            0,
        )
        .unwrap();
        doc.insert_object(rect_path(start, ObjectParent::Group(group), amalith_core::geom::Rect::new(0., 0., 20., 20.), false), 0)
            .unwrap();
        doc.insert_object(rect_path(end, ObjectParent::Group(group), amalith_core::geom::Rect::new(100., 0., 120., 20.), false), 1)
            .unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);

        assert_eq!(
            topmost_selectable_at(&doc, Point::new(10., 10.), visible, 0.0),
            Some(group),
            "clicking shape 1 (the blend's start) must select the blend group, not nothing and not the shape itself"
        );
        assert_eq!(
            topmost_selectable_at(&doc, Point::new(110., 10.), visible, 0.0),
            Some(group),
            "clicking shape 2 (the blend's end) must select the blend group, not nothing and not the shape itself"
        );
    }

    /// Same as above, but "shape 1" is a near-flat line carrying a live
    /// Zig Zag effect on its stroke — the exact repro that used to fail
    /// (the flat *base* line's own degenerate bbox sits nowhere near
    /// where the rendered zigzag actually is, so a click on the visible
    /// squiggle used to miss the whole blend group entirely).
    #[test]
    fn clicking_a_zig_zag_effect_blend_endpoint_selects_the_blend_group() {
        let mut doc = Document::new("blend-select-zigzag");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let group = ObjectId::new();
        let start = ObjectId::new();
        let end = ObjectId::new();
        doc.insert_object(
            Object::new(
                group,
                ObjectParent::Layer(layer),
                ObjectKind::Group(GroupData {
                    children: Vec::new(),
                    clip: None,
                    blend: Some(amalith_core::BlendData {
                        start,
                        end,
                        spine: None,
                        spacing: amalith_core::BlendSpacing::SmoothColor,
                        spine_reversed: false,
                        stack_reversed: false,
                    }),
                }),
            ),
            0,
        )
        .unwrap();
        let mut geometry = vello::kurbo::BezPath::new();
        geometry.move_to((0.0, 0.0));
        geometry.line_to((100.0, 0.0));
        let mut start_obj = Object::new(
            start,
            ObjectParent::Group(group),
            ObjectKind::Path(PathData::from_bezpath(crate::convert::bez_path_to_core(&geometry))),
        );
        start_obj.appearance.set_fill(Paint::None);
        start_obj.appearance.set_stroke_width(2.0);
        if let Some(item) = start_obj.appearance.items.iter_mut().rev().find(|i| i.is_stroke()) {
            item.effects_mut().push(amalith_core::Effect::ZigZag(amalith_core::ZigZagEffect {
                size: 20.0,
                ridges_per_segment: 4.0,
                smooth: false,
            }));
        }
        doc.insert_object(start_obj, 0).unwrap();
        doc.insert_object(rect_path(end, ObjectParent::Group(group), amalith_core::geom::Rect::new(200., 0., 220., 20.), false), 1)
            .unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);

        let peak = object_contour(&doc, start)
            .expect("path has a contour")
            .elements()
            .iter()
            .filter_map(|el| match el {
                vello::kurbo::PathEl::MoveTo(p) | vello::kurbo::PathEl::LineTo(p) => Some(*p),
                _ => None,
            })
            .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
            .expect("zig zag produces at least one vertex");
        assert_eq!(
            topmost_selectable_at(&doc, peak, visible, 0.0),
            Some(group),
            "clicking the rendered zigzag on a blend's own endpoint must select the blend group"
        );
    }

    #[test]
    fn visible_top_level_bounds_still_includes_locked_objects() {
        let mut doc = Document::new("locked");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let id = ObjectId::new();
        doc.insert_object(
            rect_path(id, ObjectParent::Layer(layer), amalith_core::geom::Rect::new(0., 0., 10., 10.), true),
            0,
        )
        .unwrap();
        let found = visible_top_level_bounds(&doc, Rect::new(-100., -100., 100., 100.), &[]);
        assert_eq!(found.len(), 1, "a locked object is still a valid alignment target");
        assert_eq!(found[0].0, id);
    }

    #[test]
    fn bounds_within_skips_the_clip_mask_and_blend_steps_but_keeps_locked_content() {
        let mut doc = Document::new("clip");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let group = ObjectId::new();
        let mask = ObjectId::new();
        let content = ObjectId::new();
        doc.insert_object(
            Object::new(group, ObjectParent::Layer(layer), ObjectKind::Group(GroupData { clip: Some(mask), ..GroupData::default() })),
            0,
        )
        .unwrap();
        doc.insert_object(
            rect_path(mask, ObjectParent::Group(group), amalith_core::geom::Rect::new(0., 0., 5., 5.), false),
            0,
        )
        .unwrap();
        doc.insert_object(
            rect_path(content, ObjectParent::Group(group), amalith_core::geom::Rect::new(20., 20., 30., 30.), true),
            1,
        )
        .unwrap();
        let found = bounds_within(&doc, group, Rect::new(-100., -100., 100., 100.), &[]);
        assert_eq!(found.len(), 1, "the mask is excluded, the locked content is not");
        assert_eq!(found[0].0, content);
    }

    #[test]
    fn bounds_within_is_empty_for_a_non_group() {
        let mut doc = Document::new("bare");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let id = ObjectId::new();
        doc.insert_object(
            rect_path(id, ObjectParent::Layer(layer), amalith_core::geom::Rect::new(0., 0., 10., 10.), false),
            0,
        )
        .unwrap();
        assert!(bounds_within(&doc, id, Rect::new(-100., -100., 100., 100.), &[]).is_empty());
    }

    /// A click just past an unstroked filled shape's edge must miss it
    /// cleanly, not fall through to an unrelated neighbor's outline. The
    /// stroke-outline fallback used to apply a fixed minimum tolerance
    /// (>= 2 document units) regardless of whether the shape had any
    /// stroke at all, putting an invisible "sticky" halo around every
    /// filled shape's edge — wide enough that a click meant for one of
    /// several closely packed shapes (adjacent filled rectangles with a
    /// narrow gap, no strokes) could register against a neighbor instead
    /// of missing.
    #[test]
    fn topmost_selectable_at_does_not_add_a_phantom_halo_to_an_unstroked_filled_shape() {
        let mut doc = Document::new("adjacent");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let left = ObjectId::new();
        let right = ObjectId::new();
        let mut left_obj = Object::new(left, ObjectParent::Layer(layer), ObjectKind::Path(PathData::rectangle(amalith_core::geom::Rect::new(0., 0., 100., 100.))));
        left_obj.appearance.set_stroke(Paint::None);
        doc.insert_object(left_obj, 0).unwrap();
        let mut right_obj = Object::new(right, ObjectParent::Layer(layer), ObjectKind::Path(PathData::rectangle(amalith_core::geom::Rect::new(101., 0., 201., 100.))));
        right_obj.appearance.set_stroke(Paint::None);
        doc.insert_object(right_obj, 1).unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);

        // In the 1-unit gap, closer to `left`'s edge than `right`'s —
        // neither shape's real fill covers this point, and neither has a
        // stroke to click, so this must miss entirely.
        let hit = topmost_selectable_at(&doc, Point::new(100.3, 50.), visible, 0.0);
        assert_eq!(hit, None, "a click in the gap between two unstroked shapes must not snap to either one");
    }

    /// A circle's bounding box is a square that reaches well past its
    /// actual round edge — clicking in one of that square's empty
    /// corners used to hit the circle anyway (pure box test), even when
    /// a second, genuinely overlapping circle's real fill covers that
    /// exact point instead.
    #[test]
    fn topmost_selectable_at_uses_the_real_circle_not_its_bounding_box() {
        let mut doc = Document::new("circles");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let back = ObjectId::new();
        let front = ObjectId::new();
        // `front` (a circle centered at the origin, r=100) is drawn on
        // top of `back` (centered at (90,70), r=90) — but the click
        // point sits in `front`'s empty bounding-box corner and
        // squarely inside `back`'s real circle.
        doc.insert_object(
            Object::new(back, ObjectParent::Layer(layer), ObjectKind::Path(PathData::ellipse(amalith_core::geom::Rect::new(0., -20., 180., 160.)))),
            0,
        )
        .unwrap();
        doc.insert_object(
            Object::new(front, ObjectParent::Layer(layer), ObjectKind::Path(PathData::ellipse(amalith_core::geom::Rect::new(-100., -100., 100., 100.)))),
            1,
        )
        .unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);
        let hit = topmost_selectable_at(&doc, Point::new(95., 65.), visible, 0.0);
        assert_eq!(hit, Some(back), "the click is really inside the back circle, not front's empty corner");
    }

    /// A stroke-only path (no fill) has no "inside" for the real-outline
    /// fill test to check at all — clicking squarely on its visible
    /// stroke line has to fall back to a stroke-distance test instead of
    /// requiring (impossible) fill containment.
    #[test]
    fn topmost_selectable_at_hits_an_unfilled_paths_stroke() {
        let mut doc = Document::new("unfilled");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let id = ObjectId::new();
        let mut obj = Object::new(id, ObjectParent::Layer(layer), ObjectKind::Path(PathData::rectangle(amalith_core::geom::Rect::new(0., 0., 100., 100.))));
        obj.appearance.set_fill(Paint::None);
        obj.appearance.set_stroke_width(4.0);
        doc.insert_object(obj, 0).unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);

        // On the top edge's stroke: a hit.
        let on_stroke = topmost_selectable_at(&doc, Point::new(50., 0.), visible, 0.0);
        assert_eq!(on_stroke, Some(id), "clicking right on the unfilled rect's stroke should select it");

        // Deep in the "fill" area, which doesn't exist: no hit at all —
        // not the old bounding-box behavior, and not a false stroke hit.
        let in_middle = topmost_selectable_at(&doc, Point::new(50., 50.), visible, 0.0);
        assert_eq!(in_middle, None, "an unfilled shape's empty middle isn't clickable");
    }

    /// A perfectly straight (here: vertical) unfilled stroke path has a
    /// bounding box that's zero-width along one axis — `x0 == x1` — since
    /// `bounds_of` is just the raw geometry's box, with no stroke width
    /// added. The real bug behind "can't click on paths at all": gating
    /// the stroke-distance test on `b.contains(point)` first meant almost
    /// no click (only one landing on that exact float x) could ever pass,
    /// even squarely on the visible line.
    #[test]
    fn topmost_selectable_at_hits_a_straight_lines_degenerate_bbox() {
        let mut doc = Document::new("hairline");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let id = ObjectId::new();
        let mut geometry = vello::kurbo::BezPath::new();
        geometry.move_to((40.0, -80.0));
        geometry.line_to((40.0, -5.0));
        let mut obj = Object::new(
            id,
            ObjectParent::Layer(layer),
            ObjectKind::Path(PathData::from_bezpath(crate::convert::bez_path_to_core(&geometry))),
        );
        obj.appearance.set_fill(Paint::None);
        obj.appearance.set_stroke_width(4.0);
        doc.insert_object(obj, 0).unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);

        // A couple of pixels off the line, well within its stroke's
        // clickable tolerance, but nowhere near `x == 40.0` exactly.
        let hit = topmost_selectable_at(&doc, Point::new(41.5, -50.0), visible, 0.0);
        assert_eq!(hit, Some(id), "a click near a hairline stroke should hit it, not require exact bbox containment");
    }

    /// A thin (1pt) stroke's own exact half-width alone is a very small
    /// target — `tol` is the extra grab slop meant to fix that: a click
    /// just outside the stroke's literal geometry, but still a
    /// reasonable "I meant to click this line" distance away, has to
    /// hit with slop and miss without it.
    #[test]
    fn topmost_selectable_at_extra_tolerance_forgives_a_near_miss_on_a_thin_stroke() {
        let mut doc = Document::new("thin-stroke-slop");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let id = ObjectId::new();
        let mut geometry = vello::kurbo::BezPath::new();
        geometry.move_to((0.0, 0.0));
        geometry.line_to((100.0, 0.0));
        let mut obj = Object::new(id, ObjectParent::Layer(layer), ObjectKind::Path(PathData::from_bezpath(crate::convert::bez_path_to_core(&geometry))));
        obj.appearance.set_fill(Paint::None);
        obj.appearance.set_stroke_width(1.0);
        doc.insert_object(obj, 0).unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);

        // 2 units off a 1pt (0.5 half-width) line: outside the stroke's
        // own geometry, but within a reasonable grab tolerance.
        let near_miss = Point::new(50.0, 2.0);
        assert_eq!(
            topmost_selectable_at(&doc, near_miss, visible, 0.0),
            None,
            "with no extra tolerance, missing a thin stroke by 2 units should still miss"
        );
        assert_eq!(
            topmost_selectable_at(&doc, near_miss, visible, DEFAULT_CLICK_TOLERANCE),
            Some(id),
            "the same near-miss should hit once extra grab tolerance is applied"
        );
    }

    /// A straight line can carry a real (non-`None`) fill color — e.g. the
    /// default fill a new path inherits from tool state — while still
    /// being geometrically degenerate (zero area). The fill/winding test
    /// can never find such a shape's "inside", so relying on it alone
    /// (as the unfilled/filled either-or branch used to) left this exact
    /// shape permanently unclickable despite rendering a visible line;
    /// the fix has to fall back to the stroke test regardless of fill.
    #[test]
    fn topmost_selectable_at_hits_a_degenerate_but_filled_lines_stroke() {
        let mut doc = Document::new("filled-hairline");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let id = ObjectId::new();
        let mut geometry = vello::kurbo::BezPath::new();
        geometry.move_to((-5.5, -81.4));
        geometry.line_to((-5.5, -12.7));
        let mut obj = Object::new(
            id,
            ObjectParent::Layer(layer),
            ObjectKind::Path(PathData::from_bezpath(crate::convert::bez_path_to_core(&geometry))),
        );
        obj.appearance.set_stroke_width(4.0);
        // `fill` defaults to a real color (not `Paint::None`) on a fresh
        // `Appearance`, matching a newly drawn path's inherited tool state.
        doc.insert_object(obj, 0).unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);

        let hit = topmost_selectable_at(&doc, Point::new(-3.5, -30.0), visible, 0.0);
        assert_eq!(hit, Some(id), "a filled-but-zero-area line must still hit via its stroke");
    }

    /// Most Pen-tool paths stay open (no explicit close) unless the user
    /// clicks back on the start anchor, but a fill still renders across
    /// the implicitly-closed contour — so clicking dead center of one has
    /// to hit, not just clicking is bounding box or its (nonexistent)
    /// `ClosePath` edge.
    #[test]
    fn topmost_selectable_at_hits_an_open_but_filled_paths_interior() {
        let mut doc = Document::new("open-filled");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let id = ObjectId::new();
        let mut geometry = vello::kurbo::BezPath::new();
        geometry.move_to((0.0, 0.0));
        geometry.line_to((100.0, 0.0));
        geometry.line_to((100.0, 100.0));
        geometry.line_to((0.0, 100.0));
        // Deliberately no `close_path()` — an open subpath, like an
        // unfinished Pen-tool path.
        let obj = Object::new(id, ObjectParent::Layer(layer), ObjectKind::Path(PathData::from_bezpath(crate::convert::bez_path_to_core(&geometry))));
        doc.insert_object(obj, 0).unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);
        let hit = topmost_selectable_at(&doc, Point::new(50., 50.), visible, 0.0);
        assert_eq!(hit, Some(id), "an open path's fill still covers its interior");
    }

    /// A sparse group (e.g. a radial "starburst" of thin spokes) has a
    /// bounding box that's mostly empty space — clicking in that empty
    /// space, well inside the box but nowhere near any actual spoke, must
    /// deselect rather than grab the group, matching a click on visibly
    /// empty canvas anywhere else.
    #[test]
    fn topmost_selectable_at_does_not_treat_a_sparse_groups_whole_bbox_as_clickable() {
        let mut doc = Document::new("starburst");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let group = ObjectId::new();
        doc.insert_object(
            Object::new(group, ObjectParent::Layer(layer), ObjectKind::Group(GroupData::default())),
            0,
        )
        .unwrap();
        // Two thin spokes near the left and right edges of the group's
        // bounding box, leaving its whole center empty.
        let left = ObjectId::new();
        let mut left_obj = Object::new(left, ObjectParent::Group(group), ObjectKind::Path(PathData::rectangle(amalith_core::geom::Rect::new(0., 0., 4., 100.))));
        left_obj.appearance.set_stroke(Paint::None);
        doc.insert_object(left_obj, 0).unwrap();
        let right = ObjectId::new();
        let mut right_obj = Object::new(right, ObjectParent::Group(group), ObjectKind::Path(PathData::rectangle(amalith_core::geom::Rect::new(196., 0., 200., 100.))));
        right_obj.appearance.set_stroke(Paint::None);
        doc.insert_object(right_obj, 1).unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);

        // Dead center of the group's overall bounding box — empty canvas.
        let hit = topmost_selectable_at(&doc, Point::new(100., 50.), visible, 0.0);
        assert_eq!(hit, None, "clicking the empty middle of a sparse group's bbox must not select the group");

        // On one of the actual spokes: still hits the group.
        let hit_spoke = topmost_selectable_at(&doc, Point::new(2., 50.), visible, 0.0);
        assert_eq!(hit_spoke, Some(group), "clicking an actual spoke still selects the group");
    }

    /// A near-flat line with a live Zig Zag effect on its stroke has a
    /// degenerate (near-zero-height) *base* bounding box, but the
    /// rendered zigzag has real vertical extent — clicking (or entering
    /// isolation mode on) the actual visible squiggle must not fall back
    /// to hit-testing the invisible flat line underneath.
    #[test]
    fn bounds_and_hit_testing_reflect_a_live_zig_zag_effect_on_the_stroke() {
        let mut doc = Document::new("zigzag");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let id = ObjectId::new();
        let mut geometry = vello::kurbo::BezPath::new();
        geometry.move_to((0.0, 0.0));
        geometry.line_to((100.0, 0.0));
        let mut obj = Object::new(
            id,
            ObjectParent::Layer(layer),
            ObjectKind::Path(PathData::from_bezpath(crate::convert::bez_path_to_core(&geometry))),
        );
        obj.appearance.set_fill(Paint::None);
        obj.appearance.set_stroke_width(2.0);
        if let Some(item) = obj.appearance.items.iter_mut().rev().find(|i| i.is_stroke()) {
            item.effects_mut().push(amalith_core::Effect::ZigZag(amalith_core::ZigZagEffect {
                size: 20.0,
                ridges_per_segment: 4.0,
                smooth: false,
            }));
        }
        doc.insert_object(obj, 0).unwrap();

        let b = bounds(&doc, id).expect("effect-grown bounds");
        assert!(
            b.height() > 10.0,
            "bounds should grow to include the rendered zigzag, not the flat base line, got height {}",
            b.height()
        );

        // Click directly on an actual zigzag peak vertex (a real point on
        // the rendered contour, wherever the algorithm put it) — this has
        // to hit, unlike testing against the base line's own geometry.
        let contour = object_contour(&doc, id).expect("path has a contour");
        let peak = contour
            .elements()
            .iter()
            .filter_map(|el| match el {
                vello::kurbo::PathEl::MoveTo(p) | vello::kurbo::PathEl::LineTo(p) => Some(*p),
                _ => None,
            })
            .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
            .expect("zig zag produces at least one vertex");
        assert!(peak.y > 10.0, "expected an actual zigzag peak well off the flat line, got {peak:?}");
        let visible = Rect::new(-1000., -1000., 1000., 1000.);
        let hit = topmost_selectable_at(&doc, peak, visible, 0.0);
        assert_eq!(hit, Some(id), "clicking a real zigzag peak vertex should hit the stroke");

        // The plain underlying path (what the hover/selection outline
        // now traces — see `base_contour`) is still a valid click
        // target too: the effect is non-destructive, so the real path
        // is still "the object," not just wherever its live effect
        // happens to have moved the painted result.
        let base_point = Point::new(50.0, 0.0);
        let min_dist_to_rendered = contour
            .segments()
            .map(|seg: PathSeg| seg.nearest(base_point, 0.1).distance_sq)
            .fold(f64::INFINITY, f64::min)
            .sqrt();
        assert!(
            min_dist_to_rendered > 2.0,
            "test setup: the base line point must not already coincide with the rendered zigzag, got distance {min_dist_to_rendered}"
        );
        assert_eq!(
            topmost_selectable_at(&doc, base_point, visible, 0.0),
            Some(id),
            "clicking the plain underlying path should still hit, even where the zigzag has moved the rendered stroke away from it"
        );
    }

    /// A Fill item and a Stroke item can carry *independent* live effect
    /// stacks on the very same object — a big inset Offset Path on the
    /// fill, no effect at all on the stroke. Hit-testing has to honor
    /// each item's own shape rather than picking one "representative"
    /// contour for the whole object (which would either miss the
    /// stroke's un-shrunk line, or wrongly treat the shrunk-away middle
    /// ring as still-filled).
    #[test]
    fn fill_and_stroke_items_are_hit_tested_against_their_own_independent_effects() {
        use amalith_core::{Color, Effect, LineJoin, OffsetEffect, Paint};

        let mut doc = Document::new("independent-effects");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let id = ObjectId::new();
        let mut obj = Object::new(
            id,
            ObjectParent::Layer(layer),
            ObjectKind::Path(PathData::rectangle(amalith_core::geom::Rect::new(0., 0., 100., 100.))),
        );
        obj.appearance.set_fill(Paint::Solid(Color::rgb(0.2, 0.2, 0.2)));
        obj.appearance.set_stroke(Paint::Solid(Color::rgb(0.2, 0.2, 0.2)));
        obj.appearance.set_stroke_width(2.0);
        // Inset the fill by 30 — its own effect stack only, the stroke's
        // stays empty and keeps drawing at the original 100x100 edges.
        if let Some(item) = obj.appearance.items.iter_mut().rev().find(|i| i.is_fill()) {
            item.effects_mut().push(Effect::Offset(OffsetEffect {
                amount: -30.0,
                join: LineJoin::Miter,
                miter_limit: 4.0,
            }));
        }
        doc.insert_object(obj, 0).unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);

        // Dead center: still well inside the inset fill (shrunk to
        // roughly [30,70]x[30,70]) — hits via fill.
        assert_eq!(
            topmost_selectable_at(&doc, Point::new(50., 50.), visible, 0.0),
            Some(id),
            "the shrunk fill still covers its own new middle"
        );

        // Just inside the *original* square's corner, in the ring the
        // fill shrank away from and nowhere near the stroke's own
        // (un-shrunk) line at x=0/x=100/y=0/y=100 — must miss entirely.
        assert_eq!(
            topmost_selectable_at(&doc, Point::new(10., 10.), visible, 0.0),
            None,
            "the ring the inset fill vacated isn't filled, and isn't on the stroke's own un-shrunk line either"
        );

        // Right on the stroke's original left edge — the stroke has no
        // effect of its own, so it must still hit there even though the
        // fill (a different item, different effect stack) no longer
        // reaches this point at all.
        assert_eq!(
            topmost_selectable_at(&doc, Point::new(0., 50.), visible, 0.0),
            Some(id),
            "the stroke's own un-shrunk edge must still hit on its own terms"
        );
    }

    /// Object Highlighting has to light up hovering anywhere inside a
    /// filled shape's interior, not just within a few pixels of its
    /// outline — a real, reported gap: the original implementation only
    /// ever tested proximity to the contour's own segments (mirroring
    /// `anchors::segment_at`, whose actual job — Anchor/Path Labels and
    /// segment-click-to-insert-an-anchor — really is outline-only), so
    /// hovering dead center of an ordinary filled rectangle never lit up
    /// the blue highlight at all.
    #[test]
    fn nearest_painted_leaf_hits_a_filled_shapes_interior_not_just_its_outline() {
        let mut doc = Document::new("hover-fill-interior");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let id = ObjectId::new();
        let obj = rect_path(id, ObjectParent::Layer(layer), amalith_core::geom::Rect::new(0., 0., 100., 100.), false);
        doc.insert_object(obj, 0).unwrap();

        // Dead center — nowhere near the outline (at least 50 units from
        // any edge), but squarely inside the fill.
        assert_eq!(
            nearest_painted_leaf(&doc, &[id], Point::new(50., 50.), 4.0),
            Some(id),
            "hovering a filled shape's own interior should light up Object Highlighting"
        );
    }

    /// Object Highlighting (the hover outline) has to find a blend
    /// endpoint's *rendered* Zig Zag, not its flat base line — the same
    /// class of bug `topmost_selectable_at` had, in the separate code
    /// path Smart Guides' hover uses.
    #[test]
    fn nearest_painted_leaf_finds_a_zig_zag_effects_rendered_peak_not_its_base_line() {
        let mut doc = Document::new("hover-zigzag");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let id = ObjectId::new();
        let mut geometry = vello::kurbo::BezPath::new();
        geometry.move_to((0.0, 0.0));
        geometry.line_to((100.0, 0.0));
        let mut obj = Object::new(id, ObjectParent::Layer(layer), ObjectKind::Path(PathData::from_bezpath(crate::convert::bez_path_to_core(&geometry))));
        obj.appearance.set_fill(Paint::None);
        obj.appearance.set_stroke_width(2.0);
        if let Some(item) = obj.appearance.items.iter_mut().rev().find(|i| i.is_stroke()) {
            item.effects_mut().push(amalith_core::Effect::ZigZag(amalith_core::ZigZagEffect {
                size: 20.0,
                ridges_per_segment: 4.0,
                smooth: false,
            }));
        }
        doc.insert_object(obj, 0).unwrap();

        let peak = object_contour(&doc, id)
            .expect("path has a contour")
            .elements()
            .iter()
            .filter_map(|el| match el {
                vello::kurbo::PathEl::MoveTo(p) | vello::kurbo::PathEl::LineTo(p) => Some(*p),
                _ => None,
            })
            .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
            .expect("zig zag produces at least one vertex");

        assert_eq!(
            nearest_painted_leaf(&doc, &[id], peak, 1.0),
            Some(id),
            "hovering the rendered zigzag peak should light up Object Highlighting"
        );
        // Nowhere near the rendered zigzag at all (well past either end
        // of the 100-unit line, off to the side) must not light up.
        assert_eq!(
            nearest_painted_leaf(&doc, &[id], Point::new(500.0, 500.0), 1.0),
            None,
            "hovering somewhere with no rendered content nearby should not hit"
        );
    }

    /// Same as above, but the zigzag-effect shape is one of a blend's
    /// own two originals (nested under `ObjectParent::Group`, exactly
    /// what a real blend looks like), and found the same way Object
    /// Highlighting's real caller does: via `anchors::path_leaves`
    /// first, not a hand-picked `&[id]` slice.
    #[test]
    fn nearest_painted_leaf_finds_a_zig_zag_blend_endpoint_nested_in_its_group() {
        let mut doc = Document::new("hover-zigzag-blend");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let group = ObjectId::new();
        let start = ObjectId::new();
        let end = ObjectId::new();
        doc.insert_object(
            Object::new(
                group,
                ObjectParent::Layer(layer),
                ObjectKind::Group(GroupData {
                    children: Vec::new(),
                    clip: None,
                    blend: Some(amalith_core::BlendData {
                        start,
                        end,
                        spine: None,
                        spacing: amalith_core::BlendSpacing::SmoothColor,
                        spine_reversed: false,
                        stack_reversed: false,
                    }),
                }),
            ),
            0,
        )
        .unwrap();
        let mut geometry = vello::kurbo::BezPath::new();
        geometry.move_to((0.0, 0.0));
        geometry.line_to((100.0, 0.0));
        let mut start_obj = Object::new(
            start,
            ObjectParent::Group(group),
            ObjectKind::Path(PathData::from_bezpath(crate::convert::bez_path_to_core(&geometry))),
        );
        start_obj.appearance.set_fill(Paint::None);
        start_obj.appearance.set_stroke_width(2.0);
        if let Some(item) = start_obj.appearance.items.iter_mut().rev().find(|i| i.is_stroke()) {
            item.effects_mut().push(amalith_core::Effect::ZigZag(amalith_core::ZigZagEffect {
                size: 20.0,
                ridges_per_segment: 4.0,
                smooth: false,
            }));
        }
        doc.insert_object(start_obj, 0).unwrap();
        doc.insert_object(rect_path(end, ObjectParent::Group(group), amalith_core::geom::Rect::new(200., 0., 220., 20.), false), 1)
            .unwrap();

        let peak = object_contour(&doc, start)
            .expect("path has a contour")
            .elements()
            .iter()
            .filter_map(|el| match el {
                vello::kurbo::PathEl::MoveTo(p) | vello::kurbo::PathEl::LineTo(p) => Some(*p),
                _ => None,
            })
            .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
            .expect("zig zag produces at least one vertex");

        let ids = crate::anchors::path_leaves(&doc);
        assert!(ids.contains(&start), "path_leaves should still reach a blend's nested originals");
        assert_eq!(
            nearest_painted_leaf(&doc, &ids, peak, 1.0),
            Some(start),
            "hovering a blend endpoint's rendered zigzag should light up Object Highlighting"
        );
    }

    /// A blend's generated in-between steps are computed geometry, not
    /// independently selectable objects — `is_blend_step` has to flag
    /// them (and only them) so Object Highlighting's hover excludes
    /// them the same way click-selection already does once isolated
    /// (`topmost_in`). Hovering an actual generated step must find
    /// nothing at all, not the step itself.
    #[test]
    fn is_blend_step_flags_only_the_generated_middle_child_not_start_or_end() {
        let mut doc = Document::new("blend-step-flag");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let group = ObjectId::new();
        let start = ObjectId::new();
        let step = ObjectId::new();
        let end = ObjectId::new();
        doc.insert_object(
            Object::new(
                group,
                ObjectParent::Layer(layer),
                ObjectKind::Group(GroupData {
                    children: Vec::new(),
                    clip: None,
                    blend: Some(amalith_core::BlendData {
                        start,
                        end,
                        spine: None,
                        spacing: amalith_core::BlendSpacing::SmoothColor,
                        spine_reversed: false,
                        stack_reversed: false,
                    }),
                }),
            ),
            0,
        )
        .unwrap();
        doc.insert_object(rect_path(start, ObjectParent::Group(group), amalith_core::geom::Rect::new(0., 0., 20., 20.), false), 0)
            .unwrap();
        doc.insert_object(rect_path(step, ObjectParent::Group(group), amalith_core::geom::Rect::new(100., 0., 120., 20.), false), 1)
            .unwrap();
        doc.insert_object(rect_path(end, ObjectParent::Group(group), amalith_core::geom::Rect::new(200., 0., 220., 20.), false), 2)
            .unwrap();

        assert!(!is_blend_step(&doc, start), "the blend's own start must not be flagged as a step");
        assert!(is_blend_step(&doc, step), "a generated in-between child must be flagged as a step");
        assert!(!is_blend_step(&doc, end), "the blend's own end must not be flagged as a step");

        // Mirrors `sg_hovered_path_at`'s own filtering.
        let ids: Vec<ObjectId> =
            crate::anchors::path_leaves(&doc).into_iter().filter(|&id| !is_blend_step(&doc, id)).collect();
        assert_eq!(
            nearest_painted_leaf(&doc, &ids, Point::new(110., 10.), 4.0),
            None,
            "hovering a generated blend step must not highlight it — it isn't an independent object"
        );
        assert_eq!(
            nearest_painted_leaf(&doc, &ids, Point::new(10., 10.), 4.0),
            Some(start),
            "the blend's own start must still highlight normally"
        );
    }
}
