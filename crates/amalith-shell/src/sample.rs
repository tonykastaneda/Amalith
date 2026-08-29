//! A stand-in document so the canvas has something to draw until real
//! open / new-document plumbing exists.

use amalith_core::geom::{Point, Rect};
use amalith_core::ids::{ArtboardId, LayerId, ObjectId};
use amalith_core::object::PathData;
use amalith_core::{
    Appearance, Artboard, Color, Document, Layer, Object, ObjectKind, ObjectParent, Paint,
};

pub fn document() -> Document {
    let mut doc = Document::new("Untitled");

    doc.insert_artboard(
        Artboard::new(
            ArtboardId::new(),
            "Artboard 1",
            Rect::new(0.0, 0.0, 1280.0, 800.0),
        ),
        0,
    );

    let layer = LayerId::new();
    doc.insert_layer(Layer::new(layer, "Layer 1"), 0);

    let add = |doc: &mut Document, kind: PathData, fill: Color| {
        let mut o = Object::new(
            ObjectId::new(),
            ObjectParent::Layer(layer),
            ObjectKind::Path(kind),
        );
        o.appearance = Appearance {
            fill: Paint::Solid(fill),
            stroke: Paint::Solid(Color::rgb(0.10, 0.10, 0.12)),
            stroke_width: 4.0,
            opacity: 1.0,
        };
        let _ = doc.insert_object(o, usize::MAX);
    };

    add(
        &mut doc,
        PathData::rectangle(Rect::new(140.0, 150.0, 560.0, 470.0)),
        Color::rgb(0.29, 0.56, 0.96),
    );
    add(
        &mut doc,
        PathData::ellipse(Rect::new(430.0, 300.0, 960.0, 700.0)),
        Color::rgb(0.98, 0.74, 0.24),
    );
    add(
        &mut doc,
        PathData::polygon(&[
            Point::new(760.0, 90.0),
            Point::new(1140.0, 280.0),
            Point::new(880.0, 560.0),
        ]),
        Color::rgb(0.53, 0.85, 0.44),
    );

    doc
}
