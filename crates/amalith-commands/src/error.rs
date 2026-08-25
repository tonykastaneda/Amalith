use thiserror::Error;
use amalith_core::{ArtboardId, DocumentError, LayerId, ObjectId};

/// Errors from executing a [`crate::Command`] or from `undo`/`redo`.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum CommandError {
    #[error("no artboard with id {0}")]
    ArtboardNotFound(ArtboardId),
    #[error("no layer with id {0}")]
    LayerNotFound(LayerId),
    #[error("no object with id {0}")]
    ObjectNotFound(ObjectId),
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
}
