# Image Trace

Select one placed image and open **Window → Panels → Image Trace**. The panel works docked, floating, or in a panel flyout. Click **Image Trace** to start; selecting an image or opening the panel never starts tracing. Completed previews remain visible when you deselect the image, select another object, or close the panel. Reselect the image to adjust its trace. After this explicit start, Preview automatically retraces after a short pause in slider input. With Preview off, click Trace explicitly. Cancel stops an outstanding job; Hold Original compares against the source until mouse release.

Modes are Black and White (Threshold), Grayscale (Grays), and Color. Color palettes are Limited (color count), Full Tone (Detail), or Document Swatches (the document's current swatch colors). Paths controls fitting/simplification, Corners controls corner retention, and Noise filters small source-pixel regions. Numeric boxes accept typed values and Enter/Escape. Transparency preserves transparent regions using an alpha cutoff; semitransparent pixels above the cutoff are composited against white. Ignore Color masks source colors within eight levels per RGB channel of the selected color. Click its swatch to enter a six-digit RGB hex value, or Pick and then click the source image after the first trace finishes.

The View menu offers result, result with outlines, outlines, source with outlines, and original. Info reports path, anchor, and color counts for the current result. The panel hamburger saves, renames, and deletes custom presets; preset names are entered in the Preset field. Options and up to 16 custom presets persist in `image-trace.json` alongside the application's settings.

Expand creates ordinary grouped paths in one undo step, preserving the source image's ID, parent, stacking position, local size, transform, and object opacity. Undo restores the original linked/embedded image. Before Expand, previews do not modify the document and are not saved as live tracing objects. Saving retains the original image; after Expand, saving retains the editable vectors.

## Implementation

`amalith-trace` adapts VTracer's Rust library, pinned to commit `669a29481449366f0f235b6e4d24134d8f91c9b4` (1.0.0-alpha.4). It converts vector IR directly to Amalith paths. A background worker owns a reusable tracing session; generation/source tokens discard obsolete results, and cancellation also interrupts preprocessing. A transparent one-pixel border guarantees alpha keying even for tiny holes, and is removed when mapping coordinates back to original image pixels. Completely masked images bypass an upstream empty-image division by zero.

Previews use a maximum 1024-pixel edge. Expand regenerates reduced previews at source resolution before committing. Source images are limited to 32 megapixels, and output is limited to 100,000 paths / 1,000,000 anchors. Failures keep the original image intact. Outputs are filled paths: stroke reconstruction, gradient detection, live shape recognition, automatic semantic grouping, and curve-to-line snapping are not part of this release.

## Verification

- `cargo build --workspace --tests --examples`
- `cargo test --workspace`
- `cargo run -p amalith-shell --example ui_scale_review -- /tmp/trace-review.png 1 trace`

The review example renders the panel next to an actual traced PNG and writes the original test image to `/tmp/trace-review.png.source.png`. Unit tests cover thresholds, grayscale, transparent holes, fully ignored images, cancellation, preview coordinates, panel hit geometry, preset serialization, and atomic expansion/undo/redo.
