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
}
