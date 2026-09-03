//! The document: one explicit ownership tree, one coordinate system.
//!
//! # Coordinate system
//!
//! A document has a single global coordinate space ("document space"),
//! stored in canonical px (see `units.rs`). Artboards are rectangles
//! placed in that space; there is no separate per-artboard coordinate
//! system stored anywhere. An "artboard-relative" position is always
//! `document_position - artboard.rect.origin()`, computed on demand. This
//! is a deliberate deviation from apps (and from naive SVG-per-artboard
//! export) that give each artboard its own local origin: storing positions
//! artboard-relative would require re-deriving or re-stamping every
//! object's coordinates whenever it moves between artboards or when an
//! artboard is resized/repositioned, and objects that straddle two
//! artboards would have no single well-defined local space at all. One
//! canonical space, with artboards as annotations over it, avoids that
//! entire class of bugs.
//!
//! # Ownership tree
//!
//! `Document -> Layer -> Object (-> Object -> ...)`. Layers are top-level
//! and span the whole document (see `layer.rs`); artboards do **not** own
//! objects (see `artboard.rs`). This is "pick a clear tree and stick to
//! it": Illustrator's model, chosen over Inkscape's artboard-as-object
//! approach because it matches the workflows Amalith targets (see
//! `amalith-project-brief.md`).
//!
//! # Why not an XML repr tree (Inkscape-style)?
//!
//! Inkscape keeps an XML repr tree (the literal SVG DOM) as the source of
//! truth, with `SPObject`/`SPItem` as a parallel "live" tree kept in sync
//! with it via observers. That split exists because Inkscape's native
//! format *is* SVG, so the DOM has to be authoritative. Amalith's native
//! format is not SVG-as-DOM (see `amalith-io` and `DESIGN.md`); SVG is only
//! an interchange target. With no DOM to stay authoritative over, keeping
//! two synchronized trees would be pure overhead, so `Document` is the one
//! and only tree, and `amalith-io` serializes it directly.
use crate::artboard::Artboard;
use crate::asset::Asset;
use crate::error::DocumentError;
use crate::geom::{Affine, Rect};
use crate::guide::Guide;
use crate::ids::{ArtboardId, AssetId, GuideId, LayerId, ObjectId};
use crate::layer::Layer;
use crate::metadata::{Metadata, Settings};
use crate::object::{Object, ObjectKind, ObjectParent};
use crate::swatch::Swatch;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A Amalith document: metadata, artboards, layers, the object arena,
/// assets, and swatches.
///
/// Mutation happens exclusively through the methods on this type (the
/// "raw" API documented per-method below). This crate does not itself
/// enforce that external callers go through `amalith-commands::Editor`
/// rather than calling these methods directly — Rust has no cross-crate
/// "friend" visibility — but the methods are named and documented as the
/// mutation primitives commands are built from, not a public editing API.
/// GUI/plugin/script/agent code should depend on `amalith-commands`, not
/// call these directly, so every mutation is undoable and observable in
/// one place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub metadata: Metadata,
    pub settings: Settings,
    artboards: Vec<Artboard>,
    layers: Vec<Layer>,
    objects: HashMap<ObjectId, Object>,
    assets: Vec<Asset>,
    swatches: Vec<Swatch>,
    #[serde(default)]
    guides: Vec<Guide>,
}

