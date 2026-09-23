use amalith_commands::{Command, Editor};
use amalith_core::*;
#[test]
fn expand_preserves_placement_and_is_one_undo_step() {
    let mut doc = Document::new("Trace");
    let layer = LayerId::new();
    doc.insert_layer(Layer::new(layer, "Layer"), 0);
    let id = ObjectId::new();
    let mut image = Object::new(
        id,
        ObjectParent::Layer(layer),
        ObjectKind::Image(ImageData {
            asset: AssetId::new(),
            local_bounds: Rect::new(10., 20., 210., 120.),
            mask: None,
        }),
    );
    image.transform = Affine::translate((300., 400.)) * Affine::rotate(0.3);
    image.appearance.opacity = 0.6;
    doc.insert_object(image.clone(), 0).unwrap();
    let sibling = ObjectId::new();
    doc.insert_object(
        Object::rectangle(
            sibling,
            ObjectParent::Layer(layer),
            Rect::new(0., 0., 5., 5.),
        ),
        1,
    )
    .unwrap();
    let mut editor = Editor::new(doc);
    let mut path = kurbo::BezPath::new();
    path.move_to((0., 0.));
    path.line_to((100., 0.));
    path.line_to((100., 50.));
    path.line_to((0., 50.));
    path.close_path();
    editor
        .execute(Command::ExpandImageTrace {
            id,
            width: 100,
            height: 50,
            paths: vec![(PathData::from_bezpath(path), Color::rgb(1., 0., 0.))],
        })
        .unwrap();
    let group = editor.document().object(id).unwrap();
    assert_eq!(group.transform, image.transform);
    assert_eq!(group.appearance, image.appearance);
    assert_eq!(editor.document().layers()[0].children, vec![id, sibling]);
    let ObjectKind::Group(g) = &group.kind else {
        panic!("expected group")
    };
    assert_eq!(g.children.len(), 1);
    let child = g.children[0];
    assert_eq!(
        editor.document().object(child).unwrap().parent,
        ObjectParent::Group(id)
    );
    editor.undo().unwrap();
    assert_eq!(editor.document().object(id), Some(&image));
    assert!(editor.document().object(child).is_none());
    assert!(!editor.can_undo());
    editor.redo().unwrap();
    assert!(matches!(
        editor.document().object(id).unwrap().kind,
        ObjectKind::Group(_)
    ));
}

#[test]
fn invalid_trace_does_not_remove_the_source() {
    let mut doc = Document::new("Trace");
    let layer = LayerId::new();
    doc.insert_layer(Layer::new(layer, "Layer"), 0);
    let id = ObjectId::new();
    let image = Object::new(
        id,
        ObjectParent::Layer(layer),
        ObjectKind::Image(ImageData {
            asset: AssetId::new(),
            local_bounds: Rect::new(0., 0., 10., 10.),
            mask: None,
        }),
    );
    doc.insert_object(image.clone(), 0).unwrap();
    let mut editor = Editor::new(doc);
    assert!(editor
        .execute(Command::ExpandImageTrace {
            id,
            width: 10,
            height: 10,
            paths: Vec::new()
        })
        .is_err());
    assert_eq!(editor.document().object(id), Some(&image));
    assert!(!editor.can_undo());
}
