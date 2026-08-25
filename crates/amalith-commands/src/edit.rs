//! Low-level, individually-reversible document edits.
//!
//! A [`Command`](crate::Command) is a user-facing *intent* ("create a
//! rectangle"). An [`Edit`] is the primitive it compiles down to before
//! touching the document — a single call into `amalith-core`'s raw
//! mutation API that, when applied, both performs the change and hands
//! back its own exact inverse `Edit`.
//!
//! Undo/redo is built entirely on that symmetry (see `history.rs`): `undo`
//! applies the stored inverse edits and files the edits *that* produces as
//! the redo entry; `redo` does the same in the other direction. Nothing
//! ever "replays" a `Command` to redo it — replaying `CreateArtboard`
//! would mint a fresh `ArtboardId` and silently break identity across an
//! undo/redo roundtrip. Applying the captured inverse `Edit` instead keeps
//! the original ID, exactly as Inkscape's `DocumentUndo` logs low-level
//! repr diffs rather than replaying the action that caused them.
use crate::error::CommandError;
use amalith_core::{Affine, Artboard, ArtboardId, Document, Layer, LayerId, Object, ObjectId};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Edit {
    InsertArtboard {
        artboard: Artboard,
        index: usize,
    },
    RemoveArtboard {
        id: ArtboardId,
    },
    RenameArtboard {
        id: ArtboardId,
        name: String,
    },
    ResizeArtboard {
        id: ArtboardId,
        rect: amalith_core::Rect,
    },
    InsertLayer {
        layer: Layer,
        index: usize,
    },
    RemoveLayer {
        id: LayerId,
    },
    InsertObject {
        object: Box<Object>,
        index: usize,
    },
    RemoveObject {
        id: ObjectId,
    },
    SetTransform {
        id: ObjectId,
        transform: Affine,
    },
}

/// Applies `edit` to `doc`, returning its inverse (to file for undo/redo)
/// and, when the edit created a new entity, that entity's id.
pub(crate) fn apply(edit: Edit, doc: &mut Document) -> Result<(Edit, Option<NewId>), CommandError> {
    match edit {
        Edit::InsertArtboard { artboard, index } => {
            let id = artboard.id;
            doc.insert_artboard(artboard, index);
            Ok((Edit::RemoveArtboard { id }, Some(NewId::Artboard(id))))
        }
        Edit::RemoveArtboard { id } => {
            let (artboard, index) = doc
                .remove_artboard(id)
                .ok_or(CommandError::ArtboardNotFound(id))?;
            Ok((Edit::InsertArtboard { artboard, index }, None))
        }
        Edit::RenameArtboard { id, name } => {
            let artboard = doc
                .artboard_mut(id)
                .ok_or(CommandError::ArtboardNotFound(id))?;
            let old_name = std::mem::replace(&mut artboard.name, name);
            Ok((Edit::RenameArtboard { id, name: old_name }, None))
        }
        Edit::ResizeArtboard { id, rect } => {
            let artboard = doc
                .artboard_mut(id)
                .ok_or(CommandError::ArtboardNotFound(id))?;
            let old_rect = std::mem::replace(&mut artboard.rect, rect);
            Ok((Edit::ResizeArtboard { id, rect: old_rect }, None))
        }
        Edit::InsertLayer { layer, index } => {
            let id = layer.id;
            doc.insert_layer(layer, index);
            Ok((Edit::RemoveLayer { id }, Some(NewId::Layer(id))))
        }
        Edit::RemoveLayer { id } => {
            let (layer, index) = doc
                .remove_layer(id)
                .ok_or(CommandError::LayerNotFound(id))?;
            Ok((Edit::InsertLayer { layer, index }, None))
        }
        Edit::InsertObject { object, index } => {
            let id = object.id;
            doc.insert_object(*object, index)?;
            Ok((Edit::RemoveObject { id }, Some(NewId::Object(id))))
        }
        Edit::RemoveObject { id } => {
            let (object, index) = doc
                .remove_object(id)
                .ok_or(CommandError::ObjectNotFound(id))?;
            Ok((
                Edit::InsertObject {
                    object: Box::new(object),
                    index,
                },
                None,
            ))
        }
        Edit::SetTransform { id, transform } => {
            let object = doc.object_mut(id).ok_or(CommandError::ObjectNotFound(id))?;
            let old_transform = std::mem::replace(&mut object.transform, transform);
            Ok((
                Edit::SetTransform {
                    id,
                    transform: old_transform,
                },
                None,
            ))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewId {
    Artboard(ArtboardId),
    Layer(LayerId),
    Object(ObjectId),
}
