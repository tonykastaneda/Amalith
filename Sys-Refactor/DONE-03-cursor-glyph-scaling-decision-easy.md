# Cursor glyph scaling — leave fixed by default, matching Illustrator

**Complexity: Easy** — decision record; the optional opt-in checkbox, if ever built, is a small contained change.

## The problem

`icons.rs` defines cursor SVGs (`CURSOR_SELECT_SVG`,
`CURSOR_DIRECT_SELECT_SVG`, `CURSOR_PEN_DRAWING_SVG`,
`CURSOR_PEN_CLOSING_SVG`, lines 31-38) drawn via `draw_cursor` (line 605),
which scales the embedded SVG's 0..100 viewBox to whatever `box_: Rect` it's
given. That box size is a hardcoded literal at the call site, not derived
from any named constant or preference:

- `app/render/main_view.rs:796` — `let sz = 30.0;` for the tool cursor
  glyph.
- `app/render/main_view.rs:892` — same `let sz = 30.0;` for the `ThreadPort`
  cursor.
- The procedural scale/rotate/fit-up cursors (`draw_scale_cursor`,
  `draw_rotate_cursor`, `draw_fit_up_cursor`, `icons.rs:699-785`) use their
  own hardcoded absolute lengths (`hl = 8.5`, `r = 7.0`, literal arrow
  coordinates) with no size parameter at all.

Since the cursor is painted into the same scene as everything else and
picks up the single top-level DPI `Affine::scale`, it already scales with
OS DPI — but has zero connection to any future chrome-scale value.

## Illustrator precedent (confirmed via research)

Illustrator does **not** scale cursors by default. There's a separate,
explicit opt-in checkbox — "Scale Cursor Proportionately" — under UI
preferences, off by default. Out of the box, Illustrator's cursors stay a
fixed size regardless of UI scale; a user has to deliberately turn on
proportional cursor scaling.

## Decision

Match Illustrator's default: cursor glyphs stay fixed constants, not scaled
by `DONE-15-ui-scale-preference-hard.md`. This is a deliberate design choice, not an
oversight — recorded here so a future "why doesn't the cursor scale" report
isn't mistaken for a bug.

## What needs to happen (optional follow-up, not required)

If wanted later, an equivalent opt-in "Scale Cursor Proportionately"
checkbox could be added to `Settings`, defaulting to `false`, which when
true multiplies the `sz`/`hl`/`r` literals above by the chrome-scale factor.
This is explicitly optional scope, not part of the base scale-preference
work — do not build it unless specifically requested.

## How it needs to happen

N/A for the base decision (leave as-is). If the optional opt-in is ever
built, it's a small, contained change: thread the scale factor into
`main_view.rs`'s two `sz` literals and `icons.rs`'s procedural cursor
functions, gated on the new checkbox.
