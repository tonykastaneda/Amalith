//! Geometry primitives: affine transforms, points, vectors, rectangles.
//!
//! Amalith reuses [`kurbo`] rather than reimplementing 2geom-style affine
//! transforms and Bezier math. Kurbo covers exactly the primitives 2geom
//! provides (`Affine`, `Point`, `Vec2`, `Rect`, `BezPath`) with the same
//! "value type, not scene-graph node" philosophy Inkscape's `Geom::Affine`
//! uses, so there is no lesson from 2geom to re-derive here — just reuse.
//!
//! All coordinates in this crate are **document space** unless a doc
//! comment says otherwise (see [`crate::document`] for the coordinate
//! system writeup): a document has one global coordinate system, artboards
//! are rectangles placed within it, and object transforms compose upward
//! through parent groups to document space. There is no separate
//! "artboard-local" storage — artboard-relative coordinates are always a
//! derived view (`object position - artboard origin`), computed on demand,
//! so they can never drift out of sync with the canonical document-space
//! values.
pub use kurbo::{flatten, Affine, BezPath, PathEl, Point, Rect, Shape, Size, Vec2};

/// Bounding box of a Bezier path, in the path's own coordinate space.
///
/// Thin wrapper over `kurbo::Shape::bounding_box` so object.rs doesn't need
/// to import the `Shape` trait itself just to call one method.
pub fn bez_path_bounds(path: &BezPath) -> Rect {
    path.bounding_box()
}

/// A polyline approximation of every subpath in `path`, in `path`'s own
/// coordinate space. A free function (not just [`PathData::flattened_
/// points`](crate::object::PathData::flattened_points), which delegates
/// here) so callers with a `BezPath` that didn't come from a `PathData` —
/// e.g. a live edit preview, cloned and nudged but not yet committed —
/// can flatten it the same way, without needing a whole `PathData` to
/// wrap it in first.
pub fn flattened_points(path: &BezPath, tolerance: f64) -> Vec<Vec<Point>> {
    let mut paths: Vec<Vec<Point>> = Vec::new();
    let mut current: Option<Vec<Point>> = None;
    flatten(path, tolerance, |element| match element {
        PathEl::MoveTo(point) => {
            if let Some(points) = current.take() {
                if points.len() >= 2 {
                    paths.push(points);
                }
            }
            current = Some(vec![point]);
        }
        PathEl::LineTo(point) => {
            if let Some(points) = current.as_mut() {
                points.push(point);
            }
        }
        PathEl::ClosePath => {
            if let Some(points) = current.take() {
                if points.len() >= 2 {
                    paths.push(points);
                }
            }
        }
        PathEl::QuadTo(_, _) | PathEl::CurveTo(_, _, _) => {}
    });
    if let Some(points) = current {
        if points.len() >= 2 {
            paths.push(points);
        }
    }
    paths
}

/// Returns the element indices that represent editable anchor points.
///
/// A `MoveTo` is the first anchor of a subpath; `LineTo`, `QuadTo`, and
/// `CurveTo` each contribute their endpoint. For a closed path whose final
/// endpoint exactly equals its `MoveTo` point, those two elements describe
/// one physical (merged) anchor, so only the `MoveTo` index is returned.
pub fn anchor_indices(path: &BezPath) -> Vec<usize> {
    let subpaths = subpaths(path);
    path.elements()
        .iter()
        .enumerate()
        .filter_map(|(index, element)| {
            let is_anchor = matches!(
                element,
                PathEl::MoveTo(_)
                    | PathEl::LineTo(_)
                    | PathEl::QuadTo(_, _)
                    | PathEl::CurveTo(_, _, _)
            );
            let merged_closing = subpaths
                .iter()
                .any(|subpath| subpath.merged && subpath.closing == Some(index));
            (is_anchor && !merged_closing).then_some(index)
        })
        .collect()
}

/// Returns an anchor's local-space position, or `None` when `index` is not
/// an editable anchor (including the duplicate closing slot of a merged
/// closed subpath).
pub fn anchor_position(path: &BezPath, index: usize) -> Option<Point> {
    anchor_indices(path)
        .contains(&index)
        .then(|| endpoint(path.elements().get(index)?))
        .flatten()
}

