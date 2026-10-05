use amalith_commands::{Command, CommandOutcome, Editor};
use amalith_core::{Appearance, Document, Layer, LayerId, ObjectParent, Paint, PathData, Point};

#[test]
fn styled_path_creation_and_appearance_undo_together() {
    let mut doc = Document::new("Pencil");
    let layer = LayerId::new();
    doc.insert_layer(Layer::new(layer, "Layer"), 0);
    let mut editor = Editor::new(doc);
    let mut appearance = Appearance::default();
    appearance.set_fill(Paint::None);
    appearance.set_stroke_width(3.0);
    let path = PathData::polyline(&[Point::new(0.0, 0.0), Point::new(30.0, 20.0)]);
    let CommandOutcome::Object(id) = editor
        .execute(Command::CreateStyledPath {
            parent: ObjectParent::Layer(layer),
            path,
            name: None,
            appearance: appearance.clone(),
        })
        .unwrap()
    else {
        panic!("expected a path")
    };
    assert_eq!(editor.document().object(id).unwrap().appearance, appearance);
    editor.undo().unwrap();
    assert!(editor.document().object(id).is_none());
    assert!(!editor.can_undo());
    editor.redo().unwrap();
    assert_eq!(editor.document().object(id).unwrap().appearance, appearance);
}