impl Document {
    /// Creates an empty document with no artboards, layers, or objects.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            metadata: Metadata {
                title: Some(title.into()),
                created_with: Some(concat!("amalith/", env!("CARGO_PKG_VERSION")).to_string()),
                ..Metadata::default()
            },
            settings: Settings::default(),
            artboards: Vec::new(),
            layers: Vec::new(),
            objects: HashMap::new(),
            assets: Vec::new(),
            swatches: Vec::new(),
            guides: Vec::new(),
        }
    }

    // ---- Artboards ---------------------------------------------------

    pub fn artboards(&self) -> &[Artboard] {
        &self.artboards
    }

    pub fn artboard(&self, id: ArtboardId) -> Option<&Artboard> {
        self.artboards.iter().find(|a| a.id == id)
    }

    pub fn artboard_mut(&mut self, id: ArtboardId) -> Option<&mut Artboard> {
        self.artboards.iter_mut().find(|a| a.id == id)
    }

    /// Raw: inserts an artboard at `index` (clamped to the current length).
    /// Building blocks for `amalith-commands::Command::CreateArtboard`.
    pub fn insert_artboard(&mut self, artboard: Artboard, index: usize) {
        let index = index.min(self.artboards.len());
        self.artboards.insert(index, artboard);
    }

    /// Raw: removes an artboard, returning it and its former index (so an
    /// undo can reinsert at the same position). Objects are never owned by
    /// artboards (see module docs), so this never touches `objects`.
    pub fn remove_artboard(&mut self, id: ArtboardId) -> Option<(Artboard, usize)> {
        let index = self.artboards.iter().position(|a| a.id == id)?;
        Some((self.artboards.remove(index), index))
    }

    // ---- Layers --------------------------------------------------------

    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Raw: inserts a layer at `index` (clamped to the current length).
    pub fn insert_layer(&mut self, layer: Layer, index: usize) {
        let index = index.min(self.layers.len());
        self.layers.insert(index, layer);
    }

    /// Raw: removes a layer, returning it and its former index. Does not
    /// remove the objects it owned from the object arena (mirrors
    /// `remove_object`'s group-children caveat below); a future
    /// `DeleteLayer` command that also deletes contents should remove each
    /// child explicitly first.
    pub fn remove_layer(&mut self, id: LayerId) -> Option<(Layer, usize)> {
        let index = self.layers.iter().position(|l| l.id == id)?;
        Some((self.layers.remove(index), index))
    }

    // ---- Objects ---------------------------------------------------------

    pub fn object(&self, id: ObjectId) -> Option<&Object> {
        self.objects.get(&id)
    }

    pub fn object_mut(&mut self, id: ObjectId) -> Option<&mut Object> {
        self.objects.get_mut(&id)
    }

    pub fn objects(&self) -> impl Iterator<Item = &Object> {
        self.objects.values()
    }

    /// Ordered child-id list for a layer or group, matching the paint-order
    /// convention documented on `Layer::children`.
    pub fn children_of(&self, parent: ObjectParent) -> &[ObjectId] {
        self.children_vec(parent).map(Vec::as_slice).unwrap_or(&[])
    }

    fn children_vec(&self, parent: ObjectParent) -> Option<&Vec<ObjectId>> {
        match parent {
            ObjectParent::Layer(id) => self.layer(id).map(|l| &l.children),
            ObjectParent::Group(id) => match &self.object(id)?.kind {
                ObjectKind::Group(g) => Some(&g.children),
                _ => None,
            },
        }
    }

    fn children_vec_mut(&mut self, parent: ObjectParent) -> Option<&mut Vec<ObjectId>> {
        match parent {
            ObjectParent::Layer(id) => self.layer_mut(id).map(|l| &mut l.children),
            ObjectParent::Group(id) => match &mut self.object_mut(id)?.kind {
                ObjectKind::Group(g) => Some(&mut g.children),
                _ => None,
            },
        }
    }

    /// Raw command-engine primitive: replaces a parent's complete paint order,
    /// returning the previous order. Callers must preserve the same child ids.
    pub fn replace_child_order(
        &mut self,
        parent: ObjectParent,
        order: Vec<ObjectId>,
    ) -> Option<Vec<ObjectId>> {
        let children = self.children_vec_mut(parent)?;
        if children.len() != order.len() || !children.iter().all(|id| order.contains(id)) {
            return None;
        }
        Some(std::mem::replace(children, order))
    }

    /// Raw: inserts `object` into the arena and into its parent's child
    /// list at `index` (clamped). Fails if `object.parent` doesn't exist,
    /// or (for a group parent) isn't a group.
    pub fn insert_object(&mut self, object: Object, index: usize) -> Result<(), DocumentError> {
        match object.parent {
            ObjectParent::Layer(id) => {
                self.layer(id).ok_or(DocumentError::LayerNotFound(id))?;
            }
            ObjectParent::Group(id) => {
                let parent = self.object(id).ok_or(DocumentError::ObjectNotFound(id))?;
                if !parent.is_group() {
                    return Err(DocumentError::NotAGroup(id));
                }
            }
        }
        let id = object.id;
        let parent = object.parent;
        self.objects.insert(id, object);
        let children = self
            .children_vec_mut(parent)
            .expect("parent existence validated above");
        let index = index.min(children.len());
        children.insert(index, id);
        Ok(())
    }

    /// Raw: removes an object from the arena and from its parent's child
    /// list, returning it and its former index within that list.
    ///
    /// Does **not** recursively remove a group's children — they become
    /// unreachable from any layer/group child list but remain in the
    /// arena. No command in the current set deletes groups with contents;
    /// a future `DeleteObject` that must cascade should walk
    /// `GroupData::children` itself before calling this.
    pub fn remove_object(&mut self, id: ObjectId) -> Option<(Object, usize)> {
        let object = self.objects.remove(&id)?;
        let children = self
            .children_vec_mut(object.parent)
            .expect("an object's recorded parent must exist while the object exists");
        let index = children.iter().position(|&c| c == id)?;
        children.remove(index);
        Some((object, index))
    }

    // ---- Assets / swatches ---------------------------------------------

    pub fn assets(&self) -> &[Asset] {
        &self.assets
    }

    pub fn add_asset(&mut self, asset: Asset) {
        self.assets.push(asset);
    }

    pub fn asset(&self, id: AssetId) -> Option<&Asset> {
        self.assets.iter().find(|a| a.id == id)
    }

    pub fn insert_asset(&mut self, asset: Asset, index: usize) {
        let i = index.min(self.assets.len());
        self.assets.insert(i, asset);
    }

    pub fn remove_asset(&mut self, id: AssetId) -> Option<(Asset, usize)> {
        let i = self.assets.iter().position(|a| a.id == id)?;
        Some((self.assets.remove(i), i))
    }

    pub fn swatches(&self) -> &[Swatch] {
        &self.swatches
    }

    pub fn add_swatch(&mut self, swatch: Swatch) {
        self.swatches.push(swatch);
    }

    // ---- Guides -------------------------------------------------------

    pub fn guides(&self) -> &[Guide] {
        &self.guides
    }

    pub fn guide(&self, id: GuideId) -> Option<&Guide> {
        self.guides.iter().find(|g| g.id == id)
    }

    pub fn guide_mut(&mut self, id: GuideId) -> Option<&mut Guide> {
        self.guides.iter_mut().find(|g| g.id == id)
    }

    /// Raw: inserts a guide at `index` (clamped). Building block for
    /// `amalith-commands::Command::AddGuide`.
    pub fn insert_guide(&mut self, guide: Guide, index: usize) {
        let i = index.min(self.guides.len());
        self.guides.insert(i, guide);
    }

    /// Raw: removes a guide, returning it and its former index so an undo
    /// can reinsert at the same spot.
    pub fn remove_guide(&mut self, id: GuideId) -> Option<(Guide, usize)> {
        let i = self.guides.iter().position(|g| g.id == id)?;
        Some((self.guides.remove(i), i))
    }

    // ---- Transforms / bounds --------------------------------------------

    /// Composes local transforms from `id` up through parent groups to
    /// document space. Layers contribute no transform (see module docs),
    /// so the composition stops there. Returns identity for an unknown id.
    pub fn world_transform(&self, id: ObjectId) -> Affine {
        let Some(object) = self.objects.get(&id) else {
            return Affine::IDENTITY;
        };
        let parent_transform = match object.parent {
            ObjectParent::Group(parent_id) => self.world_transform(parent_id),
            ObjectParent::Layer(_) => Affine::IDENTITY,
        };
        parent_transform * object.transform
    }

    /// Document-space axis-aligned bounds of an object. For a group, this
    /// is the union of each child's document-space bounds (each child's
    /// own transform, and its ancestors' up to and including this group,
    /// already folded in via `world_transform`) — not "transform the
    /// group's own local bbox", which would double-count nested rotation.
    /// `None` for an unknown id or an object with no contributing geometry
    /// (e.g. an empty group).
    pub fn bounds_of(&self, id: ObjectId) -> Option<Rect> {
        let object = self.objects.get(&id)?;
        match &object.kind {
            ObjectKind::Group(group) => group
                .children
                .iter()
                .filter_map(|&child| self.bounds_of(child))
                .reduce(|a, b| a.union(b)),
            _ => {
                let local = object.kind.own_local_bounds()?;
                Some(self.world_transform(id).transform_rect_bbox(local))
            }
        }
    }

    /// `id`'s own bounds in *its own* local space, ignoring `id`'s own
    /// `transform` — the recursive, group-aware counterpart to
    /// [`ObjectKind::own_local_bounds`], which stops at `Group` because a
    /// group's own bounds can only be computed by looking at its children
    /// (which `ObjectKind` alone can't reach; only a `Document` can).
    ///
    /// This is what `object_quad`/selection-handle code needs so a Group
    /// gets an oriented bounding quad the same way a Path does, instead of
    /// selection/move/scale/rotate silently no-oping on it — the two are
    /// meant to satisfy `bounds_of(id) == world_transform(id).transform_rect_bbox(local_bounds_of(id))`
    /// for every kind, the same relationship `bounds_of` already has with
    /// [`ObjectKind::own_local_bounds`] for a non-group. For a group, each
    /// child contributes `child.transform` (its own local-to-group-space
    /// transform, not the child's full `world_transform`) applied to that
    /// child's own `local_bounds_of`, recursing through nested subgroups.
    /// `None` for an unknown id or no contributing geometry (e.g. an empty
    /// group), same as `bounds_of`.
    pub fn local_bounds_of(&self, id: ObjectId) -> Option<Rect> {
        let object = self.objects.get(&id)?;
        match &object.kind {
            ObjectKind::Group(group) => group
                .children
                .iter()
                .filter_map(|&child| {
                    let child_object = self.objects.get(&child)?;
                    let child_local = self.local_bounds_of(child)?;
                    Some(child_object.transform.transform_rect_bbox(child_local))
                })
                .reduce(|a, b| a.union(b)),
            _ => object.kind.own_local_bounds(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artboard::Artboard;
    use crate::layer::Layer;
    use crate::object::Object;

    fn rect_doc() -> (Document, LayerId, ObjectId, Rect) {
        let mut doc = Document::new("Test");
        let layer = Layer::new(LayerId::new(), "Layer 1");
        let layer_id = layer.id;
        doc.insert_layer(layer, 0);

        let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
        let object = Object::rectangle(ObjectId::new(), ObjectParent::Layer(layer_id), rect);
        let object_id = object.id;
        doc.insert_object(object, 0).unwrap();

        (doc, layer_id, object_id, rect)
    }

    #[test]
    fn empty_document_has_no_content() {
        let doc = Document::new("Empty");
        assert!(doc.artboards().is_empty());
        assert!(doc.layers().is_empty());
        assert!(doc.objects().next().is_none());
    }

    #[test]
    fn add_artboard_layer_and_rectangle() {
        let (doc, layer_id, object_id, rect) = rect_doc();
        assert_eq!(doc.layers().len(), 1);
        assert_eq!(doc.children_of(ObjectParent::Layer(layer_id)), &[object_id]);
        assert_eq!(doc.bounds_of(object_id), Some(rect));
    }

    #[test]
    fn artboards_add_remove_reorder() {
        let mut doc = Document::new("Artboards");
        let a = Artboard::new(ArtboardId::new(), "A", Artboard::preset_rect(100.0, 100.0));
        let b = Artboard::new(ArtboardId::new(), "B", Artboard::preset_rect(200.0, 200.0));
        let (a_id, b_id) = (a.id, b.id);

        doc.insert_artboard(a, 0);
        doc.insert_artboard(b, 0); // insert at front -> [B, A]
        assert_eq!(
            doc.artboards().iter().map(|a| a.id).collect::<Vec<_>>(),
            vec![b_id, a_id]
        );

        let (removed, index) = doc.remove_artboard(b_id).unwrap();
        assert_eq!(removed.id, b_id);
        assert_eq!(index, 0);
        assert_eq!(doc.artboards().len(), 1);
        assert!(doc.artboard(b_id).is_none());
        assert!(doc.artboard(a_id).is_some());
    }

    #[test]
    fn object_transform_composes_through_groups() {
        let mut doc = Document::new("Nested");
        let layer = Layer::new(LayerId::new(), "Layer 1");
        let layer_id = layer.id;
        doc.insert_layer(layer, 0);

        let group = Object::new(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            ObjectKind::Group(Default::default()),
        );
        let group_id = group.id;
        let mut group = group;
        group.transform = Affine::translate((10.0, 0.0));
        doc.insert_object(group, 0).unwrap();

        let mut rect = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Group(group_id),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        rect.transform = Affine::translate((0.0, 5.0));
        let rect_id = rect.id;
        doc.insert_object(rect, 0).unwrap();

        // World transform composes group (x+10) then rect (y+5).
        let world = doc.world_transform(rect_id);
        assert_eq!(world * Point_ORIGIN, crate::geom::Point::new(10.0, 5.0));

        let bounds = doc.bounds_of(group_id).unwrap();
        assert_eq!(bounds, Rect::new(10.0, 5.0, 20.0, 15.0));

        // `local_bounds_of` is `bounds_of` one level short: it folds in
        // the rect's own transform (relative to the group) but not the
        // group's own transform — so it must equal `bounds_of(group_id)`
        // translated back by the group's own (x+10) offset.
        let local = doc.local_bounds_of(group_id).unwrap();
        assert_eq!(local, Rect::new(0.0, 5.0, 10.0, 15.0));
        assert_eq!(
            doc.world_transform(group_id).transform_rect_bbox(local),
            bounds
        );
    }

    // Small helper to avoid importing kurbo::Point directly in the test above.
    #[allow(non_upper_case_globals)]
    const Point_ORIGIN: crate::geom::Point = crate::geom::Point::new(0.0, 0.0);

    #[test]
    fn removing_object_updates_parent_children() {
        let (mut doc, layer_id, object_id, _rect) = rect_doc();
        let (removed, index) = doc.remove_object(object_id).unwrap();
        assert_eq!(removed.id, object_id);
        assert_eq!(index, 0);
        assert!(doc.children_of(ObjectParent::Layer(layer_id)).is_empty());
        assert!(doc.object(object_id).is_none());
    }

    #[test]
    fn insert_object_rejects_missing_parent() {
        let mut doc = Document::new("Bad parent");
        let object = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Layer(LayerId::new()),
            Rect::new(0.0, 0.0, 1.0, 1.0),
        );
        let err = doc.insert_object(object, 0).unwrap_err();
        assert!(matches!(err, DocumentError::LayerNotFound(_)));
    }

    #[test]
    fn insert_object_rejects_non_group_parent() {
        let (mut doc, _layer_id, object_id, _rect) = rect_doc();
        let child = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Group(object_id), // object_id is a Path, not a Group
            Rect::new(0.0, 0.0, 1.0, 1.0),
        );
        let err = doc.insert_object(child, 0).unwrap_err();
        assert!(matches!(err, DocumentError::NotAGroup(_)));
    }

    #[test]
    fn document_is_clone_safe_and_preserves_ids() {
        let (doc, layer_id, object_id, _rect) = rect_doc();
        let cloned = doc.clone();
        assert_eq!(cloned.layer(layer_id).unwrap().id, layer_id);
        assert_eq!(cloned.object(object_id).unwrap().id, object_id);
        assert_eq!(doc, cloned);
    }
}
