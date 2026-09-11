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

/// Illustrator's built-in "Width Profile" presets — a handful of canned
/// taper shapes you can drop onto a stroke instead of hand-placing width
/// points. Picking one just seeds [`preset_points`]'s output as that
/// object's `width_points`; the points are then ordinary, freely
/// draggable width points from then on, exactly as if placed by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidthProfilePreset {
    Uniform,
    /// Point → wide → point, symmetric (the classic "lens").
    Profile1,
    /// Two unequal rounded lobes connected by a narrow waist.
    Profile2,
    /// Point → wide plateau → point (flat through the middle, a hex).
    Profile3,
    /// Wide at the start, tapering to a sharp point at the end.
    Profile4,
    /// Point at the start, widening to a peak past the midpoint, back to
    /// a point at the end — the peak skewed toward the far end.
    Profile5,
    /// A rounded, one-sided arch with the path forming its flat edge.
    Profile6,
}

impl WidthProfilePreset {
    pub const ALL: [WidthProfilePreset; 7] = [
        WidthProfilePreset::Uniform,
        WidthProfilePreset::Profile1,
        WidthProfilePreset::Profile2,
        WidthProfilePreset::Profile3,
        WidthProfilePreset::Profile4,
        WidthProfilePreset::Profile5,
        WidthProfilePreset::Profile6,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WidthProfilePreset::Uniform => "Uniform",
            WidthProfilePreset::Profile1 => "Width Profile 1",
            WidthProfilePreset::Profile2 => "Width Profile 2",
            WidthProfilePreset::Profile3 => "Width Profile 3",
            WidthProfilePreset::Profile4 => "Width Profile 4",
            WidthProfilePreset::Profile5 => "Width Profile 5",
            WidthProfilePreset::Profile6 => "Width Profile 6",
        }
    }
}

