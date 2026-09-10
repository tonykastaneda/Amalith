//! Layers: the global, document-wide ownership containers for objects.
//!
//! Amalith follows Illustrator's layer model rather than Inkscape's: layers
//! are top-level, span the entire pasteboard, and are independent of
//! artboards. An artboard is a rectangular *region* of document space
//! (see `artboard.rs`); a layer is a *stacking bucket* that can contain
//! objects anywhere in that space, including objects that straddle several
//! artboards or sit outside all of them on the pasteboard. This is the
//! documented ownership choice from `DESIGN.md`: `Document -> Layer ->
//! Object` is the one ownership tree; artboards do not own objects.
use crate::ids::{LayerId, ObjectId};
use crate::swatch::Color;
use serde::{Deserialize, Serialize};

/// A layer's selection/outline color — Illustrator picks from a fixed
/// named palette (Layer Options' "Color" dropdown) rather than an
/// arbitrary RGB picker, and uses it to tint that layer's selection
/// outlines/handles so you can tell at a glance which layer an object on
/// screen belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerColor {
    Blue,
    Red,
    Green,
    Yellow,
    Orange,
    Violet,
    Gray,
    Cyan,
    Magenta,
    LightBlue,
    LightRed,
    LightGreen,
    DarkGreen,
    Teal,
    Brown,
    Black,
}

impl LayerColor {
    pub const ALL: [LayerColor; 16] = [
        LayerColor::Blue,
        LayerColor::Red,
        LayerColor::Green,
        LayerColor::Yellow,
        LayerColor::Orange,
        LayerColor::Violet,
        LayerColor::Gray,
        LayerColor::Cyan,
        LayerColor::Magenta,
        LayerColor::LightBlue,
        LayerColor::LightRed,
        LayerColor::LightGreen,
        LayerColor::DarkGreen,
        LayerColor::Teal,
        LayerColor::Brown,
        LayerColor::Black,
    ];

    /// The dropdown/label name — matches the "Color:" field in Layer
    /// Options.
    pub fn label(self) -> &'static str {
        match self {
            LayerColor::Blue => "Blue",
            LayerColor::Red => "Red",
            LayerColor::Green => "Green",
            LayerColor::Yellow => "Yellow",
            LayerColor::Orange => "Orange",
            LayerColor::Violet => "Violet",
            LayerColor::Gray => "Gray",
            LayerColor::Cyan => "Cyan",
            LayerColor::Magenta => "Magenta",
            LayerColor::LightBlue => "Light Blue",
            LayerColor::LightRed => "Light Red",
            LayerColor::LightGreen => "Light Green",
            LayerColor::DarkGreen => "Dark Green",
            LayerColor::Teal => "Teal",
            LayerColor::Brown => "Brown",
            LayerColor::Black => "Black",
        }
    }

    /// sRGB swatch/tint color — used for the Layers panel's color bar, the
    /// Layer Options swatch, and (when enabled) that layer's selection
    /// outline color.
    pub fn rgb(self) -> Color {
        let (r, g, b) = match self {
            LayerColor::Blue => (0x4a, 0x90, 0xe2),
            LayerColor::Red => (0xe0, 0x50, 0x50),
            LayerColor::Green => (0x4c, 0xb7, 0x6b),
            LayerColor::Yellow => (0xe0, 0xc5, 0x3a),
            LayerColor::Orange => (0xe8, 0x8a, 0x3a),
            LayerColor::Violet => (0x9b, 0x6c, 0xf0),
            LayerColor::Gray => (0x9a, 0x9a, 0x9a),
            LayerColor::Cyan => (0x3a, 0xc7, 0xd6),
            LayerColor::Magenta => (0xd6, 0x4a, 0xb0),
            LayerColor::LightBlue => (0x8a, 0xc4, 0xf0),
            LayerColor::LightRed => (0xf0, 0x8a, 0x8a),
            LayerColor::LightGreen => (0x9a, 0xdb, 0xa0),
            LayerColor::DarkGreen => (0x2f, 0x6b, 0x3a),
            LayerColor::Teal => (0x3a, 0x9a, 0x8f),
            LayerColor::Brown => (0x8a, 0x5a, 0x3a),
            LayerColor::Black => (0x2a, 0x2a, 0x2a),
        };
        Color::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }
}

impl Default for LayerColor {
    fn default() -> Self {
        LayerColor::Blue
    }
}

/// A layer: an ordered bucket of top-level objects, with panel-style
/// visibility/lock state and Illustrator's own Layer Options fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    /// The Layers-panel color bar / selection outline tint (Layer
    /// Options' "Color" field).
    #[serde(default)]
    pub color: LayerColor,
    /// Layer Options' "Template": a locked, dimmed, non-printing/
    /// non-exporting tracing-reference layer. Implies `locked` and
    /// implies not printing, same as Illustrator.
    #[serde(default)]
    pub template: bool,
    /// Layer Options' "Print" — whether this layer's content is included
    /// in export/print output. Independent of `visible` (a layer can be
    /// shown on screen but excluded from output, or vice versa).
    #[serde(default = "default_true")]
    pub print: bool,
    /// Layer Options' "Preview" — render this layer's own content in full
    /// preview (`true`) or as hairline outlines only (`false`),
    /// independent of the document-wide Outline View toggle.
    #[serde(default = "default_true")]
    pub preview: bool,
    /// Layer Options' "Dim Images to X%" — `None` when unchecked. Only
    /// affects placed raster images on this layer, not vector content.
    #[serde(default)]
    pub dim_images_to: Option<u8>,
    /// Top-level objects owned directly by this layer, in stacking order:
    /// index 0 paints first (bottom), the last entry paints last (top).
    /// This matches the paint order used for `GroupData::children` so the
    /// two "ordered children" cases in the object tree behave identically.
    pub children: Vec<ObjectId>,
}

fn default_true() -> bool {
    true
}

impl Layer {
    pub fn new(id: LayerId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            visible: true,
            locked: false,
            color: LayerColor::default(),
            template: false,
            print: true,
            preview: true,
            dim_images_to: None,
            children: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-enforced coverage, matching the pattern used for
    /// `Tool::ALL`/`PanelKind::ALL` elsewhere: a forgotten variant in
    /// `LayerColor::ALL` fails this test to compile (the `match` has no
    /// wildcard), not silently passes.
    #[test]
    fn every_layer_color_has_a_distinct_label_and_appears_in_all() {
        for c in LayerColor::ALL {
            assert!(!c.label().is_empty());
            let count = LayerColor::ALL.iter().filter(|other| other.label() == c.label()).count();
            assert_eq!(count, 1, "{:?} shares a label with another color", c);
        }
    }

    #[test]
    fn new_layer_defaults_match_illustrators_own() {
        let l = Layer::new(LayerId::new(), "Layer 1");
        assert!(l.visible);
        assert!(!l.locked);
        assert_eq!(l.color, LayerColor::Blue);
        assert!(!l.template);
        assert!(l.print);
        assert!(l.preview);
        assert_eq!(l.dim_images_to, None);
    }
}