/// Translates one anchor and its cubic Bezier handles by `delta`.
///
/// The endpoint's incoming cubic handle (`p2`) and the following segment's
/// outgoing cubic handle (`p1`) move with the anchor. Quadratic controls are
/// deliberately left untouched: their sole control point is shared by both
/// endpoints, so it cannot remain rigid relative to both in a one-anchor
/// move. Amalith's native rectangle and ellipse constructors do not emit
/// quadratic segments.
pub fn translate_anchor(path: &mut BezPath, index: usize, delta: Vec2) {
    if !anchor_indices(path).contains(&index) {
        return;
    }

    let subpaths = subpaths(path);
    let Some(subpath) = subpaths.iter().find(|subpath| {
        let next_start = subpaths
            .iter()
            .filter(|other| other.start > subpath.start)
            .map(|other| other.start)
            .min()
            .unwrap_or(path.elements().len());
        index >= subpath.start && index < next_start
    }) else {
        return;
    };

    let merged_start = index == subpath.start && subpath.merged;
    translate_endpoint(path.elements_mut().get_mut(index), delta);
    if merged_start {
        if let Some(closing) = subpath.closing {
            translate_endpoint(path.elements_mut().get_mut(closing), delta);
            translate_incoming_handle(path.elements_mut().get_mut(closing), delta);
        }
    } else {
        translate_incoming_handle(path.elements_mut().get_mut(index), delta);
    }
    translate_outgoing_handle(path.elements_mut().get_mut(index + 1), delta);
}

#[derive(Clone, Copy, Debug)]
struct Subpath {
    start: usize,
    closing: Option<usize>,
    merged: bool,
}

/// Splits `path` at `MoveTo`s and records the endpoint immediately before a
/// `ClosePath`. Keeping this one walk shared prevents selection and mutation
/// from disagreeing about a merged closing anchor.
fn subpaths(path: &BezPath) -> Vec<Subpath> {
    let elements = path.elements();
    let mut result = Vec::new();
    let mut current: Option<(usize, Option<usize>, bool)> = None;

    let finish = |current: Option<(usize, Option<usize>, bool)>, result: &mut Vec<Subpath>| {
        let Some((start, closing, closed)) = current else {
            return;
        };
        let merged = closed
            && closing
                .and_then(|index| endpoint(&elements[index]))
                .is_some_and(|point| endpoint(&elements[start]) == Some(point));
        result.push(Subpath {
            start,
            closing: closed.then_some(closing).flatten(),
            merged,
        });
    };

    for (index, element) in elements.iter().enumerate() {
        match element {
            PathEl::MoveTo(_) => {
                finish(current.take(), &mut result);
                current = Some((index, None, false));
            }
            PathEl::LineTo(_) | PathEl::QuadTo(_, _) | PathEl::CurveTo(_, _, _) => {
                if let Some((_, closing, _)) = &mut current {
                    *closing = Some(index);
                }
            }
            PathEl::ClosePath => {
                if let Some((_, _, closed)) = &mut current {
                    *closed = true;
                }
            }
        }
    }
    finish(current, &mut result);
    result
}

fn endpoint(element: &PathEl) -> Option<Point> {
    match *element {
        PathEl::MoveTo(point) | PathEl::LineTo(point) => Some(point),
        PathEl::QuadTo(_, point) => Some(point),
        PathEl::CurveTo(_, _, point) => Some(point),
        PathEl::ClosePath => None,
    }
}

fn translate_endpoint(element: Option<&mut PathEl>, delta: Vec2) {
    match element {
        Some(PathEl::MoveTo(point)) | Some(PathEl::LineTo(point)) => *point += delta,
        Some(PathEl::QuadTo(_, point)) => *point += delta,
        Some(PathEl::CurveTo(_, _, point)) => *point += delta,
        Some(PathEl::ClosePath) | None => {}
    }
}

fn translate_incoming_handle(element: Option<&mut PathEl>, delta: Vec2) {
    if let Some(PathEl::CurveTo(_, p2, _)) = element {
        *p2 += delta;
    }
}

fn translate_outgoing_handle(element: Option<&mut PathEl>, delta: Vec2) {
    if let Some(PathEl::CurveTo(p1, _, _)) = element {
        *p1 += delta;
    }
}

/// Axis-aligned bounding box, in whatever space the caller documents.
///
/// This is a type alias rather than a newtype: every bounds computation in
/// this crate already returns a plain [`kurbo::Rect`], and callers (this
/// crate, `amalith-commands`, `amalith-io`) need normal `Rect` arithmetic
/// (`union`, `intersect`) without an unwrap/wrap ceremony at each call site.
pub type Bounds = Rect;

/// Transforms an axis-aligned rect by an affine transform and returns the
/// new axis-aligned bounding box of the (possibly rotated/sheared) result.
///
/// This is the standard "transform all 4 corners, then take the AABB"
/// operation `kurbo::Affine::transform_rect_bbox` already performs; this
/// wrapper exists purely so call sites in this crate read as document
/// vocabulary ("bounds of X after transform") rather than a raw kurbo call.
pub fn transformed_bounds(transform: Affine, rect: Rect) -> Bounds {
    transform.transform_rect_bbox(rect)
}

