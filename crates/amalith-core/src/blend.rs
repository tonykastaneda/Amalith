//! Blend-tool geometry and color interpolation: producing one in-between
//! step's outline and paint at parameter `t` (0..1) between two source
//! shapes, and the "Smooth Color" step-count heuristic.
//!
//! A step's outline is built by resampling both source outlines to an
//! equal point count via arc length, sidestepping anchor-count /
//! correspondence entirely. The trade-off: a step is always a many-point
//! corner-anchor polygon, not the sources' original bezier curves — at
//! [`BLEND_SAMPLES`] points per subpath this reads as smooth as the
//! originals at ordinary zoom levels, but isn't an exact re-derivation of
//! their curves the way Illustrator's own (unpublished) blend algorithm
//! is. Each shape is detached from its own center before blending, then
//! the blended outline is re-placed at the step's own target center — the
//! straight line between the sources' centers by default, or a point
//! along a spine path (see [`point_on_path`]). This decouples *where* a
//! step sits from *what shape* it is, which is what makes Replace Spine
//! possible: steps still morph start-shape to end-shape while their
//! centers walk the spine instead of the line.

use crate::appearance::Paint;
use crate::geom::{Point, Vec2};
use crate::object::{Anchor, PathData, Subpath};
use crate::swatch::Color;

/// Points per interpolated subpath.
const BLEND_SAMPLES: usize = 64;

/// Average of every point across every subpath — "center" for the
/// purposes of detaching a shape from its position before blending.
/// `Point::ORIGIN` for an empty shape.
pub fn shape_center(subpaths: &[Vec<Point>]) -> Point {
    let mut sum = Vec2::ZERO;
    let mut n = 0usize;
    for sp in subpaths {
        for &p in sp {
            sum += p.to_vec2();
            n += 1;
        }
    }
    if n == 0 {
        Point::ORIGIN
    } else {
        Point::ORIGIN + sum / n as f64
    }
}

