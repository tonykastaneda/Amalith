//! Path-anchor queries for the Direct Selection tool. Ported from
//! `amalith-app`'s direct_selection.rs. Coordinates are vello kurbo unless
//! noted.

use amalith_core::{geom, Document, ObjectId, ObjectKind, ObjectParent};
use vello::kurbo::{Point, Rect};

use crate::convert;

/// Every leaf path id under a visible layer, in paint order (groups
/// expanded — Direct Selection reaches into groups).
pub fn path_leaves(doc: &Document) -> Vec<ObjectId> {
    fn rec(doc: &Document, parent: ObjectParent, out: &mut Vec<ObjectId>) {
        for &id in doc.children_of(parent) {
            match doc.object(id).map(|o| &o.kind) {
                Some(ObjectKind::Path(_)) => out.push(id),
                Some(ObjectKind::Group(_)) => rec(doc, ObjectParent::Group(id), out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for layer in doc.layers() {
        if layer.visible {
            rec(doc, ObjectParent::Layer(layer.id), &mut out);
        }
    }
    out
}

/// `(anchor index, document-space position)` for every anchor of `id`.
pub fn anchors_of(doc: &Document, id: ObjectId) -> Vec<(usize, Point)> {
    let Some(obj) = doc.object(id) else {
        return Vec::new();
    };
    let ObjectKind::Path(pd) = &obj.kind else {
        return Vec::new();
    };
    let m = convert::affine(doc.world_transform(id));
    geom::anchor_indices(&pd.geometry)
        .into_iter()
        .filter_map(|i| geom::anchor_position(&pd.geometry, i).map(|p| (i, m * convert::point(p))))
        .collect()
}

/// Topmost path anchor within `radius` document units of `p`.
pub fn topmost_anchor_at(doc: &Document, p: Point, radius: f64) -> Option<(ObjectId, usize)> {
    let r2 = radius * radius;
    for id in path_leaves(doc).into_iter().rev() {
        let best = anchors_of(doc, id)
            .into_iter()
            .filter_map(|(i, ap)| {
                let d2 = (ap - p).hypot2();
                (d2 <= r2).then_some((i, d2))
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        if let Some((i, _)) = best {
            return Some((id, i));
        }
    }
    None
}

/// Topmost anchor within `radius` document units of `p`, restricted to
/// `ids` (the paths whose nodes are currently on screen). Illustrator's
/// white arrow only grabs nodes of a path you've already selected.
pub fn topmost_anchor_among(
    doc: &Document,
    ids: &[ObjectId],
    p: Point,
    radius: f64,
) -> Option<(ObjectId, usize)> {
    let r2 = radius * radius;
    for &id in ids.iter().rev() {
        let best = anchors_of(doc, id)
            .into_iter()
            .filter_map(|(i, ap)| {
                let d2 = (ap - p).hypot2();
                (d2 <= r2).then_some((i, d2))
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        if let Some((i, _)) = best {
            return Some((id, i));
        }
    }
    None
}

/// Every anchor of `ids` whose position falls inside `doc_rect`.
pub fn within_of(doc: &Document, ids: &[ObjectId], doc_rect: Rect) -> Vec<(ObjectId, usize)> {
    let mut out = Vec::new();
    for &id in ids {
        for (i, ap) in anchors_of(doc, id) {
            if doc_rect.contains(ap) {
                out.push((id, i));
            }
        }
    }
    out
}

/// Every anchor whose position falls inside `doc_rect`.
pub fn within(doc: &Document, doc_rect: Rect) -> Vec<(ObjectId, usize)> {
    let mut out = Vec::new();
    for id in path_leaves(doc) {
        for (i, ap) in anchors_of(doc, id) {
            if doc_rect.contains(ap) {
                out.push((id, i));
            }
        }
    }
    out
}

/// `pd.geometry` with `indices` translated by `delta` (document space) —
/// for the live drag preview. Core kurbo (so the caller converts).
pub fn deformed(
    geometry: &geom::BezPath,
    indices: &[usize],
    delta: amalith_core::Vec2,
) -> geom::BezPath {
    let mut g = geometry.clone();
    for &i in indices {
        geom::translate_anchor(&mut g, i, delta);
    }
    g
}
