//! Star: an outer radius (Radius 1), an inner radius (Radius 2) and a
//! point count (3+). Centred on the click point, first outer point up.

use std::f64::consts::{FRAC_PI_2, PI};

use vello::kurbo::Point;

use super::{Field, Geometry, Params, Shape};

pub(crate) struct Star;

impl Shape for Star {
    fn rows(&self, p: &Params) -> Vec<Field> {
        vec![
            Field::len("Radius 1", p.star.0),
            Field::len("Radius 2", p.star.1),
            Field::count("Points", p.star.2),
        ]
    }

    fn geometry(&self, a: Point, v: &[f64]) -> Geometry {
        let (r1, r2) = (v[0].max(0.5), v[1].max(0.5));
        let n = v[2].round().max(3.0) as usize;
        let pts: Vec<amalith_core::Point> = (0..n * 2)
            .map(|i| {
                let ang = -FRAC_PI_2 + i as f64 * PI / n as f64;
                let r = if i % 2 == 0 { r1 } else { r2 };
                amalith_core::Point::new(a.x + r * ang.cos(), a.y + r * ang.sin())
            })
            .collect();
        Geometry::Path(amalith_core::PathData::polygon(&pts))
    }

    fn write_params(&self, v: &[f64], p: &mut Params) {
        p.star = (v[0], v[1], v[2]);
    }
}
