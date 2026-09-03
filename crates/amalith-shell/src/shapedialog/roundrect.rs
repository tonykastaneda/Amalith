//! Rounded Rectangle: Width × Height plus a Corner Radius (clamped to half
//! the shorter side). Top-left corner at the click point.
//!
//! When this grows its own controls (corner style, per-corner radii),
//! turn this file into `roundrect/mod.rs` + `roundrect/options.rs` and
//! implement the `options_*` methods — nothing above changes.

use vello::kurbo::Point;

use super::{Field, Geometry, Params, Shape};

pub(crate) struct RoundRect;

impl Shape for RoundRect {
    fn rows(&self, p: &Params) -> Vec<Field> {
        vec![
            Field::len("Width", p.round.0),
            Field::len("Height", p.round.1),
            Field::len("Radius", p.round.2),
        ]
    }

    fn geometry(&self, a: Point, v: &[f64]) -> Geometry {
        let (w, h) = (v[0].max(1.0), v[1].max(1.0));
        let r = amalith_core::Rect::new(a.x, a.y, a.x + w, a.y + h);
        let rad = v[2].min(w * 0.5).min(h * 0.5);
        Geometry::Path(amalith_core::PathData::rounded_rectangle(r, rad))
    }

    fn write_params(&self, v: &[f64], p: &mut Params) {
        p.round = (v[0], v[1], v[2]);
    }

    fn has_link(&self) -> bool {
        true
    }
}
