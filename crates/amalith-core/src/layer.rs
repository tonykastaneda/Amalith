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
use serde::{Deserialize, Serialize};

/// A layer: an ordered bucket of top-level objects, with panel-style
/// visibility/lock state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    /// Top-level objects owned directly by this layer, in stacking order:
    /// index 0 paints first (bottom), the last entry paints last (top).
    /// This matches the paint order used for `GroupData::children` so the
    /// two "ordered children" cases in the object tree behave identically.
    pub children: Vec<ObjectId>,
}

impl Layer {
    pub fn new(id: LayerId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            visible: true,
            locked: false,
            children: Vec::new(),
        }
    }
}
