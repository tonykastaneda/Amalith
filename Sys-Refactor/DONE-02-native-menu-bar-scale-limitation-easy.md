# Native menu bar text size — permanent platform limitation, document don't chase

**Complexity: Easy** — no code required; this is a decision record.

## The problem

`crates/amalith-shell/src/app/native_menu.rs` builds the native macOS/Windows
menu bar via `muda` (`NSMenu` on macOS via `menu.init_for_nsapp()` at
line 325; `HMENU`/`init_for_hwnd_with_theme` on Windows at line 338). The
entire build function only sets item labels, accelerators, checked/enabled
state, and (Windows-only) a dark theme (`MenuTheme::Dark`, line 338). There
is no font, text-size, or DPI-related API call anywhere in the file, and
`muda`'s public surface doesn't expose one either — menu text is rendered
entirely by the OS's own menu subsystem (AppKit / Win32), using whatever
system UI font size the OS's own accessibility/display settings dictate.

## Why it matters

If `DONE-15-ui-scale-preference-hard.md` ships, the rest of the app's chrome (labels,
icons, buttons) will grow at 125%/150%, but the native menu bar sitting at
the top of the screen has no lever this app can pull — it will stay at
whatever the OS's own text scaling says, permanently. This is not a bug to
fix; it's a hard platform ceiling.

This mirrors a real, reported limitation in Adobe Illustrator itself: users
have hit the same "UI scaling works fine except the ruler/native chrome
stays small" mismatch on Windows machines with high-DPI displays (see the
"Ruler" search results referenced during the design discussion — Illustrator
has the identical class of inconsistency for a different UI element, and
apparently never fully fixed it).

## What needs to happen

Nothing code-side. This document exists so the mismatch is a documented,
known, deliberate non-fix rather than something a future contributor
re-discovers, assumes is a bug, and spends time trying to "fix" by hacking
around `muda`/the OS menu APIs (which, per the investigation above, offer no
such hook).

If it becomes a real user complaint, the only two honest paths forward are:
1. Point users at the OS's own text-scaling setting for menu bars
   (macOS: System Settings ▸ Accessibility ▸ Display; Windows: Settings ▸
   Display ▸ Scale).
2. Replace the native menu bar with an in-app-drawn menu (a much larger,
   separate project, and a real UX tradeoff — losing native menu bar
   integration, Spotlight/Siri search visibility of commands, etc. — not
   something to take on incidentally as part of a scale-preference feature).

## How it needs to happen

N/A — this is a decision record, not an action item.
