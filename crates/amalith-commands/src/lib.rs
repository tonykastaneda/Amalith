//! `amalith-commands`: the command engine and undo/redo history.
//!
//! This is the *only* sanctioned way to mutate a [`amalith_core::Document`].
//! GUI tools, keyboard shortcuts, plugins, scripts, the CLI, and AI agents
//! should all depend on this crate rather than poking `amalith-core`'s
//! insert/remove methods directly:
//!
//! ```
//! use amalith_commands::{Command, Editor};
//! use amalith_core::{Document, Rect};
//!
//! let mut editor = Editor::new(Document::new("Untitled"));
//! let outcome = editor.execute(Command::CreateArtboard {
//!     name: "Artboard 1".into(),
//!     rect: Rect::new(0.0, 0.0, 1920.0, 1080.0),
//!     index: None,
//! }).unwrap();
//! assert!(matches!(outcome, amalith_commands::CommandOutcome::Artboard(_)));
//! editor.undo().unwrap();
//! assert!(editor.document().artboards().is_empty());
//! editor.redo().unwrap();
//! assert_eq!(editor.document().artboards().len(), 1);
//! ```
//!
//! See `editor.rs` for why history lives on `Editor` rather than on
//! `Document` itself, and `edit.rs` for why undo/redo replays captured
//! inverse edits rather than re-running commands.
mod align;
mod command;
mod edit;
mod editor;
mod error;
mod history;
mod pathfinder;

