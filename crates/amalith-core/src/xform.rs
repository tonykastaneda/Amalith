//! Numeric Transform panel: X / Y / W / H / rotation / shear from an affine.
//!
//! Objects store a local-to-parent [`Affine`], not a lone x/y. This module
//! is the readout and the inverse: given a field edit, produce the new
//! local transform, pivoting around a 9-point reference on the local box.

use crate::geom::{Affine, Point, Rect};

/// Which of the 9 bounding-box handles is the Transform origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RefPoint {
    /// 0 left, 1 centre, 2 right.
    pub col: u8,
    /// 0 top, 1 middle, 2 bottom.
    pub row: u8,
}

impl Default for RefPoint {
    fn default() -> Self {
        Self::CENTER
    }
}

impl RefPoint {
    pub const CENTER: Self = Self { col: 1, row: 1 };

    pub fn t(self) -> f64 {
        self.col.min(2) as f64 * 0.5
    }

    pub fn u(self) -> f64 {
        self.row.min(2) as f64 * 0.5
    }
}

/// Decomposed transform as the panel shows it.
#[derive(Clone, Copy, Debug)]
pub struct TransformValues {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub rotation_deg: f64,
    pub shear_deg: f64,
}

/// Local-space point of `rp` on `bounds`.
pub fn ref_local(bounds: Rect, rp: RefPoint) -> Point {
    Point::new(
        bounds.x0 + bounds.width() * rp.t(),
        bounds.y0 + bounds.height() * rp.u(),
    )
}

/// Document-space position of the reference point.
pub fn ref_world(world: Affine, bounds: Rect, rp: RefPoint) -> Point {
    world * ref_local(bounds, rp)
}

/// Panel readout from a world (document-space) affine and local bounds.
pub fn values(world: Affine, bounds: Rect, rp: RefPoint) -> TransformValues {
    let p = ref_world(world, bounds, rp);
    let d = decompose(world);
    TransformValues {
        x: p.x,
        y: p.y,
        w: bounds.width() * d.sx.abs(),
        h: bounds.height() * d.sy.abs(),
        rotation_deg: d.rotation_deg,
        shear_deg: d.shear_deg,
    }
}

/// New local-to-parent transform after setting X (document space).
pub fn set_x(local: Affine, parent: Affine, bounds: Rect, rp: RefPoint, x: f64) -> Affine {
    let world = parent * local;
    let p = ref_world(world, bounds, rp);
    relocal(parent, Affine::translate((x - p.x, 0.0)) * world)
}

/// New local-to-parent transform after setting Y (document space).
pub fn set_y(local: Affine, parent: Affine, bounds: Rect, rp: RefPoint, y: f64) -> Affine {
    let world = parent * local;
    let p = ref_world(world, bounds, rp);
    relocal(parent, Affine::translate((0.0, y - p.y)) * world)
}

/// New local transform after setting width. `constrain` scales height too.
pub fn set_w(
    local: Affine,
    parent: Affine,
    bounds: Rect,
    rp: RefPoint,
    w: f64,
    constrain: bool,
) -> Affine {
    let cur = values(parent * local, bounds, rp);
    let fx = if cur.w.abs() < 1e-9 {
        1.0
    } else {
        w / cur.w
    };
    let fy = if constrain { fx } else { 1.0 };
    scale_local(local, bounds, rp, fx, fy)
}

/// New local transform after setting height. `constrain` scales width too.
pub fn set_h(
    local: Affine,
    parent: Affine,
    bounds: Rect,
    rp: RefPoint,
    h: f64,
    constrain: bool,
) -> Affine {
    let cur = values(parent * local, bounds, rp);
    let fy = if cur.h.abs() < 1e-9 {
        1.0
    } else {
        h / cur.h
    };
    let fx = if constrain { fy } else { 1.0 };
    scale_local(local, bounds, rp, fx, fy)
}

/// Rotate in document space around the reference point to `deg`.
pub fn set_rotation(
    local: Affine,
    parent: Affine,
    bounds: Rect,
    rp: RefPoint,
    deg: f64,
) -> Affine {
    let world = parent * local;
    let cur = decompose(world).rotation_deg;
    let d = (deg - cur).to_radians();
    let p = ref_world(world, bounds, rp);
    let xf = Affine::translate(p.to_vec2()) * Affine::rotate(d) * Affine::translate(-p.to_vec2());
    relocal(parent, xf * world)
}

/// Shear in local space around the reference point to `deg`.
pub fn set_shear(
    local: Affine,
    parent: Affine,
    bounds: Rect,
    rp: RefPoint,
    deg: f64,
) -> Affine {
    let world = parent * local;
    let cur = decompose(world).shear_deg;
    let d = (deg - cur).to_radians();
    let tan = d.tan();
    let r = ref_local(bounds, rp);
    let sh = Affine::translate(r.to_vec2())
        * Affine::new([1.0, 0.0, tan, 1.0, 0.0, 0.0])
        * Affine::translate(-r.to_vec2());
    relocal(parent, world * sh)
}

/// Flip horizontally around the reference point.
pub fn flip_h(local: Affine, bounds: Rect, rp: RefPoint) -> Affine {
    scale_local(local, bounds, rp, -1.0, 1.0)
}

/// Flip vertically around the reference point.
pub fn flip_v(local: Affine, bounds: Rect, rp: RefPoint) -> Affine {
    scale_local(local, bounds, rp, 1.0, -1.0)
}

fn scale_local(local: Affine, bounds: Rect, rp: RefPoint, fx: f64, fy: f64) -> Affine {
    let r = ref_local(bounds, rp);
    let s = Affine::translate(r.to_vec2())
        * Affine::scale_non_uniform(fx, fy)
        * Affine::translate(-r.to_vec2());
    local * s
}

