//! Save/load of the `.amalith` zip container. See `manifest.rs` for the
//! on-disk schema and why it's split from `document.json` into per-layer
//! `artwork/*.json` files.
use crate::assets::AssetStore;
use crate::error::IoError;
use crate::manifest::{
    artwork_container_path, ArtworkFile, DocumentManifest, LayerManifest, FORMAT_VERSION,
};
use amalith_core::{AssetSource, Document, Layer, Object, ObjectKind};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// Writes `document` (plus any embedded asset bytes in `assets`) to a
/// `.amalith` zip container at `path`, overwriting any existing file.
pub fn save(
    document: &Document,
    assets: &AssetStore,
    path: impl AsRef<Path>,
) -> Result<(), IoError> {
    let file = File::create(path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let manifest = DocumentManifest {
        format_version: FORMAT_VERSION,
        metadata: document.metadata.clone(),
        settings: document.settings,
        artboards: document.artboards().to_vec(),
        swatches: document.swatches().to_vec(),
        gradients: document.gradients().to_vec(),
        guides: document.guides().to_vec(),
        assets: document.assets().to_vec(),
        layers: document
            .layers()
            .iter()
            .map(|layer| LayerManifest {
                id: layer.id,
                name: layer.name.clone(),
                visible: layer.visible,
                locked: layer.locked,
            })
            .collect(),
    };
    zip.start_file("document.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &manifest)?;

    for layer in document.layers() {
        let artwork = ArtworkFile {
            layer_id: layer.id,
            objects: gather_layer_objects(document, layer),
        };
        zip.start_file(artwork_container_path(layer.id), options)?;
        serde_json::to_writer_pretty(&mut zip, &artwork)?;
    }

    for (container_path, bytes) in assets.iter() {
        zip.start_file(container_path, options)?;
        zip.write_all(bytes)?;
    }

    zip.finish()?;
    Ok(())
}

/// Reads a `.amalith` zip container from `path`, returning the document and
/// the bytes of any embedded assets it referenced.
pub fn load(path: impl AsRef<Path>) -> Result<(Document, AssetStore), IoError> {
    let file = File::open(path)?;
    let mut zip = ZipArchive::new(file)?;

    let manifest: DocumentManifest = {
        let entry = zip.by_name("document.json")?;
        serde_json::from_reader(entry)?
    };

    let title = manifest
        .metadata
        .title
        .clone()
        .unwrap_or_else(|| "Untitled".to_string());
    let mut document = Document::new(title);
    document.metadata = manifest.metadata;
    document.settings = manifest.settings;

    for artboard in manifest.artboards {
        let index = document.artboards().len();
        document.insert_artboard(artboard, index);
    }
    for swatch in manifest.swatches {
        document.add_swatch(swatch);
    }
    for gradient in manifest.gradients {
        document.add_gradient(gradient);
    }
    for guide in manifest.guides {
        let index = document.guides().len();
        document.insert_guide(guide, index);
    }
    for asset in manifest.assets {
        document.add_asset(asset);
    }

    for layer_manifest in manifest.layers {
        let layer = Layer {
            id: layer_manifest.id,
            name: layer_manifest.name,
            visible: layer_manifest.visible,
            locked: layer_manifest.locked,
            children: Vec::new(),
        };
        let index = document.layers().len();
        document.insert_layer(layer, index);

        let artwork: ArtworkFile = {
            let entry = zip.by_name(&artwork_container_path(layer_manifest.id))?;
            serde_json::from_reader(entry)?
        };
        for object in artwork.objects {
            let index = document.children_of(object.parent).len();
            document.insert_object(object, index)?;
        }
    }

    let mut assets = AssetStore::new();
    for asset in document.assets() {
        if let AssetSource::Embedded { container_path } = &asset.source {
            let mut entry = zip.by_name(container_path)?;
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            assets.insert(container_path.clone(), bytes);
        }
    }

    Ok((document, assets))
}

/// Flattens a layer's object tree into DFS pre-order (parent before every
/// descendant), matching the order `load` replays with `insert_object`.
fn gather_layer_objects(document: &Document, layer: &Layer) -> Vec<Object> {
    let mut out = Vec::new();
    for &id in &layer.children {
        gather_recursive(document, id, &mut out);
    }
    out
}

fn gather_recursive(document: &Document, id: amalith_core::ObjectId, out: &mut Vec<Object>) {
    let object = document
        .object(id)
        .expect("object reachable from a layer's children must exist in the arena");
    let mut serialized = object.clone();
    let child_ids = match &mut serialized.kind {
        // `insert_object` rebuilds a group's `children` list as each child
        // is replayed below, so the list serialized here would just be
        // re-appended onto the same (already-correct) list on load,
        // doubling every entry. Serialize an empty list; the original is
        // still used to drive the recursion via `child_ids` below.
        ObjectKind::Group(group) => {
            let child_ids = std::mem::take(&mut group.children);
            Some(child_ids)
        }
        _ => None,
    };
    out.push(serialized);
    if let Some(child_ids) = child_ids {
        for child_id in child_ids {
            gather_recursive(document, child_id, out);
        }
    }
}
