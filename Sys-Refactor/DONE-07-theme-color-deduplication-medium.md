# Theme color deduplication — hardcoded hex literals drift from `Theme`

**Complexity: Medium** — the live `LINKED_INK` bug fix is a one-line easy win; auditing and merging the other 6 duplicate-color groups is the medium-effort part.

## The problem

`Theme` (`crates/amalith-shell/src/theme.rs:7`) is meant to be the single
source of truth for app colors, and its `accent` field is user-customizable
via `Settings::accent` / `Theme::set_accent` (`theme.rs:71`). But several
raw hex literals are duplicated verbatim elsewhere instead of reading
`Theme` fields:

- **Live bug, already shipping**: `theme.rs:102` defines
  `accent: Color::from_rgb8(0x3b, 0x9b, 0xff)`, but
  `crates/amalith-shell/src/canvas.rs:39` separately hardcodes
  `const LINKED_INK: Color = Color::from_rgb8(0x3b, 0x9b, 0xff)`, used at
  `canvas.rs:1707` to draw the "linked image" indicator. If a user picks a
  non-default accent color in Preferences, this one indicator silently keeps
  drawing in the *old default* blue instead of following the theme.
- `0x1a1a1a` ("ink") duplicated 5x: `icons.rs:15` and
  `app/render/main_view.rs:813,835,902,916`.
- The "no-paint slash" red (`0xd0,0x30,0x30`) duplicated 4x: `canvas.rs:739`,
  `textedit.rs:652`, `panels/color.rs:24` (as `const SLASH`),
  `panels/mod.rs:857`.
- The "mixed-swatch" background (`0x3c,0x3c,0x3c`) duplicated:
  `panels/tools.rs:168`, `panels/mod.rs:845`.
- `0xe0,0x40,0x40` duplicated: `panels/links.rs:37`,
  `context_bar/artboard.rs:278`.
- `0x88,0x88,0x88` duplicated 3x: `canvas.rs:1716`, `export/mod.rs:812,825`.
- `0xcc,0xcc,0xcc` duplicated: `canvas.rs:1302`, `panels/gradient.rs:180`.

None of these are compile-checked; they're copy-pasted magic numbers that
happen to agree today, with no code tying them to each other or to `Theme`.

## Why it matters

The `LINKED_INK` case is a real, currently-live bug: accent-color
customization silently doesn't propagate to that one indicator. The rest
are style debt with the same failure mode waiting to happen the next time
someone changes one of these colors in one place and doesn't know the other
2-4 copies exist.

## What needs to happen

1. Fix the live bug first: change `canvas.rs`'s `LINKED_INK` (and its call
   site at `canvas.rs:1707`) to read `theme.accent` instead of a hardcoded
   literal.
2. For each of the other duplicated literal groups, decide whether it's
   actually a semantic `Theme` concept that deserves a named field (e.g. an
   "ink"/"on-light-surface" token, a "destructive/no-paint" token, a
   "muted-swatch-bg" token), or a genuine one-off that happens to share a
   value by coincidence — not every duplicate is necessarily a bug, some may
   be deliberately-identical-but-conceptually-separate colors. Audit each
   group rather than mechanically merging all of them.
3. For the ones that are the same concept: add the `Theme` field, replace
   every duplicate literal with a reference to it, and delete the local
   `const`.

## How it needs to happen

- Fix item 1 (the live `LINKED_INK` bug) as a standalone, immediate patch —
  it's a one-line change with a clear, already-diagnosed root cause, no
  need to wait on the rest of this document.
- The remaining dedup work is low-risk but should be done with visual
  verification (the `run` skill) per group, since merging two "coincidentally
  identical" colors that are actually meant to be independently tunable
  would be a regression, not a fix.
