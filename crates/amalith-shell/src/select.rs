//! Document hit-testing for the selection tool, ported from
//! `amalith-app`'s `topmost_selectable_at` / marquee logic.
//!
//! Everything is bounds-based (document-space AABBs from
//! [`Document::bounds_of`], which already unions a group's descendants) and
//! culled to the visible rect, so a click selects the whole group and
//! never reaches an off-screen object. Coordinates are vello kurbo.

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

/// Document-space bounds of `id` (vello kurbo), or `None` if it has none.
/// For a clip group this is the mask shape's bounds, not the union of the
/// (partly hidden) contents.
pub fn bounds(doc: &Document, id: ObjectId) -> Option<Rect> {
    let target = clip_target(doc, id).unwrap_or(id);
    doc.bounds_of(target).map(convert::rect)
}

/// Frontmost layer-child whose bounds contain `point` and overlap
/// `visible`. Layer direct children only — a `Group` is selected as a unit
/// (its bounding box stands in for it, same as ever — clicking a gap
/// between its children still hits the group). A plain path or compound
/// path additionally has to have `point` inside its *real* outline, not
/// just its circumscribing box — a circle's own bounding box has empty
/// corners that read as "inside" under a pure box test, which used to let
/// a click there wrongly hit that circle instead of whatever (or nothing)
/// is actually drawn there.
pub fn topmost_selectable_at(doc: &Document, point: Point, visible: Rect) -> Option<ObjectId> {
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
            if matches!(obj.kind, ObjectKind::Path(_) | ObjectKind::CompoundPath(_)) {
                // A click hits a path if it's inside a real fill, OR near
                // the visible stroke line — same as Illustrator, where a
                // filled shape's outline is just as clickable as its
                // interior. The outline test isn't just for unfilled
                // shapes: a geometrically degenerate path (near-zero
                // area — e.g. a perfectly straight line, open or closed)
                // has no real "inside" for the fill/winding test to ever
                // find true, filled or not, so it would otherwise be
                // permanently unclickable despite rendering a visible
                // line. Paths also skip the `b.contains(point)` box
                // pre-filter every other object kind uses below — a
                // straight line's bounding box is zero-width along one
                // axis (e.g. `x0 == x1`), which would reject all but a
                // click landing on that exact float coordinate.
                let filled = obj.appearance.fill != amalith_core::Paint::None
                    && point_in_fill(doc, id, point);
                // The outline fallback only applies when there's an
                // actual visible stroke to click — gating it on a fixed
                // minimum tolerance instead (regardless of whether the
                // object even has a stroke) put an invisible ~2-unit
                // "sticky" halo around every filled shape's edge, wide
                // enough that a click meant for one of several closely
                // packed shapes could register against its unstroked
                // neighbor's outline instead of missing cleanly.
                let has_stroke = obj.appearance.stroke != amalith_core::Paint::None
                    && obj.appearance.stroke_width > 0.0;
                if !filled
                    && (!has_stroke
                        || !near_contour(doc, id, point, obj.appearance.stroke_width * 0.5))
                {
                    continue;
                }
            } else if !b.contains(point) {
                continue;
            }
            return Some(id);
        }
    }
    None
}

