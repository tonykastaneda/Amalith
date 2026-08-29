//! Document hit-testing and object bounds, in document space (vello kurbo).

use amalith_core::{Document, ObjectId, ObjectKind};
use vello::kurbo::{Point, Rect};

use crate::convert;

/// Axis-aligned document-space bounds of `id`: its own transform applied,
/// groups unioning their (already-transformed) children.
pub fn object_bbox(doc: &Document, id: ObjectId) -> Option<Rect> {
    let obj = doc.object(id)?;
    let m = convert::affine(obj.transform);

    if let ObjectKind::Group(g) = &obj.kind {
        let mut acc: Option<Rect> = None;
        for &child in &g.children {
            if let Some(b) = object_bbox(doc, child) {
                acc = Some(acc.map_or(b, |a| a.union(b)));
            }
        }
        return acc.map(|b| m.transform_rect_bbox(b));
    }

    let local = match &obj.kind {
        ObjectKind::Path(pd) => Some(convert::rect(pd.local_bounds())),
        ObjectKind::CompoundPath(cp) => cp.local_bounds().map(convert::rect),
        other => other.own_local_bounds().map(convert::rect),
    };
    local.map(|b| m.transform_rect_bbox(b))
}

/// Frontmost object whose bounds contain `doc_point` (paint order is
/// back-to-front, so the last child of the last layer wins).
pub fn hit(doc: &Document, doc_point: Point) -> Option<ObjectId> {
    for layer in doc.layers().iter().rev() {
        if !layer.visible {
            continue;
        }
        for &id in layer.children.iter().rev() {
            if object_bbox(doc, id).is_some_and(|b| b.contains(doc_point)) {
                return Some(id);
            }
        }
    }
    None
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

/// Every visible object whose bounds overlap `doc_rect`.
pub fn within(doc: &Document, doc_rect: Rect) -> Vec<ObjectId> {
    let mut out = Vec::new();
    for layer in doc.layers() {
        if !layer.visible {
            continue;
        }
        for &id in &layer.children {
            if object_bbox(doc, id).is_some_and(|b| overlaps(doc_rect, b)) {
                out.push(id);
            }
        }
    }
    out
}
