# Minimum window size — doesn't exist today, needed once chrome can grow

**Complexity: Easy** — small, additive-only change, best folded into `15`'s rollout.

## The problem

Searched for `min_inner_size` / `set_min_inner_size` / any winit min-size
builder call across the whole shell crate: **not found, doesn't exist.**
None of the `Window::default_attributes()` call sites (main window at
`app/mod.rs:6959`, `7069`, `7283`; the four dialog files; `export.rs`) call
`.with_min_inner_size(...)`.

## Why it matters

Today this isn't a bug — chrome metrics are fixed, so there's no way a user
resize can clip anything that isn't already handled by the existing
responsive/compact layout logic (`layout.rs`'s `COMPACT_BREAKPOINT` etc.).
But once `DONE-15-ui-scale-preference-hard.md` lands and chrome metrics can grow at
125%/150%, a user on a small display who picks a larger scale could shrink
or already have the window sized below what the now-larger chrome needs,
with nothing stopping clipped or overlapping controls — there's no floor to
catch that case.

## What needs to happen

1. Add `.with_min_inner_size(LogicalSize::new(min_w, min_h))` to the main
   window's creation (and reasonably, the floating dialog windows too, each
   with their own appropriate minimum).
2. Compute `min_w`/`min_h` from the same `Metrics`/scale plumbing introduced
   in `14`/`15`, so the floor grows along with the scale preference instead
   of being a second, independently-hardcoded value that itself could drift
   out of sync with the chrome it's meant to protect.

## How it needs to happen

- Natural to build as part of `DONE-15-ui-scale-preference-hard.md`'s rollout — the
  same commit/PR that makes chrome grow with the preference is the right
  place to also introduce the floor that keeps it from breaking on small
  displays, rather than treating this as a separate follow-up someone might
  forget.
- Low risk, additive-only change (a window that already fits comfortably at
  100% scale won't be affected by a minimum sized for 100% scale; the floor
  only matters once the preference is raised).

## Completion — 2026-09-09

The main window now receives a minimum logical size derived from runtime
Metrics. The floor is applied at creation and updated when UI scale changes;
an already smaller main window is enlarged to meet it. Resizable floating
hosts also get a scaled chrome minimum. Fixed dialogs use their full scaled
content dimensions.

Validation: `cargo test --workspace --offline` passed (320 tests; one existing
ignored doc test). Focused scale tests also passed after the final Metrics
cleanup. Coverage includes DPI coordinate round trips, actual preference
button hit geometry, settings round trips, normalized workspace geometry,
dialog/panel height scaling, text scaling and document-text independence.
Production Vello renders of Preferences and the picker were inspected at
100%, 125% and 150%, plus a panel gallery at 150%. The repeatable renderer is
`crates/amalith-shell/examples/ui_scale_review.rs`.