/// Unions two optional bounds, treating `None` as "no contribution yet".
///
/// Used when folding bounds over a list of children that may be empty or
/// whose members may individually have no geometry (e.g. an empty group).
pub fn union_bounds(a: Option<Bounds>, b: Option<Bounds>) -> Option<Bounds> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn identity_transform_preserves_bounds() {
        let rect = Rect::new(10.0, 10.0, 50.0, 30.0);
        let bounds = transformed_bounds(Affine::IDENTITY, rect);
        assert_eq!(bounds, rect);
    }

    #[test]
    fn translation_moves_bounds() {
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let bounds = transformed_bounds(Affine::translate((5.0, 5.0)), rect);
        assert_eq!(bounds, Rect::new(5.0, 5.0, 15.0, 15.0));
    }

    #[test]
    fn rotation_grows_axis_aligned_bounds() {
        // A 90 degree rotation of a non-square rect around the origin
        // swaps width/height in the resulting AABB.
        let rect = Rect::new(0.0, 0.0, 20.0, 10.0);
        let bounds = transformed_bounds(Affine::rotate(FRAC_PI_2), rect);
        assert!((bounds.width() - 10.0).abs() < 1e-9);
        assert!((bounds.height() - 20.0).abs() < 1e-9);
    }

    #[test]
    fn union_bounds_handles_missing_sides() {
        let a = Rect::new(0.0, 0.0, 1.0, 1.0);
        assert_eq!(union_bounds(Some(a), None), Some(a));
        assert_eq!(union_bounds(None, Some(a)), Some(a));
        assert_eq!(union_bounds(None, None), None);
    }

    #[test]
    fn rectangle_has_four_anchors_and_moves_no_handles() {
        let mut path = crate::PathData::rectangle(Rect::new(10.0, 20.0, 40.0, 60.0)).geometry;
        assert_eq!(anchor_indices(&path), vec![0, 1, 2, 3]);

        translate_anchor(&mut path, 2, Vec2::new(5.0, -3.0));
        assert_eq!(path.elements()[0], PathEl::MoveTo(Point::new(10.0, 20.0)));
        assert_eq!(path.elements()[1], PathEl::LineTo(Point::new(40.0, 20.0)));
        assert_eq!(path.elements()[2], PathEl::LineTo(Point::new(45.0, 57.0)));
        assert_eq!(path.elements()[3], PathEl::LineTo(Point::new(10.0, 60.0)));
        assert_eq!(path.elements()[4], PathEl::ClosePath);
    }

    #[test]
    fn ellipse_merged_anchor_moves_both_sides_of_the_seam_and_handles() {
        let mut path = crate::PathData::ellipse(Rect::new(0.0, 0.0, 100.0, 80.0)).geometry;
        assert_eq!(anchor_indices(&path), vec![0, 1, 2, 3]);

        let delta = Vec2::new(7.0, -11.0);
        let original = path.clone();
        translate_anchor(&mut path, 0, delta);

        let PathEl::MoveTo(start) = path.elements()[0] else {
            panic!()
        };
        let PathEl::CurveTo(next_p1, _, _) = path.elements()[1] else {
            panic!()
        };
        let PathEl::CurveTo(_, closing_p2, closing_end) = path.elements()[4] else {
            panic!()
        };
        let PathEl::MoveTo(original_start) = original.elements()[0] else {
            panic!()
        };
        let PathEl::CurveTo(original_next_p1, _, _) = original.elements()[1] else {
            panic!()
        };
        let PathEl::CurveTo(_, original_closing_p2, original_closing_end) = original.elements()[4]
        else {
            panic!()
        };
        assert_eq!(start, original_start + delta);
        assert_eq!(closing_end, original_closing_end + delta);
        assert_eq!(closing_p2, original_closing_p2 + delta);
        assert_eq!(next_p1, original_next_p1 + delta);
    }

    #[test]
    fn independent_closing_anchor_has_no_phantom_outgoing_handle() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.curve_to((10.0, 0.0), (20.0, 10.0), (30.0, 0.0));
        path.close_path();
        assert_eq!(anchor_indices(&path), vec![0, 1]);

        let original = path.clone();
        translate_anchor(&mut path, 1, Vec2::new(4.0, 5.0));
        let PathEl::CurveTo(p1, p2, end) = path.elements()[1] else {
            panic!()
        };
        let PathEl::CurveTo(original_p1, original_p2, original_end) = original.elements()[1] else {
            panic!()
        };
        assert_eq!(p1, original_p1);
        assert_eq!(p2, original_p2 + Vec2::new(4.0, 5.0));
        assert_eq!(end, original_end + Vec2::new(4.0, 5.0));
    }
}
