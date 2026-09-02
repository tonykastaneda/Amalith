# Align: `&self` vs `&mut self`, and what actually got faster

Companion to [`PERFORMANCE.md`](PERFORMANCE.md). Written after the Align
panel landed, when we looked at making `Editor::compile` mutable so it
could use the bounds cache. **Do not flip `compile` to `&mut self` for
Align.** This file is why.

## `&self` vs `&mut self`

Rust’s way of saying “can this function change the thing it’s called on?”

- **`&self`** — read only. Several places can look at the editor at once.
- **`&mut self`** — exclusive write. Only one caller can hold it, because
  it might change fields.

## Where the bounds cache lives

`Document::bounds_of` always recomputes. That is the source of truth.

The memoized path is `Editor::bounds_of(&mut self, id)`. Filling the cache
is a write (“I computed this box, store it”), so the method takes `&mut
self`. The cache is wiped after every successful `execute` / `undo` /
`redo`. See `PERFORMANCE.md` §§2–3.

## Why Align compile cannot use the cache today

Align compiles through `Editor::compile(&self, …)`. Read-only. It is not
allowed to touch `bounds_cache`, so it calls `document.bounds_of` and
recomputes every time.

That is a Rust rule, not a design preference. `execute` already has `&mut
self` (it mutates the document and history). If `compile` were `&mut self`
too, Align *could* call `Editor::bounds_of`. We did not do that.

## Ramifications of making `compile` `&mut self`

Flipping it is a **safety change**, not a free speedup.

Today compile is read-only. It can look at the document and produce a list
of edits. It cannot move, insert, or delete anything. Rust enforces that.

`&mut self` means compile is allowed to change the Editor. The bounds
cache lives on the Editor, so that’s the only reason to do it. It also
means compile *could* call `Document`’s raw mutators (`insert_object`,
`remove_object`, …). Undo, redo, and “every change goes through commands”
would no longer be compiler-enforced for that path — only a comment and
review. That fights `PERFORMANCE.md` §6.

### Why Align still would not get much faster

`execute` already does:

1. Compile the command (read the document)
2. Apply the edits (objects actually move)
3. **Wipe the whole bounds cache**

Even if compile filled the cache, that work is thrown away as soon as the
align finishes. The cache is for **reads between commands** (paint,
hit-test, culling), not for the align click itself.

On one Align click we ask bounds once per selected object. The cache only
helps if we asked the **same** id twice, or a group and then its children.
Typical Align does not.

### The cache is mostly unused on the UI path anyway

Paint, selection, and culling call `document.bounds_of` on a shared
`&Document`. They never go through `Editor::bounds_of`, because those
paths do not have exclusive access to the Editor.

| Do it? | What you get | What you risk |
|---|---|---|
| Keep `compile` read-only (now) | Undo stays mechanically safe | Align of thousands of objects still recomputes each bbox once per click |
| Make `compile` `&mut self` | Compile *could* memoize overlapping bbox queries | Compile can mutate the document; a future “optimization” can skip undo |

For Align, don’t do it.

## What we actually changed (and shipped)

Not the `&mut self` flip. Two small, equivalent-result cuts:

1. **Align click** (`compile_align` in `amalith-commands`) — apply the
   document-space delta with **one parent-chain walk** per object instead
   of two. Layer children skip the inverse entirely (`translate * local`).
2. **Options-bar hover** (`context_bar_tip_ctx` in the shell) — tooltips
   no longer recompute the Transform X/Y/W/H readout (`local_bounds_of` +
   `world_transform`, recursive for groups) on every mouse move. Tip
   layout only needs selection count, text-context, and pointer.

## What is cheap by design

- Panel / Control-bar icon paint: a few dozen rects per redraw.
- Key-object outline: one extra oriented quad per frame, only with a key.
- `sync_align_mode`: O(selection) on click, not per frame.
- Click-to-set-key slop: no canvas redraw until the pointer moves 4 screen
  px, so a click is not a drag.
- Distribute sort: O(n log n) on the click; typical n is small.

No per-frame align math. No extra GPU tessellation.

## If huge selections ever feel slow

The useful move is using the cache on **paint / hit** (same bounds asked
every frame), not unlocking compile. That means the shell calling
`Editor::bounds_of` where it today holds only `&Document` — a borrow-shape
change, not an Align-panel tweak.

`compile(&mut self)` so Align can use `Editor::bounds_of` is a
command-engine change. Do not do it without a measured problem and a
plan that still keeps compile from mutating the document.
