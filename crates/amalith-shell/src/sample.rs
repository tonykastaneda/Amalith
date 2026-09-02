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
            // Centred on the document origin — the ruler origin sits at
            // the artboard's centre.
            Rect::new(-640.0, -400.0, 640.0, 400.0),
        ),
        0,
    );
    doc.insert_layer(Layer::new(LayerId::new(), "Layer 1"), 0);
    doc
}
