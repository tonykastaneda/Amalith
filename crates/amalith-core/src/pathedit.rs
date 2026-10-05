//! Anchor-model edits behind the path-editing tools that live beside the
//! Pen: Scissors (cut a path open or in two), Delete Anchor Point (drop
//! a point but keep the shape closed), Anchor Point (pull symmetric
//! handles out of a point), Curvature (a smooth curve through clicked
//! points) and Reshape (drag a point on a path, its neighbours following
//! with a soft falloff).
//!
//! Same conventions as the rest of the anchor model in [`crate::object`]:
//! anchors are addressed by flat ordinal across every subpath, segments
//! by flat segment ordinal, everything in the object's local space.

use crate::geom::{Point, Vec2};
use crate::object::{insert_anchor, locate, refit_across, Anchor, HandleMode, Subpath};
use kurbo::{CubicBez, ParamCurveNearest};

/// What a [`split_at_anchor`] / [`split_at_segment`] cut did.
#[derive(Debug, Clone, PartialEq)]
pub enum Split {
    /// A closed subpath was cut open at the point — still one subpath,
    /// now starting and ending at the cut.
    Opened,
    /// An open subpath was cut in two: the subpath keeps everything up to
    /// the cut, and this is everything after it (to become its own
    /// object, Illustrator's Scissors behaviour).
    Detached(Subpath),
}

/// Cuts the path at anchor `n` (flat ordinal). A closed subpath opens
/// there (the anchor is duplicated so both new free ends sit on it); an
/// open subpath splits in two at an interior anchor. `None` when there's
/// nothing to cut — an out-of-range ordinal, an open path's own free end,
/// or a subpath too short to cut.
pub fn split_at_anchor(subpaths: &mut [Subpath], n: usize) -> Option<Split> {
    let (si, ai) = locate(subpaths, n)?;
    let sp = &mut subpaths[si];
    let m = sp.anchors.len();
    if m < 2 {
        return None;
    }
    if sp.closed {
        sp.anchors.rotate_left(ai);
        let mut end = sp.anchors[0];
        end.handle_out = None;
        sp.anchors[0].handle_in = None;
        sp.anchors.push(end);
        sp.closed = false;
        return Some(Split::Opened);
    }
    if ai == 0 || ai + 1 == m {
        return None;
    }
    let mut tail: Vec<Anchor> = sp.anchors.split_off(ai);
    let mut head_end = tail[0];
    head_end.handle_out = None;
    sp.anchors.push(head_end);
    tail[0].handle_in = None;
    Some(Split::Detached(Subpath { anchors: tail, closed: false }))
}

/// Cuts the path at parameter `t` of segment `seg` (flat segment
/// ordinal): a new anchor goes in there (keeping the curve) and the path
/// is cut at it, as [`split_at_anchor`].
pub fn split_at_segment(subpaths: &mut [Subpath], seg: usize, t: f64) -> Option<Split> {
    let n = insert_anchor(subpaths, seg, t)?;
    split_at_anchor(subpaths, n)
}

/// The Delete Anchor Point tool: removes anchor `n` and refits its
/// neighbours' facing handles so the one segment left approximates the
/// two it replaces. Unlike [`crate::delete_anchor`] (the Delete key,
/// which deliberately opens a closed shape), a closed subpath stays
/// closed. Subpaths that fall below two anchors are dropped.
pub fn remove_anchor(subpaths: &mut Vec<Subpath>, n: usize) {
    let Some((si, ai)) = locate(subpaths, n) else {
        return;
    };
    let sp = &mut subpaths[si];
    let m = sp.anchors.len();
    if sp.closed && m >= 3 {
        refit_across(sp, (ai + m - 1) % m, ai, (ai + 1) % m);
    } else if !sp.closed && ai > 0 && ai + 1 < m {
        refit_across(sp, ai - 1, ai, ai + 1);
    } else if !sp.closed && m > 2 {
        // Dropping a free end: the neighbour becomes the new end, so its
        // now-dangling handle goes too.
        if ai == 0 {
            sp.anchors[1].handle_in = None;
        } else {
            sp.anchors[m - 2].handle_out = None;
        }
    }
    sp.anchors.remove(ai);
    subpaths.retain(|s| s.anchors.len() >= 2);
}

