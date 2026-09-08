//! Illustrator-style variable-width strokes: user-placed points along a
//! path's arc length locally widen or narrow it, tapering back to the
//! base stroke weight at the path's own two ends and between consecutive
//! points. Rendered as one filled ribbon outline (see [`width_outline`])
//! rather than a uniform-width stroke, so an asymmetric or tapering
//! profile is a single flat vector shape.
//!
//! Scoped to open, single-subpath paths for now — the same limitation
//! `pathtext` already has for the path a Type-on-a-Path object follows,
//! and for the same reason: a closed path's ribbon would need to render
//! as a hollow ring (two nested contours with opposing winding) rather
//! than a simple polygon, and a compound path has no single spine to
//! walk. Both are reasonable follow-ups once the simple case is solid.

use crate::geom::{BezPath, Point};
use crate::pathtext::ArcLengthPath;
use serde::{Deserialize, Serialize};

/// One user-placed width point: `distance` is its position along the
/// path's arc length; `left` and `right` are the half-width (the
/// perpendicular distance from the centerline to that edge) on each
/// side, independently adjustable for an asymmetric taper. Both default
/// to the stroke's own base half-width when a point is first placed, so
/// dragging it further out widens the stroke there and dragging it in
/// narrows it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WidthPoint {
    pub distance: f64,
    pub left: f64,
    pub right: f64,
}

/// The effective (left, right) half-width at arc-length `distance`,
/// given a base stroke half-width of `base_half` (what the point tapers
/// back to at the path's two ends and beyond `points`' own span).
/// `points` need not be sorted or deduplicated. Piecewise-linear — a
/// vector-native approximation of Illustrator's smoother taper, in
/// keeping with how this app already approximates other per-point
/// profiles (see `canvas::paint_freeform_fill`).
pub fn width_at(points: &[WidthPoint], total_length: f64, base_half: f64, distance: f64) -> (f64, f64) {
    if total_length <= 0.0 {
        return (base_half, base_half);
    }
    let mut nodes: Vec<(f64, f64, f64)> = points
        .iter()
        .map(|p| (p.distance.clamp(0.0, total_length), p.left.max(0.0), p.right.max(0.0)))
        .collect();
    nodes.sort_by(|a, b| a.0.total_cmp(&b.0));
    if nodes.first().is_none_or(|n| n.0 > 0.0) {
        nodes.insert(0, (0.0, base_half, base_half));
    }
    if nodes.last().is_none_or(|n| n.0 < total_length) {
        nodes.push((total_length, base_half, base_half));
    }
    let d = distance.clamp(0.0, total_length);
    let idx = nodes
        .partition_point(|n| n.0 <= d)
        .saturating_sub(1)
        .min(nodes.len() - 2);
    let (d0, l0, r0) = nodes[idx];
    let (d1, l1, r1) = nodes[idx + 1];
    let t = if d1 > d0 { (d - d0) / (d1 - d0) } else { 0.0 };
    (l0 + (l1 - l0) * t, r0 + (r1 - r0) * t)
}

/// The filled ribbon outline of an open path's variable-width stroke: the
/// left edge, then the right edge reversed, closed into one polygon.
/// Straight ("butt") ends only — a variable-width ribbon's cap style
/// isn't wired to the object's own `LineCap` yet, another disclosed v1
/// limitation. `None` for a closed path, an empty profile, or a
/// degenerate (zero-length) path.
pub fn width_outline(arc: &ArcLengthPath, points: &[WidthPoint], base_half: f64) -> Option<BezPath> {
    if arc.is_closed() || points.is_empty() {
        return None;
    }
    let total = arc.total_length();
    if total <= 0.0 {
        return None;
    }
    // A fixed resolution independent of the caller's own flattening: fine
    // enough that the taper reads as smooth, coarse enough to stay cheap
    // even for a very long path.
    let steps = ((total / 3.0).ceil() as usize).clamp(16, 400);
    let mut dists: Vec<f64> = (0..=steps).map(|i| total * i as f64 / steps as f64).collect();
    for p in points {
        dists.push(p.distance.clamp(0.0, total));
    }
    dists.sort_by(f64::total_cmp);
    dists.dedup_by(|a, b| (*a - *b).abs() < 1e-9);

    let mut left_edge = Vec::with_capacity(dists.len());
    let mut right_edge = Vec::with_capacity(dists.len());
    for &d in &dists {
        let (p, angle) = arc.point_and_tangent(d);
        let (nx, ny) = (-angle.sin(), angle.cos());
        let (lh, rh) = width_at(points, total, base_half, d);
        left_edge.push(Point::new(p.x + nx * lh, p.y + ny * lh));
        right_edge.push(Point::new(p.x - nx * rh, p.y - ny * rh));
    }

    let mut bez = BezPath::new();
    bez.move_to(left_edge[0]);
    for pt in &left_edge[1..] {
        bez.line_to(*pt);
    }
    for pt in right_edge.iter().rev() {
        bez.line_to(*pt);
    }
    bez.close_path();
    Some(bez)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_tapers_to_base_at_the_ends_and_holds_the_point_value_at_it() {
        let points = [WidthPoint { distance: 50.0, left: 10.0, right: 4.0 }];
        assert_eq!(width_at(&points, 100.0, 2.0, 0.0), (2.0, 2.0));
        assert_eq!(width_at(&points, 100.0, 2.0, 50.0), (10.0, 4.0));
        assert_eq!(width_at(&points, 100.0, 2.0, 100.0), (2.0, 2.0));
        assert_eq!(width_at(&points, 100.0, 2.0, 25.0), (6.0, 3.0));
    }

    #[test]
    fn width_interpolates_linearly_between_two_points() {
        let points = [
            WidthPoint { distance: 20.0, left: 4.0, right: 4.0 },
            WidthPoint { distance: 80.0, left: 12.0, right: 12.0 },
        ];
        assert_eq!(width_at(&points, 100.0, 2.0, 50.0), (8.0, 8.0));
    }

    #[test]
    fn outline_is_none_for_a_closed_path_or_an_empty_profile() {
        let arc = ArcLengthPath::new(
            &[Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0)],
            true,
        );
        assert!(width_outline(&arc, &[WidthPoint { distance: 5.0, left: 3.0, right: 3.0 }], 1.0).is_none());
        let open = ArcLengthPath::new(&[Point::new(0.0, 0.0), Point::new(10.0, 0.0)], false);
        assert!(width_outline(&open, &[], 1.0).is_none());
    }

    #[test]
    fn outline_widens_at_a_placed_point_on_a_straight_line() {
        let open = ArcLengthPath::new(&[Point::new(0.0, 0.0), Point::new(100.0, 0.0)], false);
        let points = [WidthPoint { distance: 50.0, left: 8.0, right: 8.0 }];
        let outline = width_outline(&open, &points, 1.0).unwrap();
        let bounds = crate::geom::bez_path_bounds(&outline);
        assert!((bounds.height() - 16.0).abs() < 1e-6);
        assert!((bounds.y0 + 8.0).abs() < 1e-6);
    }
}
