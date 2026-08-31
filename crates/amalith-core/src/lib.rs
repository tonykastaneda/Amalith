//! `amalith-core`: the Amalith document model.
//!
//! This crate defines what a Amalith document *is* — its structure, IDs,
//! coordinate system, units, transforms, bounds, and ownership tree — with
//! no dependency on any UI, renderer, or command/undo system. It is
//! intentionally mutable-by-value: nothing here enforces undo/redo or
//! change notification, because that is `amalith-commands`'s job, built on
//! top of the primitives exported here. See `document.rs` for the
//! coordinate-system and ownership-tree writeup, and `DESIGN.md` for why
//! this crate does not model an XML/SVG repr tree.
pub mod appearance;
pub mod artboard;
pub mod asset;
pub mod document;
pub mod error;
pub mod geom;
pub mod ids;
pub mod layer;
pub mod metadata;
pub mod object;
pub mod swatch;
pub mod units;

pub use appearance::{Appearance, LineCap, LineJoin, Paint, StrokeAlign, StrokeStyle};
pub use artboard::Artboard;
pub use asset::{Asset, AssetKind, AssetSource};
pub use document::Document;
pub use error::DocumentError;
pub use geom::{Affine, Bounds, Point, Rect, Size, Vec2};
pub use ids::{ArtboardId, AssetId, LayerId, ObjectId};
pub use layer::Layer;
pub use metadata::{Bleed, ColorMode, Metadata, PreviewMode, RasterEffects, Settings};
pub use object::{
    Anchor, CompoundPathData, GroupData, HandleMode, ImageData, Object, ObjectKind, ObjectParent,
    PathData, Subpath, SymbolData, TextAlign, TextData, TextKind, TextPosition, TextStyle,
};
pub use swatch::{Color, Swatch};
pub use units::{Length, Unit};
