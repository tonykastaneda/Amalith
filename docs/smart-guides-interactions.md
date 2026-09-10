# Smart Guides interaction coverage

This is a workflow checklist, not a claim of 70% Illustrator parity. There is no
established denominator for all Illustrator interactions. Prioritize common
creation and editing gestures, then modifiers, transformations, and complex
artwork. Each interaction needs a hover → press → drag → release check where
applicable, at multiple zoom levels. A unit test or implementation inspection is
not a side-by-side Illustrator verification.

## Reference and acceptance criteria

- [Adobe: Smart Guides behavior](https://helpx.adobe.com/illustrator/desktop/measure-and-align/grids-and-guides/work-with-smart-guides.html): geometry-based alignment during creation, movement, and transformation; objects and artboards; interaction with grid/pixel modes.
- [Adobe: Smart Guides options](https://helpx.adobe.com/illustrator/desktop/measure-and-align/grids-and-guides/smart-guides-options.html): labels, construction angles, measurements, transformations, spacing, and tolerance.
- [Adobe: Pen workflow](https://www.adobe.com/learn/illustrator/web/use-pen-tool): direction handles, corners, and returning to the last anchor to remove the outgoing direction handle.
- [Adobe: path editing](https://www.adobe.com/us/learn/illustrator/web/edit-paths-you-draw): anchors, direction lines, and editing with alignment feedback.
- User acceptance criterion: after placing the first Pen anchor and dragging the
  second, hovering either visible direction handle must display pink `handle`
  text immediately, before the path is committed. This exact wording is a user
  requirement; Adobe's option descriptions do not specify every hover string.

## Implemented interaction baseline

“Unit” means a focused geometry/state regression exists. “Code” means the wiring
has been inspected; native gesture verification remains necessary for every row.

| Workflow | Interaction | Evidence |
| --- | --- | --- |
| Live Pen | Hover incoming handle before committing the path | Unit: both handles, multiple zooms |
| Live Pen | Hover outgoing handle before committing the path | Unit: both handles, multiple zooms |
| Live Pen | Handle label takes precedence over a nearby path/construction cue | Code: hover resolution order |
| Live Pen | Handle hover does not snap the next anchor to a control point | Code: hover and placement are separate |
| Live Pen | Hover an already placed anchor | Unit: live anchor lookup |
| Live Pen | Hover an already drawn Bézier segment | Unit: live path geometry |
| Live Pen | Align next point to earlier anchors on X/Y | Code: live anchors enter alignment candidates |
| Live Pen | Align next point to existing object edges/centers | Code: shared alignment solver |
| Live Pen | Construction guides preview continuously | Code: preview and click share resolution |
| Live Pen | Construction directions work on both sides of the origin | Unit: bidirectional projection |
| Live Pen | Construction attraction respects screen tolerance at different zooms | Unit: three zooms and out-of-range rejection |
| Live Pen | Click last anchor to remove outgoing handle, preserving incoming curve | Unit: curve unchanged after transition |
| Point editing | Hover visible handles of selected, committed anchors | Code: node paths and selected-anchor filtering |
| Point editing | Snap to actual path geometry, including transformed curves | Unit: projection and curve position |
| Point editing | Snap to a path intersection | Unit: crossing; curves use local flattening approximation |
| Point editing | Anchor drag preserves grab offset | Code: snap proposed anchor position, then derive movement |
| Visibility | Hidden paths cannot attract anchors | Unit: object visibility |
| Visibility | Hidden groups/layers cannot expose descendant anchors | Unit: ancestor and layer visibility |
| Object move | Align to distant visible objects | Unit: separation on the perpendicular axis |
| Object move | Show both X and Y matches | Unit: multiple-hit result |
| Object move | Equal spacing to right/left/up/down neighbors | Unit: all four directions |
| Object move | Center between two neighbors | Unit: corrected equal-gap translation |
| Object move | Spacing remains enabled when Alignment Guides is disabled | Unit: independent feature switches |
| Object move | Spacing and perpendicular alignment coexist | Unit: X spacing with Y alignment |
| Object move | Unrelated rows do not become spacing neighbors | Unit: perpendicular overlap filter |
| Shape/line drawing | Snap initial point and dragged endpoint | Code: press and motion wiring |
| Measurement | Show movement deltas | Code: uses stored drag delta |
| Measurement | Show dimensions and line length/angle | Code: uses constrained shape/line geometry |
| Transform | Show scale percentages, rotation angle, and shear angle | Code: reads preview transform |
| Hover appearance | Highlight actual grouped/compound contours | Code: visible leaf paths and transformed geometry |
| Lifecycle | Clear guides on release and leaving the window | Code: event cleanup |
| Preferences | Type arbitrary construction angles, submit, cancel, Tab/Shift+Tab | Unit: field geometry/commit; keyboard wiring inspected |
| Preferences | Reject nonfinite tolerance/angles | Unit: malformed settings |
| Preferences | Persist feature switches, angles, and tolerance | Unit: settings round trip |
| Live Pen | Close a 2-anchor path into a curved loop (Illustrator's "leaf" shape) | Code: close gate lowered from 3 to 2 anchors, shared by the click handler and the hover cue |
| Live Pen | Alt/Option splits a still-live handle without discarding the curve already pulled into the anchor | Code: the incoming handle freezes at its last shape instead of clearing to `None` |
| Live Pen | Space repositions the anchor mid-drag with Smart Guides snapping, not just a raw offset | Code: origin-based recompute each frame, same shape as `MoveAnchors`'s own snap |
| Live Pen | Resume drawing from an existing open path's free endpoint instead of always starting a new object | Unit: core `extend_open_subpath`/`open_endpoint_subpath` (forward, reversed-prepend, and closed-while-resuming cases); Unit: `ExtendOpenPath` command round-trips through undo |
| Point editing | Shift locks a dragged handle to 45°/8 directions from its anchor, live-reactive to the modifier changing mid-drag | Code: shares the Pen handle's re-snap-on-modifier-change pattern |
| Point editing | Alt breaks a dragged handle's mirror, decided at release | Unit: core `break_handle_mirror` leaves both handles untouched, only stops future mirroring; Code: wired through `MoveHandle`'s `break_mirror` flag |
| Point editing | Handle length/angle feedback while dragging | Code: measurement text reads the live handle position against its anchor |
| Point editing | Snap to a sibling anchor (or that path's own center/segments) on the very path being edited, without the dragged anchor attracting itself | Code: exclusion narrowed from the whole object to the exact anchor ordinals being dragged |
| Point editing | Hover cue updates immediately after an anchor-selection change, not only on the next pointer move | Code: forced refresh after every canvas press and release |
| Object move | Locked objects still act as alignment targets | Unit: `visible_top_level_bounds` keeps locked objects, only hit-testing excludes them |
| Object move | Alignment scopes to the isolated group instead of leaking in the whole document | Unit: `bounds_within`, including its clip-mask and blend-step exclusion |
| Object move | Artboard edges and their bleed-expanded edges are alignment targets, regardless of isolation depth | Code: artboards always added to the candidate list |
| Object move | Ruler guides are alignment targets, single-axis, excluded from spacing, and skipped while guides are hidden | Unit: axis isolation and spacing non-contamination |
| Transform | Scale handle drag snaps the dragged corner/edge to alignment targets | Unit: axis-masking so an edge handle can't pick up a cross-axis nudge; Code: wired into the bounding-box Scale drag |
| Transform | Rotate/Scale/Reflect/Shear tools' re-placed pivot point snaps to nearby anchors/centers/guides | Code: pivot placement routed through the same point-snap as everything else |
| Transform | Rotate (both the bounding-box handle and the dedicated tool) snaps its angle to the user's construction-angle list when Shift isn't locking it to 45° | Code: reuses Pen's own construction-angle solver; an exact angle match takes priority over the plain pre-drag reference line |
| Transform | Reflect tool's mirror axis snaps to the construction-angle list the same way | Code: same solver, applied to the axis angle instead of a placement point |
| Transform | Rotated/scaled objects and groups are already correct alignment targets | Unit: `Document::bounds_of` was already world-space (transform-aware) before any Smart Guides code touched it — nothing needed changing |
| Transform | Shear's angle also snaps to the construction-angle list, folded into its own `(-90°, 90°)` half-turn domain | Unit: preset folding and tolerance rejection |
| Transform | Rotate/RotateTool/Reflect also snap to *another nearby object's own rotation angle*, not just the preset list | Code: candidate angles decomposed from each candidate's world transform, excluding the object(s) being transformed themselves |
| Transform | Scale offers two more snap targets per axis: matching a candidate object's own width/height ("matching dimensions"), and matching this same shape's own untouched dimension (square/circle from a rectangle/ellipse) | Unit: fixed-corner/sign math for all four corner directions, and that an edge handle only ever offers its own axis |
| Object move | Moving or resizing an artboard itself snaps the same way an object drag does (edges/centers/guides/other artboards, plus matching-dimension/square cues on resize) | Code: relative-delta resize math recovers the actual candidate edge position before snapping, since (unlike Scale) the press point needn't land exactly on the handle |
| Display | An unboxed hover label (anchor/path/endpoint/center/intersect) clamps to the canvas viewport instead of drawing off-screen into a docked panel | Code: same flip-then-clamp shape `draw_tooltip` already used for the boxed labels |

## Next passes: unresolved or only partially covered

These are explicit remaining work, not interactions implicitly counted as done. Every
gap from earlier revisions of this list — Pen transitions, anchor/handle edits,
alignment targets, and now Transforms/Spacing/Display fidelity/artboard
manipulation — is implemented (rows above). All of it still needs the same native
side-by-side pass as everything else; nothing below has changed the "Code"/"Unit"
caveat at the top of this document, and none of this session's transform/spacing/
display work has been run in the live app at all yet.

1. **Native reference validation:** replay the user’s exact Pen sequence and the
   full matrix above (every revision's new rows included) in Illustrator and
   Amalith. Check cue placement, timing, flicker, and disappearance, not only
   the snapped coordinates.
2. **Transforms, still open:** negative-scale specific edge cases haven't been
   exercised by a test, though nothing in the scale-snap code special-cases sign.
   A defined precedence when a point target, a construction angle, an object's
   own rotation angle, and an alignment edge all compete in the *same* drag is
   still informal (each tool picks its own fallback order; there's no shared rule
   written down).
3. **Spacing and dimensions, still open:** arbitrary multi-object equalization
   (distributing N objects, not just matching one gap) and distance guides
   between two arbitrary points (not tied to a move/scale drag) remain
   unaddressed — current spacing still only compares immediate/established
   neighbor gaps, one pair at a time.
4. **Display fidelity, still open:** ~~independent per-layer guide color~~ — done:
   `Layer` now has a real `color: LayerColor` (Layer Options' named palette, set
   via the new dialog — double-click a layer's color swatch in the Layers
   panel), and Object Highlighting tints the hovered path's outline with its
   own layer's color instead of the fixed Smart Guides pink. Also open:
   exact (not flattened-approximate) curve-on-curve intersection, verifying
   appearance at high DPI (the existing overlay code reuses the same scale-aware
   metrics/stroke widths as every other canvas overlay, so it's likely already
   fine, but unverified), and dense-document performance (every alignment/anchor
   scan walks the visible document fresh each frame — unmeasured, no blind
   optimization applied without profiling data).
5. **Other modes, still open:** text/glyph baseline guides (would need font
   metrics — text objects already participate via their plain bounding box today,
   just not baseline/x-height specifically), and Snap to Last Location (remembering
   and re-offering the last point actually snapped to — deferred: it needs a
   stateful "last successful snap" tracker that doesn't fit the current `&self`-
   only snap-query methods without a larger refactor, and it's the most niche
   item on this whole list).

## View ▸ Show Grid / Snap to Grid / Snap to Pixel / Snap to Point

Built as their own View-menu group (checkmarks + ⌘'/⇧⌘'/⌥⌘' accelerators, a
Windows-parity keyboard fallback, command-palette entries), separate systems
from Smart Guides — matching real Illustrator, where these live in the View
menu independent of the Smart Guides master switch, not as one of its
sub-features:

| Item | Behavior | Evidence |
| --- | --- | --- |
| Show Grid | Ruled grid, `grid_spacing` canonical px apart (default 72), drawn under artboards/objects so a solid fill covers it, same layering as the transparency checker | Code: inspected against the transparency-grid precedent it reuses |
| Snap to Grid | Nearest grid intersection — tried only after Smart Guides' own alignment/point matches come up empty (or when Smart Guides is off entirely), so a real object edge still wins when both are close | Unit: rounding math, non-finite/non-positive spacing is a no-op |
| Snap to Pixel | Nearest whole canonical-px unit, same fallback order as Snap to Grid, whichever of the two is on | Unit: rounding math |
| Snap to Point | With Smart Guides on, the same full anchor+center+path scan it already had; with Smart Guides *off*, a strictly anchor-points-only scan (matching real Illustrator's own narrower Snap to Point) — its own toggle, defaulting on | Code: `sg_point_candidate` gained a `points_only` flag, `sg_point_snap` picks the scan by which toggle is actually driving it |
| Grid Spacing (Preferences ▸ General) | A numeric stepper next to Keyboard Increment — the only value this system has to configure, so it didn't need a page of its own | Code: same stepper pattern as Keyboard Increment/Cull Inset |
| Pen tool final placement | Now also falls back to Snap to Grid/Pixel when nothing else (anchor, alignment, construction angle) matched | Code: one more fallback stage at the end of `sg_pen_snap` |

Deliberately not built: grid/pixel snapping for Rotate/Reflect/Shear's own
*angle* — position-based rounding has no sensible meaning applied to a
rotation/shear angle, so that stays out. Their **pivot placement** (the
click-to-reposition gesture) already goes through the same `sg_point_snap`
used everywhere else, so it picked up grid/pixel/point snapping for free
once `sg_point_snap` itself did. None of this has been run in the live app
yet either.

## Release gate for a workflow

- Verify the actual hit target against the visible cue.
- Verify preview and committed geometry agree, including undo/redo.
- Repeat at 50%, 100%, and 400% zoom and with relevant modifier transitions.
- Toggle master Smart Guides and the relevant subfeature independently.
- Verify hover-only labels never change placement semantics.
- Check hidden/grouped artwork and hovering over canvas chrome.
- Record Illustrator observations separately from product decisions and tests.
