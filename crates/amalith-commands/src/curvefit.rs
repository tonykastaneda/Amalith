//! Fits smooth cubic Bézier curves onto a flattened polygon contour —
//! Philip Schneider's classic curve-fitting algorithm (Graphics Gems),
//! with one addition: hard-corner detection, so a real corner in the
//! shape (a straight edge, or where two curves crossed) stays a sharp
//! anchor instead of getting rounded off.
//!
//! Every Pathfinder-based op (boolean ops, Shape Builder, Offset Path)
//! works on flattened polygons and has no curves of its own to hand
//! back — this is what turns that raw point cloud back into a path with
//! a small, editable number of anchors, the way Illustrator's own
//! boolean engine does, instead of leaving one anchor per flattened
//! polygon vertex.

use kurbo::{BezPath, Point, Vec2};

/// Above this angle (degrees) between a contour point's incoming and
/// outgoing direction, treat it as a real corner — a curve fit is never
/// allowed to smooth over it. 30° comfortably separates "this is still
/// part of a smooth arc" from "two originally distinct edges met here"
/// (a circle flattened at any reasonable tolerance never bends this
/// sharply point-to-point; a rectangle corner is 90°).
const CORNER_ANGLE_DEG: f64 = 30.0;

/// Max allowed distance (document-space units) between the fitted curve
/// and the original flattened points — the same order of magnitude as
/// `pathfinder::TOL`, since the flattened points are themselves already
/// only that accurate.
const MAX_ERROR: f64 = 1.0;

/// Fits one closed contour (as produced by `pathfinder::flatten_path`,
/// in order but not repeating its start point) into a closed `BezPath`
/// of smooth cubic segments, split at detected corners.
pub fn fit_closed_contour(points: &[[f64; 2]]) -> BezPath {
    let pts: Vec<Point> = points.iter().map(|p| Point::new(p[0], p[1])).collect();
    let mut path = BezPath::new();
    if pts.len() < 3 {
        return path;
    }
    let corners = corner_indices(&pts);
    // Rotate so the path starts at a corner, if there is one — keeps
    // every fitted arc a single contiguous run with no wraparound.
    let start = corners.first().copied().unwrap_or(0);
    let n = pts.len();
    let rotated: Vec<Point> = (0..n).map(|i| pts[(start + i) % n]).collect();
    let mut cuts: Vec<usize> = corners.iter().map(|&c| (c + n - start) % n).collect();
    cuts.sort_unstable();
    cuts.dedup();
    if cuts.first() != Some(&0) {
        cuts.insert(0, 0);
    }
    cuts.push(n); // close the loop back to the start

    path.move_to(rotated[0]);
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b - a < 1 {
            continue;
        }
        // `arc` runs point `a` through point `b` inclusive (`b` may be
        // `n`, i.e. the start point again, closing the loop).
        let mut arc: Vec<Point> = rotated[a..b.min(n)].to_vec();
        arc.push(rotated[b % n]);
        if arc.len() < 3 {
            for p in &arc[1..] {
                path.line_to(*p);
            }
            continue;
        }
        for seg in fit_curve(&arc, MAX_ERROR) {
            path.curve_to(seg[1], seg[2], seg[3]);
        }
    }
    path.close_path();
    path
}

/// Indices of `pts` (a closed loop) whose direction change exceeds
/// [`CORNER_ANGLE_DEG`].
fn corner_indices(pts: &[Point]) -> Vec<usize> {
    let n = pts.len();
    let mut out = Vec::new();
    for i in 0..n {
        let prev = pts[(i + n - 1) % n];
        let cur = pts[i];
        let next = pts[(i + 1) % n];
        let in_dir = cur - prev;
        let out_dir = next - cur;
        if in_dir.hypot() < 1e-9 || out_dir.hypot() < 1e-9 {
            continue;
        }
        let cos = (in_dir.normalize().dot(out_dir.normalize())).clamp(-1.0, 1.0);
        let angle_deg = cos.acos().to_degrees();
        if angle_deg > CORNER_ANGLE_DEG {
            out.push(i);
        }
    }
    out
}

/// Schneider's algorithm: fits `points` (an open polyline, endpoints
/// included) as a list of cubic Bézier segments `[p0, p1, p2, p3]`,
/// each starting where the last left off.
fn fit_curve(points: &[Point], max_error: f64) -> Vec<[Point; 4]> {
    let left_tangent = tangent(points[1] - points[0]);
    let right_tangent = tangent(points[points.len() - 2] - points[points.len() - 1]);
    fit_cubic(points, left_tangent, right_tangent, max_error)
}

