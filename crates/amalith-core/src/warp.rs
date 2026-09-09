//! Quad-to-quad projective transform (homography) — the math behind Free
//! Transform's Perspective Distort and Free Distort modes. Neither mode
//! is affine (an object's four corners don't stay a parallelogram), so
//! unlike every other transform tool in this codebase they can't just
//! premultiply an `Affine` onto `Object::transform` — they instead push
//! every path anchor and bezier handle through the homography solved
//! here (see `amalith_commands::Command::WarpPaths`, and [`warp_path_
//! data`], which both that command and the shell's live preview call so
//! the two can never disagree on what a warp produces).

use crate::geom::{Affine, Point};
use crate::object::PathData;

/// A 2D projective transform: the full 3x3 homogeneous matrix `[a b c;
/// d e f; g h i]`, acting on `(x, y, 1)` as `((a·x + b·y + c) / w, (d·x +
/// e·y + f) / w)` where `w = g·x + h·y + i`. An affine transform is the
/// special case `g = h = 0, i = 1`.
///
/// The bottom-right entry `i` is stored explicitly rather than assumed
/// to be `1` — [`Self::conjugate`] can produce a matrix whose natural
/// normalization has `i = 0` (the conjugated map sends the local origin
/// to the line at infinity, which happens for perfectly ordinary corner
/// drags once an object's own world transform is folded in), and forcing
/// a `/ 1` fallback in that case silently substitutes a wrong matrix
/// instead of the right one. Carrying `i` generally sidesteps needing a
/// fallback at all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Homography([f64; 9]);

impl Homography {
    /// The identity map (every point maps to itself).
    pub const IDENTITY: Self = Self([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);

    /// Maps four corresponding corners, returning identity for a singular map.
    pub fn solve(src: [Point; 4], dst: [Point; 4]) -> Self {
        Self::try_solve(src, dst).unwrap_or(Self::IDENTITY)
    }

    /// Checked solve for interactive edits. Normalize coordinates before
    /// elimination to avoid document offsets and tiny selections degrading pivots.
    pub fn try_solve(src: [Point; 4], dst: [Point; 4]) -> Option<Self> {
        let normalize = |q: [Point; 4]| -> Option<Affine> {
            if q.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) { return None; }
            let center = Point::new(q.iter().map(|p| p.x / 4.0).sum(), q.iter().map(|p| p.y / 4.0).sum());
            let radius = q.iter().map(|p| (*p - center).hypot()).fold(0.0, f64::max);
            if radius == 0.0 || !radius.is_finite() { return None; }
            Some(Affine::scale(1.0 / radius) * Affine::translate(-center.to_vec2()))
        };
        let pre = normalize(src)?;
        let dest = normalize(dst)?;
        let src = src.map(|p| pre * p);
        let dst = dst.map(|p| dest * p);
        // Each correspondence (x, y) -> (X, Y) contributes two rows to
        // the 8x8 linear system A * [a b c d e f g h]^T = B, solving with
        // the bottom-right entry fixed to 1 — the standard normalization
        // for a homography built directly from finite point
        // correspondences (as opposed to one built by composing/
        // conjugating existing matrices, which is what needs the general
        // 9-coefficient form above).
        let mut a = [[0.0f64; 8]; 8];
        let mut b = [0.0f64; 8];
        for i in 0..4 {
            let (x, y) = (src[i].x, src[i].y);
            let (xp, yp) = (dst[i].x, dst[i].y);
            let r0 = 2 * i;
            let r1 = r0 + 1;
            a[r0] = [x, y, 1.0, 0.0, 0.0, 0.0, -x * xp, -y * xp];
            b[r0] = xp;
            a[r1] = [0.0, 0.0, 0.0, x, y, 1.0, -x * yp, -y * yp];
            b[r1] = yp;
        }
        let [a, b, c, d, e, f, g, h] = gauss_solve(a, b)?;
        let result = Self([a, b, c, d, e, f, g, h, 1.0]).conjugate(pre, dest.inverse());
        result.0.iter().all(|v| v.is_finite()).then_some(result)
    }

    /// Applies the map, retaining the input when it lies on the horizon.
    /// Editing code uses `try_apply` so invalid geometry cannot be committed.
    pub fn apply(&self, p: Point) -> Point {
        self.try_apply(p).unwrap_or(p)
    }

    pub fn try_apply(&self, p: Point) -> Option<Point> {
        let [a,b,c,d,e,f,g,h,i] = self.0;
        let w = g * p.x + h * p.y + i;
        let scale = (g * p.x).abs() + (h * p.y).abs() + i.abs();
        if !w.is_finite() || w.abs() <= 1e-12 * scale { return None; }
        let q = Point::new((a*p.x+b*p.y+c)/w, (d*p.x+e*p.y+f)/w);
        (q.x.is_finite() && q.y.is_finite()).then_some(q)
    }