/// Real point-in-fill test for a path / compound path's own outline
/// (world space), using the nonzero winding rule — the same rule the
/// renderer fills with.
///
/// An open subpath (no trailing `ClosePath`) is still filled by the
/// renderer — `canvas.rs`'s `paint_path` fills it with an implicitly
/// closed contour, same as Illustrator/SVG, while only the *stroke*
/// stays open. `kurbo::BezPath::winding` has no such implicit-close
/// behavior (it only treats a subpath as closed when it actually ends
/// in `ClosePath`), so most Pen-tool paths — left open unless the user
/// explicitly clicks back on the start anchor — would otherwise wind to
/// 0 everywhere and become unclickable despite rendering filled.
/// `close_open_subpaths` restores that parity before the winding test.
fn point_in_fill(doc: &Document, id: ObjectId, point: Point) -> bool {
    object_contour(doc, id).is_some_and(|bez| close_open_subpaths(&bez).winding(point) != 0)
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

/// Frontmost direct child of `group` whose bounds contain `point` —
/// isolation-mode hit-testing, where selection is scoped to one group.
///
/// In a clip group the mask shape is skipped by the normal (bounds) pass
/// so clicks fall through to the masked content; the mask is only picked
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
        Some(_) => {
            // A bare object was isolated: only it is selectable, hit either
            // by its bounds or (for a shape) close to its contour.
            let inside = bounds(doc, group).is_some_and(|b| b.contains(point));
            return (inside || near_contour(doc, group, point, contour_tol)).then_some(group);
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
        if bounds(doc, id).is_some_and(|b| b.contains(point)) {
            return Some(id);
        }
    }
    // Only the mask's contour is clickable.
    clip.filter(|&cid| near_contour(doc, cid, point, contour_tol))
}

/// `id`'s outline as a document-space `BezPath` — paths and compound
/// paths only.
pub fn object_contour(doc: &Document, id: ObjectId) -> Option<vello::kurbo::BezPath> {
    let obj = doc.object(id)?;
    let m: Affine = convert::affine(doc.world_transform(id));
    match &obj.kind {
        ObjectKind::Path(pd) => Some(m * convert::bez_path(&pd.geometry)),
        ObjectKind::CompoundPath(cp) => {
            let mut b = vello::kurbo::BezPath::new();
            for sub in &cp.subpaths {
                b.extend(convert::bez_path(sub));
            }
            Some(m * b)
        }
        _ => None,
    }
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
        left_obj.appearance.stroke = Paint::None;
        doc.insert_object(left_obj, 0).unwrap();
        let mut right_obj = Object::new(right, ObjectParent::Layer(layer), ObjectKind::Path(PathData::rectangle(amalith_core::geom::Rect::new(101., 0., 201., 100.))));
        right_obj.appearance.stroke = Paint::None;
        doc.insert_object(right_obj, 1).unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);

        // In the 1-unit gap, closer to `left`'s edge than `right`'s —
        // neither shape's real fill covers this point, and neither has a
        // stroke to click, so this must miss entirely.
        let hit = topmost_selectable_at(&doc, Point::new(100.3, 50.), visible);
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
        let hit = topmost_selectable_at(&doc, Point::new(95., 65.), visible);
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
        obj.appearance.fill = Paint::None;
        obj.appearance.stroke_width = 4.0;
        doc.insert_object(obj, 0).unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);

        // On the top edge's stroke: a hit.
        let on_stroke = topmost_selectable_at(&doc, Point::new(50., 0.), visible);
        assert_eq!(on_stroke, Some(id), "clicking right on the unfilled rect's stroke should select it");

        // Deep in the "fill" area, which doesn't exist: no hit at all —
        // not the old bounding-box behavior, and not a false stroke hit.
        let in_middle = topmost_selectable_at(&doc, Point::new(50., 50.), visible);
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
        obj.appearance.fill = Paint::None;
        obj.appearance.stroke_width = 4.0;
        doc.insert_object(obj, 0).unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);

        // A couple of pixels off the line, well within its stroke's
        // clickable tolerance, but nowhere near `x == 40.0` exactly.
        let hit = topmost_selectable_at(&doc, Point::new(41.5, -50.0), visible);
        assert_eq!(hit, Some(id), "a click near a hairline stroke should hit it, not require exact bbox containment");
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
        obj.appearance.stroke_width = 4.0;
        // `fill` defaults to a real color (not `Paint::None`) on a fresh
        // `Appearance`, matching a newly drawn path's inherited tool state.
        doc.insert_object(obj, 0).unwrap();
        let visible = Rect::new(-1000., -1000., 1000., 1000.);

        let hit = topmost_selectable_at(&doc, Point::new(-3.5, -30.0), visible);
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
        let hit = topmost_selectable_at(&doc, Point::new(50., 50.), visible);
        assert_eq!(hit, Some(id), "an open path's fill still covers its interior");
    }
}
