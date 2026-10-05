//! The liquify brush behind Illustrator's Warp, Twirl, Pucker, Bloat,
//! Scallop, Crystallize and Wrinkle tools. Ported from vectorcraft's
//! `distort/liquify.rs` (MIT/Apache-2.0) onto Amalith's anchor model.
//!
//! A stroke is resampled into dabs spaced a fraction of the brush apart.
//! For each dab the segments the brush touches are first subdivided
//! (Detail: more anchors where the brush passes; straight segments become
//! curves so the result stays smooth), then anchors and handles move by
//! the tool's displacement field, weighted by a smooth `(1 − r²)²`
//! falloff inside the (elliptical, rotated) brush. Scallop, Crystallize
//! and Wrinkle add noise hashed from the dab and anchor indices, so a
//! stroke's result is a pure function of its inputs — replaying it gives
//! exactly the same path.
//!
//! Everything here is in one coordinate space: callers transform a path
//! into document space, apply the stroke, and transform it back.

use crate::geom::{Affine, Point, Rect, Vec2};
use crate::object::{insert_anchor_in, Anchor, HandleMode, PathData, Subpath};
use kurbo::{CubicBez, ParamCurve, ParamCurveArclen, ParamCurveExtrema};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiquifyKind {
    Warp,
    Twirl,
    Pucker,
    Bloat,
    Scallop,
    Crystallize,
    Wrinkle,
}

/// Brush and tool options (Illustrator's Warp Tool Options and friends).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LiquifyParams {
    pub kind: LiquifyKind,
    /// Brush width / height (document units) and rotation in degrees.
    pub width: f64,
    pub height: f64,
    pub angle: f64,
    /// 0..1.
    pub intensity: f64,
    /// 1..10: anchor density added where the brush passes.
    pub detail: f64,
    /// 0..100: removal of redundant flat anchors afterwards.
    pub simplify: f64,
    /// Twirl rate, degrees (−180..180).
    pub rate: f64,
    /// Scallop / Crystallize / Wrinkle complexity (0..15).
    pub complexity: f64,
    /// Wrinkle horizontal / vertical amount (0..1).
    pub horizontal: f64,
    pub vertical: f64,
}

impl LiquifyParams {
    pub fn new(kind: LiquifyKind) -> Self {
        Self {
            kind,
            width: 50.0,
            height: 50.0,
            angle: 0.0,
            intensity: 0.5,
            detail: 2.0,
            simplify: 50.0,
            rate: 40.0,
            complexity: 1.0,
            horizontal: 0.0,
            vertical: 1.0,
        }
    }

    fn radii(&self) -> (f64, f64) {
        (self.width / 2.0, self.height / 2.0)
    }

    /// Spacing between dabs.
    pub fn dab_spacing(&self) -> f64 {
        let (rx, ry) = self.radii();
        (rx.min(ry) * 0.2).max(0.5)
    }

    /// Target segment length inside the brush.
    fn detail_spacing(&self) -> f64 {
        let (rx, ry) = self.radii();
        let extra = match self.kind {
            LiquifyKind::Scallop | LiquifyKind::Crystallize | LiquifyKind::Wrinkle => 1.0 + self.complexity * 0.5,
            _ => 1.0,
        };
        (rx.min(ry) * 2.0 / (self.detail * 2.0 + 1.0) / extra).max(0.5)
    }

    /// Bounding box of the brush at `c`.
    pub fn brush_bounds(&self, c: Point) -> Rect {
        let r = self.width.max(self.height) / 2.0;
        Rect::new(c.x - r, c.y - r, c.x + r, c.y + r)
    }

    /// Falloff weight of `q` for a brush centred at `c` (0 outside).
    pub fn falloff(&self, c: Point, q: Point) -> f64 {
        let (rx, ry) = self.radii();
        let (s, co) = (-self.angle.to_radians()).sin_cos();
        let d = q - c;
        let u = Vec2::new(d.x * co - d.y * s, d.x * s + d.y * co);
        let r2 = (u.x / rx).powi(2) + (u.y / ry).powi(2);
        if r2 >= 1.0 {
            0.0
        } else {
            (1.0 - r2).powi(2)
        }
    }
}

