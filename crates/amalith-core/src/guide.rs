//! Ruler guides — infinite horizontal / vertical reference lines at a
//! document coordinate. They carry no geometry of their own, don't print
//! or export, and (like Illustrator's) live on the document, not an
//! artboard. Visibility and locking are a viewer concern and kept in the
//! shell, not here.

use serde::{Deserialize, Serialize};

pub use crate::ids::GuideId;

/// Which axis a guide runs along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuideOrient {
    /// A horizontal line; [`Guide::pos`] is its `y`.
    Horizontal,
    /// A vertical line; [`Guide::pos`] is its `x`.
    Vertical,
}

/// One ruler guide.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    pub id: GuideId,
    pub orient: GuideOrient,
    /// Canonical px in document space — the `y` of a horizontal guide, the
    /// `x` of a vertical one.
    pub pos: f64,
}

impl Guide {
    pub fn new(orient: GuideOrient, pos: f64) -> Self {
        Self {
            id: GuideId::new(),
            orient,
            pos,
        }
    }
}
