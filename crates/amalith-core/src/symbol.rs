//! Symbol definitions: the document's pool of reusable artwork.
//!
//! A [`SymbolDefinition`] owns a top-level child-id list exactly like a
//! [`crate::Layer`] does — [`ObjectParent::Symbol`](crate::ObjectParent::Symbol)
//! is a third ownership root alongside `Layer` and `Group`, so a
//! definition's content lives in the same object arena as everything
//! else, just never reachable from any `Layer::children`. An instance
//! ([`crate::ObjectKind::Symbol`]) references a definition by [`SymbolId`],
//! the same pooled-reference shape [`crate::Paint::Gradient`] already uses
//! for [`crate::Gradient`] via [`crate::GradientId`] — so editing a
//! definition's content in place (e.g. via isolation mode) is instantly
//! visible through every instance, with no separate "redefine" step or
//! instance-tracking needed.

use crate::ids::{ObjectId, SymbolId};
use serde::{Deserialize, Serialize};

/// One entry in the document's symbol pool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolDefinition {
    pub id: SymbolId,
    pub name: String,
    /// Top-level content, paint-order — the same convention as
    /// [`crate::Layer::children`]/[`crate::object::GroupData::children`].
    /// Resolve each id via [`crate::Document::object`]; never assume one
    /// still exists (see `amalith_io::manifest`'s format-compatibility
    /// policy — a definition entry from a newer file this build only
    /// half-understands can still end up with a dangling child).
    pub children: Vec<ObjectId>,
}
