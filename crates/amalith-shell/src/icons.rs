//! Tool icons, drawn as vello paths in a 24×24 box and scaled to fit.
//!
//! Hand-built rather than SVG-loaded: `vello_svg` still targets vello 0.9,
//! and drawing them directly means they tint for active / inactive with no
//! extra machinery.

use vello::kurbo::{Affine, BezPath, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Select,
    DirectSelect,
    Pen,
    Rectangle,
    Ellipse,
    Artboard,
}

/// Draw `icon` filling `box_` (already in screen px), in `color`.
pub fn draw(scene: &mut Scene, icon: Icon, box_: Rect, color: Color) {
    // Everything below is authored in a 0..24 space; map it into `box_`.
    let s = box_.width().min(box_.height()) / 24.0;
    let t = Affine::translate((box_.x0, box_.y0)) * Affine::scale(s);

    match icon {
        Icon::Select => {
            // Solid arrow cursor.
            let mut p = BezPath::new();
            p.move_to((5.0, 3.0));
            p.line_to((5.0, 19.0));
            p.line_to((9.5, 15.0));
            p.line_to((12.5, 21.5));
            p.line_to((15.0, 20.3));
            p.line_to((12.0, 14.0));
            p.line_to((18.0, 14.0));
            p.close_path();
            scene.fill(Fill::NonZero, t, color, None, &p);
        }
        Icon::DirectSelect => {
            // Outlined arrow cursor.
            let mut p = BezPath::new();
            p.move_to((5.0, 3.0));
            p.line_to((5.0, 19.0));
            p.line_to((9.5, 15.0));
            p.line_to((12.5, 21.5));
            p.line_to((15.0, 20.3));
            p.line_to((12.0, 14.0));
            p.line_to((18.0, 14.0));
            p.close_path();
            scene.stroke(&Stroke::new(1.6), t, color, None, &p);
        }
        Icon::Pen => {
            // Nib + body.
            let mut body = BezPath::new();
            body.move_to((12.0, 2.5));
            body.line_to((18.0, 14.0));
            body.line_to((6.0, 14.0));
            body.close_path();
            scene.stroke(&Stroke::new(1.6), t, color, None, &body);
            let mut nib = BezPath::new();
            nib.move_to((10.0, 14.0));
            nib.line_to((14.0, 14.0));
            nib.line_to((12.0, 21.0));
            nib.close_path();
            scene.fill(Fill::NonZero, t, color, None, &nib);
        }
        Icon::Rectangle => {
            scene.stroke(
                &Stroke::new(1.8),
                t,
                color,
                None,
                &Rect::new(4.0, 6.0, 20.0, 18.0),
            );
        }
        Icon::Ellipse => {
            let e = vello::kurbo::Ellipse::new((12.0, 12.0), (8.5, 7.0), 0.0);
            scene.stroke(&Stroke::new(1.8), t, color, None, &e);
        }
        Icon::Artboard => {
            scene.stroke(
                &Stroke::new(1.6),
                t,
                color,
                None,
                &Rect::new(4.0, 5.0, 20.0, 19.0),
            );
            // Corner ticks.
            let mut ticks = BezPath::new();
            for (x, y) in [(4.0, 9.0), (20.0, 9.0)] {
                ticks.move_to((x - 2.0, y));
                ticks.line_to((x + 2.0, y));
            }
            scene.stroke(&Stroke::new(1.2), t, color, None, &ticks);
        }
    }
}