pub use align::{AlignKind, AlignTo};
pub use command::{Command, CommandOutcome, GradientRef, PasteStack, PathfinderOp};
pub use pathfinder::has_visible_stroke;
pub use editor::Editor;
pub use error::CommandError;

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::{
        Affine, AssetKind, AssetSource, Color, Document, GradientKind, Layer, LayerId, Object,
        ObjectId, ObjectKind, ObjectParent, Paint, Rect, Vec2,
    };

    fn new_editor() -> Editor {
        Editor::new(Document::new("Test"))
    }

    #[test]
    fn create_artboard_undo_redo_roundtrip() {
        let mut editor = new_editor();
        let outcome = editor
            .execute(Command::CreateArtboard {
                name: "Artboard 1".into(),
                rect: Rect::new(0.0, 0.0, 1920.0, 1080.0),
                index: None,
            })
            .unwrap();
        let CommandOutcome::Artboard(id) = outcome else {
            panic!("expected Artboard outcome");
        };
        assert_eq!(editor.document().artboards().len(), 1);

        editor.undo().unwrap();
        assert!(editor.document().artboards().is_empty());
        assert!(!editor.can_undo());
        assert!(editor.can_redo());

        editor.redo().unwrap();
        assert_eq!(editor.document().artboards().len(), 1);
        // Redo must restore the *same* id, not mint a new one.
        assert_eq!(editor.document().artboards()[0].id, id);
        assert_eq!(
            editor.document().artboards()[0].rect,
            Rect::new(0.0, 0.0, 1920.0, 1080.0)
        );
    }

    #[test]
    fn delete_artboard_undo_restores_index_and_data() {
        let mut editor = new_editor();
        let a = editor
            .execute(Command::CreateArtboard {
                name: "A".into(),
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                index: None,
            })
            .unwrap();
        let b = editor
            .execute(Command::CreateArtboard {
                name: "B".into(),
                rect: Rect::new(0.0, 0.0, 200.0, 200.0),
                index: None,
            })
            .unwrap();
        let CommandOutcome::Artboard(a_id) = a else {
            panic!()
        };
        let CommandOutcome::Artboard(b_id) = b else {
            panic!()
        };

        editor
            .execute(Command::DeleteArtboard { id: a_id })
            .unwrap();
        assert_eq!(editor.document().artboards().len(), 1);
        assert_eq!(editor.document().artboards()[0].id, b_id);

        editor.undo().unwrap();
        assert_eq!(editor.document().artboards().len(), 2);
        // A was originally created first (index 0); undoing the delete
        // must put it back there, not at the end.
        assert_eq!(editor.document().artboards()[0].id, a_id);
        assert_eq!(editor.document().artboards()[1].id, b_id);
    }

    #[test]
    fn rename_and_resize_artboard_undo_redo() {
        let mut editor = new_editor();
        let CommandOutcome::Artboard(id) = editor
            .execute(Command::CreateArtboard {
                name: "Original".into(),
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };

        editor
            .execute(Command::RenameArtboard {
                id,
                name: "Renamed".into(),
            })
            .unwrap();
        editor
            .execute(Command::ResizeArtboard {
                id,
                rect: Rect::new(0.0, 0.0, 500.0, 300.0),
            })
            .unwrap();
        assert_eq!(editor.document().artboard(id).unwrap().name, "Renamed");
        assert_eq!(
            editor.document().artboard(id).unwrap().rect,
            Rect::new(0.0, 0.0, 500.0, 300.0)
        );

        editor.undo().unwrap(); // undo resize
        assert_eq!(
            editor.document().artboard(id).unwrap().rect,
            Rect::new(0.0, 0.0, 100.0, 100.0)
        );
        editor.undo().unwrap(); // undo rename
        assert_eq!(editor.document().artboard(id).unwrap().name, "Original");

        editor.redo().unwrap();
        editor.redo().unwrap();
        assert_eq!(editor.document().artboard(id).unwrap().name, "Renamed");
        assert_eq!(
            editor.document().artboard(id).unwrap().rect,
            Rect::new(0.0, 0.0, 500.0, 300.0)
        );
    }

    #[test]
    fn create_layer_and_rect_then_move_undo_redo() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer_id) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };

        let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
        let CommandOutcome::Object(object_id) = editor
            .execute(Command::CreateRect {
                layer: layer_id,
                rect,
                name: Some("Rectangle 1".into()),
            })
            .unwrap()
        else {
            panic!()
        };

        assert_eq!(editor.document().bounds_of(object_id), Some(rect));
        assert_eq!(
            editor.document().children_of(ObjectParent::Layer(layer_id)),
            &[object_id]
        );

        editor
            .execute(Command::MoveObject {
                object: object_id,
                delta: Vec2::new(30.0, 10.0),
            })
            .unwrap();
        let moved = Rect::new(30.0, 10.0, 130.0, 60.0);
        assert_eq!(editor.document().bounds_of(object_id), Some(moved));

        editor.undo().unwrap(); // undo move
        assert_eq!(editor.document().bounds_of(object_id), Some(rect));

        editor.redo().unwrap(); // redo move
        assert_eq!(editor.document().bounds_of(object_id), Some(moved));

        editor.undo().unwrap(); // undo move
        editor.undo().unwrap(); // undo create rect
        assert!(editor.document().object(object_id).is_none());
        assert!(editor
            .document()
            .children_of(ObjectParent::Layer(layer_id))
            .is_empty());

        editor.undo().unwrap(); // undo create layer
        assert!(editor.document().layers().is_empty());
        assert!(!editor.can_undo());

        // Full redo replays create-layer, create-rect, move in order and
        // must land on identical ids and geometry.
        editor.redo().unwrap();
        editor.redo().unwrap();
        editor.redo().unwrap();
        assert_eq!(editor.document().layers()[0].id, layer_id);
        assert_eq!(editor.document().bounds_of(object_id), Some(moved));
    }

    #[test]
    fn create_image_links_an_asset_and_undo_removes_both() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(id) = editor
            .execute(Command::CreateImage {
                layer,
                path: "/tmp/photo.png".into(),
                bounds: Rect::new(0.0, 0.0, 200.0, 100.0),
                transform: Affine::translate((10.0, 20.0)),
                name: Some("photo".into()),
                embedded: false,
            })
            .unwrap()
        else {
            panic!()
        };
        let obj = editor.document().object(id).unwrap();
        let ObjectKind::Image(img) = &obj.kind else {
            panic!("expected an image object");
        };
        assert_eq!(obj.name.as_deref(), Some("photo"));
        assert_eq!(img.local_bounds, Rect::new(0.0, 0.0, 200.0, 100.0));
        assert_eq!(editor.document().assets().len(), 1);
        assert_eq!(editor.document().assets()[0].kind, AssetKind::Image);
        assert!(matches!(
            &editor.document().assets()[0].source,
            AssetSource::Linked { path } if path == "/tmp/photo.png"
        ));
        assert_eq!(editor.document().bounds_of(id), Some(Rect::new(10.0, 20.0, 210.0, 120.0)));

        editor.undo().unwrap();
        assert!(editor.document().object(id).is_none());
        assert!(editor.document().assets().is_empty());

        editor.redo().unwrap();
        assert!(editor.document().object(id).is_some());
        assert_eq!(editor.document().assets().len(), 1);
    }

    #[test]
    fn set_transform_undo_redo() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer_id) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(object_id) = editor
            .execute(Command::CreateRect {
                layer: layer_id,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };

        let transform = Affine::translate((5.0, 5.0)) * Affine::scale(2.0);
        editor
            .execute(Command::SetTransform {
                object: object_id,
                transform,
            })
            .unwrap();
        assert_eq!(
            editor.document().object(object_id).unwrap().transform,
            transform
        );

        editor.undo().unwrap();
        assert_eq!(
            editor.document().object(object_id).unwrap().transform,
            Affine::IDENTITY
        );

        editor.redo().unwrap();
        assert_eq!(
            editor.document().object(object_id).unwrap().transform,
            transform
        );
    }

    #[test]
    fn move_anchors_undo_redo() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer_id) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(object_id) = editor
            .execute(Command::CreateEllipse {
                layer: layer_id,
                rect: Rect::new(0.0, 0.0, 100.0, 80.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let before = match &editor.document().object(object_id).unwrap().kind {
            ObjectKind::Path(path) => path.geometry.clone(),
            _ => panic!(),
        };

        editor
            .execute(Command::MoveAnchors {
                anchors: vec![(object_id, 0)],
                delta: Vec2::new(12.0, -6.0),
            })
            .unwrap();
        let moved = match &editor.document().object(object_id).unwrap().kind {
            ObjectKind::Path(path) => path.geometry.clone(),
            _ => panic!(),
        };
        assert_ne!(moved, before);

        editor.undo().unwrap();
        assert_eq!(
            match &editor.document().object(object_id).unwrap().kind {
                ObjectKind::Path(path) => path.geometry.clone(),
                _ => panic!(),
            },
            before
        );

        editor.redo().unwrap();
        assert_eq!(
            match &editor.document().object(object_id).unwrap().kind {
                ObjectKind::Path(path) => path.geometry.clone(),
                _ => panic!(),
            },
            moved
        );
    }

    #[test]
    fn move_anchors_on_two_objects_undoes_as_one_step() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer_id) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let create = |editor: &mut Editor, rect| match editor
            .execute(Command::CreateEllipse {
                layer: layer_id,
                rect,
                name: None,
            })
            .unwrap()
        {
            CommandOutcome::Object(id) => id,
            _ => panic!(),
        };
        let first = create(&mut editor, Rect::new(0.0, 0.0, 20.0, 20.0));
        let second = create(&mut editor, Rect::new(50.0, 0.0, 70.0, 20.0));
        let before_first = match &editor.document().object(first).unwrap().kind {
            ObjectKind::Path(path) => path.geometry.clone(),
            _ => panic!(),
        };
        let before_second = match &editor.document().object(second).unwrap().kind {
            ObjectKind::Path(path) => path.geometry.clone(),
            _ => panic!(),
        };

        editor
            .execute(Command::MoveAnchors {
                anchors: vec![(first, 0), (second, 2), (first, 0)],
                delta: Vec2::new(3.0, 4.0),
            })
            .unwrap();
        assert_ne!(
            match &editor.document().object(first).unwrap().kind {
                ObjectKind::Path(path) => path.geometry.clone(),
                _ => panic!(),
            },
            before_first
        );
        assert_ne!(
            match &editor.document().object(second).unwrap().kind {
                ObjectKind::Path(path) => path.geometry.clone(),
                _ => panic!(),
            },
            before_second
        );

        editor.undo().unwrap();
        assert_eq!(
            match &editor.document().object(first).unwrap().kind {
                ObjectKind::Path(path) => path.geometry.clone(),
                _ => panic!(),
            },
            before_first
        );
        assert_eq!(
            match &editor.document().object(second).unwrap().kind {
                ObjectKind::Path(path) => path.geometry.clone(),
                _ => panic!(),
            },
            before_second
        );
    }

    #[test]
    fn duplicate_object_preserves_original_and_undo_removes_copy() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer_id) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let rect = Rect::new(0.0, 0.0, 20.0, 10.0);
        let CommandOutcome::Object(original) = editor
            .execute(Command::CreateRect {
                layer: layer_id,
                rect,
                name: Some("Rectangle 1".into()),
            })
            .unwrap()
        else {
            panic!()
        };
        let original_transform = editor.document().object(original).unwrap().transform;

        let CommandOutcome::Object(copy) = editor
            .execute(Command::DuplicateObject {
                object: original,
                delta: Vec2::new(30.0, 7.0),
            })
            .unwrap()
        else {
            panic!()
        };

        assert_ne!(copy, original);
        assert_eq!(
            editor.document().object(original).unwrap().transform,
            original_transform
        );
        assert_eq!(
            editor.document().bounds_of(copy),
            Some(Rect::new(30.0, 7.0, 50.0, 17.0))
        );

        editor.undo().unwrap();
        assert!(editor.document().object(copy).is_none());
        assert!(editor.document().object(original).is_some());
    }

    #[test]
    fn delete_object_undo_restores_id_and_geometry() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer_id) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let rect = Rect::new(3.0, 4.0, 23.0, 14.0);
        let CommandOutcome::Object(object) = editor
            .execute(Command::CreateRect {
                layer: layer_id,
                rect,
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };

        editor
            .execute(Command::DeleteObject { id: object })
            .unwrap();
        assert!(editor.document().object(object).is_none());
        editor.undo().unwrap();
        assert_eq!(editor.document().bounds_of(object), Some(rect));
    }

    #[test]
    fn delete_artboard_undo_restores_selection_target() {
        let mut editor = new_editor();
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let CommandOutcome::Artboard(id) = editor
            .execute(Command::CreateArtboard {
                name: "Artboard 1".into(),
                rect,
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };

        editor.execute(Command::DeleteArtboard { id }).unwrap();
        assert!(editor.document().artboard(id).is_none());
        editor.undo().unwrap();
        assert_eq!(editor.document().artboard(id).unwrap().rect, rect);
    }

    fn editor_with_artboard_and_rect(
        object_rect: Rect,
    ) -> (Editor, amalith_core::ArtboardId, amalith_core::ObjectId) {
        let mut editor = new_editor();
        let CommandOutcome::Artboard(artboard) = editor
            .execute(Command::CreateArtboard {
                name: "Artboard 1".into(),
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(object) = editor
            .execute(Command::CreateRect {
                layer,
                rect: object_rect,
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        (editor, artboard, object)
    }

    #[test]
    fn move_artboard_moves_intersecting_artwork_and_undo_restores_both() {
        let object_rect = Rect::new(10.0, 20.0, 40.0, 50.0);
        let (mut editor, artboard, object) = editor_with_artboard_and_rect(object_rect);
        let delta = Vec2::new(120.0, 35.0);

        editor
            .execute(Command::MoveArtboard {
                id: artboard,
                delta,
            })
            .unwrap();
        assert_eq!(
            editor.document().artboard(artboard).unwrap().rect,
            Rect::new(120.0, 35.0, 220.0, 135.0)
        );
        assert_eq!(
            editor.document().bounds_of(object),
            Some(object_rect + delta)
        );

        editor.undo().unwrap();
        assert_eq!(
            editor.document().artboard(artboard).unwrap().rect,
            Rect::new(0.0, 0.0, 100.0, 100.0)
        );
        assert_eq!(editor.document().bounds_of(object), Some(object_rect));
    }

    #[test]
    fn move_artboard_leaves_outside_artwork_behind() {
        let object_rect = Rect::new(150.0, 20.0, 180.0, 50.0);
        let (mut editor, artboard, object) = editor_with_artboard_and_rect(object_rect);

        editor
            .execute(Command::MoveArtboard {
                id: artboard,
                delta: Vec2::new(40.0, 10.0),
            })
            .unwrap();
        assert_eq!(editor.document().bounds_of(object), Some(object_rect));
    }

    #[test]
    fn resize_artboard_does_not_move_artwork() {
        let object_rect = Rect::new(10.0, 20.0, 40.0, 50.0);
        let (mut editor, artboard, object) = editor_with_artboard_and_rect(object_rect);

        editor
            .execute(Command::ResizeArtboard {
                id: artboard,
                rect: Rect::new(0.0, 0.0, 180.0, 140.0),
            })
            .unwrap();
        assert_eq!(editor.document().bounds_of(object), Some(object_rect));
    }

    #[test]
    fn duplicate_artboard_copies_intersecting_artwork_and_undo_removes_only_copies() {
        let object_rect = Rect::new(10.0, 20.0, 40.0, 50.0);
        let (mut editor, artboard, original) = editor_with_artboard_and_rect(object_rect);
        let delta = Vec2::new(120.0, 35.0);

        let CommandOutcome::Artboard(copy_artboard) = editor
            .execute(Command::DuplicateArtboard {
                id: artboard,
                delta,
            })
            .unwrap()
        else {
            panic!("duplicate should return its new artboard id")
        };
        assert_eq!(editor.document().artboards().len(), 2);
        assert_eq!(
            editor.document().artboard(copy_artboard).unwrap().rect,
            Rect::new(120.0, 35.0, 220.0, 135.0)
        );
        assert_eq!(editor.document().bounds_of(original), Some(object_rect));

        let copied_objects: Vec<_> = editor
            .document()
            .objects()
            .filter(|object| object.id != original)
            .collect();
        assert_eq!(copied_objects.len(), 1);
        assert_eq!(
            editor.document().bounds_of(copied_objects[0].id),
            Some(object_rect + delta)
        );

        editor.undo().unwrap();
        assert_eq!(editor.document().artboards().len(), 1);
        assert!(editor.document().artboard(copy_artboard).is_none());
        assert_eq!(editor.document().objects().count(), 1);
        assert_eq!(editor.document().bounds_of(original), Some(object_rect));
    }

    #[test]
    fn stack_nudge_swaps_paint_order_and_undo_restores_it() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let mut create = |name: &str| {
            let CommandOutcome::Object(id) = editor
                .execute(Command::CreateRect {
                    layer,
                    rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                    name: Some(name.into()),
                })
                .unwrap()
            else {
                panic!()
            };
            id
        };
        let bottom = create("Bottom");
        let top = create("Top");
        let parent = ObjectParent::Layer(layer);

        editor
            .execute(Command::NudgeStack {
                ids: vec![bottom],
                steps: 1,
            })
            .unwrap();
        assert_eq!(editor.document().children_of(parent), &[top, bottom]);
        editor.undo().unwrap();
        assert_eq!(editor.document().children_of(parent), &[bottom, top]);

        editor
            .execute(Command::NudgeStack {
                ids: vec![top],
                steps: -1,
            })
            .unwrap();
        assert_eq!(editor.document().children_of(parent), &[top, bottom]);
        editor.undo().unwrap();
        assert_eq!(editor.document().children_of(parent), &[bottom, top]);
    }

    #[test]
    fn stack_nudge_at_front_is_a_no_op() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(top) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: Some("Top".into()),
            })
            .unwrap()
        else {
            panic!()
        };
        editor
            .execute(Command::NudgeStack {
                ids: vec![top],
                steps: 1,
            })
            .unwrap();
        assert_eq!(
            editor.document().children_of(ObjectParent::Layer(layer)),
            &[top]
        );
        editor.undo().unwrap();
        assert!(editor.document().object(top).is_none());
    }

    #[test]
    fn stack_nudge_reorders_within_a_group_not_just_layers() {
        // `NudgeStack` must key off each selected object's *actual* parent
        // (layer or group), not just walk `Document::layers()` — otherwise
        // nudging objects nested in a group is a silent no-op.
        let mut document = Document::new("Test");
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

        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let bottom = Object::rectangle(ObjectId::new(), ObjectParent::Group(group_id), rect);
        let bottom_id = bottom.id;
        document.insert_object(bottom, 0).unwrap();
        let top = Object::rectangle(ObjectId::new(), ObjectParent::Group(group_id), rect);
        let top_id = top.id;
        document.insert_object(top, 1).unwrap();

        let mut editor = Editor::new(document);
        let parent = ObjectParent::Group(group_id);
        assert_eq!(editor.document().children_of(parent), &[bottom_id, top_id]);

        editor
            .execute(Command::NudgeStack {
                ids: vec![bottom_id],
                steps: 1,
            })
            .unwrap();
        assert_eq!(editor.document().children_of(parent), &[top_id, bottom_id]);

        editor.undo().unwrap();
        assert_eq!(editor.document().children_of(parent), &[bottom_id, top_id]);
    }

    #[test]
    fn stack_nudge_moves_multiple_ids_together_preserving_relative_order() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let create = |editor: &mut Editor, name: &str| {
            let CommandOutcome::Object(id) = editor
                .execute(Command::CreateRect {
                    layer,
                    rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                    name: Some(name.into()),
                })
                .unwrap()
            else {
                panic!()
            };
            id
        };
        // Bottom to top: A, B, C, D.
        let a = create(&mut editor, "A");
        let b = create(&mut editor, "B");
        let c = create(&mut editor, "C");
        let d = create(&mut editor, "D");
        let parent = ObjectParent::Layer(layer);
        assert_eq!(editor.document().children_of(parent), &[a, b, c, d]);

        // Bring A and C forward together: each hops over its non-selected
        // neighbor, landing as [B, A, D, C] — order between A and C
        // preserved, and they don't leapfrog each other.
        editor
            .execute(Command::NudgeStack {
                ids: vec![a, c],
                steps: 1,
            })
            .unwrap();
        assert_eq!(editor.document().children_of(parent), &[b, a, d, c]);

        editor.undo().unwrap();
        assert_eq!(editor.document().children_of(parent), &[a, b, c, d]);
    }

    #[test]
    fn stack_nudge_clamps_at_the_end_instead_of_wrapping() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let create = |editor: &mut Editor, name: &str| {
            let CommandOutcome::Object(id) = editor
                .execute(Command::CreateRect {
                    layer,
                    rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                    name: Some(name.into()),
                })
                .unwrap()
            else {
                panic!()
            };
            id
        };
        let bottom = create(&mut editor, "Bottom");
        let top = create(&mut editor, "Top");
        let parent = ObjectParent::Layer(layer);

        // Sending the bottom object back by more steps than there are
        // positions must clamp at index 0, not wrap around to the front.
        editor
            .execute(Command::NudgeStack {
                ids: vec![bottom],
                steps: -50,
            })
            .unwrap();
        assert_eq!(editor.document().children_of(parent), &[bottom, top]);

        // Bringing it forward by more steps than there are positions must
        // clamp at the front, not wrap to the back.
        editor
            .execute(Command::NudgeStack {
                ids: vec![bottom],
                steps: 50,
            })
            .unwrap();
        assert_eq!(editor.document().children_of(parent), &[top, bottom]);
    }

    #[test]
    fn undo_with_empty_history_errors() {
        let mut editor = new_editor();
        assert_eq!(editor.undo().unwrap_err(), CommandError::NothingToUndo);
        assert_eq!(editor.redo().unwrap_err(), CommandError::NothingToRedo);
    }

    #[test]
    fn executing_new_command_clears_redo_stack() {
        let mut editor = new_editor();
        editor
            .execute(Command::CreateArtboard {
                name: "A".into(),
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                index: None,
            })
            .unwrap();
        editor.undo().unwrap();
        assert!(editor.can_redo());

        editor
            .execute(Command::CreateArtboard {
                name: "B".into(),
                rect: Rect::new(0.0, 0.0, 20.0, 20.0),
                index: None,
            })
            .unwrap();
        assert!(!editor.can_redo());
    }

    #[test]
    fn editor_bounds_of_agrees_with_document_bounds_of() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer_id) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
        let CommandOutcome::Object(object_id) = editor
            .execute(Command::CreateRect {
                layer: layer_id,
                rect,
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };

        assert_eq!(editor.document().bounds_of(object_id), Some(rect));
        assert_eq!(editor.bounds_of(object_id), Some(rect));
    }

    #[test]
    fn editor_bounds_of_is_not_stale_after_move() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer_id) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
        let CommandOutcome::Object(object_id) = editor
            .execute(Command::CreateRect {
                layer: layer_id,
                rect,
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };

        // Populate the cache before the move, so this actually exercises
        // invalidation rather than a lucky first read.
        assert_eq!(editor.bounds_of(object_id), Some(rect));

        editor
            .execute(Command::MoveObject {
                object: object_id,
                delta: Vec2::new(30.0, 10.0),
            })
            .unwrap();
        let moved = Rect::new(30.0, 10.0, 130.0, 60.0);
        assert_eq!(editor.bounds_of(object_id), Some(moved));
        assert_eq!(editor.document().bounds_of(object_id), Some(moved));
    }

    #[test]
    fn editor_bounds_of_restored_by_undo() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer_id) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
        let CommandOutcome::Object(object_id) = editor
            .execute(Command::CreateRect {
                layer: layer_id,
                rect,
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };

        editor
            .execute(Command::MoveObject {
                object: object_id,
                delta: Vec2::new(30.0, 10.0),
            })
            .unwrap();
        let moved = Rect::new(30.0, 10.0, 130.0, 60.0);
        assert_eq!(editor.bounds_of(object_id), Some(moved));

        editor.undo().unwrap();
        assert_eq!(editor.bounds_of(object_id), Some(rect));

        editor.redo().unwrap();
        assert_eq!(editor.bounds_of(object_id), Some(moved));
    }

    #[test]
    fn paste_in_place_copies_two_rects_with_new_ids_and_same_bounds_undo_redo() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(a) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: Some("A".into()),
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(b) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(20.0, 0.0, 30.0, 10.0),
                name: Some("B".into()),
            })
            .unwrap()
        else {
            panic!()
        };
        let a_bounds = editor.document().bounds_of(a).unwrap();
        let b_bounds = editor.document().bounds_of(b).unwrap();

        editor.copy(&[a, b]).unwrap();
        assert!(editor.has_clipboard());

        let before = editor.document().objects().count();
        let new_ids = editor.paste(Vec2::ZERO, PasteStack::Top).unwrap();
        assert_eq!(new_ids.len(), 2);
        assert!(!new_ids.contains(&a));
        assert!(!new_ids.contains(&b));
        assert_eq!(editor.document().objects().count(), before + 2);
        assert_eq!(editor.document().bounds_of(new_ids[0]), Some(a_bounds));
        assert_eq!(editor.document().bounds_of(new_ids[1]), Some(b_bounds));
        // Sources are untouched.
        assert_eq!(editor.document().bounds_of(a), Some(a_bounds));
        assert_eq!(editor.document().bounds_of(b), Some(b_bounds));

        editor.undo().unwrap();
        assert_eq!(editor.document().objects().count(), before);
        assert!(editor.document().object(new_ids[0]).is_none());
        assert!(editor.document().object(new_ids[1]).is_none());

        editor.redo().unwrap();
        assert_eq!(editor.document().objects().count(), before + 2);
        assert_eq!(editor.document().bounds_of(new_ids[0]), Some(a_bounds));
        assert_eq!(editor.document().bounds_of(new_ids[1]), Some(b_bounds));
    }

    #[test]
    fn paste_with_delta_moves_only_the_clone() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(source) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let source_bounds = editor.document().bounds_of(source).unwrap();

        editor.copy(&[source]).unwrap();
        let delta = Vec2::new(50.0, 5.0);
        let new_ids = editor.paste(delta, PasteStack::Top).unwrap();
        let clone_id = new_ids[0];

        assert_eq!(editor.document().bounds_of(source), Some(source_bounds));
        assert_eq!(
            editor.document().bounds_of(clone_id),
            Some(source_bounds + delta)
        );
    }

    fn three_stacked_rects(editor: &mut Editor) -> (LayerId, ObjectId, ObjectId, ObjectId) {
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let create = |editor: &mut Editor, name: &str| {
            let CommandOutcome::Object(id) = editor
                .execute(Command::CreateRect {
                    layer,
                    rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                    name: Some(name.into()),
                })
                .unwrap()
            else {
                panic!()
            };
            id
        };
        let bottom = create(editor, "Bottom");
        let source = create(editor, "Source");
        let top = create(editor, "Top");
        (layer, bottom, source, top)
    }

    #[test]
    fn paste_in_front_places_clone_as_next_sibling_after_source() {
        let mut editor = new_editor();
        let (layer, bottom, source, top) = three_stacked_rects(&mut editor);
        let parent = ObjectParent::Layer(layer);
        assert_eq!(
            editor.document().children_of(parent),
            &[bottom, source, top]
        );

        editor.copy(&[source]).unwrap();
        let new_ids = editor.paste(Vec2::ZERO, PasteStack::InFront).unwrap();
        let clone_id = new_ids[0];

        assert_eq!(
            editor.document().children_of(parent),
            &[bottom, source, clone_id, top]
        );
    }

    #[test]
    fn paste_in_back_places_clone_as_previous_sibling() {
        let mut editor = new_editor();
        let (layer, bottom, source, top) = three_stacked_rects(&mut editor);
        let parent = ObjectParent::Layer(layer);

        editor.copy(&[source]).unwrap();
        let new_ids = editor.paste(Vec2::ZERO, PasteStack::Behind).unwrap();
        let clone_id = new_ids[0];

        assert_eq!(
            editor.document().children_of(parent),
            &[bottom, clone_id, source, top]
        );
    }

    #[test]
    fn copy_group_deep_copies_children_with_new_ids() {
        let mut document = Document::new("Test");
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

        let child_a = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Group(group_id),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        let child_a_id = child_a.id;
        document.insert_object(child_a, 0).unwrap();
        let child_b = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Group(group_id),
            Rect::new(20.0, 0.0, 30.0, 10.0),
        );
        let child_b_id = child_b.id;
        document.insert_object(child_b, 1).unwrap();

        let mut editor = Editor::new(document);
        editor.copy(&[group_id]).unwrap();

        let new_ids = editor.paste(Vec2::ZERO, PasteStack::Top).unwrap();
        let new_group_id = new_ids[0];
        assert_ne!(new_group_id, group_id);

        let new_children = editor
            .document()
            .children_of(ObjectParent::Group(new_group_id))
            .to_vec();
        assert_eq!(new_children.len(), 2);
        assert!(!new_children.contains(&child_a_id));
        assert!(!new_children.contains(&child_b_id));
        assert_eq!(
            editor.document().bounds_of(new_group_id),
            editor.document().bounds_of(group_id)
        );

        // Deep copy: the new tree doesn't share child objects with the
        // original, so moving a new child leaves the original untouched.
        editor
            .execute(Command::MoveObject {
                object: new_children[0],
                delta: Vec2::new(100.0, 0.0),
            })
            .unwrap();
        assert_ne!(
            editor.document().bounds_of(group_id),
            editor.document().bounds_of(new_group_id)
        );
        assert_eq!(
            editor.document().children_of(ObjectParent::Group(group_id)),
            &[child_a_id, child_b_id]
        );
    }

    #[test]
    fn copy_then_delete_original_then_paste_in_place_still_works() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(source) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let source_bounds = editor.document().bounds_of(source).unwrap();

        editor.copy(&[source]).unwrap();
        editor
            .execute(Command::DeleteObject { id: source })
            .unwrap();
        assert!(editor.document().object(source).is_none());
        assert!(editor.has_clipboard());

        let new_ids = editor.paste(Vec2::ZERO, PasteStack::Top).unwrap();
        assert_eq!(editor.document().bounds_of(new_ids[0]), Some(source_bounds));
        // Falls back to the recorded parent layer, which still exists even
        // though the copied source object itself is gone.
        assert_eq!(
            editor.document().children_of(ObjectParent::Layer(layer)),
            &[new_ids[0]]
        );
    }

    #[test]
    fn paste_with_empty_clipboard_errors_cleanly() {
        let mut editor = new_editor();
        assert!(!editor.has_clipboard());

        let err = editor.paste(Vec2::ZERO, PasteStack::Top).unwrap_err();
        assert_eq!(err, CommandError::EmptyClipboard);

        let err = editor
            .execute(Command::Paste {
                delta: Vec2::ZERO,
                stack: PasteStack::Top,
            })
            .unwrap_err();
        assert_eq!(err, CommandError::EmptyClipboard);
    }

    #[test]
    fn copy_from_svg_pastes_external_content_without_a_gui() {
        // The headless path: no `Document` the shape ever lived in, no
        // GUI, no OS clipboard — exactly what a CLI or agent has to work
        // with when importing SVG someone handed it directly. A layer
        // still has to exist to paste into, same as any other insert (see
        // `RectangleTool::finish_drag`'s identical create-if-absent).
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg">
            <rect x="10" y="20" width="100" height="50" transform="translate(5 5)" />
        </svg>"#;

        let mut editor = new_editor();
        editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap();

        editor.copy_from_svg(svg).unwrap();
        assert!(editor.has_clipboard());
        assert_eq!(
            editor.clipboard_bounds(),
            Some(Rect::new(15.0, 25.0, 115.0, 75.0))
        );

        let new_ids = editor.paste(Vec2::ZERO, PasteStack::Top).unwrap();
        assert_eq!(new_ids.len(), 1);
        assert_eq!(
            editor.document().bounds_of(new_ids[0]),
            Some(Rect::new(15.0, 25.0, 115.0, 75.0))
        );

        // Pasting into a document with no layer at all fails cleanly
        // rather than silently fabricating one.
        let mut empty_editor = new_editor();
        empty_editor.copy_from_svg(svg).unwrap();
        assert_eq!(
            empty_editor.paste(Vec2::ZERO, PasteStack::Top).unwrap_err(),
            CommandError::NoLayerAvailable
        );
    }

    #[test]
    fn copy_from_svg_rejects_non_svg_text() {
        let mut editor = new_editor();
        let err = editor.copy_from_svg("not svg").unwrap_err();
        assert!(matches!(err, CommandError::SvgImport(_)));
        assert!(!editor.has_clipboard());
    }

    #[test]
    fn duplicate_objects_preserves_relative_order_and_undoes_as_one_group() {
        let mut editor = new_editor();
        let (layer, bottom, top) = {
            let CommandOutcome::Layer(layer) = editor
                .execute(Command::CreateLayer {
                    name: "Layer 1".into(),
                    index: None,
                })
                .unwrap()
            else {
                panic!()
            };
            let CommandOutcome::Object(bottom) = editor
                .execute(Command::CreateRect {
                    layer,
                    rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                    name: Some("Bottom".into()),
                })
                .unwrap()
            else {
                panic!()
            };
            let CommandOutcome::Object(top) = editor
                .execute(Command::CreateRect {
                    layer,
                    rect: Rect::new(20.0, 0.0, 30.0, 10.0),
                    name: Some("Top".into()),
                })
                .unwrap()
            else {
                panic!()
            };
            (layer, bottom, top)
        };
        let parent = ObjectParent::Layer(layer);
        let before = editor.document().objects().count();

        let delta = Vec2::new(5.0, 5.0);
        let new_ids = editor.duplicate_objects(&[bottom, top], delta).unwrap();

        assert_eq!(new_ids.len(), 2);
        assert_eq!(editor.document().objects().count(), before + 2);
        // Both originals plus both duplicates, in the same relative order
        // the source ids were given.
        assert_eq!(
            editor.document().children_of(parent),
            &[bottom, top, new_ids[0], new_ids[1]]
        );
        assert_eq!(
            editor.document().bounds_of(new_ids[0]),
            Some(Rect::new(0.0, 0.0, 10.0, 10.0) + delta)
        );
        assert_eq!(
            editor.document().bounds_of(new_ids[1]),
            Some(Rect::new(20.0, 0.0, 30.0, 10.0) + delta)
        );

        editor.undo().unwrap();
        assert_eq!(editor.document().objects().count(), before);
        assert!(editor.document().object(new_ids[0]).is_none());
        assert!(editor.document().object(new_ids[1]).is_none());
        assert_eq!(editor.document().children_of(parent), &[bottom, top]);
    }

    #[test]
    fn duplicate_objects_deep_copies_a_group_without_sharing_children() {
        let mut document = Document::new("Test");
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
        let child = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Group(group_id),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        let child_id = child.id;
        document.insert_object(child, 0).unwrap();

        let mut editor = Editor::new(document);
        let new_ids = editor
            .duplicate_objects(&[group_id], Vec2::new(50.0, 0.0))
            .unwrap();
        let new_group_id = new_ids[0];

        assert_ne!(new_group_id, group_id);
        let new_children = editor
            .document()
            .children_of(ObjectParent::Group(new_group_id));
        assert_eq!(new_children.len(), 1);
        assert_ne!(new_children[0], child_id);
        // Original group untouched.
        assert_eq!(
            editor.document().children_of(ObjectParent::Group(group_id)),
            &[child_id]
        );
    }

    #[test]
    fn duplicate_objects_never_touches_the_clipboard() {
        // The bug this exists to fix: alt-drag-duplicating a multi
        // selection must not clobber whatever the user last copied with
        // Cmd+C — they're unrelated actions that happen to share some
        // deep-copy machinery internally.
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(copied) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: Some("Copied".into()),
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(dragged) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(20.0, 0.0, 30.0, 10.0),
                name: Some("Dragged".into()),
            })
            .unwrap()
        else {
            panic!()
        };

        editor.copy(&[copied]).unwrap();
        let clipboard_bounds_before = editor.clipboard_bounds();

        editor
            .duplicate_objects(&[dragged], Vec2::new(1.0, 1.0))
            .unwrap();

        assert!(editor.has_clipboard());
        assert_eq!(editor.clipboard_bounds(), clipboard_bounds_before);
        // Pasting still yields a copy of `copied`, not the dragged object.
        let pasted = editor.paste(Vec2::ZERO, PasteStack::InFront).unwrap();
        assert_eq!(
            editor.document().bounds_of(pasted[0]),
            editor.document().bounds_of(copied)
        );
    }

    #[test]
    fn duplicate_objects_errors_cleanly_on_an_empty_list() {
        let mut editor = new_editor();
        assert_eq!(
            editor.duplicate_objects(&[], Vec2::ZERO).unwrap_err(),
            CommandError::NothingToDuplicate
        );
    }

    #[test]
    fn group_preserves_stacking_position_and_child_order_undo_redo() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let create = |editor: &mut Editor, name: &str| {
            let CommandOutcome::Object(id) = editor
                .execute(Command::CreateRect {
                    layer,
                    rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                    name: Some(name.into()),
                })
                .unwrap()
            else {
                panic!()
            };
            id
        };
        // Bottom to top: a, b, c, d.
        let a = create(&mut editor, "A");
        let b = create(&mut editor, "B");
        let c = create(&mut editor, "C");
        let d = create(&mut editor, "D");
        let parent = ObjectParent::Layer(layer);
        let bounds_before: Vec<_> = [a, b, c, d]
            .iter()
            .map(|&id| editor.document().bounds_of(id))
            .collect();

        // Group the two middle objects (b, c) — a stays below the group,
        // d stays above it, matching where the topmost grouped object (c)
        // originally sat.
        let outcome = editor
            .execute(Command::Group {
                ids: vec![b, c],
                name: Some("My Group".into()),
            })
            .unwrap();
        let CommandOutcome::Object(group_id) = outcome else {
            panic!()
        };

        assert_eq!(editor.document().children_of(parent), &[a, group_id, d]);
        assert_eq!(
            editor.document().children_of(ObjectParent::Group(group_id)),
            &[b, c]
        );
        assert_eq!(
            editor.document().object(group_id).unwrap().name.as_deref(),
            Some("My Group")
        );
        // Grouping must not move anything on screen.
        for (&id, before) in [a, b, c, d].iter().zip(&bounds_before) {
            assert_eq!(editor.document().bounds_of(id), *before);
        }

        editor.undo().unwrap();
        assert_eq!(editor.document().children_of(parent), &[a, b, c, d]);
        assert!(editor.document().object(group_id).is_none());

        editor.redo().unwrap();
        assert_eq!(editor.document().children_of(parent), &[a, group_id, d]);
        assert_eq!(
            editor.document().children_of(ObjectParent::Group(group_id)),
            &[b, c]
        );
    }

    #[test]
    fn group_errors_cleanly_on_an_empty_list() {
        let mut editor = new_editor();
        assert_eq!(
            editor
                .execute(Command::Group {
                    ids: vec![],
                    name: None,
                })
                .unwrap_err(),
            CommandError::NothingToGroup
        );
    }

    #[test]
    fn group_errors_when_objects_span_different_layers() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer_a) = editor
            .execute(Command::CreateLayer {
                name: "A".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Layer(layer_b) = editor
            .execute(Command::CreateLayer {
                name: "B".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(a) = editor
            .execute(Command::CreateRect {
                layer: layer_a,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(b) = editor
            .execute(Command::CreateRect {
                layer: layer_b,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            editor
                .execute(Command::Group {
                    ids: vec![a, b],
                    name: None,
                })
                .unwrap_err(),
            CommandError::ObjectsSpanMultipleParents
        );
    }

    #[test]
    fn rename_layer_undo_redo() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Original".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        editor
            .execute(Command::RenameLayer {
                id: layer,
                name: "Renamed".into(),
            })
            .unwrap();
        assert_eq!(editor.document().layer(layer).unwrap().name, "Renamed");

        editor.undo().unwrap();
        assert_eq!(editor.document().layer(layer).unwrap().name, "Original");

        editor.redo().unwrap();
        assert_eq!(editor.document().layer(layer).unwrap().name, "Renamed");
    }

    #[test]
    fn rename_object_undo_redo_including_clearing_the_name() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(object) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: Some("Original".into()),
            })
            .unwrap()
        else {
            panic!()
        };

        editor
            .execute(Command::RenameObject {
                id: object,
                name: Some("Renamed".into()),
            })
            .unwrap();
        assert_eq!(
            editor.document().object(object).unwrap().name.as_deref(),
            Some("Renamed")
        );

        editor
            .execute(Command::RenameObject {
                id: object,
                name: None,
            })
            .unwrap();
        assert_eq!(editor.document().object(object).unwrap().name, None);

        editor.undo().unwrap();
        assert_eq!(
            editor.document().object(object).unwrap().name.as_deref(),
            Some("Renamed")
        );
        editor.undo().unwrap();
        assert_eq!(
            editor.document().object(object).unwrap().name.as_deref(),
            Some("Original")
        );
    }

    #[test]
    fn ungroup_restores_children_at_the_groups_position_undo_redo() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let create = |editor: &mut Editor, name: &str| {
            let CommandOutcome::Object(id) = editor
                .execute(Command::CreateRect {
                    layer,
                    rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                    name: Some(name.into()),
                })
                .unwrap()
            else {
                panic!()
            };
            id
        };
        let a = create(&mut editor, "A");
        let b = create(&mut editor, "B");
        let c = create(&mut editor, "C");
        let d = create(&mut editor, "D");
        let parent = ObjectParent::Layer(layer);

        let CommandOutcome::Object(group_id) = editor
            .execute(Command::Group {
                ids: vec![b, c],
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(editor.document().children_of(parent), &[a, group_id, d]);

        let freed = editor.ungroup(&[group_id]).unwrap();
        assert_eq!(freed, vec![b, c]);
        assert_eq!(editor.document().children_of(parent), &[a, b, c, d]);
        assert!(editor.document().object(group_id).is_none());

        editor.undo().unwrap();
        assert_eq!(editor.document().children_of(parent), &[a, group_id, d]);
        assert_eq!(
            editor.document().children_of(ObjectParent::Group(group_id)),
            &[b, c]
        );

        editor.redo().unwrap();
        assert_eq!(editor.document().children_of(parent), &[a, b, c, d]);
    }

    #[test]
    fn ungroup_bakes_the_groups_transform_into_its_children() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(a) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(b) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(20.0, 0.0, 30.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(group_id) = editor
            .execute(Command::Group {
                ids: vec![a, b],
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let xf = Affine::translate((40.0, 15.0)) * Affine::scale(2.0);
        editor
            .execute(Command::SetTransform {
                object: group_id,
                transform: xf,
            })
            .unwrap();
        let world_a = editor.document().world_transform(a);
        let world_b = editor.document().world_transform(b);
        let bounds_a = editor.document().bounds_of(a);
        let bounds_b = editor.document().bounds_of(b);

        editor.ungroup(&[group_id]).unwrap();
        assert_eq!(editor.document().world_transform(a), world_a);
        assert_eq!(editor.document().world_transform(b), world_b);
        assert_eq!(editor.document().bounds_of(a), bounds_a);
        assert_eq!(editor.document().bounds_of(b), bounds_b);
        assert_eq!(
            editor.document().object(a).unwrap().transform,
            world_a
        );

        editor.undo().unwrap();
        assert_eq!(editor.document().world_transform(a), world_a);
        assert_eq!(
            editor.document().object(group_id).unwrap().transform,
            xf
        );
    }

    #[test]
    fn ungroup_errors_on_a_non_group_object() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(object) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(editor.ungroup(&[object]).is_err());
    }

    #[test]
    fn ungroup_errors_cleanly_on_an_empty_list() {
        let mut editor = new_editor();
        assert_eq!(
            editor.ungroup(&[]).unwrap_err(),
            CommandError::NothingToUngroup
        );
    }

    #[test]
    fn new_rect_defaults_to_a_visible_fill_and_stroke() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(object) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let appearance = editor.document().object(object).unwrap().appearance;
        assert!(matches!(appearance.fill, Paint::Solid(_)));
        assert!(matches!(appearance.stroke, Paint::Solid(_)));
        assert_eq!(
            appearance.stroke_width,
            amalith_core::Appearance::DEFAULT_STROKE_WIDTH
        );
    }

    #[test]
    fn apply_gradient_mints_pool_entry_assigns_paint_and_undoes() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(obj) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };

        let CommandOutcome::Gradient(gid) = editor
            .execute(Command::ApplyGradient {
                objects: vec![obj],
                stroke: false,
                source: GradientRef::New(GradientKind::Linear),
            })
            .unwrap()
        else {
            panic!("expected Gradient outcome");
        };
        assert_eq!(editor.document().gradients().len(), 1);
        assert_eq!(
            editor.document().object(obj).unwrap().appearance.fill,
            Paint::Gradient(gid)
        );

        // Edit the pooled definition, then undo it.
        let mut edited = editor.document().gradient(gid).unwrap().clone();
        edited.kind = GradientKind::Radial;
        edited.stops.push(amalith_core::GradientStop::new(
            0.5,
            Color::rgb(1.0, 0.0, 0.0),
        ));
        edited.normalize();
        editor
            .execute(Command::EditGradient {
                id: gid,
                gradient: edited,
            })
            .unwrap();
        assert_eq!(
            editor.document().gradient(gid).unwrap().kind,
            GradientKind::Radial
        );
        assert_eq!(editor.document().gradient(gid).unwrap().stops.len(), 3);

        editor.undo().unwrap(); // undo edit
        assert_eq!(
            editor.document().gradient(gid).unwrap().kind,
            GradientKind::Linear
        );
        assert_eq!(editor.document().gradient(gid).unwrap().stops.len(), 2);

        editor.undo().unwrap(); // undo apply: pool entry + paint both revert
        assert!(editor.document().gradients().is_empty());
        assert!(matches!(
            editor.document().object(obj).unwrap().appearance.fill,
            Paint::Solid(_)
        ));

        editor.redo().unwrap();
        assert_eq!(editor.document().gradients().len(), 1);
        assert_eq!(
            editor.document().object(obj).unwrap().appearance.fill,
            Paint::Gradient(gid)
        );
    }

    #[test]
    fn set_fill_and_stroke_apply_to_every_object_and_undo_redo() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let create = |editor: &mut Editor| {
            let CommandOutcome::Object(id) = editor
                .execute(Command::CreateRect {
                    layer,
                    rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                    name: None,
                })
                .unwrap()
            else {
                panic!()
            };
            id
        };
        let a = create(&mut editor);
        let b = create(&mut editor);
        let original_fill = editor.document().object(a).unwrap().appearance.fill;

        let red = Paint::Solid(Color::rgb(1.0, 0.0, 0.0));
        editor
            .execute(Command::SetFill {
                objects: vec![a, b],
                paint: red,
            })
            .unwrap();
        assert_eq!(editor.document().object(a).unwrap().appearance.fill, red);
        assert_eq!(editor.document().object(b).unwrap().appearance.fill, red);

        editor
            .execute(Command::SetStroke {
                objects: vec![a, b],
                paint: Paint::None,
            })
            .unwrap();
        assert_eq!(
            editor.document().object(a).unwrap().appearance.stroke,
            Paint::None
        );
        assert_eq!(
            editor.document().object(b).unwrap().appearance.stroke,
            Paint::None
        );

        editor.undo().unwrap(); // undo stroke
        assert!(matches!(
            editor.document().object(a).unwrap().appearance.stroke,
            Paint::Solid(_)
        ));
        editor.undo().unwrap(); // undo fill
        assert_eq!(
            editor.document().object(a).unwrap().appearance.fill,
            original_fill
        );

        editor.redo().unwrap(); // redo fill
        editor.redo().unwrap(); // redo stroke
        assert_eq!(editor.document().object(a).unwrap().appearance.fill, red);
        assert_eq!(
            editor.document().object(a).unwrap().appearance.stroke,
            Paint::None
        );
    }

    #[test]
    fn set_stroke_width_and_opacity_apply_to_every_object_and_undo_redo() {
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let create = |editor: &mut Editor| match editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        {
            CommandOutcome::Object(id) => id,
            _ => panic!(),
        };
        let a = create(&mut editor);
        let b = create(&mut editor);
        let original_width = editor.document().object(a).unwrap().appearance.stroke_width;
        let original_opacity = editor.document().object(a).unwrap().appearance.opacity;

        editor
            .execute(Command::SetStrokeWidth {
                objects: vec![a, b],
                width: 3.5,
            })
            .unwrap();
        assert_eq!(
            editor.document().object(a).unwrap().appearance.stroke_width,
            3.5
        );
        assert_eq!(
            editor.document().object(b).unwrap().appearance.stroke_width,
            3.5
        );

        editor
            .execute(Command::SetOpacity {
                objects: vec![a, b],
                opacity: 0.35,
            })
            .unwrap();
        assert_eq!(
            editor.document().object(a).unwrap().appearance.opacity,
            0.35
        );
        assert_eq!(
            editor.document().object(b).unwrap().appearance.opacity,
            0.35
        );

        editor.undo().unwrap();
        assert_eq!(
            editor.document().object(a).unwrap().appearance.opacity,
            original_opacity
        );
        editor.undo().unwrap();
        assert_eq!(
            editor.document().object(a).unwrap().appearance.stroke_width,
            original_width
        );

        editor.redo().unwrap();
        editor.redo().unwrap();
        assert_eq!(
            editor.document().object(a).unwrap().appearance.stroke_width,
            3.5
        );
        assert_eq!(
            editor.document().object(a).unwrap().appearance.opacity,
            0.35
        );
    }

    #[test]
    fn set_stroke_style_applies_to_every_object_and_roundtrips_through_undo() {
        use amalith_core::{LineCap, LineJoin, StrokeAlign, StrokeStyle};
        let mut editor = new_editor();
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let create = |editor: &mut Editor| match editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        {
            CommandOutcome::Object(id) => id,
            _ => panic!(),
        };
        let a = create(&mut editor);
        let b = create(&mut editor);
        let original = editor.document().object(a).unwrap().appearance.stroke_style;

        let style = StrokeStyle {
            cap: LineCap::Round,
            join: LineJoin::Bevel,
            miter_limit: 4.0,
            align: StrokeAlign::Outside,
            dashed: true,
            dash: [6.0, 3.0, 0.0, 0.0, 0.0, 0.0],
            dash_offset: 1.0,
        };
        editor
            .execute(Command::SetStrokeStyle {
                objects: vec![a, b],
                style,
            })
            .unwrap();
        assert_eq!(editor.document().object(a).unwrap().appearance.stroke_style, style);
        assert_eq!(editor.document().object(b).unwrap().appearance.stroke_style, style);

        editor.undo().unwrap();
        assert_eq!(editor.document().object(a).unwrap().appearance.stroke_style, original);
        assert_eq!(editor.document().object(b).unwrap().appearance.stroke_style, original);

        editor.redo().unwrap();
        assert_eq!(editor.document().object(a).unwrap().appearance.stroke_style, style);
    }

    #[test]
    fn set_visible_and_locked_roundtrip_through_undo() {
        let mut editor = Editor::new(Document::new("Flags"));
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(id) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(editor.document().object(id).unwrap().visible);
        assert!(!editor.document().object(id).unwrap().locked);

        editor
            .execute(Command::SetVisible {
                objects: vec![id],
                visible: false,
            })
            .unwrap();
        editor
            .execute(Command::SetLocked {
                objects: vec![id],
                locked: true,
            })
            .unwrap();
        assert!(!editor.document().object(id).unwrap().visible);
        assert!(editor.document().object(id).unwrap().locked);

        editor.undo().unwrap();
        editor.undo().unwrap();
        assert!(editor.document().object(id).unwrap().visible);
        assert!(!editor.document().object(id).unwrap().locked);

        editor.redo().unwrap();
        editor.redo().unwrap();
        assert!(!editor.document().object(id).unwrap().visible);
        assert!(editor.document().object(id).unwrap().locked);
    }
}
