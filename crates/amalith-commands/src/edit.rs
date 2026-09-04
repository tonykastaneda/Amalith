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
use amalith_core::{
    Affine, Artboard, ArtboardId, Asset, AssetId, Color, Document, DocumentError, Gradient,
    GradientId, Guide, GuideId, ColorMode, Layer, LayerId, Object, ObjectId, ObjectKind,
    ObjectParent, Paint, PathData, StrokeStyle, TextData, Unit,
};

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
    SetArtboardFill {
        id: ArtboardId,
        fill: Option<Color>,
    },
    SetDocumentUnit {
        unit: Unit,
    },
    SetColorMode {
        mode: ColorMode,
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
    RenameLayer {
        id: LayerId,
        name: String,
    },
    InsertObject {
        object: Box<Object>,
        index: usize,
    },
    RemoveObject {
        id: ObjectId,
    },
    RenameObject {
        id: ObjectId,
        name: Option<String>,
    },
    SetTransform {
        id: ObjectId,
        transform: Affine,
    },
    SetPathData {
        id: ObjectId,
        data: PathData,
    },
    SetTextData {
        id: ObjectId,
        data: TextData,
    },
    SetChildOrder {
        parent: ObjectParent,
        order: Vec<ObjectId>,
    },
    SetFill {
        id: ObjectId,
        paint: Paint,
    },
    SetStroke {
        id: ObjectId,
        paint: Paint,
    },
    SetStrokeWidth {
        id: ObjectId,
        width: f64,
    },
    SetStrokeStyle {
        id: ObjectId,
        style: StrokeStyle,
    },
    SetOpacity {
        id: ObjectId,
        opacity: f32,
    },
    SetVisible {
        id: ObjectId,
        visible: bool,
    },
    SetLocked {
        id: ObjectId,
        locked: bool,
    },
    InsertAsset {
        asset: Asset,
        index: usize,
    },
    RemoveAsset {
        id: AssetId,
    },
    SetClip {
        group: ObjectId,
        clip: Option<ObjectId>,
    },
    InsertGuide {
        guide: Guide,
        index: usize,
    },
    RemoveGuide {
        id: GuideId,
    },
    SetGuidePos {
        id: GuideId,
        pos: f64,
    },
    InsertGradient {
        gradient: Gradient,
        index: usize,
    },
    RemoveGradient {
        id: GradientId,
    },
    SetGradient {
        id: GradientId,
        gradient: Gradient,
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
        Edit::SetArtboardFill { id, fill } => {
            let artboard = doc
                .artboard_mut(id)
                .ok_or(CommandError::ArtboardNotFound(id))?;
            let old = std::mem::replace(&mut artboard.fill, fill);
            Ok((Edit::SetArtboardFill { id, fill: old }, None))
        }
        Edit::SetDocumentUnit { unit } => {
            let old = std::mem::replace(&mut doc.settings.default_unit, unit);
            Ok((Edit::SetDocumentUnit { unit: old }, None))
        }
        Edit::SetColorMode { mode } => {
            let old = std::mem::replace(&mut doc.settings.color_mode, mode);
            Ok((Edit::SetColorMode { mode: old }, None))
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
        Edit::RenameLayer { id, name } => {
            let layer = doc.layer_mut(id).ok_or(CommandError::LayerNotFound(id))?;
            let old_name = std::mem::replace(&mut layer.name, name);
            Ok((Edit::RenameLayer { id, name: old_name }, None))
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
        Edit::RenameObject { id, name } => {
            let object = doc.object_mut(id).ok_or(CommandError::ObjectNotFound(id))?;
            let old_name = std::mem::replace(&mut object.name, name);
            Ok((Edit::RenameObject { id, name: old_name }, None))
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
        Edit::SetPathData { id, data } => {
            let object = doc.object_mut(id).ok_or(CommandError::ObjectNotFound(id))?;
            let ObjectKind::Path(path) = &mut object.kind else {
                return Err(CommandError::NotAPath(id));
            };
            let old = std::mem::replace(path, data);
            Ok((Edit::SetPathData { id, data: old }, None))
        }
        Edit::SetTextData { id, data } => {
            let object = doc.object_mut(id).ok_or(CommandError::ObjectNotFound(id))?;
            let ObjectKind::Text(text) = &mut object.kind else {
                return Err(CommandError::NotText(id));
            };
            let old = std::mem::replace(text, data);
            Ok((Edit::SetTextData { id, data: old }, None))
        }
        Edit::SetChildOrder { parent, order } => {
            let old_order = doc
                .replace_child_order(parent, order)
                .ok_or_else(|| match parent {
                    ObjectParent::Layer(id) => CommandError::LayerNotFound(id),
                    ObjectParent::Group(id) => CommandError::ObjectNotFound(id),
                })?;
            Ok((
                Edit::SetChildOrder {
                    parent,
                    order: old_order,
                },
                None,
            ))
        }
        Edit::SetFill { id, paint } => {
            let object = doc.object_mut(id).ok_or(CommandError::ObjectNotFound(id))?;
            let old_paint = std::mem::replace(&mut object.appearance.fill, paint);
            Ok((
                Edit::SetFill {
                    id,
                    paint: old_paint,
                },
                None,
            ))
        }
        Edit::SetStroke { id, paint } => {
            let object = doc.object_mut(id).ok_or(CommandError::ObjectNotFound(id))?;
            let old_paint = std::mem::replace(&mut object.appearance.stroke, paint);
            Ok((
                Edit::SetStroke {
                    id,
                    paint: old_paint,
                },
                None,
            ))
        }
        Edit::SetStrokeWidth { id, width } => {
            let object = doc.object_mut(id).ok_or(CommandError::ObjectNotFound(id))?;
            let old_width = std::mem::replace(&mut object.appearance.stroke_width, width);
            Ok((
                Edit::SetStrokeWidth {
                    id,
                    width: old_width,
                },
                None,
            ))
        }
        Edit::SetStrokeStyle { id, style } => {
            let object = doc.object_mut(id).ok_or(CommandError::ObjectNotFound(id))?;
            let old_style = std::mem::replace(&mut object.appearance.stroke_style, style);
            Ok((Edit::SetStrokeStyle { id, style: old_style }, None))
        }
        Edit::SetOpacity { id, opacity } => {
            let object = doc.object_mut(id).ok_or(CommandError::ObjectNotFound(id))?;
            let old_opacity = std::mem::replace(&mut object.appearance.opacity, opacity);
            Ok((
                Edit::SetOpacity {
                    id,
                    opacity: old_opacity,
                },
                None,
            ))
        }
        Edit::SetVisible { id, visible } => {
            let object = doc.object_mut(id).ok_or(CommandError::ObjectNotFound(id))?;
            let old = std::mem::replace(&mut object.visible, visible);
            Ok((Edit::SetVisible { id, visible: old }, None))
        }
        Edit::SetLocked { id, locked } => {
            let object = doc.object_mut(id).ok_or(CommandError::ObjectNotFound(id))?;
            let old = std::mem::replace(&mut object.locked, locked);
            Ok((Edit::SetLocked { id, locked: old }, None))
        }
        Edit::InsertAsset { asset, index } => {
            let id = asset.id;
            doc.insert_asset(asset, index);
            Ok((Edit::RemoveAsset { id }, None))
        }
        Edit::RemoveAsset { id } => {
            let (asset, index) = doc
                .remove_asset(id)
                .ok_or(DocumentError::AssetNotFound(id))?;
            Ok((Edit::InsertAsset { asset, index }, None))
        }
        Edit::SetClip { group, clip } => {
            let object = doc.object_mut(group).ok_or(CommandError::ObjectNotFound(group))?;
            match &mut object.kind {
                ObjectKind::Group(g) => {
                    let old = std::mem::replace(&mut g.clip, clip);
                    Ok((Edit::SetClip { group, clip: old }, None))
                }
                _ => Err(CommandError::ObjectNotFound(group)),
            }
        }
        Edit::InsertGuide { guide, index } => {
            let id = guide.id;
            doc.insert_guide(guide, index);
            Ok((Edit::RemoveGuide { id }, Some(NewId::Guide(id))))
        }
        Edit::RemoveGuide { id } => {
            let (guide, index) = doc
                .remove_guide(id)
                .ok_or(CommandError::GuideNotFound(id))?;
            Ok((Edit::InsertGuide { guide, index }, None))
        }
        Edit::SetGuidePos { id, pos } => {
            let guide = doc.guide_mut(id).ok_or(CommandError::GuideNotFound(id))?;
            let old = std::mem::replace(&mut guide.pos, pos);
            Ok((Edit::SetGuidePos { id, pos: old }, None))
        }
        Edit::InsertGradient { gradient, index } => {
            let id = gradient.id;
            doc.insert_gradient(gradient, index);
            Ok((
                Edit::RemoveGradient { id },
                Some(NewId::Gradient(id)),
            ))
        }
        Edit::RemoveGradient { id } => {
            let (gradient, index) = doc
                .remove_gradient(id)
                .ok_or(CommandError::GradientNotFound(id))?;
            Ok((Edit::InsertGradient { gradient, index }, None))
        }
        Edit::SetGradient { id, gradient } => {
            let slot = doc
                .gradient_mut(id)
                .ok_or(CommandError::GradientNotFound(id))?;
            let old = std::mem::replace(slot, gradient);
            Ok((Edit::SetGradient { id, gradient: old }, None))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewId {
    Artboard(ArtboardId),
    Layer(LayerId),
    Object(ObjectId),
    Guide(GuideId),
    Gradient(GradientId),
}
