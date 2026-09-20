# Vector/Raster system

Amalith is not "Illustrator plus a bolted-on Photoshop." It's one object
model and one canvas that happen to hold two kinds of content, edited
through two tool contexts that share as much machinery as possible. The
guiding rule for every raster decision from here on: a raster tool earns
its place by extending the *one* existing model (one selection concept,
one transform concept, one undo system), not by importing a convention
wholesale from Illustrator or Photoshop because that's "how it's done
elsewhere." This document is that model, written down, plus a resolved
answer to the open question that prompted it (should Free Transform have
a separate Photoshop-style `⌘T` identity in raster mode).

## `LayerKind` is a hint, not a partition

`Layer.kind` (`amalith-core/src/layer.rs`) is `Vector` or `Raster`. It
picks which Tools-panel grid shows (`panels::tools::vector_slots` /
`raster_slots`) and which toolset a fresh selection defaults into. It is
**not** a content restriction: a Vector layer can hold a placed image
exactly as Illustrator always could, and nothing stops a Raster layer
from holding a path if one ends up there. The layer is a *workspace*, not
a type system.

## One object model, so object-level transform was never a raster problem

Every `ObjectKind` — `Path`, `Image`, `Group`, `Text`, `Symbol` — carries
the same `transform: Affine` and gets the same Select-tool bounding-box
handles and `Tool::FreeTransform` (`E`) behavior, regardless of which
layer it lives on. This was true before any raster tool existed, because
a placed image has always been just another object. So the "images
always have transform handles in Vector mode" observation isn't a raster
exception to explain away — it's the baseline every object already gets,
and Raster layers inherited it for free. There is no separate
raster-specific transform mechanism to design here; see below for the
one place this baseline actually runs out.

## Raster content has no mutable buffer — it's copy-on-write assets

There is deliberately no per-layer pixel canvas anywhere in the data
model. An image is `ObjectKind::Image { asset: AssetId, local_bounds }`,
and a "pixel edit" is: decode the current asset's bytes, mutate a CPU
buffer, encode a brand-new PNG, insert it as a **new** asset, and
repoint the object at it (`Command::ReplaceImageAsset`, which compiles to
`Edit::InsertAsset` + `Edit::SetImageAsset` — the old asset is never
deleted, so undo/redo and any earlier duplicate still resolve against
intact bytes). `raster_brush.rs`'s `commit_raster_brush` is the reference
implementation: composite the stroke over the base image, `write_to` a
PNG, `asset_store.insert`, execute `ReplaceImageAsset`.

This is the same "why" as `LayerKind` — it lets raster editing ride the
*existing* undo/asset infrastructure instead of inventing a second one —
but it has a real, known cost: every brush stroke leaves its previous
version's bytes sitting in the document forever. That's an accepted
tradeoff for now, not an oversight; asset garbage collection (drop an
asset once nothing references it and it's fallen out of undo range) is
future work, not blocking anything today.

## One pixel selection, shared by every tool that needs one

`Doc.pixel_selection: Option<PixelSelection>` (`{ object: ObjectId,
contours: Vec<Vec<Point>> }`, in the object's own local space) is the
single shared primitive behind Magic Wand, the rectangular/elliptical
Marquee, and Lasso (`app/raster_selection.rs`) — each just produces a
different contour and writes it to the same field. It is transient and
explicitly **not** part of the `Command`/`Edit` undo stack, the same
treatment ordinary object `selection: Vec<ObjectId>` already gets
(Photoshop/GIMP-style: making a selection isn't a document edit).

The Brush already consumes it as a paint mask (`raster_brush.rs`'s
`Stroke::stamp` checks `inside(selection, pixel)` before touching a
pixel) — this is the "raster masking" workflow from the original ask,
now load-bearing rather than aspirational: Magic Wand (or Marquee/Lasso)
selects a region, Brush respects it, and the eventual "cut/clip to
selection" operation is a small step from here (it has what it needs:
the same contours, the same object, the same bake-to-new-asset commit
path Brush already exercises).

## The open question: does raster mode need its own `⌘T`?

No. There are two genuinely different "transform" operations, and only
one of them exists today:

1. **Transform the object** — move/scale/rotate the whole placed image
   as a unit. Already fully built, already unified, needs nothing new:
   it's the Select-tool handles / `Tool::FreeTransform` every object
   already has, and it doesn't care what `LayerKind` the layer is.
2. **Transform the pixel content of a selection** — Photoshop's `⌘T`:
   select a region (Lasso an eye, say), then scale/rotate *just those
   pixels* in place, independent of the rest of the image. This does not
   exist yet, and it's the only piece actually missing.

The fix is not to give raster mode a second, Photoshop-flavored shortcut
living alongside Illustrator's own `E` for the same word ("transform") —
that's exactly the two-apps-bolted-together outcome this whole model is
trying to avoid, and it means every future raster tool decision has to
ask "which app's convention wins" instead of "what does *our* model do."
Recommendation: keep the existing `Tool::FreeTransform` (`E`) as the only
transform entry point, and make it **selection-aware**:

- `doc.pixel_selection` is `None` → today's behavior, unchanged: transform
  the object.
- `doc.pixel_selection` is `Some` → crop the selected pixels into a
  floating buffer, show transform handles around *that* region's bounds
  (not the whole image's), and on commit bake the transformed result back
  via the same `ReplaceImageAsset` path Brush already uses.

One shortcut, one mental model, one commit path, in both modes. This is
also the test to apply to the next raster idea that shows up because
"that's how Photoshop does it": does it extend one of the four things
above (layer-as-workspace, unified object transform, copy-on-write
assets, shared pixel selection), or does it start a second, parallel
system that only raster mode knows about? If the latter, it needs its
own design pass before it lands, not just a keybinding.

## Not yet decided / explicitly deferred

- Pixel-content transform (above) is designed, not implemented.
- Asset garbage collection for superseded brush-stroke bytes.
- Cut/clip-to-selection as an actual command (contours + bake-to-asset
  both already exist; the command itself doesn't yet).
- Whether a Raster layer should ever get its own persistent scratch
  canvas (a real per-layer buffer) once brush/eraser usage patterns make
  the copy-on-write cost visible — not needed today, worth revisiting
  once there's real usage to measure against.
