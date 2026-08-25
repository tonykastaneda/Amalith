//! End-to-end test wiring all three crates together, mirroring the
//! acceptance flow from the top-level task: create document -> add
//! artboard -> add a rectangle object -> move it via a command -> undo ->
//! save -> load -> identical state.
use tempfile::tempdir;
use amalith_commands::{Command, CommandOutcome, Editor};
use amalith_core::{Document, Rect, Vec2};
use amalith_io::{load, save, AssetStore};

#[test]
fn create_move_undo_save_load_roundtrip() {
    let mut editor = Editor::new(Document::new("Milestone 0.1"));

    let CommandOutcome::Artboard(artboard_id) = editor
        .execute(Command::CreateArtboard {
            name: "Artboard 1".into(),
            rect: Rect::new(0.0, 0.0, 1920.0, 1080.0),
            index: None,
        })
        .unwrap()
    else {
        panic!("expected Artboard outcome");
    };

    let CommandOutcome::Layer(layer_id) = editor
        .execute(Command::CreateLayer {
            name: "Layer 1".into(),
            index: None,
        })
        .unwrap()
    else {
        panic!("expected Layer outcome");
    };

    let original_rect = Rect::new(200.0, 200.0, 400.0, 350.0);
    let CommandOutcome::Object(object_id) = editor
        .execute(Command::CreateRect {
            layer: layer_id,
            rect: original_rect,
            name: Some("Rectangle 1".into()),
        })
        .unwrap()
    else {
        panic!("expected Object outcome");
    };

    editor
        .execute(Command::MoveObject {
            object: object_id,
            delta: Vec2::new(150.0, -50.0),
        })
        .unwrap();
    let moved_rect = Rect::new(350.0, 150.0, 550.0, 300.0);
    assert_eq!(editor.document().bounds_of(object_id), Some(moved_rect));

    // "Cmd/Ctrl + Z: rectangle moves back."
    editor.undo().unwrap();
    assert_eq!(editor.document().bounds_of(object_id), Some(original_rect));

    // "Save. Quit Amalith. Open file. Everything is exactly where it was."
    let dir = tempdir().unwrap();
    let path = dir.path().join("milestone-0-1.amalith");
    let document_before_save = editor.into_document();
    save(&document_before_save, &AssetStore::new(), &path).unwrap();
    let (loaded, _assets) = load(&path).unwrap();

    assert_eq!(
        loaded, document_before_save,
        "loaded document must equal what was saved"
    );
    assert_eq!(
        loaded.artboard(artboard_id).unwrap().rect,
        Rect::new(0.0, 0.0, 1920.0, 1080.0)
    );
    assert_eq!(loaded.bounds_of(object_id), Some(original_rect));
    assert_eq!(
        loaded.object(object_id).unwrap().name.as_deref(),
        Some("Rectangle 1")
    );
    assert_eq!(
        loaded.children_of(amalith_core::ObjectParent::Layer(layer_id)),
        &[object_id]
    );
}