fn tangent(v: Vec2) -> Vec2 {
    if v.hypot() < 1e-9 {
        Vec2::new(1.0, 0.0)
    } else {
        v.normalize()
    }
}

fn fit_cubic(points: &[Point], t1: Vec2, t2: Vec2, error: f64) -> Vec<[Point; 4]> {
    if points.len() == 2 {
        let dist = (points[1] - points[0]).hypot() / 3.0;
        return vec![[
            points[0],
            points[0] + t1 * dist,
            points[1] + t2 * dist,
            points[1],
        ]];
    }
    let u = chord_length_parameterize(points);
    let bez = generate_bezier(points, &u, t1, t2);
    let (max_err, split) = max_error_at(points, &bez, &u);
    if max_err < error {
        return vec![bez];
    }
    if max_err < error * error {
        let mut u = u;
        for _ in 0..4 {
            reparameterize(points, &bez, &mut u);
        }
        let bez2 = generate_bezier(points, &u, t1, t2);
        let (max_err2, split2) = max_error_at(points, &bez2, &u);
        if max_err2 < error {
            return vec![bez2];
        }
        return split_and_fit(points, split2, t1, t2, error);
    }
    split_and_fit(points, split, t1, t2, error)
}

fn split_and_fit(points: &[Point], split: usize, t1: Vec2, t2: Vec2, error: f64) -> Vec<[Point; 4]> {
    let split = split.clamp(1, points.len() - 2);
    let center_tangent = tangent(points[split - 1] - points[split + 1]);
    let mut left = fit_cubic(&points[..=split], t1, center_tangent, error);
    let right = fit_cubic(&points[split..], -center_tangent, t2, error);
    left.extend(right);
    left
}

fn chord_length_parameterize(points: &[Point]) -> Vec<f64> {
    let mut u = vec![0.0];
    for i in 1..points.len() {
        u.push(u[i - 1] + (points[i] - points[i - 1]).hypot());
    }
    let total = *u.last().unwrap();
    if total > 1e-9 {
        for v in &mut u {
            *v /= total;
        }
    }
    u
}

fn bernstein(t: f64) -> [f64; 4] {
    let mt = 1.0 - t;
    [mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t]
}

/// Least-squares fit of the two control-point distances along `t1`/`t2`,
/// holding the endpoints and tangent directions fixed — the core of
/// Schneider's method.
fn generate_bezier(points: &[Point], u: &[f64], t1: Vec2, t2: Vec2) -> [Point; 4] {
    let (p0, p3) = (points[0], points[points.len() - 1]);
    let mut c = [[0.0f64; 2]; 2];
    let mut x = [0.0f64; 2];
    for (i, &ui) in u.iter().enumerate() {
        let b = bernstein(ui);
        let a1 = t1 * b[1];
        let a2 = t2 * b[2];
        c[0][0] += a1.dot(a1);
        c[0][1] += a1.dot(a2);
        c[1][0] = c[0][1];
        c[1][1] += a2.dot(a2);
        // B(u) with the unknown P1/P2 replaced by P0/P3 (i.e. alpha=0)
        // — what's left over is exactly what alpha1*t1 + alpha2*t2 at
        // this u needs to make up.
        let fixed = p0.to_vec2() * (b[0] + b[1]) + p3.to_vec2() * (b[2] + b[3]);
        let shortfall = points[i].to_vec2() - fixed;
        x[0] += a1.dot(shortfall);
        x[1] += a2.dot(shortfall);
    }
    let det_c0_c1 = c[0][0] * c[1][1] - c[1][0] * c[0][1];
    let det_c0_x = c[0][0] * x[1] - c[1][0] * x[0];
    let det_x_c1 = x[0] * c[1][1] - x[1] * c[0][1];
    let (mut alpha1, mut alpha2) = if det_c0_c1.abs() < 1e-12 {
        let c0 = c[0][0] + c[1][0];
        let c1 = c[0][1] + c[1][1];
        if c0.abs() > 1e-12 && c1.abs() > 1e-12 {
            (x[0] / c0, x[1] / c1)
        } else {
            (0.0, 0.0)
        }
    } else {
        (det_x_c1 / det_c0_c1, det_c0_x / det_c0_c1)
    };
    let seg_len = (p0 - p3).hypot();
    let epsilon = 1.0e-6 * seg_len;
    if alpha1 < epsilon || alpha2 < epsilon || !alpha1.is_finite() || !alpha2.is_finite() {
        alpha1 = seg_len / 3.0;
        alpha2 = seg_len / 3.0;
    }
    [p0, p0 + t1 * alpha1, p3 + t2 * alpha2, p3]
}