/// The Anchor Point tool's drag off an anchor: anchor `n` becomes a
/// symmetric smooth point whose out-handle sits at `handle_out` and whose
/// in-handle mirrors it. A handle dragged back onto the anchor itself
/// leaves a plain corner.
pub fn pull_anchor_handles(subpaths: &mut [Subpath], n: usize, handle_out: Point) {
    let Some((si, ai)) = locate(subpaths, n) else {
        return;
    };
    let a = &mut subpaths[si].anchors[ai];
    if (handle_out - a.point).hypot() < 1e-9 {
        a.handle_in = None;
        a.handle_out = None;
        a.mode = HandleMode::Corner;
        return;
    }
    a.handle_out = Some(handle_out);
    a.handle_in = Some(a.point + (a.point - handle_out));
    a.mode = HandleMode::Symmetric;
}

/// One Curvature tool point: where it is, and whether it's a corner
/// (Alt-click / double-click) rather than a smooth point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurvaturePoint {
    pub point: Point,
    pub corner: bool,
}

/// A smooth curve through `points` — the Curvature tool's whole model.
/// Each smooth point gets Catmull-Rom tangents from its two neighbours
/// (handles a sixth of the neighbour-to-neighbour chord, so the curve
/// passes through every point); corners, an open path's two ends, and
/// anything with fewer than three points get no handles. Every anchor is
/// tagged [`HandleMode::Corner`] or [`HandleMode::Smooth`] by its point's
/// own flag — even a handle-less end, so a smooth end stays smooth once
/// the path grows past it — which is how [`curvature_points`] reads the
/// model back off a path.
pub fn curvature_subpath(points: &[CurvaturePoint], closed: bool) -> Subpath {
    let n = points.len();
    let closed = closed && n > 2;
    let anchors = (0..n)
        .map(|i| {
            let p = points[i].point;
            let bare = Anchor {
                mode: if points[i].corner { HandleMode::Corner } else { HandleMode::Smooth },
                ..Anchor::corner(p)
            };
            let end = !closed && (i == 0 || i + 1 == n);
            if points[i].corner || end || n < 3 {
                return bare;
            }
            let prev = points[(i + n - 1) % n].point;
            let next = points[(i + 1) % n].point;
            let t = (next - prev) / 6.0;
            if t.hypot() < 1e-9 {
                return bare;
            }
            Anchor { point: p, handle_in: Some(p - t), handle_out: Some(p + t), mode: HandleMode::Smooth }
        })
        .collect();
    Subpath { anchors, closed }
}

/// Reads a subpath back as Curvature points: every anchor is a point, and
/// it's a corner exactly when its [`HandleMode`] is `Corner`.
pub fn curvature_points(sp: &Subpath) -> Vec<CurvaturePoint> {
    sp.anchors
        .iter()
        .map(|a| CurvaturePoint { point: a.point, corner: a.mode == HandleMode::Corner })
        .collect()
}

/// The point on the path nearest `p`: `(subpath, segment within that
/// subpath, t, distance)`.
pub fn nearest_on_subpaths(subpaths: &[Subpath], p: Point) -> Option<(usize, usize, f64, f64)> {
    let mut best: Option<(usize, usize, f64, f64)> = None;
    for (si, sp) in subpaths.iter().enumerate() {
        let m = sp.anchors.len();
        let segs = if sp.closed { m } else { m.saturating_sub(1) };
        for li in 0..segs {
            let a = sp.anchors[li];
            let b = sp.anchors[(li + 1) % m];
            let c = CubicBez::new(a.point, a.handle_out.unwrap_or(a.point), b.handle_in.unwrap_or(b.point), b.point);
            let near = c.nearest(p, 1e-6);
            let d = near.distance_sq.sqrt();
            if best.is_none_or(|(.., bd)| d < bd) {
                best = Some((si, li, near.t, d));
            }
        }
    }
    best
}

