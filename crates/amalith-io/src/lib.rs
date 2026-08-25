//! `amalith-io`: serialization of Amalith documents to/from the open,
//! zip-based `.amalith` container.
//!
//! ```no_run
//! use amalith_core::Document;
//! use amalith_io::{save, load, AssetStore};
//!
//! let document = Document::new("Untitled");
//! save(&document, &AssetStore::new(), "design.amalith").unwrap();
//! let (loaded, _assets) = load("design.amalith").unwrap();
//! assert_eq!(document, loaded);
//! ```
//!
//! See `manifest.rs` for the on-disk schema and `container.rs` for the
//! save/load implementation.
mod assets;
mod container;
mod error;
mod manifest;

pub use assets::AssetStore;
pub use container::{load, save};
pub use error::IoError;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use amalith_core::{
        Affine, Artboard, ArtboardId, Color, Document, Layer, LayerId, Object, ObjectId,
        ObjectKind, ObjectParent, Rect, Swatch, Vec2,
    };

    #[test]
    fn roundtrip_empty_document() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.amalith");
        let document = Document::new("Empty");

        save(&document, &AssetStore::new(), &path).unwrap();
        let (loaded, assets) = load(&path).unwrap();

        assert_eq!(document, loaded);
        assert!(assets.is_empty());
    }

    #[test]
    fn roundtrip_artboard_layer_rect_move() {
        // Mirrors the brief's Milestone 0.1 flow, exercised at the
        // document/command layer instead of through UI: create doc, add a
        // 1920x1080 artboard, add a rectangle, move it, save, load, and
        // confirm every id/geometry/stacking detail survived identically.
        let dir = tempdir().unwrap();
        let path = dir.path().join("design.amalith");

        let mut document = Document::new("Milestone 0.1");
        let artboard = Artboard::new(
            ArtboardId::new(),
            "Artboard 1",
            Artboard::preset_rect(1920.0, 1080.0),
        );
        let artboard_id = artboard.id;
        document.insert_artboard(artboard, 0);

        let layer = Layer::new(LayerId::new(), "Layer 1");
        let layer_id = layer.id;
        document.insert_layer(layer, 0);

        let rect = Rect::new(100.0, 100.0, 300.0, 250.0);
        let mut object = Object::rectangle(ObjectId::new(), ObjectParent::Layer(layer_id), rect);
        object.name = Some("Rectangle 1".into());
        let object_id = object.id;
        document.insert_object(object, 0).unwrap();

        // "Move it": apply a translation the same way
        // amalith-commands::Command::MoveObject would (see that crate for
        // the command-level equivalent of this transform update).
        let delta = Vec2::new(50.0, -20.0);
        let existing_transform = document.object(object_id).unwrap().transform;
        document.object_mut(object_id).unwrap().transform =
            Affine::translate(delta) * existing_transform;

        document.add_swatch(Swatch::new("Brand Blue", Color::rgb(0.1, 0.3, 0.9)));

        let expected_bounds = document.bounds_of(object_id).unwrap();

        save(&document, &AssetStore::new(), &path).unwrap();
        let (loaded, _assets) = load(&path).unwrap();

        assert_eq!(loaded, document, "full document equality after roundtrip");
        assert_eq!(
            loaded.artboard(artboard_id).unwrap().rect,
            Artboard::preset_rect(1920.0, 1080.0)
        );
        assert_eq!(
            loaded.object(object_id).unwrap().name.as_deref(),
            Some("Rectangle 1")
        );
        assert_eq!(loaded.bounds_of(object_id), Some(expected_bounds));
        assert_eq!(
            loaded.children_of(ObjectParent::Layer(layer_id)),
            &[object_id],
            "stacking order preserved"
        );
        assert_eq!(loaded.swatches().len(), 1);
    }

    #[test]
    fn roundtrip_preserves_group_stacking_order() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("stacking.amalith");

        let mut document = Document::new("Stacking");
        let layer = Layer::new(LayerId::new(), "Layer 1");
        let layer_id = layer.id;
        document.insert_layer(layer, 0);

        let group = Object::new(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            ObjectKind::Group(Default::default()),
        );
        let group_id = group.id;
        document.insert_object(group, 0).unwrap();

        let mut child_ids = Vec::new();
        for i in 0..3 {
            let rect = Rect::new(i as f64 * 10.0, 0.0, i as f64 * 10.0 + 5.0, 5.0);
            let child = Object::rectangle(ObjectId::new(), ObjectParent::Group(group_id), rect);
            child_ids.push(child.id);
            // Insert each new child at the bottom (index 0) so the final
            // order is the reverse of insertion — a real test of order
            // preservation, not just append-friendly luck.
            document.insert_object(child, 0).unwrap();
        }
        child_ids.reverse();

        save(&document, &AssetStore::new(), &path).unwrap();
        let (loaded, _assets) = load(&path).unwrap();

        assert_eq!(
            loaded.children_of(ObjectParent::Group(group_id)),
            child_ids.as_slice()
        );
        assert_eq!(loaded, document);
    }

    #[test]
    fn roundtrip_embeds_and_recovers_asset_bytes() {
        use amalith_core::{Asset, AssetId, AssetKind};

        let dir = tempdir().unwrap();
        let path = dir.path().join("with-asset.amalith");

        let mut document = Document::new("With asset");
        let asset_id = AssetId::new();
        document.add_asset(Asset::embedded(
            asset_id,
            "Texture",
            AssetKind::Image,
            "images/texture-001.png",
        ));

        let mut assets = AssetStore::new();
        let payload = vec![0x89, b'P', b'N', b'G', 1, 2, 3, 4];
        assets.insert("images/texture-001.png", payload.clone());

        save(&document, &assets, &path).unwrap();
        let (loaded, loaded_assets) = load(&path).unwrap();

        assert_eq!(loaded.assets().len(), 1);
        assert_eq!(
            loaded_assets.get("images/texture-001.png"),
            Some(payload.as_slice())
        );
        let _ = loaded.assets()[0].id; // sanity: id survived
        assert_eq!(loaded.assets()[0].id, asset_id);
    }
}
