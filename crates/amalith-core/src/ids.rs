//! Stable, globally-unique identifiers for document entities.
//!
//! Every addressable entity in a [`crate::Document`] (artboards, layers,
//! objects, assets) is identified by a newtype wrapping a [`Uuid`]. Using
//! UUIDs rather than array indices means:
//!
//! - IDs remain valid across reorders, deletes, undo/redo, and serialization.
//! - Cloning a `Document` never needs to "rewrite" identity: a clone carries
//!   exactly the same IDs as its source, and two documents can be merged or
//!   diffed without an ID-remapping pass.
//! - Commands, plugins, scripts, and agents can reference entities by ID
//!   across process boundaries (e.g. a CLI invocation) without holding a
//!   live reference into the document.
//!
//! IDs of different kinds are distinct Rust types, so `ObjectId` and
//! `LayerId` cannot be confused at compile time (unlike Inkscape's shared
//! `std::string` id namespace keyed by XML `id` attribute).

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! define_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a new, random identifier.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wraps an existing UUID, e.g. when deserializing or migrating data.
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Returns the underlying UUID.
            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

define_id!(ArtboardId, "Identifies an [`crate::Artboard`].");
define_id!(LayerId, "Identifies a [`crate::Layer`].");
define_id!(
    ObjectId,
    "Identifies an [`crate::Object`] (path, group, text, image, ...)."
);
define_id!(
    AssetId,
    "Identifies an [`crate::Asset`] (linked or embedded resource)."
);
define_id!(GuideId, "Identifies a [`crate::Guide`] (ruler guide line).");
define_id!(
    GradientId,
    "Identifies a [`crate::Gradient`] in the document's gradient pool."
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_across_many_generations() {
        let mut seen = HashSet::new();
        for _ in 0..10_000 {
            let id = ObjectId::new();
            assert!(seen.insert(id), "duplicate ObjectId generated");
        }
    }

    #[test]
    fn ids_survive_clone_unchanged() {
        let id = ArtboardId::new();
        let cloned = id;
        assert_eq!(id, cloned);
        assert_eq!(id.as_uuid(), cloned.as_uuid());
    }

    #[test]
    fn different_id_kinds_do_not_collide_by_type() {
        // This is primarily a compile-time guarantee (ObjectId and LayerId
        // are distinct types), but we also confirm equal underlying UUIDs
        // are still usable independently per-kind.
        let uuid = Uuid::new_v4();
        let object_id = ObjectId::from_uuid(uuid);
        let layer_id = LayerId::from_uuid(uuid);
        assert_eq!(object_id.as_uuid(), layer_id.as_uuid());
    }

    #[test]
    fn serde_roundtrip_is_transparent_string() {
        let id = AssetId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{}\"", id.as_uuid()));
        let back: AssetId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
