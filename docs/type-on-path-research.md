# Type on a path: interaction and implementation

Research and implementation review · 7 September 2026  
Audience: Amalith contributors and interaction reviewers

## What the tool should do

Converting a curve to path text should produce one editable text object that owns the curve. Clicking chooses where the text begins. Selection brackets control its available range and position; character editing follows the curve. The original shape’s paint disappears. The stray straight baseline, separate painted circle, and inactive handles in the reported screenshot were implementation defects.

This review covers horizontal type on a single open or closed Bézier path. It uses Adobe’s documentation and an isolated scripting probe in Illustrator 30.2.1. It does not claim complete Illustrator feature parity or an exhaustive comparison of its graphical interface.

## Evidence and decisions

| Behavior | Evidence | Amalith decision |
| --- | --- | --- |
| Conversion removes the source fill and stroke; clicking chooses the starting position. | Adobe’s [Create type on a path](https://helpx.adobe.com/ca/illustrator/using/creating-type-path.html), updated 18 March 2025. | Replace the path in place with a text object containing the curve. Keep identity, parent, transform, and stacking order. Undo restores the original object and appearance. |
| Start and end delimit the available text range; the center bracket moves the range. | Adobe’s [Illustrator reference](https://helpx.adobe.com/pdf/cs6/illustrator_reference.pdf), “Creating type on a path,” and [Move or flip text on paths](https://helpx.adobe.com/ca/illustrator/desktop/design-with-text/edit-format-text/move-or-flip-text-on-paths.html), updated 11 February 2026. | Share bracket geometry between painting and hit-testing. Keep drag distances continuous across a closed curve’s seam. |
| Moving across the curve flips text. The detailed reference describes Ctrl/Command as preventing accidental flipping during movement. | Adobe’s [Illustrator reference](https://helpx.adobe.com/pdf/cs6/illustrator_reference.pdf). The shorter current move/flip page omits some modifier nuance. | Center dragging moves and flips; Command/Ctrl preserves the original side. A small neutral band prevents flicker near the curve. |
| Align to Path offers Baseline, Ascender, Descender, and Center. Baseline Shift moves text relative to the curve without reversing it. | Adobe’s [Create type on a path](https://helpx.adobe.com/ca/illustrator/using/creating-type-path.html). | Expose alignment and Flip in the selected text’s context menu. Apply baseline shift along the local normal, including in outlines. |
| Illustrator also offers Rainbow, Skew, 3D Ribbon, Stair Step, and Gravity. | Adobe’s [Apply effects to text on paths](https://helpx.adobe.com/illustrator/desktop/design-with-text/edit-format-text/apply-effects-to-text-on-paths.html), updated 27 October 2025. | This change implements tangent-following placement. The other effects remain outside this implementation. |

## Native Illustrator observation

A script created a temporary RGB document with one filled, stroked circle, converted it using `textFrames.pathText`, inspected the result, exported a reference image, and closed only that temporary document without saving. It did not modify existing documents.

Illustrator 30.2.1 returned:

```text
Text type:           PATHTEXT
Separate path items: 0
Text frames:        1
Owned curve points: 4
Curve filled:       false
Curve stroked:      false
Start parameter:    0
End parameter:      3.99000000953674
Geometric bounds:   100,500,400,200
```

The bounds matched the original circle. This directly supports owning the geometry inside the text frame. The segment-relative end parameter stops just short of a full lap; it is not an arc-length value and is not copied numerically into Amalith. This probe verifies scripting behavior, not the exact appearance or hit areas of Illustrator’s UI handles.

## Changes in Amalith

- New conversions own their geometry. Moving and duplicating text therefore move and duplicate the curve. The old linked-path representation remains readable.
- The Type cursor changes near an eligible curve before conversion. Locked layers and multi-subpath paths are excluded from conversion targeting.
- Selected path text no longer draws a straight text baseline. Its selection geometry follows the owned curve.
- Start and end stems are separately reachable when they coincide on a closed loop. Bracket hits precede transform hits; hover and drag show hand cursors.
- Range changes preview during dragging and commit as an undoable text edit. Overset content stays in the text, with a red plus on the selected end bracket.
- Glyph placement, baseline shift, combining-mark offsets, and exported outlines use matching curved coordinates. Direct-selection anchor edits update the owned curve and preview text against it.
- Empty path text retains its curve. Saved text without the new optional geometry field still loads.

## Verification

Automated checks cover conversion and atomic undo/redo, retained geometry after duplication and original deletion, serialization compatibility, endpoint tangents, rotated handle hits, overlapping closed-loop handles, repeated seam crossings, alignment, and flip suppression.

The reproducible visual fixture uses the production Vello glyph painter and outline exporter:

```sh
cargo run -p amalith-shell --example path_type_review -- /tmp/path-type.png
```

Its top row renders live glyphs; the bottom row renders exported outlines. The three cases are normal, flipped, and center-aligned circle text. All six views were inspected. The two rows matched with a mean channel difference of `0.0000 / 255` in the local run. This is a rendering check, not an end-to-end mouse automation test of the application.

## Limits and remaining differences

Adobe’s detailed guide and its newer [alignment and spacing page](https://helpx.adobe.com/illustrator/desktop/design-with-text/edit-format-text/adjust-text-alignment-and-spacing-on-paths.html) describe the direction of the special curve-spacing adjustment differently. The detailed guide distinguishes it from tracking and says it does not affect straight segments. No undocumented curve-spacing algorithm is claimed here.

Vertical type, special path effects, exact Illustrator cursor artwork, path-text threading ports, compound-path conversion, and automatic migration of old linked-path text are not implemented by this change. Existing linked-path files retain their legacy relationship; new conversions use the owned-curve model. The separate opposite-side bracket stems and drag dead zone are Amalith interaction choices, not claims of pixel-identical Illustrator behavior.

Research stopped after conversion, range movement, flip, alignment, and ownership had primary support or direct native observation. The remaining spacing and UI-detail gaps are explicitly bounded above.
