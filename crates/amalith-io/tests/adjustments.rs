//! Adjustment layers survive a save/load round trip, and an adjustment op
//! this build doesn't know (one a newer Amalith wrote) degrades to
//! `ObjectKind::Unknown` without losing data.
use amalith_commands::{Command, CommandOutcome, Editor, LayerOptions};
use amalith_core::{
    AdjustmentData, AdjustmentOp, Affine, Asset, AssetId, AssetKind, CurvePoint, CurvesParams, Document, LayerId,
    LayerKind, ObjectKind, ObjectParent, Rect,
};
use amalith_io::{load, save, AssetStore};
use std::io::{Read, Write};
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

fn raster_layer(editor: &mut Editor) -> LayerId {
    let CommandOutcome::Layer(id) = editor.execute(Command::CreateLayer { name: "Photo".into(), index: None }).unwrap()
    else {
        panic!()
    };
    let l = editor.document().layer(id).unwrap();
    let options = LayerOptions {
        name: l.name.clone(),
        color: l.color,
        visible: l.visible,
        locked: l.locked,
        template: l.template,
        print: l.print,
        preview: l.preview,
        dim_images_to: l.dim_images_to,
        kind: LayerKind::Raster,
    };
    editor.execute(Command::SetLayerOptions { id, options }).unwrap();
    id
}

fn read_entry(path: &std::path::Path, entry: &str) -> String {
    let mut archive = ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut raw = String::new();
    archive.by_name(entry).unwrap().read_to_string(&mut raw).unwrap();
    raw
}

fn replace_entry(path: &std::path::Path, target: &str, bytes: &[u8]) {
    let mut archive = ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let names: Vec<String> = archive.file_names().map(str::to_string).collect();
    let mut entries = Vec::new();
    for name in names {
        let mut buf = Vec::new();
        archive.by_name(&name).unwrap().read_to_end(&mut buf).unwrap();
        entries.push((name, buf));
    }
    let mut zip = ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, buf) in entries {
        zip.start_file(&name, SimpleFileOptions::default()).unwrap();
        zip.write_all(if name == target { bytes } else { &buf }).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn a_masked_adjustment_round_trips_with_its_mask_bytes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("adjusted.amalith");
    let mut editor = Editor::new(Document::new("Adjusted"));
    let layer = raster_layer(&mut editor);
    let CommandOutcome::Object(image) = editor
        .execute(Command::CreateImage {
            parent: amalith_core::ObjectParent::Layer(layer), index: None,
            path: "images/photo.png".into(),
            bounds: Rect::new(0., 0., 64., 48.),
            transform: Affine::IDENTITY,
            name: None,
            embedded: true,
            modified: None,
            size: None,
        })
        .unwrap()
    else {
        panic!()
    };

    let mut curves = CurvesParams::default();
    curves.channels[0] = vec![
        CurvePoint { x: 0.0, y: 10.0 },
        CurvePoint { x: 128.0, y: 150.0 },
        CurvePoint { x: 255.0, y: 245.0 },
    ];
    let CommandOutcome::Object(adj) = editor
        .execute(Command::CreateAdjustment {
            layer,
            index: None,
            name: Some("Contrast".into()),
            data: AdjustmentData::new(AdjustmentOp::Curves(curves)),
        })
        .unwrap()
    else {
        panic!()
    };
    let mask_asset = AssetId::new();
    editor
        .execute(Command::AddLayerMask {
            object: adj,
            asset: Asset::embedded(mask_asset, "Mask", AssetKind::Image, "images/adj-mask.png"),
        })
        .unwrap();

    let mut assets = AssetStore::new();
    assets.insert("images/photo.png", vec![0x89, b'P', b'N', b'G', 1]);
    assets.insert("images/adj-mask.png", vec![0x89, b'P', b'N', b'G', 2]);
    save(editor.document(), &assets, &path).unwrap();

    let (loaded, loaded_assets) = load(&path).unwrap();
    assert_eq!(loaded.children_of(ObjectParent::Layer(layer)), &[image, adj], "stacking order survives");
    let before = editor.document().object(adj).unwrap();
    let after = loaded.object(adj).unwrap();
    assert_eq!(after, before, "the whole adjustment object round-trips");
    let ObjectKind::Adjustment(data) = &after.kind else { panic!("loaded as {:?}", after.kind) };
    let mask = data.mask.expect("mask kept");
    let Some(amalith_core::AssetSource::Embedded { container_path: mask_path }) = loaded.asset(mask.asset).map(|a| a.source.clone())
    else {
        panic!("mask asset should be embedded")
    };
    assert_eq!(loaded_assets.get(&mask_path), Some(&[0x89, b'P', b'N', b'G', 2][..]));
    assert_eq!(loaded.layer(layer).unwrap().kind, LayerKind::Raster);
}

#[test]
fn an_unknown_future_adjustment_op_opens_as_unknown_and_resaves_losslessly() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("future-op.amalith");
    let mut editor = Editor::new(Document::new("Future op"));
    let layer = raster_layer(&mut editor);
    editor
        .execute(Command::CreateAdjustment {
            layer,
            index: None,
            name: None,
            data: AdjustmentData::new(AdjustmentOp::Invert),
        })
        .unwrap();
    save(editor.document(), &AssetStore::new(), &path).unwrap();

    // Swap the op for one this build has never heard of.
    let entry = format!("artwork/layer-{layer}.json");
    let mut artwork: serde_json::Value = serde_json::from_str(&read_entry(&path, &entry)).unwrap();
    let future = serde_json::json!({ "Adjustment": { "op": { "Posterize": { "levels": 4 } }, "blend_mode": "Normal" } });
    artwork["objects"][0]["kind"] = future.clone();
    replace_entry(&path, &entry, &serde_json::to_vec(&artwork).unwrap());

    let (loaded, assets) = load(&path).unwrap();
    let id = loaded.children_of(ObjectParent::Layer(layer))[0];
    assert!(
        matches!(&loaded.object(id).unwrap().kind, ObjectKind::Unknown { kind, .. } if kind == "Adjustment"),
        "an op this build doesn't know keeps the whole adjustment as Unknown"
    );

    let resaved = dir.path().join("resaved.amalith");
    save(&loaded, &assets, &resaved).unwrap();
    let artwork: serde_json::Value = serde_json::from_str(&read_entry(&resaved, &entry)).unwrap();
    assert_eq!(artwork["objects"][0]["kind"], future, "re-saving doesn't lose the future op");
}
