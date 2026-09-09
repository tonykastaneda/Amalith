# Multi-monitor DPI bug — pre-existing, unrelated to the scale preference, but blocks it

**Complexity: Medium** — conceptually contained (one field needs to move from `App` to per-window), but touches event handling, rendering, and pointer math together, and is hard to test without multi-DPI hardware.

## The problem

`App::scale: f64` (`crates/amalith-shell/src/app/mod.rs:1174`, default `1.0`
at line `1405`) is a **single global field** shared by every window the app
owns — the main window and every floating panel/dialog.

- It's only ever updated for the **main** window:
  ```rust
  WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
      if Some(id) == self.main_id { self.scale = scale_factor; }
      // other windows: just request_redraw(), the new scale_factor is dropped
  }
  ```
  (`app/mod.rs:7396-7399`)
- `redraw(id: WindowId)` (`app/render/mod.rs:11-40`) uses that same single
  `self.scale` — not `host.window.scale_factor()` — to compute logical
  width/height and the render `Affine::scale(scale)`
  (`render/mod.rs:1017`), **for whichever window is currently being
  redrawn**, main or floating.
- Pointer position math is likewise global:
  `self.pointer = Point::new(position.x / self.scale, ...)`
  (`app/mod.rs:7405`), regardless of which window the `CursorMoved` event
  actually came from.

## Why it matters

If a user drags a floating panel (color picker, shape dialog, a torn-off
panel group) to a second monitor with a different native DPI than the main
display, that panel is **already** rendered and hit-tested using the *main*
window's DPI factor today — this is a real, currently-shipping bug,
independent of any UI-scale preference work.

It directly blocks `DONE-15-ui-scale-preference-hard.md`: stacking a second global
"chrome scale" multiplier on top of this already-conflated single `scale`
field doesn't just risk "conflicting" with it — it makes an already-broken
per-window scale story actively harder to reason about, since there'd be two
factors that are each supposed to be per-window but are both implemented as
one app-global value.

## What needs to happen

1. Track scale factor per `WindowHost`, not as one `App`-level field. Add a
   `scale_factor: f64` (or similar) to whatever struct represents an open
   window (`WindowHost`, per the render-pipeline reference in the research —
   confirm the exact struct name in `app/mod.rs`).
2. Update `WindowEvent::ScaleFactorChanged` to store the new factor on the
   *specific* window that fired the event, not gate the update on
   `Some(id) == self.main_id`.
3. Update `redraw(id: WindowId)` to read that window's own stored factor
   instead of the single `self.scale`.
4. Update pointer-position math (`app/mod.rs:7405` and any other consumer of
   `self.scale`) to use the scale factor of the window the input event
   actually targeted, not a single global.

## How it needs to happen

- This should land **before or alongside** `DONE-15-ui-scale-preference-hard.md`, not
  after — building a chrome-scale preference on top of the current
  single-global-`scale` field means the eventual per-window fix has to
  untangle two conflated concerns instead of one.
- Test explicitly with a floating panel dragged to a second, different-DPI
  display if such hardware/a virtual display is available; short of that, at
  minimum add a unit test asserting that two different `WindowHost`s can
  hold two different scale-factor values simultaneously without one
  overwriting the other.

## Completion — 2026-09-09

Removed App's global DPI field. Each WindowHost owns a WindowDpi value,
initialized from its window and updated on that window's ScaleFactorChanged.
Rendering and cursor conversion use that host's DPI; resize, wheel and
floating-window geometry paths use the corresponding window's factor.
The coordinate test exercises simultaneous 1x/2x hosts and moving only the
floating host to 1.5x. A physical mixed-DPI monitor test remains a manual
hardware check.

Validation: `cargo test --workspace --offline` passed (320 tests; one existing
ignored doc test). Focused scale tests also passed after the final Metrics
cleanup. Coverage includes DPI coordinate round trips, actual preference
button hit geometry, settings round trips, normalized workspace geometry,
dialog/panel height scaling, text scaling and document-text independence.
Production Vello renders of Preferences and the picker were inspected at
100%, 125% and 150%, plus a panel gallery at 150%. The repeatable renderer is
`crates/amalith-shell/examples/ui_scale_review.rs`.
