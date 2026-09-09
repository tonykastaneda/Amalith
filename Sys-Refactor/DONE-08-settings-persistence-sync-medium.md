# Settings persistence drift — `load()`/`save()` silently out of sync with `Settings`

**Complexity: Medium** — a cheap test-based safety net is easy; the full serde migration is the medium-effort real fix.

## The problem

`Settings` (`crates/amalith-shell/src/prefs.rs:255`) is a plain struct.
Persistence is hand-written and completely decoupled from it:

- `load()` — `crates/amalith-shell/src/settings.rs:45` — a string-keyed
  `match k { "nudge_step" => ..., "show_tooltips" => ..., ... }` (lines 58-77).
- `save()` — `crates/amalith-shell/src/settings.rs:105` — a hand-built
  `format!` string that lists the same fields again (lines 109-121).

Adding a new scalar field to `Settings` compiles fine with just a `Default`
value. Nothing forces a matching edit to `load()` or `save()`.

`tool_keys`/`action_keys` are the one part of `Settings` that's safe from
this: they're indexed by `Tool::ALL`/`PrefAction::ALL`
(`settings.rs:122-129`), so they always round-trip completely by
construction. Every other field is exposed to this drift.

## Why it matters

This is the most dangerous pattern found in the whole audit, because the
failure mode is **completely silent**. If `save()` is missed for a new
field: no compile error, no runtime panic, no log line — the setting just
quietly reverts to its default every relaunch. Someone would only notice by
accident, when a preference "doesn't stick," and the root cause (a forgotten
line in a `format!` call) is nowhere near where the bug is noticed.

## What needs to happen

Replace the hand-written key-value round trip with something that can't
silently drift from the `Settings` struct:

1. Prefer a real serialization derive (`serde` with a human-readable format
   like TOML or the existing custom format re-derived via a macro) so adding
   a field to the struct is the only edit needed — the (de)serializer
   handles both directions automatically.
2. If the existing hand-rolled `key = value` text format needs to be kept
   for backward compatibility with users' existing `settings.txt` files,
   at minimum add a test that round-trips every field of `Default::default()`
   through `save()` → `load()` and asserts equality — so a missing field
   fails a test instead of failing silently in production. This is the
   lower-effort option if a full serde migration isn't wanted.
3. Either way, the goal is: adding a field to `Settings` and forgetting to
   wire it into persistence must be caught by CI (a failing test) or
   impossible by construction (a derive), not discovered by a user
   report.

## How it needs to happen

- Start with option 2 (a round-trip test) as a cheap, immediate safety net
  regardless of whether the bigger serde migration happens later — it's a
  same-day fix that stops new fields from silently regressing even before
  the structural fix lands.
- If migrating to serde: keep the on-disk format's existing keys intact
  where reasonably possible so existing users' `settings.txt` files aren't
  invalidated on upgrade, or write a one-time migration path.
