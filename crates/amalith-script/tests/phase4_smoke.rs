//! Phase 4 acceptance: mirrors RAGE script 5 — `createOutline()` on a
//! real text object converts it into path geometry through the ported
//! `outline_text_data`, using a real system font (no mocked glyph data).

use amalith_commands::{Command, CommandOutcome, Editor};
use amalith_core::{Affine, Document, ObjectKind, Paint, TextData};

use amalith_script::run::run_pipeline_with_host;

#[test]
fn create_outline_converts_text_into_filled_path_geometry() {
    let mut editor = Editor::new(Document::new("Test"));
    let layer_id = match editor.execute(Command::CreateLayer { name: "Layer 1".into(), index: None }).unwrap() {
        CommandOutcome::Layer(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };

    let mut data = TextData { content: "AB".into(), ..TextData::default() };
    data.style.family = "Helvetica".into();
    data.style.size = 48.0;

    let text_id = match editor
        .execute(Command::CreateText { parent: amalith_core::ObjectParent::Layer(layer_id), data, transform: Affine::translate((10.0, 10.0)), name: Some("nam".into()) })
        .unwrap()
    {
        CommandOutcome::Object(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };
    editor.execute(Command::SetFill { objects: vec![text_id], paint: Paint::Solid(amalith_core::Color::rgb(0.0, 0.0, 0.0)) }).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.amalith");
    amalith_io::save(editor.document(), &amalith_io::AssetStore::new(), &in_path).unwrap();

    let script_path = dir.path().join("outline.jsx");
    std::fs::write(
        &script_path,
        format!(
            r#"
            var doc = app.open(new File({in_path:?}));
            var layer = doc.layers[0];
            var items = layer.pageItems;
            var outlined = 0;
            for (var i = items.length - 1; i >= 0; i--) {{
                if (items[i].typename === "TextFrame") {{
                    items[i].createOutline();
                    outlined++;
                }}
            }}
            $.global.outlined = outlined;
            "#,
        ),
    )
    .unwrap();

    let host = run_pipeline_with_host(&[script_path]).unwrap();
    let state = host.borrow();
    let open = state.documents.values().next().unwrap();
    let doc = open.editor.document();

    let text_frames = doc.objects().filter(|o| matches!(o.kind, ObjectKind::Text(_))).count();
    assert_eq!(text_frames, 0, "the TextFrame should have been replaced by outlined geometry");

    let paths: Vec<_> = doc
        .objects()
        .filter(|o| matches!(o.kind, ObjectKind::Path(_) | ObjectKind::CompoundPath(_)))
        .collect();
    assert!(!paths.is_empty(), "createOutline() should have produced at least one path object");

    let has_glyph_geometry = paths.iter().any(|o| match &o.kind {
        ObjectKind::Path(pd) => !pd.subpaths().is_empty(),
        ObjectKind::CompoundPath(_) => true,
        _ => false,
    });
    assert!(has_glyph_geometry, "outlined path(s) should contain real glyph geometry, not be empty");
}
