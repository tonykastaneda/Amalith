//! Phase 2 acceptance: a script reads a real `Editor`-backed document,
//! removes one object and locks another through the real `Command`
//! pipeline, saves it back out to `.amalith`, and — separately — the same
//! kind of script-driven change is proven reversible via `Editor::undo()`.

use amalith_commands::{Command, CommandOutcome, Editor};
use amalith_core::{Document, Rect};
use amalith_script::run::run_pipeline_with_host;

fn build_fixture_document() -> Editor {
    let mut editor = Editor::new(Document::new("Test"));
    let layer_id = match editor
        .execute(Command::CreateLayer { name: "Layer 1".into(), index: None })
        .unwrap()
    {
        CommandOutcome::Layer(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };
    editor
        .execute(Command::CreateRect {
            layer: layer_id,
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            name: Some("keep".into()),
        })
        .unwrap();
    editor
        .execute(Command::CreateRect {
            layer: layer_id,
            rect: Rect::new(20.0, 20.0, 30.0, 30.0),
            name: Some("doomed".into()),
        })
        .unwrap();
    editor
}

#[test]
fn script_removes_and_locks_page_items_through_real_editor() {
    let editor = build_fixture_document();
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.amalith");
    let out_path = dir.path().join("out.amalith");
    amalith_io::save(editor.document(), &amalith_io::AssetStore::new(), &in_path).unwrap();

    let script_path = dir.path().join("script.jsx");
    std::fs::write(
        &script_path,
        format!(
            r#"
            var doc = app.open(new File({in_path:?}));
            var layer = doc.layers[0];
            var items = layer.pageItems;
            for (var i = items.length - 1; i >= 0; i--) {{
                if (items[i].name === "doomed") {{
                    items[i].remove();
                }} else {{
                    items[i].locked = true;
                }}
            }}
            doc.saveAs(new File({out_path:?}));
            doc.close();
            "#,
        ),
    )
    .unwrap();

    let host = run_pipeline_with_host(&[script_path]).unwrap();
    // The script closed its document, so HostState should be empty again —
    // proving `Document.close()` really reached HostState, not just JS.
    assert!(host.borrow().documents.is_empty());

    let (saved, _assets) = amalith_io::load(&out_path).unwrap();
    let names: Vec<_> = saved.objects().filter_map(|o| o.name.clone()).collect();
    assert_eq!(names, vec!["keep".to_string()], "the 'doomed' object should have been removed");
    let kept = saved.objects().find(|o| o.name.as_deref() == Some("keep")).unwrap();
    assert!(kept.locked, "the surviving object should have been locked by the script");
}

#[test]
fn same_kind_of_change_is_undoable_through_the_real_editor() {
    let mut editor = build_fixture_document();
    let doomed_id = editor
        .document()
        .objects()
        .find(|o| o.name.as_deref() == Some("doomed"))
        .unwrap()
        .id;

    editor.execute(Command::DeleteObject { id: doomed_id }).unwrap();
    assert_eq!(editor.document().objects().count(), 1);

    editor.undo().unwrap();
    assert_eq!(editor.document().objects().count(), 2);
    assert!(editor.document().object(doomed_id).is_some());

    editor.redo().unwrap();
    assert_eq!(editor.document().objects().count(), 1);
}
