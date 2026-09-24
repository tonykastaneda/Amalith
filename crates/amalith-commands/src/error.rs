use amalith_core::{ArtboardId, DocumentError, GradientId, GuideId, LayerId, ObjectId};
use thiserror::Error;

/// Errors from executing a [`crate::Command`] or from `undo`/`redo`.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum CommandError {
    #[error("object {0} is not an image")]
    NotAnImage(ObjectId),
    #[error("trace contains no usable paths")]
    InvalidTrace,
    #[error("the distortion maps path geometry to infinity")]
    InvalidWarp,
    #[error("no artboard with id {0}")]
    ArtboardNotFound(ArtboardId),
    #[error("no layer with id {0}")]
    LayerNotFound(LayerId),
    #[error("no object with id {0}")]
    ObjectNotFound(ObjectId),
    #[error("object {0} is not an empty vector row")]
    NotAVectorSlot(ObjectId),
    #[error("no guide with id {0}")]
    GuideNotFound(GuideId),
    #[error("no gradient with id {0}")]
    GradientNotFound(GradientId),
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
    #[error("nothing to align")]
    NothingToAlign,
    #[error("cannot move a group into itself")]
    CannotReparent,
    #[error("object {0} is not a blend group")]
    NotABlend(ObjectId),
    #[error("select exactly two open-path endpoints to join")]
    JoinNeedsTwoOpenEndpoints,
    #[error("no objects to make a symbol from")]
    NothingToDefine,
    #[error("object {0} is not a symbol instance")]
    NotASymbolInstance(ObjectId),
    #[error("object {0} already has a layer mask")]
    AlreadyHasMask(ObjectId),
    #[error("object {0} has no layer mask")]
    NoLayerMask(ObjectId),
    #[error("object {0} can't have a layer mask (only images and adjustments can)")]
    NotMaskable(ObjectId),
    #[error("object {0} is not an adjustment")]
    NotAnAdjustment(ObjectId),
    #[error("adjustments can only be added to a raster layer")]
    NotARasterLayer(LayerId),
    #[error("vector object rows can only be added to a vector layer")]
    NotAVectorLayer(LayerId),
    #[error("layer {0} is locked")]
    LayerLocked(LayerId),
    #[error("adjustment settings are out of range")]
    InvalidAdjustment,
    #[error("adjustments can't be grouped; they belong directly in a layer")]
    CannotGroupAdjustment,
    #[error("sublayers belong directly in a layer; they can't be grouped, clipped, blended, made into symbols or nested")]
    SublayerNotAllowed,
}
