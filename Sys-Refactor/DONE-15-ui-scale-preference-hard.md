# UI scale preference — the actual feature

**Complexity: Hard** — the feature itself is small once its prerequisites (`14`, `11`) land, but it's blocked on both of those, plus follow-up work (`12`, `05`).

## The problem

There is no user-adjustable UI scale today, only the OS-reported HiDPI
factor:

- `App.scale: f64` (`crates/amalith-shell/src/app/mod.rs:1174`) is set once
  from `window.scale_factor()` (`app/mod.rs:7300`) and live-updated on
  `WindowEvent::ScaleFactorChanged` (`app/mod.rs:7396-7398`).
- It feeds exactly one place in rendering: a single top-level
  `Affine::scale(scale)` applied to the whole chrome+canvas scene
  (`app/render/mod.rs:1017`).
- Pointer position is derived from the same factor:
  `self.pointer = Point::new(position.x / self.scale, ...)`
  (`app/mod.rs:7405`).

So there's exactly one existing injection point for a *physical* DPI factor,
wired 1:1 to the OS value — no separate "user preference" multiplier
anywhere, and no distinction today between "OS DPI scale" and "user chrome
scale."

## Why it matters

Users on external displays, users who just prefer larger text/targets, and
accessibility needs all want a chrome-scale option independent of pure OS
DPI. This is the feature the rest of this folder's structural work
(`DONE-14-metrics-refactor-hard.md` especially) exists to unblock.

## What needs to happen

1. **Hard prerequisite**: `DONE-14-metrics-refactor-hard.md` must land first (or at
   minimum far enough along) that chrome geometry is readable from a runtime
   struct, not baked into `const`s. A scale preference has nothing to
   multiply until that's true.
2. **Hard prerequisite**: `DONE-11-multi-monitor-dpi-fix-medium.md` should land first or
   alongside — `self.scale` is currently one global field shared across
   every window, not tracked per-`WindowHost`. Adding a second global
   multiplier on top of an already-conflated single field makes an existing
   bug worse, not just adds a new feature next to it.
3. Add `Settings.ui_scale: f64` (default `1.0`), following the exact pattern
   `Theme::set_accent`/`apply_theme_accent()` already establishes
   (`app/mod.rs:4495-4501`) for "push a Settings field into a live runtime
   object."
4. Apply `ui_scale` at the `Metrics` level (from `14`), not at the final
   render `Affine` — the final-affine approach would scale the document
   canvas along with chrome, which is explicitly not wanted (canvas already
   has its own independent zoom via `CanvasView::to_screen()`,
   `canvas.rs:50-54`).
5. Apply the same multiplier to `TextContext`'s size parameter internally
   (`text.rs` — `measure`/`draw`/`draw_bold`/`draw_column`/`wrap` all take an
   explicit `size: f32`; multiply once inside `TextContext` rather than at
   each of the 259 call sites across 47 files).
6. Explicitly do **not** scale: selection/anchor handle size (see
   `10-selection-anchor-size-preference-medium.md` — separate preference, Illustrator
   precedent confirmed), cursor glyphs (see
   `03-cursor-glyph-scaling-decision-easy.md` — Illustrator default is off),
   ruler thickness (see `04-ruler-thickness-scaling-decision-easy.md` —
   Illustrator doesn't either, confirmed via research), and the canvas-space
   grab radii cataloged in `DONE-06-tooltip-and-hit-radius-audit-easy.md`.
7. Also scale, at the same time: floating dialog/panel OS window sizes (see
   `DONE-12-floating-window-scale-sizing-medium.md`) and add a scaled minimum main-window
   size floor (see `DONE-05-min-window-size-floor-easy.md`).
8. Document, don't attempt to fix: the native OS menu bar's text size is
   permanently outside this app's control (see
   `02-native-menu-bar-scale-limitation-easy.md`).

## How it needs to happen

- Land the two hard prerequisites (`14`, `11`) first.
- Ship the Settings field + General-preferences-page control (a
  100%/125%/150% segmented control or similar, following the existing
  numeric-stepper pattern in `Prefs`, `prefs.rs:319-359`) as its own small,
  visible change once the plumbing exists.
- Then work through `12`, `10`, `05` as the "make the rest of the UI
  actually agree with the new scale" follow-ups — each is independently
  shippable and independently documented in this folder.

## Completion — 2026-09-09

Added persisted Settings.ui_scale (default 1.0), with 100% / 125% / 150%
buttons on Preferences > General. OK applies it to live chrome, text,
floating-window sizes and the minimum main-window size. Cancel leaves the
applied preference alone. Old settings files retain 100%; invalid/nonfinite
scale values are normalized safely.

Canvas zoom, document text, rulers and cursor glyphs remain independent.
Native menu text size is documented in Preferences and the root README as
following system settings. Selection/anchor sizing remains a separate task
(10); it is not coupled to UI scale.

Prerequisites 11 and 14, window sizing tasks 05 and 12, and tooltip/radius
audit 06 are completed together with this feature.

Validation: `cargo test --workspace --offline` passed (320 tests; one existing
ignored doc test). Focused scale tests also passed after the final Metrics
cleanup. Coverage includes DPI coordinate round trips, actual preference
button hit geometry, settings round trips, normalized workspace geometry,
dialog/panel height scaling, text scaling and document-text independence.
Production Vello renders of Preferences and the picker were inspected at
100%, 125% and 150%, plus a panel gallery at 150%. The repeatable renderer is
`crates/amalith-shell/examples/ui_scale_review.rs`.
