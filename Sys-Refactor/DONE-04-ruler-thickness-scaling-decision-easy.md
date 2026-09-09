# Ruler thickness — leave fixed, matching Illustrator's (imperfect) actual behavior

**Complexity: Easy** — decision record; the only action is excluding two constants from the `14` sweep.

## The problem

`rulers.rs:24` — `pub const THICK: f64 = 18.0;` (ruler strip thickness,
logical px). `rulers.rs:31` — `const LABEL_SIZE: f32 = 10.0;` (ruler label
glyph size). Both are plain constants, currently outside any scale
mechanism, and the open question was whether `DONE-15-ui-scale-preference-hard.md`
should grow them along with chrome.

## Illustrator precedent (confirmed via research)

Rulers do **not** scale proportionally with Illustrator's UI-scale
preference. This is confirmed via multiple user reports
(Adobe Community threads, an open Illustrator feature request titled "Make
rulers bigger in with larger UI scales") — users specifically complain that
turning UI scale up makes every other panel/button larger while the ruler
stays the same small size, particularly reported on Windows/high-DPI setups.
This is not a deliberate Illustrator design choice — it reads as an
unaddressed inconsistency/bug in Illustrator itself, not a feature.

## Decision

Leave `THICK`/`LABEL_SIZE` as fixed constants, not participating in
`DONE-14-metrics-refactor-hard.md`'s `Metrics` scaling. This matches Illustrator's
actual shipped behavior (whether or not that behavior was originally
intentional on Adobe's part), and avoids introducing scope Illustrator
itself doesn't handle.

This is recorded as a **decision to make**, not purely a "no action" note,
because unlike cursor glyphs (`03`, where Illustrator's non-scaling is an
explicit, documented preference default) ruler non-scaling in Illustrator
looks more like an acknowledged gap than an intentional choice. If user
feedback specifically asks for rulers to scale with chrome, that's a
legitimate product decision to revisit — just don't silently "fix" it during
the `14`/`15` refactor without that conversation happening first, since it
would be diverging from the reference app's actual behavior, not converging
with it.

## What needs to happen

Nothing for the base implementation. When `DONE-14-metrics-refactor-hard.md` is done,
explicitly exclude `rulers.rs`'s `THICK`/`LABEL_SIZE` from the `Metrics`
struct (or include them but don't wire the scale multiplier to them) so a
blanket "everything moved into Metrics scales" assumption doesn't
accidentally grow the ruler anyway.

## How it needs to happen

N/A — decision record. Revisit only if explicitly requested.