    /// Shared by live preview and command compilation: preserves editable
    /// anchors, handles, contour closure and width data. Fails atomically.
    pub fn warp_path(&self, path: &crate::PathData) -> Option<crate::PathData> {
        let mut subpaths = path.subpaths().to_vec();
        for sp in &mut subpaths {
            for anchor in &mut sp.anchors {
                anchor.point = self.try_apply(anchor.point)?;
                if let Some(p) = &mut anchor.handle_in { *p = self.try_apply(*p)?; }
                if let Some(p) = &mut anchor.handle_out { *p = self.try_apply(*p)?; }
            }
        }
        let mut result = path.clone();
        result.edit_subpaths(|s| *s = subpaths);
        Some(result)
    }

    /// Composes `post ∘ self ∘ pre`, where `pre`/`post` are affine maps
    /// applied before/after this homography. Used to re-express a
    /// homography solved in one coordinate space (document space, where
    /// a corner drag naturally lives) as the equivalent homography in
    /// another (an object's own local space, where `PathData`'s anchors
    /// actually live): `pre` = local-to-document (the object's world
    /// transform), `post` = document-to-local (its inverse).
    pub fn conjugate(&self, pre: Affine, post: Affine) -> Self {
        Self::from_mat3(mat3_mul(mat3_mul(affine_mat3(post), self.to_mat3()), affine_mat3(pre)))
    }

    fn to_mat3(self) -> [[f64; 3]; 3] {
        let [a,b,c,d,e,f,g,h,i] = self.0;
        [[a,b,c],[d,e,f],[g,h,i]]
    }

    fn from_mat3(m: [[f64; 3]; 3]) -> Self {
        Self([m[0][0],m[0][1],m[0][2],m[1][0],m[1][1],m[1][2],m[2][0],m[2][1],m[2][2]])
    }
}

/// An affine map as a 3x3 homogeneous matrix — the subgroup of
/// [`Homography`] with a fixed `[0 0 1]` bottom row.
fn affine_mat3(a: Affine) -> [[f64; 3]; 3] {
    let c = a.as_coeffs(); // kurbo order: xx, yx, xy, yy, x0, y0
    [[c[0], c[2], c[4]], [c[1], c[3], c[5]], [0.0, 0.0, 1.0]]
}

