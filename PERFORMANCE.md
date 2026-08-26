# Performance standard

Law, not essay. Read before touching rendering, hit-testing, or bounds.

## 1. egui draws chrome; artwork rendering may later be wgpu/Vello

egui owns panels, tabs, handles, and other UI chrome. Artwork rendering
(the pasteboard/canvas contents) may later move to a wgpu/Vello pipeline
for performance. Do not mix a second document model into that GPU layer —
it reads `amalith-core::Document` (via `amalith-commands::Editor`) the same
as everything else. No parallel scene graph.

## 2. Bounds caching never lives on `Document`

`Document`'s raw mutators (`insert_object`, `object_mut`, `remove_object`,
etc.) never gain a bounds cache. A cache on those primitives goes stale the
moment a caller mutates an object without going through the cache
invalidation path — and Rust cannot stop a caller from doing that (see
`DESIGN.md`, "No compiler-enforced commands-only boundary"). `Document`
stays the correct-by-construction source of truth: `Document::bounds_of`
keeps recomputing on every call.

The cache lives on `Editor`, and is invalidated only at the three places
`Editor` mutates the document: `execute`, `undo`, `redo`.

## 3. Cache policy: wipe on every successful mutation

The first cache implementation clears the *entire* bounds cache after any
successful `execute` / `undo` / `redo` — no per-ID diffing. This is
deliberately conservative: correctness over cleverness for the first cut.

Incremental invalidation (clear only the IDs a given `Edit` touched) is
allowed later, but only as logic inside the command engine
(`amalith-commands`), never as a cache write from GUI/CLI/agent code and
never by touching `Document`.

## 4. Cull before draw and hit-test

Before tessellating or hit-testing, the UI crate (`amalith-app`) must cull
objects whose bounds don't intersect the visible viewport. Off-screen
objects on the infinite pasteboard must not cost tessellation time or
hit-test time. This is UI-crate work, not core/commands work, and is not
implemented yet.

## 5. No spatial index until cull + cache exist

Do not build a spatial index (grid, R-tree, BVH) before viewport culling
and the `Editor` bounds cache both exist and are in use. When a spatial
index is finally justified, the next step is a coarse uniform grid — not a
BVH. A BVH is not a starter project; it is what you build after the grid
turns out not to be enough, with profiling data in hand.

## 6. Never bypass the command engine to "optimize"

No shortcut — caching, culling, batching, or otherwise — may call
`Document`'s raw mutators directly to skip `Editor::execute`. Every
mutation stays undoable and observable in one place. If a mutation path is
slow, fix the path inside `amalith-commands`; don't route around it.