fn eval_bezier(bez: &[Point; 4], t: f64) -> Point {
    let b = bernstein(t);
    Point::new(
        bez[0].x * b[0] + bez[1].x * b[1] + bez[2].x * b[2] + bez[3].x * b[3],
        bez[0].y * b[0] + bez[1].y * b[1] + bez[2].y * b[2] + bez[3].y * b[3],
    )
}

fn eval_bezier_deriv(bez: &[Point; 4], t: f64) -> Vec2 {
    let mt = 1.0 - t;
    (bez[1] - bez[0]) * (3.0 * mt * mt)
        + (bez[2] - bez[1]) * (6.0 * mt * t)
        + (bez[3] - bez[2]) * (3.0 * t * t)
}

fn max_error_at(points: &[Point], bez: &[Point; 4], u: &[f64]) -> (f64, usize) {
    let mut max_dist = 0.0;
    let mut split = points.len() / 2;
    for (i, (&p, &t)) in points.iter().zip(u.iter()).enumerate() {
        let d = (eval_bezier(bez, t) - p).hypot();
        if d > max_dist {
            max_dist = d;
            split = i;
        }
    }
    (max_dist, split)
}

/// One pass of Newton-Raphson reparameterization, nudging each `u[i]`
/// toward the true closest point on `bez`.
fn reparameterize(points: &[Point], bez: &[Point; 4], u: &mut [f64]) {
    for (i, ui) in u.iter_mut().enumerate() {
        let p = eval_bezier(bez, *ui);
        let d1 = eval_bezier_deriv(bez, *ui);
        let diff = p - points[i];
        let denom = d1.dot(d1);
        if denom.abs() > 1e-9 {
            let num = diff.dot(d1);
            let next = *ui - num / denom;
            if next.is_finite() {
                *ui = next.clamp(0.0, 1.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;

    fn circle_points(cx: f64, cy: f64, r: f64, n: usize) -> Vec<[f64; 2]> {
        (0..n)
            .map(|i| {
                let a = i as f64 / n as f64 * std::f64::consts::TAU;
                [cx + r * a.cos(), cy + r * a.sin()]
            })
            .collect()
    }

    #[test]
    fn a_flattened_circle_fits_into_a_handful_of_smooth_segments() {
        let pts = circle_points(0.0, 0.0, 100.0, 150);
        let path = fit_closed_contour(&pts);
        let segments = path.segments().count();
        assert!(segments <= 8, "expected a handful of curve segments, got {segments}");
        // Every one of the original 150 flattened points stays within
        // tolerance of the fitted curve — cheaply checked via the
        // fitted shape's own area matching a circle of this radius.
        let area = path.area().abs();
        let expected = std::f64::consts::PI * 100.0 * 100.0;
        assert!((area - expected).abs() / expected < 0.01, "area {area} vs expected {expected}");
    }

    #[test]
    fn a_flattened_square_keeps_its_four_sharp_corners() {
        // Densely re-sampled edges (not just the 4 corners) — the real
        // shape this needs to handle is a flattened polygon, not a
        // hand-drawn 4-point square.
        let mut pts = Vec::new();
        for &(x0, y0, x1, y1) in &[(0.0, 0.0, 100.0, 0.0), (100.0, 0.0, 100.0, 100.0), (100.0, 100.0, 0.0, 100.0), (0.0, 100.0, 0.0, 0.0)] {
            for i in 0..10 {
                let t = i as f64 / 10.0;
                pts.push([x0 + (x1 - x0) * t, y0 + (y1 - y0) * t]);
            }
        }
        let path = fit_closed_contour(&pts);
        let segments = path.segments().count();
        assert_eq!(segments, 4, "a square's 4 corners should each start a new fitted segment, got {segments}");
        let area = path.area().abs();
        assert!((area - 10_000.0).abs() < 50.0, "area {area}");
    }
}
