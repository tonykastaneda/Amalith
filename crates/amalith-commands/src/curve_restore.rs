//! Restore boolean boundaries from the original Bézier spans. Polygon vertices
//! are for topology, not a request to replace the source curves with a fit.
use kurbo::{BezPath, ParamCurve, ParamCurveExtrema, ParamCurveNearest, PathSeg, Point, Vec2};

const MATCH_TOL: f64 = 0.11; // twice Pathfinder's 0.05pt flattening tolerance

#[derive(Clone, Copy)]
struct Edge {
    source: Option<usize>,
    t0: f64,
    t1: f64,
    start: Point,
    end: Point,
}

fn same_run(a: Edge, b: Edge) -> bool {
    a.source.is_some()
        && a.source == b.source
        && (a.t1 - a.t0) * (b.t1 - b.t0) > 0.0
        && (a.t1 - b.t0).abs() < 1e-6
}

fn derivative(s: PathSeg, t: f64) -> Vec2 {
    match s {
        PathSeg::Line(l) => l.p1 - l.p0,
        PathSeg::Quad(q) => (q.p1 - q.p0) * (2.0 * (1.0 - t)) + (q.p2 - q.p1) * (2.0 * t),
        PathSeg::Cubic(c) => {
            (c.p1 - c.p0) * (3.0 * (1.0 - t).powi(2))
                + (c.p2 - c.p1) * (6.0 * t * (1.0 - t))
                + (c.p3 - c.p2) * (3.0 * t * t)
        }
    }
}

/// Refine a polygon crossing on both original curves, so adjacent restored
/// spans share one exact endpoint rather than independently snapping apart.
fn crossing(
    a: PathSeg,
    mut ta: f64,
    b: PathSeg,
    mut tb: f64,
    near: Point,
) -> Option<(f64, f64, Point)> {
    for _ in 0..12 {
        let pa = a.eval(ta);
        let pb = b.eval(tb);
        let f = pa - pb;
        if f.hypot() < 1e-8 {
            let q = pa.midpoint(pb);
            return ((q - near).hypot() <= MATCH_TOL * 2.0).then_some((ta, tb, q));
        }
        let da = derivative(a, ta);
        let db = derivative(b, tb);
        let det = da.cross(db);
        if det.abs() <= 1e-12 * da.hypot() * db.hypot() {
            return None;
        }
        ta += db.cross(f) / det;
        tb += da.cross(f) / det;
        if !(-1e-7..=1.0 + 1e-7).contains(&ta) || !(-1e-7..=1.0 + 1e-7).contains(&tb) {
            return None;
        }
        ta = ta.clamp(0.0, 1.0);
        tb = tb.clamp(0.0, 1.0);
    }
    None
}

fn match_edge(start: Point, end: Point, sources: &[PathSeg]) -> Edge {
    let mid = start.midpoint(end);
    let mut best = None;
    for (id, seg) in sources.iter().enumerate() {
        let bounds = seg.bounding_box().inflate(MATCH_TOL, MATCH_TOL);
        if !bounds.contains(start) || !bounds.contains(end) {
            continue;
        }
        let a = seg.nearest(start, 1e-7);
        let b = seg.nearest(end, 1e-7);
        let m = seg.nearest(mid, 1e-7);
        if [a.distance_sq, b.distance_sq, m.distance_sq]
            .into_iter()
            .any(|d| d > MATCH_TOL * MATCH_TOL)
        {
            continue;
        }
        if (a.t - b.t).abs() < 1e-12 {
            continue;
        }
        // Endpoint agreement distinguishes a real source edge from a nearby
        // crossing; the chord midpoint also guards against spanning a bulge.
        let score = a.distance_sq + b.distance_sq + m.distance_sq;
        if best.is_none_or(|(old, _, _, _)| score < old) {
            best = Some((score, id, a.t, b.t));
        }
    }
    let (source, t0, t1) = best.map_or((None, 0.0, 1.0), |(_, id, a, b)| (Some(id), a, b));
    Edge {
        source,
        t0,
        t1,
        start,
        end,
    }
}

