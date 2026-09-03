//! Regular polygon: circumradius plus a side count (3+). Centred on the
//! click point, first vertex pointing up.

use std::f64::consts::{FRAC_PI_2, TAU};

use vello::kurbo::Point;

use super::{Field, Geometry, Params, Shape};

pub(crate) struct Polygon;

impl Shape for Polygon {
    fn rows(&self, p: &Params) -> Vec<Field> {
        vec![
            Field::len("Radius", p.polygon.0),
            Field::count("Sides", p.polygon.1),
        ]
    }

    fn geometry(&self, a: Point, v: &[f64]) -> Geometry {
        let rad = v[0].max(1.0);
        let n = v[1].round().max(3.0) as usize;
        let pts: Vec<amalith_core::Point> = (0..n)
            .map(|i| {
                let ang = -FRAC_PI_2 + i as f64 * TAU / n as f64;
                amalith_core::Point::new(a.x + rad * ang.cos(), a.y + rad * ang.sin())
            })
            .collect();
        Geometry::Path(amalith_core::PathData::polygon(&pts))
    }

    fn write_params(&self, v: &[f64], p: &mut Params) {
        p.polygon = (v[0], v[1]);
    }
}
