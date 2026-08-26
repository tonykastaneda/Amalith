//! The public command vocabulary. GUI tools, keyboard shortcuts, plugins,
//! scripts, the CLI, and agents all describe mutations as a `Command`
//! value and hand it to [`crate::Editor::execute`] — nothing constructs a
//! `Command` and applies it any other way, so there is exactly one place
//! (`crate::edit`) that turns intent into an actual, undoable document
//! change. This is the Rust translation of Inkscape's `DocumentUndo`
//! discipline: never mutate ad hoc, always go through the logged path.
use amalith_core::{Affine, ArtboardId, LayerId, ObjectId, Rect, Vec2};

/// A single, undoable document mutation.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Creates a new artboard. `index` is the position among existing
    /// artboards (`None` appends at the end).
    CreateArtboard {
        name: String,
        rect: Rect,
        index: Option<usize>,
    },
    DeleteArtboard {
        id: ArtboardId,
    },
    DeleteObject {
        id: ObjectId,
    },
    DeleteObjects {
        ids: Vec<ObjectId>,
    },
    RenameArtboard {
        id: ArtboardId,
        name: String,
    },
    ResizeArtboard {
        id: ArtboardId,
        rect: Rect,
    },
    /// Translates an artboard and every object intersecting its pre-move
    /// bounds as one undoable action.
    MoveArtboard {
        id: ArtboardId,
        delta: Vec2,
    },
    /// Duplicates an artboard and intersecting top-level artwork.
    DuplicateArtboard {
        id: ArtboardId,
        delta: Vec2,
    },
    /// Creates a new, empty layer. `index` is the position among existing
    /// layers (`None` appends at the end / top).
    CreateLayer {
        name: String,
        index: Option<usize>,
    },
    /// Creates a rectangle path object as the top-most child of `layer`.
    CreateRect {
        layer: LayerId,
        rect: Rect,
        name: Option<String>,
    },
    /// Translates an object by `delta`, in the coordinate space of the
    /// object's parent (document space for a layer-level object).
    MoveObject {
        object: ObjectId,
        delta: Vec2,
    },
    MoveObjects {
        objects: Vec<ObjectId>,
        delta: Vec2,
    },
    /// Duplicates one object as a top child of its existing parent and moves
    /// only the copy by `delta`.
    DuplicateObject {
        object: ObjectId,
        delta: Vec2,
    },
    /// Replaces an object's local-to-parent transform outright.
    SetTransform {
        object: ObjectId,
        transform: Affine,
    },
    SetTransforms {
        items: Vec<(ObjectId, Affine)>,
    },
}

/// What a successfully executed command produced, when relevant to the
/// caller (e.g. the newly created entity's ID). Commands that only modify
/// an existing entity (rename, resize, move, set transform, delete) yield
/// `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    None,
    Artboard(ArtboardId),
    Layer(LayerId),
    Object(ObjectId),
}
