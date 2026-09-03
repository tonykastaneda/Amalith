//! Rectangle: Width × Height, with a constrain-link. The shape lands with
//! its top-left corner at the click point.

use vello::kurbo::Point;

use super::{Field, Geometry, Params, Shape};

pub(crate) struct Rectangle;

impl Shape for Rectangle {
    fn rows(&self, p: &Params) -> Vec<Field> {
        vec![
            Field::len("Width", p.rect.0),
            Field::len("Height", p.rect.1),
        ]
    }

    fn geometry(&self, a: Point, v: &[f64]) -> Geometry {
        let (w, h) = (v[0].max(1.0), v[1].max(1.0));
        Geometry::Rect(amalith_core::Rect::new(a.x, a.y, a.x + w, a.y + h))
    }

    fn write_params(&self, v: &[f64], p: &mut Params) {
        p.rect = (v[0], v[1]);
    }

    fn has_link(&self) -> bool {
        true
    }
}