fn relocal(parent: Affine, new_world: Affine) -> Affine {
    parent.inverse() * new_world
}

struct Decomp {
    sx: f64,
    sy: f64,
    rotation_deg: f64,
    shear_deg: f64,
}

fn decompose(a: Affine) -> Decomp {
    let [a, b, c, d, _, _] = a.as_coeffs();
    let sx = (a * a + b * b).sqrt();
    if sx < 1e-12 {
        return Decomp {
            sx: 0.0,
            sy: (c * c + d * d).sqrt(),
            rotation_deg: 0.0,
            shear_deg: 0.0,
        };
    }
    let rotation = b.atan2(a);
    let (cos, sin) = (a / sx, b / sx);
    let shx = c * cos + d * sin;
    let sy_signed = -c * sin + d * cos;
    let shear = shx.atan2(sy_signed);
    Decomp {
        sx,
        sy: sy_signed,
        rotation_deg: rotation.to_degrees(),
        shear_deg: shear.to_degrees(),
    }
}

/// Format a document-space length for the panel (`133.0203 px`).
pub fn fmt_px(v: f64) -> String {
    fmt_num(v, " px")
}

/// Format an angle for the panel (`0°`).
pub fn fmt_deg(v: f64) -> String {
    fmt_num(v, "°")
}

fn fmt_num(v: f64, unit: &str) -> String {
    let r = (v * 10_000.0).round() / 10_000.0;
    if (r - r.round()).abs() < 5e-5 {
        format!("{}{unit}", r.round() as i64)
    } else {
        let s = format!("{r:.4}");
        format!(
            "{}{unit}",
            s.trim_end_matches('0').trim_end_matches('.')
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box10() -> Rect {
        Rect::new(0.0, 0.0, 10.0, 20.0)
    }

    #[test]
    fn identity_readout_uses_ref_point() {
        let b = box10();
        let tl = values(Affine::IDENTITY, b, RefPoint { col: 0, row: 0 });
        assert!((tl.x - 0.0).abs() < 1e-9 && (tl.y - 0.0).abs() < 1e-9);
        assert!((tl.w - 10.0).abs() < 1e-9 && (tl.h - 20.0).abs() < 1e-9);
        let c = values(Affine::IDENTITY, b, RefPoint::CENTER);
        assert!((c.x - 5.0).abs() < 1e-9 && (c.y - 10.0).abs() < 1e-9);
        assert!((c.rotation_deg).abs() < 1e-9 && (c.shear_deg).abs() < 1e-9);
    }

    #[test]
    fn set_x_moves_the_ref_point_and_keeps_size() {
        let b = box10();
        let local = Affine::IDENTITY;
        let next = set_x(local, Affine::IDENTITY, b, RefPoint::CENTER, 100.0);
        let v = values(next, b, RefPoint::CENTER);
        assert!((v.x - 100.0).abs() < 1e-9);
        assert!((v.y - 10.0).abs() < 1e-9);
        assert!((v.w - 10.0).abs() < 1e-9);
    }

    #[test]
    fn set_w_around_center_keeps_center() {
        let b = box10();
        let next = set_w(
            Affine::IDENTITY,
            Affine::IDENTITY,
            b,
            RefPoint::CENTER,
            40.0,
            false,
        );
        let v = values(next, b, RefPoint::CENTER);
        assert!((v.w - 40.0).abs() < 1e-9);
        assert!((v.x - 5.0).abs() < 1e-9);
        assert!((v.y - 10.0).abs() < 1e-9);
        assert!((v.h - 20.0).abs() < 1e-9);
    }

    #[test]
    fn constrain_scales_both_axes() {
        let b = box10();
        let next = set_w(
            Affine::IDENTITY,
            Affine::IDENTITY,
            b,
            RefPoint::CENTER,
            20.0,
            true,
        );
        let v = values(next, b, RefPoint::CENTER);
        assert!((v.w - 20.0).abs() < 1e-9);
        assert!((v.h - 40.0).abs() < 1e-9);
    }

    #[test]
    fn rotation_around_center_keeps_center() {
        let b = box10();
        let next = set_rotation(
            Affine::IDENTITY,
            Affine::IDENTITY,
            b,
            RefPoint::CENTER,
            90.0,
        );
        let v = values(next, b, RefPoint::CENTER);
        assert!((v.x - 5.0).abs() < 1e-9);
        assert!((v.y - 10.0).abs() < 1e-9);
        assert!((v.rotation_deg - 90.0).abs() < 1e-6);
    }

    #[test]
    fn shear_readout_round_trips() {
        let b = box10();
        let next = set_shear(
            Affine::IDENTITY,
            Affine::IDENTITY,
            b,
            RefPoint::CENTER,
            15.0,
        );
        let v = values(next, b, RefPoint::CENTER);
        assert!((v.shear_deg - 15.0).abs() < 1e-6);
        assert!((v.x - 5.0).abs() < 1e-9);
        assert!((v.y - 10.0).abs() < 1e-9);
    }

    #[test]
    fn grouped_object_set_x_is_in_document_space() {
        let b = box10();
        let parent = Affine::translate((50.0, 0.0));
        let local = Affine::IDENTITY;
        let next = set_x(local, parent, b, RefPoint { col: 0, row: 0 }, 80.0);
        let world = parent * next;
        let v = values(world, b, RefPoint { col: 0, row: 0 });
        assert!((v.x - 80.0).abs() < 1e-9);
    }

    #[test]
    fn fmt_px_trims_trailing_zeros() {
        assert_eq!(fmt_px(100.0), "100 px");
        assert_eq!(fmt_px(133.0203), "133.0203 px");
        assert_eq!(fmt_deg(0.0), "0°");
    }
}
