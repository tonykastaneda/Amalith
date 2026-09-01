use crate::ids::{ArtboardId, AssetId, LayerId, ObjectId};
use thiserror::Error;

/// Errors from the low-level document mutation API (`Document`'s `raw`
/// methods). `amalith-commands` maps these into its own `CommandError` so
/// callers of the command engine never need to depend on this crate's
/// error type directly.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DocumentError {
    #[error("no artboard with id {0}")]
    ArtboardNotFound(ArtboardId),
    #[error("no layer with id {0}")]
    LayerNotFound(LayerId),
    #[error("no object with id {0}")]
    ObjectNotFound(ObjectId),
    #[error("object {0} is not a group")]
    NotAGroup(ObjectId),
    #[error("no asset with id {0}")]
    AssetNotFound(AssetId),
}
