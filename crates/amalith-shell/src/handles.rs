//! Selection-box transform handles: the 8 scale grips, the rotation halo,
//! and the scale math. Ported from `amalith-app`'s selection.rs /
//! main.rs, kept in vello kurbo types (the shell's working space).

use vello::kurbo::{Affine, Point, Rect};

const MIN_SIZE: f64 = 1.0;

/// A scale grip: four corners + four edge midpoints.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handle {
    Nw,
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
}

impl Handle {
    pub const ALL: [Handle; 8] = [
        Handle::Nw,
        Handle::N,
        Handle::Ne,
        Handle::E,
        Handle::Se,
        Handle::S,
        Handle::Sw,
        Handle::W,
    ];
}

/// Corners of `r`, clockwise from top-left.
pub fn rect_quad(r: Rect) -> [Point; 4] {
    [
        Point::new(r.x0, r.y0),
        Point::new(r.x1, r.y0),
        Point::new(r.x1, r.y1),
        Point::new(r.x0, r.y1),
    ]
}

pub fn quad_center(q: [Point; 4]) -> Point {
    Point::new(
        (q[0].x + q[1].x + q[2].x + q[3].x) / 4.0,
        (q[0].y + q[1].y + q[2].y + q[3].y) / 4.0,
    )
}

/// Anchor of `h` on `quad` (corner, or edge midpoint).
pub fn handle_pos(quad: [Point; 4], h: Handle) -> Point {
    let mid = |a: Point, b: Point| Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
    match h {
        Handle::Nw => quad[0],
        Handle::N => mid(quad[0], quad[1]),
        Handle::Ne => quad[1],
        Handle::E => mid(quad[1], quad[2]),
        Handle::Se => quad[2],
        Handle::S => mid(quad[2], quad[3]),
        Handle::Sw => quad[3],
        Handle::W => mid(quad[3], quad[0]),
    }
}

fn point_in_convex_quad(p: Point, quad: [Point; 4]) -> bool {
    let mut pos = false;
    let mut neg = false;
    for i in 0..4 {
        let edge = quad[(i + 1) % 4] - quad[i];
        let to_p = p - quad[i];
        let cross = edge.x * to_p.y - edge.y * to_p.x;
        pos |= cross > 0.0;
        neg |= cross < 0.0;
    }
    !(pos && neg)
}

/// Which handle `p` (screen px) is over, given the selection quad in
/// screen px. 14px grab square, matching amalith-app — scaled by the
/// Selection & Anchor Display size preference ([`crate::handle_scale`]),
/// kept in step with the drawn handle so clicking always matches what's
/// on screen.
pub fn hit_handle(p: Point, quad_screen: [Point; 4]) -> Option<Handle> {
    let r = 7.0 * crate::handle_scale::multiplier();
    Handle::ALL
        .into_iter()
        .find(|&h| (p - handle_pos(quad_screen, h)).hypot() <= r)
}

/// Is `p` (screen px) in the rotation halo just outside a handle?
pub fn hit_rotate_halo(p: Point, quad_screen: [Point; 4]) -> bool {
    rotate_halo_handle(p, quad_screen).is_some()
}

/// Which handle's rotation halo `p` (screen px) is in — index into
/// `Handle::ALL` (0 = Nw, then N, Ne, E, Se, S, Sw, W). The halo is the
/// band just outside each of the 8 grips. `None` if not in any halo.
pub fn rotate_halo_handle(p: Point, quad_screen: [Point; 4]) -> Option<usize> {
    if point_in_convex_quad(p, quad_screen) {
        return None;
    }
    let center = quad_center(quad_screen);
    Handle::ALL.iter().position(|&h| {
        let anchor = handle_pos(quad_screen, h);
        let outward = anchor - center;
        let len = outward.hypot();
        if len <= f64::EPSILON {
            return false;
        }
        let offset = p - anchor;
        let dist = offset.hypot();
        let m = crate::handle_scale::multiplier();
        offset.dot(outward / len) > 0.0 && (8.0 * m..=32.0 * m).contains(&dist)
    })
}

