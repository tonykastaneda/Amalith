//! Rectangular Grid and Polar Grid: Width × Height (with a constrain-link)
//! and the two divider counts — horizontal / vertical, or concentric /
//! radial. The grid lands with its top-left corner at the click point,
//! like Rectangle. The counts are remembered for the tool's drags too.

use vello::kurbo::Point;

use super::{Field, Geometry, Params, Shape};

pub(crate) struct Grid {
    pub polar: bool,
}

impl Shape for Grid {
    fn rows(&self, p: &Params) -> Vec<Field> {
        if self.polar {
            let (w, h, c, r) = p.polar_grid;
            vec![Field::len("Width", w), Field::len("Height", h), Field::count("Concentric", c), Field::count("Radial", r)]
        } else {
            let (w, h, hd, vd) = p.rect_grid;
            vec![Field::len("Width", w), Field::len("Height", h), Field::count("Horizontal", hd), Field::count("Vertical", vd)]
        }
    }

    fn geometry(&self, a: Point, v: &[f64]) -> Geometry {
        let (w, h) = (v[0].max(1.0), v[1].max(1.0));
        let r = amalith_core::Rect::new(a.x, a.y, a.x + w, a.y + h);
        let (m, n) = (dividers(v[2]), dividers(v[3]));
        Geometry::Paths(if self.polar {
            amalith_core::grid::polar_grid(r, m, n)
        } else {
            amalith_core::grid::rectangular_grid(r, m, n)
        })
    }

    fn write_params(&self, v: &[f64], p: &mut Params) {
        let row = (v[0], v[1], dividers(v[2]) as f64, dividers(v[3]) as f64);
        if self.polar {
            p.polar_grid = row;
        } else {
            p.rect_grid = row;
        }
    }

    fn has_link(&self) -> bool {
        true
    }
}

/// A typed divider count, kept to something drawable.
pub(crate) fn dividers(v: f64) -> u32 {
    v.round().clamp(0.0, 999.0) as u32
}
