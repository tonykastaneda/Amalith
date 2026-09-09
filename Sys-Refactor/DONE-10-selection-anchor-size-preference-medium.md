# Selection & anchor handle size — its own preference, separate from chrome scale

**Complexity: Medium** — a new preference plus wiring into a handful of canvas-space constants; independent of the rest of this folder.

## The problem / design question raised

Selection handles (the 8×8 corner/edge squares drawn at
`canvas.rs:462`/`canvas.rs:662`) and their hit radius
(`handles.rs:85` — 7px grab radius; `handles.rs:110` — 8-32px rotation halo
band) currently have fixed pixel sizes, riding along with the OS DPI factor
via the shared render pipeline but otherwise independent of the document's
own zoom (correct — a 1.25px hairline should stay a crisp screen-space line
regardless of zoom level, that's the deliberate, existing behavior).

The open question was whether these should scale with a future chrome
UI-scale preference (`DONE-15-ui-scale-preference-hard.md`).

## Illustrator precedent (confirmed via research)

Illustrator has a **dedicated, separate preference** for this: Edit ▸
Preferences ▸ Selection & Anchor Display (macOS: Illustrator ▸ Settings ▸
Preferences ▸ Selection & Anchor Display). Under "Anchor Points, Handle, and
Bounding Box Display," a slider adjusts size from Default to Max. This is
completely independent of Illustrator's own general UI-scaling preference —
confirming that "handles get their own scaling control" is not a novel idea,
it's exactly how the reference app does it. Illustrator's version also
bundles in Handle Style, "highlight anchors on mouse over," and "show
handles when multiple anchors are selected" as related but separate toggles.

## What needs to happen

1. Add a new preference — e.g. `Settings.handle_size: HandleSize` (an enum
   `Small`/`Default`/`Large`, or a continuous `f64` multiplier if a slider is
   preferred over presets) — independent of `Settings.ui_scale`
   (`DONE-15-ui-scale-preference-hard.md`).
2. Apply this multiplier specifically to: the handle square size
   (`canvas.rs:462`, `canvas.rs:662`, currently hardcoded `(8.0, 8.0)`), the
   grab radius (`handles.rs:85`, currently `7.0`), and the rotation halo band
   (`handles.rs:110`, currently `8.0..=32.0`) — keeping hit-test radius and
   drawn size scaled together so clicking still matches what's drawn.
3. Do **not** wire this to `Settings.ui_scale` — keep it a fully separate
   preference, matching Illustrator's actual separation of concerns. A user
   who wants bigger anchor points for a stylus/accessibility reason
   shouldn't be forced to also scale every panel and button.

## How it needs to happen

- Independent of `DONE-14-metrics-refactor-hard.md`/`DONE-15-ui-scale-preference-hard.md` — this
  preference reads canvas-space constants in `canvas.rs`/`handles.rs`, not
  the chrome `Metrics` those documents cover. Can be built and shipped on
  its own timeline.
- Follow the existing `Prefs` numeric-control pattern
  (`prefs.rs:319-359`) for whatever UI control is chosen (segmented
  Small/Default/Large buttons most directly mirror Illustrator's actual
  precedent; a slider is closer to Illustrator's literal implementation but
  more UI work here).
