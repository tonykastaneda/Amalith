//! The boot document: an empty artboard + one layer, matching what
//! File ▸ New produces.

use amalith_core::geom::Rect;
use amalith_core::ids::{ArtboardId, LayerId};
use amalith_core::{Artboard, Document, Layer};

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
    doc.insert_layer(Layer::new(LayerId::new(), "Layer 1"), 0);
    doc
}
