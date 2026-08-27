//! The on-disk `.amalith` schema types.
//!
//! These mirror `amalith-core`'s types but are declared separately rather
//! than serializing `Document` as one opaque blob. Two reasons:
//!
//! 1. **Splitting content from structure.** `document.json` holds
//!    everything *except* per-layer drawing content (artboards, settings,
//!    swatches, asset metadata, layer list); each layer's object tree gets
//!    its own `artwork/layer-<id>.json`. A tool that only needs to know an
//!    artboard's size, or a diff view that only needs the layer panel,
//!    never has to parse a large document's entire object arena. This
//!    mirrors why the brief's format sketch splits `artwork/` out from
//!    `document.json` at all.
//! 2. **Decoupling the file format from in-memory representation.** If
//!    `Document`'s internal fields change shape, the on-disk schema
//!    doesn't silently change with it — `DocumentManifest` is the explicit,
//!    versioned contract external tools/plugins read.
use amalith_core::{Artboard, Asset, LayerId, Metadata, Object, Settings, Swatch};
use serde::{Deserialize, Serialize};

/// Current `.amalith` container schema version. Bump when `DocumentManifest`
/// or `ArtworkFile` change shape in a way older readers can't tolerate.
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DocumentManifest {
    pub format_version: u32,
    pub metadata: Metadata,
    pub settings: Settings,
    pub artboards: Vec<Artboard>,
    pub swatches: Vec<Swatch>,
    pub assets: Vec<Asset>,
    pub layers: Vec<LayerManifest>,
}

/// Layer identity/panel-state only; `Layer::children` lives in
/// `ArtworkFile` instead (it's derived from the object tree on load).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LayerManifest {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
}

/// One layer's object tree, flattened in DFS pre-order (each object
/// appears after its parent, so replaying the list with
/// `Document::insert_object` never references a not-yet-inserted group).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArtworkFile {
    pub layer_id: LayerId,
    pub objects: Vec<Object>,
}

pub(crate) fn artwork_container_path(layer_id: LayerId) -> String {
    format!("artwork/layer-{layer_id}.json")
}
