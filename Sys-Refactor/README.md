# Sys-Refactor

Tasks 05, 06, 11, 12, 14 and 15 are implemented and marked with a `DONE-`
prefix so they sort together. Their completion sections record the changes
and validation. The remaining
action items are still planned; 02–04 remain decision records. Each file
maps out one identified issue: what's wrong, why it matters, what needs
to happen, and how. They came out of two sessions of codebase auditing: one
scoping a UI-scale preference, one broadening to "what else in this codebase
has the same disease" (one conceptual thing requiring N manually-synced
edits, with little or no compiler enforcement).

Files are numbered 1 (easiest) to 15 (hardest).

| # | Complexity | File | Issue |
|---|---|---|---|
| 1 | Easy | `01-prefaction-and-tool-all-sync-easy.md` | `Tool::ALL`/`PrefAction::ALL` are hand-maintained arrays with no compiler tie to their enum |
| 2 | Easy | `02-native-menu-bar-scale-limitation-easy.md` | Native OS menu bar text size is outside this app's control — documented limitation, not a fix |
| 3 | Easy | `03-cursor-glyph-scaling-decision-easy.md` | Cursor glyphs should stay fixed by default, confirmed via Illustrator precedent |
| 4 | Easy | `04-ruler-thickness-scaling-decision-easy.md` | Ruler thickness should stay fixed, matching (if not exactly endorsing) Illustrator's actual behavior |
| 5 | Easy | `DONE-05-min-window-size-floor-easy.md` | No minimum window size exists today — needed once chrome can grow |
| 6 | Easy | `DONE-06-tooltip-and-hit-radius-audit-easy.md` | Tooltip literals not caught by a const-only sweep, plus a catalogue of canvas-space hit radii that should stay out of `Metrics` |
| 7 | Medium | `07-theme-color-deduplication-medium.md` | Hardcoded hex color literals duplicated instead of referencing `Theme` — includes one already-live bug |
| 8 | Medium | `08-settings-persistence-sync-medium.md` | `Settings` struct vs. `load()`/`save()` can drift silently — worst failure mode found, zero visible symptom |
| 9 | Medium | `09-native-menu-action-registry-medium.md` | Native menu wiring — a menu item can render, look clickable, and silently do nothing |
| 10 | Medium | `10-selection-anchor-size-preference-medium.md` | Selection handle size should be its own preference, confirmed via Illustrator precedent |
| 11 | Medium | `DONE-11-multi-monitor-dpi-fix-medium.md` | Pre-existing bug: `App::scale` is one global field, not per-window — floating panels already mis-scale on a second display today |
| 12 | Medium | `DONE-12-floating-window-scale-sizing-medium.md` | Dialog/panel OS window sizes are hardcoded across 8+ files, outside the `14` sweep |
| 13 | Hard | `13-panel-registry-refactor-hard.md` | `PanelId` is a bare string, not an enum — 9-10 touch points to add one panel, none compiler-enforced |
| 14 | Hard | `DONE-14-metrics-refactor-hard.md` | ~200 chrome layout constants scattered across 40+ files instead of one `Metrics` struct |
| 15 | Hard | `DONE-15-ui-scale-preference-hard.md` | The actual feature — depends on `14` and `11` |

## A note on order: easy-to-hard ≠ build order

This numbering is a pure difficulty ranking, not a dependency-ordered
implementation plan. In particular, `15` (the UI scale preference itself)
depends on `14` and `11` landing first even though both are numbered
earlier by coincidence of also being lower-numbered — that's not a
guarantee for every doc. If/when this work actually starts, re-derive the
sequence from each doc's own "How it needs to happen" section rather than
assuming ascending file number is buildable order. Broadly:

- `01` can be done anytime, independent of everything else.
- `07`'s live-bug fix (inside the doc) is standalone and cheap.
- `14` (metrics) should happen before `15` (the scale preference) and before
  `12` (floating window sizing, which reads the scale value `15` introduces).
- `11` (multi-monitor DPI) should land before or alongside `15`, not after —
  see that doc for why.
- `08`, `13`, `09` are independent of the scaling work and of each other.
- `10` is independent of everything else in this folder.
- `02`, `03`, `04` are decision records, not action items — no code required
  unless product direction changes.
