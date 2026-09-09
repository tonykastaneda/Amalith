# Floating dialog/panel window sizing — hardcoded OS window sizes outside the Metrics sweep

**Complexity: Medium** — mechanical once `15` exists, but ~9 call sites need to move together in one pass, plus live-resize handling for windows that can stay open.

## The problem

Multiple call sites create real winit `Window`s with hardcoded
`LogicalSize` values, each defined in its own small, separate constants
module — none of these are part of the `DONE-14-metrics-refactor-hard.md` chrome
catalogue, since they live in per-dialog files rather than the
layout/chrome/dock modules:

- `spawn_master_window` — `app/mod.rs:6958-6980`, sized from caller-supplied
  `(w, h)`.
- Torn-off panel/group tearoff — `FLOAT_W: f64 = 264.0`,
  `FLOAT_H: f64 = 320.0` (`app/mod.rs:96-97`), used at `app/mod.rs:7010-7016`
  (`detach_group_live`) and `7024-7030` (`detach_panel_live`).
- `spawn_picker_window` — `app/mod.rs:7037-7089`;
  `picker::W/H = 680.0/386.0` (`picker.rs:11-12`).
- `spawn_shape_dialog` — `app/shape_dialog.rs:91-120`;
  `shapedialog::W = 268.0` (`shapedialog/mod.rs:50`).
- `offset_dialog.rs:38-54`; `offsetdlg::W = 270.0` (`offsetdlg.rs:15`).
- `blend_dialog.rs:31-47`; `blenddlg::W = 240.0` (`blenddlg.rs:20`).
- `xform_dialog.rs:64-80`; `xformdlg::W = 320.0` (`xformdlg.rs:24`).
- `export.rs:51-67`; `export::W/H = 900.0/600.0` (`export/mod.rs:27,29`).
- `about::WIDTH/HEIGHT = 900.0/633.0` (`about.rs:56-57`).
- `stroke_panel::W/H = 240.0/232.0` (`stroke_panel.rs:19-20`) — feeds a
  similar sizing path even though it's not a separate OS window.

## Why it matters

These aren't drawn `Rect`s that a `Metrics` sweep would naturally catch —
they size **actual OS windows** via `with_inner_size(LogicalSize::new(...))`.
Two consequences specific to this being OS-window sizing rather than
scene-drawing:

1. winit's `LogicalSize` is already multiplied by *that window's own*
   `scale_factor()` at creation time (DPI is handled). A chrome-scale
   preference is a second, independent multiplier that has to be applied on
   top, at window-creation time, at every one of these ~9 call sites — it
   can't ride along with a single `Metrics` sweep the way drawn-rect
   geometry can.
2. If the scale preference changes while a floating window is already open,
   the fixed-at-creation size won't retroactively resize it — that needs an
   explicit `window.request_inner_size(...)` call, a pattern the app already
   uses elsewhere for the main window (`app/mod.rs:1547`, `6898`) but not
   yet for these floating hosts.

Without this fix, shipping `DONE-15-ui-scale-preference-hard.md` alone means: chrome
inside the main window scales, but every popped-out dialog (color picker,
shape dialog, export, about, blend/offset/transform dialogs) stays sized for
100% — visibly cramped and inconsistent the moment a user opens one at
125%+.

## What needs to happen

1. At each of the ~9 call sites above, multiply the target `LogicalSize` by
   the current UI-scale preference (from `DONE-15-ui-scale-preference-hard.md`) before
   passing it to `with_inner_size`.
2. For dialogs that can remain open across a live preference change (check
   which of these are modal-and-short-lived vs. can-stay-open), add a
   `request_inner_size` call triggered by the same "apply_ui_scale" plumbing
   that `15` introduces for the main window.

## How it needs to happen

- Depends on `DONE-15-ui-scale-preference-hard.md` existing first (needs the
  preference value to read).
- Do this as one pass across all ~9 sites together, not incrementally — a
  half-migrated state (some dialogs scale-aware, others not) is more
  confusing to test than doing them all at once, since the symptom
  ("this one dialog looks small") would otherwise need re-diagnosing per
  dialog instead of being a known, single completion checklist.

## Completion — 2026-09-09

Floating panel, picker, shape, offset, blend, transform and export window
creation now consumes scaled runtime dimensions. These dimensions already
include UI scale, so creation does not multiply them a second time. Applying
a new preference resizes existing floating OS windows by the new/old scale
ratio. Their monitor DPI remains an independent OS conversion.

Correction to the original catalogue: About is an in-app card, not a separate
OS window. Its runtime dimensions and wrapped text scale as chrome, as do
Preferences and the Stroke popover.

Validation: `cargo test --workspace --offline` passed (320 tests; one existing
ignored doc test). Focused scale tests also passed after the final Metrics
cleanup. Coverage includes DPI coordinate round trips, actual preference
button hit geometry, settings round trips, normalized workspace geometry,
dialog/panel height scaling, text scaling and document-text independence.
Production Vello renders of Preferences and the picker were inspected at
100%, 125% and 150%, plus a panel gallery at 150%. The repeatable renderer is
`crates/amalith-shell/examples/ui_scale_review.rs`.
