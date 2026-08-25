//! Artboards: named rectangular regions of document space.
//!
//! An artboard does not own objects (see `layer.rs` for why ownership is
//! layer-based). It is purely a geometric + naming annotation over the
//! infinite document/pasteboard space — the same role Illustrator artboards
//! play: export boundaries and canvas framing, not a parenting relationship.
//! "Which objects are on artboard X" is therefore a query (geometric
//! intersection of an object's document-space bounds with the artboard
//! rect), not a stored edge, so it can never go stale as objects move.
use crate::geom::Rect;
use crate::ids::ArtboardId;
use serde::{Deserialize, Serialize};

/// A named rectangular region of document space (canonical px).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artboard {
    pub id: ArtboardId,
    pub name: String,
    /// Position and size in document space, canonical px.
    pub rect: Rect,
}

impl Artboard {
    pub fn new(id: ArtboardId, name: impl Into<String>, rect: Rect) -> Self {
        Self {
            id,
            name: name.into(),
            rect,
        }
    }

    /// Width/height preset in canonical px (e.g. `Artboard::new(id, "Screen",
    /// Artboard::preset_rect(1920.0, 1080.0))`), placed at the document
    /// origin. Callers that need a specific origin should build the `Rect`
    /// directly.
    pub fn preset_rect(width_px: f64, height_px: f64) -> Rect {
        Rect::new(0.0, 0.0, width_px, height_px)
    }
}