fn lerp_point(a: Point, b: Point, t: f64) -> Point {
    Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

fn lerp_vec2(a: Vec2, b: Vec2, t: f64) -> Vec2 {
    Vec2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

/// Cumulative arc length at each point of `points`, plus the closing
/// segment back to `points[0]` when `closed`.
fn arc_lengths(points: &[Point], closed: bool) -> (Vec<Point>, Vec<f64>) {
    let mut pts = points.to_vec();
    if closed {
        if let Some(&first) = points.first() {
            pts.push(first);
        }
    }
    let mut cum = vec![0.0f64; pts.len()];
    for i in 1..pts.len() {
        cum[i] = cum[i - 1] + (pts[i] - pts[i - 1]).hypot();
    }
    (pts, cum)
}

/// Resamples a point loop to exactly `n` points, evenly spaced by arc
/// length. `closed` includes the closing segment in the total (matching
/// [`crate::object::Subpath::closed`]); an open loop runs start to end
/// without wrapping.
fn resample(points: &[Point], closed: bool, n: usize) -> Vec<Point> {
    if n == 0 || points.is_empty() {
        return Vec::new();
    }
    if points.len() == 1 {
        return vec![points[0]; n];
    }
    let (pts, cum) = arc_lengths(points, closed);
    let total = *cum.last().unwrap_or(&0.0);
    if total <= 0.0 {
        return vec![pts[0]; n];
    }
    let denom = if closed { n as f64 } else { (n - 1).max(1) as f64 };
    (0..n)
        .map(|i| {
            let target = (total * i as f64 / denom).min(total);
            let seg = cum
                .iter()
                .rposition(|&c| c <= target)
                .unwrap_or(0)
                .min(pts.len() - 2);
            let (c0, c1) = (cum[seg], cum[seg + 1]);
            let frac = if c1 > c0 { (target - c0) / (c1 - c0) } else { 0.0 };
            lerp_point(pts[seg], pts[seg + 1], frac)
        })
        .collect()
}

/// A point at arc-length fraction `t` (0 = start, 1 = end, clamped) along
/// an open polyline — a spine is a path, never treated as closed here
/// even if the object itself is a closed shape.
pub fn point_on_path(points: &[Point], t: f64) -> Point {
    let Some(&first) = points.first() else {
        return Point::ORIGIN;
    };
    if points.len() == 1 {
        return first;
    }
    let (pts, cum) = arc_lengths(points, false);
    let total = *cum.last().unwrap_or(&0.0);
    if total <= 0.0 {
        return first;
    }
    let target = total * t.clamp(0.0, 1.0);
    let seg = cum
        .iter()
        .rposition(|&c| c <= target)
        .unwrap_or(0)
        .min(pts.len() - 2);
    let (c0, c1) = (cum[seg], cum[seg + 1]);
    let frac = if c1 > c0 { (target - c0) / (c1 - c0) } else { 0.0 };
    lerp_point(pts[seg], pts[seg + 1], frac)
}

/// One interpolated step's outline, `t` of the way from `a` to `b`. Both
/// are already flattened into the blend group's local space (each
/// subpath's points alongside its `closed` flag), subpaths paired by
/// index up to the shorter side's count. `center_a`/`center_b` are
/// [`shape_center`] of `a`/`b`; the result is re-centered on `center`
/// (the step's own target point).
pub fn interpolate_step(
    a: &[(Vec<Point>, bool)],
    b: &[(Vec<Point>, bool)],
    center_a: Point,
    center_b: Point,
    center: Point,
    t: f64,
) -> PathData {
    let n = a.len().min(b.len());
    let mut subpaths = Vec::with_capacity(n);
    for i in 0..n {
        let (pa, closed) = &a[i];
        let (pb, _) = &b[i];
        let ra = resample(pa, *closed, BLEND_SAMPLES);
        let rb = resample(pb, *closed, BLEND_SAMPLES);
        let anchors: Vec<Anchor> = ra
            .iter()
            .zip(rb.iter())
            .map(|(&pa, &pb)| {
                let rel = lerp_vec2(pa - center_a, pb - center_b, t);
                Anchor::corner(center + rel)
            })
            .collect();
        subpaths.push(Subpath { anchors, closed: *closed });
    }
    PathData::from_subpaths(subpaths)
}

/// `a` and `b`'s paint, `t` of the way from one to the other. Only two
/// solid colors interpolate smoothly (real per-channel lerp); any other
/// combination (a gradient, or no fill) just switches at the midpoint,
/// since blending gradients or `None` into a color has no single obvious
/// answer.
pub fn lerp_paint(a: Paint, b: Paint, t: f64) -> Paint {
    match (a, b) {
        (Paint::Solid(ca), Paint::Solid(cb)) => Paint::Solid(Color {
            r: ca.r + (cb.r - ca.r) * t as f32,
            g: ca.g + (cb.g - ca.g) * t as f32,
            b: ca.b + (cb.b - ca.b) * t as f32,
            a: ca.a + (cb.a - ca.a) * t as f32,
        }),
        _ => {
            if t < 0.5 {
                a
            } else {
                b
            }
        }
    }
}

/// "Smooth Color" step count between two fills: enough steps that
/// adjacent ones differ by roughly one sRGB unit (0..255 per channel,
/// averaged across channels) at most. Illustrator's own rule for this is
/// unpublished — this approximates the visible effect (near-identical
/// colors need few steps, black-to-white needs many) rather than
/// reproducing an exact formula. Anything that isn't two solid colors
/// falls back to a fixed, moderate step count. 256 mirrors Illustrator's
/// own published ceiling on blend steps.
pub fn smooth_color_steps(a: Paint, b: Paint) -> u32 {
    let (Paint::Solid(ca), Paint::Solid(cb)) = (a, b) else {
        return 25;
    };
    let d = ((ca.r - cb.r).abs() + (ca.g - cb.g).abs() + (ca.b - cb.b).abs()) / 3.0;
    ((d * 255.0).round() as u32).clamp(1, 256)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_center_of_a_symmetric_square_is_its_middle() {
        let square = vec![vec![
            Point::new(-10.0, -10.0),
            Point::new(10.0, -10.0),
            Point::new(10.0, 10.0),
            Point::new(-10.0, 10.0),
        ]];
        let c = shape_center(&square);
        assert!(c.x.abs() < 1e-9 && c.y.abs() < 1e-9);
    }

    #[test]
    fn point_on_path_hits_the_endpoints_and_midpoint_of_a_line() {
        let line = vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)];
        assert_eq!(point_on_path(&line, 0.0), Point::new(0.0, 0.0));
        assert_eq!(point_on_path(&line, 1.0), Point::new(10.0, 0.0));
        assert_eq!(point_on_path(&line, 0.5), Point::new(5.0, 0.0));
    }

    #[test]
    fn lerp_paint_of_two_solids_is_the_per_channel_midpoint() {
        let a = Paint::Solid(Color::rgb(0.0, 0.0, 0.0));
        let b = Paint::Solid(Color::rgb(1.0, 1.0, 1.0));
        let Paint::Solid(mid) = lerp_paint(a, b, 0.5) else {
            panic!("expected a solid paint");
        };
        assert!((mid.r - 0.5).abs() < 1e-6);
        assert!((mid.g - 0.5).abs() < 1e-6);
        assert!((mid.b - 0.5).abs() < 1e-6);
    }

    #[test]
    fn smooth_color_steps_wants_few_for_near_identical_colors_and_many_for_black_to_white() {
        let near = smooth_color_steps(
            Paint::Solid(Color::rgb(0.5, 0.5, 0.5)),
            Paint::Solid(Color::rgb(0.51, 0.5, 0.5)),
        );
        let far = smooth_color_steps(
            Paint::Solid(Color::rgb(0.0, 0.0, 0.0)),
            Paint::Solid(Color::rgb(1.0, 1.0, 1.0)),
        );
        assert!(near < far);
        assert_eq!(far, 255);
    }

    #[test]
    fn interpolating_two_identical_squares_at_different_positions_reproduces_the_square_at_the_requested_center() {
        // Same shape, shifted 100 units apart — a halfway blend should be
        // that same square, centered wherever we ask it to be (this is
        // exactly the "shape follows a spine" decoupling the whole
        // module exists for).
        let square = |cx: f64| {
            vec![vec![
                Point::new(cx - 10.0, -10.0),
                Point::new(cx + 10.0, -10.0),
                Point::new(cx + 10.0, 10.0),
                Point::new(cx - 10.0, 10.0),
            ]]
        };
        let a = square(0.0);
        let b = square(100.0);
        let ca = shape_center(&a);
        let cb = shape_center(&b);
        let a_pairs: Vec<(Vec<Point>, bool)> = a.into_iter().map(|sp| (sp, true)).collect();
        let b_pairs: Vec<(Vec<Point>, bool)> = b.into_iter().map(|sp| (sp, true)).collect();
        let target_center = Point::new(500.0, 500.0);
        let step = interpolate_step(&a_pairs, &b_pairs, ca, cb, target_center, 0.5);
        let bounds = step.local_bounds();
        let got_center = bounds.center();
        assert!((got_center.x - target_center.x).abs() < 1e-6);
        assert!((got_center.y - target_center.y).abs() < 1e-6);
        assert!((bounds.width() - 20.0).abs() < 0.5);
        assert!((bounds.height() - 20.0).abs() < 0.5);
    }
}