pub(crate) fn restore(contours: &[Vec<[f64; 2]>], originals: &[BezPath]) -> BezPath {
    let sources: Vec<_> = originals.iter().flat_map(|p| p.segments()).collect();
    let mut path = BezPath::new();
    for contour in contours {
        let mut pts: Vec<_> = contour.iter().map(|p| Point::new(p[0], p[1])).collect();
        pts.dedup();
        if pts.len() > 1 && pts.first() == pts.last() {
            pts.pop();
        }
        if pts.len() < 3 {
            continue;
        }
        let n = pts.len();
        let mut edges: Vec<_> = (0..n)
            .map(|i| match_edge(pts[i], pts[(i + 1) % n], &sources))
            .collect();
        // Do not split an original span just because the polygon loop starts
        // in its middle. Rotate the loop to a genuine source-span boundary.
        let start = (0..n)
            .find(|&i| !same_run(edges[(i + n - 1) % n], edges[i]))
            .unwrap_or(0);
        edges.rotate_left(start);
        let mut runs: Vec<Edge> = Vec::new();
        for e in edges {
            if let Some(last) = runs.last_mut().filter(|last| same_run(**last, e)) {
                last.t1 = e.t1;
                last.end = e.end;
            } else {
                runs.push(e);
            }
        }
        let n = runs.len();
        for i in 0..n {
            let j = (i + 1) % n;
            let (a, b) = (runs[i], runs[j]);
            if let (Some(ai), Some(bi)) = (a.source, b.source) {
                if let Some((ta, tb, q)) = crossing(sources[ai], a.t1, sources[bi], b.t0, a.end) {
                    runs[i].t1 = ta;
                    runs[i].end = q;
                    runs[j].t0 = tb;
                    runs[j].start = q;
                }
            }
        }
        path.move_to(runs[0].start);
        for r in runs {
            match r.source.map(|id| sources[id].subsegment(r.t0..r.t1)) {
                Some(PathSeg::Cubic(c)) => {
                    path.curve_to(c.p1 + (r.start - c.p0), c.p2 + (r.end - c.p3), r.end)
                }
                Some(PathSeg::Quad(q)) => {
                    path.quad_to(q.p1 + ((r.start - q.p0) + (r.end - q.p2)) * 0.5, r.end)
                }
                _ => path.line_to(r.end),
            }
        }
        path.close_path();
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::PathData;
    use kurbo::Rect;

    #[test]
    fn restoring_a_circle_keeps_its_four_original_cubic_spans() {
        let source = PathData::ellipse(Rect::new(0., 0., 400., 400.)).geometry;
        let polygon = crate::pathfinder::flatten_path(&source);
        let restored = PathData::from_bezpath(restore(&polygon, &[source]));
        assert_eq!(restored.subpaths().len(), 1);
        assert_eq!(restored.subpaths()[0].anchors.len(), 4);
        assert!(
            restored
                .geometry
                .segments()
                .all(|s| matches!(s, PathSeg::Cubic(_)))
        );
    }

    #[test]
    fn shallow_original_corners_are_not_smoothed_away() {
        let mut source = BezPath::new();
        source.move_to((0., 0.));
        source.line_to((100., 0.));
        source.line_to((200., 20.));
        source.line_to((200., 100.));
        source.line_to((0., 100.));
        source.close_path();
        let polygon = crate::pathfinder::flatten_path(&source);
        let restored = PathData::from_bezpath(restore(&polygon, &[source]));
        assert_eq!(restored.subpaths()[0].anchors.len(), 5);
        assert!(
            restored.subpaths()[0]
                .anchors
                .iter()
                .any(|a| (a.point - Point::new(100., 0.)).hypot() < 1e-8)
        );
        assert!(
            restored
                .geometry
                .segments()
                .all(|s| matches!(s, PathSeg::Line(_)))
        );
    }
}
