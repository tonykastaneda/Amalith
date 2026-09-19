//! Save/load of the `.amalith` zip container. See `manifest.rs` for the
//! on-disk schema and why it's split from `document.json` into per-layer
//! `artwork/*.json` files.
use crate::assets::AssetStore;
use crate::error::IoError;
use crate::manifest::{
    artwork_container_path, symbol_container_path, ArtworkFile, DocumentManifest, LayerManifest,
    SymbolArtworkFile, SymbolManifest, FORMAT_VERSION,
};
use amalith_core::{AssetSource, Document, Layer, Object, ObjectKind, SymbolDefinition};
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
                color: layer.color,
                template: layer.template,
                print: layer.print,
                preview: layer.preview,
                dim_images_to: layer.dim_images_to,
                kind: layer.kind,
            })
            .collect(),
        symbols: document
            .symbols()
            .iter()
            .map(|s| SymbolManifest { id: s.id, name: s.name.clone() })
            .collect(),
    };
    zip.start_file("document.json", options)?;
    serde_json::to_writer_pretty(&mut zip, &manifest)?;

    for layer in document.layers() {
        let artwork = ArtworkFile {
            layer_id: layer.id,
            objects: gather_children_objects(document, &layer.children),
        };
        zip.start_file(artwork_container_path(layer.id), options)?;
        serde_json::to_writer_pretty(&mut zip, &artwork)?;
    }

    for symbol in document.symbols() {
        let artwork = SymbolArtworkFile {
            symbol_id: symbol.id,
            objects: gather_children_objects(document, &symbol.children),
        };
        zip.start_file(symbol_container_path(symbol.id), options)?;
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
            color: layer_manifest.color,
            template: layer_manifest.template,
            print: layer_manifest.print,
            preview: layer_manifest.preview,
            dim_images_to: layer_manifest.dim_images_to,
            kind: layer_manifest.kind,
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

    // Symbol definitions are added to the pool *before* their content is
    // replayed — `insert_object` validates `ObjectParent::Symbol(id)`
    // against `Document::symbol`, same as a layer needing to exist first.
    for symbol_manifest in manifest.symbols {
        document.add_symbol(SymbolDefinition {
            id: symbol_manifest.id,
            name: symbol_manifest.name,
            children: Vec::new(),
        });

        let artwork: SymbolArtworkFile = {
            let entry = zip.by_name(&symbol_container_path(symbol_manifest.id))?;
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

/// Flattens a container's (layer or symbol definition's) top-level object
/// tree into DFS pre-order (parent before every descendant), matching the
/// order `load` replays with `insert_object`.
fn gather_children_objects(document: &Document, children: &[amalith_core::ObjectId]) -> Vec<Object> {
    let mut out = Vec::new();
    for &id in children {
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
