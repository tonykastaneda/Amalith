# amalith-core design notes

This file records the decisions that aren't obvious from the code itself —
places where Inkscape does something different and we deliberately didn't
copy it, or where two reasonable designs existed and one was picked. Each
decision below is backed by a test that encodes it, so a future change that
contradicts the decision fails loudly instead of silently drifting.

## One tree, not a repr tree + item tree

Inkscape keeps two synchronized trees: the XML repr tree (the literal SVG
DOM, source of truth) and the `SPObject`/`SPItem` tree (the "live" object
graph, kept in sync with the repr via observers). That split exists because
Inkscape's native file format *is* SVG — the DOM has to be authoritative,
and `SPObject` exists to give that DOM live behavior (bounding boxes, style
cascade, etc).

Amalith's native format is not "serialized DOM" (see `amalith-io`): the
`.amalith` container's `document.json` / `artwork/*.json` files are a direct
serialization of `Document`'s own structures, not a markup tree Amalith
happens to also interpret as a document. With no DOM to stay authoritative
over, a second synchronized tree would be pure bookkeeping overhead with no
payoff. `Document` is the one and only tree. SVG becomes relevant only as
an interchange format later, at which point it's an *export/import*
problem (translate `Document` <-> SVG DOM), not an internal representation
choice.

## Layers are global; artboards don't own objects

Two reasonable ownership trees existed: `Document -> Artboard -> Layer ->
Object` (Inkscape-style, where each SVG document is really one drawing
surface and "artboards" are closer to bespoke viewport annotations) or
`Document -> Layer -> Object`, with artboards as independent geometric
regions (Illustrator's model).

Amalith picked the Illustrator model, because the brief's compatibility
target is Illustrator workflows specifically: layers that span the whole
pasteboard and are independent of which artboard(s) objects happen to sit
under, objects that can straddle two artboards or live on the infinite
pasteboard outside all of them, artboards that reorder/resize without
reparenting anything. Making artboards own objects would have required
picking a single artboard owner for every straddling/pasteboard object,
which doesn't correspond to anything a designer does.

The consequence: "what's on artboard X" is a geometric query (intersect
object bounds with the artboard rect), computed on demand — never a stored
edge — so it can't go stale as objects move. See `artboard.rs` and
`layer.rs`.

## One coordinate system, not one-per-artboard

Object transforms and path geometry are stored in a single document-space
coordinate system (canonical px), never artboard-relative. An
artboard-relative position is always `document_position -
artboard.rect.origin()`, computed on demand.

The alternative — storing each object's position relative to "its"
artboard — falls apart for exactly the objects that motivated the layer
decision above: an object with no single owning artboard (pasteboard
content, or content straddling two artboards) would have no well-defined
local space to be stored relative to, and moving an object between
artboards or resizing an artboard would require rewriting stored
coordinates rather than just changing the artboard's own rect.

## IDs are UUIDs, not array indices or incrementing counters

`ArtboardId`/`LayerId`/`ObjectId`/`AssetId` wrap a `Uuid` (see `ids.rs`)
rather than an index into a `Vec` or a document-local counter. Indices
break the moment something is removed or reordered (every ID after the
removed entry silently shifts); a document-local counter works until two
documents' content needs to be merged, diffed, or referenced from an
external tool/script/agent across a process boundary — exactly the
scenario the brief's CLI/plugin/agent surfaces require. UUIDs cost 16 bytes
and a fixed generation call; both problems disappear.

## Bounds are computed, not cached

`Document::bounds_of` recomputes an object's document-space bounds by
composing world transforms and geometry on every call, rather than storing
a cached bounding box on each object that mutation code would have to
remember to invalidate. Given the size of documents this crate deals with
today (no rendering/hit-testing performance work has happened yet), a
correct-by-construction computed value that can never go stale beats a
cached one that can silently disagree with the geometry it was supposed to
describe. If profiling ever demands a cache, it should be an
invalidate-on-write cache built on top of the command engine's mutation
points (`amalith-commands`), not a field threaded through `amalith-core`'s
raw mutators.

## kurbo instead of a bespoke 2geom port

The brief suggests `kurbo` as Rust's analogue to 2geom. It already
provides exactly the primitives 2geom does — `Affine`, `Point`, `Vec2`,
`Rect`, `BezPath` — with the same "immutable value type" philosophy
`Geom::Affine` uses. There was no 2geom lesson left to translate:
reimplementing affine composition or Bezier bounding-box math by hand
would just be reproducing kurbo with extra bugs. `geom.rs` re-exports
kurbo's types directly and adds only the handful of helpers
(`transformed_bounds`, `union_bounds`) that make call sites elsewhere in
this crate read as document vocabulary instead of raw kurbo calls.

## No compiler-enforced "commands only" boundary

The brief requires that "GUI must never own or mutate document logic" —
all mutation should go through `amalith-commands`. Rust has no cross-crate
"friend" visibility (unlike C++ `friend class`), so `Document`'s raw
mutation methods (`insert_object`, `remove_artboard`, `object_mut`, etc.)
are necessarily `pub`, since `amalith-commands` is a separate crate and
needs to call them. This crate cannot make it a compile error for a UI
crate to call `document.object_mut(id).transform = ...` directly.

The boundary is therefore a naming/documentation convention, not a type
one: every raw mutator's doc comment says it's a primitive `amalith-commands`
composes into undoable commands, not a public editing API. This matches
how Inkscape itself enforces "never mutate ad hoc" — `DocumentUndo`'s
discipline is also a convention Inkscape's own code has to follow, not
something `SPObject`'s C++ types make impossible to violate.
