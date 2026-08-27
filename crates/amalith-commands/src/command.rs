//! The public command vocabulary. GUI tools, keyboard shortcuts, plugins,
//! scripts, the CLI, and agents all describe mutations as a `Command`
//! value and hand it to [`crate::Editor::execute`] — nothing constructs a
//! `Command` and applies it any other way, so there is exactly one place
//! (`crate::edit`) that turns intent into an actual, undoable document
//! change. This is the Rust translation of Inkscape's `DocumentUndo`
//! discipline: never mutate ad hoc, always go through the logged path.
use amalith_core::{Affine, ArtboardId, LayerId, ObjectId, Paint, Rect, Vec2};

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
    RenameLayer {
        id: LayerId,
        name: String,
    },
    /// Renames an object (including a group) — the same field an object
    /// is created with, so `None` clears it back to the panel's fallback
    /// display name rather than an empty string.
    RenameObject {
        id: ObjectId,
        name: Option<String>,
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
    /// Creates a closed ellipse path as the top-most child of `layer`.
    CreateEllipse {
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
    /// Translates whole path anchors (and their cubic handles) by one shared
    /// local-space delta. Multiple anchors on one or several paths form one
    /// undoable gesture.
    MoveAnchors {
        anchors: Vec<(ObjectId, usize)>,
        delta: Vec2,
    },
    /// Duplicates one object as a top child of its existing parent and moves
    /// only the copy by `delta`.
    DuplicateObject {
        object: ObjectId,
        delta: Vec2,
    },
    /// Duplicates several objects at once (deep-copying any group's full
    /// descendant tree, with fresh ids throughout), one undo group. Unlike
    /// [`Command::Paste`], each duplicate lands as the top child of *its
    /// own* existing parent — not funneled into one shared target — and
    /// nothing here touches [`crate::Editor`]'s clipboard. Relative order
    /// among objects sharing a parent is preserved.
    DuplicateObjects {
        objects: Vec<ObjectId>,
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
    /// Nudges selected siblings in paint order. Positive steps move toward
    /// the front; negative steps move toward the back.
    NudgeStack {
        ids: Vec<ObjectId>,
        steps: i32,
    },
    /// Inserts a deep copy of [`crate::Editor`]'s clipboard (see
    /// [`crate::Editor::copy`]) as one undo group. `delta` translates each
    /// pasted root in its target parent's coordinate space; group
    /// descendants keep their copied relative transforms untouched.
    /// `stack` picks where each clone lands in paint order. Errors if the
    /// clipboard is empty.
    Paste {
        delta: Vec2,
        stack: PasteStack,
    },
    /// Groups `ids` into one new group object, as one undo group. `ids`
    /// must all share the same current parent (a layer, or another
    /// group) — that parent becomes the new group's parent too. The
    /// group lands at the position of the topmost (frontmost) grouped
    /// object, so grouping never changes stacking relative to untouched
    /// siblings; the grouped objects themselves keep their relative order
    /// (bottom to top) as the group's children. Each object's own
    /// transform is untouched, and the new group's own transform is
    /// identity, so grouping never moves anything on screen.
    Group {
        ids: Vec<ObjectId>,
        name: Option<String>,
    },
    /// The inverse of [`Command::Group`]: dissolves each group in `ids`,
    /// splicing its children back into the group's own parent at the
    /// position the group occupied (preserving both the children's
    /// relative order and stacking relative to untouched siblings). Errors
    /// if any id isn't a group. Group descendants that are themselves
    /// groups are left alone — this dissolves exactly the groups named,
    /// not everything nested inside them.
    Ungroup {
        ids: Vec<ObjectId>,
    },
    /// Sets every listed object's fill paint, one undo group.
    SetFill {
        objects: Vec<ObjectId>,
        paint: Paint,
    },
    /// Sets every listed object's stroke paint, one undo group. Stroke
    /// *width* isn't user-controllable yet (see
    /// [`amalith_core::Appearance::DEFAULT_STROKE_WIDTH`]) — this only
    /// ever changes the color/None-ness of the stroke.
    SetStroke {
        objects: Vec<ObjectId>,
        paint: Paint,
    },
}

/// Where a pasted clone lands in its target parent's paint order. See
/// [`Command::Paste`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteStack {
    /// Top of the target parent's stack (plain Paste / Paste in Place).
    Top,
    /// Immediately above the copied root's current source object, if it
    /// still exists; otherwise falls back to `Top` (Paste in Front).
    InFront,
    /// Immediately below the copied root's current source object, if it
    /// still exists; otherwise falls back to the bottom of the target
    /// parent's stack (Paste in Back).
    Behind,
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
    /// The single new object (or, for [`Command::Paste`] with several
    /// copied roots, the first root — same relative order as the
    /// clipboard) created by the command.
    Object(ObjectId),
}
