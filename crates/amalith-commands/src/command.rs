//! The public command vocabulary. GUI tools, keyboard shortcuts, plugins,
//! scripts, the CLI, and agents all describe mutations as a `Command`
//! value and hand it to [`crate::Editor::execute`] â nothing constructs a
//! `Command` and applies it any other way, so there is exactly one place
//! (`crate::edit`) that turns intent into an actual, undoable document
//! change. This is the Rust translation of Inkscape's `DocumentUndo`
//! discipline: never mutate ad hoc, always go through the logged path.
use amalith_core::{
    Affine, Appearance, ArtboardId, AssetId, AssetSource, GuideId, Color, Gradient, GradientId, GradientKind,
    GuideOrient, Homography, LayerColor, LayerId, ObjectId, ObjectParent, Paint, PathData, ColorMode, Rect,
    StrokeStyle, TextData, Unit, Vec2,
};
use crate::align::{AlignKind, AlignTo};

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
    /// Adds a ruler guide. Yields [`CommandOutcome::Guide`].
    AddGuide {
        orient: GuideOrient,
        /// Canonical px: `y` for a horizontal guide, `x` for a vertical.
        pos: f64,
    },
    /// Slides an existing guide to a new coordinate.
    MoveGuide {
        id: GuideId,
        pos: f64,
    },
    DeleteGuide {
        id: GuideId,
    },
    /// Removes every guide in the document.
    ClearGuides,
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
    /// Sets an artboard's background fill (`None` = transparent).
    SetArtboardFill {
        id: ArtboardId,
        fill: Option<Color>,
    },
    /// Sets the document's default measurement unit (rulers, dialogs).
    /// Geometry stays in canonical px â this is a display-only setting.
    SetDocumentUnit {
        unit: Unit,
    },
    /// Sets the document colour mode (CMYK / RGB) — affects the New
    /// Document defaults and colour readouts.
    SetColorMode {
        mode: ColorMode,
    },
    RenameLayer {
        id: LayerId,
        name: String,
    },
    /// Layer Options (double-click a layer's color swatch): every field
    /// the dialog exposes, committed together as one undo step, matching
    /// the dialog's own all-or-nothing OK/Cancel.
    SetLayerOptions {
        id: LayerId,
        options: LayerOptions,
    },
    /// Renames an object (including a group) â the same field an object
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
    /// Creates an arbitrary path primitive as the top-most child of `layer`.
    CreatePath {
        layer: LayerId,
        path: PathData,
        name: Option<String>,
    },
    /// Places a raster image as the top-most child of `layer`.
    /// `path` is the source file for a linked asset, or the container path
    /// for an embedded one (`embedded: true`). `bounds` is the image's
    /// local box (typically `0,0,px_w,px_h`); `transform` puts that box
    /// in the parent's space. The original file is never moved. `modified`/
    /// `size` are the source file's stamp at place time, for a linked
    /// asset (this crate never touches the filesystem itself — the caller
    /// already has to read the file to get its pixel dimensions, so it
    /// reads this stamp at the same time; `None` for an embedded asset, or
    /// if the stamp couldn't be read).
    CreateImage {
        layer: LayerId,
        path: String,
        bounds: Rect,
        transform: Affine,
        name: Option<String>,
        embedded: bool,
        modified: Option<i64>,
        size: Option<u64>,
    },
    /// Creates a text object as the top-most child of `layer`, with
    /// `transform` placing its anchor in document space.
    CreateText {
        layer: LayerId,
        data: TextData,
        transform: Affine,
        name: Option<String>,
    },
    /// Replaces a text object's whole [`TextData`] (content, style, box,
    /// alignment, bounds) in one undoable step. v1's text edits are coarse
    /// by design â the shell rebuilds the full `TextData` and swaps it.
    SetText {
        object: ObjectId,
        data: TextData,
    },
    /// Replaces several text objects' `TextData` in one undoable step â
    /// used when a multi-frame drag re-sizes every selected text box at
    /// once.
    SetTexts {
        items: Vec<(ObjectId, TextData)>,
    },
    /// Links `to` after `from` in a text thread: `from.thread_next = to`,
    /// `to.thread_prev = from`, and `to`'s own content is cleared (the
    /// story lives on the head). Both must be text objects.
    ThreadText {
        from: ObjectId,
        to: ObjectId,
    },
    /// Removes `object` from its thread, stitching its predecessor and
    /// successor together. A no-op if it isn't threaded.
    UnthreadText {
        object: ObjectId,
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
    /// local-space delta. Anchors are flat ordinals across all subpaths.
    /// Multiple anchors on one or several paths form one undoable gesture.
    MoveAnchors {
        anchors: Vec<(ObjectId, usize)>,
        delta: Vec2,
    },
    /// Moves one bezier handle of anchor `anchor` (flat ordinal) on
    /// `object` by `delta` (local space). When the anchor is a smooth /
    /// symmetric point its opposite handle stays mirrored, unless
    /// `break_mirror` is set (the Alt/Option-drag "split the handle"
    /// gesture) — the anchor switches to `Corner` first, so this move
    /// leaves the opposite handle exactly where it already was.
    MoveHandle {
        object: ObjectId,
        anchor: usize,
        side: amalith_core::HandleSide,
        delta: Vec2,
        break_mirror: bool,
    },
    /// Toggles anchor `anchor` (flat ordinal) on `object` between a sharp
    /// corner (no handles) and a smooth point (mirrored handles from the
    /// neighbour directions).
    ToggleAnchorSmooth { object: ObjectId, anchor: usize },
    /// Converts anchor `anchor` (flat ordinal) on `object` explicitly to a
    /// smooth point or a sharp corner.
    SetAnchorSmooth {
        object: ObjectId,
        anchor: usize,
        smooth: bool,
    },
    /// Splits segment `segment` (flat ordinal) of `object` at parameter
    /// `t`, keeping the curve shape.
    InsertAnchor {
        object: ObjectId,
        segment: usize,
        t: f64,
    },
    /// Removes anchor `anchor` (flat ordinal) from `object`.
    DeleteAnchor { object: ObjectId, anchor: usize },
    /// Joins two open-path endpoint anchors — the Join tool's endpoint-
    /// connect drag, or the right-click Join context-menu item.
    /// `anchor_a`'s object always survives (keeps its id, appearance, and
    /// z-order); when the two anchors belong to different objects,
    /// `anchor_b`'s object is removed once its geometry (transformed into
    /// `anchor_a`'s object's local space) has been appended and joined in
    /// — the two objects must share a parent
    /// (`CommandError::ObjectsSpanMultipleParents` otherwise, matching
    /// `PathfinderOp`). Already-coincident endpoints collapse into one
    /// shared anchor with no extra segment; otherwise a straight segment
    /// connects them.
    JoinAnchors {
        anchor_a: (ObjectId, usize),
        anchor_b: (ObjectId, usize),
    },
    /// The Join tool's overlap-trim drag: trims each of two open paths'
    /// terminal segments back to where they cross, then joins the two
    /// newly-coincident free ends — one undo step for the trim and the
    /// join together.
    TrimAndJoinPaths { a: JoinTrim, b: JoinTrim },
    /// Grows an open path with anchors placed while resuming it from one
    /// of its own free endpoints (Pen tool, clicking an existing open
    /// path's end instead of starting a new one) — the live-drawing
    /// complement to `JoinAnchors`, which only connects two endpoints
    /// that already exist. `endpoint` replaces `subpath`'s own endpoint
    /// anchor (carrying forward any handle reshaped by the first new
    /// drag); `new_anchors` walks away from it in placement order,
    /// regardless of `at_end` — see `amalith_core::extend_open_subpath`.
    /// `close` seals the whole subpath into a loop afterward (clicking
    /// back near the resume point itself, the only close gesture the
    /// live-drawing preview can detect while resuming).
    ExtendOpenPath {
        object: ObjectId,
        subpath: usize,
        at_end: bool,
        endpoint: amalith_core::Anchor,
        new_anchors: Vec<amalith_core::Anchor>,
        close: bool,
    },
    /// Replaces `object`'s variable-width stroke profile outright (the
    /// Width tool's add / move / delete of a width point all compile to
    /// this, with the shell recomputing the whole list each time). Empty
    /// clears back to an ordinary uniform-width stroke.
    SetWidthPoints {
        object: ObjectId,
        points: Vec<amalith_core::WidthPoint>,
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
    /// own* existing parent â not funneled into one shared target â and
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
    /// Pushes every anchor point and bezier handle of each listed
    /// object's path data through that object's own projective
    /// transform — Free Transform's Perspective Distort and Free Distort
    /// sub-modes both compile to this. Every object shares one
    /// destination quad (the shell solved one homography in document
    /// space for the whole drag), but each item here already carries
    /// that homography re-expressed in *its own* local space — conjugated
    /// by the object's world transform, since a homography solved in
    /// document space isn't the right map for anchors stored before that
    /// transform (see `amalith_core::warp::Homography::conjugate`).
    /// Objects with no path data (images, groups, symbols, compound
    /// paths) are left untouched, not an error — v1 doesn't warp raster
    /// or composite content.
    WarpPaths {
        items: Vec<(ObjectId, Homography)>,
    },
    /// Nudges selected siblings in paint order. Positive steps move toward
    /// the front; negative steps move toward the back.
    NudgeStack {
        ids: Vec<ObjectId>,
        steps: i32,
    },
    /// Moves `ids` under `parent` (a layer or a group), as one undo group.
    /// The ids need not currently share a parent; they are spliced in
    /// contiguously so that â reading front to back â they keep the order
    /// given, landing so the frontmost sits just in front of `parent`'s
    /// existing child at stacking position `index` (an index into
    /// `parent`'s child list *before* this move; positions vacated by ids
    /// already in `parent` are accounted for). Each object keeps its
    /// on-screen position: its transform is rebased by the difference
    /// between its old and new parent's world transforms. Errors if
    /// `parent` is missing / not a group, or if moving a group would put
    /// it inside itself. A move that changes nothing compiles to no edits.
    Reparent {
        ids: Vec<ObjectId>,
        parent: ObjectParent,
        index: usize,
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
    /// group) â that parent becomes the new group's parent too. The
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
    /// relative order and stacking relative to untouched siblings). Each
    /// child's transform is composed with the group's so on-screen
    /// position, scale, and rotation survive. Errors if any id isn't a
    /// group. Group descendants that are themselves groups are left
    /// alone â this dissolves exactly the groups named, not everything
    /// nested inside them.
    Ungroup {
        ids: Vec<ObjectId>,
    },
    /// Makes a clipping mask: wraps `objects` in a new clip group (like
    /// [`Command::Group`]) whose topmost member becomes the clip path.
    /// Yields [`CommandOutcome::Object`] for the new group.
    ClipMake {
        objects: Vec<ObjectId>,
        name: Option<String>,
    },
    /// Releases a clip group: clears its clip and dissolves it like
    /// [`Command::Ungroup`]. Errors if `group` isn't a clip group.
    ClipRelease {
        group: ObjectId,
    },
    /// Blends two shapes: wraps them in a new blend group (like
    /// [`Command::Group`]) and generates the in-between steps between them
    /// (Smooth Color spacing, no spine, straight line between their
    /// centers). Both must be paths. Yields [`CommandOutcome::Object`] for
    /// the new group. The generated steps live-rebuild afterward whenever
    /// a later command edits the two originals, their fill, or (once set)
    /// the spine — see `Editor::execute`.
    MakeBlend {
        start: ObjectId,
        end: ObjectId,
        name: Option<String>,
    },
    /// Replaces a blend's spacing and/or spine, then regenerates its
    /// steps. Errors if `group` isn't a blend group, or (when set) `spine`
    /// isn't a path.
    SetBlendOptions {
        group: ObjectId,
        spacing: amalith_core::BlendSpacing,
        spine: Option<ObjectId>,
    },
    /// Sets every listed object's fill paint, one undo group.
    SetFill {
        objects: Vec<ObjectId>,
        paint: Paint,
    },
    /// Sets every listed object's stroke paint, one undo group.
    SetStroke {
        objects: Vec<ObjectId>,
        paint: Paint,
    },
    /// Sets fill and/or stroke paint on every listed object in one undo
    /// group â for the Fill/Stroke proxy's swap and reset.
    SetPaints {
        objects: Vec<ObjectId>,
        fill: Option<Paint>,
        stroke: Option<Paint>,
    },
    /// Points every listed object's fill (or stroke, if `stroke`) at a
    /// gradient. With [`GradientRef::New`] a fresh default gradient of the
    /// given kind is minted into the pool first; with
    /// [`GradientRef::Existing`] the pooled gradient is reused. One undo
    /// group. Yields [`CommandOutcome::Gradient`] with the applied id.
    ApplyGradient {
        objects: Vec<ObjectId>,
        stroke: bool,
        source: GradientRef,
    },
    /// Replaces a pooled gradient's whole definition (kind, stops,
    /// geometry). The workhorse for the Gradient panel and the on-canvas
    /// gradient tool: one command per edit commit / drag.
    EditGradient {
        id: GradientId,
        gradient: Gradient,
    },
    /// Removes a gradient from the document pool. Objects still pointing at
    /// the id keep an inert `Paint::Gradient` that renders as nothing.
    DeleteGradient {
        id: GradientId,
    },
    /// Sets every listed object's stroke width, one undo group.
    SetStrokeWidth {
        objects: Vec<ObjectId>,
        width: f64,
    },
    /// Sets every listed object's full stroke style (cap / join / miter
    /// limit / alignment / dash pattern), one undo group.
    SetStrokeStyle {
        objects: Vec<ObjectId>,
        style: StrokeStyle,
    },
    /// Sets every listed object's compositing opacity, one undo group.
    SetOpacity {
        objects: Vec<ObjectId>,
        opacity: f32,
    },
    /// Sets every listed object's `visible` flag, one undo group.
    SetVisible {
        objects: Vec<ObjectId>,
        visible: bool,
    },
    /// Sets every listed object's `locked` flag, one undo group.
    SetLocked {
        objects: Vec<ObjectId>,
        locked: bool,
    },
    /// Relink / Embed / Unembed / Update Link, whichever produced `source`
    /// (the shell decides which — see `Edit::SetAssetSource`). Moving any
    /// bytes between the asset store and disk is the caller's job; this
    /// only swaps the asset's source pointer.
    SetAssetSource {
        id: AssetId,
        source: AssetSource,
    },
    /// Pathfinder boolean on `objects` (paint order back â front).
    Pathfinder {
        op: PathfinderOp,
        objects: Vec<ObjectId>,
    },
    /// Outline each listed object's stroke into a filled path.
    ExpandStroke {
        objects: Vec<ObjectId>,
    },
    /// Object ▸ Path ▸ Offset Path: replaces each listed path's geometry
    /// with its boundary grown (`offset > 0`) or shrunk (`offset < 0`) by
    /// `offset`, joined per `join` (used only for `LineJoin::Miter`).
    OffsetPath {
        objects: Vec<ObjectId>,
        offset: f64,
        join: amalith_core::LineJoin,
        miter_limit: f64,
    },
    /// The Shape Builder tool's one drag, resolved: `objects` is the
    /// selection it was drawn over (paint order); `touched` is the union
    /// of every face the drag actually swept across, in world space.
    /// Every listed object that geometrically overlaps `touched` loses
    /// that part of itself — the rest of the selection (and any part of
    /// a touched object that wasn't swept) is untouched, keeping its own
    /// id. Plain-dragging (`erase: false`) also adds one new object
    /// covering `touched` with `appearance` (the face the drag started
    /// on); Alt-dragging (`erase: true`) just removes it, and
    /// `appearance` is ignored.
    ShapeBuilder {
        objects: Vec<ObjectId>,
        touched: PathData,
        erase: bool,
        appearance: Option<Appearance>,
    },
    /// The Eraser tool's one stroke: `area` (world space) is the swept
    /// brush shape. Each of `objects` independently loses whatever part
    /// of itself overlaps `area` and keeps its own id if anything
    /// survives; one fully consumed by the stroke is removed outright.
    /// Unlike `ShapeBuilder`, `objects` don't need a common parent —
    /// each is handled entirely on its own, so one stroke can erase
    /// across different layers or groups at once.
    EraseArea {
        objects: Vec<ObjectId>,
        area: PathData,
    },
    /// Align / distribute `objects` in document space. `key` is the object
    /// that stays put when `to` is [`AlignTo::KeyObject`]. `artboard` is
    /// the frame when `to` is [`AlignTo::Artboard`]. `spacing` is the
    /// exact gap for Distribute Spacing (`None` = Auto).
    Align {
        objects: Vec<ObjectId>,
        kind: AlignKind,
        to: AlignTo,
        key: Option<ObjectId>,
        artboard: Option<ArtboardId>,
        spacing: Option<f64>,
    },
}

/// One side of a [`Command::TrimAndJoinPaths`]: where to cut one open
/// path back to the shared intersection point the Join tool found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JoinTrim {
    pub object: ObjectId,
    pub subpath: usize,
    /// The free end being trimmed is that subpath's last anchor (`true`)
    /// or first anchor (`false`).
    pub at_end: bool,
    /// Parameter along the terminal segment, in
    /// `amalith_core::insert_anchor`/`trim_to_split`'s own
    /// stored-direction convention.
    pub t: f64,
}

/// Every field the Layer Options dialog exposes — see
/// [`Command::SetLayerOptions`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerOptions {
    pub name: String,
    pub color: LayerColor,
    pub visible: bool,
    pub locked: bool,
    pub template: bool,
    pub print: bool,
    pub preview: bool,
    pub dim_images_to: Option<u8>,
}

/// Illustrator Pathfinder panel operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathfinderOp {
    Unite,
    MinusFront,
    Intersect,
    Exclude,
    Divide,
    Trim,
    Merge,
    Crop,
    Outline,
    MinusBack,
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
/// Where the paint for [`Command::ApplyGradient`] comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientRef {
    /// Reuse a gradient already in the document pool.
    Existing(GradientId),
    /// Mint a fresh default gradient of this kind into the pool.
    New(GradientKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    None,
    Artboard(ArtboardId),
    /// The gradient applied / created by [`Command::ApplyGradient`].
    Gradient(GradientId),
    Layer(LayerId),
    /// The single new object (or, for [`Command::Paste`] with several
    /// copied roots, the first root â same relative order as the
    /// clipboard) created by the command.
    Object(ObjectId),
    /// The new guide created by [`Command::AddGuide`].
    Guide(GuideId),
}
