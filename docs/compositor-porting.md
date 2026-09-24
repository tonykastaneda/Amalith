# Compositor → Amalith Rust ports

Source: https://github.com/robbietilton/Compositor
Reviewed revisions: each section names the upstream commit it was ported
from. The earlier ports (brush tip, tiled painting, Clone Stamp, pixel
transform) used `37dbe59b3cf71184b016e4f2f4aa74eada533aec`; adjustment
layers use `430620694ab001d80448e0dad44342f108ebbfde`. The per-file record
is the table in the notice below.
Upstream is read for reference, never merged into Amalith.
License and attribution: [Compositor notice](../third_party/compositor/NOTICE.md).

## Integration approach

Port pixel algorithms and interaction behavior into the existing Rust
model described in [vector-raster-system.md](vector-raster-system.md).
Keep Amalith's editable vector shapes and text, object transforms,
pixel selections, `ReplaceImageAsset` commands, and undo system.

Compositor uses Swift/AppKit/Core Graphics, C pixel kernels, and Metal
rendering. Algorithms can be translated; UI and framework calls need
native implementations in Amalith's winit/Vello/wgpu stack. Its
premultiplied RGBA operations must be adapted to Amalith's straight-alpha
CPU images rather than copied verbatim.

## First completed port

`BrushRaster.falloff` in `Compositor/Document/BrushStroke.swift` is now
implemented in `app/brush_tip.rs`: normalized Gaussian soft-tip falloff.
Brush and Pixel Eraser have independent 0–100% hardness settings.
Shift+[ / Shift+] adjust hardness by ten percentage points; [ / ] still
adjust diameter. Hardness is displayed beside the cursor. The settings
are session-local and captured at stroke start. The default remains hard.

This ports the falloff, not the full upstream brush engine. Amalith still
uses its existing per-stroke maximum coverage and asset commit path.
Compositor additionally has soft-dab accumulation and distance-based
stroke sampling; those remain separate work.

## Tiled live painting

Amalith's `app/paint_tiles.rs` implements sparse 256×256 stroke-coverage
tiles and immutable preview images. At stroke start the base image is
split into preview tiles once. Subsequent movement rebuilds only dirty
tiles and any neighboring sampling gutters; unchanged GPU image blobs
are shared. The canvas renders these tiles through the existing image
object, retaining transforms, opacity, clipping, and stack position.

Coverage allocations are limited to touched tiles. The decoded base image
is still held in full, and stroke commit still assembles and encodes a
full PNG for the existing copy-on-write undo/save path. Asset garbage
collection and tiled undo are not included. The existing 16-megapixel
editing limit remains. This tile implementation is native Rust code,
not a translation of Compositor's Core Graphics/Metal renderer.

Tests compare tiled composition against dense painting/erasing, check
edge gutters and partial tiles, verify untouched preview blob reuse, and
provide a GPU comparison against whole-image rendering for rotated,
translucent content.

## Clone Stamp

`Document/EditorSession+Brush.swift`'s clone behavior is now
`Tool::RasterCloneStamp` in `app/raster_brush.rs`. It reuses Brush's
entire stroke pipeline unchanged — `Stroke.color: Rgba<u8>` became
`Stroke.source: PixelSource` (`Flat` for Brush/Eraser/Fill, byte-identical
to before; `Clone { offset }` for the stamp), so the same soft-tip
`coverage()`, tiled ink, live preview, and `ReplaceImageAsset` commit
path all apply to Clone Stamp with no duplicated machinery. Target
resolution (find the object under the cursor, decode it, enforce the
16-megapixel cap, project the current pixel selection) was factored out
of `raster_brush_press` into `raster_paint_target`, shared by both tools.

Option-click sets the source point on the image under the cursor; a
plain click paints, sampling `base` (never the in-progress `ink`, so a
stroke can't smear its own output back into its source) at a fixed
pixel-space offset from the destination. Aligned (Photoshop's own
default, on) locks that offset for every stroke until a new source is
set; Shift+A toggles it off, which resamples from the original source
point at the start of each new stroke instead. [ / ] and Shift+[ / ]
adjust Clone Stamp's own independent size/hardness, same convention as
Brush/Eraser. Scoped to one image at a time: the source and the paint
target must be the same object, matching Brush's own single-target scope.

Not ported: cross-layer/cross-document sampling, and Compositor's own
soft-dab accumulation (still out of scope per the falloff port above).

## Selected-pixel transform

`Document/FloatingSelection.swift`'s lift-transform-commit flow is now
`Drag::PixelTransform` in the new `app/pixel_transform.rs`, entered by
pressing `Tool::FreeTransform` (still `E` — no new shortcut) while
`doc.pixel_selection` is set. `app/free_transform.rs`'s own machinery
(`Drag::Warp`) is vector-path-specific (it solves a homography and warps
Bézier points), so it isn't what's reused; what genuinely carries over is
`handles.rs`'s plain geometry — `hit_handle`/`hit_rotate_halo` for
hit-testing and `scaled_transform`/`rotate_transform` for the live affine
— the exact same functions the ordinary Select-tool bounding-box handles
already use for whole-object scale/rotate. Scoped to translate + scale +
rotate; no shear/perspective, which would need real projective image
warping, a separate, larger piece of work.

Press lifts the selection: crop the target image to the selection
contours' bounding box into a floating buffer (mask outside the contour
to transparent, reusing `raster_brush::inside`), and clear that same area
in a clone of the base image (the "hole" left behind, shown live via the
same transient-preview-asset technique `raster_brush::raster_preview_doc`
already provides for Brush/Clone Stamp). The live drag needs no CPU
resampling at all — the floating piece draws straight through
`Scene::draw_image` at its current affine, the same free GPU rotation
every placed image already gets. Release bakes the floating buffer into
the hole at its final transform via a hand-written bilinear
inverse-affine sampler (`resample_into`, new and unit-tested — the one
piece Vello's own scene graph can't do off-GPU) and commits one
`ReplaceImageAsset`, exactly like every other raster edit. Escape or a
transform that never moved commits nothing.