/// The Reshape tool: grabs the path at `at` — its nearest anchor if one
/// is within `tolerance`, else a new anchor inserted at the nearest point
/// on the curve — and moves it by `delta`. Every other anchor of that
/// subpath follows with a smooth `(1 − r²)²` falloff over half the
/// subpath's bounding diagonal, so the shape bends rather than kinks.
/// Returns `false` (and changes nothing) for an empty path.
pub fn reshape(subpaths: &mut [Subpath], at: Point, delta: Vec2, tolerance: f64) -> bool {
    let Some((si, li, t, _)) = nearest_on_subpaths(subpaths, at) else {
        return false;
    };
    let nearest_anchor = subpaths[si]
        .anchors
        .iter()
        .enumerate()
        .map(|(i, a)| (i, (a.point - at).hypot()))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let grabbed = match nearest_anchor {
        Some((i, d)) if d <= tolerance => i,
        _ => match crate::object::insert_anchor_in(subpaths, si, li, t) {
            Some(i) => i,
            None => return false,
        },
    };
    let sp = &mut subpaths[si];
    let origin = sp.anchors[grabbed].point;
    let bounds = sp
        .anchors
        .iter()
        .fold(kurbo::Rect::from_points(origin, origin), |r, a| r.union_pt(a.point));
    let radius = (bounds.width().hypot(bounds.height()) / 2.0).max(1.0);
    for (i, a) in sp.anchors.iter_mut().enumerate() {
        let w = if i == grabbed {
            1.0
        } else {
            let q = ((a.point - origin).hypot() / radius).min(1.0);
            (1.0 - q * q).powi(2)
        };
        let v = delta * w;
        a.point += v;
        if let Some(h) = &mut a.handle_in {
            *h += v;
        }
        if let Some(h) = &mut a.handle_out {
            *h += v;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(points: &[(f64, f64)]) -> Subpath {
        Subpath { anchors: points.iter().map(|&(x, y)| Anchor::corner(Point::new(x, y))).collect(), closed: false }
    }

    #[test]
    fn closed_cut_opens_at_the_anchor() {
        let mut sp = vec![Subpath { closed: true, ..open(&[(0., 0.), (10., 0.), (10., 10.), (0., 10.)]) }];
        assert_eq!(split_at_anchor(&mut sp, 2), Some(Split::Opened));
        assert!(!sp[0].closed);
        let pts: Vec<_> = sp[0].anchors.iter().map(|a| (a.point.x, a.point.y)).collect();
        assert_eq!(pts, vec![(10., 10.), (0., 10.), (0., 0.), (10., 0.), (10., 10.)]);
    }

    #[test]
    fn open_cut_detaches_the_tail_and_ends_do_nothing() {
        let mut sp = vec![open(&[(0., 0.), (10., 0.), (20., 0.)])];
        assert_eq!(split_at_anchor(&mut sp, 0), None);
        assert_eq!(split_at_anchor(&mut sp, 2), None);
        let Some(Split::Detached(tail)) = split_at_anchor(&mut sp, 1) else { panic!("expected a detached tail") };
        assert_eq!(sp[0].anchors.len(), 2);
        assert_eq!(tail.anchors.len(), 2);
        assert_eq!(sp[0].anchors[1].point, tail.anchors[0].point);
    }

    #[test]
    fn remove_keeps_a_closed_shape_closed() {
        let mut sp = vec![Subpath { closed: true, ..open(&[(0., 0.), (10., 0.), (10., 10.), (0., 10.)]) }];
        remove_anchor(&mut sp, 0);
        assert!(sp[0].closed);
        assert_eq!(sp[0].anchors.len(), 3);
    }

    #[test]
    fn curvature_passes_through_every_point() {
        let pts = [(0., 0.), (50., 50.), (100., 0.)]
            .map(|(x, y)| CurvaturePoint { point: Point::new(x, y), corner: false });
        let sp = curvature_subpath(&pts, false);
        assert_eq!(sp.anchors[1].point, Point::new(50., 50.));
        assert_eq!(sp.anchors[1].mode, HandleMode::Smooth);
        // Tangent parallel to first → last.
        assert!((sp.anchors[1].handle_out.unwrap().y - 50.0).abs() < 1e-9);
        assert!(sp.anchors[0].handle_out.is_none());
        assert_eq!(curvature_points(&sp), pts.to_vec());
    }

    #[test]
    fn reshape_moves_the_grabbed_point_fully_and_far_points_less() {
        let mut sp = vec![open(&[(0., 0.), (50., 0.), (100., 0.)])];
        assert!(reshape(&mut sp, Point::new(50., 0.), Vec2::new(0., 20.), 1.0));
        assert_eq!(sp[0].anchors[1].point, Point::new(50., 20.));
        assert!(sp[0].anchors[0].point.y < 20.0);
    }
}
