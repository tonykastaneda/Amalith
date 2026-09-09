# Native menu action wiring — a click that silently does nothing

**Complexity: Medium** — consolidating 3 hand-maintained lists into one declarative table, plus a coverage test.

## The problem

Wiring one new native (macOS/Windows) menu command touches up to 5 separate
places:

1. Menu item creation — `crates/amalith-shell/src/app/native_menu.rs:61+`
   (one `let` per item, its own label string, e.g. `let new_i = mk("New", ...)`).
2. Insertion into the `Submenu::with_items(...)` tree —
   `native_menu.rs:155-312`.
3. A separate, independently hand-built `Vec<(MenuId, MenuAction)>` mapping
   table — `native_menu.rs:345+`.
4. `run_menu_action()` dispatch — `crates/amalith-shell/src/app/mod.rs:3397`
   — this one *is* an exhaustive match over `MenuAction`, compiler-enforced.
5. A **second**, independently hand-maintained
   `(&str, &str, MenuAction)` array that drives the ⌘K command palette —
   `crates/amalith-shell/src/app/command_palette.rs:40-70`.

Only step 4 is compiler-checked. Steps 1-3 and 5 are hand-authored lists that
have to be kept in sync by hand, with no exhaustiveness check tying them to
the `MenuAction` enum's variant list.

## Why it matters

Forgetting the id-map insertion at step 3 is the worst case: the menu item
still gets created (step 1) and still gets placed in the menu tree (step 2),
so it renders and looks completely normal and clickable — but clicking it
resolves to nothing in the id map, so `run_menu_action` never fires. The
result is a menu item that visibly exists and silently does nothing when
clicked, which is about as bad a failure mode as this whole audit found,
just for a smaller number of items (new native menu commands are added far
less often than tools or panels).

## What needs to happen

1. Fold steps 1-3 into a single declarative table — one array of
   `(label, accelerator, MenuAction)` (or similar) that both builds the
   `Submenu` tree *and* the `MenuId → MenuAction` map from the same data, so
   there's structurally only one list to maintain instead of three that have
   to agree with each other by hand.
2. Feed the command-palette table (step 5) from that same declarative source
   where the two genuinely overlap (native menu items that should also be
   palette-searchable), instead of maintaining a second, separately-typed
   list. Where they don't overlap (palette-only entries with no menu
   presence), keep those explicit, but stop duplicating the ones that are
   supposed to be the same command.
3. Add a test that every `MenuAction` variant referenced by
   `run_menu_action`'s match has at least one corresponding entry in the
   unified menu table, catching the "created a MenuAction the menu never
   actually offers" direction of the drift too.

## How it needs to happen

- Lower urgency than `08`/`13` (settings, panels) since new native menu
  commands are added far less frequently than new tools/panels, and the
  person adding one is very likely to click their own new menu item once
  before shipping, catching a totally-broken wiring immediately. The risk
  this document is really flagging is the *silent* half: a working-but-not-
  in-the-palette or in-the-palette-but-not-working drift that isn't
  caught by "did I click it once."