Not ported: cross-object/floating-selection persistence across multiple
edits (Compositor's `FloatingSelection` can stay "lifted" through several
operations; here it always resolves to one commit or a full cancel), and
shear/perspective.

## Layer masks

Implemented, but this is Amalith's own code rather than a translation.
`ImageMask { asset, enabled }` hangs off `ImageData` (`amalith-core`), and
only its alpha matters: 255 reveals, 0 hides. `AddLayerMask`,
`ReplaceMaskAsset`, `SetMaskEnabled` and `RemoveLayerMask` are copy-on-write
asset commands, each one undo step. The Layers panel footer's mask button
adds a mask (a white seed) or toggles `Doc.editing_mask`. While that's on,
Brush, Eraser, Fill and Clone Stamp paint into the mask and commit through
`ReplaceMaskAsset`. The canvas composites the mask with `Compose::DestIn`
(`canvas.rs` `paint_raster` / `paint_tiled`). No UI calls the disable and
delete commands yet.

## Magic Wand

Also Amalith's own code, not a port: `crates/amalith-shell/src/magicwand.rs`
flood-fills by color tolerance and traces the result into the shared
`Doc.pixel_selection` contours used by Marquee and Lasso.

## Adjustment layers (in progress)

Photoshop-style adjustment layers, ported from upstream at
`430620694ab001d80448e0dad44342f108ebbfde`: `Document/LayerAdjustment.swift`,
`Levels.swift`, `Curves.swift`, `HueSaturation.swift`,
`ImageAdjustments.swift`, `Rendering/AdjustPixels.c`, `LevelsPixels.c` and
`NoisePixels.c`. Upstream's blurs are CoreImage, so Gaussian and Motion
Blur port the behavior (radius as sigma, streak length, margins), not code.

How it fits Amalith's model:

- An adjustment is an object, `ObjectKind::Adjustment`, among a layer's
  top-level children. It changes everything beneath it **in its own layer
  only**, since a raster layer is a self-contained unit. Opacity, visibility
  and name are the ordinary object fields; masks reuse `ImageMask`.
- Pixel math lives in the `amalith-adjust` crate as plain Rust. Each color
  op compiles to a lookup table: exact 1D tables for Levels, Curves, Exposure
  and Invert, and a 33³ cube for Hue/Saturation, Color Balance and
  Black & White. One GPU shader applies any of them.
- The canvas renders a layer's content beneath each adjustment offscreen,
  runs the shader, and swaps the result in through vello's `override_image`.
  PDF and SVG export bake adjusted layers into pixels.

Status:

- Done: the model and commands; the color math (`amalith-adjust`, with a
  65³ cube rather than upstream's 33³ for accuracy); GPU rendering on the
  canvas, in split panes and in PNG/JPG export (`amalith-shell/src/adjust/`),
  checked against the CPU reference on a real GPU and translated to HLSL
  and MSL in tests. Adjustments can be created from the command palette.
- Next: the Layers panel button and Layer menu, adjustment masks on
  screen, the Properties panel, Levels/Curves editors, blur and noise, and
  baking adjusted layers into PDF/SVG.

## Candidate ports, in suggested order

| Feature | Upstream implementation | Amalith integration |
| --- | --- | --- |
| Tiled brush updates | `Document/BrushStroke.swift`, `Rendering/MetalBrushCoverage.swift` | Live CPU coverage and GPU previews implemented in native Rust; tiled undo and GPU paint kernels remain deferred. |
| Adjustment layers | See the section above | In progress. |
| Healing/content-aware fill | `Rendering/HealPixels.c`, `Rendering/ContentFill.c` | Port pure kernels to Rust with deterministic fixtures, explicit bounds checks, and cancellation for expensive work. |

Do not import Compositor's document/history model or macOS interface as
a parallel raster editor. Preserve the copyright/license notice with
each translated portion and record its exact upstream revision here and
in the notice table.
