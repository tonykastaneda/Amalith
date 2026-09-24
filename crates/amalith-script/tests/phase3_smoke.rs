//! Phase 3 acceptance: mirrors RAGE script 1 (embed every linked image,
//! typename flips live from `PlacedItem` to `RasterItem`) and exercises
//! `Document.saveAs` retargeted to `.svg` as a secondary export target.

use amalith_commands::{Command, CommandOutcome, Editor};
use amalith_core::{Affine, Document, Rect};
use amalith_script::run::run_pipeline_with_host;

#[test]
fn script_embeds_every_linked_placed_item_and_typename_flips_live() {
    let dir = tempfile::tempdir().unwrap();
    let linked_path = dir.path().join("photo.png");
    std::fs::write(&linked_path, b"not a real png, embed() only reads bytes").unwrap();

    let mut editor = Editor::new(Document::new("Test"));
    let layer_id = match editor.execute(Command::CreateLayer { name: "Layer 1".into(), index: None }).unwrap() {
        CommandOutcome::Layer(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };
    editor
        .execute(Command::CreateImage {
            parent: amalith_core::ObjectParent::Layer(layer_id),
            path: linked_path.to_string_lossy().into_owned(),
            bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
            transform: Affine::IDENTITY,
            name: Some("photo".into()),
            embedded: false,
            modified: None,
            size: None,
        })
        .unwrap();

    let in_path = dir.path().join("in.amalith");
    let out_path = dir.path().join("out.amalith");
    amalith_io::save(editor.document(), &amalith_io::AssetStore::new(), &in_path).unwrap();

    let script_path = dir.path().join("embed.jsx");
    std::fs::write(
        &script_path,
        format!(
            r#"
            var doc = app.open(new File({in_path:?}));
            var layer = doc.layers[0];
            var linked = [];
            var placed = layer.placedItems;
            for (var i = 0; i < placed.length; i++) {{
                if (placed[i].embedded === false) {{
                    linked.push(placed[i]);
                }}
            }}
            for (var j = 0; j < linked.length; j++) {{
                linked[j].embed();
            }}
            var stillLinked = 0;
            var items = layer.pageItems;
            for (var k = 0; k < items.length; k++) {{
                if (items[k].typename === "PlacedItem") {{ stillLinked++; }}
                if (items[k].typename === "RasterItem") {{
                    $.global.rasterName = items[k].name;
                }}
            }}
            $.global.stillLinked = stillLinked;
            doc.saveAs(new File({out_path:?}));
            "#,
        ),
    )
    .unwrap();

    let host = run_pipeline_with_host(&[script_path]).unwrap();
    let state = host.borrow();
    let open = state.documents.values().next().unwrap();
    let asset = open.editor.document().assets().first().unwrap();
    assert!(asset.is_embedded(), "the linked asset should have been embedded by the script");
    assert!(
        !open.assets.get(match &asset.source {
            amalith_core::AssetSource::Embedded { container_path } => container_path,
            _ => panic!("expected embedded"),
        })
        .unwrap()
        .is_empty(),
        "embedded bytes should have been copied into the document's AssetStore"
    );

    let (saved, _) = amalith_io::load(&out_path).unwrap();
    assert_eq!(saved.assets().len(), 1);
    assert!(saved.assets()[0].is_embedded());
}

#[test]
fn document_save_as_retargets_to_svg() {
    let dir = tempfile::tempdir().unwrap();
    let mut editor = Editor::new(Document::new("Test"));
    let layer_id = match editor.execute(Command::CreateLayer { name: "Layer 1".into(), index: None }).unwrap() {
        CommandOutcome::Layer(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };
    editor
        .execute(Command::CreateRect {
            parent: amalith_core::ObjectParent::Layer(layer_id),
            rect: Rect::new(0.0, 0.0, 50.0, 50.0),
            name: Some("square".into()),
        })
        .unwrap();

    let in_path = dir.path().join("in.amalith");
    let svg_path = dir.path().join("out.svg");
    amalith_io::save(editor.document(), &amalith_io::AssetStore::new(), &in_path).unwrap();

    let script_path = dir.path().join("export.jsx");
    std::fs::write(
        &script_path,
        format!(
            r#"
            var doc = app.open(new File({in_path:?}));
            doc.saveAs(new File({svg_path:?}));
            "#,
        ),
    )
    .unwrap();

    run_pipeline_with_host(&[script_path]).unwrap();

    let svg = std::fs::read_to_string(&svg_path).unwrap();
    assert!(svg.contains("<svg"), "expected a well-formed SVG export, got: {svg}");
}
