//! Arc-length parameterization of a flattened path, for placing glyphs
//! along a path (Text on a Path).
//!
//! This is deliberately separate from [`crate::blend::point_on_path`]
//! rather than sharing its arc-length walk: a blend spine is sampled at a
//! *fraction* 0..1 of the whole path, while a glyph run is sampled at
//! *absolute* successive distances (each glyph's advance width farther
//! than the last), and a closed path wraps for text (a ring of type keeps
//! flowing past the seam) but never does for a blend spine. Small enough
//! math to duplicate rather than force one function to serve both.

use crate::geom::Point;

/// A flattened path's cumulative arc length, built once per paint / hit
/// pass and queried once per glyph.
pub struct ArcLengthPath {
    points: Vec<Point>,
    cum: Vec<f64>,
    closed: bool,
}

impl ArcLengthPath {
    /// `points` is an already-flattened polyline in the path's local
    /// space. `closed` adds the closing segment back to `points[0]` and
    /// makes [`Self::point_and_tangent`] wrap instead of clamp.
    pub fn new(points: &[Point], closed: bool) -> Self {
        let mut pts = points.to_vec();
        pts.dedup_by(|a, b| (*a - *b).hypot2() < 1e-18);
        if closed {
            if let Some(&first) = points.first() {
                if pts.last() != Some(&first) {
                    pts.push(first);
                }
            }
        }
        let mut cum = vec![0.0f64; pts.len()];
        for i in 1..pts.len() {
            cum[i] = cum[i - 1] + (pts[i] - pts[i - 1]).hypot();
        }
        Self {
            points: pts,
            cum,
            closed,
        }
    }

    pub fn total_length(&self) -> f64 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Position and tangent angle (radians, `atan2` convention) at
    /// absolute arc-length `distance` along the path. A closed path wraps
    /// (`rem_euclid`); an open one clamps to `[0, total_length]`.
    pub fn point_and_tangent(&self, distance: f64) -> (Point, f64) {
        let total = self.total_length();
        if self.points.len() < 2 || total <= 0.0 {
            return (self.points.first().copied().unwrap_or(Point::ORIGIN), 0.0);
        }
        let d = if self.closed {
            distance.rem_euclid(total)
        } else {
            distance.clamp(0.0, total)
        };
        let seg = self
            .cum
            .partition_point(|&c| c <= d)
            .saturating_sub(1)
            .min(self.points.len() - 2);
        let (c0, c1) = (self.cum[seg], self.cum[seg + 1]);
        let (p0, p1) = (self.points[seg], self.points[seg + 1]);
        let frac = if c1 > c0 { (d - c0) / (c1 - c0) } else { 0.0 };
        let p = Point::new(p0.x + (p1.x - p0.x) * frac, p0.y + (p1.y - p0.y) * frac);
        let angle = (p1.y - p0.y).atan2(p1.x - p0.x);
        (p, angle)
    }

    /// The arc-length distance of the closest point on the path to `p` —
    /// projects `p` onto every segment and keeps the nearest. Drives the
    /// start/end/center bracket handles: dragging one finds where along
    /// the path the pointer currently is.
    pub fn nearest_distance(&self, p: Point) -> f64 {
        let mut best_d2 = f64::INFINITY;
        let mut best_dist = 0.0;
        for i in 0..self.points.len().saturating_sub(1) {
            let (p0, p1) = (self.points[i], self.points[i + 1]);
            let seg = crate::geom::Vec2::new(p1.x - p0.x, p1.y - p0.y);
            let len2 = seg.x * seg.x + seg.y * seg.y;
            let t = if len2 > 0.0 {
                (((p.x - p0.x) * seg.x + (p.y - p0.y) * seg.y) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let proj = Point::new(p0.x + seg.x * t, p0.y + seg.y * t);
            let d2 = (p.x - proj.x).powi(2) + (p.y - proj.y).powi(2);
            if d2 < best_d2 {
                best_d2 = d2;
                best_dist = self.cum[i] + (self.cum[i + 1] - self.cum[i]) * t;
            }
        }
        best_dist
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_points_do_not_destroy_endpoint_tangent() {
        let path = ArcLengthPath::new(
            &[
                Point::ORIGIN,
                Point::ORIGIN,
                Point::new(0.0, 10.0),
                Point::new(0.0, 10.0),
            ],
            false,
        );
        assert_eq!(path.total_length(), 10.0);
        assert_eq!(path.point_and_tangent(10.0).1, std::f64::consts::FRAC_PI_2);
    }

    #[test]
    fn owned_path_text_roundtrips_and_old_text_loads() {
        use crate::{PathData, Rect, TextData};
        let mut data = TextData::default();
        data.path_geometry = Some(PathData::ellipse(Rect::new(0.0, 0.0, 100.0, 100.0)));
        let json = serde_json::to_value(&data).unwrap();
        assert_eq!(
            serde_json::from_value::<TextData>(json.clone()).unwrap(),
            data
        );
        let mut legacy = json;
        legacy.as_object_mut().unwrap().remove("path_geometry");
        assert!(serde_json::from_value::<TextData>(legacy)
            .unwrap()
            .path_geometry
            .is_none());
    }

    #[test]
    fn nearest_distance_projects_onto_the_segment() {
        let path = ArcLengthPath::new(&[Point::new(0.0, 0.0), Point::new(100.0, 0.0)], false);
        // A point above the middle of the segment projects straight down.
        assert_eq!(path.nearest_distance(Point::new(40.0, 25.0)), 40.0);
        // Past either end, the closest point is that endpoint.
        assert_eq!(path.nearest_distance(Point::new(-20.0, 5.0)), 0.0);
        assert_eq!(path.nearest_distance(Point::new(150.0, 5.0)), 100.0);
    }

    #[test]
    fn straight_line_midpoint() {
        let path = ArcLengthPath::new(&[Point::new(0.0, 0.0), Point::new(100.0, 0.0)], false);
        assert_eq!(path.total_length(), 100.0);
        let (p, angle) = path.point_and_tangent(50.0);
        assert_eq!(p, Point::new(50.0, 0.0));
        assert_eq!(angle, 0.0);
    }

    #[test]
    fn open_path_clamps_past_the_ends() {
        let path = ArcLengthPath::new(&[Point::new(0.0, 0.0), Point::new(10.0, 0.0)], false);
        assert_eq!(path.point_and_tangent(-5.0).0, Point::new(0.0, 0.0));
        assert_eq!(path.point_and_tangent(50.0).0, Point::new(10.0, 0.0));
    }

    #[test]
    fn closed_path_wraps() {
        let path = ArcLengthPath::new(
            &[
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 10.0),
                Point::new(0.0, 10.0),
            ],
            true,
        );
        let total = path.total_length();
        assert_eq!(total, 40.0);
        let (p_zero, _) = path.point_and_tangent(0.0);
        let (p_wrapped, _) = path.point_and_tangent(total + 5.0);
        let (p_direct, _) = path.point_and_tangent(5.0);
        assert_eq!(p_wrapped, p_direct);
        assert_eq!(p_zero, Point::new(0.0, 0.0));
    }

    #[test]
    fn vertical_segment_tangent_points_up() {
        let path = ArcLengthPath::new(&[Point::new(0.0, 0.0), Point::new(0.0, 10.0)], false);
        let (_, angle) = path.point_and_tangent(5.0);
        assert!((angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    }
}
