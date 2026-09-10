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

## Next passes: unresolved or only partially covered

These are explicit remaining work, not interactions implicitly counted as done.

1. **Native reference validation:** replay the user’s exact Pen sequence and the
   matrix above in Illustrator and Amalith. Check cue placement, timing, flicker,
   and disappearance, not only the snapped coordinates.
2. **Pen transitions:** two-anchor curved closure, close-path cursor/label,
   click-drag on the last anchor, Alt/Option splitting of direction handles,
   temporary Direct Selection, resuming open endpoints, and Space repositioning.
   Existing tool behavior needs individual reference checks.
3. **Anchor/handle edits:** modifier changes during a drag, handle-angle feedback,
   snapping to other anchors on the same path without attracting the dragged
   anchor to itself, and hover visibility after selection changes.
4. **Alignment targets:** artboard edges/centers/bleeds, guide intersections,
   isolation scope, clipping masks, locked artwork, and construction references
   from nearby objects. Current object alignment is based on visible top-level
   object bounds; Pen also references its live anchors.
5. **Transforms:** edge/corner snapping while scaling, reflection, constrained
   transformations, pivot movement, transformed groups, negative scale, and
   predictable precedence between point, angle, and alignment targets.
6. **Spacing and dimensions:** matching dimensions, square/circle cues, arbitrary
   multi-object equalization, and distance guides. Current spacing covers local
   neighbor relationships, not every possible distribution.
7. **Display fidelity:** labels near canvas boundaries, independent guide color,
   layer-colored hover contours (layers currently have no color property),
   high-DPI appearance, curve intersections, and dense-document performance.
8. **Other modes:** text/glyph guides, Snap to Grid/Pixel mode arbitration,
   Snap to Last Location, and artboard manipulation.

## Release gate for a workflow

- Verify the actual hit target against the visible cue.
- Verify preview and committed geometry agree, including undo/redo.
- Repeat at 50%, 100%, and 400% zoom and with relevant modifier transitions.
- Toggle master Smart Guides and the relevant subfeature independently.
- Verify hover-only labels never change placement semantics.
- Check hidden/grouped artwork and hovering over canvas chrome.
- Record Illustrator observations separately from product decisions and tests.