/// Width presets normalized to the stroke weight. Curved silhouettes are
/// sampled into ordinary width points so rendering, editing, and previews
/// all use the same geometry without introducing a second profile format.
pub fn preset_points(preset: WidthProfilePreset, total_length: f64, base_half: f64) -> Vec<WidthPoint> {
    let at = |t: f64, left: f64, right: f64| WidthPoint {
        distance: total_length * t,
        left: base_half * left,
        right: base_half * right,
    };
    match preset {
        WidthProfilePreset::Uniform => Vec::new(),
        WidthProfilePreset::Profile3 => vec![
            at(0.0, 0.0, 0.0), at(0.125, 1.0, 1.0),
            at(0.875, 1.0, 1.0), at(1.0, 0.0, 0.0),
        ],
        WidthProfilePreset::Profile4 => vec![at(0.0, 1.0, 1.0), at(1.0, 0.0, 0.0)],
        _ => (0..=64).map(|i| {
            let t = i as f64 / 64.0;
            let lens = |u: f64| 4.0 * u * (1.0 - u);
            let half = match preset {
                WidthProfilePreset::Profile1 | WidthProfilePreset::Profile6 => lens(t),
                WidthProfilePreset::Profile2 => {
                    // A smaller leading lobe and a larger trailing lobe,
                    // joined by a narrow, nonzero waist.
                    let nodes = [(0.0, 0.0), (0.20, 0.82), (0.40, 0.12), (0.73, 1.0), (1.0, 0.0)];
                    let i = nodes.partition_point(|n| n.0 <= t).saturating_sub(1).min(3);
                    let (x0, y0) = nodes[i];
                    let (x1, y1) = nodes[i + 1];
                    let u = (t - x0) / (x1 - x0);
                    // Horizontal tangents at the internal extrema; pointed
                    // outer ends rather than rounded capsule ends.
                    let m0 = if i == 0 { 1.5 * (y1 - y0) } else { 0.0 };
                    let m1 = if i == 3 { 1.5 * (y1 - y0) } else { 0.0 };
                    (2.0*u*u*u - 3.0*u*u + 1.0)*y0
                        + (u*u*u - 2.0*u*u + u)*m0
                        + (-2.0*u*u*u + 3.0*u*u)*y1
                        + (u*u*u - u*u)*m1
                }
                WidthProfilePreset::Profile5 => {
                    if t <= 0.70 {
                        let u = t / 0.70;
                        // A long fine entry swelling into the far-end bulb.
                        u + u*u - u*u*u
                    } else {
                        let u = (t - 0.70) / 0.30;
                        1.0 - u*u
                    }
                }
                _ => unreachable!(),
            };
            let half = half.clamp(0.0, 1.0);
            if preset == WidthProfilePreset::Profile6 {
                // The path is the flat edge; the entire width lies on one side.
                at(t, 0.0, 2.0 * half)
            } else {
                at(t, half, half)
            }
        }).collect(),
    }
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

    /// `WidthProfilePreset::ALL` is hand-written, not derived — same
    /// compile-enforced safety net as `Tool::ALL`: `covered` is an
    /// exhaustive match with no wildcard, so adding a variant and
    /// forgetting it here fails to *compile*, not just fails to pass.
    #[test]
    fn width_profile_preset_all_covers_every_variant_exactly_once() {
        fn covered(p: WidthProfilePreset) -> bool {
            match p {
                WidthProfilePreset::Uniform
                | WidthProfilePreset::Profile1
                | WidthProfilePreset::Profile2
                | WidthProfilePreset::Profile3
                | WidthProfilePreset::Profile4
                | WidthProfilePreset::Profile5
                | WidthProfilePreset::Profile6 => true,
            }
        }
        for p in WidthProfilePreset::ALL {
            assert!(covered(p), "{p:?} missing from the exhaustive check above");
        }
        let mut seen: Vec<WidthProfilePreset> = Vec::new();
        for p in WidthProfilePreset::ALL {
            assert!(!seen.contains(&p), "{p:?} appears more than once in ALL");
            seen.push(p);
        }
    }

    #[test]
    fn uniform_preset_is_always_empty() {
        assert!(preset_points(WidthProfilePreset::Uniform, 100.0, 2.0).is_empty());
    }

    #[test]
    fn profile1_tapers_to_a_point_at_both_ends_and_peaks_in_the_middle() {
        let pts = preset_points(WidthProfilePreset::Profile1, 100.0, 2.0);
        assert_eq!(width_at(&pts, 100.0, 2.0, 0.0), (0.0, 0.0));
        assert_eq!(width_at(&pts, 100.0, 2.0, 100.0), (0.0, 0.0));
        let (l, r) = width_at(&pts, 100.0, 2.0, 50.0);
        assert_eq!((l, r), (2.0, 2.0), "peak should preserve the stroke weight");
    }

    #[test]
    fn rounded_profiles_and_asymmetric_arch_match_their_silhouettes() {
        let lens = preset_points(WidthProfilePreset::Profile1, 100.0, 5.0);
        assert!((width_at(&lens, 100.0, 5.0, 25.0).0 - 3.75).abs() < 1e-9);
        let lobes = preset_points(WidthProfilePreset::Profile2, 100.0, 5.0);
        let waist = width_at(&lobes, 100.0, 5.0, 40.0).0;
        assert!(waist > 0.0 && waist < 1.0);
        assert!(width_at(&lobes, 100.0, 5.0, 73.0).0 > width_at(&lobes, 100.0, 5.0, 20.0).0);
        let arch = preset_points(WidthProfilePreset::Profile6, 100.0, 5.0);
        assert!(arch.iter().all(|p| p.left == 0.0));
        assert_eq!(width_at(&arch, 100.0, 5.0, 50.0), (0.0, 10.0));
        for preset in WidthProfilePreset::ALL {
            for p in preset_points(preset, 100.0, 5.0) {
                assert!(p.left >= 0.0 && p.right >= 0.0);
                assert!(p.left + p.right <= 10.0 + 1e-9);
            }
        }
    }

    #[test]
    fn profile4_is_a_one_sided_taper_from_wide_to_a_point() {
        let pts = preset_points(WidthProfilePreset::Profile4, 100.0, 2.0);
        assert_eq!(width_at(&pts, 100.0, 2.0, 0.0), (2.0, 2.0));
        assert_eq!(width_at(&pts, 100.0, 2.0, 100.0), (0.0, 0.0));
    }
}
