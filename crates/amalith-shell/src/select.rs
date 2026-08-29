//! Document hit-testing for the selection tool, ported from
//! `amalith-app`'s `topmost_selectable_at` / marquee logic.
//!
//! Everything is bounds-based (document-space AABBs from
//! [`Document::bounds_of`], which already unions a group's descendants) and
//! culled to the visible rect, so a click selects the whole group and
//! never reaches an off-screen object. Coordinates are vello kurbo.

use amalith_core::{Document, ObjectId, ObjectParent};
use vello::kurbo::{Point, Rect};

use crate::convert;

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

/// Document-space bounds of `id` (vello kurbo), or `None` if it has none.
pub fn bounds(doc: &Document, id: ObjectId) -> Option<Rect> {
    doc.bounds_of(id).map(convert::rect)
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
            if !obj.visible {
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

/// Layer-children whose bounds intersect `marquee` (document space).
pub fn within(doc: &Document, marquee: Rect) -> Vec<ObjectId> {
    let mut out = Vec::new();
    for layer in doc.layers() {
        if !layer.visible {
            continue;
        }
        for &id in doc.children_of(ObjectParent::Layer(layer.id)) {
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
        let id = ids[0];
        let local = convert::rect(doc.local_bounds_of(id)?);
        let m = convert::affine(doc.world_transform(id));
        return Some(crate::handles::rect_quad(local).map(|p| m * p));
    }
    union_bounds(doc, ids).map(crate::handles::rect_quad)
}
