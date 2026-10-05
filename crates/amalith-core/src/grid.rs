//! The Rectangular Grid and Polar Grid tools' geometry: each grid is a
//! handful of separate paths, which the tools put in one group.

use crate::geom::{Point, Rect};
use crate::object::PathData;
use std::f64::consts::{FRAC_PI_2, TAU};

/// A frame around `r` plus `horizontal` evenly spaced horizontal dividers
/// and `vertical` vertical ones — frame first (bottom of the stack), then
/// the horizontal lines top to bottom, then the vertical lines left to
/// right.
pub fn rectangular_grid(r: Rect, horizontal: u32, vertical: u32) -> Vec<PathData> {
    let r = r.abs();
    let mut out = vec![PathData::rectangle(r)];
    for i in 1..=horizontal {
        let y = r.y0 + r.height() * i as f64 / (horizontal + 1) as f64;
        out.push(PathData::polyline(&[Point::new(r.x0, y), Point::new(r.x1, y)]));
    }
    for i in 1..=vertical {
        let x = r.x0 + r.width() * i as f64 / (vertical + 1) as f64;
        out.push(PathData::polyline(&[Point::new(x, r.y0), Point::new(x, r.y1)]));
    }
    out
}

/// `concentric` evenly spaced ellipses inside the outer ellipse fitting
/// `r` (outermost last), plus `radial` dividers from the centre to the
/// rim, the first pointing straight up.
pub fn polar_grid(r: Rect, concentric: u32, radial: u32) -> Vec<PathData> {
    let r = r.abs();
    let c = r.center();
    let mut out: Vec<PathData> = (1..=concentric + 1)
        .map(|i| {
            let f = i as f64 / (concentric + 1) as f64;
            PathData::ellipse(Rect::from_center_size(c, (r.width() * f, r.height() * f)))
        })
        .collect();
    for i in 0..radial {
        let a = -FRAC_PI_2 + TAU * i as f64 / radial as f64;
        let rim = Point::new(c.x + r.width() / 2.0 * a.cos(), c.y + r.height() / 2.0 * a.sin());
        out.push(PathData::polyline(&[c, rim]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangular_grid_is_a_frame_plus_its_dividers() {
        let g = rectangular_grid(Rect::new(0., 0., 100., 60.), 2, 3);
        assert_eq!(g.len(), 1 + 2 + 3);
        assert!(g[0].subpaths()[0].closed);
        assert_eq!(g[1].subpaths()[0].anchors[0].point, Point::new(0., 20.));
        assert_eq!(g[3].subpaths()[0].anchors[0].point, Point::new(25., 0.));
    }

    #[test]
    fn polar_grid_rings_and_spokes() {
        let g = polar_grid(Rect::new(0., 0., 100., 100.), 2, 4);
        assert_eq!(g.len(), 3 + 4);
        let outer = g[2].local_bounds();
        assert!((outer.width() - 100.0).abs() < 1e-6);
        let spoke = &g[3].subpaths()[0].anchors;
        assert_eq!(spoke[0].point, Point::new(50., 50.));
        assert!((spoke[1].point.y - 0.0).abs() < 1e-9, "the first spoke points up");
    }
}
