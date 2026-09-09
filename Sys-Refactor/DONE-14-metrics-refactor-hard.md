# Metrics refactor — centralize scattered chrome layout constants

**Complexity: Hard** — touches 40+ files, ~200 constants, is a prerequisite for other docs in this folder.

## The problem

UI chrome geometry (toolbar cell size, panel row heights, dialog widths, button
sizes, padding) is not defined in one place. It's ~200 separate module-level
`const … : f64` declarations spread across 40+ files, plus an estimated ~4,100
inline float literals used directly in `Rect::new(...)` calls.

Representative examples:
- `crates/amalith-shell/src/app/mod.rs:79-88` — `APP_BAR_H`, `TAB_BAR_H`,
  `OPT_BAR_H`, `CHROME_TOP`
- `crates/amalith-shell/src/panels/tools.rs:21` — `CELL`, `TOP`, `PROXY_H`
- `crates/amalith-shell/src/stroke_panel.rs:19-29` — `W`, `H`, `PAD`, `CTRL_X`,
  `BTN_W`, `BTN_H`, `BTN_GAP`, `FIELD_W`, `STEP_W`
- `crates/amalith-shell/src/layout.rs:14-53` — `MASTER_MIN_W`, `MASTER_MAX_W`,
  `TOOLS_MIN_W`, `COMPACT_BREAKPOINT`, `DOCK_EDGE`, `DOCK_SEAM`,
  `STACK_ROW_H`, `HEADER_H`, `GROUP_HANDLE_H`
- `crates/amalith-shell/src/dock.rs:113-115` — `TAB_CONTENT_MIN_H/MAX_H/DEFAULT_H`

`Theme` (`crates/amalith-shell/src/theme.rs:44-66`) already carries a handful
of runtime metric fields (`tab_strip_h`, `group_title_h`,
`splitter_thickness`, `tab_pad_x`, `panel_menu_w`, `group_close_w`,
`panel_collapse_w`) — proof the pattern is already half-adopted in one place,
just not extended everywhere.

## Why it matters

Because these are plain `const`s, not struct fields, nothing at runtime can
multiply them by a preference (e.g. a UI-scale factor — see
`DONE-15-ui-scale-preference-hard.md`). Any feature that needs chrome geometry to be
adjustable at runtime is blocked on this refactor first.

It's also just diffuse: the same conceptual "how tall is a stack row" value
is redefined per-file instead of referenced from one place, so tuning one
metric consistently across the app means hunting down every file that
independently declared its own copy.

## What needs to happen

1. Introduce a `Metrics` (or extend `Theme`) struct that owns every chrome
   geometry constant currently scattered as module-level `const`s — cell
   sizes, row heights, dialog dimensions, padding, gaps, strip thicknesses.
2. Convert each scattered `const` into a `Metrics` field with the same
   default value, so the initial refactor is behavior-preserving (no visual
   change).
3. Thread `Metrics` (or `self.theme`/a new `self.metrics`) through every
   `paint()`/`hit()`/layout function that currently reads the removed
   `const`s directly — this is the bulk of the mechanical work, touching the
   40+ files that own a piece of chrome layout.
4. Painting and hit-testing must read the *same* `Metrics` values — many
   widgets store `Rect` fields set in `paint()` and read back in `on_press()`
   (see `crates/amalith-shell/src/prefs.rs:319-359`'s `Prefs` struct for the
   existing pattern of paint-computed rects consumed by press handling).
   Keep that coupling intact; don't let paint and hit-test read from two
   different metric sources.

## How it needs to happen

- Do this file-by-file, starting with the files that have the most other
  work depending on them (`layout.rs`, `dock.rs`, `chrome.rs` first, since
  `DONE-15-ui-scale-preference-hard.md` and future dialog work depend on this being
  done first).
- Keep the refactor itself value-preserving — no behavior or pixel-position
  change should ship in the same commit as "move constant into struct."
  Verify with the app running (`run` skill) before and after each file's
  conversion.
- Do **not** fold in the canvas-space values cataloged in
  `04-ruler-thickness-scaling-decision-easy.md` and the grab-radius list in
  `DONE-06-tooltip-and-hit-radius-audit-easy.md` — those are deliberately staying as
  fixed constants, not becoming `Metrics` fields. Keep them separate so a
  blanket "scale everything in Metrics" sweep doesn't accidentally net them.

## Scope note

This is the single largest piece of work in this whole set of documents. It
is also the prerequisite for `DONE-15-ui-scale-preference-hard.md`,
`DONE-12-floating-window-scale-sizing-medium.md`, and touches the same files as
`DONE-06-tooltip-and-hit-radius-audit-easy.md`. Consider it the foundation the other
scaling-related docs build on.

## Completion — 2026-09-09

Introduced metrics.rs with a centralized catalogue of 200+ chrome geometry
measurements, plus named tooltip metrics. Existing module-level geometry
constants became runtime accessors. Composite dimensions are derived from
the shared fields. Inline distances, control widths, row spacing, icon
geometry and font baselines were audited across panels, menus and dialogs.
At 100%, migrated distances retain their original values.

Implementation choice: the single winit UI thread owns an applied Metrics
snapshot, accessed through small module helpers. Paint and hit testing read
the same snapshot; this avoids adding a redundant parameter to every pure
geometry helper. Theme's existing dimensions and TextContext are updated in
the same App::apply_ui_scale operation. This is deliberately separate from
per-window OS DPI and the final scene transform.

Rulers, cursor glyphs, document geometry and the canvas grab-radius catalogue
remain excluded. Workspace snapshots normalize panel dimensions to 100%, so
reloading or switching workspaces cannot compound the preference.

Validation: `cargo test --workspace --offline` passed (320 tests; one existing
ignored doc test). Focused scale tests also passed after the final Metrics
cleanup. Coverage includes DPI coordinate round trips, actual preference
button hit geometry, settings round trips, normalized workspace geometry,
dialog/panel height scaling, text scaling and document-text independence.
Production Vello renders of Preferences and the picker were inspected at
100%, 125% and 150%, plus a panel gallery at 150%. The repeatable renderer is
`crates/amalith-shell/examples/ui_scale_review.rs`.