/// Resample a stroke polyline into dabs spaced `spacing` apart (the first
/// point is always a dab).
pub fn dabs(points: &[Point], spacing: f64) -> Vec<Point> {
    let mut out: Vec<Point> = vec![];
    let Some(&first) = points.first() else { return out };
    out.push(first);
    let mut last = first;
    let mut carry = 0.0;
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b - a).hypot();
        if len < 1e-12 {
            continue;
        }
        let mut s = spacing - carry;
        while s <= len {
            last = a.lerp(b, s / len);
            out.push(last);
            s += spacing;
        }
        carry = len - (s - spacing);
        if out.len() > 20_000 {
            break;
        }
    }
    if let Some(&end) = points.last() {
        if (end - last).hypot() > spacing * 0.25 {
            out.push(end);
        }
    }
    out
}

fn hash(mut x: u64) -> u64 {
    // splitmix64
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// Deterministic noise in [-1, 1].
fn noise(a: u64, b: u64, c: u64) -> f64 {
    let h = hash(a.wrapping_mul(0x1000_0000_01b3) ^ hash(b.wrapping_mul(31) ^ hash(c)));
    (h >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

const MAX_ANCHORS: usize = 20_000;

fn segment_count(sp: &Subpath) -> usize {
    let n = sp.anchors.len();
    if sp.closed {
        n
    } else {
        n.saturating_sub(1)
    }
}

fn segment(sp: &Subpath, seg: usize) -> CubicBez {
    let n = sp.anchors.len();
    let (a, b) = (sp.anchors[seg % n], sp.anchors[(seg + 1) % n]);
    CubicBez::new(a.point, a.handle_out.unwrap_or(a.point), b.handle_in.unwrap_or(b.point), b.point)
}

/// Split the segments of `sp` that the brush at `c` touches until they
/// are at most `spacing` long. Straight segments become curves first so
/// the deformation stays smooth.
fn subdivide(sp: &mut Subpath, prm: &LiquifyParams, c: Point, spacing: f64) {
    let bb = prm.brush_bounds(c);
    let mut seg = 0;
    while seg < segment_count(sp) {
        if sp.anchors.len() >= MAX_ANCHORS {
            return;
        }
        let cub = segment(sp, seg);
        let sb = cub.bounding_box();
        let touched = sb.intersect(bb).area() > 0.0
            || (sb.width() == 0.0 || sb.height() == 0.0) && sb.inflate(1e-6, 1e-6).intersect(bb).area() > 0.0;
        if !touched || !(0..=8).any(|i| prm.falloff(c, cub.eval(i as f64 / 8.0)) > 0.0) {
            seg += 1;
            continue;
        }
        let k = (cub.arclen(1e-3) / spacing).ceil() as usize;
        if k <= 1 {
            seg += 1;
            continue;
        }
        let n = sp.anchors.len();
        let (i0, i1) = (seg % n, (seg + 1) % n);
        if sp.anchors[i0].handle_out.is_none() && sp.anchors[i1].handle_in.is_none() {
            let (a, b) = (sp.anchors[i0].point, sp.anchors[i1].point);
            sp.anchors[i0].handle_out = Some(a.lerp(b, 1.0 / 3.0));
            sp.anchors[i1].handle_in = Some(a.lerp(b, 2.0 / 3.0));
        }
        // Split into k equal-parameter pieces: at 1/k, then 1/(k-1) of the
        // rest, …
        let mut cur = seg;
        for j in 0..k - 1 {
            let t = 1.0 / (k - j) as f64;
            match insert_anchor_in(std::slice::from_mut(sp), 0, cur, t) {
                Some(i) => cur = i,
                None => break,
            }
        }
        seg = cur + 1;
    }
}

/// Displace one point `q`; `anchor_new` is the already-displaced anchor
/// when `q` is one of its handles (Scallop / Crystallize move handles
/// relative to it).
#[allow(clippy::too_many_arguments)]
fn displace(
    prm: &LiquifyParams,
    c: Point,
    prev: Point,
    dab: u64,
    key: u64,
    q: Point,
    anchor_new: Option<Point>,
    anchor_old: Point,
) -> Point {
    let f = prm.falloff(c, q);
    let i = prm.intensity;
    let (rx, ry) = prm.radii();
    let r = rx.max(ry);
    let d = q - c;
    let len = d.hypot();
    let dir = if len > 1e-9 { d / len } else { Vec2::ZERO };
    match prm.kind {
        LiquifyKind::Warp => q + (c - prev) * (i * f),
        LiquifyKind::Twirl => {
            let a = prm.rate.to_radians() * i * f * 0.25;
            let (s, co) = a.sin_cos();
            c + Vec2::new(d.x * co - d.y * s, d.x * s + d.y * co)
        }
        LiquifyKind::Pucker => q - d * (i * f * 0.2),
        LiquifyKind::Bloat => q + dir * (i * f * 0.1 * r),
        LiquifyKind::Scallop => match anchor_new {
            None => q - d * (i * f * 0.12),
            Some(a) => {
                // Handles swing sideways (curls) and stretch: arc-like
                // details along the outline.
                let h = q - anchor_old;
                let phi = noise(dab, key, 7) * std::f64::consts::FRAC_PI_2 * i * f * (1.0 + prm.complexity * 0.2);
                let (s, co) = phi.sin_cos();
                a + Vec2::new(h.x * co - h.y * s, h.x * s + h.y * co) * (1.0 + 0.6 * i * f)
            }
        },
        LiquifyKind::Crystallize => match anchor_new {
            None => q + dir * (i * f * 0.1 * r * (0.5 + 0.5 * noise(dab, key, 3).abs())),
            // Handles are pulled in: spikes.
            Some(a) => a + (q - anchor_old) * (1.0 - 0.6 * i * f),
        },
        LiquifyKind::Wrinkle => {
            let amp = i * f * 0.06 * r * (1.0 + prm.complexity * 0.1);
            q + Vec2::new(noise(dab, key, 11) * prm.horizontal * amp, noise(dab, key, 13) * prm.vertical * amp)
        }
    }
}

/// Apply one dab to a subpath (after subdivision).
fn apply_dab(sp: &mut Subpath, prm: &LiquifyParams, c: Point, prev: Point, dab: u64, salt: u64) -> bool {
    let bb = prm.brush_bounds(c);
    let mut changed = false;
    for (ai, a) in sp.anchors.iter_mut().enumerate() {
        let old = *a;
        let (h_in, h_out) = (old.handle_in.unwrap_or(old.point), old.handle_out.unwrap_or(old.point));
        if !bb.contains(old.point) && !bb.contains(h_in) && !bb.contains(h_out) {
            continue;
        }
        let key = hash(salt ^ (ai as u64).wrapping_mul(0x9e37_79b9));
        let np = displace(prm, c, prev, dab, key, old.point, None, old.point);
        let handle = |h: Option<Point>, k: u64| -> Option<Point> {
            let h = h?;
            if (h - old.point).hypot() < 1e-12 {
                return Some(np);
            }
            Some(match prm.kind {
                LiquifyKind::Scallop | LiquifyKind::Crystallize => {
                    displace(prm, c, prev, dab, key ^ k, h, Some(np), old.point)
                }
                _ => displace(prm, c, prev, dab, key ^ k, h, None, old.point),
            })
        };
        let new = Anchor {
            point: np,
            handle_in: handle(old.handle_in, 1),
            handle_out: handle(old.handle_out, 2),
            mode: if prm.kind == LiquifyKind::Crystallize { HandleMode::Corner } else { old.mode },
        };
        if new != old {
            changed = true;
            *a = new;
        }
    }
    changed
}

/// Distance from `p` to segment `ab`.
fn dist_seg(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let l2 = ab.hypot2();
    if l2 < 1e-18 {
        return (p - a).hypot();
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (p - (a + ab * t)).hypot()
}

/// Remove anchors inside `region` lying flat between their neighbours
/// (every control point within `tol` of the chord).
fn simplify(sp: &mut Subpath, tol: f64, region: Rect) {
    if tol <= 0.0 {
        return;
    }
    let mut i = 1;
    loop {
        let n = sp.anchors.len();
        if n < if sp.closed { 4 } else { 3 } {
            return;
        }
        let last = if sp.closed { n } else { n - 1 };
        if i >= last {
            return;
        }
        let (pa, k, pb) = (sp.anchors[i - 1], sp.anchors[i], sp.anchors[(i + 1) % n]);
        let pa_out = pa.handle_out.unwrap_or(pa.point);
        let pb_in = pb.handle_in.unwrap_or(pb.point);
        let flat = region.contains(k.point)
            && [k.point, k.handle_in.unwrap_or(k.point), k.handle_out.unwrap_or(k.point), pa_out, pb_in]
                .iter()
                .all(|q| dist_seg(*q, pa.point, pb.point) <= tol)
            && (k.point - pa.point).dot(pb.point - k.point) > 0.0;
        if flat {
            let lab = (pb.point - pa.point).hypot();
            let lak = (k.point - pa.point).hypot().max(1e-9);
            let lkb = (pb.point - k.point).hypot().max(1e-9);
            sp.anchors[i - 1].handle_out = pa.handle_out.map(|h| pa.point + (h - pa.point) * (lab / lak));
            sp.anchors[(i + 1) % n].handle_in = pb.handle_in.map(|h| pb.point + (h - pb.point) * (lab / lkb));
            sp.anchors.remove(i);
        } else {
            i += 1;
        }
    }
}

fn subpath_bounds(sp: &Subpath) -> Option<Rect> {
    let mut it = sp.anchors.iter().flat_map(|a| [Some(a.point), a.handle_in, a.handle_out].into_iter().flatten());
    let f = it.next()?;
    Some(it.fold(Rect::from_points(f, f), |r, p| r.union_pt(p)))
}

/// Apply a liquify stroke (already resampled with [`dabs`]) to
/// `subpaths`. `salt` decorrelates the noise between paths. Returns
/// whether anything moved.
pub fn apply_stroke(subpaths: &mut [Subpath], dab_pts: &[Point], prm: &LiquifyParams, salt: u64) -> bool {
    let spacing = prm.detail_spacing();
    let mut changed = false;
    let mut region: Option<Rect> = None;
    for (si, sp) in subpaths.iter_mut().enumerate() {
        let salt = hash(salt ^ (si as u64 + 1).wrapping_mul(0x1234_5678_9abc_def1));
        for (di, c) in dab_pts.iter().enumerate() {
            let prev = if di == 0 { *c } else { dab_pts[di - 1] };
            let bb = prm.brush_bounds(*c);
            let Some(spb) = subpath_bounds(sp) else { continue };
            if spb.intersect(bb).area() <= 0.0 && !(spb.width() == 0.0 || spb.height() == 0.0) {
                continue;
            }
            subdivide(sp, prm, *c, spacing);
            if apply_dab(sp, prm, *c, prev, di as u64, salt) {
                changed = true;
                region = Some(region.map_or(bb, |r| r.union(bb)));
            }
        }
    }
    if let (true, Some(r)) = (changed, region) {
        let tol = prm.simplify / 100.0 * spacing * 0.02;
        for sp in subpaths.iter_mut() {
            simplify(sp, tol, r);
        }
    }
    changed
}

/// Applies a stroke (document space, already resampled with [`dabs`]) to
/// a path whose local space maps to document space by `to_doc`. `None`
/// when the brush moved nothing.
pub fn liquify_path(path: &PathData, to_doc: Affine, dab_pts: &[Point], prm: &LiquifyParams, salt: u64) -> Option<PathData> {
    let map = |sp: &mut [Subpath], m: Affine| {
        for a in sp.iter_mut().flat_map(|s| s.anchors.iter_mut()) {
            a.point = m * a.point;
            a.handle_in = a.handle_in.map(|h| m * h);
            a.handle_out = a.handle_out.map(|h| m * h);
        }
    };
    let mut out = path.clone();
    let changed = out.edit_subpaths_ret(|sp| {
        map(sp, to_doc);
        let changed = apply_stroke(sp, dab_pts, prm, salt);
        map(sp, to_doc.inverse());
        changed
    });
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Subpath> {
        let pts = [(0.0, 0.0), (200.0, 0.0), (200.0, 200.0), (0.0, 200.0)];
        vec![Subpath { anchors: pts.iter().map(|&(x, y)| Anchor::corner(Point::new(x, y))).collect(), closed: true }]
    }

    fn prm(kind: LiquifyKind) -> LiquifyParams {
        LiquifyParams { intensity: 1.0, width: 100.0, height: 100.0, ..LiquifyParams::new(kind) }
    }

    fn bounds(sp: &[Subpath]) -> Rect {
        sp.iter().filter_map(subpath_bounds).reduce(|a, b| a.union(b)).unwrap()
    }

    #[test]
    fn dabs_are_evenly_spaced() {
        let d = dabs(&[Point::new(0.0, 0.0), Point::new(100.0, 0.0)], 10.0);
        assert_eq!(d.len(), 11);
        assert!(d.windows(2).all(|w| ((w[1] - w[0]).hypot() - 10.0).abs() < 1e-9));
        assert_eq!(dabs(&[Point::new(5.0, 5.0)], 10.0), vec![Point::new(5.0, 5.0)]);
    }

    #[test]
    fn warp_is_deterministic_and_localized() {
        let p = prm(LiquifyKind::Warp);
        let d = dabs(&[Point::new(200.0, 100.0), Point::new(240.0, 100.0)], p.dab_spacing());
        let (mut a, mut b) = (square(), square());
        assert!(apply_stroke(&mut a, &d, &p, 7));
        apply_stroke(&mut b, &d, &p, 7);
        assert_eq!(a, b, "same input, same output");
        assert!(bounds(&a).x1 > 210.0, "the right edge bulged outward");
        assert!(a[0].anchors.len() > 4, "detail added anchors");
        assert!(a[0].anchors.iter().any(|an| an.point == Point::new(0.0, 0.0)), "far anchors stay put");
    }

    #[test]
    fn pucker_pulls_in_and_bloat_pushes_out() {
        let mut a = square();
        apply_stroke(&mut a, &[Point::new(200.0, 100.0)], &prm(LiquifyKind::Pucker), 1);
        assert!(bounds(&a).x1 <= 200.0 + 1e-9);
        let mut b = square();
        apply_stroke(&mut b, &[Point::new(180.0, 100.0); 3], &prm(LiquifyKind::Bloat), 1);
        let bb = bounds(&b);
        assert!(bb.x1 > 205.0, "{bb:?}");
        assert_eq!((bb.x0, bb.y0, bb.y1), (0.0, 0.0, 200.0));
    }

    #[test]
    fn twirl_rotates_about_the_brush_centre() {
        let c = Point::new(100.0, 100.0);
        let mut sp = vec![Subpath {
            anchors: vec![Anchor::corner(Point::new(80.0, 100.0)), Anchor::corner(Point::new(120.0, 100.0))],
            closed: false,
        }];
        let p = LiquifyParams { detail: 1.0, simplify: 0.0, ..prm(LiquifyKind::Twirl) };
        apply_stroke(&mut sp, &[c], &p, 0);
        for a in &sp[0].anchors {
            let d = (a.point - c).hypot();
            assert!((d - 20.0).abs() < 1e-9 || d < 1e-9, "{d}");
        }
        assert_ne!(sp[0].anchors[0].point.y, 100.0);
    }

    #[test]
    fn noisy_tools_are_reproducible() {
        for kind in [LiquifyKind::Scallop, LiquifyKind::Crystallize, LiquifyKind::Wrinkle] {
            let p = LiquifyParams { horizontal: 1.0, ..prm(kind) };
            let d = dabs(&[Point::new(200.0, 50.0), Point::new(200.0, 150.0)], p.dab_spacing());
            let (mut a, mut b) = (square(), square());
            assert!(apply_stroke(&mut a, &d, &p, 3), "{kind:?}");
            apply_stroke(&mut b, &d, &p, 3);
            assert_eq!(a, b, "{kind:?}");
            assert_ne!(a, square());
        }
    }
}
