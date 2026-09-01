use amalith_core::{ArtboardId, DocumentError, LayerId, ObjectId};
use thiserror::Error;

/// Errors from executing a [`crate::Command`] or from `undo`/`redo`.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum CommandError {
    #[error("no artboard with id {0}")]
    ArtboardNotFound(ArtboardId),
    #[error("no layer with id {0}")]
    LayerNotFound(LayerId),
    #[error("no object with id {0}")]
    ObjectNotFound(ObjectId),
    #[error("object {0} is not a path")]
    NotAPath(ObjectId),
    #[error("object {0} is not text")]
    NotText(ObjectId),
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
    #[error("clipboard is empty")]
    EmptyClipboard,
    #[error("no layer available to paste into")]
    NoLayerAvailable,
    #[error("no objects to duplicate")]
    NothingToDuplicate,
    #[error("could not import SVG: {0}")]
    SvgImport(#[from] amalith_io::SvgError),
    #[error("no objects to group")]
    NothingToGroup,
    #[error("objects must share a parent to be grouped together")]
    ObjectsSpanMultipleParents,
    #[error("no groups to ungroup")]
    NothingToUngroup,
    #[error("select at least two paths")]
    PathfinderNeedTwo,
    #[error("pathfinder produced no geometry")]
    PathfinderEmpty,
    #[error("no stroke to expand")]
    NoStrokeToExpand,
}
