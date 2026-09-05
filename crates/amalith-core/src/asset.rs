//! Assets: linked or embedded external resources (images, fonts, ICC
//! profiles) referenced by objects.
use crate::ids::AssetId;
use serde::{Deserialize, Serialize};

/// What kind of resource an asset is, independent of how it's stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetKind {
    Image,
    Font,
    ColorProfile,
}

/// Where an asset's bytes actually live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetSource {
    /// A path outside the `.amalith` container (relative or absolute); the
    /// document does not own these bytes and must re-resolve the path on
    /// load, matching Illustrator's "Linked" file behavior.
    Linked {
        path: String,
        /// The linked file's mtime (Unix seconds) and byte size, captured
        /// when linked or last updated — compared against the live file
        /// to tell a Links panel's "Modified" status from "OK" (this
        /// crate never reads the filesystem itself; a caller with fs
        /// access captures these and passes them in). `None` for an
        /// asset saved before this existed, or if the stamp couldn't be
        /// read at link time — either way it just reads as "OK" until an
        /// actual mismatch is ever recorded.
        #[serde(default)]
        modified: Option<i64>,
        #[serde(default)]
        size: Option<u64>,
    },
    /// A path *inside* the `.amalith` container (e.g. `images/photo-001.png`),
    /// copied in at embed time. `amalith-io` owns reading/writing the bytes
    /// at this container path; the document model only tracks the pointer.
    Embedded { container_path: String },
}

/// A document-level asset referenced by one or more [`crate::Object`]s
/// (currently `ObjectKind::Image`) via [`AssetId`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub id: AssetId,
    pub name: String,
    pub kind: AssetKind,
    pub source: AssetSource,
}

impl Asset {
    /// `modified`/`size` are the linked file's stamp at link time, if the
    /// caller could read it (this crate has no filesystem access of its
    /// own — see [`AssetSource::Linked`]).
    pub fn linked(
        id: AssetId,
        name: impl Into<String>,
        kind: AssetKind,
        path: impl Into<String>,
        modified: Option<i64>,
        size: Option<u64>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            source: AssetSource::Linked { path: path.into(), modified, size },
        }
    }

    pub fn embedded(
        id: AssetId,
        name: impl Into<String>,
        kind: AssetKind,
        container_path: impl Into<String>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            source: AssetSource::Embedded {
                container_path: container_path.into(),
            },
        }
    }

    pub fn is_embedded(&self) -> bool {
        matches!(self.source, AssetSource::Embedded { .. })
    }

    pub fn is_linked(&self) -> bool {
        matches!(self.source, AssetSource::Linked { .. })
    }
}
