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
mod command;
mod edit;
mod editor;
mod error;
mod history;

pub use command::{Command, CommandOutcome};
pub use editor::Editor;
pub use error::CommandError;

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::{Affine, Document, ObjectParent, Rect, Vec2};

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
}