/// The affine to premultiply onto each object's transform for a scale
/// drag. `bounds` is the selection's axis-aligned box, `pointer` the
/// document-space cursor, `uniform` = shift (aspect lock), `from_center`
/// = alt.
pub fn scaled_transform(
    bounds: Rect,
    handle: Handle,
    pointer: Point,
    uniform: bool,
    from_center: bool,
) -> Affine {
    let center = bounds.center();
    let changes_x = !matches!(handle, Handle::N | Handle::S);
    let changes_y = !matches!(handle, Handle::E | Handle::W);
    let left = matches!(handle, Handle::Nw | Handle::Sw | Handle::W);
    let top = matches!(handle, Handle::Nw | Handle::N | Handle::Ne);

    let mut sx = if changes_x {
        if from_center {
            if left {
                (center.x - pointer.x) * 2.0 / bounds.width()
            } else {
                (pointer.x - center.x) * 2.0 / bounds.width()
            }
        } else if left {
            (bounds.x1 - pointer.x) / bounds.width()
        } else {
            (pointer.x - bounds.x0) / bounds.width()
        }
    } else {
        1.0
    };
    let mut sy = if changes_y {
        if from_center {
            if top {
                (center.y - pointer.y) * 2.0 / bounds.height()
            } else {
                (pointer.y - center.y) * 2.0 / bounds.height()
            }
        } else if top {
            (bounds.y1 - pointer.y) / bounds.height()
        } else {
            (pointer.y - bounds.y0) / bounds.height()
        }
    } else {
        1.0
    };
    sx = sx.max(MIN_SIZE / bounds.width());
    sy = sy.max(MIN_SIZE / bounds.height());
    if uniform {
        let s = if changes_x && changes_y {
            sx.max(sy)
        } else if changes_x {
            sx
        } else {
            sy
        }
        .max((MIN_SIZE / bounds.width()).max(MIN_SIZE / bounds.height()));
        sx = s;
        sy = s;
    }

    let pivot = if from_center {
        center
    } else {
        Point::new(
            if left {
                bounds.x1
            } else if changes_x {
                bounds.x0
            } else {
                center.x
            },
            if top {
                bounds.y1
            } else if changes_y {
                bounds.y0
            } else {
                center.y
            },
        )
    };
    Affine::translate(pivot.to_vec2())
        * Affine::scale_non_uniform(sx, sy)
        * Affine::translate(-pivot.to_vec2())
}

/// The affine to premultiply for a rotate drag: `theta` about `center`,
/// snapped to 45° when `uniform` (shift).
pub fn rotate_transform(center: Point, start_angle: f64, pointer: Point, uniform: bool) -> Affine {
    let now = (pointer.y - center.y).atan2(pointer.x - center.x);
    let mut theta = now - start_angle;
    if uniform {
        let step = std::f64::consts::FRAC_PI_4;
        theta = (theta / step).round() * step;
    }
    Affine::translate(center.to_vec2())
        * Affine::rotate(theta)
        * Affine::translate(-center.to_vec2())
}

/// Angle (radians) from `center` to `p`.
pub fn angle_to(center: Point, p: Point) -> f64 {
    (p.y - center.y).atan2(p.x - center.x)
}

/// The affine to premultiply for a Reflect-tool drag: mirrors about
/// `pivot` across the axis at `axis_deg` to the horizontal — the drag
/// vector's own angle, so sweeping the pointer sweeps the mirror line.
/// Same reflection formula as `amalith_core::xform::reflect_about`'s
/// world-space step, just applied directly (every selected object's
/// stored transform is already in document space here, like
/// [`rotate_transform`]'s `m * s`).
pub fn reflect_transform(pivot: Point, axis_deg: f64) -> Affine {
    let (s, c) = (axis_deg * 2.0).to_radians().sin_cos();
    let m = Affine::new([c, s, s, -c, 0.0, 0.0]);
    Affine::translate(pivot.to_vec2()) * m * Affine::translate(-pivot.to_vec2())
}

/// The affine to premultiply for a Scale-tool drag: `pivot` stays fixed,
/// each axis scales by (current offset from pivot) / (offset at press),
/// independently — `uniform` (shift) instead uses one factor (the ratio
/// of the two offsets' lengths) for both axes. `eps` (document units)
/// guards the ratio when press landed on the pivot along an axis.
pub fn scale_tool_transform(
    pivot: Point,
    press_doc: Point,
    pointer_doc: Point,
    uniform: bool,
    eps: f64,
) -> Affine {
    let r = press_doc - pivot;
    let c = pointer_doc - pivot;
    let axis_ratio = |rv: f64, cv: f64| if rv.abs() > eps { cv / rv } else { 1.0 };
    let (mut sx, mut sy) = (axis_ratio(r.x, c.x), axis_ratio(r.y, c.y));
    if uniform {
        let (rl, cl) = (r.hypot(), c.hypot());
        let s = if rl > eps { cl / rl } else { 1.0 };
        sx = s;
        sy = s;
    }
    Affine::translate(pivot.to_vec2())
        * Affine::scale_non_uniform(sx, sy)
        * Affine::translate(-pivot.to_vec2())
}

/// The affine to premultiply for a Shear-tool drag: shears about `pivot`
/// by `shear_deg` along the axis at `axis_deg` to the horizontal. Same
/// convention as [`reflect_transform`]; mirrors
/// `amalith_core::xform::shear_about`'s world-space step.
pub fn shear_transform(pivot: Point, shear_deg: f64, axis_deg: f64) -> Affine {
    let tan = shear_deg.to_radians().tan();
    let rot = axis_deg.to_radians();
    let sh = Affine::new([1.0, 0.0, tan, 1.0, 0.0, 0.0]);
    let m = Affine::rotate(rot) * sh * Affine::rotate(-rot);
    Affine::translate(pivot.to_vec2()) * m * Affine::translate(-pivot.to_vec2())
}
