//! Document hit-testing for the selection tool, ported from
//! `amalith-app`'s `topmost_selectable_at` / marquee logic.
//!
//! Everything is bounds-based (document-space AABBs from
//! [`Document::bounds_of`], which already unions a group's descendants) and
//! culled to the visible rect, so a click selects the whole group and
//! never reaches an off-screen object. Coordinates are vello kurbo.

use amalith_core::{Document, ObjectId, ObjectKind, ObjectParent};
use vello::kurbo::{Affine, ParamCurveNearest, PathSeg, Point, Rect};

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
/// `visible`. Layer direct children only — a `Group` is selected as a unit.
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
            if let Some(b) = bounds(doc, id) {
                if overlaps(b, visible) && b.contains(point) {
                    return Some(id);
                }
            }
        }
    }
    None
}

/// `(id, bounds)` for every visible, unlocked, layer-direct-child object
/// overlapping `visible` — Smart Guides' Alignment Guides candidate list.
/// Same walk as `topmost_selectable_at`, minus `excluding` (the object(s)
/// currently being dragged, which shouldn't snap to their own bounds).
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
            if !obj.visible || obj.locked {
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
