//! Path-anchor queries for the Direct Selection tool. Ported from
//! `amalith-app`'s direct_selection.rs. Coordinates are vello kurbo unless
//! noted.

use amalith_core::{geom, Document, HandleSide, ObjectId, ObjectKind, ObjectParent};
use vello::kurbo::{Affine, ParamCurveNearest, Point, Rect};

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

/// `(flat anchor ordinal, document-space position)` for every anchor of
/// `id`, in subpath walk order.
pub fn anchors_of(doc: &Document, id: ObjectId) -> Vec<(usize, Point)> {
    let Some(obj) = doc.object(id) else {
        return Vec::new();
    };
    let ObjectKind::Path(pd) = &obj.kind else {
        return Vec::new();
    };
    let m = convert::affine(doc.world_transform(id));
    pd.subpaths()
        .iter()
        .flat_map(|s| s.anchors.iter())
        .enumerate()
        .map(|(n, a)| (n, m * convert::point(a.point)))
        .collect()
}

/// `(anchor ordinal, side, document-space handle position)` for every
/// present bezier handle of `id`.
pub fn handles_of(doc: &Document, id: ObjectId) -> Vec<(usize, HandleSide, Point)> {
    let Some(obj) = doc.object(id) else {
        return Vec::new();
    };
    let ObjectKind::Path(pd) = &obj.kind else {
        return Vec::new();
    };
    let m = convert::affine(doc.world_transform(id));
    let mut out = Vec::new();
    for (n, a) in pd.subpaths().iter().flat_map(|s| s.anchors.iter()).enumerate() {
        if let Some(h) = a.handle_in {
            out.push((n, HandleSide::In, m * convert::point(h)));
        }
        if let Some(h) = a.handle_out {
            out.push((n, HandleSide::Out, m * convert::point(h)));
        }
    }
    out
}

/// Topmost handle within `radius` doc units of `p`, restricted to `ids`.
pub fn handle_at(
    doc: &Document,
    ids: &[ObjectId],
    p: Point,
    radius: f64,
) -> Option<(ObjectId, usize, HandleSide)> {
    let r2 = radius * radius;
    for &id in ids.iter().rev() {
        let best = handles_of(doc, id)
            .into_iter()
            .filter_map(|(n, side, hp)| {
                let d2 = (hp - p).hypot2();
                (d2 <= r2).then_some((n, side, d2))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
        if let Some((n, side, _)) = best {
            return Some((id, n, side));
        }
    }
    None
}

/// Topmost path segment within `radius` doc units of `p`, restricted to
/// `ids`. Returns `(id, flat segment ordinal, t)` where `t` in `0..=1`
/// locates the closest point along that segment.
pub fn segment_at(
    doc: &Document,
    ids: &[ObjectId],
    p: Point,
    radius: f64,
) -> Option<(ObjectId, usize, f64)> {
    let r2 = radius * radius;
    for &id in ids.iter().rev() {
        let Some(obj) = doc.object(id) else { continue };
        let ObjectKind::Path(pd) = &obj.kind else {
            continue;
        };
        let m: Affine = convert::affine(doc.world_transform(id));
        let g = m * convert::bez_path(&pd.geometry);
        let best = g
            .segments()
            .enumerate()
            .map(|(i, seg)| {
                let near = seg.nearest(p, 0.05);
                (i, near.distance_sq, near.t)
            })
            .filter(|&(_, d2, _)| d2 <= r2)
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        if let Some((i, _, t)) = best {
            return Some((id, i, t));
        }
    }
    None
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

/// `pd`'s geometry with the given anchor `ordinals` translated by `delta`
/// (local space) — for the live drag preview. Core kurbo (the caller
/// converts).
pub fn deformed(
    pd: &amalith_core::PathData,
    ordinals: &[usize],
    delta: amalith_core::Vec2,
) -> geom::BezPath {
    let mut sp = pd.subpaths().to_vec();
    for &n in ordinals {
        amalith_core::translate_anchor_n(&mut sp, n, delta);
    }
    amalith_core::subpaths_to_bezpath(&sp)
}

/// `pd` with anchor `n`'s `side` handle nudged by `delta` (local space),
/// its partner kept mirrored per the anchor's mode — the live preview for
/// a handle drag.
pub fn deformed_handle(
    pd: &amalith_core::PathData,
    n: usize,
    side: HandleSide,
    delta: amalith_core::Vec2,
) -> amalith_core::PathData {
    let mut sp = pd.subpaths().to_vec();
    let cur = amalith_core::anchor_at(&sp, n).and_then(|a| match side {
        HandleSide::In => a.handle_in,
        HandleSide::Out => a.handle_out,
    });
    if let Some(c) = cur {
        let moved = amalith_core::geom::Point::new(c.x + delta.x, c.y + delta.y);
        amalith_core::set_handle(&mut sp, n, side, Some(moved));
    }
    amalith_core::PathData::from_subpaths(sp)
}