fn mat3_mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut r = [[0.0; 3]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

/// Solves the 8x8 linear system `a * x = b` by Gauss-Jordan elimination
/// with partial pivoting. `None` if `a` is singular (to working
/// precision).
fn gauss_solve(mut a: [[f64; 8]; 8], mut b: [f64; 8]) -> Option<[f64; 8]> {
    for col in 0..8 {
        let (pivot, _) = (col..8)
            .map(|r| (r, a[r][col].abs()))
            .max_by(|x, y| x.1.total_cmp(&y.1))?;
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let d = a[col][col];
        for k in col..8 {
            a[col][k] /= d;
        }
        b[col] /= d;
        for r in 0..8 {
            if r == col {
                continue;
            }
            let f = a[r][col];
            if f != 0.0 {
                for k in col..8 {
                    a[r][k] -= f * a[col][k];
                }
                b[r] -= f * b[col];
            }
        }
    }
    Some(b)
}

/// Pushes `data`'s every anchor point and bezier handle through `h`,
/// returning the warped `PathData`. The one and only place this warp
/// actually gets applied to real geometry — `Command::WarpPaths` calls
/// this to build the committed result, and the shell's live preview
/// calls it too (on a per-frame homography derived the same way,
/// conjugated by the same object's world transform) so the preview can
/// never diverge from what release will actually commit. That
/// divergence is real if the two ever warp geometry two different ways
/// (e.g. warping a flattened polyline for preview vs. warping control
/// points for the commit) — a homography doesn't distribute over Bezier
/// interpolation, so the projective image of a curve isn't the curve
/// through the projected control points' *flattening*, only through the
/// control points themselves.
pub fn warp_path_data(data: &PathData, h: &Homography) -> PathData {
    h.warp_path(data).unwrap_or_else(|| data.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: Point, b: Point) {
        assert!((a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6, "{a:?} != {b:?}");
    }

    #[test]
    fn identity_quad_round_trips() {
        let quad = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let h = Homography::solve(quad, quad);
        for p in [Point::new(3.0, 7.0), Point::new(-2.0, 12.0), Point::new(5.0, 5.0)] {
            approx_eq(h.apply(p), p);
        }
    }

    #[test]
    fn matches_pure_affine_scale_translate() {
        let src = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        // Scale 2x and translate by (5, 5) — still a plain affine map.
        let dst = src.map(|p| Point::new(p.x * 2.0 + 5.0, p.y * 2.0 + 5.0));
        let h = Homography::solve(src, dst);
        for p in [Point::new(3.0, 7.0), Point::new(6.0, 2.0)] {
            approx_eq(h.apply(p), Point::new(p.x * 2.0 + 5.0, p.y * 2.0 + 5.0));
        }
    }

    #[test]
    fn genuine_perspective_maps_corners_exactly() {
        let src = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        // A keystone trapezoid: top edge narrowed toward the center.
        let dst = [
            Point::new(2.0, 0.0),
            Point::new(8.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let h = Homography::solve(src, dst);
        for i in 0..4 {
            approx_eq(h.apply(src[i]), dst[i]);
        }
    }

    #[test]
    fn conjugate_with_identity_affines_is_a_no_op() {
        let src = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let dst = [
            Point::new(2.0, 0.0),
            Point::new(8.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let h = Homography::solve(src, dst);
        let conjugated = h.conjugate(Affine::IDENTITY, Affine::IDENTITY);
        for p in [Point::new(3.0, 7.0), Point::new(6.0, 2.0)] {
            approx_eq(h.apply(p), conjugated.apply(p));
        }
    }

    #[test]
    fn conjugate_matches_manual_local_to_document_round_trip() {
        let src = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let dst = [
            Point::new(2.0, 0.0),
            Point::new(8.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let h_doc = Homography::solve(src, dst);
        // An object rotated 30° about the origin, offset by (5, -3).
        let world = Affine::translate((5.0, -3.0)) * Affine::rotate(30f64.to_radians());
        let h_local = h_doc.conjugate(world, world.inverse());
        let local_p = Point::new(4.0, 6.0);
        let expected = world.inverse() * h_doc.apply(world * local_p);
        approx_eq(h_local.apply(local_p), expected);
    }

    /// Regression for a real bug: conjugating with a world transform
    /// whose translation, combined with this homography's `g`/`h`,
    /// makes the naturally-composed matrix's bottom-right entry zero
    /// (the local origin maps to the line at infinity). An 8-coefficient
    /// representation that assumes that entry is always 1 has no way to
    /// represent this and silently produces a wrong map instead; the
    /// general 9-coefficient form just carries the zero through.
    #[test]
    fn conjugate_handles_a_zero_bottom_right_entry() {
        // h = -0.02 so that, composed with a (0, 50) world translation
        // below, the resulting bottom-right entry is exactly h*50+1 = 0.
        let h_doc = Homography([0.8, -0.2, 2.0, 0.0, 0.8, 0.0, 0.0, -0.02, 1.0]);
        let world = Affine::translate((0.0, 50.0));
        let h_local = h_doc.conjugate(world, world.inverse());
        let local_p = Point::new(4.0, -45.0);
        let expected = world.inverse() * h_doc.apply(world * local_p);
        approx_eq(h_local.apply(local_p), expected);
    }
    #[test]
    fn normalized_solve_handles_small_and_distant_quads() {
        for (offset, size) in [(0.0, 1e-8), (1e5, 10.0)] {
            let src = [Point::new(offset,offset),Point::new(offset+size,offset),Point::new(offset+size,offset+size),Point::new(offset,offset+size)];
            let mut dst = src;
            dst[0].x += size*0.2;
            let h = Homography::try_solve(src,dst).unwrap();
            for i in 0..4 { assert!((h.apply(src[i])-dst[i]).hypot() < size*1e-7); }
        }
    }

    #[test]
    fn singular_and_nonfinite_quads_are_rejected() {
        let line = [Point::new(0.,0.),Point::new(1.,0.),Point::new(2.,0.),Point::new(3.,0.)];
        assert!(Homography::try_solve(line,line).is_none());
        assert!(Homography::try_solve([Point::new(f64::NAN,0.);4],line).is_none());
    }

    #[test]
    fn nested_affines_preserve_document_space_correspondences() {
        let src = [Point::new(0.,0.),Point::new(10.,0.),Point::new(10.,10.),Point::new(0.,10.)];
        let mut dst=src; dst[0]=Point::new(2.,1.);
        let h=Homography::solve(src,dst);
        let world=Affine::translate((12.,-8.))*Affine::rotate(0.7)*Affine::new([2.,0.2,0.4,-3.,5.,6.]);
        let local=h.conjugate(world,world.inverse());
        for p in src { approx_eq(world*local.apply(world.inverse()*p),h.apply(p)); }
    }

    #[test]
    fn path_warp_preserves_handles_and_rejects_horizon_atomically() {
        let mut geometry=crate::geom::BezPath::new();
        geometry.move_to((0.,0.)); geometry.curve_to((0.,10.),(10.,10.),(10.,0.));
        let path=crate::PathData::from_bezpath(geometry);
        let h=Homography([0.8,-0.2,2.,0.,0.8,0.,0.,-0.02,1.]);
        let warped=h.warp_path(&path).unwrap();
        for (before,after) in path.subpaths()[0].anchors.iter().zip(&warped.subpaths()[0].anchors) {
            approx_eq(after.point,h.apply(before.point));
            assert_eq!(after.handle_in,before.handle_in.map(|p|h.apply(p)));
            assert_eq!(after.handle_out,before.handle_out.map(|p|h.apply(p)));
        }
        assert_eq!(warp_path_data(&path,&h),warped);
        let horizon=Homography([1.,0.,0.,0.,1.,0.,0.,-0.1,1.]);
        assert!(horizon.warp_path(&path).is_none());
        assert_eq!(path.geometry.elements().len(),2);
    }

}
