# Panel registration refactor — `PanelId` string dispatch has zero compiler enforcement

**Complexity: Hard** — 9-10 touch points, plus a serialization-compat pass for existing saved layouts.

## The problem

`PanelId` (`crates/amalith-shell/src/dock.rs:24`) is
`pub struct PanelId(pub &'static str)` — a bare string wrapper, not an enum.
Every place that needs to do something different per panel is a
`match id.0 { "tools" => ..., "layers" => ..., ... }` over string literals.

Touch points required to add one new panel correctly:

- `crates/amalith-shell/src/panels/mod.rs`: `menu()` (line 412), `paint()`
  (line 450), `hit()` (line 502), `min_body_height()` (line 572),
  `fixed_content_height()` (line 602), `tip()` (line 676) — up to 7 separate
  `match id.0` arms in this one file.
- `crates/amalith-shell/src/panel_icon.rs:19` — `draw()`'s `match panel.0`.
- `crates/amalith-shell/src/app/mod.rs:7803` — `tab_label()`'s `match panel.0`.
- `crates/amalith-shell/src/app/mod.rs:7877` — `WINDOW_PANELS` const array
  (drives both the Windows ▸ Panels menu and the ⌘K command palette listing
  at `command_palette.rs:97`).

That's 9-10 separate edit sites, and every one of them silently no-ops on a
typo or an omitted arm instead of failing to compile:

- Miss an arm in `paint`/`hit`/`menu`/`tip` → falls to `_ => {}` / `_ => None`
  (lines 495, 551, 417, 685) — the panel draws blank or is unclickable, no
  error.
- Miss it in `panel_icon.rs` → falls to the generic "unrecognized panel"
  glyph (line 44) — wrong icon, not a build failure.
- Miss it in `tab_label` → falls to `other => other` (line 7830) — the raw
  id string leaks into the UI as the tab title.
- Miss it in `WINDOW_PANELS` → the panel never appears in the Windows menu
  or command palette, with nothing indicating it's missing.
- Miss `min_body_height`/`fixed_content_height` → falls to a hardcoded
  `60.0` default (line 593) or `None` — wrong sizing, no signal.

## Why it matters

This is worse than the equivalent `Tool` enum pattern (see
`01-prefaction-and-tool-all-sync-easy.md`), because `Tool` is at least a real enum
— several of its touch points get compiler-enforced exhaustiveness for free.
`PanelId` gives up that safety net entirely by being a string. Every one of
the 9-10 touch points above is a place a new panel can be *partially* wired
and still compile and run, just wrong.

## What needs to happen

1. Replace `PanelId(&'static str)` with a real `enum Panel { Tools, Layers,
   Links, ... }` (or keep `PanelId` as a thin wrapper if the string is used
   as a stable serialization key for saved layouts — in that case, add a
   `Panel::id_str()`/`Panel::from_str()` pair and route every internal
   dispatch through the enum, keeping the string only at the
   serialize/deserialize boundary).
2. Convert every `match id.0 { "…" => … }` listed above into a `match panel {
   Panel::X => … }` with no wildcard arm, so the compiler forces every call
   site to handle a newly-added panel.
3. Fold `WINDOW_PANELS` into a `Panel::ALL` const array derived the same way
   `Tool::ALL` is (see `01-prefaction-and-tool-all-sync-easy.md` for why even that
   pattern still needs a lint/test safety net).

## How it needs to happen

- This is a mechanical, file-by-file conversion once the enum exists — the
  risk is in doing it incompletely (leaving some dispatch sites on the old
  string match while others move to the enum). Do it in one pass across all
  9-10 sites, not incrementally, so there's never a period where both
  dispatch styles coexist and can silently diverge from each other.
- Existing saved `layout.json`/`workspaces.json` files on disk reference
  panels by string (see `crates/amalith-shell/src/workspaces.rs`) — any
  serialization boundary must keep reading those strings correctly, so this
  refactor needs a deserialize-compat pass, not just a mechanical rename.
