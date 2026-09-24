//! Proves the format's forward-compatibility guarantee end-to-end: a
//! `.amalith` file containing an `ObjectKind` this build has never heard
//! of (simulating one a *newer* Amalith wrote) still opens, and a re-save
//! doesn't destroy the part it didn't understand. See
//! `amalith_io::manifest`'s module doc for the policy this proves.
use amalith_commands::{Command, CommandOutcome, Editor};
use amalith_core::{Document, ObjectKind, Rect};
use amalith_io::{load, save, AssetStore};
use std::io::{Read, Write};
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// Rewrites `path`'s zip container, replacing `target_entry`'s bytes with
/// `new_bytes` and leaving every other entry byte-for-byte unchanged.
fn replace_zip_entry(path: &std::path::Path, target_entry: &str, new_bytes: &[u8]) {
    let mut archive = ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let names: Vec<String> = archive.file_names().map(str::to_string).collect();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for name in names {
        let mut buf = Vec::new();
        archive.by_name(&name).unwrap().read_to_end(&mut buf).unwrap();
        entries.push((name, buf));
    }

    let mut zip = ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = SimpleFileOptions::default();
    for (name, bytes) in entries {
        zip.start_file(&name, options).unwrap();
        if name == target_entry {
            zip.write_all(new_bytes).unwrap();
        } else {
            zip.write_all(&bytes).unwrap();
        }
    }
    zip.finish().unwrap();
}

#[test]
fn a_future_object_kind_still_opens_and_round_trips_losslessly() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("future.amalith");

    // A minimal real document: one layer, one rectangle.
    let mut editor = Editor::new(Document::new("Forward compat"));
    let CommandOutcome::Layer(layer_id) = editor
        .execute(Command::CreateLayer { name: "Layer 1".into(), index: None })
        .unwrap()
    else {
        panic!("expected Layer outcome");
    };
    let CommandOutcome::Object(object_id) = editor
        .execute(Command::CreateRect {
            parent: amalith_core::ObjectParent::Layer(layer_id),
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            name: Some("Rectangle 1".into()),
        })
        .unwrap()
    else {
        panic!("expected Object outcome");
    };
    save(editor.document(), &AssetStore::new(), &path).unwrap();

    // Overwrite the rectangle's `kind` with a tag this build has never
    // heard of — standing in for a real future variant (e.g. `Mesh`)
    // written by a newer Amalith.
    let artwork_entry = format!("artwork/layer-{layer_id}.json");
    let mut archive = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut raw = String::new();
    archive.by_name(&artwork_entry).unwrap().read_to_string(&mut raw).unwrap();
    drop(archive);
    let mut artwork: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let objects = artwork["objects"].as_array_mut().unwrap();
    assert_eq!(objects.len(), 1, "expected exactly the one rectangle object");
    let future_kind = serde_json::json!({"stops": [0.0, 1.0], "note": "from the future"});
    objects[0]["kind"] = serde_json::json!({ "Mesh": future_kind });
    let mutated = serde_json::to_vec(&artwork).unwrap();
    replace_zip_entry(&path, &artwork_entry, &mutated);

    // The file still opens — no hard failure on the unrecognized tag.
    let (document, assets) = load(&path).expect("a future object kind must not fail the whole load");
    let object = document.object(object_id).expect("the object itself must still exist");
    match &object.kind {
        ObjectKind::Unknown { kind, raw } => {
            assert_eq!(kind, "Mesh");
            assert_eq!(raw, &future_kind);
        }
        other => panic!("expected ObjectKind::Unknown, got {other:?}"),
    }

    // Re-saving must not destroy the part this build doesn't understand —
    // it round-trips back to the *original* wire shape (`{"Mesh": ...}`),
    // not the derive's default `{"Unknown": {"kind": ..., "raw": ...}}`.
    let resaved = dir.path().join("future_resaved.amalith");
    save(&document, &assets, &resaved).unwrap();
    let mut archive2 = ZipArchive::new(std::fs::File::open(&resaved).unwrap()).unwrap();
    let mut raw2 = String::new();
    archive2.by_name(&artwork_entry).unwrap().read_to_string(&mut raw2).unwrap();
    let artwork2: serde_json::Value = serde_json::from_str(&raw2).unwrap();
    let objects2 = artwork2["objects"].as_array().unwrap();
    assert_eq!(objects2[0]["kind"], serde_json::json!({ "Mesh": future_kind }));
}
