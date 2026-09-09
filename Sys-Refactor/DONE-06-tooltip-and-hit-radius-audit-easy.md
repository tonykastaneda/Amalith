# Tooltip/context-menu literals and canvas hit-radius catalogue — easy to miss in a mechanical sweep

**Complexity: Easy** — a handful of `let` literals to convert to named constants, plus a reference catalogue for `14` to check against.

## The problem

Two distinct findings that fall outside a naive "grep for `const` and scale
it" approach to `DONE-14-metrics-refactor-hard.md`:

### A. Tooltip positioning uses inline literals, not named constants

`draw_tooltip` (`app/render/overlays.rs:672-716`) uses local `let` bindings,
not `pub const`s: `let fs = 11.5;`, `let pad = 7.0;`, offset
`anchor.x + 12.0` / `anchor.y + 18.0`, clamp margins `- 4.0`. Because these
are function-local `let`s rather than named module-level constants, a
mechanical "grep for `const`" pass used to build the `14` catalogue would
silently miss them — they need a separate, deliberate audit pass over
function bodies, not just top-of-file declarations.

`const PM_ROW: f64 = 28.0;` (panel-menu row height, `app/mod.rs:2552`) *is*
a named const, more likely to already be caught by `14`'s sweep — flagged
here anyway since it governs the same class of context-menu-adjacent sizing
as the tooltip literals, for completeness.

### B. Canvas-space hit-test/grab radii — correctly out of scope, but need cataloguing so they aren't accidentally swept in

These live in document/screen space and should almost certainly **stay
fixed** with respect to chrome scale (same reasoning as
`04-ruler-thickness-scaling-decision-easy.md`), but are worth enumerating so
nobody doing the `14` refactor accidentally nets them into `Metrics` by
mistake:

- `handles.rs:85` — 7.0px transform-handle grab radius (also relevant to
  `10-selection-anchor-size-preference-medium.md`).
- `handles.rs:110` — 8.0-32.0px rotation-halo band (also relevant to `10`).
- `app/input/press.rs:1096` — `12.0 / self.doc.view.zoom` anchor-pick radius.
- `app/input/press.rs:1103` — `let close_r = 8.0 / self.doc.view.zoom;`
  pen-close-path radius.
- `app/render/mod.rs:357, 444` — `<= 8.0 / self.doc.view.zoom` anchor-hover
  radius.
- `pathtext.rs:484` — `pub const HANDLE_GRAB: f64 = 9.0;`.
- `app/width_tool.rs:16,19` — `WIDTH_HANDLE_GRAB`/`WIDTH_PATH_GRAB = 8.0`.
- `scroll_view.rs:29` — `const GRAB_SLOP: f64 = 6.0;`.
- `app/mod.rs:90,94` — `GRAB_SLOP: f64 = 5.0;`, `DRAG_THRESHOLD: f64 = 5.0;`
  (splitter grab slop / press-to-drag threshold — these two are chrome-space,
  not canvas-space, so worth a second look during `14` for whether they
  *should* scale with chrome after all, unlike the canvas-space ones above).

## Why it matters

Part A is a real, easy-to-miss addition to `DONE-14-metrics-refactor-hard.md`'s
constants sweep — worth fixing in the same pass since it's cheap. Part B
isn't a bug at all, but its absence from a written list is exactly how it
would accidentally get swept into `Metrics` and start scaling with chrome
when it shouldn't (handle grab radius scaling with chrome instead of with
the `10` handle-size preference would make clicking handles feel wrong the
moment a user changes UI scale without touching handle size).

## What needs to happen

1. Convert the tooltip's local `let` literals (part A) into named constants
   alongside `PM_ROW`, then let them participate in `14`'s `Metrics` sweep
   normally.
2. During `DONE-14-metrics-refactor-hard.md`'s implementation, explicitly check this
   document's part-B list and confirm none of those values were
   accidentally pulled into `Metrics`. `GRAB_SLOP`/`DRAG_THRESHOLD`
   specifically deserve a deliberate decision (scale with chrome, since
   they're chrome-space splitter/drag thresholds — likely *should* scale)
   rather than defaulting to either "in" or "out" without thinking about it.

## How it needs to happen

- Fold directly into `DONE-14-metrics-refactor-hard.md`'s work — this document exists
  to make sure that refactor's author has the full list in hand rather than
  discovering these piecemeal.

## Completion — 2026-09-09

Tooltip font size, padding, offsets, margin and flip gap are named Metrics
fields. The tooltip height includes the scaled font height. Splitter grab
slop and the chrome drag threshold scale with chrome. Canvas anchor-pick,
pen-close, path-text, width-tool and transform-handle radii remain fixed;
scroll_view's separate grab slop also remains outside this scale sweep.

Validation: `cargo test --workspace --offline` passed (320 tests; one existing
ignored doc test). Focused scale tests also passed after the final Metrics
cleanup. Coverage includes DPI coordinate round trips, actual preference
button hit geometry, settings round trips, normalized workspace geometry,
dialog/panel height scaling, text scaling and document-text independence.
Production Vello renders of Preferences and the picker were inspected at
100%, 125% and 150%, plus a panel gallery at 150%. The repeatable renderer is
`crates/amalith-shell/examples/ui_scale_review.rs`.
